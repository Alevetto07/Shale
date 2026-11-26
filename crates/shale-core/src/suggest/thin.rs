//! Walls and specks thinner than the line.

use std::collections::BTreeMap;

use super::geom::thinnest_wall_mm;
use super::{placed, tip, Scene, Suggestion, SuggestionFix};

pub fn check(scene: &Scene) -> Vec<Suggestion> {
    let line = scene.printer.line_width_mm.max(0.2);
    let mut out = Vec::new();
    for object in &scene.objects {
        if object.mesh.triangles.len() > 40_000 {
            continue;
        }
        let Some(thickness) = thinnest_wall_mm(&placed(object)) else {
            continue;
        };
        if thickness >= line * 0.95 || thickness < 0.05 {
            continue;
        }
        let mut settings = BTreeMap::new();
        settings.insert("line_width".into(), "0.20".into());
        let nozzle = if thickness < 0.3 {
            " A 0.2 mm nozzle prints it reliably; a 0.4 mm nozzle only gets closer by narrowing the line."
        } else {
            " Narrowing the line to 0.2 mm lets this nozzle trace it. A 0.2 mm nozzle is the cleaner fix if the detail is lettering."
        };
        out.push(tip(
            "thin_walls",
            format!("thin_walls:{}", object.id),
            "Walls thinner than the line",
            format!("A wall is about {thickness:.2} mm and the line is {line:.2} mm.{nozzle}"),
            vec![object.id.clone()],
            Some(SuggestionFix {
                label: "Use a 0.2 mm line".into(),
                object_settings: BTreeMap::from([(object.id.clone(), settings)]),
                ..SuggestionFix::default()
            }),
            80,
        ));
        out.last_mut().unwrap().severity = super::Severity::Warning;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mesh::TriangleMesh;
    use crate::suggest::geom::{append_box, box_mesh};
    use crate::suggest::testing::{object, scene};

    #[test]
    fn a_solid_cube_is_thick_enough() {
        let mesh = box_mesh([0.0, 0.0, 0.0], [20.0, 20.0, 20.0]);
        assert!(check(&scene(vec![object("cube", mesh)])).is_empty());
    }

    #[test]
    fn a_fin_thinner_than_the_line_is_a_warning() {
        let mut mesh = TriangleMesh {
            vertices: Vec::new(),
            triangles: Vec::new(),
        };
        append_box(&mut mesh, [0.0, 0.0, 0.0], [20.0, 20.0, 10.0]);
        append_box(&mut mesh, [9.0, 0.0, 10.0], [9.2, 20.0, 18.0]);
        let found = check(&scene(vec![object("fin", mesh)]));
        assert_eq!(found.len(), 1, "{found:?}");
        assert_eq!(found[0].severity, crate::suggest::Severity::Warning);
        assert_eq!(
            found[0].fix.as_ref().unwrap().object_settings["fin"]
                .get("line_width")
                .map(String::as_str),
            Some("0.20")
        );
    }
}
