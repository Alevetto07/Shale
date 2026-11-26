//! A Bambu `.gcode.3mf`: the zip Bambu Studio uploads and the printer reprints.
//!
//! The layout follows `src/libslic3r/Format/bbs_3mf.cpp` in bambulab/BambuStudio
//! (AGPL-3.0). That file writes `[Content_Types].xml`, `_rels/.rels`,
//! `3D/3dmodel.model`, `Metadata/plate_N.gcode`, `Metadata/plate_N.gcode.md5`
//! (uppercase MD5 of the G-code), `Metadata/plate_N.png` and
//! `Metadata/plate_N_small.png`, `Metadata/slice_info.config`,
//! `Metadata/model_settings.config`, and `Metadata/project_settings.config`.
//!
//! Studio keeps the AMS tray map on the `project_file` command. The same array
//! is also written here, into `slice_info.config` and `project_settings.config`,
//! so a file and the command that starts it name the same trays. Filament
//! identity (type, colour, metres) is the part Studio puts in `slice_info`.
//!
//! The thumbnails are flat previews of a fixed size, not a render of the mesh.
//! The printer uses them as the plate picture on its screen.

use std::io::{Cursor, Write};

use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipArchive, ZipWriter};

/// One filament row in `slice_info.config`. `id` is 1-based, as Studio writes it.
#[derive(Clone, Debug, PartialEq)]
pub struct FilamentLine {
    pub id: u32,
    pub material: String,
    pub colour: String,
    pub tray_info_idx: String,
    pub used_m: f64,
    pub used_g: f64,
}

/// One sliced plate inside the archive. `index` is 1-based.
#[derive(Clone, Debug, PartialEq)]
pub struct PlateFile {
    pub index: u32,
    pub name: String,
    pub gcode: Vec<u8>,
    pub printer_model_id: String,
    pub nozzle: String,
    pub seconds: u64,
    pub filaments: Vec<FilamentLine>,
    /// Tool index to tray. `-1` is the external spool. The same array goes on
    /// the `project_file` command.
    pub ams_mapping: Vec<i32>,
    pub bed_type: String,
}

/// A plate a reader found. `gcode_path` is the `param` a reprint command uses.
#[derive(Clone, Debug, PartialEq)]
pub struct FoundPlate {
    pub index: u32,
    pub gcode_path: String,
    pub ams_mapping: Vec<i32>,
}

/// Build one plate from a G-code file. Time comes from `;TIME:`. Filament rows
/// follow the tray map, one per entry, with metres taken from that tool.
#[allow(clippy::too_many_arguments)]
pub fn plate_from_gcode(
    gcode: &[u8],
    name: &str,
    printer_model_id: &str,
    nozzle: &str,
    material: &str,
    colour: &str,
    ams_mapping: &[i32],
    bed_type: &str,
) -> PlateFile {
    let text = String::from_utf8_lossy(gcode);
    let seconds = time_seconds(&text);
    let usage = metres_by_tool(&text);
    let mapping = if ams_mapping.is_empty() {
        vec![-1]
    } else {
        ams_mapping.to_vec()
    };
    let material = if material.trim().is_empty() {
        "PLA".to_string()
    } else {
        material.trim().to_string()
    };
    let colour = if colour.trim().is_empty() {
        "#FFFFFF".to_string()
    } else {
        colour.trim().to_string()
    };
    let filaments = mapping
        .iter()
        .enumerate()
        .map(|(index, _slot)| {
            let used_m = usage.get(&(index as u8)).copied().unwrap_or(0.0);
            FilamentLine {
                id: (index as u32) + 1,
                material: material.clone(),
                colour: colour.clone(),
                tray_info_idx: material.clone(),
                used_m,
                used_g: grams_from_metres(used_m),
                }
        })
        .collect();
    PlateFile {
        index: 1,
        name: if name.trim().is_empty() {
            "plate".to_string()
        } else {
            name.trim().to_string()
        },
        gcode: gcode.to_vec(),
        printer_model_id: printer_model_id.to_string(),
        nozzle: if nozzle.trim().is_empty() {
            "0.4".to_string()
        } else {
            nozzle.trim().to_string()
        },
        seconds,
        filaments,
        ams_mapping: mapping,
        bed_type: bed_type.to_string(),
    }
}

/// Write the archive. Plate indexes are the ones on each [`PlateFile`].
pub fn write(plates: &[PlateFile]) -> Result<Vec<u8>, String> {
    if plates.is_empty() {
        return Err("A sliced 3MF needs at least one plate.".into());
    }
    let mut cursor = Cursor::new(Vec::new());
    let mut zip = ZipWriter::new(&mut cursor);
    let stored = SimpleFileOptions::default().compression_method(CompressionMethod::Stored);
    let deflated = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);
    let mut add = |name: &str, bytes: &[u8], options: SimpleFileOptions| -> Result<(), String> {
        zip.start_file(name, options)
            .map_err(|err| format!("Could not add {name}: {err}"))?;
        zip.write_all(bytes)
            .map_err(|err| format!("Could not write {name}: {err}"))?;
        Ok(())
    };
    add("[Content_Types].xml", CONTENT_TYPES.as_bytes(), deflated)?;
    add("_rels/.rels", RELATIONSHIPS.as_bytes(), deflated)?;
    add("3D/3dmodel.model", MODEL.as_bytes(), deflated)?;
    add(
        "Metadata/slice_info.config",
        slice_info(plates).as_bytes(),
        deflated,
    )?;
    add(
        "Metadata/model_settings.config",
        model_settings(plates).as_bytes(),
        deflated,
    )?;
    add(
        "Metadata/project_settings.config",
        project_settings(plates).as_bytes(),
        deflated,
    )?;
    for plate in plates {
        let index = plate.index.max(1);
        let gcode_name = format!("Metadata/plate_{index}.gcode");
        add(&gcode_name, &plate.gcode, deflated)?;
        let digest = md5_hex(&plate.gcode);
        add(
            &format!("Metadata/plate_{index}.gcode.md5"),
            digest.as_bytes(),
            deflated,
        )?;
        // Studio stores thumbnails uncompressed.
        add(
            &format!("Metadata/plate_{index}.png"),
            &png(128, 128, [40, 44, 52, 255]),
            stored,
        )?;
        add(
            &format!("Metadata/plate_{index}_small.png"),
            &png(32, 32, [40, 44, 52, 255]),
            stored,
        )?;
    }
    zip.finish()
        .map_err(|err| format!("Could not finish the 3MF: {err}"))?;
    Ok(cursor.into_inner())
}

/// Plates in a `.gcode.3mf`, lowest index first. A plate named only in
/// `slice_info.config` is kept when its G-code is in the archive. The tray
/// map is the one in `slice_info`, or `project_settings` when that metadata
/// is absent.
pub fn plates_in(bytes: &[u8]) -> Result<Vec<FoundPlate>, String> {
    let mut archive = ZipArchive::new(Cursor::new(bytes))
        .map_err(|err| format!("This is not a 3MF archive: {err}"))?;
    let mut names = Vec::new();
    for index in 0..archive.len() {
        let file = archive
            .by_index(index)
            .map_err(|err| format!("Could not read the archive: {err}"))?;
        names.push(file.name().replace('\\', "/"));
    }
    let slice = read_named(&mut archive, "Metadata/slice_info.config").unwrap_or_default();
    let project = read_named(&mut archive, "Metadata/project_settings.config").unwrap_or_default();
    let from_info = indexes_in_slice_info(&slice);
    let from_files = gcode_indexes(&names);
    let mut indexes = from_info;
    for index in from_files {
        if !indexes.contains(&index) {
            indexes.push(index);
        }
    }
    indexes.retain(|index| names.iter().any(|name| name == &format!("Metadata/plate_{index}.gcode")));
    indexes.sort_unstable();
    indexes.dedup();
    if indexes.is_empty() {
        return Err("This 3MF has no sliced plate.".into());
    }
    let mapping = ams_from_slice_info(&slice).or_else(|| ams_from_project(&project));
    Ok(indexes
        .into_iter()
        .map(|index| FoundPlate {
            index,
            gcode_path: format!("Metadata/plate_{index}.gcode"),
            ams_mapping: mapping.clone().unwrap_or_default(),
        })
        .collect())
}

fn read_named(archive: &mut ZipArchive<Cursor<&[u8]>>, name: &str) -> Option<String> {
    let mut file = archive.by_name(name).ok()?;
    let mut text = String::new();
    std::io::Read::read_to_string(&mut file, &mut text).ok()?;
    Some(text)
}

fn indexes_in_slice_info(xml: &str) -> Vec<u32> {
    let mut found = Vec::new();
    for (key, value) in metadata_pairs(xml) {
        if key == "index" {
            if let Ok(index) = value.parse::<u32>() {
                if index > 0 {
                    found.push(index);
                }
            }
        }
    }
    found
}

fn ams_from_slice_info(xml: &str) -> Option<Vec<i32>> {
    for (key, value) in metadata_pairs(xml) {
        if key == "ams_mapping" {
            return Some(parse_mapping(&value));
        }
    }
    None
}

fn metadata_pairs(xml: &str) -> Vec<(String, String)> {
    let mut pairs = Vec::new();
    let mut rest = xml;
    while let Some(start) = rest.find("<metadata ") {
        rest = &rest[start + 10..];
        let end = rest.find("/>").unwrap_or(rest.len());
        let tag = &rest[..end];
        rest = &rest[end..];
        let key = attr(tag, "key");
        let value = attr(tag, "value");
        if let (Some(key), Some(value)) = (key, value) {
            pairs.push((key, value));
        }
    }
    pairs
}

fn attr(tag: &str, name: &str) -> Option<String> {
    let needle = format!("{name}=\"");
    let start = tag.find(&needle)? + needle.len();
    let end = tag[start..].find('"')? + start;
    Some(tag[start..end].to_string())
}

fn gcode_indexes(names: &[String]) -> Vec<u32> {
    let mut found = Vec::new();
    for name in names {
        let Some(rest) = name.strip_prefix("Metadata/plate_") else {
            continue;
        };
        let Some(number) = rest.strip_suffix(".gcode") else {
            continue;
        };
        if number.contains('.') {
            continue;
        }
        if let Ok(index) = number.parse::<u32>() {
            if index > 0 {
                found.push(index);
            }
        }
    }
    found
}

fn ams_from_project(json: &str) -> Option<Vec<i32>> {
    let value: serde_json::Value = serde_json::from_str(json).ok()?;
    let list = value.get("ams_mapping")?.as_array()?;
    Some(
        list.iter()
            .filter_map(|item| item.as_i64().and_then(|value| i32::try_from(value).ok()))
            .collect(),
    )
}

fn parse_mapping(text: &str) -> Vec<i32> {
    text.split([' ', ','])
        .filter(|part| !part.is_empty())
        .filter_map(|part| part.parse::<i32>().ok())
        .collect()
}

fn slice_info(plates: &[PlateFile]) -> String {
    let mut out = String::from(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<config>\n  <header>\n    <header_item key=\"X-BBL-Client-Type\" value=\"slicer\"/>\n    <header_item key=\"X-BBL-Client-Version\" value=\"00.01.04.00\"/>\n  </header>\n",
    );
    for plate in plates {
        let index = plate.index.max(1);
        let weight: f64 = plate.filaments.iter().map(|line| line.used_g).sum();
        out.push_str("  <plate>\n");
        meta(&mut out, "index", &index.to_string());
        meta(&mut out, "printer_model_id", &plate.printer_model_id);
        meta(&mut out, "nozzle_diameters", &plate.nozzle);
        meta(&mut out, "timelapse_type", "0");
        meta(&mut out, "prediction", &plate.seconds.to_string());
        meta(&mut out, "weight", &format!("{weight:.2}"));
        meta(&mut out, "outside", "false");
        meta(&mut out, "support_used", "false");
        meta(&mut out, "label_object_enabled", "false");
        meta(
            &mut out,
            "ams_mapping",
            &plate
                .ams_mapping
                .iter()
                .map(|slot| slot.to_string())
                .collect::<Vec<_>>()
                .join(" "),
        );
        for line in &plate.filaments {
            out.push_str(&format!(
                "    <filament id=\"{}\" tray_info_idx=\"{}\" type=\"{}\" color=\"{}\" used_m=\"{:.2}\" used_g=\"{:.2}\"/>\n",
                line.id,
                xml(&line.tray_info_idx),
                xml(&line.material),
                xml(&line.colour),
                line.used_m,
                line.used_g,
            ));
        }
        out.push_str("  </plate>\n");
    }
    out.push_str("</config>\n");
    out
}

fn meta(out: &mut String, key: &str, value: &str) {
    out.push_str(&format!(
        "    <metadata key=\"{key}\" value=\"{}\"/>\n",
        xml(value)
    ));
}

fn model_settings(plates: &[PlateFile]) -> String {
    let mut out = String::from("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<config>\n");
    for plate in plates {
        let index = plate.index.max(1);
        out.push_str("  <plate>\n");
        meta(&mut out, "plater_id", &index.to_string());
        meta(&mut out, "plater_name", &plate.name);
        meta(&mut out, "locked", "false");
        if !plate.bed_type.is_empty() {
            meta(&mut out, "bed_type", &plate.bed_type);
        }
        meta(
            &mut out,
            "gcode_file",
            &format!("Metadata/plate_{index}.gcode"),
        );
        meta(
            &mut out,
            "thumbnail_file",
            &format!("Metadata/plate_{index}.png"),
        );
        meta(
            &mut out,
            "thumbnail_no_light_file",
            &format!("Metadata/plate_{index}_small.png"),
        );
        out.push_str("  </plate>\n");
    }
    out.push_str("</config>\n");
    out
}

fn project_settings(plates: &[PlateFile]) -> String {
    let plate = &plates[0];
    let colours: Vec<&str> = plate.filaments.iter().map(|line| line.colour.as_str()).collect();
    let types: Vec<&str> = plate.filaments.iter().map(|line| line.material.as_str()).collect();
    let ids: Vec<&str> = plate
        .filaments
        .iter()
        .map(|line| line.tray_info_idx.as_str())
        .collect();
    serde_json::json!({
        "name": "project_settings",
        "from": "project",
        "version": "0.1.4",
        "filament_colour": colours,
        "filament_type": types,
        "filament_ids": ids,
        "ams_mapping": plate.ams_mapping,
    })
    .to_string()
}

fn xml(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

fn time_seconds(gcode: &str) -> u64 {
    for line in gcode.lines() {
        if let Some(value) = line.trim().strip_prefix(";TIME:") {
            if let Ok(seconds) = value.trim().parse::<f64>() {
                if seconds > 0.0 && seconds != 6666.0 {
                    return seconds.round() as u64;
                }
            }
        }
    }
    0
}

fn metres_by_tool(gcode: &str) -> std::collections::BTreeMap<u8, f64> {
    let mut absolute = true;
    let mut position = 0.0f64;
    let mut tool = 0u8;
    let mut totals = std::collections::BTreeMap::new();
    for raw in gcode.lines() {
        let line = raw.trim();
        if line.starts_with(';') {
            continue;
        }
        if line.starts_with("M82") {
            absolute = true;
            continue;
        }
        if line.starts_with("M83") {
            absolute = false;
            continue;
        }
        if let Some(rest) = line.strip_prefix('T') {
            let digits: String = rest.chars().take_while(|ch| ch.is_ascii_digit()).collect();
            if let Ok(index) = digits.parse::<u8>() {
                tool = index;
            }
            continue;
        }
        let Some(e) = extrusion(line) else {
            continue;
        };
        let delta = if absolute { (e - position).max(0.0) } else { e.max(0.0) };
        if absolute {
            position = e;
        }
        *totals.entry(tool).or_insert(0.0) += delta / 1000.0;
    }
    totals
}

fn extrusion(line: &str) -> Option<f64> {
    if !line.starts_with('G') {
        return None;
    }
    for part in line.split_whitespace() {
        if let Some(value) = part.strip_prefix('E').or_else(|| part.strip_prefix('e')) {
            return value.parse::<f64>().ok();
        }
    }
    None
}

fn grams_from_metres(metres: f64) -> f64 {
    // 1.75 mm filament, PLA at 1.24 g/cm³. A missing length stays zero.
    let radius_cm = 0.0875;
    metres * 100.0 * std::f64::consts::PI * radius_cm * radius_cm * 1.24
}

const CONTENT_TYPES: &str = "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
<Types xmlns=\"http://schemas.openxmlformats.org/package/2006/content-types\">\n\
 <Default Extension=\"rels\" ContentType=\"application/vnd.openxmlformats-package.relationships+xml\"/>\n\
 <Default Extension=\"model\" ContentType=\"application/vnd.ms-package.3dmanufacturing-3dmodel+xml\"/>\n\
 <Default Extension=\"png\" ContentType=\"image/png\"/>\n\
 <Default Extension=\"gcode\" ContentType=\"text/x.gcode\"/>\n\
</Types>\n";

const RELATIONSHIPS: &str = "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
<Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\">\n\
 <Relationship Target=\"/3D/3dmodel.model\" Id=\"rel-1\" Type=\"http://schemas.microsoft.com/3dmanufacturing/2013/01/3dmodel\"/>\n\
</Relationships>\n";

const MODEL: &str = "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
<model unit=\"millimeter\" xml:lang=\"en-US\" xmlns=\"http://schemas.microsoft.com/3dmanufacturing/core/2015/02\">\n\
 <metadata name=\"Application\">Shale</metadata>\n\
 <resources>\n\
  <object id=\"1\" type=\"model\">\n\
   <mesh>\n\
    <vertices>\n\
     <vertex x=\"0\" y=\"0\" z=\"0\"/>\n\
     <vertex x=\"1\" y=\"0\" z=\"0\"/>\n\
     <vertex x=\"0\" y=\"1\" z=\"0\"/>\n\
    </vertices>\n\
    <triangles>\n\
     <triangle v1=\"0\" v2=\"1\" v3=\"2\"/>\n\
    </triangles>\n\
   </mesh>\n\
  </object>\n\
 </resources>\n\
 <build>\n\
  <item objectid=\"1\"/>\n\
 </build>\n\
</model>\n";

/// Uppercase MD5, the 32 characters Studio stores in `plate_N.gcode.md5`.
pub fn md5_hex(bytes: &[u8]) -> String {
    md5(bytes)
        .iter()
        .map(|byte| format!("{byte:02X}"))
        .collect()
}

fn md5(message: &[u8]) -> [u8; 16] {
    const S: [u32; 64] = [
        7, 12, 17, 22, 7, 12, 17, 22, 7, 12, 17, 22, 7, 12, 17, 22, 5, 9, 14, 20, 5, 9, 14, 20, 5,
        9, 14, 20, 5, 9, 14, 20, 4, 11, 16, 23, 4, 11, 16, 23, 4, 11, 16, 23, 4, 11, 16, 23, 6, 10,
        15, 21, 6, 10, 15, 21, 6, 10, 15, 21, 6, 10, 15, 21,
    ];
    let mut k = [0u32; 64];
    for (index, slot) in k.iter_mut().enumerate() {
        *slot = (2f64.powf(32.0) * ((index as f64 + 1.0).sin().abs())) as u32;
    }
    let bit_len = (message.len() as u64).wrapping_mul(8);
    let mut data = message.to_vec();
    data.push(0x80);
    while (data.len() % 64) != 56 {
        data.push(0);
    }
    data.extend_from_slice(&bit_len.to_le_bytes());
    let mut a0 = 0x67452301u32;
    let mut b0 = 0xefcdab89u32;
    let mut c0 = 0x98badcfeu32;
    let mut d0 = 0x10325476u32;
    for chunk in data.chunks(64) {
        let mut words = [0u32; 16];
        for (index, word) in words.iter_mut().enumerate() {
            let block = &chunk[index * 4..index * 4 + 4];
            *word = u32::from_le_bytes([block[0], block[1], block[2], block[3]]);
        }
        let (mut a, mut b, mut c, mut d) = (a0, b0, c0, d0);
        for i in 0..64 {
            let (f, g) = if i < 16 {
                ((b & c) | (!b & d), i)
            } else if i < 32 {
                ((d & b) | (!d & c), (5 * i + 1) % 16)
            } else if i < 48 {
                (b ^ c ^ d, (3 * i + 5) % 16)
            } else {
                (c ^ (b | !d), (7 * i) % 16)
            };
            let next = b.wrapping_add(
                a.wrapping_add(f)
                    .wrapping_add(k[i])
                    .wrapping_add(words[g])
                    .rotate_left(S[i]),
            );
            a = d;
            d = c;
            c = b;
            b = next;
        }
        a0 = a0.wrapping_add(a);
        b0 = b0.wrapping_add(b);
        c0 = c0.wrapping_add(c);
        d0 = d0.wrapping_add(d);
    }
    let mut out = [0u8; 16];
    out[0..4].copy_from_slice(&a0.to_le_bytes());
    out[4..8].copy_from_slice(&b0.to_le_bytes());
    out[8..12].copy_from_slice(&c0.to_le_bytes());
    out[12..16].copy_from_slice(&d0.to_le_bytes());
    out
}

fn png(width: u32, height: u32, rgba: [u8; 4]) -> Vec<u8> {
    let mut raw = Vec::with_capacity((width as usize + 1) * height as usize * 4);
    for _row in 0..height {
        raw.push(0);
        for _column in 0..width {
            raw.extend_from_slice(&rgba);
        }
    }
    let mut ihdr = Vec::new();
    ihdr.extend_from_slice(&width.to_be_bytes());
    ihdr.extend_from_slice(&height.to_be_bytes());
    ihdr.extend_from_slice(&[8, 6, 0, 0, 0]);
    let mut png = vec![0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A];
    png.extend(chunk(b"IHDR", &ihdr));
    png.extend(chunk(b"IDAT", &zlib_store(&raw)));
    png.extend(chunk(b"IEND", &[]));
    png
}

fn chunk(kind: &[u8; 4], data: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(&(data.len() as u32).to_be_bytes());
    out.extend_from_slice(kind);
    out.extend_from_slice(data);
    let mut crc_bytes = kind.to_vec();
    crc_bytes.extend_from_slice(data);
    out.extend_from_slice(&crc32(&crc_bytes).to_be_bytes());
    out
}

fn zlib_store(data: &[u8]) -> Vec<u8> {
    let mut out = vec![0x78, 0x01];
    let mut offset = 0;
    while offset < data.len() || data.is_empty() && offset == 0 {
        let take = (data.len() - offset).min(65535);
        let last = offset + take >= data.len();
        out.push(if last { 1 } else { 0 });
        out.extend_from_slice(&(take as u16).to_le_bytes());
        out.extend_from_slice(&(!take as u16).to_le_bytes());
        out.extend_from_slice(&data[offset..offset + take]);
        offset += take;
        if data.is_empty() {
            break;
        }
    }
    out.extend_from_slice(&adler32(data).to_be_bytes());
    out
}

fn adler32(data: &[u8]) -> u32 {
    let mut a = 1u32;
    let mut b = 0u32;
    for byte in data {
        a = (a + u32::from(*byte)) % 65521;
        b = (b + a) % 65521;
    }
    (b << 16) | a
}

fn crc32(data: &[u8]) -> u32 {
    let mut crc = 0xFFFF_FFFFu32;
    for byte in data {
        crc ^= u32::from(*byte);
        for _ in 0..8 {
            let mask = (crc & 1).wrapping_neg();
            crc = (crc >> 1) ^ (0xEDB8_8320 & mask);
        }
    }
    !crc
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample(index: u32, mapping: &[i32]) -> PlateFile {
        let mut plate = plate_from_gcode(
            b";TIME:90\nT0\nG1 E10\nT1\nG1 E5\n",
            "bracket",
            "N2S",
            "0.4",
            "PLA",
            "#112233",
            mapping,
            "textured_plate",
        );
        plate.index = index;
        plate
    }

    #[test]
    fn the_archive_matches_studios_plate_files() {
        let bytes = write(&[sample(1, &[0, -1])]).unwrap();
        let mut archive = ZipArchive::new(Cursor::new(bytes.as_slice())).unwrap();
        let names: Vec<String> = (0..archive.len())
            .map(|index| archive.by_index(index).unwrap().name().to_string())
            .collect();
        for required in [
            "[Content_Types].xml",
            "_rels/.rels",
            "3D/3dmodel.model",
            "Metadata/plate_1.gcode",
            "Metadata/plate_1.gcode.md5",
            "Metadata/plate_1.png",
            "Metadata/plate_1_small.png",
            "Metadata/slice_info.config",
            "Metadata/model_settings.config",
            "Metadata/project_settings.config",
        ] {
            assert!(names.iter().any(|name| name == required), "{required}");
        }
        let mut digest = String::new();
        std::io::Read::read_to_string(
            &mut archive.by_name("Metadata/plate_1.gcode.md5").unwrap(),
            &mut digest,
        )
        .unwrap();
        let mut gcode = Vec::new();
        std::io::Read::read_to_end(
            &mut archive.by_name("Metadata/plate_1.gcode").unwrap(),
            &mut gcode,
        )
        .unwrap();
        assert_eq!(digest, md5_hex(&gcode));
        assert_eq!(md5_hex(b""), "D41D8CD98F00B204E9800998ECF8427E");
        let mut png_bytes = Vec::new();
        std::io::Read::read_to_end(
            &mut archive.by_name("Metadata/plate_1.png").unwrap(),
            &mut png_bytes,
        )
        .unwrap();
        assert_eq!(&png_bytes[..8], b"\x89PNG\r\n\x1a\n");
        let mut slice = String::new();
        std::io::Read::read_to_string(
            &mut archive.by_name("Metadata/slice_info.config").unwrap(),
            &mut slice,
        )
        .unwrap();
        assert!(slice.contains("key=\"index\" value=\"1\""));
        assert!(slice.contains("key=\"printer_model_id\" value=\"N2S\""));
        assert!(slice.contains("key=\"ams_mapping\" value=\"0 -1\""));
        assert!(slice.contains("tray_info_idx=\"PLA\""));
        assert!(slice.contains("type=\"PLA\""));
        let mut settings = String::new();
        std::io::Read::read_to_string(
            &mut archive.by_name("Metadata/project_settings.config").unwrap(),
            &mut settings,
        )
        .unwrap();
        assert!(settings.contains("\"ams_mapping\":[0,-1]"));
        let mut model = String::new();
        std::io::Read::read_to_string(
            &mut archive.by_name("Metadata/model_settings.config").unwrap(),
            &mut model,
        )
        .unwrap();
        assert!(model.contains("Metadata/plate_1.gcode"));
        assert!(model.contains("key=\"plater_id\" value=\"1\""));
    }

    #[test]
    fn a_file_whose_only_plate_is_the_second_is_not_read_as_the_first() {
        let bytes = write(&[sample(2, &[2, 0])]).unwrap();
        let plates = plates_in(&bytes).unwrap();
        assert_eq!(plates.len(), 1);
        assert_eq!(plates[0].index, 2);
        assert_eq!(plates[0].gcode_path, "Metadata/plate_2.gcode");
        assert_eq!(plates[0].ams_mapping, vec![2, 0]);
    }
}
