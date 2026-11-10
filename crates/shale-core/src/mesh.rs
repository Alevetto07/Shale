//! Triangle meshes from STL, 3MF, and tessellated STEP.
//!
//! STEP is tessellated in [`crate::cad`] before it becomes one of these meshes.
//! 3MF is already a mesh; Shale reads the triangles rather than tessellating a solid.
//!
//! CuraEngine's command-line loader is given STL. Imported 3MF is converted
//! before slicing. Transforms match three.js object order: scale, then the
//! XYZ Euler rotation used by TransformControls, then translation. Z is up.

use std::collections::HashMap;
use std::fs::{self, File};
use std::io::{Cursor, Read, Write};
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};

use serde::{Deserialize, Serialize};
use zip::ZipArchive;

use crate::error::Error;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
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

    /// Triangle soup for the viewport. Large meshes keep contiguous runs of
    /// triangles so the preview stays responsive; slicing uses the full mesh.
    pub fn preview_positions(&self, max_triangles: usize) -> Vec<f32> {
        let total = self.triangles.len();
        let budget = max_triangles.max(1);
        let mut positions = Vec::with_capacity(total.min(budget) * 9);
        let push = |triangle: &[u32; 3], positions: &mut Vec<f32>| {
            for index in triangle {
                if let Some(vertex) = self.vertices.get(*index as usize) {
                    positions.extend_from_slice(vertex);
                }
            }
        };
        if total <= budget {
            for triangle in &self.triangles {
                push(triangle, &mut positions);
            }
            return positions;
        }
        // Keep contiguous runs: sampling single triangles leaves a sprinkle of
        // lone facets that reads as broken geometry on any real model.
        let runs = 96usize;
        let run = (budget / runs).max(1);
        let stride = total.div_ceil(runs);
        let mut start = 0;
        while start < total && positions.len() / 9 < budget {
            let end = (start + run).min(total);
            for triangle in &self.triangles[start..end] {
                push(triangle, &mut positions);
            }
            start += stride;
        }
        positions
    }
}

pub fn load_mesh(path: &Path) -> Result<TriangleMesh, Error> {
    Ok(load_mesh_with(path, &crate::cad::TessellationTolerance::standard())?.mesh)
}

/// Load a model, tessellating STEP at `tolerance`. STL and 3MF ignore it.
pub fn load_mesh_with(
    path: &Path,
    tolerance: &crate::cad::TessellationTolerance,
) -> Result<crate::cad::ImportedMesh, Error> {
    load_mesh_reporting(path, tolerance, &mut |_, _| {}, &AtomicBool::new(false))
}

/// Like [`load_mesh_with`], but reports progress and stops when `cancel` is set.
///
/// Binary STL is read straight from the file in 50-byte records. The preview
/// the window receives is a decimated flat buffer, not one object per triangle.
pub fn load_mesh_reporting(
    path: &Path,
    tolerance: &crate::cad::TessellationTolerance,
    progress: &mut dyn FnMut(f32, &str),
    cancel: &AtomicBool,
) -> Result<crate::cad::ImportedMesh, Error> {
    let started = std::time::Instant::now();
    if cancel.load(Ordering::Relaxed) {
        return Err(Error::Mesh("Import cancelled".into()));
    }
    let extension = path
        .extension()
        .and_then(|ext| ext.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    let mut imported = match extension.as_str() {
        "3mf" => {
            progress(0.05, "reading 3MF");
            let bytes = std::fs::read(path)?;
            crate::cad::ImportedMesh::from_mesh(load_3mf(&bytes)?, "3mf")
        }
        "step" | "stp" => {
            progress(0.05, "reading STEP");
            let bytes = std::fs::read(path)?;
            if cancel.load(Ordering::Relaxed) {
                return Err(Error::Mesh("Import cancelled".into()));
            }
            progress(0.2, "tessellating STEP");
            crate::cad::import_step(&bytes, tolerance)?
        }
        _ => {
            progress(0.02, "reading STL");
            let mesh = load_stl_path(path, progress, cancel)?;
            crate::cad::ImportedMesh::from_mesh(mesh, "stl")
        }
    };
    if cancel.load(Ordering::Relaxed) {
        return Err(Error::Mesh("Import cancelled".into()));
    }
    if right_way_round(&mut imported.mesh) {
        progress(0.99, "turned the mesh right way round");
    }
    if imported.report.milliseconds == 0 {
        imported.report.milliseconds = started.elapsed().as_millis() as u64;
    }
    progress(1.0, "import finished");
    Ok(imported)
}

/// Load model bytes already in memory. `file_name` supplies the extension.
///
/// STEP bytes are tessellated here and are not rewritten as a mesh, so a
/// project can store the original exchange file.
pub fn load_named_bytes(
    file_name: &str,
    bytes: &[u8],
    tolerance: &crate::cad::TessellationTolerance,
    progress: &mut dyn FnMut(f32, &str),
    cancel: &AtomicBool,
) -> Result<crate::cad::ImportedMesh, Error> {
    let started = std::time::Instant::now();
    if cancel.load(Ordering::Relaxed) {
        return Err(Error::Mesh("Import cancelled".into()));
    }
    let extension = Path::new(file_name)
        .extension()
        .and_then(|ext| ext.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    // A named part can lose its extension on the way through a project archive
    // or a duplicate. The bytes still say what they are, so trust them when the
    // name is unhelpful.
    let extension = match extension.as_str() {
        "3mf" | "step" | "stp" | "stl" => extension,
        _ => sniff_format(bytes).to_string(),
    };
    let mut imported = match extension.as_str() {
        "3mf" => {
            progress(0.2, "reading 3MF");
            crate::cad::ImportedMesh::from_mesh(load_3mf(bytes)?, "3mf")
        }
        "step" | "stp" => {
            progress(0.2, "tessellating STEP");
            if cancel.load(Ordering::Relaxed) {
                return Err(Error::Mesh("Import cancelled".into()));
            }
            crate::cad::import_step(bytes, tolerance)?
        }
        _ => {
            progress(0.2, "reading STL");
            crate::cad::ImportedMesh::from_mesh(load_stl(bytes)?, "stl")
        }
    };
    if right_way_round(&mut imported.mesh) {
        progress(0.99, "turned the mesh right way round");
    }
    if imported.report.milliseconds == 0 {
        imported.report.milliseconds = started.elapsed().as_millis() as u64;
    }
    progress(1.0, "import finished");
    Ok(imported)
}

/// Guess a mesh format from the first bytes: zips are 3MF, STEP starts with a
/// readable header, and everything else is treated as STL.
fn sniff_format(bytes: &[u8]) -> &'static str {
    if bytes.starts_with(b"PK\x03\x04") || bytes.starts_with(b"PK\x05\x06") {
        return "3mf";
    }
    let head = &bytes[..bytes.len().min(32)];
    if head.starts_with(b"ISO-10303") {
        return "step";
    }
    "stl"
}

fn load_stl_path(
    path: &Path,
    progress: &mut dyn FnMut(f32, &str),
    cancel: &AtomicBool,
) -> Result<TriangleMesh, Error> {
    let mut file = File::open(path)?;
    let len = file.metadata()?.len();
    let mut header = [0u8; 84];
    if file.read(&mut header)? < 84 {
        return Err(Error::Mesh("STL is too small to be a binary mesh".into()));
    }
    let count = u32::from_le_bytes(header[80..84].try_into().unwrap()) as u64;
    let expected = 84u64.saturating_add(count.saturating_mul(50));
    let named_solid = header.starts_with(b"solid") || header.starts_with(b"SOLID");
    let binary = count > 0 && len >= expected && (len == expected || !named_solid);
    if binary {
        return parse_binary_stl_records(&mut file, count as usize, progress, cancel);
    }
    drop(file);
    progress(0.1, "reading ASCII STL");
    let bytes = std::fs::read(path)?;
    if cancel.load(Ordering::Relaxed) {
        return Err(Error::Mesh("Import cancelled".into()));
    }
    load_stl(&bytes)
}

fn parse_binary_stl_records<R: Read>(
    reader: &mut R,
    count: usize,
    progress: &mut dyn FnMut(f32, &str),
    cancel: &AtomicBool,
) -> Result<TriangleMesh, Error> {
    let mut vertices = Vec::with_capacity(count.saturating_mul(3));
    let mut triangles = Vec::with_capacity(count);
    let mut record = [0u8; 50];
    for index in 0..count {
        if index % 100_000 == 0 {
            if cancel.load(Ordering::Relaxed) {
                return Err(Error::Mesh("Import cancelled".into()));
            }
            progress(
                index as f32 / count.max(1) as f32,
                &format!("triangle {index} of {count}"),
            );
        }
        reader.read_exact(&mut record).map_err(|_| {
            Error::Mesh(format!(
                "binary STL ended early at triangle {index} of {count}"
            ))
        })?;
        let mut tri = [0; 3];
        for (corner, slot) in tri.iter_mut().enumerate() {
            let offset = 12 + corner * 12;
            *slot = vertices.len() as u32;
            vertices.push([
                f32::from_le_bytes(record[offset..offset + 4].try_into().unwrap()),
                f32::from_le_bytes(record[offset + 4..offset + 8].try_into().unwrap()),
                f32::from_le_bytes(record[offset + 8..offset + 12].try_into().unwrap()),
            ]);
        }
        triangles.push(tri);
    }
    progress(1.0, &format!("{count} triangles"));
    Ok(TriangleMesh {
        vertices,
        triangles,
    })
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
    let mut cursor = Cursor::new(&bytes[84..]);
    parse_binary_stl_records(&mut cursor, count, &mut |_, _| {}, &AtomicBool::new(false))
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
            vertices.append(&mut facet);
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

/// Axis-aligned extent of a mesh: `(min, max)`.
pub fn bounds(mesh: &TriangleMesh) -> ([f64; 3], [f64; 3]) {
    let mut min = [f64::INFINITY; 3];
    let mut max = [f64::NEG_INFINITY; 3];
    for vertex in &mesh.vertices {
        for axis in 0..3 {
            let value = vertex[axis] as f64;
            min[axis] = min[axis].min(value);
            max[axis] = max[axis].max(value);
        }
    }
    if mesh.vertices.is_empty() {
        ([0.0; 3], [0.0; 3])
    } else {
        (min, max)
    }
}

/// Signed volume of a closed mesh: positive when the faces point outward.
pub fn signed_volume(mesh: &TriangleMesh) -> f64 {
    mesh.triangles
        .iter()
        .map(|triangle| {
            let [a, b, c] = triangle.map(|index| mesh.vertices[index as usize].map(f64::from));
            (a[0] * (b[1] * c[2] - b[2] * c[1]) - a[1] * (b[0] * c[2] - b[2] * c[0])
                + a[2] * (b[0] * c[1] - b[1] * c[0]))
                / 6.0
        })
        .sum()
}

/// Turn a mesh the right way round, and say whether it had to.
///
/// An STL carries no statement about which side of a triangle is out, and
/// exporters do get it backwards: `tee.stl` here arrived entirely inside out —
/// the signed volume was minus the part's volume. CuraEngine slices such a mesh
/// without complaint, so nothing looked wrong, but every analysis that reads a
/// normal saw the part upside down: overhang detection found no underside,
/// auto-orient scored the top faces as the overhang, and the support proposer
/// came back with an empty plan. Repairing it at the door is what Cura and
/// Bambu do, and it is cheaper than teaching each analysis to distrust its own
/// winding.
/// One mesh per connected shell, biggest first.
///
/// A single file often holds several bodies that touch nothing at all: a card
/// of parts, a clip and its lid, a print-in-place hinge. They arrive as one
/// part, and a part can only be moved, painted or spooled whole — so the parts
/// inside it can never be given their own settings or their own bay.
///
/// Corners are welded by position first, because an STL repeats every corner
/// once per facet and a seam would otherwise read as a break. Two shells that
/// merely share a face are still two shells: nothing is merged that the file
/// did not join.
pub fn split_objects(mesh: &TriangleMesh) -> Vec<TriangleMesh> {
    let count = mesh.triangles.len();
    if count == 0 {
        return Vec::new();
    }

    // Weld corners to the grid they sit on, so a corner written twice at the
    // same place is one vertex.
    const GRID: f32 = 10_000.0; // 0.1 µm
    let mut ids: std::collections::HashMap<(i64, i64, i64), u32> = std::collections::HashMap::new();
    let mut corner = Vec::with_capacity(mesh.triangles.len() * 3);
    for triangle in &mesh.triangles {
        for index in triangle {
            let vertex = mesh
                .vertices
                .get(*index as usize)
                .copied()
                .unwrap_or([0.0; 3]);
            let key = (
                (vertex[0] * GRID).round() as i64,
                (vertex[1] * GRID).round() as i64,
                (vertex[2] * GRID).round() as i64,
            );
            let next = ids.len() as u32;
            corner.push(*ids.entry(key).or_insert(next));
        }
    }

    // Union-find over the welded corners: a triangle joins the corners it uses.
    let mut parent: Vec<u32> = (0..ids.len() as u32).collect();
    fn find(parent: &mut [u32], mut node: u32) -> u32 {
        while parent[node as usize] != node {
            let grand = parent[parent[node as usize] as usize];
            parent[node as usize] = grand;
            node = grand;
        }
        node
    }
    for triangle in corner.as_chunks::<3>().0 {
        let a = find(&mut parent, triangle[0]);
        let b = find(&mut parent, triangle[1]);
        let c = find(&mut parent, triangle[2]);
        let root = a;
        for other in [b, c] {
            let other = find(&mut parent, other);
            if other != root {
                parent[other as usize] = root;
            }
        }
    }

    // Group the triangles by the root of their first corner.
    let mut groups: std::collections::HashMap<u32, Vec<usize>> = std::collections::HashMap::new();
    for (index, triangle) in corner.as_chunks::<3>().0.iter().enumerate() {
        let root = find(&mut parent, triangle[0]);
        groups.entry(root).or_default().push(index);
    }

    let mut pieces: Vec<TriangleMesh> = Vec::with_capacity(groups.len());
    for (_, faces) in groups {
        let mut vertices: Vec<[f32; 3]> = Vec::new();
        let mut index_of: std::collections::HashMap<u32, u32> = std::collections::HashMap::new();
        let mut triangles: Vec<[u32; 3]> = Vec::with_capacity(faces.len());
        for face in faces {
            let source = mesh.triangles[face];
            let mut rebuilt = [0u32; 3];
            for (slot, original) in source.iter().enumerate() {
                let vertex = mesh
                    .vertices
                    .get(*original as usize)
                    .copied()
                    .unwrap_or([0.0; 3]);
                // Key on the welded corner, not on the file's index: a corner
                // written once per facet must land as one vertex here too.
                let welded = corner[face * 3 + slot];
                let next = vertices.len() as u32;
                let index = *index_of.entry(welded).or_insert(next);
                if index == next {
                    vertices.push(vertex);
                }
                rebuilt[slot] = index;
            }
            triangles.push(rebuilt);
        }
        pieces.push(TriangleMesh {
            vertices,
            triangles,
        });
    }

    pieces.sort_by_key(|piece| std::cmp::Reverse(piece.triangles.len()));
    pieces
}

pub fn right_way_round(mesh: &mut TriangleMesh) -> bool {
    if mesh.triangles.len() < 4 || signed_volume(mesh) >= 0.0 {
        return false;
    }
    for triangle in &mut mesh.triangles {
        triangle.swap(1, 2);
    }
    true
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
    // A negative scale is a mirror, and a mirror turns a solid inside out: the
    // corners move but every face now points inward, so an engine that fills by
    // winding sees a void and drops the walls. Swapping the last two corners of
    // each triangle puts the faces back on the outside.
    let inside_out = transform.scale[0] * transform.scale[1] * transform.scale[2] < 0.0;
    let triangles = if inside_out {
        mesh.triangles
            .iter()
            .map(|triangle| [triangle[0], triangle[2], triangle[1]])
            .collect()
    } else {
        mesh.triangles.clone()
    };
    TriangleMesh {
        vertices,
        triangles,
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

/// One object out of a file: what it is called, and its own triangles.
///
/// A 3MF is a package of objects and a build that places them, so importing one
/// as a single mesh throws away the parts it is made of — and a Bambu model
/// downloaded from MakerWorld is exactly that, several objects in one file.
#[derive(Clone, Debug)]
pub struct NamedObject {
    pub name: String,
    pub mesh: TriangleMesh,
}

/// Read every object a file holds.
///
/// A 3MF can hold several; an STL and a STEP hold one. Parts keep the positions
/// the file gave them, because a multi-part model is an assembly and moving its
/// parts to the middle of the plate would pull it apart.
pub fn load_objects(
    file_name: &str,
    bytes: &[u8],
    tolerance: &crate::cad::TessellationTolerance,
    progress: &mut dyn FnMut(f32, &str),
    cancel: &AtomicBool,
) -> Result<Vec<NamedObject>, Error> {
    if cancel.load(Ordering::Relaxed) {
        return Err(Error::Mesh("Import cancelled".into()));
    }
    let extension = Path::new(file_name)
        .extension()
        .and_then(|ext| ext.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    let extension = match extension.as_str() {
        "3mf" | "step" | "stp" | "stl" => extension,
        _ => sniff_format(bytes).to_string(),
    };
    let mut objects = match extension.as_str() {
        "3mf" => {
            progress(0.2, "reading 3MF");
            load_3mf_objects(bytes)?
                .into_iter()
                .map(|(name, mesh)| NamedObject { name, mesh })
                .collect::<Vec<_>>()
        }
        "step" | "stp" => {
            progress(0.2, "tessellating STEP");
            let imported = crate::cad::import_step(bytes, tolerance)?;
            vec![NamedObject {
                name: "part".to_string(),
                mesh: imported.mesh,
            }]
        }
        _ => {
            progress(0.2, "reading STL");
            vec![NamedObject {
                name: "part".to_string(),
                mesh: load_stl(bytes)?,
            }]
        }
    };
    for object in &mut objects {
        if cancel.load(Ordering::Relaxed) {
            return Err(Error::Mesh("Import cancelled".into()));
        }
        right_way_round(&mut object.mesh);
    }
    progress(1.0, "import finished");
    Ok(objects)
}

fn load_3mf(bytes: &[u8]) -> Result<TriangleMesh, Error> {
    let objects = load_3mf_objects(bytes)?;
    Ok(crate::exchange::merge(
        &objects
            .into_iter()
            .map(|(_, mesh)| mesh)
            .collect::<Vec<_>>(),
    ))
}

/// One mesh per object the build places, with the build's transforms applied.
fn load_3mf_objects(bytes: &[u8]) -> Result<Vec<(String, TriangleMesh)>, Error> {
    let mut archive = ZipArchive::new(Cursor::new(bytes))
        .map_err(|err| Error::Mesh(format!("3MF is not a zip archive: {err}")))?;
    let root_path = threemf_root_part(&mut archive)?;
    let mut parts: HashMap<String, ModelPart> = HashMap::new();
    let mut pending = vec![(root_path.clone(), 0usize)];
    while let Some((path, depth)) = pending.pop() {
        if parts.contains_key(&path) {
            continue;
        }
        let xml = read_zip_text(&mut archive, &path)?;
        let part = parse_3mf_part(&xml)?;
        for (_, object) in &part.objects {
            for component in &object.components {
                let Some(child) = component.path.as_deref() else {
                    continue;
                };
                let child = normalise_part_path(child);
                if !parts.contains_key(&child) && depth < THREEMF_MAX_PART_DEPTH {
                    pending.push((child, depth + 1));
                }
            }
        }
        parts.insert(path, part);
    }
    let root = parts
        .get(&root_path)
        .ok_or_else(|| Error::Mesh("3MF archive has no root model part".into()))?;
    let scale = unit_scale(&root.unit);
    let items = if root.build.is_empty() {
        root.objects
            .iter()
            .filter(|(_, object)| !object.triangles.is_empty() || !object.components.is_empty())
            .map(|(id, _)| (*id, identity_transform()))
            .take(1)
            .collect::<Vec<_>>()
    } else {
        root.build.clone()
    };
    if items.is_empty() {
        return Err(Error::Mesh(
            "3MF model has no mesh. Component-only files without a build item are not supported yet.".into(),
        ));
    }
    let mut objects = Vec::new();
    for (id, transform) in items {
        let mut vertices = Vec::new();
        let mut triangles = Vec::new();
        flatten_object(
            id,
            transform,
            root,
            &parts,
            &mut vertices,
            &mut triangles,
            0,
        )?;
        if triangles.is_empty() {
            continue;
        }
        if scale != 1.0 {
            for vertex in &mut vertices {
                vertex[0] *= scale as f32;
                vertex[1] *= scale as f32;
                vertex[2] *= scale as f32;
            }
        }
        let name = root
            .objects
            .iter()
            .find(|(object_id, _)| *object_id == id)
            .and_then(|(_, object)| object.name.clone())
            .unwrap_or_else(|| format!("object {id}"));
        objects.push((
            name,
            TriangleMesh {
                vertices,
                triangles,
            },
        ));
    }
    if objects.is_empty() {
        return Err(Error::Mesh(
            "3MF model did not contain any triangles".into(),
        ));
    }
    Ok(objects)
}

/// A 3MF package can hold several `.model` parts. Bambu Studio, Orca and
/// PrusaSlicer keep each object in `3D/Objects/*.model` and place it from a
/// component that carries a `p:path`, so reading a single XML document fails
/// on most files these slicers export. Parts are read on demand, bounded by
/// [`THREEMF_MAX_PART_DEPTH`].
const THREEMF_MAX_PART_DEPTH: usize = 4;

fn threemf_root_part(archive: &mut ZipArchive<Cursor<&[u8]>>) -> Result<String, Error> {
    let rels = read_zip_text(archive, "_rels/.rels").unwrap_or_default();
    for relationship in rels.split("<Relationship").skip(1) {
        if !relationship.contains("3dmodel") {
            continue;
        }
        let Some(target) = relationship
            .split("Target=\"")
            .nth(1)
            .and_then(|rest| rest.split('"').next())
        else {
            continue;
        };
        let path = normalise_part_path(target);
        if resolve_zip_name(archive, &path).is_some() {
            return Ok(path);
        }
    }
    if resolve_zip_name(archive, "3D/3dmodel.model").is_some() {
        return Ok("3D/3dmodel.model".to_string());
    }
    for index in 0..archive.len() {
        let file = archive
            .by_index(index)
            .map_err(|err| Error::Mesh(format!("3MF entry: {err}")))?;
        if file.name().to_ascii_lowercase().ends_with(".model") {
            return Ok(file.name().to_string());
        }
    }
    Err(Error::Mesh("3MF archive has no .model part".into()))
}

fn normalise_part_path(path: &str) -> String {
    path.trim_start_matches('/').replace('\\', "/")
}

/// Zip entry names are case sensitive, but packages written on Windows often
/// disagree with their own relationship targets about `3D/` against `3d/`.
fn resolve_zip_name(archive: &ZipArchive<Cursor<&[u8]>>, path: &str) -> Option<String> {
    if let Some(name) = archive.file_names().find(|name| *name == path) {
        return Some(name.to_string());
    }
    let wanted = path.to_ascii_lowercase();
    archive
        .file_names()
        .find(|name| name.to_ascii_lowercase() == wanted)
        .map(str::to_string)
}

fn read_zip_text(archive: &mut ZipArchive<Cursor<&[u8]>>, path: &str) -> Result<String, Error> {
    let name = resolve_zip_name(archive, path)
        .ok_or_else(|| Error::Mesh(format!("3MF part {path} is missing")))?;
    let mut file = archive
        .by_name(&name)
        .map_err(|err| Error::Mesh(format!("3MF part {path}: {err}")))?;
    let mut text = String::new();
    file.read_to_string(&mut text)
        .map_err(|err| Error::Mesh(format!("3MF part {path}: {err}")))?;
    Ok(text)
}

fn unit_scale(unit: &str) -> f64 {
    match unit {
        "inch" => 25.4,
        "meter" => 1000.0,
        "micron" => 0.001,
        _ => 1.0,
    }
}

struct RawObject {
    /// `<object name="...">`, when the file names its objects.
    name: Option<String>,
    vertices: Vec<[f32; 3]>,
    triangles: Vec<[u32; 3]>,
    components: Vec<RawComponent>,
}

struct RawComponent {
    id: u32,
    transform: [f64; 12],
    /// `p:path` of the part that holds this component, when it is not the
    /// part the component itself was declared in.
    path: Option<String>,
}

struct ModelPart {
    unit: String,
    objects: Vec<(u32, RawObject)>,
    build: Vec<(u32, [f64; 12])>,
}

fn parse_3mf_part(xml: &str) -> Result<ModelPart, Error> {
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

    Ok(ModelPart {
        unit,
        objects,
        build,
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
                    name: xml_attr(event, "name").filter(|n| !n.trim().is_empty()),
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
                object.components.push(RawComponent {
                    id,
                    transform: parse_transform(xml_attr(event, "transform").as_deref()),
                    path: xml_attr(event, "path"),
                });
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
    part: &ModelPart,
    parts: &HashMap<String, ModelPart>,
    vertices: &mut Vec<[f32; 3]>,
    triangles: &mut Vec<[u32; 3]>,
    depth: u8,
) -> Result<(), Error> {
    if depth > 6 {
        return Err(Error::Mesh("3MF component nesting is too deep".into()));
    }
    let Some((_, object)) = part.objects.iter().find(|(object_id, _)| *object_id == id) else {
        return Err(Error::Mesh(format!("3MF object {id} is missing")));
    };
    let base = vertices.len() as u32;
    for vertex in &object.vertices {
        vertices.push(apply_affine(*vertex, &transform));
    }
    for triangle in &object.triangles {
        triangles.push([base + triangle[0], base + triangle[1], base + triangle[2]]);
    }
    let components: Vec<(u32, [f64; 12], Option<String>)> = object
        .components
        .iter()
        .map(|component| (component.id, component.transform, component.path.clone()))
        .collect();
    for (child, child_transform, child_path) in components {
        let child_part = match child_path.as_deref() {
            Some(path) => {
                let path = normalise_part_path(path);
                parts
                    .get(&path)
                    .ok_or_else(|| Error::Mesh(format!("3MF part {path} is missing")))?
            }
            None => part,
        };
        flatten_object(
            child,
            compose(transform, child_transform),
            child_part,
            parts,
            vertices,
            triangles,
            depth + 1,
        )?;
    }
    Ok(())
}

fn apply_affine(vertex: [f32; 3], matrix: &[f64; 12]) -> [f32; 3] {
    // 3MF writes a 4x3 matrix in row-major order, so the twelve numbers are
    // m00 m01 m02 | m10 m11 m12 | m20 m21 m22 | m30 m31 m32 and the last three
    // are the translation. Reading it as 3x4 puts a translation into a
    // rotation slot, which threw Bambu projects hundreds of metres off plate.
    let x = vertex[0] as f64;
    let y = vertex[1] as f64;
    let z = vertex[2] as f64;
    [
        (matrix[0] * x + matrix[3] * y + matrix[6] * z + matrix[9]) as f32,
        (matrix[1] * x + matrix[4] * y + matrix[7] * z + matrix[10]) as f32,
        (matrix[2] * x + matrix[5] * y + matrix[8] * z + matrix[11]) as f32,
    ]
}

fn compose(parent: [f64; 12], child: [f64; 12]) -> [f64; 12] {
    let mut out = [0.0; 12];
    for row in 0..3 {
        for col in 0..3 {
            let mut sum = 0.0;
            for k in 0..3 {
                sum += parent[row * 3 + k] * child[k * 3 + col];
            }
            out[row * 3 + col] = sum;
        }
        let mut translate = parent[row * 3 + 3];
        for k in 0..3 {
            translate += parent[row * 3 + k] * child[k * 3 + 3];
        }
        out[row * 3 + 3] = translate;
    }
    out
}

fn identity_transform() -> [f64; 12] {
    [1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0]
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

    /// A closed box, which is the cheapest thing with a volume and a top face.
    fn cube_at(x: f32, y: f32) -> TriangleMesh {
        let mut vertices = Vec::new();
        for (dx, dy, dz) in [
            (0.0, 0.0, 0.0),
            (10.0, 0.0, 0.0),
            (10.0, 10.0, 0.0),
            (0.0, 10.0, 0.0),
            (0.0, 0.0, 10.0),
            (10.0, 0.0, 10.0),
            (10.0, 10.0, 10.0),
            (0.0, 10.0, 10.0),
        ] {
            vertices.push([x + dx, y + dy, dz]);
        }
        TriangleMesh {
            vertices,
            triangles: vec![
                [0, 2, 1],
                [0, 3, 2],
                [4, 5, 6],
                [4, 6, 7],
                [0, 1, 5],
                [0, 5, 4],
                [1, 2, 6],
                [1, 6, 5],
                [2, 3, 7],
                [2, 7, 6],
                [3, 0, 4],
                [3, 4, 7],
            ],
        }
    }

    fn merged(meshes: &[TriangleMesh]) -> TriangleMesh {
        let mut out = TriangleMesh {
            vertices: Vec::new(),
            triangles: Vec::new(),
        };
        for mesh in meshes {
            let offset = out.vertices.len() as u32;
            out.vertices.extend_from_slice(&mesh.vertices);
            for triangle in &mesh.triangles {
                out.triangles.push([
                    triangle[0] + offset,
                    triangle[1] + offset,
                    triangle[2] + offset,
                ]);
            }
        }
        out
    }

    #[test]
    fn a_file_of_two_bodies_splits_into_two_bodies() {
        // Two boxes that touch nothing, written as one file — a kit card, a
        // clip and its lid. Each corner is duplicated per facet, as an STL has
        // it, so the weld is under test as much as the walk.
        let both = merged(&[cube_at(0.0, 0.0), cube_at(40.0, 0.0)]);
        let pieces = split_objects(&both);
        assert_eq!(pieces.len(), 2, "two shells, two pieces");
        assert_eq!(pieces[0].triangles.len(), 12);
        assert_eq!(pieces[1].triangles.len(), 12);
        for piece in &pieces {
            assert_eq!(piece.vertices.len(), 8, "a box has eight corners, welded");
            let (min, max) = piece.bounds();
            assert!((max[0] - min[0] - 10.0).abs() < 1e-4);
        }
        // Two shells of equal size have no "biggest", so the check is that the
        // two places they stood are both still there.
        let mut xs: Vec<f32> = pieces.iter().map(|piece| piece.bounds().0[0]).collect();
        xs.sort_by(|left, right| left.partial_cmp(right).unwrap());
        assert!(
            (xs[0] - 0.0).abs() < 1e-3 && (xs[1] - 40.0).abs() < 1e-3,
            "still where they were: {xs:?}"
        );
    }

    #[test]
    fn one_body_is_left_alone() {
        let pieces = split_objects(&cube_at(0.0, 0.0));
        assert_eq!(pieces.len(), 1);
        assert_eq!(pieces[0].triangles.len(), 12);
        assert!((signed_volume(&pieces[0]) - 1000.0).abs() < 1.0);
    }

    fn unit_tetrahedron() -> TriangleMesh {
        TriangleMesh {
            vertices: vec![
                [0.0, 0.0, 0.0],
                [1.0, 0.0, 0.0],
                [0.0, 1.0, 0.0],
                [0.0, 0.0, 1.0],
            ],
            triangles: vec![[0, 2, 1], [0, 1, 3], [1, 2, 3], [2, 0, 3]],
        }
    }

    /// A closed tetrahedron with its lowest corner at `(x, 0, 0)`.
    fn tetrahedron_at(x: f32) -> TriangleMesh {
        let mut mesh = unit_tetrahedron();
        for vertex in &mut mesh.vertices {
            vertex[0] += x;
        }
        mesh
    }

    #[test]
    fn a_three_mf_of_two_objects_comes_back_as_two_parts() {
        let left = tetrahedron_at(-40.0);
        let right = tetrahedron_at(40.0);
        let bytes = crate::exchange::to_3mf(&[("left arm", &left), ("right arm", &right)]).unwrap();
        let objects = load_objects(
            "assembly.3mf",
            &bytes,
            &crate::cad::TessellationTolerance::standard(),
            &mut |_, _| {},
            &AtomicBool::new(false),
        )
        .unwrap();
        assert_eq!(objects.len(), 2, "two build items, two parts");
        assert_eq!(objects[0].name, "left arm");
        assert_eq!(objects[1].name, "right arm");
        assert_eq!(objects[0].mesh.triangles.len(), left.triangles.len());
        // The parts kept the places the file gave them: an assembly is not two
        // parts stacked in the middle of the plate.
        let centre = |mesh: &TriangleMesh| {
            mesh.vertices.iter().map(|v| v[0]).sum::<f32>() / mesh.vertices.len() as f32
        };
        assert!(centre(&objects[0].mesh) < -20.0, "left part stayed left");
        assert!(centre(&objects[1].mesh) > 20.0, "right part stayed right");
    }

    #[test]
    fn right_way_round_flips_an_inside_out_solid_and_leaves_a_sound_one() {
        let sound = unit_tetrahedron();
        let mut inverted = sound.clone();
        for triangle in &mut inverted.triangles {
            triangle.swap(1, 2);
        }
        assert!(signed_volume(&sound) > 0.0, "the fixture should be outward");
        assert!(signed_volume(&inverted) < 0.0, "the copy should be inward");

        let mut flipped = inverted.clone();
        assert!(
            right_way_round(&mut flipped),
            "an inward mesh must be repaired"
        );
        assert!(
            (signed_volume(&flipped) - signed_volume(&sound)).abs() < 1e-12,
            "repairing must restore the sound volume, got {} against {}",
            signed_volume(&flipped),
            signed_volume(&sound)
        );
        assert_eq!(flipped.vertices.len(), sound.vertices.len());
        assert_eq!(flipped.triangles.len(), sound.triangles.len());

        let mut untouched = sound.clone();
        assert!(
            !right_way_round(&mut untouched),
            "a sound mesh must be left alone"
        );
        assert_eq!(untouched.triangles, sound.triangles);
    }

    /// A binary STL carrying `mesh` exactly as it stands, winding included.
    fn binary_stl_bytes(mesh: &TriangleMesh) -> Vec<u8> {
        let mut bytes = vec![0u8; 80];
        bytes.extend_from_slice(&(mesh.triangles.len() as u32).to_le_bytes());
        for triangle in &mesh.triangles {
            for _ in 0..3 {
                bytes.extend_from_slice(&0f32.to_le_bytes());
            }
            for index in triangle {
                for axis in mesh.vertices[*index as usize] {
                    bytes.extend_from_slice(&axis.to_le_bytes());
                }
            }
            bytes.extend_from_slice(&0u16.to_le_bytes());
        }
        bytes
    }

    #[test]
    fn an_inside_out_stl_is_turned_round_on_the_way_in() {
        let mut inverted = unit_tetrahedron();
        for triangle in &mut inverted.triangles {
            triangle.swap(1, 2);
        }
        let bytes = binary_stl_bytes(&inverted);
        let imported = load_named_bytes(
            "inside-out.stl",
            &bytes,
            &crate::cad::TessellationTolerance::standard(),
            &mut |_, _| {},
            &AtomicBool::new(false),
        )
        .expect("the inverted mesh should still import");
        assert!(
            signed_volume(&imported.mesh) > 0.0,
            "the loader must hand back an outward mesh, got {}",
            signed_volume(&imported.mesh)
        );
    }

    #[test]
    fn a_mirrored_bake_keeps_the_faces_on_the_outside() {
        let mesh = unit_tetrahedron();
        assert!((signed_volume(&mesh) - 1.0 / 6.0).abs() < 1e-9);

        let mirrored = bake(
            &mesh,
            &Transform {
                translation: [0.0, 0.0, 0.0],
                rotation_deg: [0.0, 0.0, 0.0],
                scale: [-1.0, 1.0, 1.0],
            },
        );
        assert_eq!(mirrored.triangles.len(), 4);
        assert!(
            signed_volume(&mirrored) > 0.0,
            "a mirrored solid must not end up inside out"
        );
        assert!((signed_volume(&mirrored).abs() - 1.0 / 6.0).abs() < 1e-9);
    }

    #[test]
    fn a_three_mf_imports_even_when_its_name_has_no_extension() {
        let mesh = TriangleMesh {
            vertices: vec![[0.0, 0.0, 0.0], [10.0, 0.0, 0.0], [0.0, 10.0, 0.0]],
            triangles: vec![[0, 1, 2]],
        };
        let bytes = crate::exchange::to_3mf(&[("part", &mesh)]).unwrap();
        let tolerance = crate::cad::TessellationTolerance::standard();
        let imported = load_named_bytes(
            "Arm.3mf copy",
            &bytes,
            &tolerance,
            &mut |_, _| {},
            &AtomicBool::new(false),
        )
        .unwrap();
        assert_eq!(imported.mesh.triangles.len(), 1);
        assert_eq!(imported.report.source, "3mf");
    }

    #[test]
    fn reads_components_that_live_in_separate_parts() {
        let root = r#"<?xml version="1.0"?>
<model unit="millimeter" xmlns="http://schemas.microsoft.com/3dmanufacturing/core/2015/02">
  <resources>
    <object id="2" type="model">
      <components>
        <component objectid="1" p:path="/3D/Objects/object_1.model" transform="1 0 0 0 1 0 0 0 1 5 0 0"/>
        <component objectid="3" p:path="/3D/Objects/object_1.model" transform="1 0 0 0 1 0 0 0 1 15 0 0"/>
      </components>
    </object>
  </resources>
  <build>
    <item objectid="2"/>
  </build>
</model>"#;
        let part = r#"<?xml version="1.0"?>
<model unit="millimeter" xmlns="http://schemas.microsoft.com/3dmanufacturing/core/2015/02">
  <resources>
    <object id="1" type="model">
      <mesh>
        <vertices>
          <vertex x="0" y="0" z="0"/>
          <vertex x="1" y="0" z="0"/>
          <vertex x="0" y="1" z="0"/>
        </vertices>
        <triangles>
          <triangle v1="0" v2="1" v3="2"/>
        </triangles>
      </mesh>
    </object>
    <object id="3" type="model">
      <mesh>
        <vertices>
          <vertex x="0" y="0" z="0"/>
          <vertex x="2" y="0" z="0"/>
          <vertex x="0" y="2" z="0"/>
        </vertices>
        <triangles>
          <triangle v1="0" v2="1" v3="2"/>
        </triangles>
      </mesh>
    </object>
  </resources>
</model>"#;
        let rels = r#"<?xml version="1.0"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">
  <Relationship Target="/3D/3dmodel.model" Id="rel0" Type="http://schemas.microsoft.com/3dmanufacturing/2013/01/3dmodel"/>
</Relationships>"#;
        let mut cursor = std::io::Cursor::new(Vec::new());
        {
            let mut writer = zip::ZipWriter::new(&mut cursor);
            for (name, body) in [
                ("_rels/.rels", rels),
                ("3D/3dmodel.model", root),
                ("3D/Objects/object_1.model", part),
            ] {
                writer
                    .start_file(name, SimpleFileOptions::default())
                    .unwrap();
                writer.write_all(body.as_bytes()).unwrap();
            }
            writer.finish().unwrap();
        }
        let mesh = load_3mf(&cursor.into_inner()).unwrap();
        assert_eq!(mesh.triangles.len(), 2);
        // Both components resolve object ids inside the same part, 5 mm and
        // 15 mm along X, so the widest vertex sits at 15 + 2.
        let max_x = mesh.vertices.iter().map(|v| v[0]).fold(f32::MIN, f32::max);
        assert!((max_x - 17.0).abs() < 1e-3, "max x was {max_x}");
    }

    #[test]
    fn a_capped_preview_keeps_contiguous_runs() {
        // Lone sampled triangles read as broken geometry, so a capped preview
        // must keep neighbouring facets together.
        let mut vertices = Vec::new();
        for step in 0..9_600u32 {
            vertices.push([step as f32, 0.0, 0.0]);
        }
        let triangles = (0..9_600u32)
            .map(|step| [step, step + 1, step + 2])
            .collect::<Vec<_>>();
        let mesh = TriangleMesh {
            vertices,
            triangles,
        };
        let preview = mesh.preview_positions(960);
        assert_eq!(preview.len() / 9, 960, "preview kept the whole budget");
        // The first run is contiguous: triangle 0 then triangle 1.
        assert!((preview[0] - 0.0).abs() < 1e-6);
        assert!(
            (preview[9] - 1.0).abs() < 1e-6,
            "second kept triangle follows the first"
        );
        assert!((preview[18] - 2.0).abs() < 1e-6);
    }

    #[test]
    fn applies_a_build_item_transform_in_the_3mf_layout() {
        // 90 degrees about Z then a (10, 20, 30) offset. 3MF writes the matrix
        // as four rows of three, so the offset is the last three numbers; a
        // 3x4 reading would place (-1) in the x translation instead.
        let xml = r#"<?xml version="1.0"?>
<model unit="millimeter" xmlns="http://schemas.microsoft.com/3dmanufacturing/core/2015/02">
  <resources>
    <object id="1" type="model">
      <mesh>
        <vertices>
          <vertex x="1" y="0" z="0"/>
          <vertex x="0" y="1" z="0"/>
          <vertex x="0" y="0" z="1"/>
        </vertices>
        <triangles>
          <triangle v1="0" v2="1" v3="2"/>
        </triangles>
      </mesh>
    </object>
  </resources>
  <build>
    <item objectid="1" transform="0 1 0 -1 0 0 0 0 1 10 20 30"/>
  </build>
</model>"#;
        let mut buffer = Cursor::new(Vec::new());
        {
            let mut writer = zip::ZipWriter::new(&mut buffer);
            writer
                .start_file("3D/3dmodel.model", SimpleFileOptions::default())
                .unwrap();
            writer.write_all(xml.as_bytes()).unwrap();
            writer.finish().unwrap();
        }
        let mesh = load_3mf(&buffer.into_inner()).unwrap();
        assert_eq!(mesh.triangles.len(), 1);
        let rotated = mesh.vertices[0];
        assert!((rotated[0] - 10.0).abs() < 1e-3, "x was {}", rotated[0]);
        assert!((rotated[1] - 21.0).abs() < 1e-3, "y was {}", rotated[1]);
        assert!((rotated[2] - 30.0).abs() < 1e-3, "z was {}", rotated[2]);
    }

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
    <item objectid="1" transform="1 0 0 0 1 0 0 0 1 10 0 0"/>
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
    fn binary_stl_one_and_five_million_triangles() {
        for count in [1_000_000usize, 5_000_000] {
            let path = std::env::temp_dir().join(format!("shale-tris-{count}.stl"));
            write_flat_binary_stl(&path, count).unwrap();
            let before = resident_mb();
            let started = std::time::Instant::now();
            let mesh =
                super::load_stl_path(&path, &mut |_, _| {}, &AtomicBool::new(false)).unwrap();
            let elapsed = started.elapsed();
            let after = resident_mb();
            let mesh_mb =
                (mesh.vertices.len() * 12 + mesh.triangles.len() * 12) as f64 / 1_000_000.0;
            eprintln!(
                "{count} triangles: {:.2}s, mesh {mesh_mb:.1} MB, rss {before:.0} -> {after:.0} MB",
                elapsed.as_secs_f64()
            );
            assert_eq!(mesh.triangles.len(), count);
            assert!(
                elapsed.as_secs() < 90,
                "{count} triangles took {}s",
                elapsed.as_secs()
            );
            let _ = std::fs::remove_file(&path);
        }
    }

    fn write_flat_binary_stl(path: &Path, count: usize) -> std::io::Result<()> {
        use std::io::Write;
        let mut file = std::fs::File::create(path)?;
        file.write_all(&[0u8; 80])?;
        file.write_all(&(count as u32).to_le_bytes())?;
        let mut record = [0u8; 50];
        record[12..16].copy_from_slice(&1.0f32.to_le_bytes());
        record[24..28].copy_from_slice(&1.0f32.to_le_bytes());
        record[32..36].copy_from_slice(&1.0f32.to_le_bytes());
        for _ in 0..count {
            file.write_all(&record)?;
        }
        Ok(())
    }

    fn resident_mb() -> f64 {
        let Ok(text) = std::fs::read_to_string("/proc/self/status") else {
            return 0.0;
        };
        for line in text.lines() {
            if let Some(rest) = line.strip_prefix("VmRSS:") {
                let kb: f64 = rest
                    .split_whitespace()
                    .next()
                    .unwrap_or("0")
                    .parse()
                    .unwrap_or(0.0);
                return kb / 1024.0;
            }
        }
        0.0
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
