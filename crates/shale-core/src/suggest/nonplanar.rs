//! A shallow curved top the existing planner would actually curve.

use super::geom::top_shape;
use super::{placed, tip, Scene, Suggestion, SuggestionFix};
use crate::nonplanar_bridge::to_nonplanar;
use crate::NonplanarOptions;

/// Tops under this many triangles get the real planner. Larger meshes use the
/// same slope test so Prepare does not wait on a height field.
const PLANNER_TRIANGLES: usize = 8_000;

pub fn check(scene: &Scene) -> Vec<Suggestion> {
    let mode = scene.nonplanar_mode.to_ascii_lowercase();
    if mode == "auto" || mode == "curved-tops" {
        return Vec::new();
    }
    let mut out = Vec::new();
    for object in &scene.objects {
        let mesh = placed(object);
        let top = top_shape(&mesh);
        let footprint = (top.sloped_area_mm2).max(0.0);
        let interesting =
            top.spread_mm >= 0.8 && (1.5..10.5).contains(&top.max_slope_deg) && footprint > 20.0;
        if !interesting {
            continue;
        }
        if mesh.triangles.len() <= PLANNER_TRIANGLES {
            let planned = crate::plan_auto_placement(
                &to_nonplanar(&mesh),
                &[],
                &NonplanarOptions::default(),
                0.2,
            );
            if planned.enabled_surface_count == 0 {
                continue;
            }
        }
        out.push(tip(
            "nonplanar_hint",
            format!("nonplanar_hint:{}", object.id),
            "This top can be printed curved",
            format!(
                "The top rises {:.1} mm at about {:.0}°, inside the 10° clearance. Non-planar layers follow that slope instead of stepping it.",
                top.spread_mm, top.max_slope_deg
            ),
            vec![object.id.clone()],
            Some(SuggestionFix {
                label: "Use non-planar".into(),
                nonplanar_mode: Some("auto".into()),
                ..SuggestionFix::default()
            }),
            44,
        ));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mesh::TriangleMesh;
    use crate::suggest::geom::box_mesh;
    use crate::suggest::testing::{object, scene};

    fn ramp() -> TriangleMesh {
        TriangleMesh {
            vertices: vec![
                [0.0, 0.0, 0.0],
                [40.0, 0.0, 0.0],
                [40.0, 40.0, 0.0],
                [0.0, 40.0, 0.0],
                [0.0, 0.0, 6.0],
                [40.0, 0.0, 6.0],
                [40.0, 40.0, 10.0],
                [0.0, 40.0, 10.0],
            ],
            triangles: vec![
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
            ],
        }
    }

    #[test]
    fn a_flat_cube_is_not_a_curved_top() {
        let mesh = box_mesh([0.0, 0.0, 0.0], [20.0, 20.0, 20.0]);
        assert!(check(&scene(vec![object("cube", mesh)])).is_empty());
    }

    #[test]
    fn a_shallow_ramp_points_at_nonplanar() {
        let found = check(&scene(vec![object("lid", ramp())]));
        assert_eq!(found.len(), 1, "{found:?}");
        assert_eq!(
            found[0].fix.as_ref().unwrap().nonplanar_mode.as_deref(),
            Some("auto")
        );
    }

    #[test]
    fn an_active_curved_mode_does_not_repeat_itself() {
        let mut plate = scene(vec![object("lid", ramp())]);
        plate.nonplanar_mode = "auto".into();
        assert!(check(&plate).is_empty());
    }
}
