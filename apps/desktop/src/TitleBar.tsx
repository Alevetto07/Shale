import { getCurrentWindow } from "@tauri-apps/api/window";
import { useEffect, useState, type ReactNode } from "react";
import mark from "./brand/shale-mark-dark.svg";

/**
 * Shale's own title bar.
 *
 * The window is undecorated, so this strip is the frame. The empty runs carry
 * `data-tauri-drag-region`, which is what makes the OS treat them as the
 * caption: dragging moves the window, double-click maximises, and Aero snap
 * still works because the window keeps its resize styles. The menu bar lives
 * here rather than in a second strip, so the window has one line of chrome.
 */
export function TitleBar({
  projectName,
  children,
}: {
  projectName: string;
  /// The menu bar, dropped between the wordmark and the window buttons.
  children: ReactNode;
}) {
  const [maximised, setMaximised] = useState(false);

  useEffect(() => {
    const win = getCurrentWindow();
    let cancelled = false;
    const read = () => {
      void win.isMaximized().then((value) => {
        if (!cancelled) setMaximised(value);
      }).catch(() => undefined);
    };
    read();
    const stop = win.onResized(() => read());
    return () => {
      cancelled = true;
      void stop.then((off) => off());
    };
  }, []);

  const win = getCurrentWindow();

  return (
    <header className="titlebar" data-tauri-drag-region>
      <div className="brand" data-tauri-drag-region>
        <img className="brand-mark" src={mark} alt="" data-tauri-drag-region />
        <p className="wordmark" data-tauri-drag-region>
          Shale
        </p>
        {projectName && (
          <p className="project-name" data-tauri-drag-region>
            {projectName}
          </p>
        )}
      </div>
      {children}
      <span className="titlebar-gap" data-tauri-drag-region />
      <div className="titlebar-buttons">
        <button type="button" aria-label="Minimise" onClick={() => void win.minimize()}>
          _
        </button>
        <button
          type="button"
          aria-label={maximised ? "Restore" : "Maximise"}
          onClick={() => void win.toggleMaximize()}
        >
          {maximised ? "\u2750" : "\u25a1"}
        </button>
        <button
          type="button"
          aria-label="Close"
          className="titlebar-close"
          onClick={() => void win.close()}
        >
          \u00d7
        </button>
      </div>
    </header>
  );
}
