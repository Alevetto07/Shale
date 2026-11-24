//! Scale extrusion (`E`) when local layer thickness changes after the inverse map.

use crate::deform::DeformField;

/// Multiply extrusion delta by this factor for a move from `(x0,y0,z0)` to `(x1,y1,z1)`
/// in **physical** space after inverse mapping.
///
/// The factor is the local physical/deformed thickness ratio with no clamp: a
/// layer squashed to 0.02 mm really does need E at a fifth of nominal, and
/// holding it at half (the old clamp) laid down five times the plastic the layer
/// had room for. If the ratio leaves `[min_layer, max_layer] / nominal` the
/// field is wrong, and the safety pass rejects it as a layer-thickness
/// violation instead of hiding it here.
pub fn extrusion_scale(
    field: &DeformField,
    x0: f64,
    y0: f64,
    z0: f64,
    x1: f64,
    y1: f64,
    z1: f64,
) -> f64 {
    let mx = 0.5 * (x0 + x1);
    let my = 0.5 * (y0 + y1);
    let ratio = field.layer_thickness_ratio(mx, my, z0, z1);
    if ratio.is_finite() && ratio > 0.0 {
        1.0 / ratio
    } else {
        1.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::deform::build_deform_field;
    use crate::height_field::build_height_field;
    use crate::test_meshes::box_mesh;

    #[test]
    fn unity_on_flat_plateau() {
        let mesh = box_mesh(10.0, 10.0, 5.0);
        let grid = build_height_field(&mesh, 1.0, 30.0, 1.0);
        let field = build_deform_field(grid, 1.0, 0.2, 0.2, 30.0);
        let s = extrusion_scale(&field, 0.0, 0.0, 2.0, 1.0, 0.0, 2.2);
        assert!((s - 1.0).abs() < 0.15);
    }
}