//! Triangle meshes from STL and a practical subset of 3MF.
//!
//! CuraEngine's command-line loader is given STL. Imported 3MF is converted
//! before slicing. Transforms match three.js object order: scale, then the
//! XYZ Euler rotation used by TransformControls, then translation. Z is up.

use std::fs;
use std::io::{Cursor, Read, Write};
use std::path::Path;

use serde::{Deserialize, Serialize};
use zip::ZipArchive;

use crate::error::Error;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Transform {
    pub translation: [f64; 3],
    pub rotation_deg: [f64; 3],
    pub scale: [f64; 3],
}

impl Default for Transform {
    fn default() -> Self {
        Self {
            translation: [0.0, 0.0, 0.0],
            rotation_deg: [0.0, 0.0, 0.0],
            scale: [1.0, 1.0, 1.0],
        }
    }
}

#[derive(Clone, Debug)]
pub struct TriangleMesh {
    pub vertices: Vec<[f32; 3]>,
    pub triangles: Vec<[u32; 3]>,
}

impl TriangleMesh {
    pub fn bounds(&self) -> ([f32; 3], [f32; 3]) {
        let mut min = [f32::MAX, f32::MAX, f32::MAX];
        let mut max = [f32::MIN, f32::MIN, f32::MIN];
        for vertex in &self.vertices {
            for axis in 0..3 {
                min[axis] = min[axis].min(vertex[axis]);
                max[axis] = max[axis].max(vertex[axis]);
            }
        }
        if self.vertices.is_empty() {
            return ([0.0; 3], [0.0; 3]);
        }
        (min, max)
    }

    /// Triangle soup for the viewport. Large meshes keep every nth triangle so
    /// the preview stays responsive; slicing still uses the full mesh.
    pub fn preview_positions(&self, max_triangles: usize) -> Vec<f32> {
        let step = if self.triangles.len() > max_triangles {
            self.triangles.len().div_ceil(max_triangles)
        } else {
            1
        };
        let mut positions = Vec::with_capacity(self.triangles.len().div_ceil(step) * 9);
        for triangle in self.triangles.iter().step_by(step) {
            for index in triangle {
                if let Some(vertex) = self.vertices.get(*index as usize) {
                    positions.extend_from_slice(vertex);
                }
            }
        }
        positions
    }
}

pub fn load_mesh(path: &Path) -> Result<TriangleMesh, Error> {
    let bytes = fs::read(path)?;
    let extension = path
        .extension()
        .and_then(|ext| ext.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    match extension.as_str() {
        "3mf" => load_3mf(&bytes),
        _ => load_stl(&bytes),
    }
}

pub fn load_stl(bytes: &[u8]) -> Result<TriangleMesh, Error> {
    if looks_like_binary_stl(bytes) {
        return parse_binary_stl(bytes);
    }
    if bytes.starts_with(b"solid") || bytes.starts_with(b"SOLID") {
        return parse_ascii_stl(bytes);
    }
    parse_binary_stl(bytes)
}

fn looks_like_binary_stl(bytes: &[u8]) -> bool {
    if bytes.len() < 84 {
        return false;
    }
    let count = u32::from_le_bytes(bytes[80..84].try_into().unwrap()) as usize;
    bytes.len() == 84 + count * 50 && count > 0
}

fn parse_binary_stl(bytes: &[u8]) -> Result<TriangleMesh, Error> {
    if bytes.len() < 84 {
        return Err(Error::Mesh("STL is too small to be a binary mesh".into()));
    }
    let count = u32::from_le_bytes(bytes[80..84].try_into().unwrap()) as usize;
    if bytes.len() < 84 + count * 50 {
        return Err(Error::Mesh("binary STL ended early".into()));
    }
    let mut vertices = Vec::with_capacity(count * 3);
    let mut triangles = Vec::with_capacity(count);
    for index in 0..count {
        let start = 84 + index * 50;
        let record = &bytes[start..start + 50];
        let mut tri = [0; 3];
        for corner in 0..3 {
            let offset = 12 + corner * 12;
            let vertex = [
                f32::from_le_bytes(record[offset..offset + 4].try_into().unwrap()),
                f32::from_le_bytes(record[offset + 4..offset + 8].try_into().unwrap()),
                f32::from_le_bytes(record[offset + 8..offset + 12].try_into().unwrap()),
            ];
            tri[corner] = vertices.len() as u32;
            vertices.push(vertex);
        }
        triangles.push(tri);
    }
    Ok(TriangleMesh {
        vertices,
        triangles,
    })
}

fn parse_ascii_stl(bytes: &[u8]) -> Result<TriangleMesh, Error> {
    let text = String::from_utf8_lossy(bytes);
    let mut vertices = Vec::new();
    let mut facet = Vec::new();
    let mut triangles = Vec::new();
    for line in text.lines() {
        let line = line.trim();
        let Some(rest) = line
            .strip_prefix("vertex")
            .or_else(|| line.strip_prefix("VERTEX"))
        else {
            continue;
        };
        let mut coords = rest.split_whitespace();
        let x: f32 = coords
            .next()
            .and_then(|value| value.parse().ok())
            .ok_or_else(|| Error::Mesh("ASCII STL vertex is missing a coordinate".into()))?;
        let y: f32 = coords
            .next()
            .and_then(|value| value.parse().ok())
            .ok_or_else(|| Error::Mesh("ASCII STL vertex is missing a coordinate".into()))?;
        let z: f32 = coords
            .next()
            .and_then(|value| value.parse().ok())
            .ok_or_else(|| Error::Mesh("ASCII STL vertex is missing a coordinate".into()))?;
        facet.push([x, y, z]);
        if facet.len() == 3 {
            let base = vertices.len() as u32;
            vertices.extend(facet.drain(..));
            triangles.push([base, base + 1, base + 2]);
        }
    }
    if triangles.is_empty() {
        return Err(Error::Mesh(
            "ASCII STL did not contain any triangles".into(),
        ));
    }
    Ok(TriangleMesh {
        vertices,
        triangles,
    })
}

pub fn write_binary_stl(path: &Path, mesh: &TriangleMesh) -> Result<(), Error> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let mut file = fs::File::create(path)?;
    file.write_all(&[0u8; 80])?;
    file.write_all(&(mesh.triangles.len() as u32).to_le_bytes())?;
    for triangle in &mesh.triangles {
        let a = vertex(mesh, triangle[0])?;
        let b = vertex(mesh, triangle[1])?;
        let c = vertex(mesh, triangle[2])?;
        let normal = face_normal(a, b, c);
        write_vec3(&mut file, normal)?;
        write_vec3(&mut file, a)?;
        write_vec3(&mut file, b)?;
        write_vec3(&mut file, c)?;
        file.write_all(&0u16.to_le_bytes())?;
    }
    Ok(())
}

fn vertex(mesh: &TriangleMesh, index: u32) -> Result<[f32; 3], Error> {
    mesh.vertices
        .get(index as usize)
        .copied()
        .ok_or_else(|| Error::Mesh(format!("triangle index {index} is out of range")))
}

fn write_vec3(file: &mut fs::File, value: [f32; 3]) -> Result<(), Error> {
    for component in value {
        file.write_all(&component.to_le_bytes())?;
    }
    Ok(())
}

fn face_normal(a: [f32; 3], b: [f32; 3], c: [f32; 3]) -> [f32; 3] {
    let ux = b[0] - a[0];
    let uy = b[1] - a[1];
    let uz = b[2] - a[2];
    let vx = c[0] - a[0];
    let vy = c[1] - a[1];
    let vz = c[2] - a[2];
    let normal = [uy * vz - uz * vy, uz * vx - ux * vz, ux * vy - uy * vx];
    let length = (normal[0] * normal[0] + normal[1] * normal[1] + normal[2] * normal[2]).sqrt();
    if length < 1e-12 {
        [0.0, 0.0, 0.0]
    } else {
        [normal[0] / length, normal[1] / length, normal[2] / length]
    }
}

pub fn apply_transform(vertex: [f64; 3], transform: &Transform) -> [f64; 3] {
    let x = vertex[0] * transform.scale[0];
    let y = vertex[1] * transform.scale[1];
    let z = vertex[2] * transform.scale[2];
    let (rx, ry, rz) = (
        transform.rotation_deg[0].to_radians(),
        transform.rotation_deg[1].to_radians(),
        transform.rotation_deg[2].to_radians(),
    );
    let (a, b) = (rx.cos(), rx.sin());
    let (c, d) = (ry.cos(), ry.sin());
    let (e, f) = (rz.cos(), rz.sin());
    let ae = a * e;
    let af = a * f;
    let be = b * e;
    let bf = b * f;
    // three.js Matrix4.makeRotationFromEuler, order XYZ, column-major.
    let te0 = c * e;
    let te4 = -c * f;
    let te8 = d;
    let te1 = af + be * d;
    let te5 = ae - bf * d;
    let te9 = -b * c;
    let te2 = bf - ae * d;
    let te6 = be + af * d;
    let te10 = a * c;
    [
        te0 * x + te4 * y + te8 * z + transform.translation[0],
        te1 * x + te5 * y + te9 * z + transform.translation[1],
        te2 * x + te6 * y + te10 * z + transform.translation[2],
    ]
}

pub fn bake(mesh: &TriangleMesh, transform: &Transform) -> TriangleMesh {
    let vertices = mesh
        .vertices
        .iter()
        .map(|vertex| {
            let point = apply_transform(
                [vertex[0] as f64, vertex[1] as f64, vertex[2] as f64],
                transform,
            );
            [point[0] as f32, point[1] as f32, point[2] as f32]
        })
        .collect();
    TriangleMesh {
        vertices,
        triangles: mesh.triangles.clone(),
    }
}

/// Shift a mesh from Shale's machine coordinates into the space CuraEngine expects.
///
/// `MeshGroup::finalize` adds half the bed when `machine_center_is_zero` is
/// false, because the frontend it was written for treats the bed centre as
/// the origin. Shale's scene uses the front-left origin in that case. The
/// copy written for the engine is moved back by half the bed so the G-code
/// lands on the part. A centre-origin machine is left unchanged.
pub fn to_engine_space(mesh: &mut TriangleMesh, width: f64, depth: f64, center_is_zero: bool) {
    if center_is_zero {
        return;
    }
    let dx = (width * 0.5) as f32;
    let dy = (depth * 0.5) as f32;
    for vertex in &mut mesh.vertices {
        vertex[0] -= dx;
        vertex[1] -= dy;
    }
}

/// Centre the mesh on the bed and sit it on Z = 0. Rotation and scale are reset.
pub fn place_on_bed(
    mesh: &TriangleMesh,
    width: f64,
    depth: f64,
    center_is_zero: bool,
) -> Transform {
    let (min, max) = mesh.bounds();
    let centre_x = (min[0] as f64 + max[0] as f64) * 0.5;
    let centre_y = (min[1] as f64 + max[1] as f64) * 0.5;
    let (tx, ty) = if center_is_zero {
        (-centre_x, -centre_y)
    } else {
        (width * 0.5 - centre_x, depth * 0.5 - centre_y)
    };
    Transform {
        translation: [tx, ty, -(min[2] as f64)],
        ..Transform::default()
    }
}

/// Move the mesh along Z so its lowest transformed vertex sits on the plate.
pub fn drop_to_bed(mesh: &TriangleMesh, transform: &mut Transform) {
    let mut min_z = f64::INFINITY;
    for vertex in &mesh.vertices {
        let point = apply_transform(
            [vertex[0] as f64, vertex[1] as f64, vertex[2] as f64],
            transform,
        );
        min_z = min_z.min(point[2]);
    }
    if min_z.is_finite() {
        transform.translation[2] -= min_z;
    }
}

fn load_3mf(bytes: &[u8]) -> Result<TriangleMesh, Error> {
    let mut archive = ZipArchive::new(Cursor::new(bytes))
        .map_err(|err| Error::Mesh(format!("3MF is not a zip archive: {err}")))?;
    let mut model_xml = None;
    for index in 0..archive.len() {
        let mut file = archive
            .by_index(index)
            .map_err(|err| Error::Mesh(format!("3MF entry: {err}")))?;
        let name = file.name().to_string();
        if name.ends_with(".model") {
            let mut text = String::new();
            file.read_to_string(&mut text)
                .map_err(|err| Error::Mesh(format!("3MF model: {err}")))?;
            model_xml = Some(text);
            break;
        }
    }
    let Some(xml) = model_xml else {
        return Err(Error::Mesh("3MF archive has no .model part".into()));
    };
    parse_3mf_model(&xml)
}

struct RawObject {
    vertices: Vec<[f32; 3]>,
    triangles: Vec<[u32; 3]>,
    components: Vec<(u32, [f64; 12])>,
}

fn parse_3mf_model(xml: &str) -> Result<TriangleMesh, Error> {
    use quick_xml::events::Event;
    use quick_xml::Reader;

    let mut reader = Reader::from_str(xml);
    reader.config_mut().trim_text(true);
    let mut buf = Vec::new();
    let mut unit = "millimeter".to_string();
    let mut objects: Vec<(u32, RawObject)> = Vec::new();
    let mut current: Option<(u32, RawObject)> = None;
    let mut in_vertices = false;
    let mut in_triangles = false;
    let mut in_components = false;
    let mut build: Vec<(u32, [f64; 12])> = Vec::new();

    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(event)) => {
                note_3mf_start(
                    &event,
                    &mut unit,
                    &mut current,
                    &mut in_vertices,
                    &mut in_triangles,
                    &mut in_components,
                    &mut build,
                );
            }
            Ok(Event::Empty(event)) => {
                note_3mf_start(
                    &event,
                    &mut unit,
                    &mut current,
                    &mut in_vertices,
                    &mut in_triangles,
                    &mut in_components,
                    &mut build,
                );
            }
            Ok(Event::End(event)) => {
                let name = local_name(event.name().as_ref());
                match name.as_str() {
                    "vertices" => in_vertices = false,
                    "triangles" => in_triangles = false,
                    "components" => in_components = false,
                    "object" => {
                        if let Some(object) = current.take() {
                            objects.push(object);
                        }
                    }
                    _ => {}
                }
            }
            Ok(Event::Eof) => break,
            Err(err) => return Err(Error::Mesh(format!("3MF XML: {err}"))),
            _ => {}
        }
        buf.clear();
    }
    let _ = (in_vertices, in_triangles, in_components);

    let scale = match unit.as_str() {
        "inch" => 25.4,
        "meter" => 1000.0,
        "micron" => 0.001,
        _ => 1.0,
    };
    let items = if build.is_empty() {
        objects
            .iter()
            .filter(|(_, object)| !object.triangles.is_empty())
            .map(|(id, _)| (*id, identity_transform()))
            .take(1)
            .collect::<Vec<_>>()
    } else {
        build
    };
    if items.is_empty() {
        return Err(Error::Mesh(
            "3MF model has no mesh. Component-only files without a build item are not supported yet.".into(),
        ));
    }
    let mut vertices = Vec::new();
    let mut triangles = Vec::new();
    for (id, transform) in items {
        flatten_object(id, transform, &objects, &mut vertices, &mut triangles, 0)?;
    }
    if scale != 1.0 {
        for vertex in &mut vertices {
            vertex[0] *= scale as f32;
            vertex[1] *= scale as f32;
            vertex[2] *= scale as f32;
        }
    }
    if triangles.is_empty() {
        return Err(Error::Mesh(
            "3MF model did not contain any triangles".into(),
        ));
    }
    Ok(TriangleMesh {
        vertices,
        triangles,
    })
}

fn note_3mf_start(
    event: &quick_xml::events::BytesStart<'_>,
    unit: &mut String,
    current: &mut Option<(u32, RawObject)>,
    in_vertices: &mut bool,
    in_triangles: &mut bool,
    in_components: &mut bool,
    build: &mut Vec<(u32, [f64; 12])>,
) {
    let name = local_name(event.name().as_ref());
    match name.as_str() {
        "model" => {
            if let Some(value) = xml_attr(event, "unit") {
                *unit = value;
            }
        }
        "object" => {
            let id = xml_attr(event, "id")
                .and_then(|value| value.parse().ok())
                .unwrap_or(0);
            *current = Some((
                id,
                RawObject {
                    vertices: Vec::new(),
                    triangles: Vec::new(),
                    components: Vec::new(),
                },
            ));
        }
        "vertices" => *in_vertices = true,
        "vertex" => {
            if let Some((_, object)) = current.as_mut() {
                object.vertices.push([
                    xml_attr(event, "x")
                        .and_then(|v| v.parse().ok())
                        .unwrap_or(0.0),
                    xml_attr(event, "y")
                        .and_then(|v| v.parse().ok())
                        .unwrap_or(0.0),
                    xml_attr(event, "z")
                        .and_then(|v| v.parse().ok())
                        .unwrap_or(0.0),
                ]);
            }
        }
        "triangles" => *in_triangles = true,
        "triangle" => {
            if let Some((_, object)) = current.as_mut() {
                object.triangles.push([
                    xml_attr(event, "v1")
                        .and_then(|v| v.parse().ok())
                        .unwrap_or(0),
                    xml_attr(event, "v2")
                        .and_then(|v| v.parse().ok())
                        .unwrap_or(0),
                    xml_attr(event, "v3")
                        .and_then(|v| v.parse().ok())
                        .unwrap_or(0),
                ]);
            }
        }
        "components" => *in_components = true,
        "component" => {
            if let Some((_, object)) = current.as_mut() {
                let id = xml_attr(event, "objectid")
                    .and_then(|value| value.parse().ok())
                    .unwrap_or(0);
                object
                    .components
                    .push((id, parse_transform(xml_attr(event, "transform").as_deref())));
            }
        }
        "item" => {
            let id = xml_attr(event, "objectid")
                .and_then(|value| value.parse().ok())
                .unwrap_or(0);
            build.push((id, parse_transform(xml_attr(event, "transform").as_deref())));
        }
        _ => {}
    }
}

fn flatten_object(
    id: u32,
    transform: [f64; 12],
    objects: &[(u32, RawObject)],
    vertices: &mut Vec<[f32; 3]>,
    triangles: &mut Vec<[u32; 3]>,
    depth: u8,
) -> Result<(), Error> {
    if depth > 6 {
        return Err(Error::Mesh("3MF component nesting is too deep".into()));
    }
    let Some((_, object)) = objects.iter().find(|(object_id, _)| *object_id == id) else {
        return Err(Error::Mesh(format!("3MF object {id} is missing")));
    };
    let base = vertices.len() as u32;
    for vertex in &object.vertices {
        vertices.push(apply_affine(*vertex, &transform));
    }
    for triangle in &object.triangles {
        triangles.push([base + triangle[0], base + triangle[1], base + triangle[2]]);
    }
    let components = object.components.clone();
    for (child, child_transform) in components {
        flatten_object(
            child,
            compose(transform, child_transform),
            objects,
            vertices,
            triangles,
            depth + 1,
        )?;
    }
    Ok(())
}

fn apply_affine(vertex: [f32; 3], matrix: &[f64; 12]) -> [f32; 3] {
    let x = vertex[0] as f64;
    let y = vertex[1] as f64;
    let z = vertex[2] as f64;
    [
        (matrix[0] * x + matrix[1] * y + matrix[2] * z + matrix[3]) as f32,
        (matrix[4] * x + matrix[5] * y + matrix[6] * z + matrix[7]) as f32,
        (matrix[8] * x + matrix[9] * y + matrix[10] * z + matrix[11]) as f32,
    ]
}

fn compose(parent: [f64; 12], child: [f64; 12]) -> [f64; 12] {
    let mut out = [0.0; 12];
    for row in 0..3 {
        for col in 0..4 {
            let mut sum = 0.0;
            for k in 0..3 {
                sum += parent[row * 4 + k] * child[k * 4 + col];
            }
            if col == 3 {
                sum += parent[row * 4 + 3];
            }
            out[row * 4 + col] = sum;
        }
    }
    out
}

fn identity_transform() -> [f64; 12] {
    [1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0]
}

fn parse_transform(raw: Option<&str>) -> [f64; 12] {
    let Some(raw) = raw else {
        return identity_transform();
    };
    let numbers: Vec<f64> = raw
        .split_whitespace()
        .filter_map(|part| part.parse().ok())
        .collect();
    if numbers.len() != 12 {
        return identity_transform();
    }
    let mut matrix = [0.0; 12];
    matrix.copy_from_slice(&numbers);
    matrix
}

fn local_name(qname: &[u8]) -> String {
    let name = std::str::from_utf8(qname).unwrap_or("");
    name.rsplit(':').next().unwrap_or(name).to_string()
}

fn xml_attr(event: &quick_xml::events::BytesStart<'_>, key: &str) -> Option<String> {
    for attribute in event.attributes().flatten() {
        if attribute.key.local_name().as_ref() == key.as_bytes() {
            return attribute
                .unescape_value()
                .ok()
                .map(|value| value.into_owned());
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use zip::write::SimpleFileOptions;

    #[test]
    fn reads_a_minimal_3mf() {
        let xml = r#"<?xml version="1.0"?>
<model unit="millimeter" xmlns="http://schemas.microsoft.com/3dmanufacturing/core/2015/02">
  <resources>
    <object id="1" type="model">
      <mesh>
        <vertices>
          <vertex x="0" y="0" z="0"/>
          <vertex x="1" y="0" z="0"/>
          <vertex x="0" y="1" z="2"/>
        </vertices>
        <triangles>
          <triangle v1="0" v2="1" v3="2"/>
        </triangles>
      </mesh>
    </object>
  </resources>
  <build>
    <item objectid="1" transform="1 0 0 10 0 1 0 0 0 0 1 0"/>
  </build>
</model>"#;
        let mut cursor = std::io::Cursor::new(Vec::new());
        {
            let mut writer = zip::ZipWriter::new(&mut cursor);
            writer
                .start_file("3D/3dmodel.model", SimpleFileOptions::default())
                .unwrap();
            writer.write_all(xml.as_bytes()).unwrap();
            writer.finish().unwrap();
        }
        let mesh = load_3mf(&cursor.into_inner()).unwrap();
        assert_eq!(mesh.triangles.len(), 1);
        assert!((mesh.vertices[0][0] - 10.0).abs() < 1e-3);
    }

    #[test]
    fn z_rotation_matches_three_js() {
        let transform = Transform {
            rotation_deg: [0.0, 0.0, 90.0],
            ..Transform::default()
        };
        let point = apply_transform([1.0, 0.0, 0.0], &transform);
        assert!((point[0]).abs() < 1e-9);
        assert!((point[1] - 1.0).abs() < 1e-9);
        assert!(point[2].abs() < 1e-9);
    }

    #[test]
    fn ascii_stl_round_trip_and_drop() {
        let stl = b"solid t
facet normal 0 0 1
 outer loop
  vertex 0 0 5
  vertex 1 0 5
  vertex 0 1 5
 endloop
endfacet
endsolid t
";
        let mesh = load_stl(stl).unwrap();
        assert_eq!(mesh.triangles.len(), 1);
        let mut transform = Transform::default();
        drop_to_bed(&mesh, &mut transform);
        let baked = bake(&mesh, &transform);
        let (min, _) = baked.bounds();
        assert!(min[2].abs() < 1e-4);
    }
}
