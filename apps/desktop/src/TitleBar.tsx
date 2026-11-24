import { getCurrentWindow } from "./platform/window";
import { useCallback, useEffect, useState, type ReactNode } from "react";
import markDark from "./brand/shale-mark-dark.svg";
import markLight from "./brand/shale-mark-light.svg";

function TitleBarIcon({ kind }: { kind: "minimize" | "maximize" | "restore" | "close" }) {
  if (kind === "close") {
    return (
      <svg className="titlebar-svg" viewBox="0 0 10 10" aria-hidden focusable="false">
        <path d="M1.2 1.2 L8.8 8.8 M8.8 1.2 L1.2 8.8" stroke="currentColor" strokeWidth="1.15" />
      </svg>
    );
  }
  if (kind === "minimize") {
    return (
      <svg className="titlebar-svg" viewBox="0 0 10 10" aria-hidden focusable="false">
        <rect x="1" y="7.5" width="8" height="1" fill="currentColor" />
      </svg>
    );
  }
  if (kind === "restore") {
    return (
      <svg className="titlebar-svg" viewBox="0 0 10 10" aria-hidden focusable="false">
        <rect x="2.5" y="0.8" width="6.2" height="6.2" fill="none" stroke="currentColor" strokeWidth="1" />
        <path d="M1 3.2 H6.8 V9 H1 Z" fill="var(--bg, #06070b)" stroke="currentColor" strokeWidth="1" />
      </svg>
    );
  }
  return (
    <svg className="titlebar-svg" viewBox="0 0 10 10" aria-hidden focusable="false">
      <rect x="1.2" y="1.2" width="7.6" height="7.6" fill="none" stroke="currentColor" strokeWidth="1" />
    </svg>
  );
}

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
  leading,
  children,
}: {
  projectName: string;
  /// Pills to the left of the menus (sidebar toggle, back / forward).
  leading?: ReactNode;
  /// The menu bar, dropped between the wordmark and the window buttons.
  children: ReactNode;
}) {
  const [maximised, setMaximised] = useState(false);
  const win = getCurrentWindow();

  useEffect(() => {
    let cancelled = false;
    const read = () => {
      void win
        .isMaximized()
        .then((value) => {
          if (!cancelled) setMaximised(value);
        })
        .catch(() => undefined);
    };
    read();
    const stop = win.onResized(() => read());
    return () => {
      cancelled = true;
      void stop.then((off) => off());
    };
  }, [win]);

  const onCaptionDoubleClick = useCallback(() => {
    void win.toggleMaximize();
  }, [win]);

  return (
    <header className="titlebar" data-tauri-drag-region>
      <div className="brand" data-tauri-drag-region onDoubleClick={onCaptionDoubleClick}>
        <img className="brand-mark brand-mark--dark" src={markDark} alt="" data-tauri-drag-region draggable={false} />
        <img className="brand-mark brand-mark--light" src={markLight} alt="" data-tauri-drag-region draggable={false} />
        <p className="wordmark" data-tauri-drag-region>
          Shale
        </p>
        {projectName && (
          <p className="project-name" data-tauri-drag-region>
            {projectName}
          </p>
        )}
      </div>
      {leading ? <div className="titlebar-leading">{leading}</div> : null}
      <div className="titlebar-menus" data-tauri-drag-region onDoubleClick={onCaptionDoubleClick}>
        {children}
      </div>
      <span className="titlebar-gap" data-tauri-drag-region onDoubleClick={onCaptionDoubleClick} />
      <div className="titlebar-buttons">
        <button
          type="button"
          className="titlebar-minimize"
          aria-label="Minimise"
          onClick={() => void win.minimize()}
        >
          <TitleBarIcon kind="minimize" />
        </button>
        <button
          type="button"
          className={maximised ? "titlebar-restore" : "titlebar-maximize"}
          aria-label={maximised ? "Restore" : "Maximise"}
          onClick={() => void win.toggleMaximize()}
        >
          <TitleBarIcon kind={maximised ? "restore" : "maximize"} />
        </button>
        <button type="button" aria-label="Close" className="titlebar-close" onClick={() => void win.close()}>
          <TitleBarIcon kind="close" />
        </button>
      </div>
    </header>
  );
}