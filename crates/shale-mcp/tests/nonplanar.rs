// SPDX-License-Identifier: AGPL-3.0-or-later
//! The non-planar surface of the MCP tools: input validation that does not need
//! CuraEngine, and one end-to-end plan against the real catalogue and the demo
//! part, which returns early when the engine environment is not configured.

use std::path::PathBuf;

use rmcp::model::{CallToolRequestParams, CallToolResult, ContentBlock};
use rmcp::service::RunningService;
use rmcp::ServiceExt;
use serde_json::{json, Value};
use shale_core::{Catalog, CliEngine, ResourcePaths, Session};
use shale_mcp::ShaleMcp;

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

fn demo_part() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples/nonplanar-demo.stl")
}

async fn serve(session: Session) -> RunningService<rmcp::RoleClient, ()> {
    let server = ShaleMcp::new(session);
    let (server_io, client_io) = tokio::io::duplex(1024 * 1024);
    tokio::spawn(async move {
        let running = server.serve(server_io).await.expect("server handshake");
        let _ = running.waiting().await;
    });
    ().serve(client_io).await.expect("client handshake")
}

fn params(name: &str, arguments: Value) -> CallToolRequestParams {
    let mut params = CallToolRequestParams::new(name.to_string());
    if let Some(arguments) = arguments.as_object().cloned() {
        params = params.with_arguments(arguments);
    }
    params
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
    client: &RunningService<rmcp::RoleClient, ()>,
    name: &str,
    arguments: Value,
) -> Value {
    let result = client
        .call_tool(params(name, arguments))
        .await
        .unwrap_or_else(|err| panic!("{name} failed: {err}"));
    tool_json(result)
}

/// The message a refused call carries; the tool reports problems as errors so a
/// caller cannot mistake a clamped request for an accepted one.
async fn call_error(
    client: &RunningService<rmcp::RoleClient, ()>,
    name: &str,
    arguments: Value,
) -> String {
    match client.call_tool(params(name, arguments)).await {
        Ok(result) => panic!("{name} should have failed, got {result:?}"),
        Err(err) => err.to_string(),
    }
}

#[tokio::test]
async fn tool_refuses_nonplanar_values_it_cannot_honour() {
    ensure_test_cache();
    let client = serve(Session::new(
        fixture_catalog(),
        Box::new(CliEngine::from_env()),
    ))
    .await;

    let message = call_error(
        &client,
        "set_settings",
        json!({ "nonplanar_mode": "curvy" }),
    )
    .await;
    assert!(message.contains("unknown nonplanar_mode"), "{message}");
    assert!(message.contains("curved-tops"), "{message}");

    let message = call_error(
        &client,
        "set_settings",
        json!({ "nonplanar_max_angle_deg": 12.0 }),
    )
    .await;
    assert!(message.contains("at most 10"), "{message}");

    // A Bambu printer that ships without a Shale toolhead envelope: a curved
    // mode has to be refused rather than silently made planar, and the message
    // has to name the printers that would work.
    let message = call_error(
        &client,
        "set_settings",
        json!({ "printer_id": "bambu_lab_h2s", "nonplanar_mode": "auto" }),
    )
    .await;
    assert!(message.contains("toolhead envelope"), "{message}");
    assert!(message.contains("bambu_lab_a1"), "{message}");

    // Planar slicing stays available, and the view says so.
    let view = call(&client, "set_settings", json!({ "nonplanar_mode": "off" })).await;
    assert_eq!(view["nonplanar"]["mode"], "off");
    assert!(view["nonplanar"]["supported_by_printer"].is_boolean());
    let view = call(&client, "get_settings", json!({})).await;
    assert_eq!(view["nonplanar"]["max_angle_deg"], 10.0);
    assert_eq!(view["nonplanar"]["strict"], false);

    client.cancel().await.ok();
}

#[tokio::test]
async fn engine_plans_curved_tops_for_the_demo_part_when_configured() {
    if std::env::var_os("SHALE_CURAENGINE").is_none()
        || std::env::var_os("SHALE_CURA_DIR").is_none()
    {
        return;
    }
    ensure_test_cache();
    let catalog = Catalog::open(ResourcePaths::discover().expect("resources")).expect("catalog");
    let client = serve(Session::new(catalog, Box::new(CliEngine::from_env()))).await;

    let view = call(
        &client,
        "set_settings",
        json!({
            "printer_id": "bambu_lab_a1",
            "nonplanar_mode": "auto",
            "nonplanar_max_angle_deg": 8.0,
        }),
    )
    .await;
    assert_eq!(view["nonplanar"]["mode"], "auto");
    assert_eq!(view["nonplanar"]["max_angle_deg"], 8.0);
    assert_eq!(view["nonplanar"]["supported_by_printer"], true);

    let loaded = call(&client, "load_model", json!({ "path": demo_part() })).await;
    let id = loaded["id"].as_u64().expect("model id");

    let plan = call(&client, "plan_nonplanar", json!({ "id": id })).await;
    let features = plan["detected_features"]
        .as_array()
        .unwrap_or_else(|| panic!("detected_features: {plan}"));
    assert!(
        !features.is_empty(),
        "the demo lid has a curved top: {plan}"
    );
    let enabled = plan["enabled"]
        .as_array()
        .unwrap_or_else(|| panic!("enabled: {plan}"));
    assert!(
        !enabled.is_empty(),
        "auto should curve the demo lid: {plan}"
    );
    for surface in enabled {
        let tilt = surface["tilt_deg"].as_f64().expect("tilt");
        assert!(
            tilt.abs() <= 8.0 + 1e-9,
            "tilt {tilt} above the cap: {plan}"
        );
        assert!(surface["kind"].is_string(), "{plan}");
    }
    for surface in plan["disabled"].as_array().expect("disabled") {
        assert!(
            surface["reason"]
                .as_str()
                .is_some_and(|why| !why.is_empty()),
            "a disabled surface must say why: {plan}"
        );
    }
    assert!(
        plan["preview_triangles"].as_u64().is_some(),
        "preview is reported as a count: {plan}"
    );
    assert!(plan.get("preview").is_none(), "no float arrays: {plan}");

    client.cancel().await.ok();
}
