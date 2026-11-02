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
        on_progress(EngineProgress {
            fraction: 0.4,
            message: format!(
                "Processing insets for layer 1 of {}",
                request.estimated_layers
            ),
        });
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
        on_progress(EngineProgress {
            fraction: 0.93,
            message: "starting export".into(),
        });
        Ok(EngineOutput {
            gcode_path: request.output_gcode.clone(),
            log_tail: "Total print time: 120\n".into(),
            print_time_seconds: Some(120.0),
        })
    }
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
        "export_gcode",
    ] {
        assert!(
            tools.iter().any(|tool| tool.name == name),
            "missing tool {name}"
        );
    }

    let printers = call(&client, "list_printers", json!({})).await;
    assert!(
        !printers.as_array().unwrap().is_empty(),
        "the catalog has no printers"
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
async fn in_process_client_slices_the_20mm_cube() {
    let destination =
        std::env::temp_dir().join(format!("shale-mcp-cube-{}.gcode", std::process::id()));
    let summary = slice_cube(fixture_catalog(), Box::new(FakeEngine), &destination).await;
    assert_eq!(summary["layer_count"], 100);
    let _ = std::fs::remove_file(&destination);
}

#[tokio::test]
async fn real_engine_slices_the_20mm_cube_when_configured() {
    if std::env::var_os("SHALE_CURAENGINE").is_none()
        || std::env::var_os("SHALE_CURA_DIR").is_none()
    {
        return;
    }
    let destination =
        std::env::temp_dir().join(format!("shale-mcp-real-{}.gcode", std::process::id()));
    let engine = CliEngine::from_env();
    assert!(engine.available(), "CuraEngine did not answer help");
    let catalog = Catalog::open(ResourcePaths::discover().expect("resources")).expect("catalog");
    let summary = slice_cube(catalog, Box::new(engine), &destination).await;
    assert_eq!(summary["layer_count"], 100, "{summary}");
    let _ = std::fs::remove_file(&destination);
}
