import { invoke as tauriInvoke } from "@tauri-apps/api/core";
import { mockInvoke } from "../mock/invoke-router";

export function invoke<T>(cmd: string, args?: Record<string, unknown>): Promise<T> {
  if (import.meta.env.VITE_SHALE_MOCK === "1") {
    return mockInvoke<T>(cmd, args ?? {});
  }
  return tauriInvoke<T>(cmd, args);
}