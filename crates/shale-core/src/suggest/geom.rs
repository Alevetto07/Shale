//! Cheap measurements on a triangle mesh.
//!
//! Every rule that looks at a part starts here. The grids are coarse on purpose:
//! a suggestion after an import has to come back before the next click.

use crate::mesh::TriangleMesh;

/// How close to the bed a triangle still counts as first-layer contact.
pub const CONTACT_BAND_MM: f64 = 0.6;

#[derive(Clone, Debug)]
pub struct Contact {
    pub area_mm2: f64,
    pub islands_mm2: Vec<f64>,
    pub height_mm: f64,
    pub footprint_mm2: f64,
    pub base_x_mm: f64,
    pub base_y_mm: f64,
    /// Centre of mass height above the lowest point.
    pub com_above_base_mm: f64,
}

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
        return ([0.0; 3], [0.0; 3]);
    }
    (min, max)
}

/// First-layer contact, from triangles that sit on the bottom of the part.
pub fn contact(mesh: &TriangleMesh) -> Contact {
    let (min, max) = bounds(mesh);
    let height = (max[2] - min[2]).max(0.0);
    let base_x = (max[0] - min[0]).max(0.0);
    let base_y = (max[1] - min[1]).max(0.0);
    let footprint = base_x * base_y;
    let com = centre_of_mass(mesh);
    let com_above = if com[2].is_finite() {
        (com[2] - min[2]).max(0.0)
    } else {
        height * 0.5
    };
    let (area, islands) = contact_islands(mesh, min[2]);
    Contact {
        area_mm2: area,
        islands_mm2: islands,
        height_mm: height,
        footprint_mm2: footprint,
        base_x_mm: base_x,
        base_y_mm: base_y,
        com_above_base_mm: com_above,
    }
}

fn contact_islands(mesh: &TriangleMesh, bed_z: f64) -> (f64, Vec<f64>) {
    let mut faces: Vec<[[f64; 2]; 3]> = Vec::new();
    let mut span_min = [f64::INFINITY; 2];
    let mut span_max = [f64::NEG_INFINITY; 2];
    for triangle in &mesh.triangles {
        let pts: Vec<[f64; 3]> = triangle
            .iter()
            .filter_map(|index| mesh.vertices.get(*index as usize))
            .map(|v| [v[0] as f64, v[1] as f64, v[2] as f64])
            .collect();
        if pts.len() != 3 {
            continue;
        }
        let top = pts[0][2].max(pts[1][2]).max(pts[2][2]);
        if top > bed_z + CONTACT_BAND_MM {
            continue;
        }
        for point in &pts {
            span_min[0] = span_min[0].min(point[0]);
            span_min[1] = span_min[1].min(point[1]);
            span_max[0] = span_max[0].max(point[0]);
            span_max[1] = span_max[1].max(point[1]);
        }
        faces.push([
            [pts[0][0], pts[0][1]],
            [pts[1][0], pts[1][1]],
            [pts[2][0], pts[2][1]],
        ]);
    }
    if faces.is_empty() || !span_min[0].is_finite() {
        return (0.0, Vec::new());
    }
    let span_x = (span_max[0] - span_min[0]).max(0.5);
    let span_y = (span_max[1] - span_min[1]).max(0.5);
    let mut cell = 0.5;
    let cells = (span_x / cell) * (span_y / cell);
    if cells > 80_000.0 {
        cell = ((span_x * span_y) / 80_000.0).sqrt().max(0.5);
    }
    let width = ((span_x / cell).ceil() as usize).clamp(1, 400) + 1;
    let height = ((span_y / cell).ceil() as usize).clamp(1, 400) + 1;
    let mut grid = vec![0_u8; width * height];
    for face in &faces {
        fill_triangle(&mut grid, width, height, span_min, cell, face);
    }
    let mut seen = vec![false; grid.len()];
    let mut islands = Vec::new();
    let mut filled = 0_usize;
    for index in 0..grid.len() {
        if grid[index] == 0 || seen[index] {
            continue;
        }
        let mut count = 0_usize;
        let mut stack = vec![index];
        seen[index] = true;
        while let Some(cell_index) = stack.pop() {
            count += 1;
            let x = cell_index % width;
            let y = cell_index / width;
            for (nx, ny) in [
                (x.wrapping_sub(1), y),
                (x + 1, y),
                (x, y.wrapping_sub(1)),
                (x, y + 1),
            ] {
                if nx >= width || ny >= height {
                    continue;
                }
                let next = ny * width + nx;
                if grid[next] == 1 && !seen[next] {
                    seen[next] = true;
                    stack.push(next);
                }
            }
        }
        filled += count;
        islands.push(count as f64 * cell * cell);
    }
    islands.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    (filled as f64 * cell * cell, islands)
}

fn fill_triangle(
    grid: &mut [u8],
    width: usize,
    height: usize,
    origin: [f64; 2],
    cell: f64,
    face: &[[f64; 2]; 3],
) {
    let min_x = face[0][0].min(face[1][0]).min(face[2][0]);
    let max_x = face[0][0].max(face[1][0]).max(face[2][0]);
    let min_y = face[0][1].min(face[1][1]).min(face[2][1]);
    let max_y = face[0][1].max(face[1][1]).max(face[2][1]);
    let x0 = (((min_x - origin[0]) / cell).floor() as isize).max(0) as usize;
    let x1 = (((max_x - origin[0]) / cell).ceil() as usize).min(width.saturating_sub(1));
    let y0 = (((min_y - origin[1]) / cell).floor() as isize).max(0) as usize;
    let y1 = (((max_y - origin[1]) / cell).ceil() as usize).min(height.saturating_sub(1));
    for y in y0..=y1 {
        for x in x0..=x1 {
            let point = [
                origin[0] + (x as f64 + 0.5) * cell,
                origin[1] + (y as f64 + 0.5) * cell,
            ];
            if point_in_triangle(point, face) {
                grid[y * width + x] = 1;
            }
        }
    }
}

fn point_in_triangle(p: [f64; 2], face: &[[f64; 2]; 3]) -> bool {
    let sign = |a: [f64; 2], b: [f64; 2], c: [f64; 2]| {
        (a[0] - c[0]) * (b[1] - c[1]) - (b[0] - c[0]) * (a[1] - c[1])
    };
    let d1 = sign(p, face[0], face[1]);
    let d2 = sign(p, face[1], face[2]);
    let d3 = sign(p, face[2], face[0]);
    let neg = d1 < -1e-9 || d2 < -1e-9 || d3 < -1e-9;
    let pos = d1 > 1e-9 || d2 > 1e-9 || d3 > 1e-9;
    !(neg && pos)
}

pub fn centre_of_mass(mesh: &TriangleMesh) -> [f64; 3] {
    let mut volume = 0.0;
    let mut moment = [0.0; 3];
    for triangle in &mesh.triangles {
        let Some(a) = vertex(mesh, triangle[0]) else {
            continue;
        };
        let Some(b) = vertex(mesh, triangle[1]) else {
            continue;
        };
        let Some(c) = vertex(mesh, triangle[2]) else {
            continue;
        };
        let tet = scalar_triple(a, b, c) / 6.0;
        volume += tet;
        for axis in 0..3 {
            moment[axis] += tet * (a[axis] + b[axis] + c[axis]) / 4.0;
        }
    }
    if volume.abs() < 1e-8 {
        if mesh.vertices.is_empty() {
            return [0.0; 3];
        }
        let mut sum = [0.0; 3];
        for vertex in &mesh.vertices {
            for axis in 0..3 {
                sum[axis] += vertex[axis] as f64;
            }
        }
        let n = mesh.vertices.len() as f64;
        return [sum[0] / n, sum[1] / n, sum[2] / n];
    }
    [moment[0] / volume, moment[1] / volume, moment[2] / volume]
}

fn vertex(mesh: &TriangleMesh, index: u32) -> Option<[f64; 3]> {
    mesh.vertices
        .get(index as usize)
        .map(|v| [v[0] as f64, v[1] as f64, v[2] as f64])
}

fn scalar_triple(a: [f64; 3], b: [f64; 3], c: [f64; 3]) -> f64 {
    a[0] * (b[1] * c[2] - b[2] * c[1]) - a[1] * (b[0] * c[2] - b[2] * c[0])
        + a[2] * (b[0] * c[1] - b[1] * c[0])
}

/// Thinnest wall found by casting inward from vertical faces, in millimetres.
///
/// `None` when the mesh has no vertical face to measure. Faces are sampled, not
/// all of them: a suggestion does not need a micrometre.
pub fn thinnest_wall_mm(mesh: &TriangleMesh) -> Option<f64> {
    let mut samples = Vec::new();
    for triangle in &mesh.triangles {
        let Some(a) = vertex(mesh, triangle[0]) else {
            continue;
        };
        let Some(b) = vertex(mesh, triangle[1]) else {
            continue;
        };
        let Some(c) = vertex(mesh, triangle[2]) else {
            continue;
        };
        let u = sub(b, a);
        let v = sub(c, a);
        let normal = cross(u, v);
        let length = norm(normal);
        if length < 1e-8 {
            continue;
        }
        let nz = (normal[2] / length).abs();
        if nz > 0.35 {
            continue;
        }
        let area = length * 0.5;
        if area < 0.5 {
            continue;
        }
        let centroid = [
            (a[0] + b[0] + c[0]) / 3.0,
            (a[1] + b[1] + c[1]) / 3.0,
            (a[2] + b[2] + c[2]) / 3.0,
        ];
        let inward = [
            -normal[0] / length,
            -normal[1] / length,
            -normal[2] / length,
        ];
        samples.push((area, centroid, inward));
    }
    if samples.is_empty() {
        return None;
    }
    samples.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
    let step = (samples.len() / 120).max(1);
    let mut thinnest = f64::INFINITY;
    for (index, (_, centroid, inward)) in samples.iter().enumerate() {
        if index % step != 0 {
            continue;
        }
        let origin = [
            centroid[0] + inward[0] * 0.05,
            centroid[1] + inward[1] * 0.05,
            centroid[2] + inward[2] * 0.05,
        ];
        if let Some(distance) = nearest_hit(mesh, origin, *inward) {
            let thickness = distance + 0.05;
            if thickness > 0.02 {
                thinnest = thinnest.min(thickness);
            }
        }
    }
    thinnest.is_finite().then_some(thinnest)
}

/// A hole in a mid-height slice, and the thinnest wall around it.
pub struct Hole {
    pub area_mm2: f64,
    pub wall_mm: f64,
}

pub fn enclosed_hole(mesh: &TriangleMesh) -> Option<Hole> {
    if mesh.triangles.len() > 20_000 {
        return None;
    }
    let (min, max) = bounds(mesh);
    let z = (min[2] + max[2]) * 0.5;
    let span_x = (max[0] - min[0]).max(0.5);
    let span_y = (max[1] - min[1]).max(0.5);
    let cells = 36_usize;
    let cell = (span_x.max(span_y) / cells as f64).max(0.35);
    let width = ((span_x / cell).ceil() as usize).clamp(4, cells);
    let height = ((span_y / cell).ceil() as usize).clamp(4, cells);
    let mut inside = vec![false; width * height];
    for y in 0..height {
        for x in 0..width {
            let origin = [
                min[0] + (x as f64 + 0.5) * cell,
                min[1] + (y as f64 + 0.5) * cell,
                z,
            ];
            inside[y * width + x] = crossings(mesh, origin) % 2 == 1;
        }
    }
    let mut exterior = vec![false; inside.len()];
    let mut stack = Vec::new();
    for index in 0..inside.len() {
        let x = index % width;
        let y = index / width;
        let border = x == 0 || y == 0 || x + 1 == width || y + 1 == height;
        if border && !inside[index] {
            exterior[index] = true;
            stack.push(index);
        }
    }
    while let Some(index) = stack.pop() {
        let x = index % width;
        let y = index / width;
        for (nx, ny) in neighbours(x, y, width, height) {
            let next = ny * width + nx;
            if !inside[next] && !exterior[next] {
                exterior[next] = true;
                stack.push(next);
            }
        }
    }
    let mut hole_cells = Vec::new();
    for (index, _) in inside.iter().enumerate() {
        if !inside[index] && !exterior[index] {
            hole_cells.push(index);
        }
    }
    if hole_cells.is_empty() {
        return None;
    }
    let mut wall = f64::INFINITY;
    for index in &hole_cells {
        let x = index % width;
        let y = index / width;
        for (other, is_exterior) in exterior.iter().enumerate() {
            if !is_exterior {
                continue;
            }
            let ox = other % width;
            let oy = other / width;
            let dx = x as f64 - ox as f64;
            let dy = y as f64 - oy as f64;
            wall = wall.min((dx * dx + dy * dy).sqrt() * cell);
        }
    }
    Some(Hole {
        area_mm2: hole_cells.len() as f64 * cell * cell,
        wall_mm: if wall.is_finite() { wall } else { 0.0 },
    })
}

fn neighbours(x: usize, y: usize, width: usize, height: usize) -> Vec<(usize, usize)> {
    let mut out = Vec::with_capacity(4);
    if x > 0 {
        out.push((x - 1, y));
    }
    if x + 1 < width {
        out.push((x + 1, y));
    }
    if y > 0 {
        out.push((x, y - 1));
    }
    if y + 1 < height {
        out.push((x, y + 1));
    }
    out
}

fn crossings(mesh: &TriangleMesh, origin: [f64; 3]) -> u32 {
    let dir = [1.0, 0.02, 0.01];
    let mut hits = 0_u32;
    for triangle in &mesh.triangles {
        let Some(a) = vertex(mesh, triangle[0]) else {
            continue;
        };
        let Some(b) = vertex(mesh, triangle[1]) else {
            continue;
        };
        let Some(c) = vertex(mesh, triangle[2]) else {
            continue;
        };
        if ray_hit(origin, dir, a, b, c).is_some() {
            hits += 1;
        }
    }
    hits
}

fn nearest_hit(mesh: &TriangleMesh, origin: [f64; 3], dir: [f64; 3]) -> Option<f64> {
    let mut best = f64::INFINITY;
    for triangle in &mesh.triangles {
        let Some(a) = vertex(mesh, triangle[0]) else {
            continue;
        };
        let Some(b) = vertex(mesh, triangle[1]) else {
            continue;
        };
        let Some(c) = vertex(mesh, triangle[2]) else {
            continue;
        };
        if let Some(distance) = ray_hit(origin, dir, a, b, c) {
            best = best.min(distance);
        }
    }
    best.is_finite().then_some(best)
}

fn ray_hit(origin: [f64; 3], dir: [f64; 3], a: [f64; 3], b: [f64; 3], c: [f64; 3]) -> Option<f64> {
    let edge1 = sub(b, a);
    let edge2 = sub(c, a);
    let p = cross(dir, edge2);
    let det = dot(edge1, p);
    if det.abs() < 1e-8 {
        return None;
    }
    let inv = 1.0 / det;
    let tvec = sub(origin, a);
    let u = dot(tvec, p) * inv;
    if !(0.0..=1.0).contains(&u) {
        return None;
    }
    let q = cross(tvec, edge1);
    let v = dot(dir, q) * inv;
    if v < 0.0 || u + v > 1.0 {
        return None;
    }
    let t = dot(edge2, q) * inv;
    (t > 1e-4).then_some(t)
}

/// How much the top of the part rises, and the steepest top face.
pub struct TopShape {
    pub spread_mm: f64,
    pub max_slope_deg: f64,
    pub sloped_area_mm2: f64,
}

pub fn top_shape(mesh: &TriangleMesh) -> TopShape {
    let (min, max) = bounds(mesh);
    let mut spread = 0.0_f64;
    let mut slope = 0.0_f64;
    let mut area = 0.0;
    let top_floor = max[2] - (max[2] - min[2]).max(0.1) * 0.35;
    for triangle in &mesh.triangles {
        let Some(a) = vertex(mesh, triangle[0]) else {
            continue;
        };
        let Some(b) = vertex(mesh, triangle[1]) else {
            continue;
        };
        let Some(c) = vertex(mesh, triangle[2]) else {
            continue;
        };
        let average_z = (a[2] + b[2] + c[2]) / 3.0;
        if average_z < top_floor {
            continue;
        }
        let normal = cross(sub(b, a), sub(c, a));
        let length = norm(normal);
        if length < 1e-8 || normal[2] <= 0.0 {
            continue;
        }
        let face = length * 0.5;
        let tilt = (normal[2] / length).clamp(-1.0, 1.0).acos().to_degrees();
        if (1.5..12.0).contains(&tilt) {
            area += face;
            slope = slope.max(tilt);
            let z0 = a[2].min(b[2]).min(c[2]);
            let z1 = a[2].max(b[2]).max(c[2]);
            spread = spread.max(z1 - z0);
        }
    }
    // Spread across the whole top, not only one triangle.
    let mut low = f64::INFINITY;
    let mut high = f64::NEG_INFINITY;
    for vertex in &mesh.vertices {
        let z = vertex[2] as f64;
        if z >= top_floor {
            low = low.min(z);
            high = high.max(z);
        }
    }
    if low.is_finite() {
        spread = spread.max(high - low);
    }
    TopShape {
        spread_mm: spread,
        max_slope_deg: slope,
        sloped_area_mm2: area,
    }
}

#[cfg(test)]
pub fn append_box(mesh: &mut TriangleMesh, min: [f32; 3], max: [f32; 3]) {
    let base = mesh.vertices.len() as u32;
    let [x0, y0, z0] = min;
    let [x1, y1, z1] = max;
    mesh.vertices.extend_from_slice(&[
        [x0, y0, z0],
        [x1, y0, z0],
        [x1, y1, z0],
        [x0, y1, z0],
        [x0, y0, z1],
        [x1, y0, z1],
        [x1, y1, z1],
        [x0, y1, z1],
    ]);
    let corners: [[u32; 3]; 12] = [
        [0, 2, 1],
        [0, 3, 2],
        [4, 5, 6],
        [4, 6, 7],
        [0, 1, 5],
        [0, 5, 4],
        [3, 6, 2],
        [3, 7, 6],
        [0, 4, 7],
        [0, 7, 3],
        [1, 2, 6],
        [1, 6, 5],
    ];
    for corner in corners {
        mesh.triangles
            .push([corner[0] + base, corner[1] + base, corner[2] + base]);
    }
}

#[cfg(test)]
pub fn box_mesh(min: [f32; 3], max: [f32; 3]) -> TriangleMesh {
    let mut mesh = TriangleMesh {
        vertices: Vec::new(),
        triangles: Vec::new(),
    };
    append_box(&mut mesh, min, max);
    mesh
}

fn sub(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

fn cross(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

fn dot(a: [f64; 3], b: [f64; 3]) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

fn norm(a: [f64; 3]) -> f64 {
    dot(a, a).sqrt()
}
