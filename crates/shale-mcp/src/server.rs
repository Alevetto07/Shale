use std::sync::{Arc, Mutex};

use rmcp::{
    handler::server::{
        router::{prompt::PromptRouter, tool::ToolRouter},
        wrapper::Parameters,
    },
    model::*,
    prompt, prompt_handler, prompt_router, schemars,
    service::RequestContext,
    tool, tool_handler, tool_router, ErrorData as McpError, RoleServer, ServerHandler,
};
use serde::Deserialize;
use serde_json::json;
use shale_core::{
    CustomInstruction, EngineProgress, Session, SettingsPatch, SupportParams, SupportPlan,
};

#[derive(Clone)]
pub struct ShaleMcp {
    session: Arc<Mutex<Session>>,
    // The handler macros read these. Derived Clone hides that from dead-code analysis.
    #[allow(dead_code)]
    tool_router: ToolRouter<Self>,
    #[allow(dead_code)]
    prompt_router: PromptRouter<Self>,
}

impl ShaleMcp {
    pub fn new(session: Session) -> Self {
        Self {
            session: Arc::new(Mutex::new(session)),
            tool_router: Self::tool_router(),
            prompt_router: Self::prompt_router(),
        }
    }

    fn lock(&self) -> Result<std::sync::MutexGuard<'_, Session>, McpError> {
        self.session
            .lock()
            .map_err(|_| McpError::internal_error("Shale session lock was poisoned", None))
    }
}

fn ok_json(value: impl serde::Serialize) -> Result<CallToolResult, McpError> {
    let text = serde_json::to_string_pretty(&value)
        .map_err(|err| McpError::internal_error(err.to_string(), None))?;
    Ok(CallToolResult::success(vec![ContentBlock::text(text)]))
}

fn fail(err: impl std::fmt::Display) -> McpError {
    McpError::internal_error(err.to_string(), None)
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct PrinterArgs {
    /// Printer definition id, such as `creality_ender3`. Omit to use the current printer.
    printer_id: Option<String>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct LoadModelArgs {
    /// Filesystem path to an STL or 3MF file.
    path: String,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct TransformArgs {
    id: u32,
    translation: Option<[f64; 3]>,
    rotation_deg: Option<[f64; 3]>,
    scale: Option<[f64; 3]>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct ModelIdArgs {
    id: u32,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct ExplainArgs {
    /// Setting keys to explain. Empty returns the usual slice settings.
    #[serde(default)]
    keys: Vec<String>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct ExportArgs {
    /// Destination path for the G-code file.
    path: String,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct RemoveArgs {
    id: u32,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct SettingsArgs {
    printer_id: Option<String>,
    variant_name: Option<String>,
    quality_type: Option<String>,
    material_id: Option<String>,
    layer_height: Option<f64>,
    infill_sparse_density: Option<f64>,
    support_enable: Option<bool>,
    material_print_temperature: Option<f64>,
    material_bed_temperature: Option<f64>,
}

impl From<SettingsArgs> for SettingsPatch {
    fn from(args: SettingsArgs) -> Self {
        Self {
            printer_id: args.printer_id,
            variant_name: args.variant_name,
            quality_type: args.quality_type,
            material_id: args.material_id,
            layer_height: args.layer_height,
            infill_sparse_density: args.infill_sparse_density,
            support_enable: args.support_enable,
            material_print_temperature: args.material_print_temperature,
            material_bed_temperature: args.material_bed_temperature,
        }
    }
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct InstructionArgs {
    /// pause, temperature, fan, beep, message, or park_and_wait.
    kind: String,
    layer: Option<u32>,
    height_mm: Option<f64>,
    filament_change: Option<bool>,
    message: Option<String>,
    nozzle_c: Option<f64>,
    bed_c: Option<f64>,
    wait: Option<bool>,
    percent: Option<f64>,
    text: Option<String>,
    x: Option<f64>,
    y: Option<f64>,
}

impl InstructionArgs {
    fn into_instruction(self) -> Result<CustomInstruction, McpError> {
        match self.kind.as_str() {
            "pause" => Ok(CustomInstruction::Pause {
                layer: self.layer,
                height_mm: self.height_mm,
                filament_change: self.filament_change.unwrap_or(false),
                message: self.message,
            }),
            "temperature" => Ok(CustomInstruction::Temperature {
                layer: self
                    .layer
                    .ok_or_else(|| fail("temperature needs a layer"))?,
                nozzle_c: self.nozzle_c,
                bed_c: self.bed_c,
                wait: self.wait.unwrap_or(false),
            }),
            "fan" => Ok(CustomInstruction::Fan {
                height_mm: self.height_mm.ok_or_else(|| fail("fan needs height_mm"))?,
                percent: self.percent.ok_or_else(|| fail("fan needs percent"))?,
            }),
            "beep" => Ok(CustomInstruction::Beep {
                layer: self.layer.ok_or_else(|| fail("beep needs a layer"))?,
            }),
            "message" => Ok(CustomInstruction::Message {
                layer: self.layer.ok_or_else(|| fail("message needs a layer"))?,
                text: self.text.ok_or_else(|| fail("message needs text"))?,
            }),
            "park_and_wait" => Ok(CustomInstruction::ParkAndWait {
                layer: self
                    .layer
                    .ok_or_else(|| fail("park_and_wait needs a layer"))?,
                x: self.x,
                y: self.y,
                message: self.message,
            }),
            other => Err(fail(format!("unknown instruction kind {other}"))),
        }
    }
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct SupportQuery {
    id: u32,
    /// Cura's overhang angle, from the vertical. 50 is the usual default.
    overhang_angle_deg: Option<f64>,
    /// Spans up to this length, held at both ends, are bridges and are not supported.
    max_bridge_mm: Option<f64>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct ApplySupportArgs {
    /// Region ids to keep. Omit to enable every proposed region.
    ids: Option<Vec<u32>>,
}

fn support_params(query: &SupportQuery) -> SupportParams {
    let mut params = SupportParams::default();
    if let Some(value) = query.overhang_angle_deg {
        params.overhang_angle_deg = value;
    }
    if let Some(value) = query.max_bridge_mm {
        params.max_bridge_mm = value;
    }
    params
}

fn plan_for_tool(plan: &SupportPlan) -> SupportPlan {
    let mut plan = plan.clone();
    for span in &mut plan.spans {
        span.samples.clear();
    }
    plan
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct SlicePromptArgs {
    /// What the part is, in a few words.
    part: String,
}

#[tool_router]
impl ShaleMcp {
    #[tool(description = "List printers available from the fetched Cura definitions.")]
    fn list_printers(&self) -> Result<CallToolResult, McpError> {
        let session = self.lock()?;
        ok_json(session.list_printers())
    }

    #[tool(description = "List materials whose diameter matches the current or given printer.")]
    fn list_materials(
        &self,
        Parameters(PrinterArgs { printer_id }): Parameters<PrinterArgs>,
    ) -> Result<CallToolResult, McpError> {
        let session = self.lock()?;
        ok_json(
            session
                .list_materials(printer_id.as_deref())
                .map_err(fail)?,
        )
    }

    #[tool(description = "List nozzle variants and quality profiles for a printer.")]
    fn list_profiles(
        &self,
        Parameters(PrinterArgs { printer_id }): Parameters<PrinterArgs>,
    ) -> Result<CallToolResult, McpError> {
        let session = self.lock()?;
        let (variants, qualities) = session.list_profiles(printer_id.as_deref()).map_err(fail)?;
        ok_json(json!({ "variants": variants, "qualities": qualities }))
    }

    #[tool(description = "Load an STL or 3MF from a filesystem path and place it on the bed.")]
    fn load_model(
        &self,
        Parameters(LoadModelArgs { path }): Parameters<LoadModelArgs>,
    ) -> Result<CallToolResult, McpError> {
        let mut session = self.lock()?;
        ok_json(
            session
                .load_model(std::path::Path::new(&path))
                .map_err(fail)?,
        )
    }

    #[tool(
        description = "Set translation (mm), XYZ rotation (degrees), or scale for a loaded model. Omitted fields stay as they are."
    )]
    fn transform_model(
        &self,
        Parameters(TransformArgs {
            id,
            translation,
            rotation_deg,
            scale,
        }): Parameters<TransformArgs>,
    ) -> Result<CallToolResult, McpError> {
        let mut session = self.lock()?;
        ok_json(
            session
                .transform_model(id, translation, rotation_deg, scale)
                .map_err(fail)?,
        )
    }

    #[tool(description = "Centre a model on the build plate and drop it onto Z = 0.")]
    fn arrange_model(
        &self,
        Parameters(ModelIdArgs { id }): Parameters<ModelIdArgs>,
    ) -> Result<CallToolResult, McpError> {
        let mut session = self.lock()?;
        ok_json(session.arrange_model(id).map_err(fail)?)
    }

    #[tool(
        description = "Resolved setting values for the current job, with Cura's label and description where the definition has them."
    )]
    fn get_settings(
        &self,
        Parameters(ExplainArgs { keys }): Parameters<ExplainArgs>,
    ) -> Result<CallToolResult, McpError> {
        let session = self.lock()?;
        ok_json(session.get_settings(&keys).map_err(fail)?)
    }

    #[tool(
        description = "Change the printer, profile, or the key slice settings (layer height, infill, supports, temperatures). Only fields you send are updated."
    )]
    fn set_settings(
        &self,
        Parameters(patch): Parameters<SettingsArgs>,
    ) -> Result<CallToolResult, McpError> {
        let mut session = self.lock()?;
        session.set_settings(patch.into()).map_err(fail)?;
        ok_json(session.get_settings(&[]).map_err(fail)?)
    }

    #[tool(
        description = "Analyse one loaded model for overhangs, bridges, cantilevers, and islands. This does not change the job. Bridge and cantilever spans are the same map a later non-planar placement can use."
    )]
    fn analyse_overhangs(
        &self,
        Parameters(query): Parameters<SupportQuery>,
    ) -> Result<CallToolResult, McpError> {
        let session = self.lock()?;
        let analysis = session
            .analyse_overhangs(query.id, support_params(&query))
            .map_err(fail)?;
        let spans: Vec<_> = analysis
            .spans
            .iter()
            .map(|span| {
                json!({
                    "kind": span.kind,
                    "length_mm": span.length_mm,
                    "width_mm": span.width_mm,
                    "area_mm2": span.area_mm2,
                    "z_mm": span.z_mm,
                    "angle_from_vertical_deg": span.angle_from_vertical_deg,
                    "anchor_ends": span.anchor_ends,
                    "samples": span.samples.len(),
                })
            })
            .collect();
        ok_json(json!({
            "bridges": analysis.count(shale_core::FeatureKind::Bridge),
            "cantilevers": analysis.count(shale_core::FeatureKind::Cantilever),
            "islands": analysis.count(shale_core::FeatureKind::Island),
            "overhangs": analysis.count(shale_core::FeatureKind::Overhang),
            "cosmetic": analysis.count(shale_core::FeatureKind::Cosmetic),
            "spans": spans,
        }))
    }

    #[tool(
        description = "Propose support only where the analysis says it is needed. Bridges and cosmetic slopes become blockers. Cantilevers, islands, and other overhangs become support-mesh columns. Times are a bead estimate; volume and contact area are geometric, compared with supporting every overhang."
    )]
    fn propose_supports(
        &self,
        Parameters(query): Parameters<SupportQuery>,
    ) -> Result<CallToolResult, McpError> {
        let mut session = self.lock()?;
        let plan = session
            .propose_supports(query.id, support_params(&query))
            .map_err(fail)?;
        ok_json(plan_for_tool(&plan))
    }

    #[tool(
        description = "Enable a subset of the proposed support regions. Omit ids to enable them all. The next slice sends the enabled columns and blockers to CuraEngine as support_mesh and anti_overhang_mesh objects."
    )]
    fn apply_supports(
        &self,
        Parameters(ApplySupportArgs { ids }): Parameters<ApplySupportArgs>,
    ) -> Result<CallToolResult, McpError> {
        let mut session = self.lock()?;
        let plan = session.apply_supports(ids.as_deref()).map_err(fail)?;
        ok_json(plan_for_tool(&plan))
    }

    #[tool(
        description = "List custom instructions stored on this job. They are compiled into G-code after CuraEngine runs."
    )]
    fn list_instructions(&self) -> Result<CallToolResult, McpError> {
        let session = self.lock()?;
        ok_json(session.instructions())
    }

    #[tool(
        description = "Add a custom instruction. Kinds: pause (layer or height_mm, filament_change, message), temperature (layer, nozzle_c, bed_c, wait), fan (height_mm, percent), beep (layer), message (layer, text), park_and_wait (layer, x, y, message)."
    )]
    fn add_instruction(
        &self,
        Parameters(args): Parameters<InstructionArgs>,
    ) -> Result<CallToolResult, McpError> {
        let instruction = args.into_instruction()?;
        let mut session = self.lock()?;
        ok_json(session.add_instruction(instruction))
    }

    #[tool(description = "Remove a custom instruction by the id returned from add_instruction.")]
    fn remove_instruction(
        &self,
        Parameters(RemoveArgs { id }): Parameters<RemoveArgs>,
    ) -> Result<CallToolResult, McpError> {
        let mut session = self.lock()?;
        session.remove_instruction(id).map_err(fail)?;
        ok_json(session.instructions())
    }

    #[tool(
        description = "Slice the loaded models with CuraEngine, then compile custom instructions into the G-code. Returns time, filament, layer count, and warnings."
    )]
    async fn slice(&self) -> Result<CallToolResult, McpError> {
        let session = Arc::clone(&self.session);
        let summary = tokio::task::spawn_blocking(move || {
            let mut session = session
                .lock()
                .map_err(|_| McpError::internal_error("Shale session lock was poisoned", None))?;
            session
                .slice(&mut |progress: EngineProgress| {
                    tracing::info!(fraction = progress.fraction, "{}", progress.message);
                })
                .map_err(fail)
        })
        .await
        .map_err(fail)??;
        ok_json(summary)
    }

    #[tool(description = "Return the summary of the latest slice without slicing again.")]
    fn slice_summary(&self) -> Result<CallToolResult, McpError> {
        let session = self.lock()?;
        ok_json(
            session
                .summary()
                .ok_or_else(|| fail("nothing has been sliced yet"))?,
        )
    }

    #[tool(description = "Copy the latest G-code to a filesystem path.")]
    fn export_gcode(
        &self,
        Parameters(ExportArgs { path }): Parameters<ExportArgs>,
    ) -> Result<CallToolResult, McpError> {
        let session = self.lock()?;
        let written = session
            .export_gcode(std::path::Path::new(&path))
            .map_err(fail)?;
        ok_json(json!({ "path": written }))
    }
}

#[prompt_router]
impl ShaleMcp {
    /// Steps an agent should follow to slice a part.
    #[prompt(name = "slice_a_model")]
    fn slice_a_model(
        &self,
        Parameters(SlicePromptArgs { part }): Parameters<SlicePromptArgs>,
        _ctx: RequestContext<RoleServer>,
    ) -> Result<GetPromptResult, McpError> {
        let text = format!(
            "Slice \"{part}\" with Shale. Use list_printers, list_profiles, and list_materials, then set_settings. load_model from a filesystem path, arrange_model, add any pause or temperature instructions, then slice. Read slice_summary before export_gcode. Do not edit the G-code by hand; custom instructions are the way to pause, change temperature, beep, or park."
        );
        Ok(GetPromptResult::new(vec![PromptMessage::new_text(
            Role::User,
            text,
        )]))
    }
}

#[tool_handler]
#[prompt_handler]
impl ServerHandler for ShaleMcp {
    fn get_info(&self) -> ServerConfig {
        ServerConfig::new(
            ServerCapabilities::builder()
                .enable_tools()
                .enable_prompts()
                .enable_resources()
                .build(),
        )
        .with_server_info(Implementation::from_build_env())
        .with_instructions(
            "Shale drives CuraEngine. Tools share the desktop app's core: printers, materials, profiles, models, resolved settings, custom instructions, slice, and G-code export. The engine binary is configured with SHALE_CURAENGINE.".to_string(),
        )
    }

    async fn list_resources(
        &self,
        _request: Option<PaginatedRequestParams>,
        _: RequestContext<RoleServer>,
    ) -> Result<ListResourcesResult, McpError> {
        Ok(ListResourcesResult {
            resources: vec![Resource::new("shale://job", "Current Shale job")],
            ..Default::default()
        })
    }

    async fn read_resource(
        &self,
        request: ReadResourceRequestParams,
        _: RequestContext<RoleServer>,
    ) -> Result<ReadResourceResponse, McpError> {
        if request.uri.as_str() != "shale://job" {
            return Err(McpError::resource_not_found(
                "unknown resource",
                Some(json!({ "uri": request.uri })),
            ));
        }
        let session = self.lock()?;
        let body = json!({
            "models": session.models(),
            "instructions": session.instructions(),
            "support": session.support_plan().map(plan_for_tool),
            "summary": session.summary(),
        });
        Ok(ReadResourceResult::new(vec![ResourceContents::text(
            body.to_string(),
            request.uri.clone(),
        )])
        .into())
    }
}
