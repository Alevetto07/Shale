import { open as tauriOpen, save as tauriSave } from "@tauri-apps/plugin-dialog";

export async function open(options?: Parameters<typeof tauriOpen>[0]) {
  if (import.meta.env.VITE_SHALE_MOCK === "1") {
    void options;
    return mockPath("open") ?? null;
  }
  return tauriOpen(options);
}

export async function save(options?: Parameters<typeof tauriSave>[0]) {
  if (import.meta.env.VITE_SHALE_MOCK === "1") {
    void options;
    return mockPath("save") ?? null;
  }
  return tauriSave(options);
}

function mockPath(kind: string): string | null {
  try {
    const raw = sessionStorage.getItem("shale.mock.paths");
    const parsed = raw ? (JSON.parse(raw) as Record<string, string | null>) : {};
    return parsed[kind] ?? `C:\\\\mock\\\\demo.${kind === "save" ? "shale" : "stl"}`;
  } catch {
    return `C:\\\\mock\\\\demo.${kind === "save" ? "shale" : "stl"}`;
  }
}