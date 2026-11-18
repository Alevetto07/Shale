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

export async function detectSlicerInstalls(): Promise<SlicerInstall[]> {
  if (import.meta.env.VITE_SHALE_MOCK === "1") {
    return [
      {
        id: "bambu",
        label: "Bambu Studio",
        found: true,
        config_root: "/mock/BambuStudio",
        printers: 2,
        filaments: 4,
        processes: 6,
      },
    ];
  }
  return invoke<SlicerInstall[]>("slicer_detect");
}

export { parseImportManifest };