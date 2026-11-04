//! STEP import. Shale tessellates the boundary representation itself.
//!
//! The kernel is [truck](https://github.com/ricosjp/truck) (Apache-2.0): a
//! pure-Rust B-rep, not OpenCASCADE. `truck-stepio` reads the STEP file into
//! curves and surfaces. `truck-meshalgo` divides those curves so the sagitta
//! (the gap between a chord and the curve) stays within the chordal tolerance.
//! On a circle of radius `R` the step angle is `2 acos(1 - sag / R)`.
//!
//! An angular tolerance is the largest turn, in degrees, allowed between two
//! neighbouring chords of a curved edge. If the first tessellation turns more
//! sharply than that, the chordal tolerance is tightened by the square of the
//! ratio (sagitta grows with the square of the angle) and the shape is meshed
//! again. Planes ignore both tolerances: a straight edge has no turn.
//!
//! Coordinates are millimetres. A STEP length unit of centimetres, metres, or
//! inches is scaled. A file with no length unit is treated as millimetres,
//! which is what the sample parts and most mechanical STEP files use.

use std::time::Instant;

use serde::Serialize;
use truck_meshalgo::prelude::*;
use truck_stepio::r#in::alias::{Curve3D, Surface};
use truck_stepio::r#in::Table;
use truck_topology::compress::CompressedShell;

use crate::error::Error;
use crate::mesh::TriangleMesh;

/// Chordal sag and the maximum turn between chords on a curved edge.
#[derive(Clone, Copy, Debug)]
pub struct TessellationTolerance {
    pub chordal_mm: f64,
    pub angular_deg: f64,
}

impl TessellationTolerance {
    /// A 0.2 mm layer and a 0.4 mm nozzle: chord 0.1 mm, turn 15°.
    pub fn standard() -> Self {
        Self {
            chordal_mm: suggest_chord_mm(0.2, 0.4),
            angular_deg: 15.0,
        }
    }

    pub fn new(chordal_mm: f64, angular_deg: f64) -> Self {
        Self {
            chordal_mm: chordal_mm.clamp(0.02, 2.0),
            angular_deg: angular_deg.clamp(2.0, 60.0),
        }
    }
}

/// Half the layer height, and no coarser than half the nozzle.
///
/// A chord taller than half a layer would miss the curve between two slices.
/// A chord coarser than half the nozzle would flatten a fillet the nozzle can
/// still print. The result stays between 0.02 mm and 0.5 mm.
pub fn suggest_chord_mm(layer_height: f64, nozzle_mm: f64) -> f64 {
    let layer = if layer_height.is_finite() && layer_height > 0.0 {
        layer_height
    } else {
        0.2
    };
    let nozzle = if nozzle_mm.is_finite() && nozzle_mm > 0.0 {
        nozzle_mm
    } else {
        0.4
    };
    (layer * 0.5).min(nozzle * 0.5).clamp(0.02, 0.5)
}

/// What an import produced, including the tessellation check for STEP.
#[derive(Clone, Debug, Serialize)]
pub struct ImportReport {
    pub triangles: usize,
    pub max_deviation_mm: Option<f64>,
    pub max_turning_deg: Option<f64>,
    pub milliseconds: u64,
    pub source: String,
    pub chordal_mm: Option<f64>,
    pub angular_deg: Option<f64>,
}

pub struct ImportedMesh {
    pub mesh: TriangleMesh,
    pub report: ImportReport,
}

impl ImportedMesh {
    pub fn from_mesh(mesh: TriangleMesh, source: &str) -> Self {
        let triangles = mesh.triangles.len();
        Self {
            mesh,
            report: ImportReport {
                triangles,
                max_deviation_mm: None,
                max_turning_deg: None,
                milliseconds: 0,
                source: source.to_string(),
                chordal_mm: None,
                angular_deg: None,
            },
        }
    }
}

/// Faces above this are tessellated with a coarser chord and fewer refinement
/// passes: a 6 500-face assembly otherwise spends an hour chasing 0.1 mm.
const HEAVY_FACE_COUNT: usize = 3_000;
const HEAVY_CHORDAL_MM: f64 = 0.35;
const HEAVY_ANGULAR_DEG: f64 = 25.0;

pub fn import_step(bytes: &[u8], tolerance: &TessellationTolerance) -> Result<ImportedMesh, Error> {
    let started = Instant::now();
    let text = std::str::from_utf8(bytes)
        .map_err(|_| Error::Mesh("STEP file is not UTF-8 text".into()))?;
    let scale = length_scale_to_mm(text);
    let table = Table::from_step(text)
        .ok_or_else(|| Error::Mesh("STEP file could not be parsed".into()))?;
    if table.shell.is_empty() {
        return Err(Error::Mesh(
            "STEP file has no shell Shale can tessellate. Solids from a boolean, or a format truck-stepio 0.3 does not read, are the usual cause.".into(),
        ));
    }
    // A part with thousands of faces costs minutes at a 0.1 mm chord, and the
    // refinement loop multiplies that by five. For those parts a coarser first
    // pass is the difference between an import that lands and one that appears
    // to hang, so the tolerance is relaxed and the loop shortened.
    let faces = text.matches("ADVANCED_FACE").count();
    let heavy = faces > HEAVY_FACE_COUNT;
    let working = if heavy {
        TessellationTolerance {
            chordal_mm: tolerance.chordal_mm.max(HEAVY_CHORDAL_MM),
            angular_deg: tolerance.angular_deg.max(HEAVY_ANGULAR_DEG),
        }
    } else {
        tolerance.clone()
    };
    let passes = if heavy { 2 } else { 5 };
    let mut vertices = Vec::new();
    let mut triangles = Vec::new();
    let mut max_deviation = 0.0_f64;
    let mut max_turning = 0.0_f64;
    let mut any = false;
    let mut failures = Vec::new();
    for shell in table.shell.values() {
        let built = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            table.to_compressed_shell(shell)
        }));
        let shape = match built {
            Ok(Ok(shape)) => shape,
            Ok(Err(err)) => {
                failures.push(err.to_string());
                continue;
            }
            // truck fills in a zero radius when it cannot deserialize a
            // surface, and `Torus::new` panics on it. Rhino and SolidWorks
            // both write tori that trigger this, so the panic becomes a
            // reported failure instead of a dead import task.
            Err(payload) => {
                failures.push(format!(
                    "a surface could not be evaluated ({})",
                    panic_text(payload.as_ref())
                ));
                continue;
            }
        };
        let (mesh, deviation, turning) = match tessellate_shell(&shape, &working, passes) {
            Ok(result) => result,
            Err(err) => {
                failures.push(err.to_string());
                continue;
            }
        };
        let base = vertices.len() as u32;
        vertices.extend(mesh.vertices);
        triangles.extend(
            mesh.triangles
                .into_iter()
                .map(|tri| [tri[0] + base, tri[1] + base, tri[2] + base]),
        );
        max_deviation = max_deviation.max(deviation);
        max_turning = max_turning.max(turning);
        any = true;
    }
    if !failures.is_empty() {
        return Err(Error::Mesh(format!(
            "STEP file could not be read in full: {}. Re-export the part as STL or 3MF.",
            failures.join("; ")
        )));
    }
    if !any {
        return Err(Error::Mesh(format!(
            "STEP shells could not be built into a shape. {}",
            failures.first().cloned().unwrap_or_default()
        )));
    }
    if scale != 1.0 {
        for vertex in &mut vertices {
            vertex[0] *= scale as f32;
            vertex[1] *= scale as f32;
            vertex[2] *= scale as f32;
        }
        max_deviation *= scale;
    }
    if triangles.is_empty() {
        return Err(Error::Mesh(
            "STEP tessellation produced no triangles".into(),
        ));
    }
    let mesh = TriangleMesh {
        vertices,
        triangles,
    };
    Ok(ImportedMesh {
        report: ImportReport {
            triangles: mesh.triangles.len(),
            max_deviation_mm: Some(max_deviation),
            max_turning_deg: Some(max_turning),
            milliseconds: started.elapsed().as_millis() as u64,
            source: "step".into(),
            chordal_mm: Some(tolerance.chordal_mm),
            angular_deg: Some(tolerance.angular_deg),
        },
        mesh,
    })
}

fn panic_text(payload: &(dyn std::any::Any + Send)) -> String {
    if let Some(text) = payload.downcast_ref::<&str>() {
        return (*text).to_string();
    }
    if let Some(text) = payload.downcast_ref::<String>() {
        return text.clone();
    }
    "unknown failure".to_string()
}

fn tessellate_shell(
    shell: &CompressedShell<Point3, Curve3D, Surface>,
    tolerance: &TessellationTolerance,
    passes: usize,
) -> Result<(TriangleMesh, f64, f64), Error> {
    let mut tol = tolerance.chordal_mm.clamp(1.0e-4, 2.0);
    let mut best: Option<(TriangleMesh, f64, f64)> = None;
    for _ in 0..passes.max(1) {
        let meshed = match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            shell.robust_triangulation(tol)
        })) {
            Ok(meshed) => meshed,
            Err(_) => {
                return Err(Error::Mesh(format!(
                    "tessellation failed at a chord of {tol:.4} mm. The part may be smaller than the tolerance."
                )));
            }
        };
        let turning = max_turning_deg(&meshed);
        let deviation = max_sagitta(shell, &meshed);
        let mesh = polygon_to_mesh(meshed.to_polygon());
        let done = turning <= tolerance.angular_deg + 0.25 || tol <= 1.0e-4;
        best = Some((mesh, deviation, turning));
        if done {
            break;
        }
        let scale = (tolerance.angular_deg / turning.max(0.1)).powi(2);
        let next = (tol * scale * 0.85).clamp(1.0e-4, tol * 0.95);
        if (tol - next).abs() < 1.0e-6 {
            break;
        }
        tol = next;
    }
    best.ok_or_else(|| Error::Mesh("tessellation produced nothing".into()))
}

fn max_turning_deg(
    shell: &CompressedShell<Point3, PolylineCurve<Point3>, Option<PolygonMesh>>,
) -> f64 {
    let mut max_deg = 0.0_f64;
    for edge in &shell.edges {
        let points = &edge.curve.0;
        for window in points.windows(3) {
            let incoming = window[1] - window[0];
            let outgoing = window[2] - window[1];
            let left = incoming.magnitude();
            let right = outgoing.magnitude();
            if left < 1.0e-9 || right < 1.0e-9 {
                continue;
            }
            let cosine = (incoming.dot(outgoing) / (left * right)).clamp(-1.0, 1.0);
            max_deg = max_deg.max(cosine.acos().to_degrees());
        }
    }
    max_deg
}

fn max_sagitta(
    original: &CompressedShell<Point3, Curve3D, Surface>,
    meshed: &CompressedShell<Point3, PolylineCurve<Point3>, Option<PolygonMesh>>,
) -> f64 {
    if original.edges.len() != meshed.edges.len() {
        return 0.0;
    }
    let mut max_gap = 0.0_f64;
    for (source, approx) in original.edges.iter().zip(meshed.edges.iter()) {
        let points = &approx.curve.0;
        for window in points.windows(2) {
            let start = window[0];
            let end = window[1];
            let Some(t0) = source.curve.search_parameter(start, None, 8) else {
                continue;
            };
            let Some(t1) = source.curve.search_parameter(end, None, 8) else {
                continue;
            };
            let mid = parameter_midpoint(t0, t1);
            let on_curve = source.curve.subs(mid);
            max_gap = max_gap.max(point_segment_distance(on_curve, start, end));
        }
    }
    max_gap
}

fn parameter_midpoint(t0: f64, t1: f64) -> f64 {
    let (mut low, mut high) = (t0, t1);
    if high < low {
        std::mem::swap(&mut low, &mut high);
    }
    (low + high) * 0.5
}

fn point_segment_distance(point: Point3, start: Point3, end: Point3) -> f64 {
    let span = end - start;
    let length2 = span.magnitude2();
    if length2 < 1.0e-18 {
        return (point - start).magnitude();
    }
    let along = ((point - start).dot(span) / length2).clamp(0.0, 1.0);
    let closest = start + span * along;
    (point - closest).magnitude()
}

fn polygon_to_mesh(mut polygon: PolygonMesh) -> TriangleMesh {
    polygon
        .put_together_same_attrs(1.0e-4)
        .remove_degenerate_faces()
        .remove_unused_attrs();
    let mut vertices = Vec::with_capacity(polygon.positions().len());
    for position in polygon.positions() {
        vertices.push([position[0] as f32, position[1] as f32, position[2] as f32]);
    }
    let mut triangles = Vec::new();
    for face in polygon.tri_faces() {
        triangles.push([face[0].pos as u32, face[1].pos as u32, face[2].pos as u32]);
    }
    for face in polygon.quad_faces() {
        triangles.push([face[0].pos as u32, face[1].pos as u32, face[2].pos as u32]);
        triangles.push([face[0].pos as u32, face[2].pos as u32, face[3].pos as u32]);
    }
    for face in polygon.other_faces() {
        if face.len() < 3 {
            continue;
        }
        let anchor = face[0].pos as u32;
        for index in 1..face.len() - 1 {
            triangles.push([anchor, face[index].pos as u32, face[index + 1].pos as u32]);
        }
    }
    TriangleMesh {
        vertices,
        triangles,
    }
}

/// STEP length unit as a multiplier into millimetres. Missing unit means the
/// numbers are already millimetres.
fn length_scale_to_mm(step: &str) -> f64 {
    let upper = step.to_ascii_uppercase();
    if upper.contains(".MILLI.") {
        return 1.0;
    }
    if upper.contains(".CENTI.") {
        return 10.0;
    }
    if upper.contains(".KILO.") && upper.contains(".METRE.") {
        return 1.0e6;
    }
    if upper.contains("INCH") {
        return 25.4;
    }
    if upper.contains(".METRE.") {
        return 1000.0;
    }
    1.0
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::f64::consts::TAU;
    use truck_modeling::builder;
    use truck_modeling::{Point3 as ModelPoint, Rad, Shell, Solid, Vector3};

    fn header() -> truck_stepio::out::StepHeaderDescriptor {
        truck_stepio::out::StepHeaderDescriptor {
            organization_system: "shale".into(),
            ..Default::default()
        }
    }

    fn step_of(solid: &Solid) -> String {
        let compressed = solid.compress();
        truck_stepio::out::CompleteStepDisplay::new(
            truck_stepio::out::StepModel::from(&compressed),
            header(),
        )
        .to_string()
    }

    fn step_of_shell(shell: &Shell) -> String {
        let compressed = shell.compress();
        let mut models = truck_stepio::out::StepModels::default();
        models.push_shell(&compressed);
        truck_stepio::out::CompleteStepDisplay::new(models, header()).to_string()
    }

    fn box_20() -> Solid {
        let corner = builder::vertex(ModelPoint::new(0.0, 0.0, 0.0));
        let edge = builder::tsweep(&corner, Vector3::new(20.0, 0.0, 0.0));
        let face = builder::tsweep(&edge, Vector3::new(0.0, 20.0, 0.0));
        builder::tsweep(&face, Vector3::new(0.0, 0.0, 20.0))
    }

    fn cylinder() -> Solid {
        let point = builder::vertex(ModelPoint::new(10.0, 0.0, 0.0));
        let circle = builder::rsweep(&point, ModelPoint::origin(), Vector3::unit_z(), Rad(TAU));
        let disk = builder::try_attach_plane(&[circle]).expect("cylinder cap");
        builder::tsweep(&disk, Vector3::new(0.0, 0.0, 20.0))
    }

    fn sphere() -> Shell {
        // A semicircle spun about its diameter. truck will not wrap that shell
        // in a solid (the seam is not a closed oriented boundary), and the
        // surface is still a sphere, which is what the tessellator has to follow.
        let start = builder::vertex(ModelPoint::new(10.0, 0.0, 0.0));
        let end = builder::vertex(ModelPoint::new(-10.0, 0.0, 0.0));
        let arc = builder::circle_arc(&start, &end, ModelPoint::new(0.0, 0.0, 10.0));
        let wire = truck_modeling::Wire::from(vec![arc]);
        builder::rsweep(&wire, ModelPoint::origin(), Vector3::unit_x(), Rad(TAU))
    }

    fn measure(name: &str, step: &str, tolerance: TessellationTolerance) -> ImportedMesh {
        if std::env::var("SHALE_WRITE_STEP").ok().as_deref() == Some("1") {
            let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../../examples")
                .join(format!("{name}.step"));
            std::fs::write(&path, &step).unwrap();
        }
        let imported = import_step(step.as_bytes(), &tolerance).unwrap_or_else(|err| {
            panic!("{name}: {err}");
        });
        eprintln!(
            "{name}: {} triangles, deviation {:.4} mm, turn {:.2} deg, {} ms, chord {:.3} mm",
            imported.report.triangles,
            imported.report.max_deviation_mm.unwrap_or(0.0),
            imported.report.max_turning_deg.unwrap_or(0.0),
            imported.report.milliseconds,
            tolerance.chordal_mm,
        );
        imported
    }

    #[test]
    fn panic_payload_text_is_extracted() {
        let text = std::panic::catch_unwind(|| panic!("radius must be larger than 0")).unwrap_err();
        assert_eq!(panic_text(text.as_ref()), "radius must be larger than 0");
        let owned = std::panic::catch_unwind(|| std::panic::panic_any(String::from("boom"))).unwrap_err();
        assert_eq!(panic_text(owned.as_ref()), "boom");
    }

    #[test]
    fn step_parts_stay_inside_the_chord_and_the_angle() {
        let tolerance = TessellationTolerance::new(0.1, 15.0);
        let block = measure("block-20", &step_of(&box_20()), tolerance);
        assert!(
            block.report.triangles >= 12 && block.report.triangles <= 24,
            "a 20 mm box should be a handful of triangles, got {}",
            block.report.triangles
        );
        assert!(block.report.max_deviation_mm.unwrap() < 1.0e-3);
        assert!(block.report.max_turning_deg.unwrap() < 1.0);

        let tube = measure("cylinder-r10-h20", &step_of(&cylinder()), tolerance);
        assert!(
            tube.report.triangles > 40,
            "a cylinder needs more than a box"
        );
        assert!(
            tube.report.max_deviation_mm.unwrap() <= 0.12,
            "cylinder sag {}",
            tube.report.max_deviation_mm.unwrap()
        );
        assert!(
            tube.report.max_turning_deg.unwrap() <= 16.0,
            "cylinder turn {}",
            tube.report.max_turning_deg.unwrap()
        );
        let (min, max) = tube.mesh.bounds();
        assert!(
            (max[2] - min[2] - 20.0).abs() < 0.2,
            "height {:?} {:?}",
            min,
            max
        );
        let radius = tube
            .mesh
            .vertices
            .iter()
            .filter(|vertex| vertex[2] > 1.0 && vertex[2] < 19.0)
            .map(|vertex| (vertex[0].powi(2) + vertex[1].powi(2)).sqrt())
            .fold(0.0_f32, f32::max);
        assert!((radius - 10.0).abs() < 0.15, "radius {radius}");

        let ball = measure("sphere-r10", &step_of_shell(&sphere()), tolerance);
        assert!(ball.report.triangles > 40);
        assert!(ball.report.max_deviation_mm.unwrap() <= 0.15);
        assert!(ball.report.max_turning_deg.unwrap() <= 16.0);

        let started = std::time::Instant::now();
        let dense = import_step(
            step_of_shell(&sphere()).as_bytes(),
            &TessellationTolerance::new(0.01, 12.0),
        )
        .expect("dense sphere");
        eprintln!(
            "dense sphere chord 0.01 mm: {} triangles, deviation {:.4} mm, turn {:.2} deg, {} ms",
            dense.report.triangles,
            dense.report.max_deviation_mm.unwrap_or(0.0),
            dense.report.max_turning_deg.unwrap_or(0.0),
            started.elapsed().as_millis()
        );
        assert!(dense.report.triangles > ball.report.triangles);
        assert!(dense.report.max_deviation_mm.unwrap() <= 0.02);
    }

    #[test]
    fn metre_unit_scales_into_millimetres() {
        let mut step = step_of(&box_20());
        step = step.replace(
            "DATA;",
            "DATA;\n#9 = ( LENGTH_UNIT() NAMED_UNIT(*) SI_UNIT(.MILLI.,.METRE.) );",
        );
        // The box is already in millimetres; a MILLI unit must not scale it again.
        let imported = import_step(step.as_bytes(), &TessellationTolerance::standard()).unwrap();
        let (min, max) = imported.mesh.bounds();
        assert!((max[0] - min[0] - 20.0).abs() < 0.05);

        let metres = step.replace(".MILLI.", "$");
        let scaled = import_step(metres.as_bytes(), &TessellationTolerance::standard()).unwrap();
        let (min, max) = scaled.mesh.bounds();
        assert!(
            (max[0] - min[0] - 20_000.0).abs() < 5.0,
            "a metre file should become millimetres, span {}",
            max[0] - min[0]
        );
    }

    #[test]
    fn tessellated_cylinder_slices_when_the_engine_is_configured() {
        let Ok(program) = std::env::var("SHALE_CURAENGINE") else {
            return;
        };
        let Ok(cura) = std::env::var("SHALE_CURA_DIR") else {
            return;
        };
        let Ok(materials) = std::env::var("SHALE_MATERIALS_DIR") else {
            return;
        };
        let imported = import_step(
            step_of(&cylinder()).as_bytes(),
            &TessellationTolerance::new(0.1, 15.0),
        )
        .expect("cylinder");
        let catalog = crate::Catalog::open(crate::ResourcePaths {
            cura: cura.into(),
            materials: materials.into(),
        })
        .expect("catalog");
        let resolved = catalog
            .resolve(&crate::ResolveRequest {
                printer_id: "creality_ender3".into(),
                variant_name: Some("0.4mm Nozzle".into()),
                quality_type: Some("draft".into()),
                material_id: Some("generic_pla".into()),
                intent_category: None,
                settings: crate::UserSettings {
                    layer_height: Some(0.2),
                    infill_sparse_density: Some(15.0),
                    support_enable: Some(false),
                    ..crate::UserSettings::default()
                },
            })
            .expect("resolve");
        let transform = crate::place_on_bed(
            &imported.mesh,
            resolved.summary.machine_width,
            resolved.summary.machine_depth,
            resolved.summary.machine_center_is_zero,
        );
        let engine = crate::CliEngine {
            program: program.into(),
            lib_dir: std::env::var("SHALE_CURAENGINE_LIB_DIR")
                .ok()
                .map(Into::into),
        };
        let outcome = crate::slice(
            &catalog,
            &engine,
            "creality_ender3",
            Some("0.4mm Nozzle".into()),
            Some("draft".into()),
            Some("generic_pla".into()),
            crate::UserSettings {
                layer_height: Some(0.2),
                infill_sparse_density: Some(15.0),
                support_enable: Some(false),
                ..crate::UserSettings::default()
            },
            &[crate::PlacedModel {
                name: "cylinder".into(),
                mesh: imported.mesh,
                transform,
                settings: Vec::new(),
            }],
            &[],
            None,
            std::env::temp_dir().join("shale-step-cylinder.gcode"),
            &mut |_| {},
        )
        .expect("slice");
        let gcode = std::fs::read_to_string(&outcome.gcode_path).unwrap();
        assert!(gcode.contains(";LAYER:"), "{}", outcome.log_tail);
        assert!(!outcome.toolpath.layers.is_empty());
        eprintln!(
            "sliced tessellated cylinder: {} layers, {} triangles",
            outcome.toolpath.layers.len(),
            outcome
                .toolpath
                .layers
                .iter()
                .map(|layer| layer.polylines.len())
                .sum::<usize>()
        );
    }
}
