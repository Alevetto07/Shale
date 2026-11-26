//! Long bridges, and the settings that print them.

use std::collections::BTreeMap;

use crate::supports::{analyse, FeatureKind, SupportParams};

use super::{placed, tip, Scene, Suggestion, SuggestionFix};

/// A span at least this long is worth bridge settings.
pub const LONG_BRIDGE_MM: f64 = 14.0;
/// Longer than this, the reason also mentions a support blocker.
pub const BLOCKER_BRIDGE_MM: f64 = 40.0;

pub fn check(scene: &Scene) -> Vec<Suggestion> {
    let mut out = Vec::new();
    for object in &scene.objects {
        if object.mesh.triangles.len() > 40_000 {
            continue;
        }
        let mesh = placed(object);
        let analysis = analyse(&mesh, &SupportParams::default());
        let longest = analysis
            .spans
            .iter()
            .filter(|span| span.kind == FeatureKind::Bridge)
            .map(|span| span.length_mm)
            .fold(0.0, f64::max);
        if longest < LONG_BRIDGE_MM {
            continue;
        }
        let blocker = if longest >= BLOCKER_BRIDGE_MM {
            " If it sags, a support blocker on the span is the other option."
        } else {
            ""
        };
        let mut settings = BTreeMap::new();
        settings.insert("bridge_settings_enabled".into(), "true".into());
        settings.insert("bridge_fan_speed".into(), "100".into());
        out.push(tip(
            "bridging",
            format!("bridging:{}", object.id),
            format!("A {longest:.0} mm bridge"),
            format!(
                "A span of {longest:.0} mm is long enough to sag. Bridge settings slow that span and turn the fan up.{blocker}"
            ),
            vec![object.id.clone()],
            Some(SuggestionFix {
                label: "Enable bridge settings".into(),
                object_settings: BTreeMap::from([(object.id.clone(), settings)]),
                ..SuggestionFix::default()
            }),
            55,
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

    fn bridge() -> TriangleMesh {
        let mut mesh = TriangleMesh {
            vertices: Vec::new(),
            triangles: Vec::new(),
        };
        append_box(&mut mesh, [0.0, 0.0, 0.0], [8.0, 8.0, 20.0]);
        append_box(&mut mesh, [24.0, 0.0, 0.0], [32.0, 8.0, 20.0]);
        append_box(&mut mesh, [8.0, 0.0, 20.0], [24.0, 8.0, 24.0]);
        mesh
    }

    #[test]
    fn a_cube_has_no_bridge() {
        let mesh = crate::suggest::geom::box_mesh([0.0, 0.0, 0.0], [20.0, 20.0, 20.0]);
        assert!(check(&scene(vec![object("cube", mesh)])).is_empty());
    }

    #[test]
    fn a_span_between_two_pillars_offers_bridge_settings() {
        let found = check(&scene(vec![object("span", bridge())]));
        assert_eq!(found.len(), 1, "{found:?}");
        assert!(found[0].reason.contains("bridge") || found[0].title.contains("bridge"));
        assert_eq!(
            found[0].fix.as_ref().unwrap().object_settings["span"]
                .get("bridge_settings_enabled")
                .map(String::as_str),
            Some("true")
        );
    }
}
