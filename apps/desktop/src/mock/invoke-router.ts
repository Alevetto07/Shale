import {
  cubeGeometryBuffer,
  mockBed,
  mockPlates,
  mockCloud,
  mockCloudDevices,
  mockCostEstimate,
  mockCostSettings,
  mockHost,
  mockModel,
  mockPreviewPaths,
  mockRecents,
  mockSlice,
  mockStatus,
  MOCK_MATERIALS,
  MOCK_PRINTERS,
  MOCK_QUALITIES,
  MOCK_VARIANTS,
} from "./data";
import { readMockFlags } from "./scenario";

const flags = readMockFlags();
const models = flags.models ? [mockModel(1)] : [];
let cloudSession = mockCloud(flags.cloudSignedIn);
let sliceSummary = flags.sliced ? mockSlice(models.map((m) => m.id)) : null;

export async function mockInvoke<T>(cmd: string, args: Record<string, unknown>): Promise<T> {
  switch (cmd) {
    case "app_status":
      return mockStatus() as T;
    case "startup_open":
    case "take_pending_open":
      return null as T;
    case "list_printers":
      return MOCK_PRINTERS as T;
    case "retry_catalog_load":
      return undefined as T;
    case "list_variants":
    case "list_qualities":
    case "list_materials":
      return (cmd === "list_variants"
        ? MOCK_VARIANTS
        : cmd === "list_qualities"
          ? MOCK_QUALITIES
          : MOCK_MATERIALS) as T;
    case "printer_bed":
      return mockBed() as T;
    case "profile_settings":
      return {
        layer_height: "0.2",
        wall_line_count: "2",
        infill_sparse_density: "15",
        infill_pattern: "grid",
        support_enable: "0",
        material_print_temperature: "220",
        material_bed_temperature: "60",
      } as T;
    case "settings_help":
      return (args.keys as string[]).map((key) => ({
        key,
        label: key.replaceAll("_", " "),
        description: "Mock setting description for UI capture.",
      })) as T;
    case "loaded_models":
      return models as T;
    case "list_plates":
      return mockPlates(models.length) as T;
    case "plate_fit":
      return { plate_index: 0, plate_name: "plate 1", x: 0, y: 0, z: 0 } as T;
    case "imported_settings":
      return null as T;
    case "add_plate":
    case "switch_plate":
      return { plates: mockPlates(models.length), models } as T;
    case "delete_plate":
    case "rename_plate":
      return mockPlates(models.length) as T;
    case "existing_slice":
      return sliceSummary as T;
    case "list_instructions":
      return [] as T;
    case "current_support":
      return null as T;
    case "list_recents":
      return (flags.recents ? mockRecents() : []) as T;
    case "forget_recent_project":
      return [] as T;
    case "cost_settings":
      return mockCostSettings() as T;
    case "set_cost_settings":
      return (args.settings ?? mockCostSettings()) as T;
    case "cost_estimate":
      return mockCostEstimate() as T;
    case "preset_list":
      return [
        { name: "0.20mm Standard", values: { layer_height: "0.2" } },
        { name: "benchy-draft", values: { layer_height: "0.28", infill_sparse_density: "10" } },
      ] as T;
    case "preset_save":
    case "preset_rename":
    case "preset_import":
      return { name: String(args.name ?? args.to ?? "preset"), values: {} } as T;
    case "preset_delete":
    case "preset_export":
    case "cancel_import":
    case "import_model":
      return [mockModel(1)] as T;
    case "clear_job":
    case "secret_set":
    case "secret_clear":
    case "cloud_sign_out":
    case "cloud_verify":
    case "host_disconnect":
    case "host_control":
    case "host_forget":
    case "host_forget_pin":
      return undefined as T;
    case "secret_get":
    case "host_pin":
      return null as T;
    case "set_engine":
      return true as T;
    case "model_geometry":
      return cubeGeometryBuffer() as T;
    case "preview_all":
      return mockPreviewPaths() as T;
    case "preview_until":
      return mockPreviewPaths().filter((path) => path.layer <= Number(args.layer ?? 0)) as T;
    case "overhang_patches":
      return [] as T;
    case "mesh_health":
      return { triangles: 12, openEdges: 0, volumeMm3: 8000 } as T;
    case "nonplanar_supported":
      return true as T;
    case "slice_model":
      sliceSummary = mockSlice(models.map((m) => m.id));
      return sliceSummary as T;
    case "slice_tools":
      return {
        tools: [0],
        changes: 0,
        file: "C:\\mock\\plate.gcode",
        usage: [{ tool: 0, metres: 4.2 }],
      } as T;
    case "export_gcode":
      return String(args.destination ?? "C:\\mock\\plate.gcode") as T;
    case "host_defaults":
      return {
        host: "192.168.1.42",
        serial: "01S00A422400123",
        code: "",
        bedType: "textured_plate",
      } as T;
    case "host_connect":
      return mockHost(true) as T;
    case "host_status":
      return mockHost(flags.hostConnected) as T;
    case "cloud_state":
      return cloudSession as T;
    case "cloud_set_region":
      cloudSession = {
        ...cloudSession,
        signedIn: false,
        host:
          String(args.region) === "China" || String(args.region) === "CN"
            ? "https://api.bambulab.cn"
            : "https://api.bambulab.com",
      };
      return cloudSession as T;
    case "cloud_login":
      cloudSession = mockCloud(false);
      return { needsCode: true, tfaKey: "mock-tfa" } as T;
    case "cloud_devices":
      return (cloudSession.signedIn ? mockCloudDevices() : []) as T;
    case "cloud_send":
      return "mock-task-001" as T;
    case "queue_list":
      return [] as T;
    case "queue_add":
    case "queue_remove":
    case "queue_clear":
    case "queue_release":
    case "queue_plate_cleared":
      return [] as T;
    case "queue_start_next":
      return { name: "", remaining: 0, queue: [] } as T;
    case "queue_run_state":
      return { armed: false, running: false, waiting_for_plate: false, message: "" } as T;
    case "queue_arm":
    case "queue_observe":
    case "queue_stop":
      return { armed: false, running: false, waiting_for_plate: false, message: "" } as T;
    case "host_send":
      return { remote: "mock.gcode", bytes: 522_285 } as T;
    case "scratch_path":
      return `C:\\mock\\scratch\\${String(args.name ?? "untitled")}.shale` as T;
    case "save_project":
      return mockRecents() as T;
    case "open_project":
      return {
        path: String(args.path),
        name: "arm-bracket",
        printer_id: "bambu_lab_a1",
        printer_label: "Bambu Lab A1",
        variant_name: "0.4mm Nozzle",
        quality_type: "normal",
        material_id: "bambu_lab_pla_basic",
        ams: ["", "", ""],
        settings: {
          layer_height: 0.2,
          infill_sparse_density: 15,
          support_enable: false,
          material_print_temperature: 220,
          material_bed_temperature: 60,
          extra: [],
        },
        chord_mm: 0.1,
        turn_deg: 15,
        models,
        instructions: [],
        support: null,
        warnings: [],
      } as T;
    case "search_models":
      return {
        total: 1,
        hits: [
          {
            library: args.library ?? "makerworld",
            id: "1400373",
            title: "Cable clip",
            creator: "Ada Maker",
            creator_url: "https://makerworld.com/en/@ada",
            licence: "Creative Commons Attribution-NonCommercial 4.0",
            thumbnail_url: null,
            page_url: "https://makerworld.com/en/models/1400373-cable-clip",
            likes: 12,
            downloads: 40,
            nsfw: false,
          },
        ],
      } as T;
    case "model_detail":
      return {
        card: {
          library: args.library ?? "makerworld",
          id: "1400373",
          title: "Cable clip",
          creator: "Ada Maker",
          creator_url: "https://makerworld.com/en/@ada",
          licence: "Creative Commons Attribution-NonCommercial 4.0",
          thumbnail_url: null,
          page_url: "https://makerworld.com/en/models/1400373-cable-clip",
          likes: 12,
          downloads: 40,
          nsfw: false,
        },
        summary: "A clip for a desk edge, with the print profiles the author published.",
        profiles: [{ id: "4242", name: "0.20 mm A1", printer: "Bambu Lab A1" }],
        files: args.library === "thingiverse" ? [{ id: "55", name: "octopus.stl", kind: "stl", bytes: 128 }] : [],
        remote_model_id: "US300293b2858c3b",
      } as T;
    case "download_model":
    case "import_model_url":
      return {
        path: "/tmp/shale-mock-clip.3mf",
        file_name: "Cable_clip.3mf",
        title: "Cable clip",
        creator: "Ada Maker",
        licence: "Creative Commons Attribution-NonCommercial 4.0",
        page_url: "https://makerworld.com/en/models/1400373-cable-clip",
        library: "makerworld",
      } as T;
    case "thingiverse_token_status":
      return { configured: false } as T;
    case "thingiverse_token_set":
      return { configured: true } as T;
    case "thingiverse_token_clear":
      return { configured: false } as T;
    case "open_model_link":
      return undefined as T;
    default:
      return null as T;
  }
}

/** Playwright can flip cloud sign-in between captures. */
export function mockSetCloudSignedIn(value: boolean) {
  cloudSession = mockCloud(value);
}