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
import chamberUrl from "./chamber.jpg";
import { readMockFlags } from "./scenario";

/** A synthetic chamber picture. Generated for the mock; it is not a photograph. */
function chamberBase64(): Promise<string> {
  return fetch(chamberUrl)
    .then((response) => response.arrayBuffer())
    .then((buffer) => {
      const bytes = new Uint8Array(buffer);
      let binary = "";
      for (const byte of bytes) binary += String.fromCharCode(byte);
      return btoa(binary);
    });
}

function cameraQuery(name: string): string | null {
  if (typeof window === "undefined") return null;
  return new URLSearchParams(window.location.search).get(name);
}

function cameraFault(): boolean {
  return cameraQuery("camera") === "fault";
}

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
      return { state: "idle", note: null } as T;
    case "queue_arm":
      return { state: "armed", note: null } as T;
    case "queue_observe":
    case "queue_stop":
    case "queue_set_eject":
    case "queue_cooled":
    case "queue_plate_verdict":
    case "removal_send":
      return { state: "idle", note: null } as T;
    case "camera_start":
    case "camera_status":
      if (cameraFault()) {
        return {
          running: false,
          path: "jpeg",
          frames: 0,
          fps: 0,
          hasJpeg: false,
          codec: "",
          faultCode: "lan_live_view_off",
          fault:
            "LAN-only live view is off. On the printer, open the network settings and turn on LAN-only live view. The printer can stay signed in to the cloud.",
        } as T;
      }
      return {
        running: args && true,
        path: String(args.model ?? "").toLowerCase().includes("p2s") ? "rtsps" : "jpeg",
        frames: 12,
        fps: 4,
        hasJpeg: true,
        codec: "jpeg",
        faultCode: null,
        fault: null,
      } as T;
    case "camera_stop":
      return {
        running: false,
        path: "",
        frames: 0,
        fps: 0,
        hasJpeg: false,
        codec: "",
        faultCode: null,
        fault: null,
      } as T;
    case "camera_frame":
    case "camera_snapshot":
      return (cameraFault() ? null : await chamberBase64()) as T;
    case "camera_watch":
      return {
        kind: "spaghetti",
        confidence: 0.82,
        alert: cameraQuery("camera") === "alert",
        action: cameraQuery("camera") === "alert" ? "notify" : "none",
        boxes: [{ x: 0.22, y: 0.3, w: 0.28, h: 0.22, label: "spaghetti", confidence: 0.82 }],
        heatmap: Array.from({ length: 12 * 9 }, (_, index) => (index % 7 === 0 ? 0.45 : 0.05)),
        heatmapCols: 12,
        heatmapRows: 9,
        note: "Stringy strands on the part. This may be spaghetti.",
        detector: "builtin",
      } as T;
    case "camera_codec_status":
      return {
        phase: "missing",
        ready: false,
        fromSource: false,
        credit: "OpenH264 Video Codec provided by Cisco Systems, Inc.",
        message: "Cisco's OpenH264 library is not installed yet. Shale downloads it on first use.",
      } as T;
    case "camera_codec_download":
      return {
        phase: "missing",
        ready: false,
        fromSource: false,
        credit: "OpenH264 Video Codec provided by Cisco Systems, Inc.",
        message: "This preview does not download Cisco's library.",
      } as T;
    case "camera_timelapse_save":
      return {
        frames: Array.isArray(args.frames) ? args.frames.length : 0,
        directory: String(args.folder ?? ""),
        preview: `${String(args.folder ?? "")}/preview.gif`,
        video: null,
        note: "Saved the JPEG sequence and an animated preview. The MP4 needs Cisco's OpenH264 library, which this preview does not download.",
      } as T;
    case "camera_model_status":
    case "camera_model_download":
      return {
        phase: "missing",
        ready: false,
        received: 0,
        total: 202223918,
        message: "The failure model is not downloaded. The built-in detector is in use.",
      } as T;
    case "camera_cloud_still":
      throw new Error(
        "The cloud camera is not available. Bambu's remote picture uses a private relay Shale does not speak. Use the LAN camera instead.",
      );
    case "camera_save_reference":
      return null as T;
    case "camera_compare":
      return {
        clear: true,
        difference: 2.1,
        note: "The plate matches the empty-plate picture.",
      } as T;
    case "removal_preview":
      return {
        offered: String(args.printerId) === "bambu_lab_a1",
        accepted: String(args.printerId) === "bambu_lab_a1",
        reasons:
          String(args.printerId) === "bambu_lab_a1"
            ? []
            : ["Automatic removal is only offered for the A1."],
        gcode:
          String(args.printerId) === "bambu_lab_a1"
            ? "; Shale experimental print removal for the A1\nM140 S0\nM190 S28\n"
            : "",
      } as T;
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
    default:
      return null as T;
  }
}

/** Playwright can flip cloud sign-in between captures. */
export function mockSetCloudSignedIn(value: boolean) {
  cloudSession = mockCloud(value);
}