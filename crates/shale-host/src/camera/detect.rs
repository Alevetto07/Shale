//! Local print-failure watch.
//!
//! The picture is scored on the machine, on a coarse grid, with no network
//! call. Three patterns are looked for:
//!
//! * **spaghetti** — many thin, bright edges and not much solid fill
//! * **detachment** — a part that was in the previous frame and is gone
//! * **blob** — one compact lump that is not the texture of the plate
//!
//! A finding has to clear the confidence threshold, and the same finding has
//! to repeat for several frames ([`Debounce`]), before Shale notifies. Pausing
//! the print is a separate opt-in and is off unless the caller says so.
//!
//! Obico's failure-detection weights are AGPL-3.0, the same licence as
//! `obico-server`, and the ONNX file is about 193 MB
//! ([`OBICO_ONNX_URL`], [`OBICO_ONNX_SHA256`]). Shale does not ship those
//! weights. The grid detector here is the default until a download of that
//! file checks out. ONNX Runtime is an optional backend and is not used by
//! these tests.

use image::codecs::jpeg::JpegEncoder;
use image::{ExtendedColorType, ImageEncoder, Rgb, RgbImage};

/// Obico's published ONNX weights, release `ml-models-2026-11-23`.
/// The GitHub release says the file is AGPL-3.0, like the rest of obico-server.
pub const OBICO_ONNX_URL: &str = "https://github.com/TheSpaghettiDetective/obico-server/releases/download/ml-models-2026-11-23/model-weights-5a6b1be1fa.onnx";
pub const OBICO_ONNX_SHA256: &str =
    "0a6ebd8e30dbf6a450c50f9c0a5406f04ba7eb1c99fd5996e888c78bb383b9aa";
pub const OBICO_ONNX_BYTES: u64 = 202_223_918;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FailureKind {
    None,
    Spaghetti,
    Detachment,
    Blob,
}

impl FailureKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Spaghetti => "spaghetti",
            Self::Detachment => "detachment",
            Self::Blob => "blob",
        }
    }

    pub fn parse(text: &str) -> Option<Self> {
        match text.trim().to_ascii_lowercase().as_str() {
            "none" => Some(Self::None),
            "spaghetti" => Some(Self::Spaghetti),
            "detachment" => Some(Self::Detachment),
            "blob" => Some(Self::Blob),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Sensitivity {
    Low,
    Medium,
    High,
}

impl Sensitivity {
    pub fn parse(text: &str) -> Self {
        match text.trim().to_ascii_lowercase().as_str() {
            "low" => Self::Low,
            "high" => Self::High,
            _ => Self::Medium,
        }
    }

    fn stringy_cells(self) -> usize {
        match self {
            Self::Low => 8,
            Self::Medium => 5,
            Self::High => 3,
        }
    }
}

#[derive(Clone, Debug)]
pub struct WatchSettings {
    /// How often the panel asks for a fresh judgement, in seconds.
    pub sample_seconds: f64,
    /// A finding quieter than this does not count.
    pub confidence: f64,
    pub sensitivity: Sensitivity,
    /// How many frames in a row, over the threshold, before a notification.
    pub debounce_frames: u32,
    /// Pause the print when a notification fires. Off unless a person opted in
    /// for this printer.
    pub pause: bool,
}

impl Default for WatchSettings {
    fn default() -> Self {
        Self {
            sample_seconds: 5.0,
            confidence: 0.6,
            sensitivity: Sensitivity::Medium,
            debounce_frames: 3,
            pause: false,
        }
    }
}

/// A box in fractions of the frame, origin at the top left.
#[derive(Clone, Debug, PartialEq)]
pub struct BoxMark {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
    pub label: String,
    pub confidence: f64,
}

#[derive(Clone, Debug)]
pub struct FrameFinding {
    pub kind: FailureKind,
    pub confidence: f64,
    pub boxes: Vec<BoxMark>,
    /// Row-major, `heatmap_cols` by `heatmap_rows`, each value 0 to 1.
    pub heatmap: Vec<f64>,
    pub heatmap_cols: usize,
    pub heatmap_rows: usize,
    pub note: String,
}

/// What to do once a finding has survived the debounce. Pause is absent
/// unless `pause` was set by the operator for this printer.
pub fn watch_action(alert: bool, pause_opt_in: bool) -> &'static str {
    if !alert {
        "none"
    } else if pause_opt_in {
        "pause"
    } else {
        "notify"
    }
}

/// Same kind, over the threshold, `need` times in a row.
#[derive(Clone, Debug)]
pub struct Debounce {
    kind: FailureKind,
    run: u32,
    need: u32,
}

impl Default for Debounce {
    fn default() -> Self {
        Self::new(3)
    }
}

impl Debounce {
    pub fn new(need: u32) -> Self {
        Self {
            kind: FailureKind::None,
            run: 0,
            need: need.max(1),
        }
    }

    pub fn push(&mut self, finding: &FrameFinding, min_confidence: f64) -> bool {
        let counts =
            finding.kind != FailureKind::None && finding.confidence + 1e-9 >= min_confidence;
        if !counts {
            self.kind = FailureKind::None;
            self.run = 0;
            return false;
        }
        if finding.kind != self.kind {
            self.kind = finding.kind;
            self.run = 1;
        } else {
            self.run = self.run.saturating_add(1);
        }
        self.run >= self.need
    }

    pub fn reset(&mut self) {
        self.kind = FailureKind::None;
        self.run = 0;
    }

    pub fn set_need(&mut self, need: u32) {
        let need = need.max(1);
        if need != self.need {
            self.need = need;
            self.reset();
        }
    }
}

const COLS: usize = 12;
const ROWS: usize = 9;

pub fn inspect(
    frame: &RgbImage,
    previous: Option<&RgbImage>,
    settings: &WatchSettings,
) -> FrameFinding {
    let cells = cells_of(frame);
    let previous_cells = previous.map(cells_of);
    let stringy: Vec<usize> = cells
        .iter()
        .enumerate()
        .filter(|(_, cell)| cell.stringy)
        .map(|(index, _)| index)
        .collect();
    let blobs = blob_cells(&cells);

    let detachment = previous_cells.as_ref().and_then(|before| {
        let lost: Vec<usize> = before
            .iter()
            .enumerate()
            .filter(|(index, cell)| cell.fill > 0.45 && cells[*index].fill < 0.12)
            .map(|(index, _)| index)
            .collect();
        let had = before.iter().filter(|cell| cell.fill > 0.45).count();
        if had >= 2 && lost.len() >= 2 && lost.len() * 2 >= had {
            Some(lost)
        } else {
            None
        }
    });

    let heatmap: Vec<f64> = cells.iter().map(|cell| cell.edge.clamp(0.0, 1.0)).collect();

    if let Some(lost) = detachment {
        let confidence = (0.62 + lost.len() as f64 / 24.0).min(0.97);
        return finding(
            FailureKind::Detachment,
            confidence,
            &lost,
            heatmap,
            "A part that was in the previous frame is missing.",
        );
    }

    if stringy.len() >= settings.sensitivity.stringy_cells() {
        let confidence = (0.58 + stringy.len() as f64 / 18.0).min(0.97);
        return finding(
            FailureKind::Spaghetti,
            confidence,
            &stringy,
            heatmap,
            "Thin strands cover more of the frame than a normal layer.",
        );
    }

    if let Some(blob) = blobs {
        let confidence = (0.6 + blob.len() as f64 / 16.0).min(0.95);
        return finding(
            FailureKind::Blob,
            confidence,
            &blob,
            heatmap,
            "A compact lump is sitting on the plate.",
        );
    }

    FrameFinding {
        kind: FailureKind::None,
        confidence: 0.0,
        boxes: Vec::new(),
        heatmap,
        heatmap_cols: COLS,
        heatmap_rows: ROWS,
        note: "Nothing in this frame looks like a failure.".into(),
    }
}

fn finding(
    kind: FailureKind,
    confidence: f64,
    indexes: &[usize],
    heatmap: Vec<f64>,
    note: &str,
) -> FrameFinding {
    FrameFinding {
        kind,
        confidence,
        boxes: vec![box_for(indexes, kind, confidence)],
        heatmap,
        heatmap_cols: COLS,
        heatmap_rows: ROWS,
        note: note.into(),
    }
}

fn box_for(indexes: &[usize], kind: FailureKind, confidence: f64) -> BoxMark {
    let mut min_c = COLS;
    let mut min_r = ROWS;
    let mut max_c = 0;
    let mut max_r = 0;
    for index in indexes {
        let c = index % COLS;
        let r = index / COLS;
        min_c = min_c.min(c);
        min_r = min_r.min(r);
        max_c = max_c.max(c);
        max_r = max_r.max(r);
    }
    BoxMark {
        x: min_c as f64 / COLS as f64,
        y: min_r as f64 / ROWS as f64,
        w: (max_c - min_c + 1) as f64 / COLS as f64,
        h: (max_r - min_r + 1) as f64 / ROWS as f64,
        label: kind.as_str().into(),
        confidence,
    }
}

struct Cell {
    fill: f64,
    edge: f64,
    stringy: bool,
}

fn cells_of(image: &RgbImage) -> Vec<Cell> {
    let (width, height) = image.dimensions();
    let plate = plate_luma(image);
    let mut cells = Vec::with_capacity(COLS * ROWS);
    for row in 0..ROWS {
        for col in 0..COLS {
            let x0 = col * width as usize / COLS;
            let x1 = (col + 1) * width as usize / COLS;
            let y0 = row * height as usize / ROWS;
            let y1 = (row + 1) * height as usize / ROWS;
            let mut fill = 0.0;
            let mut edge = 0.0;
            let mut n = 0.0;
            for y in y0..y1 {
                for x in x0..x1 {
                    let pixel = image.get_pixel(x as u32, y as u32);
                    let here = luma(pixel[0], pixel[1], pixel[2]);
                    if (here - plate).abs() > 28.0 {
                        fill += 1.0;
                    }
                    let right = if x + 1 < x1 {
                        let p = image.get_pixel((x + 1) as u32, y as u32);
                        luma(p[0], p[1], p[2])
                    } else {
                        here
                    };
                    let below = if y + 1 < y1 {
                        let p = image.get_pixel(x as u32, (y + 1) as u32);
                        luma(p[0], p[1], p[2])
                    } else {
                        here
                    };
                    edge += ((here - right).abs() + (here - below).abs()) * 0.5;
                    n += 1.0;
                }
            }
            if n == 0.0 {
                cells.push(Cell {
                    fill: 0.0,
                    edge: 0.0,
                    stringy: false,
                });
                continue;
            }
            let fill = fill / n;
            let edge = (edge / n) / 40.0;
            let stringy = edge > 0.45 && fill > 0.04 && fill < 0.55;
            cells.push(Cell {
                fill,
                edge,
                stringy,
            });
        }
    }
    cells
}

fn plate_luma(image: &RgbImage) -> f64 {
    // The corners are usually plate, even when the middle is a failure.
    let (width, height) = image.dimensions();
    let spots = [
        (1, 1),
        (width.saturating_sub(2), 1),
        (1, height.saturating_sub(2)),
        (width.saturating_sub(2), height.saturating_sub(2)),
    ];
    let mut sum = 0.0;
    for (x, y) in spots {
        let pixel = image.get_pixel(x, y);
        sum += luma(pixel[0], pixel[1], pixel[2]);
    }
    sum / spots.len() as f64
}

fn luma(r: u8, g: u8, b: u8) -> f64 {
    0.299 * r as f64 + 0.587 * g as f64 + 0.114 * b as f64
}

fn blob_cells(cells: &[Cell]) -> Option<Vec<usize>> {
    let mut best: Option<Vec<usize>> = None;
    let mut seen = vec![false; cells.len()];
    for start in 0..cells.len() {
        if seen[start] || cells[start].fill < 0.62 {
            continue;
        }
        let mut stack = vec![start];
        let mut group = Vec::new();
        seen[start] = true;
        while let Some(index) = stack.pop() {
            group.push(index);
            let col = index % COLS;
            let row = index / COLS;
            let neighbours = [
                col.checked_sub(1).map(|c| row * COLS + c),
                (col + 1 < COLS).then_some(row * COLS + col + 1),
                row.checked_sub(1).map(|r| r * COLS + col),
                (row + 1 < ROWS).then_some((row + 1) * COLS + col),
            ];
            for next in neighbours.into_iter().flatten() {
                if !seen[next] && cells[next].fill >= 0.62 {
                    seen[next] = true;
                    stack.push(next);
                }
            }
        }
        if !(1..=8).contains(&group.len()) {
            continue;
        }
        let mut min_c = COLS;
        let mut max_c = 0;
        let mut min_r = ROWS;
        let mut max_r = 0;
        for index in &group {
            min_c = min_c.min(index % COLS);
            max_c = max_c.max(index % COLS);
            min_r = min_r.min(index / COLS);
            max_r = max_r.max(index / COLS);
        }
        let w = (max_c - min_c + 1) as f64;
        let h = (max_r - min_r + 1) as f64;
        if w.max(h) / w.min(h) > 2.6 {
            continue;
        }
        if best
            .as_ref()
            .map(|have| group.len() > have.len())
            .unwrap_or(true)
        {
            best = Some(group);
        }
    }
    best
}

#[derive(Clone, Debug)]
pub struct EvalCase {
    pub name: String,
    pub frame: RgbImage,
    pub previous: Option<RgbImage>,
    pub expect: FailureKind,
}

#[derive(Clone, Debug)]
pub struct EvalRow {
    pub name: String,
    pub expect: String,
    pub got: String,
    pub confidence: f64,
    pub pass: bool,
}

#[derive(Clone, Debug)]
pub struct EvalReport {
    pub correct: usize,
    pub total: usize,
    pub rows: Vec<EvalRow>,
}

pub fn evaluate(cases: &[EvalCase], settings: &WatchSettings) -> EvalReport {
    let mut rows = Vec::new();
    for case in cases {
        let finding = inspect(&case.frame, case.previous.as_ref(), settings);
        let pass = finding.kind == case.expect
            && (case.expect == FailureKind::None || finding.confidence >= settings.confidence);
        rows.push(EvalRow {
            name: case.name.clone(),
            expect: case.expect.as_str().into(),
            got: finding.kind.as_str().into(),
            confidence: finding.confidence,
            pass,
        });
    }
    let correct = rows.iter().filter(|row| row.pass).count();
    EvalReport {
        correct,
        total: rows.len(),
        rows,
    }
}

/// Small pictures the evaluation harness and the tests share. They are
/// synthetic: a flat plate, strands, a lump, and a part that then vanishes.
pub fn sample_frame(kind: FailureKind) -> RgbImage {
    let mut image = RgbImage::from_pixel(96, 72, Rgb([42, 44, 48]));
    match kind {
        FailureKind::None => {}
        FailureKind::Spaghetti => {
            for n in 0..18 {
                let y = 16 + (n % 9) * 5;
                for x in 18..78 {
                    if (x + n * 3) % 4 != 0 {
                        continue;
                    }
                    image.put_pixel(x, y, Rgb([230, 230, 220]));
                    if y + 1 < 72 {
                        image.put_pixel(x, y + 1, Rgb([30, 30, 34]));
                    }
                }
            }
        }
        FailureKind::Blob => {
            for y in 28..52 {
                for x in 36..64 {
                    let dx = x as i32 - 50;
                    let dy = y as i32 - 40;
                    if dx * dx + dy * dy < 160 {
                        image.put_pixel(x, y, Rgb([230, 90, 40]));
                    }
                }
            }
        }
        FailureKind::Detachment => {
            for y in 24..50 {
                for x in 30..62 {
                    image.put_pixel(x, y, Rgb([200, 170, 60]));
                }
            }
        }
    }
    image
}

pub struct SensitivityPreview {
    pub jpeg: Vec<u8>,
    pub finding: FrameFinding,
}

/// A generated frame, judged at this sensitivity. The pixels are drawn here,
/// not taken from a photograph, so the control can be tried with no printer.
pub fn preview_sensitivity(sensitivity: Sensitivity) -> Result<SensitivityPreview, String> {
    let frame = sample_frame(FailureKind::Spaghetti);
    let previous = sample_frame(FailureKind::None);
    let settings = WatchSettings {
        sensitivity,
        ..WatchSettings::default()
    };
    let finding = inspect(&frame, Some(&previous), &settings);
    let mut jpeg = Vec::new();
    JpegEncoder::new_with_quality(&mut jpeg, 80)
        .write_image(
            frame.as_raw(),
            frame.width(),
            frame.height(),
            ExtendedColorType::Rgb8,
        )
        .map_err(|err| err.to_string())?;
    Ok(SensitivityPreview { jpeg, finding })
}

/// The sample set the harness scores. Licences: these pixels are generated
/// by Shale, not taken from a photograph.
pub fn sample_cases() -> Vec<EvalCase> {
    vec![
        EvalCase {
            name: "empty plate".into(),
            frame: sample_frame(FailureKind::None),
            previous: None,
            expect: FailureKind::None,
        },
        EvalCase {
            name: "spaghetti".into(),
            frame: sample_frame(FailureKind::Spaghetti),
            previous: Some(sample_frame(FailureKind::None)),
            expect: FailureKind::Spaghetti,
        },
        EvalCase {
            name: "blob".into(),
            frame: sample_frame(FailureKind::Blob),
            previous: Some(sample_frame(FailureKind::None)),
            expect: FailureKind::Blob,
        },
        EvalCase {
            name: "detachment".into(),
            frame: sample_frame(FailureKind::None),
            previous: Some(sample_frame(FailureKind::Detachment)),
            expect: FailureKind::Detachment,
        },
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_sample_set_is_labelled_and_a_single_frame_does_not_alert() {
        let settings = WatchSettings::default();
        let report = evaluate(&sample_cases(), &settings);
        assert_eq!(report.correct, report.total, "{:?}", report.rows);
        let mut debounce = Debounce::new(settings.debounce_frames);
        let finding = inspect(&sample_frame(FailureKind::Spaghetti), None, &settings);
        assert!(!debounce.push(&finding, settings.confidence));
        assert!(!debounce.push(&finding, settings.confidence));
        assert!(debounce.push(&finding, settings.confidence));
    }

    #[test]
    fn a_test_frame_shows_what_this_sensitivity_sees() {
        let high = preview_sensitivity(Sensitivity::High).expect("preview");
        let low = preview_sensitivity(Sensitivity::Low).expect("preview");
        assert!(high.jpeg.starts_with(&[0xff, 0xd8]));
        assert!(low.jpeg.starts_with(&[0xff, 0xd8]));
        assert_eq!(high.finding.kind, FailureKind::Spaghetti);
        assert_eq!(high.jpeg, low.jpeg);
    }

    #[test]
    fn pause_stays_off_unless_the_printer_was_opted_in() {
        assert_eq!(watch_action(false, true), "none");
        assert_eq!(watch_action(true, false), "notify");
        assert_eq!(watch_action(true, true), "pause");
    }

    #[test]
    fn a_quiet_finding_never_becomes_an_alert() {
        let mut debounce = Debounce::new(1);
        let finding = FrameFinding {
            kind: FailureKind::Blob,
            confidence: 0.4,
            boxes: Vec::new(),
            heatmap: Vec::new(),
            heatmap_cols: 1,
            heatmap_rows: 1,
            note: String::new(),
        };
        assert!(!debounce.push(&finding, 0.6));
    }

    #[test]
    fn the_obico_weights_are_named_and_not_a_small_file() {
        assert!(OBICO_ONNX_URL.contains("obico-server"));
        assert_eq!(OBICO_ONNX_SHA256.len(), 64);
        const _: () = assert!(OBICO_ONNX_BYTES > 50_000_000);
    }
}
