//! An isometric picture of the parts on a plate.
//!
//! Bambu Studio stores `Metadata/plate_N.png` and `Metadata/plate_N_small.png`
//! beside the G-code. Those files are a view of the mesh, not a flat swatch.
//! This draws that view with a z-buffer. Nothing here talks to a GPU.

/// One solid on the plate, already in millimetres, after its transform.
#[derive(Clone, Debug, PartialEq)]
pub struct PreviewSolid {
    pub triangles: Vec<[[f32; 3]; 3]>,
    pub colour: [u8; 3],
}

/// RGBA bytes, tightly packed, top row first. `size` is clamped to 8–512.
pub fn isometric_rgba(solids: &[PreviewSolid], size: u32) -> Vec<u8> {
    let size = size.clamp(8, 512) as usize;
    let mut pixels = vec![0u8; size * size * 4];
    for pixel in pixels.as_chunks_mut::<4>().0 {
        pixel.copy_from_slice(&[18, 20, 24, 255]);
    }
    let inset = size / 8;
    if inset * 2 < size {
        for y in inset..size - inset {
            for x in inset..size - inset {
                let offset = (y * size + x) * 4;
                pixels[offset..offset + 4].copy_from_slice(&[46, 49, 56, 255]);
            }
        }
    }

    struct Drawn {
        points: [[f32; 3]; 3],
        colour: [u8; 3],
    }
    let mut drawn = Vec::new();
    for solid in solids {
        let step = solid.triangles.len().div_ceil(4_000).max(1);
        for (index, triangle) in solid.triangles.iter().enumerate() {
            if !index.is_multiple_of(step) {
                continue;
            }
            if triangle.iter().any(|vertex| vertex.iter().any(|axis| !axis.is_finite())) {
                continue;
            }
            let normal = face_normal(triangle);
            if normal == [0.0, 0.0, 0.0] {
                continue;
            }
            let light = shade(normal);
            let colour = [
                scale_channel(solid.colour[0], light),
                scale_channel(solid.colour[1], light),
                scale_channel(solid.colour[2], light),
            ];
            let points = [
                project(triangle[0]),
                project(triangle[1]),
                project(triangle[2]),
            ];
            drawn.push(Drawn { points, colour });
        }
    }
    if drawn.is_empty() {
        return pixels;
    }

    // Frame the bed as well as the part, so a part on the left is not drawn
    // in the same place as a part on the right. 96 mm is enough of a plate
    // for the part to read, and the frame grows if the mesh is larger.
    let mut min_x = f32::MAX;
    let mut max_x = f32::MIN;
    let mut min_y = f32::MAX;
    let mut max_y = f32::MIN;
    let mut include = |point: [f32; 3]| {
        min_x = min_x.min(point[0]);
        max_x = max_x.max(point[0]);
        min_y = min_y.min(point[1]);
        max_y = max_y.max(point[1]);
    };
    for corner in [
        [0.0, 0.0, 0.0],
        [96.0, 0.0, 0.0],
        [96.0, 96.0, 0.0],
        [0.0, 96.0, 0.0],
    ] {
        include(project(corner));
    }
    for triangle in &drawn {
        for point in &triangle.points {
            include(*point);
        }
    }
    let span_x = (max_x - min_x).max(1.0e-3);
    let span_y = (max_y - min_y).max(1.0e-3);
    let margin = size as f32 * 0.12;
    let usable = (size as f32 - margin * 2.0).max(1.0);
    let scale = (usable / span_x).min(usable / span_y);
    let origin_x = margin + (usable - span_x * scale) * 0.5;
    let origin_y = margin + (usable - span_y * scale) * 0.5;
    let map = |point: [f32; 3]| -> [f32; 3] {
        [
            origin_x + (point[0] - min_x) * scale,
            origin_y + (point[1] - min_y) * scale,
            point[2],
        ]
    };

    let mut depth = vec![f32::NEG_INFINITY; size * size];
    for triangle in &drawn {
        fill_triangle(
            &mut pixels,
            &mut depth,
            size,
            map(triangle.points[0]),
            map(triangle.points[1]),
            map(triangle.points[2]),
            triangle.colour,
        );
    }
    pixels
}

fn project(point: [f32; 3]) -> [f32; 3] {
    let x = (point[0] - point[1]) * 0.866_025_4;
    let y = -((point[0] + point[1]) * 0.5 + point[2]);
    let depth = point[0] * 0.577_350_26 - point[1] * 0.577_350_26 + point[2] * 0.577_350_26;
    [x, y, depth]
}

fn face_normal(triangle: &[[f32; 3]; 3]) -> [f32; 3] {
    let (a, b, c) = (triangle[0], triangle[1], triangle[2]);
    let ux = b[0] - a[0];
    let uy = b[1] - a[1];
    let uz = b[2] - a[2];
    let vx = c[0] - a[0];
    let vy = c[1] - a[1];
    let vz = c[2] - a[2];
    let normal = [uy * vz - uz * vy, uz * vx - ux * vz, ux * vy - uy * vx];
    let length = (normal[0] * normal[0] + normal[1] * normal[1] + normal[2] * normal[2]).sqrt();
    if length < 1.0e-8 {
        [0.0, 0.0, 0.0]
    } else {
        [normal[0] / length, normal[1] / length, normal[2] / length]
    }
}

fn shade(normal: [f32; 3]) -> f32 {
    let light = [0.35_f32, -0.55, 0.76];
    let length = (light[0] * light[0] + light[1] * light[1] + light[2] * light[2]).sqrt();
    let dot = (normal[0] * light[0] + normal[1] * light[1] + normal[2] * light[2]) / length;
    0.38 + 0.62 * dot.abs()
}

fn scale_channel(channel: u8, light: f32) -> u8 {
    (f32::from(channel) * light).round().clamp(0.0, 255.0) as u8
}

fn edge(a: [f32; 3], b: [f32; 3], c: [f32; 3]) -> f32 {
    (c[0] - a[0]) * (b[1] - a[1]) - (c[1] - a[1]) * (b[0] - a[0])
}

fn fill_triangle(
    pixels: &mut [u8],
    depth: &mut [f32],
    size: usize,
    a: [f32; 3],
    b: [f32; 3],
    c: [f32; 3],
    colour: [u8; 3],
) {
    let area = edge(a, b, c);
    if area.abs() < 1.0e-4 {
        return;
    }
    let sign = area.signum();
    let min_x = a[0].min(b[0]).min(c[0]).floor().max(0.0) as i32;
    let max_x = a[0].max(b[0]).max(c[0]).ceil().min(size as f32 - 1.0) as i32;
    let min_y = a[1].min(b[1]).min(c[1]).floor().max(0.0) as i32;
    let max_y = a[1].max(b[1]).max(c[1]).ceil().min(size as f32 - 1.0) as i32;
    if max_x < min_x || max_y < min_y {
        return;
    }
    for y in min_y..=max_y {
        for x in min_x..=max_x {
            let point = [x as f32 + 0.5, y as f32 + 0.5, 0.0];
            let w0 = edge(b, c, point);
            let w1 = edge(c, a, point);
            let w2 = edge(a, b, point);
            if w0 * sign < 0.0 || w1 * sign < 0.0 || w2 * sign < 0.0 {
                continue;
            }
            let inv = 1.0 / area;
            let z = a[2] * w0 * inv + b[2] * w1 * inv + c[2] * w2 * inv;
            let index = y as usize * size + x as usize;
            if z > depth[index] {
                depth[index] = z;
                let offset = index * 4;
                pixels[offset] = colour[0];
                pixels[offset + 1] = colour[1];
                pixels[offset + 2] = colour[2];
                pixels[offset + 3] = 255;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cube(origin: [f32; 3], size: f32, colour: [u8; 3]) -> PreviewSolid {
        let max = [origin[0] + size, origin[1] + size, origin[2] + size];
        let c = [
            origin,
            [max[0], origin[1], origin[2]],
            [max[0], max[1], origin[2]],
            [origin[0], max[1], origin[2]],
            [origin[0], origin[1], max[2]],
            [max[0], origin[1], max[2]],
            max,
            [origin[0], max[1], max[2]],
        ];
        let faces = [
            [0, 1, 2],
            [0, 2, 3],
            [4, 6, 5],
            [4, 7, 6],
            [0, 4, 5],
            [0, 5, 1],
            [3, 2, 6],
            [3, 6, 7],
            [1, 5, 6],
            [1, 6, 2],
            [0, 3, 7],
            [0, 7, 4],
        ];
        PreviewSolid {
            colour,
            triangles: faces
                .into_iter()
                .map(|face| [c[face[0]], c[face[1]], c[face[2]]])
                .collect(),
        }
    }

    fn part_pixels(rgba: &[u8]) -> usize {
        rgba.as_chunks::<4>().0.iter()
            .filter(|pixel| !matches!(*pixel, [18, 20, 24, 255] | [46, 49, 56, 255]))
            .count()
    }

    #[test]
    fn a_cube_is_drawn_and_an_empty_plate_is_only_the_bed() {
        let empty = isometric_rgba(&[], 128);
        assert_eq!(part_pixels(&empty), 0);
        let solid = isometric_rgba(&[cube([0.0, 0.0, 0.0], 20.0, [220, 40, 40])], 128);
        assert!(part_pixels(&solid) > 20, "the cube should cover the bed");
        assert_ne!(empty, solid);
    }

    #[test]
    fn moving_the_part_moves_the_picture() {
        let left = isometric_rgba(&[cube([-40.0, 0.0, 0.0], 16.0, [220, 40, 40])], 64);
        let right = isometric_rgba(&[cube([40.0, 0.0, 0.0], 16.0, [220, 40, 40])], 64);
        assert_ne!(left, right);
        assert!(part_pixels(&left) > 0);
        assert!(part_pixels(&right) > 0);
    }
}
