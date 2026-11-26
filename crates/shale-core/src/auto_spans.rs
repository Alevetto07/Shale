//! Bridge `supports::analyse` spans into the non-planar auto planner.

use crate::mesh::TriangleMesh;
use crate::supports::{self, Analysis, SupportParams, Span};

pub fn analyse_for_nonplanar(mesh: &TriangleMesh) -> Analysis {
    supports::analyse(mesh, &SupportParams::default())
}

pub fn spans_for_auto(spans: &[Span]) -> Vec<shale_nonplanar::SupportSpanInput> {
    spans.iter().map(span_to_input).collect()
}

fn span_to_input(span: &Span) -> shale_nonplanar::SupportSpanInput {
    let mut min = [f64::INFINITY; 3];
    let mut max = [f64::NEG_INFINITY; 3];
    for s in &span.samples {
        min[0] = min[0].min(s.x);
        min[1] = min[1].min(s.y);
        min[2] = min[2].min(s.z);
        max[0] = max[0].max(s.x);
        max[1] = max[1].max(s.y);
        max[2] = max[2].max(s.z);
    }
    if span.samples.is_empty() {
        min = [0.0; 3];
        max = [0.0; 3];
    }
    shale_nonplanar::SupportSpanInput {
        kind: span.kind.name().into(),
        length_mm: span.length_mm,
        width_mm: span.width_mm,
        area_mm2: span.area_mm2,
        z_mm: span.z_mm,
        angle_from_vertical_deg: span.angle_from_vertical_deg,
        anchor_ends: span.anchor_ends,
        sample_min: min,
        sample_max: max,
    }
}