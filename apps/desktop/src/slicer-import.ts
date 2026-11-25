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

export interface ScanCounts {
  seen: number;
  total: number;
  printers: number;
  filaments: number;
  processes: number;
  warnings: number;
}

export interface ScanSummary {
  slicer: string;
  label: string;
  found: boolean;
  printers: number;
  filaments: number;
  processes: number;
  warnings: number;
  warning_notes: string[];
  paths: string[];
}

export type ImportPhase = "idle" | "scanning" | "ready" | "importing" | "done" | "error";

export interface ImportJob {
  phase: ImportPhase;
  slicerId: string;
  counts: ScanCounts;
  summary: ScanSummary | null;
  report: SlicerImportReport | null;
  error: string | null;
}

const EMPTY_COUNTS: ScanCounts = {
  seen: 0,
  total: 0,
  printers: 0,
  filaments: 0,
  processes: 0,
  warnings: 0,
};

const IDLE_JOB: ImportJob = {
  phase: "idle",
  slicerId: "",
  counts: EMPTY_COUNTS,
  summary: null,
  report: null,
  error: null,
};

const SOURCE_LABELS: Record<string, string> = {
  bambu: "Bambu Studio",
  orca: "OrcaSlicer",
  prusa: "PrusaSlicer",
  cura: "UltiMaker Cura",
};

let job: ImportJob = IDLE_JOB;
let busy = false;
const listeners = new Set<() => void>();

function publish(next: ImportJob) {
  job = next;
  for (const listener of listeners) listener();
}

export function currentImportJob(): ImportJob {
  return job;
}

export function subscribeImportJob(listener: () => void): () => void {
  listeners.add(listener);
  return () => listeners.delete(listener);
}

export function resetImportJob() {
  if (busy) return;
  publish(IDLE_JOB);
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

function mockMode(): boolean {
  return import.meta.env.VITE_SHALE_MOCK === "1";
}

export async function detectSlicerInstalls(): Promise<SlicerInstall[]> {
  if (mockMode()) {
    return [
      {
        id: "bambu",
        label: "Bambu Studio",
        found: true,
        config_root: "/mock/BambuStudio",
        printers: 0,
        filaments: 0,
        processes: 0,
      },
    ];
  }
  return invoke<SlicerInstall[]>("slicer_detect");
}

export async function listSlicerPresets(slicerId: string): Promise<PresetOffer[]> {
  if (mockMode()) {
    return MOCK_OFFERS.filter((row) => row.slicer === slicerId || slicerId === "bambu");
  }
  return invoke<PresetOffer[]>("slicer_list_presets", { slicerId });
}

async function listenProgress(event: string, onProgress: (counts: ScanCounts) => void): Promise<() => void> {
  if (mockMode()) return () => undefined;
  const { listen } = await import("@tauri-apps/api/event");
  return listen<ScanCounts>(event, (message) => onProgress(message.payload));
}

export async function scanSlicerPresets(
  slicerId: string,
  onProgress: (counts: ScanCounts) => void,
): Promise<ScanSummary> {
  if (mockMode()) {
    const steps = [12, 48, 96, 140];
    for (const seen of steps) {
      await new Promise((resolve) => setTimeout(resolve, 280));
      onProgress({
        seen,
        total: 140,
        printers: 4,
        filaments: 48,
        processes: Math.max(0, seen - 52),
        warnings: 1,
      });
    }
    return {
      slicer: slicerId,
      label: SOURCE_LABELS[slicerId] ?? slicerId,
      found: true,
      printers: 4,
      filaments: 48,
      processes: 88,
      warnings: 1,
      warning_notes: ["orphan: parent `missing` is not on this machine"],
      paths: ["printer", "filament", "process"],
    };
  }
  const stop = await listenProgress("slicer-scan-progress", onProgress);
  try {
    return await invoke<ScanSummary>("slicer_scan_presets", { slicerId });
  } finally {
    stop();
  }
}

export async function importSlicerPresets(
  slicerId: string,
  paths: string[],
  onProgress?: (counts: ScanCounts) => void,
): Promise<SlicerImportReport> {
  if (mockMode()) {
    onProgress?.({
      seen: paths.length,
      total: paths.length,
      printers: 4,
      filaments: 48,
      processes: 88,
      warnings: 0,
    });
    return {
      imported: paths.length,
      skipped: 0,
      unsupported: 0,
      lines: [],
    };
  }
  const stop = onProgress ? await listenProgress("slicer-import-progress", onProgress) : () => undefined;
  try {
    return await invoke<SlicerImportReport>("slicer_import_presets", { slicerId, paths });
  } finally {
    stop();
  }
}

/** Start a check. Does nothing if one is already running, and nothing until called. */
export function startSettingsCheck(slicerId: string) {
  if (busy) return;
  busy = true;
  publish({
    phase: "scanning",
    slicerId,
    counts: EMPTY_COUNTS,
    summary: null,
    report: null,
    error: null,
  });
  void scanSlicerPresets(slicerId, (counts) => {
    publish({ ...job, counts });
  })
    .then((summary) => {
      publish({
        ...job,
        phase: summary.found ? "ready" : "error",
        summary,
        counts: {
          seen: summary.printers + summary.filaments + summary.processes,
          total: summary.printers + summary.filaments + summary.processes,
          printers: summary.printers,
          filaments: summary.filaments,
          processes: summary.processes,
          warnings: summary.warnings,
        },
        error: summary.found ? null : `No ${SOURCE_LABELS[slicerId] ?? slicerId} settings folder was found on this PC.`,
      });
    })
    .catch((err: unknown) => {
      publish({ ...job, phase: "error", error: String(err) });
    })
    .finally(() => {
      busy = false;
    });
}

export function startSettingsImport(onImported?: () => void) {
  if (busy || !job.summary || job.summary.paths.length === 0) return;
  const summary = job.summary;
  busy = true;
  publish({ ...job, phase: "importing", error: null });
  void importSlicerPresets(summary.slicer, summary.paths, (counts) => {
    publish({ ...job, counts });
  })
    .then((report) => {
      publish({ ...job, phase: "done", report });
      onImported?.();
    })
    .catch((err: unknown) => {
      publish({ ...job, phase: "error", error: String(err) });
    })
    .finally(() => {
      busy = false;
    });
}

export { parseImportManifest };
