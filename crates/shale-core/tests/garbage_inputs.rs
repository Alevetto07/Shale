//! Files that are not what they claim, fed to the loaders that read user files.
//!
//! A slicer opens whatever the user picked, and a panic there is not a warning:
//! it takes the window with it, and the window is where the error was going to be
//! shown. Every loader here therefore has to answer `Err`, at any length, for any
//! bytes — including the empty file, the header with no body, and a zip whose
//! entries are the wrong shape.

use std::path::PathBuf;

use shale_core::mesh::{load_mesh, load_stl};
use shale_core::project::read_project;

fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join("shale-garbage-inputs");
    std::fs::create_dir_all(&dir).expect("a scratch directory");
    let path = dir.join(name);
    let _ = std::fs::remove_file(&path);
    path
}

#[test]
fn an_empty_file_is_refused_by_every_loader() {
    let stl = scratch("empty.stl");
    std::fs::write(&stl, []).unwrap();
    assert!(load_stl(&[]).is_err(), "an empty STL has no triangles");
    assert!(load_mesh(&stl).is_err(), "an empty file is not a mesh");

    let three_mf = scratch("empty.3mf");
    std::fs::write(&three_mf, []).unwrap();
    assert!(load_mesh(&three_mf).is_err(), "an empty file is not a 3MF");

    let shale = scratch("empty.shale");
    std::fs::write(&shale, []).unwrap();
    assert!(
        read_project(&shale).is_err(),
        "an empty file is not a project"
    );
}

#[test]
fn a_truncated_binary_stl_is_refused_rather_than_indexed_past() {
    // A binary STL claims its triangle count in bytes 80..84; the body then has
    // to be 50 bytes per triangle. A count of 4 with no body is the shape that
    // reads past the end if the loader trusts the header.
    let mut bytes = vec![0u8; 84];
    bytes[80..84].copy_from_slice(&4u32.to_le_bytes());
    assert!(
        load_stl(&bytes).is_err(),
        "four triangles were promised, none arrived"
    );

    // And a header that is not even long enough to hold the count.
    assert!(
        load_stl(&bytes[..40]).is_err(),
        "a 40-byte file has no triangle count"
    );
}

#[test]
fn an_ascii_stl_with_no_facets_is_refused() {
    let body = b"solid empty\nendsolid empty\n";
    assert!(
        load_stl(body).is_err(),
        "a solid with no facet is not a mesh"
    );
}

#[test]
fn a_file_that_is_not_a_zip_is_not_a_project_or_a_three_mf() {
    let junk = b"this is not a zip archive, it is a sentence\n";
    assert!(load_stl(junk).is_err(), "stl");
    let path = scratch("junk.shale");
    std::fs::write(&path, junk).unwrap();
    assert!(read_project(&path).is_err(), "project");
    let mesh = scratch("junk.3mf");
    std::fs::write(&mesh, junk).unwrap();
    assert!(load_mesh(&mesh).is_err(), "3mf");
}

#[test]
fn a_step_file_of_junk_is_refused() {
    let tolerance = shale_core::cad::TessellationTolerance::standard();
    for body in [
        &b"ISO-10303-21;\nHEADER;\n"[..],
        &b"not a step file at all"[..],
        &b""[..],
    ] {
        assert!(
            shale_core::cad::import_step(body, &tolerance).is_err(),
            "a STEP loader accepted {body:?}"
        );
    }
}
