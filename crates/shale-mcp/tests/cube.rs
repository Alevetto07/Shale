//! An in-process MCP client drives Shale the way Claude Desktop would,
//! except the transport is a memory pipe and the engine is a stand-in that
//! writes one layer per estimated layer. A second test uses the real
//! CuraEngine binary when `SHALE_CURAENGINE` is set.

use std::path::{Path, PathBuf};

use rmcp::model::{CallToolRequestParams, CallToolResult, ContentBlock};
use rmcp::ServiceExt;
use serde_json::{json, Value};
use shale_core::engine::{EngineOutput, EngineProgress, EngineRequest, SlicingEngine};
use shale_core::{Catalog, CliEngine, Error, ResourcePaths};
use shale_mcp::ShaleMcp;

struct FakeEngine;

impl SlicingEngine for FakeEngine {
    fn slice(
        &self,
        request: &EngineRequest,
        on_progress: &mut dyn FnMut(EngineProgress),
    ) -> Result<EngineOutput, Error> {
        on_progress(EngineProgress::new(
            0.4,
            format!(
                "Processing insets for layer 1 of {}",
                request.estimated_layers
            ),
        ));
        let layers = request.estimated_layers.max(1);
        let mut body = String::from(";TIME:6666\n;Filament used: 0m\n");
        for layer in 0..layers {
            let z = (layer + 1) as f64 * 0.2;
            body.push_str(&format!(";LAYER:{layer}\nG1 Z{z:.3}\nG1 X10 Y10 E0.02\n"));
        }
        body.push_str(";TIME_ELAPSED:120\n");
        if let Some(parent) = request.output_gcode.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(&request.output_gcode, body)?;
        on_progress(EngineProgress::new(0.93, "starting export"));
        Ok(EngineOutput {
            gcode_path: request.output_gcode.clone(),
            log_tail: "Total print time: 120\n".into(),
            print_time_seconds: Some(120.0),
        })
    }
}

fn ensure_test_cache() {
    shale_core::set_catalog_cache_dir(shale_core::integration_test_cache_dir("shale-mcp"));
}

fn fixture_catalog() -> Catalog {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../shale-core/tests/fixtures");
    Catalog::open(ResourcePaths {
        cura: root.join("cura"),
        materials: root.join("materials"),
    })
    .expect("fixture catalog")
}

fn cube_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples/cube.stl")
}

fn tool_json(result: CallToolResult) -> Value {
    assert_ne!(result.is_error, Some(true), "{result:?}");
    let ContentBlock::Text(text) = &result.content[0] else {
        panic!("expected text content, got {result:?}");
    };
    serde_json::from_str(&text.text).unwrap_or_else(|err| {
        panic!("tool did not return JSON ({err}): {}", text.text);
    })
}

async fn call(
    client: &rmcp::service::RunningService<rmcp::RoleClient, ()>,
    name: &str,
    arguments: Value,
) -> Value {
    let arguments = arguments.as_object().cloned();
    let mut params = CallToolRequestParams::new(name.to_string());
    if let Some(arguments) = arguments {
        params = params.with_arguments(arguments);
    }
    let result = client
        .call_tool(params)
        .await
        .unwrap_or_else(|err| panic!("{name} failed: {err}"));
    tool_json(result)
}

async fn slice_cube(
    catalog: Catalog,
    engine: Box<dyn SlicingEngine + Send + Sync>,
    destination: &Path,
) -> Value {
    let session = shale_core::Session::new(catalog, engine);
    let server = ShaleMcp::new(session);
    let (server_io, client_io) = tokio::io::duplex(1024 * 1024);
    let server_task = tokio::spawn(async move {
        let running = server.serve(server_io).await.expect("server handshake");
        let _ = running.waiting().await;
    });
    let client = ().serve(client_io).await.expect("client handshake");

    let tools = client.list_tools(None).await.expect("list tools").tools;
    for name in [
        "list_printers",
        "list_materials",
        "list_profiles",
        "load_model",
        "list_plates",
        "transform_model",
        "arrange_model",
        "get_settings",
        "set_settings",
        "list_instructions",
        "add_instruction",
        "remove_instruction",
        "analyse_overhangs",
        "propose_supports",
        "apply_supports",
        "slice",
        "slice_summary",
        "slice_stats",
        "slice_variants",
        "apply_variant",
        "get_suggestions",
        "apply_suggestion",
        "dismiss_suggestion",
        "list_my_printers",
        "printer_status",
        "list_print_jobs",
        "list_recent_projects",
        "open_project",
        "save_project",
        "export_gcode",
    ] {
        assert!(
            tools.iter().any(|tool| tool.name == name),
            "missing tool {name}"
        );
    }

    let printers = call(&client, "list_printers", json!({})).await;
    assert!(
        printers.is_array(),
        "list_printers should return a JSON array"
    );

    let loaded = call(&client, "load_model", json!({ "path": cube_path() })).await;
    let id = loaded["id"].as_u64().expect("model id");
    call(&client, "arrange_model", json!({ "id": id })).await;

    let settings = call(
        &client,
        "set_settings",
        json!({ "layer_height": 0.2, "infill_sparse_density": 15 }),
    )
    .await;
    let layer_height = settings["settings"]
        .as_array()
        .unwrap()
        .iter()
        .find(|item| item["key"] == "layer_height")
        .expect("layer_height explanation");
    assert_eq!(layer_height["value"], "0.2");
    assert!(layer_height.get("label").is_some());
    assert!(layer_height.get("description").is_some());

    let added = call(
        &client,
        "add_instruction",
        json!({
            "kind": "pause",
            "layer": 40,
            "filament_change": true,
            "message": "swap to red"
        }),
    )
    .await;
    assert!(added["summary"].as_str().unwrap().contains("layer 40"));

    let summary = call(&client, "slice", json!({})).await;
    assert_eq!(summary["layer_count"], 100, "{summary}");
    assert_eq!(summary["flavour"], "marlin");

    let again = call(&client, "slice_summary", json!({})).await;
    assert_eq!(again["layer_count"], 100);

    let exported = call(&client, "export_gcode", json!({ "path": destination })).await;
    let written = exported["path"].as_str().unwrap();
    let gcode = std::fs::read_to_string(written).unwrap();
    let at = gcode.find(";LAYER:40").expect(";LAYER:40");
    let next = gcode.find(";LAYER:41").expect(";LAYER:41");
    let body = &gcode[at..next];
    assert!(body.contains("M600 ; swap to red"), "{body}");

    client.cancel().await.ok();
    server_task.abort();
    summary
}

#[tokio::test]
async fn arrange_keeps_a_rotation_and_a_typo_names_the_setting() {
    ensure_test_cache();
    let mut session = shale_core::Session::new(fixture_catalog(), Box::new(FakeEngine));
    let loaded = session.load_model(&cube_path()).expect("cube");
    session
        .transform_model(loaded.id, None, Some([0.0, 0.0, 90.0]), Some([2.0, 2.0, 2.0]))
        .unwrap();
    let placed = session.arrange_model(loaded.id).unwrap();
    assert_eq!(placed.rotation_deg[2], 90.0, "arrange dropped the rotation");
    assert_eq!(placed.scale, [2.0, 2.0, 2.0], "arrange dropped the scale");
    let plates = session.list_plates();
    assert_eq!(plates.len(), 1);
    assert_eq!(plates[0].number, 1, "an STL is plate 1, the same number load_model takes");

    session
        .set_settings(shale_core::SettingsPatch {
            adhesion_type: Some("brim".into()),
            brim_width: Some(8.0),
            support_structure: Some("tree".into()),
            settings: [("draft_shield".into(), "limited".into())].into(),
            ..shale_core::SettingsPatch::default()
        })
        .unwrap();
    let view = session
        .get_settings(&[
            "adhesion_type".into(),
            "brim_width".into(),
            "support_structure".into(),
            "draft_shield".into(),
        ])
        .unwrap();
    let value = |key: &str| {
        view.settings
            .iter()
            .find(|item| item.key == key)
            .map(|item| item.value.clone())
            .unwrap_or_else(|| panic!("missing {key}"))
    };
    assert_eq!(value("adhesion_type"), "brim");
    assert_eq!(value("brim_width"), "8");
    assert_eq!(value("support_structure"), "tree");
    assert_eq!(value("draft_shield"), "limited");

    let typo = session
        .set_settings(shale_core::SettingsPatch {
            settings: [("adhesion".into(), "skirt".into())].into(),
            ..shale_core::SettingsPatch::default()
        })
        .expect_err("an unknown key must not be stored");
    assert!(
        typo.to_string().contains("adhesion_type"),
        "expected a suggestion, got {typo}"
    );
    let auto = session
        .set_settings(shale_core::SettingsPatch {
            support_structure: Some("auto".into()),
            ..shale_core::SettingsPatch::default()
        })
        .expect_err("auto is not in this engine");
    assert!(auto.to_string().contains("not available"), "{auto}");
    let bad = session
        .set_settings(shale_core::SettingsPatch {
            adhesion_type: Some("raft".into()),
            ..shale_core::SettingsPatch::default()
        })
        .expect_err("raft is outside the adhesion enum");
    assert!(bad.to_string().contains("none, skirt, or brim"), "{bad}");
}

#[tokio::test]
async fn an_unknown_printer_is_an_error_not_an_empty_profile_list() {
    ensure_test_cache();
    let session = shale_core::Session::new(fixture_catalog(), Box::new(FakeEngine));
    let err = session
        .list_profiles(Some("not_a_printer"))
        .expect_err("unknown printer must not look like a printer with no profiles");
    let text = err.to_string();
    assert!(
        text.contains("not_a_printer"),
        "the id should be in the error, got {text}"
    );
    assert!(
        text.contains("not found"),
        "the error should say the printer was not found, got {text}"
    );
    session
        .list_profiles(None)
        .expect("the session printer still has profiles");
    let materials = session
        .list_materials(Some("not_a_printer"))
        .expect_err("materials for an unknown printer should fail the same way");
    assert!(materials.to_string().contains("not_a_printer"));
}

#[test]
fn variants_stats_and_a_saved_project_round_trip() {
    ensure_test_cache();
    let dir = tempfile::tempdir().expect("config dir");
    std::env::set_var("SHALE_CONFIG_DIR", dir.path());
    let mut session = shale_core::Session::new(fixture_catalog(), Box::new(FakeEngine));
    session
        .load_model(&cube_path())
        .expect("cube");
    session.slice(&mut |_| {}).expect("slice");
    let stats = session.slice_stats(Some(20.0), true).expect("stats");
    assert!(stats.layers > 0, "{stats:?}");
    assert!(stats.layer_seconds.as_ref().is_some_and(|rows| !rows.is_empty()));
    let rows = session
        .slice_variants(
            vec![
                shale_core::SettingsPatch {
                    infill_sparse_density: Some(15.0),
                    ..shale_core::SettingsPatch::default()
                },
                shale_core::SettingsPatch {
                    infill_sparse_density: Some(30.0),
                    ..shale_core::SettingsPatch::default()
                },
            ],
            Some(2),
            Some(20.0),
        )
        .expect("variants");
    assert_eq!(rows.len(), 2);
    assert!(rows.iter().all(|row| row.error.is_none()), "{rows:?}");
    assert!(rows.iter().all(|row| row.print_time_seconds == Some(120.0)));
    session.apply_variant(1).expect("apply the second patch");
    let missing = session
        .apply_suggestion("missing", shale_core::SuggestQuery::default())
        .expect_err("an unknown suggestion is an error");
    assert!(missing.to_string().contains("no suggestion"));
    session
        .dismiss_suggestion("not-a-real-tip", false)
        .expect("dismiss");
    let saved = dir.path().join("cube.shale");
    session.save_project(&saved).expect("save");
    let opened = session.open_project(&saved).expect("open");
    assert_eq!(opened.models, 1);
    assert!(opened.dismissed.iter().any(|id| id == "not-a-real-tip"));
    let recent = session.list_recent_projects().expect("recents");
    assert!(recent.iter().any(|entry| entry.path == saved.display().to_string()));
}

#[tokio::test]
async fn printer_tools_ask_for_a_sign_in() {
    ensure_test_cache();
    let session = shale_core::Session::new(fixture_catalog(), Box::new(FakeEngine));
    let server = ShaleMcp::new(session);
    let (server_io, client_io) = tokio::io::duplex(64 * 1024);
    let server_task = tokio::spawn(async move {
        let running = server.serve(server_io).await.expect("server handshake");
        let _ = running.waiting().await;
    });
    let client = ().serve(client_io).await.expect("client handshake");
    let err = client
        .call_tool(CallToolRequestParams::new("list_my_printers".to_string()))
        .await
        .expect_err("a signed-out account is an error");
    assert!(
        err.to_string().contains("sign in to Bambu in Shale first"),
        "{err}"
    );
    drop(client);
    let _ = server_task.await;
}

#[tokio::test]
async fn in_process_client_slices_the_20mm_cube() {
    ensure_test_cache();
    let destination = tempfile::Builder::new()
        .prefix("shale-mcp-cube-")
        .suffix(".gcode")
        .tempfile()
        .expect("gcode temp");
    let summary = slice_cube(fixture_catalog(), Box::new(FakeEngine), destination.path()).await;
    assert_eq!(summary["layer_count"], 100);
}

#[tokio::test]
async fn real_engine_slices_the_20mm_cube_when_configured() {
    if std::env::var_os("SHALE_CURAENGINE").is_none()
        || std::env::var_os("SHALE_CURA_DIR").is_none()
    {
        return;
    }
    ensure_test_cache();
    let destination = tempfile::Builder::new()
        .prefix("shale-mcp-real-")
        .suffix(".gcode")
        .tempfile()
        .expect("gcode temp");
    let engine = CliEngine::from_env();
    assert!(engine.available(), "CuraEngine did not answer help");
    let catalog = Catalog::open(ResourcePaths::discover().expect("resources")).expect("catalog");
    let summary = slice_cube(catalog, Box::new(engine), destination.path()).await;
    assert_eq!(summary["layer_count"], 100, "{summary}");
}
