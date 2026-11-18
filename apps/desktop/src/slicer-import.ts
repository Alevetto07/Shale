import { invoke } from "@tauri-apps/api/core";
import { parseImportManifest } from "./slicer-import-manifest";

export interface SlicerInstall {
  id: string;
  label: string;
  found: boolean;
  config_root?: string | null;
  printers: number;
  filaments: number;
  processes: number;
}

export interface PresetOffer {
  id: string;
  path: string;
  kind: string;
  name: string;
  slicer: string;
  supported: boolean;
  note?: string | null;
}

export interface ImportLine {
  path: string;
  name: string;
  outcome: string;
  detail?: string | null;
}

export interface SlicerImportReport {
  imported: number;
  skipped: number;
  unsupported: number;
  lines: ImportLine[];
}

const MOCK_OFFERS: PresetOffer[] = [
  {
    id: "mock-1",
    path: "/mock/BambuStudio/user/default/process/custom.json",
    kind: "process",
    name: "Custom (process)",
    slicer: "bambu",
    supported: true,
    note: null,
  },
];

export async function detectSlicerInstalls(): Promise<SlicerInstall[]> {
  if (import.meta.env.VITE_SHALE_MOCK === "1") {
    return [
      {
        id: "bambu",
        label: "Bambu Studio",
        found: true,
        config_root: "/mock/BambuStudio",
        printers: 1,
        filaments: 2,
        processes: 3,
      },
    ];
  }
  return invoke<SlicerInstall[]>("slicer_detect");
}

export async function listSlicerPresets(slicerId: string): Promise<PresetOffer[]> {
  if (import.meta.env.VITE_SHALE_MOCK === "1") {
    return MOCK_OFFERS.filter((row) => row.slicer === slicerId || slicerId === "bambu");
  }
  return invoke<PresetOffer[]>("slicer_list_presets", { slicerId });
}

export async function importSlicerPresets(
  slicerId: string,
  paths: string[],
): Promise<SlicerImportReport> {
  if (import.meta.env.VITE_SHALE_MOCK === "1") {
    return {
      imported: paths.length,
      skipped: 0,
      unsupported: 0,
      lines: paths.map((path) => ({
        path,
        name: path.split("/").pop() ?? path,
        outcome: "imported",
        detail: "mock import",
      })),
    };
  }
  return invoke<SlicerImportReport>("slicer_import_presets", { slicerId, paths });
}

export { parseImportManifest };