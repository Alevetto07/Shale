import { getCurrentWindow as tauriWindow } from "@tauri-apps/api/window";

const mockWindow = {
  minimize: async () => undefined,
  maximize: async () => undefined,
  toggleMaximize: async () => false,
  isMaximized: async () => false,
  close: async () => {
    window.close();
  },
  onResized: async () => () => undefined,
};

export function getCurrentWindow() {
  if (import.meta.env.VITE_SHALE_MOCK === "1") {
    return mockWindow;
  }
  return tauriWindow();
}