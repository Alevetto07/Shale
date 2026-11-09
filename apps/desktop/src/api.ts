import { invoke } from "@tauri-apps/api/core";

export interface Status {
  resources_ok: boolean;
  resources_message: string;
  engine_path: string;
  engine_lib_dir: string;
  engine_ok: boolean;
  engine_message: string;
  engine_backend: string;
  printer_count: number;
  demo_model?: string | null;
  /** The profile library is still being read; the window is usable meanwhile. */
  catalog_loading?: boolean;
  version: string;
}

export interface LiveLayer {
  index: number;
  z: number;
  paths: PreviewPath[];
}

export interface PrinterInfo {
  id: string;
  name: string;
  manufacturer: string;
  preferred_quality_type: string;
  preferred_variant: string;
  preferred_material: string;
  /** `cura` for the UltiMaker resource tree, `bambu` for Bambu Studio's own. */
  source: string;
}

export interface VariantInfo {
  name: string;
  nozzle_size: number | null;
}

export interface QualityInfo {
  quality_type: string;
  name: string;
}

export interface MaterialInfo {
  id: string;
  label: string;
  color: string;
  diameter: number;
  /// Grams per cubic centimetre, when the profile says: Bambu's filaments carry
  /// a density, Cura's materials do not.
  density?: number;
}

export interface Transform {
  translation: [number, number, number];
  rotation_deg: [number, number, number];
  scale: [number, number, number];
}

export type Positions = Float32Array | number[];

export interface ModelView {
  id: number;
  name: string;
  positions: Positions;
  transform: Transform;
  triangles: number;
  max_deviation_mm: number | null;
  max_turning_deg: number | null;
  import_ms: number;
  source: string;
  /// Per-part overrides; empty means the part follows the plate settings.
  settings: Record<string, string>;
  /// Which AMS spool prints this part. `null` is the machine's own extruder.
  slot: number | null;
}

export interface PlateView {
  index: number;
  name: string;
  models: number;
  active: boolean;
}

export interface PlateSwitch {
  plates: PlateView[];
  models: ModelView[];
}

export interface CalibrationBand {
  index: number;
  z_mm: number;
  /** °C, percent of flow, or K of pressure advance, depending on `kind`. */
  value: number;
}

export interface TowerBuilt {
  model: ModelView;
  bands: CalibrationBand[];
  band_height_mm: number;
  /** `temperature-tower`, `flow-coupon` or `pressure-tower`. */
  kind: string;
  /** `C`, `%` or `K`. */
  unit: string;
}

export interface CalibrationStep {
  band: number;
  layer: number;
  z_mm: number;
  value: number;
}

export interface CalibrationApplied {
  path: string;
  bytes: number;
  bands: CalibrationBand[];
  injections: CalibrationStep[];
}

/** A contiguous patch of underside: what one brush stroke would reach. */
export interface OverhangPatch {
  triangles: number[];
  area_mm2: number;
  min: [number, number, number];
  max: [number, number, number];
}

/** What is painted on a part: faces that grow support, and faces that keep it out. */
export interface PaintView {
  painted: number;
  blocked: number;
}

/** A measured distance: the line, and how much of it lies on each axis. */
export interface MeasureSpan {
  distance_mm: number;
  delta_mm: [number, number, number];
}

export interface Bed {
  width: number;
  depth: number;
  height: number;
  center_is_zero: boolean;
  layer_height: number;
  infill_sparse_density: number;
  support_enable: boolean;
  nozzle_temperature: number;
  bed_temperature: number;
  nozzle_size: number;
  line_width: number;
  adhesion_type: string;
  speed_print: number;
  material_diameter: number;
  warning_count: number;
}

/// What the printer link last heard. Empty defaults mean "no link".
export interface HostStatus {
  connected: boolean;
  /// `IDLE`, `PREPARE`, `RUNNING`, `PAUSE`, `FINISH`, `FAILED`.
  state: string;
  percent: number;
  remainingMin: number;
  layer: number;
  layers: number;
  nozzle: number;
  nozzleTarget: number;
  bed: number;
  bedTarget: number;
  file: string;
  error: number;
  note: string;
  reports: number;
  updatedAt: number;
}

export interface HostConfig {
  host: string;
  serial: string;
  code: string;
  bedType: string;
  remoteDir: string;
}

/** The printer's access code is a password, so it lives in the OS keyring rather
 * than the WebView profile — which is a plain file anything running as this user
 * can read, and which is backed up with the profile. */
export const secretSet = (key: string, value: string) =>
  invoke<void>("secret_set", { key, value });

export const secretGet = (key: string) => invoke<string | null>("secret_get", { key });

export const secretClear = (key: string) => invoke<void>("secret_clear", { key });

export const hostDefaults = () =>
  invoke<{ host: string; serial: string; code: string; bedType: string }>("host_defaults");

export const hostConnect = (config: HostConfig) => invoke<HostStatus>("host_connect", { config });

export const hostDisconnect = () => invoke<void>("host_disconnect");

export const hostStatus = () => invoke<HostStatus>("host_status");

export const hostSend = (bedType: string) =>
  invoke<{ remote: string; bytes: number }>("host_send", { bedType });

/// What the sliced file needs: which tools it selects, how often, and how much
/// filament each one takes, in metres.
export interface SliceTools {
  tools: number[];
  changes: number;
  file: string | null;
  usage: { tool: number; metres: number }[];
}

export const sliceTools = () => invoke<SliceTools>("slice_tools");

/** A job waiting for the machine. `running` marks the one it is on now. */
export interface QueueJob {
  id: number;
  name: string;
  bytes: number;
  running: boolean;
}

export const queueAdd = (name?: string) => invoke<QueueJob[]>("queue_add", { name });
export const queueList = () => invoke<QueueJob[]>("queue_list");
export const queueRemove = (id: number) => invoke<QueueJob[]>("queue_remove", { id });
export const queueClear = () => invoke<QueueJob[]>("queue_clear");
export const queueRelease = () => invoke<QueueJob[]>("queue_release");
export const queueStartNext = (bedType: string) =>
  invoke<{ name: string; remaining: number; queue: QueueJob[] }>("queue_start_next", { bedType });
/* The run's rules live in shale-core and are enforced in Rust: the pane can ask
   to start as often as it likes, and a refusal is the answer when nobody has
   cleared the plate. */
export type QueueRun = { state: string; note: string | null };
export const queueArm = () => invoke<QueueRun>("queue_arm");
export const queueObserve = (machine: string) => invoke<QueueRun>("queue_observe", { machine });
export const queuePlateCleared = () => invoke<QueueRun>("queue_plate_cleared");
export const queueStop = (why?: string) => invoke<QueueRun>("queue_stop", { why: why ?? null });
export const queueRunState = () => invoke<QueueRun>("queue_run_state");

export const hostControl = (command: "pause" | "resume" | "stop") =>
  invoke<void>("host_control", { command });

/** Forget this printer's certificate pin. The "trust the new certificate"
 *  action: the next connection accepts and records what it is offered. */
export const hostForgetPin = () => invoke<void>("host_forget_pin");

/** The pin this printer is held to, or null when the next connection is a
 *  first sighting. */
export const hostPin = () => invoke<string | null>("host_pin");

export interface SliceSummary {
  layer_count: number;
  layers: { index: number; z: number }[];
  print_time_seconds: number | null;
  filament_metres: number | null;
  warning_count: number;
  gcode_path: string;
  /**
   * The parts this slice was made from. A slice describes the plate it was
   * taken from, so a summary whose parts are not the parts on the plate is a
   * description of some other plate: the shell must not show its numbers.
   */
  model_ids: number[];
  /**
   * The slice was started by the machine's own sequence, not by Cura's generic
   * Marlin start. A Bambu machine refuses anything else.
   */
  machine_gcode: boolean;
}

export interface PreviewPath {
  kind: string;
  points: [number, number, number][];
  /** The feedrate the moves asked for, in mm/s; absent on a travel-less path. */
  feed?: number;
}

export type CustomInstruction =
  | {
      kind: "pause";
      layer?: number | null;
      height_mm?: number | null;
      filament_change?: boolean;
      message?: string | null;
    }
  | {
      kind: "temperature";
      layer: number;
      nozzle_c?: number | null;
      bed_c?: number | null;
      wait?: boolean;
    }
  | { kind: "fan"; height_mm: number; percent: number }
  | { kind: "beep"; layer: number }
  | { kind: "message"; layer: number; text: string }
  | {
      kind: "park_and_wait";
      layer: number;
      x?: number | null;
      y?: number | null;
      message?: string | null;
    };

export interface StoredInstruction {
  id: number;
  summary: string;
  instruction: CustomInstruction;
}

export interface SupportRegion {
  id: number;
  kind: string;
  role: "enforce" | "block";
  enabled: boolean;
  summary: string;
  volume_mm3: number;
  contact_area_mm2: number;
}

export interface SupportComparison {
  proposed_volume_mm3: number;
  proposed_contact_mm2: number;
  proposed_time_seconds: number;
  everywhere_volume_mm3: number;
  everywhere_contact_mm2: number;
  everywhere_time_seconds: number;
  times_from_engine: boolean;
  proposed_support_metres: number | null;
  everywhere_support_metres: number | null;
}

export interface SupportPlan {
  regions: SupportRegion[];
  comparison: SupportComparison;
}

export interface SupportOverlay {
  id: number;
  role: string;
  positions: number[];
}

export interface SupportView {
  plan: SupportPlan;
  overlays: SupportOverlay[];
}

export interface SliceSettings {
  layer_height: number;
  infill_sparse_density: number;
  support_enable: boolean;
  material_print_temperature: number;
  material_bed_temperature: number;
  extra: [string, string][];
}

export const api = {
  secretSet,
  secretGet,
  secretClear,

  status: () => invoke<Status>("app_status"),
  setEngine: (path: string, libDir: string, backend: string) =>
    invoke<boolean>("set_engine", { path, libDir: libDir || null, backend }),
  cancelImport: () => invoke<void>("cancel_import"),
  printers: () => invoke<PrinterInfo[]>("list_printers"),
  variants: (printerId: string) => invoke<VariantInfo[]>("list_variants", { printerId }),
  qualities: (printerId: string) => invoke<QualityInfo[]>("list_qualities", { printerId }),
  materials: (printerId: string) => invoke<MaterialInfo[]>("list_materials", { printerId }),
  bed: (printerId: string, variantName: string | null, qualityType: string | null, materialId: string | null) =>
    invoke<Bed>("printer_bed", { printerId, variantName, qualityType, materialId }),
  // The profile's own numbers, so an untouched field in the sheet reads what the
  // printer would actually use instead of showing nothing.
  profileSettings: (
    printerId: string,
    variantName: string | null,
    qualityType: string | null,
    materialId: string | null
  ) => invoke<Record<string, string>>("profile_settings", { printerId, variantName, qualityType, materialId }),
  // The definition's own sentence for a setting, so the sheet can say what a
  // row means instead of leaving the label to carry it.
  settingsHelp: (printerId: string, keys: string[]) =>
    invoke<{ key: string; label: string | null; description: string | null }[]>("settings_help", {
      printerId,
      keys,
    }),
  importModel: (
    path: string,
    printerId: string | null,
    variantName: string | null,
    qualityType: string | null,
    chordalMm: number,
    angularDeg: number,
  ) =>
    invoke<ModelView[]>("import_model", {
      path,
      printerId,
      variantName,
      qualityType,
      chordalMm,
      angularDeg,
    }),
  updateTransform: (id: number, transform: Transform) => invoke<void>("update_transform", { id, transform }),
  autoOrient: (id: number) => invoke<Transform>("auto_orient", { id }),
  /// Rest the chosen face on the plate. The normal is in world space.
  /** What the part is now: triangles, material, and whether it closes. */
  meshHealth: (id: number) =>
    invoke<{ triangles: number; openEdges: number; volumeMm3: number }>("mesh_health", { id }),
  placeOnFace: (id: number, normal: [number, number, number]) =>
    invoke<Transform>("place_on_face", { id, normal }),
  cut: (
    id: number,
    axis: number,
    offset: number,
    keep: string,
    pins = 0,
    pinRadius = 2,
    pinHeight = 5,
    dovetail = 0,
  ) =>
    invoke<{ views: ModelView[]; joint: string | null }>("cut_model", {
      id,
      axis,
      offset,
      keep,
      pins,
      pinRadius,
      pinHeight,
      dovetail,
    }),
  /* One part against the others: subtract keeps what the others do not cover,
     union and common are the other two answers to the same question. */
  boolean: (id: number, mode: "subtract" | "union" | "common") =>
    invoke<{ views: ModelView[]; consumed: number }>("boolean_model", { id, mode }),
  listPlates: () => invoke<PlateView[]>("list_plates"),
  addPlate: () => invoke<PlateSwitch>("add_plate"),
  switchPlate: (index: number) => invoke<PlateSwitch>("switch_plate", { index }),
  deletePlate: (index: number) => invoke<PlateView[]>("delete_plate", { index }),
  renamePlate: (index: number, name: string) => invoke<PlateView[]>("rename_plate", { index, name }),
  calibrationTower: (printerId: string | null, bands = 5, startC = 230, stepC = 5) =>
    invoke<TowerBuilt>("calibration_tower", { printerId, bands, startC, stepC }),
  flowCoupon: (printerId: string | null, bands = 5, startPct = 105, stepPct = 2.5) =>
    invoke<TowerBuilt>("flow_coupon", { printerId, bands, startPct, stepPct }),
  pressureTower: (printerId: string | null, bands = 5, startK = 0, stepK = 0.01) =>
    invoke<TowerBuilt>("pressure_tower", { printerId, bands, startK, stepK }),
  calibrateSlice: (bands: CalibrationBand[], kind: string | null = null) =>
    invoke<CalibrationApplied>("calibrate_slice", { bands, kind }),
  snapMeasure: (id: number, point: [number, number, number]) =>
    invoke<[number, number, number] | null>("snap_measure", { id, point }),
  measureSpan: (a: [number, number, number], b: [number, number, number]) =>
    invoke<MeasureSpan>("measure_span", { a, b }),
  dropModel: (id: number) => invoke<Transform>("drop_model", { id }),
  duplicateModel: (id: number) => invoke<ModelView>("duplicate_model", { id }),
  splitModel: (id: number) => invoke<ModelView[]>("split_model", { id }),
  /// Copies of one part, stepped across the bed until the plate runs out.
  fillPlate: (id: number, bed: { width: number; depth: number; centerIsZero: boolean }) =>
    invoke<ModelView[]>("fill_plate", {
      id,
      bedWidth: bed.width,
      bedDepth: bed.depth,
      centerIsZero: bed.centerIsZero,
    }),
  /// A solid with no file behind it. `size` is the footprint across x and y in
  /// millimetres (the diameter for a cylinder, cone or sphere).
  addPrimitive: (
    kind: "cube" | "cylinder" | "cone" | "sphere",
    size: number,
    height: number,
    bed: { width: number; depth: number; centerIsZero: boolean },
  ) =>
    invoke<ModelView>("add_primitive", {
      kind,
      size,
      height,
      bedWidth: bed.width,
      bedDepth: bed.depth,
      centerIsZero: bed.centerIsZero,
    }),
  setModelSettings: (id: number, settings: Record<string, string>) =>
    invoke<ModelView>("set_model_settings", { id, settings }),
  /// Which AMS spool prints a part. `null` is the machine's own extruder.
  setModelSlot: (id: number, slot: number | null) =>
    invoke<ModelView>("set_model_slot", { id, slot }),
  removeModel: (id: number) => invoke<void>("remove_model", { id }),
  restoreModels: (models: { id: number; transform: Transform }[]) =>
    invoke<ModelView[]>("restore_models", { models }),
  arrange: (printerId: string | null, variant: string | null, quality: string | null) =>
    invoke<ModelView[]>("arrange_models", {
      printerId,
      variantName: variant,
      qualityType: quality,
    }),
  slice: (
    printerId: string,
    variantName: string | null,
    qualityType: string | null,
    materialId: string | null,
    ams: string[],
    settings: SliceSettings,
  ) =>
    invoke<SliceSummary>("slice_model", {
      printerId,
      variantName,
      qualityType,
      materialId,
      ams,
      settings,
    }),
  supportPlan: () => invoke<SupportView | null>("current_support"),
  overhangPatches: (modelId: number) => invoke<OverhangPatch[]>("overhang_patches", { modelId }),
  /** `block` paints the faces as needing no support; the default holds them up. */
  paintBrush: (
    modelId: number,
    points: Array<[number, number, number]>,
    radius: number,
    block = false,
  ) => invoke<PaintView>("paint_brush", { modelId, points, radiusMm: radius, block }),
  paintSupport: (modelId: number, triangles: number[], block = false) =>
    invoke<PaintView>("paint_support", { modelId, triangles, block }),
  clearPaint: (modelId: number) => invoke<PaintView>("clear_paint", { modelId }),
  // Paint is its own small history: a stroke is triangle indices, so taking one
  // back costs nothing and a brush that cannot be undone is a trap.
  undoPaint: () => invoke<PaintView>("undo_paint"),
  redoPaint: () => invoke<PaintView>("redo_paint"),
  proposeSupports: (id: number, overhangAngleDeg: number, maxBridgeMm: number) =>
    invoke<SupportView>("propose_supports", {
      id,
      overhangAngleDeg,
      maxBridgeMm,
    }),
  applySupports: (ids: number[]) => invoke<SupportView>("apply_supports", { ids }),
  compareSupports: (
    printerId: string,
    variantName: string | null,
    qualityType: string | null,
    materialId: string | null,
    settings: SliceSettings,
  ) =>
    invoke<SupportComparison>("compare_supports", {
      printerId,
      variantName,
      qualityType,
      materialId,
      settings,
    }),
  instructions: () => invoke<StoredInstruction[]>("list_instructions"),
  addInstruction: (instruction: CustomInstruction) =>
    invoke<StoredInstruction>("add_instruction", { instruction }),
  removeInstruction: (id: number) => invoke<StoredInstruction[]>("remove_instruction", { id }),
  previewUntil: (layer: number) => invoke<PreviewPath[]>("preview_until", { layer }),
  exportPlate: (destination: string, format: "stl" | "3mf") =>
    invoke<number>("export_plate", { destination, format }),
  exportGcode: (destination: string) => invoke<string>("export_gcode", { destination }),
  loadedModels: () => invoke<ModelView[]>("loaded_models"),
  existingSlice: () => invoke<SliceSummary | null>("existing_slice"),
  saveProject: (project: SaveProject) => invoke<RecentProject[]>("save_project", { project }),
  /** Where an unsaved tab can be put away without a dialog. */
  scratchPath: (name: string) => invoke<string>("scratch_path", { name }),
  geometry: async (id: number, maxTriangles?: number): Promise<Float32Array> => {
    const raw = await invoke<ArrayBuffer | Uint8Array>("model_geometry", {
      id,
      maxTriangles: maxTriangles ?? null,
    });
    // A 4-byte little-endian count precedes the floats so the view can be a
    // window on the same buffer instead of another copy.
    const bytes = raw instanceof Uint8Array ? raw : new Uint8Array(raw);
    const length = new DataView(bytes.buffer, bytes.byteOffset, bytes.byteLength).getUint32(
      0,
      true,
    );
    return new Float32Array(bytes.buffer, bytes.byteOffset + 4, length);
  },
  openProject: (path: string) => invoke<OpenedProject>("open_project", { path }),
  clearJob: () => invoke<void>("clear_job"),
  recents: () => invoke<RecentProject[]>("list_recents"),
  forgetRecent: (path: string) => invoke<RecentProject[]>("forget_recent", { path }),
};

export interface RecentProject {
  path: string;
  name: string;
  saved_at_ms: number;
  printer: string;
  missing: boolean;
}

export interface SaveProject {
  path: string;
  name: string;
  printer_id: string;
  printer_label: string;
  variant_name: string | null;
  quality_type: string | null;
  material_id: string | null;
  /// The spools in the AMS beyond the plate's own material, in slot order.
  ams: string[];
  settings: SliceSettings;
  chord_mm: number;
  turn_deg: number;
  thumbnail: number[];
  /// Write the file without putting it in the recent list: a draft parked
  /// behind another tab is not a project the person chose to open.
  quiet?: boolean;
}

export interface OpenedProject {
  path: string;
  name: string;
  printer_id: string;
  printer_label: string;
  variant_name: string | null;
  quality_type: string | null;
  material_id: string | null;
  ams: string[];
  settings: SliceSettings;
  chord_mm: number;
  turn_deg: number;
  models: ModelView[];
  instructions: StoredInstruction[];
  support: SupportView | null;
  warnings: string[];
}
