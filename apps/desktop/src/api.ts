import { invoke } from "@tauri-apps/api/core";

export interface Status {
  resources_ok: boolean;
  resources_message: string;
  engine_path: string;
  engine_ok: boolean;
  printer_count: number;
}

export interface PrinterInfo {
  id: string;
  name: string;
  manufacturer: string;
  preferred_quality_type: string;
  preferred_variant: string;
  preferred_material: string;
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

export interface ModelView {
  id: number;
  name: string;
  positions: number[];
  transform: Transform;
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
  setEngine: (path: string, libDir: string) =>
    invoke<boolean>("set_engine", { path, libDir: libDir || null }),
  printers: () => invoke<PrinterInfo[]>("list_printers"),
  variants: (printerId: string) => invoke<VariantInfo[]>("list_variants", { printerId }),
  qualities: (printerId: string) => invoke<QualityInfo[]>("list_qualities", { printerId }),
  materials: (printerId: string) => invoke<MaterialInfo[]>("list_materials", { printerId }),
  bed: (printerId: string, variantName: string | null, qualityType: string | null, materialId: string | null) =>
    invoke<Bed>("printer_bed", { printerId, variantName, qualityType, materialId }),
  importModel: (path: string, printerId: string | null, variantName: string | null, qualityType: string | null) =>
    invoke<ModelView>("import_model", { path, printerId, variantName, qualityType }),
  updateTransform: (id: number, transform: Transform) => invoke<void>("update_transform", { id, transform }),
  dropModel: (id: number) => invoke<Transform>("drop_model", { id }),
  removeModel: (id: number) => invoke<void>("remove_model", { id }),
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
  previewUntil: (layer: number) => invoke<PreviewPath[]>("preview_until", { layer }),
  exportGcode: (destination: string) => invoke<string>("export_gcode", { destination }),
  loadedModels: () => invoke<ModelView[]>("loaded_models"),
  existingSlice: () => invoke<SliceSummary | null>("existing_slice"),
};
