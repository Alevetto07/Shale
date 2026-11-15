//! Write meshes back out: binary STL for a single blob, 3MF for parts that
//! should arrive in another slicer as separate objects with their names.
//!
//! Both writers take world-space geometry — the caller bakes the model
//! transform in first — so what comes out is the plate as it looks, not the
//! parts as they were loaded.

use std::io::{Cursor, Write};

use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipWriter};

use crate::mesh::TriangleMesh;

const CONTENT_TYPES: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types">
 <Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/>
 <Default Extension="model" ContentType="application/vnd.ms-package.3dmanufacturing-3dmodel+xml"/>
</Types>
"#;

const RELS: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">
 <Relationship Target="/3D/3dmodel.model" Id="rel0" Type="http://schemas.microsoft.com/3dmanufacturing/2013/01/3dmodel"/>
</Relationships>
"#;

/// Binary STL of a single mesh: 80-byte header, triangle count, then a normal
/// and three vertices per triangle. Every slicer reads this; nothing else does.
pub fn to_stl(mesh: &TriangleMesh) -> Vec<u8> {
    let count = mesh.triangles.len() as u32;
    let mut out = Vec::with_capacity(84 + count as usize * 50);
    let mut header = [0u8; 80];
    let stamp = b"Shale binary STL";
    header[..stamp.len()].copy_from_slice(stamp);
    out.extend_from_slice(&header);
    out.extend_from_slice(&count.to_le_bytes());
    for triangle in &mesh.triangles {
        let a = mesh.vertices[triangle[0] as usize];
        let b = mesh.vertices[triangle[1] as usize];
        let c = mesh.vertices[triangle[2] as usize];
        for value in normal(a, b, c) {
            out.extend_from_slice(&value.to_le_bytes());
        }
        for vertex in [a, b, c] {
            for value in vertex {
                out.extend_from_slice(&value.to_le_bytes());
            }
        }
        out.extend_from_slice(&0u16.to_le_bytes());
    }
    out
}

fn normal(a: [f32; 3], b: [f32; 3], c: [f32; 3]) -> [f32; 3] {
    let u = [b[0] - a[0], b[1] - a[1], b[2] - a[2]];
    let v = [c[0] - a[0], c[1] - a[1], c[2] - a[2]];
    let cross = [
        u[1] * v[2] - u[2] * v[1],
        u[2] * v[0] - u[0] * v[2],
        u[0] * v[1] - u[1] * v[0],
    ];
    let length = (cross[0] * cross[0] + cross[1] * cross[1] + cross[2] * cross[2]).sqrt();
    if length > 0.0 {
        [cross[0] / length, cross[1] / length, cross[2] / length]
    } else {
        [0.0, 0.0, 1.0]
    }
}

/// Merge several meshes into one, so a plate can leave as a single STL.
pub fn merge(meshes: &[TriangleMesh]) -> TriangleMesh {
    let mut vertices = Vec::new();
    let mut triangles = Vec::new();
    for mesh in meshes {
        let base = vertices.len() as u32;
        vertices.extend_from_slice(&mesh.vertices);
        triangles.extend(
            mesh.triangles
                .iter()
                .map(|triangle| [triangle[0] + base, triangle[1] + base, triangle[2] + base]),
        );
    }
    TriangleMesh {
        vertices,
        triangles,
    }
}

/// A 3MF holding one object per part, named. Parts arrive in another slicer as
/// separate objects, which is what "export plate as 3MF" means everywhere else.
pub fn to_3mf(parts: &[(&str, &TriangleMesh)]) -> Result<Vec<u8>, String> {
    let model = model_xml(parts);
    let mut archive = ZipWriter::new(Cursor::new(Vec::new()));
    let stored = SimpleFileOptions::default().compression_method(CompressionMethod::Stored);
    let deflated = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);
    for (name, body, options) in [
        ("[Content_Types].xml", CONTENT_TYPES, stored),
        ("_rels/.rels", RELS, stored),
        ("3D/3dmodel.model", model.as_str(), deflated),
    ] {
        archive
            .start_file(name, options)
            .map_err(|err| err.to_string())?;
        archive
            .write_all(body.as_bytes())
            .map_err(|err| err.to_string())?;
    }
    let cursor = archive.finish().map_err(|err| err.to_string())?;
    Ok(cursor.into_inner())
}

fn model_xml(parts: &[(&str, &TriangleMesh)]) -> String {
    let mut xml = String::with_capacity(1024);
    xml.push_str("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n");
    xml.push_str(
        "<model unit=\"millimeter\" xml:lang=\"en-US\" \
         xmlns=\"http://schemas.microsoft.com/3dmanufacturing/core/2015/02\">\n <resources>\n",
    );
    for (index, (name, mesh)) in parts.iter().enumerate() {
        xml.push_str(&format!(
            "  <object id=\"{}\" type=\"model\" name=\"{}\">\n   <mesh>\n    <vertices>\n",
            index + 1,
            escape(name)
        ));
        for vertex in &mesh.vertices {
            xml.push_str(&format!(
                "     <vertex x=\"{:.6}\" y=\"{:.6}\" z=\"{:.6}\"/>\n",
                vertex[0], vertex[1], vertex[2]
            ));
        }
        xml.push_str("    </vertices>\n    <triangles>\n");
        for triangle in &mesh.triangles {
            xml.push_str(&format!(
                "     <triangle v1=\"{}\" v2=\"{}\" v3=\"{}\"/>\n",
                triangle[0], triangle[1], triangle[2]
            ));
        }
        xml.push_str("    </triangles>\n   </mesh>\n  </object>\n");
    }
    xml.push_str(" </resources>\n <build>\n");
    for index in 0..parts.len() {
        xml.push_str(&format!("  <item objectid=\"{}\"/>\n", index + 1));
    }
    xml.push_str(" </build>\n</model>\n");
    xml
}

fn escape(name: &str) -> String {
    name.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Read;

    fn cube() -> TriangleMesh {
        // One triangle is enough to prove the writers agree on layout.
        TriangleMesh {
            vertices: vec![[0.0, 0.0, 0.0], [10.0, 0.0, 0.0], [0.0, 10.0, 0.0]],
            triangles: vec![[0, 1, 2]],
        }
    }

    #[test]
    fn stl_starts_with_a_count_and_one_fifty_byte_triangle() {
        let bytes = to_stl(&cube());
        assert_eq!(bytes.len(), 84 + 50);
        let count = u32::from_le_bytes([bytes[80], bytes[81], bytes[82], bytes[83]]);
        assert_eq!(count, 1);
        // Normal of the XY triangle points up.
        let ny = f32::from_le_bytes([bytes[88], bytes[89], bytes[90], bytes[91]]);
        assert!((ny - 0.0).abs() < 1e-6, "{ny}");
        let nz = f32::from_le_bytes([bytes[92], bytes[93], bytes[94], bytes[95]]);
        assert!((nz - 1.0).abs() < 1e-6, "{nz}");
    }

    #[test]
    fn merge_offsets_triangle_indices() {
        let merged = merge(&[cube(), cube()]);
        assert_eq!(merged.vertices.len(), 6);
        assert_eq!(merged.triangles, vec![[0, 1, 2], [3, 4, 5]]);
    }

    #[test]
    fn three_mf_carries_one_named_object_per_part() {
        let bytes = to_3mf(&[("Arm", &cube()), ("Bracket & co", &cube())]).unwrap();
        let mut archive = zip::ZipArchive::new(Cursor::new(bytes)).unwrap();
        assert_eq!(archive.len(), 3);
        let mut xml = String::new();
        archive
            .by_name("3D/3dmodel.model")
            .unwrap()
            .read_to_string(&mut xml)
            .unwrap();
        assert!(xml.contains("id=\"1\""), "{xml}");
        assert!(xml.contains("id=\"2\""), "{xml}");
        assert!(xml.contains("name=\"Arm\""), "{xml}");
        assert!(xml.contains("name=\"Bracket &amp; co\""), "{xml}");
        assert_eq!(xml.matches("<vertex ").count(), 6);
        assert_eq!(xml.matches("<triangle ").count(), 2);
        assert_eq!(xml.matches("<item objectid=").count(), 2);
    }
}
