//! The Obico failure model, when the file is present.
//!
//! The grid detector stays the default. This runs only after the weights have
//! been downloaded and the checksum matches, and only when the caller asks for
//! it. A missing file, a bad file, or a model that will not load falls back to
//! the grid. The class list in Obico's `model.names` is a single word,
//! `failure`. Boxes come out in the same shape the overlay already draws.

use std::path::Path;

use image::RgbImage;

use super::detect::{FrameFinding, WatchSettings};
#[cfg(any(test, feature = "onnx"))]
use super::detect::{BoxMark, FailureKind};
use super::weights;

/// One detection before it becomes a box on the frame.
#[cfg(any(test, feature = "onnx"))]
#[derive(Clone, Debug, PartialEq)]
pub struct RawBox {
    pub class_id: usize,
    pub confidence: f32,
    pub x1: f32,
    pub y1: f32,
    pub x2: f32,
    pub y2: f32,
}

/// Obico's ONNX post-process: boxes are `[batch, count, plane, 4]` or
/// `[batch, count, 4]`, confidences are `[batch, count, classes]`, and a
/// box is kept when its best class clears `threshold`. Overlaps are reduced
/// with the same IoU test Obico uses.
#[cfg(any(test, feature = "onnx"))]
pub fn decode_outputs(
    box_shape: &[usize],
    boxes: &[f32],
    conf_shape: &[usize],
    confs: &[f32],
    threshold: f32,
) -> Result<Vec<RawBox>, String> {
    let count = *box_shape.get(1).ok_or("the box output has no count")?;
    let classes = *conf_shape.get(2).ok_or("the confidence output has no classes")?;
    if conf_shape.get(1).copied() != Some(count) {
        return Err("the box and confidence outputs do not agree".into());
    }
    let mut found = Vec::new();
    for index in 0..count {
        let coords = box_at(box_shape, boxes, index)?;
        let (class_id, confidence) = best_class(confs, classes, index);
        if confidence < threshold {
            continue;
        }
        found.push(RawBox {
            class_id,
            confidence,
            x1: coords[0],
            y1: coords[1],
            x2: coords[2],
            y2: coords[3],
        });
    }
    Ok(suppress(found, 0.45))
}

#[cfg(any(test, feature = "onnx"))]
fn box_at(shape: &[usize], data: &[f32], index: usize) -> Result<[f32; 4], String> {
    let offset = match shape {
        [_, _, 4] => index * 4,
        [_, _, plane, 4] => index * plane * 4,
        _ => return Err("the box output is not a list of four coordinates".into()),
    };
    if offset + 4 > data.len() {
        return Err("the box output is shorter than its shape".into());
    }
    Ok([
        data[offset],
        data[offset + 1],
        data[offset + 2],
        data[offset + 3],
    ])
}

#[cfg(any(test, feature = "onnx"))]
fn best_class(confs: &[f32], classes: usize, index: usize) -> (usize, f32) {
    let mut best = 0;
    let mut score = 0.0;
    for class_id in 0..classes {
        let value = confs.get(index * classes + class_id).copied().unwrap_or(0.0);
        if class_id == 0 || value > score {
            best = class_id;
            score = value;
        }
    }
    (best, score)
}

#[cfg(any(test, feature = "onnx"))]
fn suppress(mut boxes: Vec<RawBox>, nms: f32) -> Vec<RawBox> {
    boxes.sort_by(|a, b| b.confidence.total_cmp(&a.confidence));
    let mut kept = Vec::new();
    for candidate in boxes {
        let overlaps = kept.iter().any(|earlier: &RawBox| {
            earlier.class_id == candidate.class_id && iou(earlier, &candidate) > nms
        });
        if !overlaps {
            kept.push(candidate);
        }
    }
    kept
}

#[cfg(any(test, feature = "onnx"))]
fn iou(a: &RawBox, b: &RawBox) -> f32 {
    let x1 = a.x1.max(b.x1);
    let y1 = a.y1.max(b.y1);
    let x2 = a.x2.min(b.x2);
    let y2 = a.y2.min(b.y2);
    let w = (x2 - x1).max(0.0);
    let h = (y2 - y1).max(0.0);
    let inter = w * h;
    let area_a = (a.x2 - a.x1).max(0.0) * (a.y2 - a.y1).max(0.0);
    let area_b = (b.x2 - b.x1).max(0.0) * (b.y2 - b.y1).max(0.0);
    let union = area_a + area_b - inter;
    if union <= 0.0 {
        0.0
    } else {
        inter / union
    }
}

#[cfg(any(test, feature = "onnx"))]
pub fn class_name(_class_id: usize) -> &'static str {
    // Obico's names file has one class, "failure".
    "failure"
}

#[cfg(any(test, feature = "onnx"))]
pub fn finding_from(boxes: &[RawBox]) -> FrameFinding {
    let marks: Vec<BoxMark> = boxes
        .iter()
        .map(|mark| {
            let x = mark.x1.min(mark.x2) as f64;
            let y = mark.y1.min(mark.y2) as f64;
            BoxMark {
                x,
                y,
                w: (mark.x2 - mark.x1).abs() as f64,
                h: (mark.y2 - mark.y1).abs() as f64,
                label: class_name(mark.class_id).into(),
                confidence: mark.confidence as f64,
            }
        })
        .collect();
    let confidence = marks.iter().map(|mark| mark.confidence).fold(0.0, f64::max);
    let kind = if marks.is_empty() {
        FailureKind::None
    } else {
        FailureKind::Spaghetti
    };
    let note = if marks.is_empty() {
        "The failure model saw nothing wrong.".to_string()
    } else {
        "The failure model marked this frame.".to_string()
    };
    FrameFinding {
        kind,
        confidence,
        boxes: marks.clone(),
        heatmap: heatmap_of(&marks),
        heatmap_cols: 12,
        heatmap_rows: 9,
        note,
    }
}

#[cfg(any(test, feature = "onnx"))]
fn heatmap_of(boxes: &[BoxMark]) -> Vec<f64> {
    let mut heat = vec![0.0; 12 * 9];
    for mark in boxes {
        for row in 0..9 {
            for col in 0..12 {
                let cx = (col as f64 + 0.5) / 12.0;
                let cy = (row as f64 + 0.5) / 9.0;
                if cx >= mark.x && cx <= mark.x + mark.w && cy >= mark.y && cy <= mark.y + mark.h {
                    let cell: f64 = heat[row * 12 + col];
                    heat[row * 12 + col] = cell.max(mark.confidence);
                }
            }
        }
    }
    heat
}

/// Score one frame. `Ok` means the model ran. `Err` means the caller should
/// use the grid detector and say why.
pub fn score(path: &Path, frame: &RgbImage, settings: &WatchSettings) -> Result<FrameFinding, String> {
    if !weights::is_verified(path) {
        return Err("The failure model has not been downloaded.".into());
    }
    run_session(path, frame, settings.confidence as f32)
}

#[cfg(not(feature = "onnx"))]
fn run_session(_path: &Path, _frame: &RgbImage, _threshold: f32) -> Result<FrameFinding, String> {
    Err("This build has no ONNX runtime.".into())
}

#[cfg(feature = "onnx")]
fn run_session(path: &Path, frame: &RgbImage, threshold: f32) -> Result<FrameFinding, String> {
    use std::sync::Mutex;

    use image::imageops::FilterType;
    use ort::session::Session;
    use ort::value::Tensor;

    struct Loaded {
        path: std::path::PathBuf,
        // The session borrows the environment in some ort builds. Keeping it
        // in the same struct holds the runtime up for the life of the model.
        #[allow(dead_code)]
        env: ort::environment::Environment,
        session: Session,
        width: u32,
        height: u32,
    }

    static LOADED: Mutex<Option<Loaded>> = Mutex::new(None);

    let mut slot = LOADED.lock().map_err(|_| "the model lock is poisoned".to_string())?;
    let same = slot.as_ref().is_some_and(|loaded| loaded.path == path);
    if !same {
        let env = ort::init()
            .with_name("shale-failure")
            .with_telemetry(false)
            .build()
            .map_err(|err| err.to_string())?;
        let session = Session::builder(&env)
            .map_err(|err| err.to_string())?
            .commit_from_file(path)
            .map_err(|err| err.to_string())?;
        let shape = session.inputs()[0]
            .dtype()
            .tensor_shape()
            .ok_or_else(|| "the model input is not a tensor".to_string())?;
        let height = dim(shape, 2)?;
        let width = dim(shape, 3)?;
        *slot = Some(Loaded {
            path: path.to_path_buf(),
            env,
            session,
            width,
            height,
        });
    }
    let loaded = slot.as_mut().expect("the model was just loaded");
    let resized = image::imageops::resize(frame, loaded.width, loaded.height, FilterType::Triangle);
    let pixels = (loaded.width as usize) * (loaded.height as usize);
    let mut data = vec![0f32; 3 * pixels];
    for y in 0..loaded.height {
        for x in 0..loaded.width {
            let pixel = resized.get_pixel(x, y);
            let index = (y * loaded.width + x) as usize;
            data[index] = pixel[0] as f32 / 255.0;
            data[pixels + index] = pixel[1] as f32 / 255.0;
            data[pixels * 2 + index] = pixel[2] as f32 / 255.0;
        }
    }
    let tensor = Tensor::from_array((
        [1usize, 3, loaded.height as usize, loaded.width as usize],
        data.into_boxed_slice(),
    ))
    .map_err(|err| err.to_string())?;
    let outputs = loaded
        .session
        .run(ort::inputs![tensor])
        .map_err(|err| err.to_string())?;
    if outputs.len() < 2 {
        return Err("the model did not return boxes and confidences".into());
    }
    let (box_shape, box_data) = outputs[0]
        .try_extract_tensor::<f32>()
        .map_err(|err| err.to_string())?;
    let (conf_shape, conf_data) = outputs[1]
        .try_extract_tensor::<f32>()
        .map_err(|err| err.to_string())?;
    let boxes = decode_outputs(
        &shape_of(box_shape),
        box_data,
        &shape_of(conf_shape),
        conf_data,
        threshold,
    )?;
    Ok(finding_from(&boxes))
}

#[cfg(feature = "onnx")]
fn dim(shape: &ort::value::Shape, index: usize) -> Result<u32, String> {
    let value = shape.get(index).copied().unwrap_or(-1);
    if value <= 0 {
        return Err("the model input size is not fixed".into());
    }
    u32::try_from(value).map_err(|_| "the model input size is not a number".into())
}

#[cfg(feature = "onnx")]
fn shape_of(shape: &ort::value::Shape) -> Vec<usize> {
    shape
        .iter()
        .map(|value| usize::try_from(*value).unwrap_or(0))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_confident_box_is_kept_and_a_weak_one_is_dropped() {
        let boxes = [0.10, 0.20, 0.50, 0.60, 0.00, 0.00, 0.20, 0.20];
        let confs = [0.91, 0.10];
        let found = decode_outputs(&[1, 2, 1, 4], &boxes, &[1, 2, 1], &confs, 0.6).expect("decode");
        assert_eq!(found.len(), 1);
        assert!((found[0].x1 - 0.10).abs() < 1e-5);
        assert!((found[0].confidence - 0.91).abs() < 1e-5);
        let finding = finding_from(&found);
        assert_eq!(finding.kind, FailureKind::Spaghetti);
        assert_eq!(finding.boxes[0].label, "failure");
        assert!((finding.boxes[0].w - 0.40).abs() < 1e-5);
    }

    #[test]
    fn overlapping_boxes_collapse_to_the_stronger_one() {
        let boxes = [0.10, 0.10, 0.60, 0.60, 0.12, 0.12, 0.58, 0.58];
        let confs = [0.80, 0.95];
        let found = decode_outputs(&[1, 2, 4], &boxes, &[1, 2, 1], &confs, 0.5).expect("decode");
        assert_eq!(found.len(), 1);
        assert!((found[0].confidence - 0.95).abs() < 1e-5);
    }
}
