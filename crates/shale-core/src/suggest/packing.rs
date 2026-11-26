//! Split a plate that does not fit, or merge plates that barely hold anything.

use std::collections::BTreeMap;

use super::geom::bounds;
use super::{placed, tip, Scene, Suggestion, SuggestionFix};

pub fn check(scene: &Scene) -> Vec<Suggestion> {
    let mut by_plate: BTreeMap<usize, Vec<usize>> = BTreeMap::new();
    for (index, object) in scene.objects.iter().enumerate() {
        by_plate.entry(object.plate).or_default().push(index);
    }
    let mut out = Vec::new();
    let bed_w = scene.printer.bed_width_mm;
    let bed_d = scene.printer.bed_depth_mm;
    for (plate, indexes) in &by_plate {
        if indexes.len() < 2 {
            continue;
        }
        let boxes: Vec<_> = indexes
            .iter()
            .map(|index| bounds(&placed(&scene.objects[*index])))
            .collect();
        let mut min = [f64::INFINITY; 2];
        let mut max = [f64::NEG_INFINITY; 2];
        let mut each_fits = true;
        for (lo, hi) in &boxes {
            min[0] = min[0].min(lo[0]);
            min[1] = min[1].min(lo[1]);
            max[0] = max[0].max(hi[0]);
            max[1] = max[1].max(hi[1]);
            if hi[0] - lo[0] > bed_w || hi[1] - lo[1] > bed_d {
                each_fits = false;
            }
        }
        let over = max[0] - min[0] > bed_w + 2.0 || max[1] - min[1] > bed_d + 2.0;
        if !over || !each_fits {
            continue;
        }
        let mut worst = 0_usize;
        let mut worst_over = 0.0;
        for (ordinal, (lo, hi)) in boxes.iter().enumerate() {
            let spill =
                (hi[0] - bed_w).max(0.0) + (hi[1] - bed_d).max(0.0) + (0.0 - lo[0]).max(0.0);
            if spill >= worst_over {
                worst_over = spill;
                worst = ordinal;
            }
        }
        let object = &scene.objects[indexes[worst]];
        let next_plate = by_plate.keys().copied().max().unwrap_or(0) + 1;
        out.push(tip(
            "plate_packing",
            format!("plate_packing:split:{plate}"),
            format!("Move one part off plate {}", plate + 1),
            format!(
                "Plate {} is wider than the {:.0} × {:.0} mm bed once the parts are taken together. {} still fits on a plate of its own.",
                plate + 1,
                bed_w,
                bed_d,
                object.name
            ),
            vec![object.id.clone()],
            Some(SuggestionFix {
                label: format!("Move to plate {}", next_plate + 1),
                plates: BTreeMap::from([(object.id.clone(), next_plate)]),
                ..SuggestionFix::default()
            }),
            50,
        ));
    }

    let singles: Vec<_> = by_plate
        .iter()
        .filter(|(_, indexes)| indexes.len() == 1)
        .map(|(plate, indexes)| (*plate, indexes[0]))
        .collect();
    if singles.len() >= 2 {
        let bed_area = bed_w * bed_d;
        let small: Vec<_> = singles
            .iter()
            .filter(|(_, index)| {
                let (lo, hi) = bounds(&placed(&scene.objects[*index]));
                let area = (hi[0] - lo[0]).max(0.0) * (hi[1] - lo[1]).max(0.0);
                area < bed_area * 0.25 && hi[0] - lo[0] < bed_w * 0.45
            })
            .copied()
            .collect();
        if small.len() >= 2 {
            let widths: Vec<f64> = small
                .iter()
                .map(|(_, index)| {
                    let (lo, hi) = bounds(&placed(&scene.objects[*index]));
                    hi[0] - lo[0]
                })
                .collect();
            let gap = 8.0 * (widths.len().saturating_sub(1) as f64);
            if widths.iter().sum::<f64>() + gap < bed_w {
                let target = small.iter().map(|(plate, _)| *plate).min().unwrap_or(0);
                let mut plates = BTreeMap::new();
                let mut ids = Vec::new();
                for (_, index) in &small {
                    let object = &scene.objects[*index];
                    if object.plate != target {
                        plates.insert(object.id.clone(), target);
                        ids.push(object.id.clone());
                    }
                }
                if !plates.is_empty() {
                    out.push(tip(
                        "plate_packing",
                        "plate_packing:merge".into(),
                        "These plates can share one bed",
                        format!(
                            "{} small parts sit on their own plates and still fit side by side on plate {}.",
                            small.len(),
                            target + 1
                        ),
                        ids,
                        Some(SuggestionFix {
                            label: format!("Merge onto plate {}", target + 1),
                            plates,
                            ..SuggestionFix::default()
                        }),
                        30,
                    ));
                }
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::suggest::geom::box_mesh;
    use crate::suggest::testing::{object, scene};

    #[test]
    fn one_part_on_one_plate_is_not_repacked() {
        let mesh = box_mesh([0.0, 0.0, 0.0], [20.0, 20.0, 20.0]);
        assert!(check(&scene(vec![object("only", mesh)])).is_empty());
    }

    #[test]
    fn two_tiny_plates_can_merge() {
        let mut a = object("a", box_mesh([0.0, 0.0, 0.0], [20.0, 20.0, 10.0]));
        a.plate = 0;
        let mut b = object("b", box_mesh([0.0, 0.0, 0.0], [20.0, 20.0, 10.0]));
        b.plate = 1;
        let found = check(&scene(vec![a, b]));
        assert!(
            found.iter().any(|item| item.id == "plate_packing:merge"),
            "{found:?}"
        );
    }
}
