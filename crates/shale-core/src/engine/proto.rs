//! Just enough of CuraEngine 5.13's `Cura.proto` to speak Arcus.
//!
//! Field numbers match that file. The `.proto` source is not vendored. Values
//! are encoded with the protobuf wire format, which is the byte layout
//! libArcus puts after the type id.

use crate::engine::LivePath;

pub fn type_id(name: &str) -> u32 {
    // FNV-1a 32-bit, the hash libArcus uses instead of std::hash so every
    // compiler agrees. The input is protobuf's GetTypeName(), `cura.proto.Slice`.
    let mut result = 2_166_136_261u32;
    for byte in name.as_bytes() {
        result ^= u32::from(*byte);
        result = result.wrapping_mul(16_777_619);
    }
    result
}

pub fn slice_message(
    global: &[(String, String)],
    extruders: &[Vec<(String, String)>],
    objects: &[ObjectMesh],
) -> Vec<u8> {
    let mut objects_bytes = Vec::new();
    for (index, object) in objects.iter().enumerate() {
        objects_bytes.extend(message_field(1, &object_message(index as i64, object)));
    }
    let mut body = Vec::new();
    body.extend(message_field(1, &objects_bytes));
    body.extend(message_field(2, &setting_list(global)));
    for (index, settings) in extruders.iter().enumerate() {
        let mut extruder = Vec::new();
        extruder.extend(varint_field(1, index as u64));
        extruder.extend(message_field(2, &setting_list(settings)));
        body.extend(message_field(3, &extruder));
    }
    body.extend(string_field(7, "5.13.0"));
    body
}

pub struct ObjectMesh {
    pub name: String,
    pub vertices: Vec<f32>,
    pub indices: Vec<i32>,
    pub settings: Vec<(String, String)>,
}

fn object_message(id: i64, object: &ObjectMesh) -> Vec<u8> {
    let mut body = Vec::new();
    body.extend(varint_field(1, id as u64));
    body.extend(bytes_field(2, &f32s(&object.vertices)));
    body.extend(bytes_field(4, &i32s(&object.indices)));
    for (name, value) in &object.settings {
        body.extend(message_field(5, &setting(name, value)));
    }
    body.extend(string_field(6, &object.name));
    body
}

fn setting_list(settings: &[(String, String)]) -> Vec<u8> {
    let mut body = Vec::new();
    for (name, value) in settings {
        body.extend(message_field(1, &setting(name, value)));
    }
    body
}

fn setting(name: &str, value: &str) -> Vec<u8> {
    let mut body = Vec::new();
    body.extend(string_field(1, name));
    // Cura.proto types the value as bytes, not string.
    body.extend(bytes_field(2, value.as_bytes()));
    body
}

#[derive(Clone, Debug)]
pub enum Incoming {
    Progress(f32),
    Layer {
        index: i32,
        z: f32,
        paths: Vec<LivePath>,
    },
    Gcode(Vec<u8>),
    PrintTime(f64),
    Finished,
    Other(u32),
}

pub fn decode(type_name_id: u32, data: &[u8]) -> Incoming {
    if type_name_id == type_id("cura.proto.Progress") {
        Incoming::Progress(first_float(data).unwrap_or(0.0))
    } else if type_name_id == type_id("cura.proto.Layer") {
        decode_layer(data, false)
    } else if type_name_id == type_id("cura.proto.LayerOptimized") {
        decode_layer(data, true)
    } else if type_name_id == type_id("cura.proto.GCodeLayer")
        || type_name_id == type_id("cura.proto.GCodePrefix")
    {
        Incoming::Gcode(first_bytes(data, 2).unwrap_or_default())
    } else if type_name_id == type_id("cura.proto.PrintTimeMaterialEstimates") {
        Incoming::PrintTime(sum_time_fields(data))
    } else if type_name_id == type_id("cura.proto.SlicingFinished") {
        Incoming::Finished
    } else {
        Incoming::Other(type_name_id)
    }
}

fn decode_layer(data: &[u8], optimized: bool) -> Incoming {
    let mut index = 0i32;
    let mut z = 0f32;
    let mut paths = Vec::new();
    for field in fields(data) {
        match field {
            Field::Varint(1, value) => index = value as i32,
            Field::Fixed32(2, value) => z = f32::from_le_bytes(value.to_le_bytes()),
            Field::Bytes(4, bytes) if !optimized => paths.extend(polygon_paths(z, &bytes)),
            Field::Bytes(4, bytes) if optimized => paths.extend(segment_paths(z, &bytes)),
            _ => {}
        }
    }
    Incoming::Layer { index, z, paths }
}

fn polygon_paths(z: f32, data: &[u8]) -> Vec<LivePath> {
    let mut kind = "extrusion".to_string();
    let mut points = Vec::new();
    for field in fields(data) {
        match field {
            Field::Varint(1, value) => kind = polygon_kind(value as i32).to_string(),
            Field::Bytes(2, bytes) => points = xy_points(z, &bytes),
            _ => {}
        }
    }
    if points.len() < 2 {
        Vec::new()
    } else {
        vec![LivePath { kind, points }]
    }
}

fn segment_paths(z: f32, data: &[u8]) -> Vec<LivePath> {
    let mut point_type = 0u64;
    let mut raw_points = Vec::new();
    let mut line_type = Vec::new();
    for field in fields(data) {
        match field {
            Field::Varint(2, value) => point_type = value,
            Field::Bytes(3, bytes) => raw_points = bytes,
            Field::Bytes(4, bytes) => line_type = bytes,
            _ => {}
        }
    }
    let points = if point_type == 1 {
        xyz_points(&raw_points)
    } else {
        xy_points(z, &raw_points)
    };
    if points.len() < 2 {
        return Vec::new();
    }
    let segments = points.len() - 1;
    if line_type.len() <= 1 {
        let kind = polygon_kind(line_type.first().copied().unwrap_or(0) as i32).to_string();
        return vec![LivePath { kind, points }];
    }
    let mut paths = Vec::new();
    let mut current_kind = String::new();
    let mut current = Vec::new();
    for index in 0..segments {
        let kind = polygon_kind(line_type.get(index).copied().unwrap_or(0) as i32).to_string();
        if current.is_empty() {
            current_kind = kind;
            current.push(points[index]);
            current.push(points[index + 1]);
        } else if kind == current_kind {
            current.push(points[index + 1]);
        } else {
            paths.push(LivePath {
                kind: current_kind,
                points: current,
            });
            current_kind = kind;
            current = vec![points[index], points[index + 1]];
        }
    }
    if current.len() >= 2 {
        paths.push(LivePath {
            kind: current_kind,
            points: current,
        });
    }
    paths
}

fn polygon_kind(value: i32) -> &'static str {
    match value {
        1 => "wall-outer",
        2 => "wall-inner",
        3 => "skin",
        4 | 7 => "support",
        5 => "skirt",
        6 => "infill",
        8 | 9 | 12 | 13 => "travel",
        10 => "support-interface",
        11 => "prime-tower",
        _ => "extrusion",
    }
}

fn xy_points(z: f32, bytes: &[u8]) -> Vec<[f32; 3]> {
    bytes
        .as_chunks::<8>().0.iter()
        .map(|chunk| {
            let x = f32::from_le_bytes(chunk[0..4].try_into().unwrap());
            let y = f32::from_le_bytes(chunk[4..8].try_into().unwrap());
            [x, y, z]
        })
        .collect()
}

fn xyz_points(bytes: &[u8]) -> Vec<[f32; 3]> {
    bytes
        .as_chunks::<12>().0.iter()
        .map(|chunk| {
            [
                f32::from_le_bytes(chunk[0..4].try_into().unwrap()),
                f32::from_le_bytes(chunk[4..8].try_into().unwrap()),
                f32::from_le_bytes(chunk[8..12].try_into().unwrap()),
            ]
        })
        .collect()
}

fn first_float(data: &[u8]) -> Option<f32> {
    fields(data).into_iter().find_map(|field| match field {
        Field::Fixed32(1, value) => Some(f32::from_le_bytes(value.to_le_bytes())),
        _ => None,
    })
}

fn first_bytes(data: &[u8], number: u32) -> Option<Vec<u8>> {
    fields(data).into_iter().find_map(|field| match field {
        Field::Bytes(n, bytes) if n == number => Some(bytes),
        _ => None,
    })
}

fn sum_time_fields(data: &[u8]) -> f64 {
    fields(data)
        .into_iter()
        .filter_map(|field| match field {
            Field::Fixed32(number, value) if (1..=12).contains(&number) => {
                Some(f64::from(f32::from_le_bytes(value.to_le_bytes())))
            }
            _ => None,
        })
        .sum()
}

enum Field {
    Varint(u32, u64),
    Fixed32(u32, u32),
    Bytes(u32, Vec<u8>),
}

fn fields(mut data: &[u8]) -> Vec<Field> {
    let mut out = Vec::new();
    while !data.is_empty() {
        let Some((key, rest)) = take_varint(data) else {
            break;
        };
        data = rest;
        let number = (key >> 3) as u32;
        let wire = (key & 7) as u8;
        match wire {
            0 => {
                let Some((value, rest)) = take_varint(data) else {
                    break;
                };
                data = rest;
                out.push(Field::Varint(number, value));
            }
            5 => {
                if data.len() < 4 {
                    break;
                }
                let value = u32::from_le_bytes(data[..4].try_into().unwrap());
                data = &data[4..];
                out.push(Field::Fixed32(number, value));
            }
            1 => {
                if data.len() < 8 {
                    break;
                }
                data = &data[8..];
            }
            2 => {
                let Some((len, rest)) = take_varint(data) else {
                    break;
                };
                data = rest;
                let len = len as usize;
                if data.len() < len {
                    break;
                }
                out.push(Field::Bytes(number, data[..len].to_vec()));
                data = &data[len..];
            }
            _ => break,
        }
    }
    out
}

fn take_varint(data: &[u8]) -> Option<(u64, &[u8])> {
    let mut value = 0u64;
    let mut shift = 0;
    for (index, byte) in data.iter().enumerate() {
        value |= u64::from(byte & 0x7f) << shift;
        if byte & 0x80 == 0 {
            return Some((value, &data[index + 1..]));
        }
        shift += 7;
        if shift > 63 {
            return None;
        }
    }
    None
}

fn f32s(values: &[f32]) -> Vec<u8> {
    let mut out = Vec::with_capacity(values.len() * 4);
    for value in values {
        out.extend(value.to_le_bytes());
    }
    out
}

fn i32s(values: &[i32]) -> Vec<u8> {
    let mut out = Vec::with_capacity(values.len() * 4);
    for value in values {
        out.extend(value.to_le_bytes());
    }
    out
}

fn varint_field(field: u32, value: u64) -> Vec<u8> {
    let mut out = Vec::new();
    write_varint(&mut out, u64::from(field << 3));
    write_varint(&mut out, value);
    out
}

fn string_field(field: u32, value: &str) -> Vec<u8> {
    bytes_field(field, value.as_bytes())
}

fn bytes_field(field: u32, value: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    write_varint(&mut out, u64::from((field << 3) | 2));
    write_varint(&mut out, value.len() as u64);
    out.extend(value);
    out
}

fn message_field(field: u32, value: &[u8]) -> Vec<u8> {
    bytes_field(field, value)
}

fn write_varint(out: &mut Vec<u8>, mut value: u64) {
    loop {
        let mut byte = (value & 0x7f) as u8;
        value >>= 7;
        if value != 0 {
            byte |= 0x80;
        }
        out.push(byte);
        if value == 0 {
            break;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fnv_matches_a_known_vector() {
        // Independent of Cura: empty string is the FNV-1a offset basis.
        assert_eq!(type_id(""), 2_166_136_261);
        assert_eq!(type_id("cura.proto.Slice"), type_id("cura.proto.Slice"));
        assert_ne!(type_id("cura.proto.Slice"), type_id("cura.proto.Progress"));
    }

    #[test]
    fn progress_round_trip() {
        let mut body = Vec::new();
        let bits = 0.5f32.to_le_bytes();
        body.extend(bytes_of_fixed32(1, u32::from_le_bytes(bits)));
        match decode(type_id("cura.proto.Progress"), &body) {
            Incoming::Progress(amount) => assert!((amount - 0.5).abs() < 1e-6),
            other => panic!("unexpected {other:?}"),
        }
    }

    fn bytes_of_fixed32(field: u32, value: u32) -> Vec<u8> {
        let mut out = Vec::new();
        write_varint(&mut out, u64::from((field << 3) | 5));
        out.extend(value.to_le_bytes());
        out
    }
}
