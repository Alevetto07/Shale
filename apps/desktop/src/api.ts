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
  warning_count: number;
}

export interface SliceSummary {
  layer_count: number;
  layers: { index: number; z: number }[];
  print_time_seconds: number | null;
  warning_count: number;
  gcode_path: string;
}

export interface PreviewPath {
  kind: string;
  points: [number, number, number][];
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
  importModel: (
    path: string,
    printerId: string | null,
    variantName: string | null,
    qualityType: string | null,
    chordalMm: number,
    angularDeg: number,
  ) =>
    invoke<ModelView>("import_model", {
      path,
      printerId,
      variantName,
      qualityType,
      chordalMm,
      angularDeg,
    }),
  updateTransform: (id: number, transform: Transform) => invoke<void>("update_transform", { id, transform }),
  dropModel: (id: number) => invoke<Transform>("drop_model", { id }),
  duplicateModel: (id: number) => invoke<ModelView>("duplicate_model", { id }),
  setModelSettings: (id: number, settings: Record<string, string>) =>
    invoke<ModelView>("set_model_settings", { id, settings }),
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
    settings: SliceSettings,
  ) =>
    invoke<SliceSummary>("slice_model", {
      printerId,
      variantName,
      qualityType,
      materialId,
      settings,
    }),
  supportPlan: () => invoke<SupportView | null>("current_support"),
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
  settings: SliceSettings;
  chord_mm: number;
  turn_deg: number;
  thumbnail: number[];
}

export interface OpenedProject {
  path: string;
  name: string;
  printer_id: string;
  printer_label: string;
  variant_name: string | null;
  quality_type: string | null;
  material_id: string | null;
  settings: SliceSettings;
  chord_mm: number;
  turn_deg: number;
  models: ModelView[];
  instructions: StoredInstruction[];
  support: SupportView | null;
  warnings: string[];
}
