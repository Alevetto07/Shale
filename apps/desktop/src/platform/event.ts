import { listen as tauriListen, type UnlistenFn } from "@tauri-apps/api/event";

export function listen<T>(event: string, handler: (payload: { payload: T }) => void): Promise<UnlistenFn> {
  if (import.meta.env.VITE_SHALE_MOCK === "1") {
    void handler;
    return Promise.resolve(() => undefined);
  }
  return tauriListen<T>(event, handler);
}