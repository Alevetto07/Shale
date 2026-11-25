//! More walls around a hole, on the part and not on the whole plate.

use std::collections::BTreeMap;

use super::geom::enclosed_hole;
use super::{number, placed, setting, tip, Scene, Suggestion, SuggestionFix};

/// A wall thinner than this around a hole wants more perimeters.
pub const THIN_WALL_MM: f64 = 1.6;
pub const MIN_HOLE_MM2: f64 = 4.0;

pub fn check(scene: &Scene) -> Vec<Suggestion> {
    let mut out = Vec::new();
    for object in &scene.objects {
        let Some(hole) = enclosed_hole(&placed(object)) else {
            continue;
        };
        if hole.area_mm2 < MIN_HOLE_MM2 || hole.wall_mm >= THIN_WALL_MM {
            continue;
        }
        let current = number(setting(scene, object, "wall_line_count")).unwrap_or(2.0);
        let next = (current + 2.0).min(6.0);
        if next <= current + 0.1 {
            continue;
        }
        let mut settings = BTreeMap::new();
        settings.insert("wall_line_count".into(), format!("{next:.0}"));
        out.push(tip(
            "local_strength",
            format!("local_strength:{}", object.id),
            "More walls around the hole",
            format!(
                "A hole leaves a wall about {:.1} mm thick. {next:.0} walls on this part carry that better than raising the infill for the whole plate.",
                hole.wall_mm
            ),
            vec![object.id.clone()],
            Some(SuggestionFix {
                label: format!("{next:.0} walls on this part"),
                object_settings: BTreeMap::from([(object.id.clone(), settings)]),
                ..SuggestionFix::default()
            }),
            48,
        ));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mesh::TriangleMesh;
    use crate::suggest::geom::append_box;
    use crate::suggest::testing::{object, scene};

    #[test]
    fn a_solid_cube_has_no_hole() {
        let mesh = crate::suggest::geom::box_mesh([0.0, 0.0, 0.0], [20.0, 20.0, 20.0]);
        assert!(check(&scene(vec![object("cube", mesh)])).is_empty());
    }

    #[test]
    fn a_thin_frame_asks_for_walls_on_the_part() {
        let mut mesh = TriangleMesh {
            vertices: Vec::new(),
            triangles: Vec::new(),
        };
        // A square tube: outer 20 mm, inner opening 16 mm, so the wall is 2 mm
        // of solid at the sides. The mid slice is a ring.
        append_box(&mut mesh, [0.0, 0.0, 0.0], [20.0, 2.0, 10.0]);
        append_box(&mut mesh, [0.0, 18.0, 0.0], [20.0, 20.0, 10.0]);
        append_box(&mut mesh, [0.0, 2.0, 0.0], [2.0, 18.0, 10.0]);
        append_box(&mut mesh, [18.0, 2.0, 0.0], [20.0, 18.0, 10.0]);
        let found = check(&scene(vec![object("frame", mesh)]));
        assert_eq!(found.len(), 1, "{found:?}");
        let walls = found[0].fix.as_ref().unwrap().object_settings["frame"]
            .get("wall_line_count")
            .cloned();
        assert_eq!(walls.as_deref(), Some("4"));
        assert!(found[0].reason.contains("this part"));
    }
}
