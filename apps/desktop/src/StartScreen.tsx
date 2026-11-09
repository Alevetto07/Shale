import { Fragment, useEffect, useRef, useState } from "react";
import type { PrinterInfo, RecentProject } from "./api";
import { andromeda } from "./field/credit";
import { PlateField } from "./field/PlateField";
import { AsciiField, AsciiSelect } from "./widgets";

type Panel = "menu" | "printers" | "settings" | "about";

interface Item {
  id: string;
  label: string;
  hint: string;
  detail?: string;
  /// The machine a recent project was saved on, kept apart from the date so a
  /// long machine name cannot eat the date when the row is narrow.
  machine?: string;
  dim?: boolean;
  run: () => void;
}

export function StartScreen({
  live,
  canReturn,
  recents,
  recentsRead,
  printers,
  printerId,
  enginePath,
  libDir,
  backend,
  version,
  onReturn,
  onNew,
  onOpen,
  onImport,
  onOpenRecent,
  onChoosePrinter,
  onEnginePath,
  onLibDir,
  onBackend,
  onApplyEngine,
  onQuit,
}: {
  live: boolean;
  canReturn: boolean;
  recents: RecentProject[];
  /// False until the list has been read once: an empty list means "none yet",
  /// but only after the read, never before it.
  recentsRead: boolean;
  printers: PrinterInfo[];
  printerId: string;
  enginePath: string;
  libDir: string;
  backend: string;
  version: string;
  onReturn: () => void;
  onNew: () => void;
  onOpen: () => void;
  onImport: () => void;
  onOpenRecent: (path: string) => void;
  onChoosePrinter: (id: string) => void;
  onEnginePath: (value: string) => void;
  onLibDir: (value: string) => void;
  onBackend: (value: string) => void;
  onApplyEngine: () => void;
  onQuit: () => void;
}) {
  const [panel, setPanel] = useState<Panel>("menu");
  const [cursor, setCursor] = useState(0);
  const [printerQuery, setPrinterQuery] = useState("");
  const buttons = useRef<Array<HTMLButtonElement | null>>([]);

  const items = menuItems({
    panel,
    canReturn,
    recents,
    printers,
    printerQuery,
    onReturn,
    onNew,
    onOpen,
    onImport,
    onOpenRecent,
    onChoosePrinter,
    onQuit,
    onPanel: setPanel,
  });

  useEffect(() => {
    setCursor(0);
  }, [panel]);

  useEffect(() => {
    const node = buttons.current[cursor];
    node?.focus();
  }, [cursor, panel, items.length]);

  useEffect(() => {
    const onKey = (event: KeyboardEvent) => {
      const target = event.target as HTMLElement | null;
      const typing = target?.tagName === "INPUT" || target?.tagName === "TEXTAREA";
      if (event.key === "ArrowDown" || event.key === "ArrowUp") {
        if (typing) return;
        event.preventDefault();
        setCursor((current) => {
          if (items.length === 0) return 0;
          const step = event.key === "ArrowDown" ? 1 : -1;
          return (current + step + items.length) % items.length;
        });
        return;
      }
      if (event.key === "Home") {
        if (typing) return;
        event.preventDefault();
        setCursor(0);
        return;
      }
      if (event.key === "End") {
        if (typing) return;
        event.preventDefault();
        setCursor(Math.max(0, items.length - 1));
        return;
      }
      if (event.key === "Escape") {
        event.preventDefault();
        if (panel !== "menu") setPanel("menu");
        else if (canReturn) onReturn();
        return;
      }
      if (typing || event.metaKey || event.ctrlKey || event.altKey) return;
      const key = event.key.toLowerCase();
      const shortcut: Record<string, () => void> = {
        n: onNew,
        o: onOpen,
        i: onImport,
        p: () => setPanel("printers"),
        s: () => setPanel("settings"),
        a: () => setPanel("about"),
        q: onQuit,
        r: () => {
          const recent = items.findIndex((item) => item.id.startsWith("recent-"));
          if (recent >= 0) setCursor(recent);
        },
      };
      const run = shortcut[key];
      if (!run) return;
      event.preventDefault();
      run();
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [items, panel, canReturn, onReturn, onNew, onOpen, onImport, onQuit]);

  return (
    <div className="start">
      <PlateField live={live} comet={live} />
      <nav className="start-menu" aria-label="Start">
        {panel === "menu" && (
          <div className="start-list">
            {items.map((item, index) => (
              <Fragment key={item.id}>
                {item.id === "printers" && recents.length === 0 && (
                  <p className="start-label">
                    {recentsRead ? "Recent. none yet" : "Recent. reading…"}
                  </p>
                )}
                {item.id.startsWith("recent-") && !items[index - 1]?.id.startsWith("recent-") && (
                  <p className="start-label">Recent</p>
                )}
                <button
                  type="button"
                  ref={(node) => {
                    buttons.current[index] = node;
                  }}
                  className={
                    [
                      item.id.startsWith("recent-") ? "recent" : "",
                      item.dim ? "dim" : "",
                    ]
                      .filter(Boolean)
                      .join(" ") || undefined
                  }
                  aria-current={index === cursor ? "true" : undefined}
                  onClick={item.run}
                  onFocus={() => setCursor(index)}
                >
                  <span>{item.label}</span>
                  {item.detail ? (
                    <span className="start-detail">
                      {item.detail}
                      {item.machine ? (
                        <>
                          <span className="start-dot" aria-hidden="true">
                            ·
                          </span>
                          <span className="start-machine">{item.machine}</span>
                        </>
                      ) : null}
                    </span>
                  ) : null}
                  {item.hint ? <span className="start-hint">{item.hint}</span> : null}
                </button>
              </Fragment>
            ))}
          </div>
        )}
        {panel === "printers" && (
          <div className="start-panel">
            <button type="button" className="start-back" onClick={() => setPanel("menu")}>
              Back
            </button>
            <AsciiField label="filter printers" value={printerQuery} placeholder="Filter" onChange={setPrinterQuery} />
            <div className="start-scroll">
              {items.map((item, index) => (
                <button
                  key={item.id}
                  type="button"
                  ref={(node) => {
                    buttons.current[index] = node;
                  }}
                  className={item.id === `printer-${printerId}` ? "selected" : undefined}
                  onClick={item.run}
                  onFocus={() => setCursor(index)}
                >
                  {item.label}
                </button>
              ))}
            </div>
          </div>
        )}
        {panel === "settings" && (
          <div className="start-panel">
            <button type="button" className="start-back" onClick={() => setPanel("menu")}>
              Back
            </button>
            <label>
              backend
              <AsciiSelect
                label="backend"
                value={backend}
                options={[
                  { value: "cli", label: "cli" },
                  { value: "arcus", label: "arcus" },
                ]}
                onChange={onBackend}
              />
            </label>
            <label>
              binary
              <AsciiField label="binary" value={enginePath} onChange={onEnginePath} />
            </label>
            <label>
              libs
              <AsciiField label="libs" value={libDir} onChange={onLibDir} />
            </label>
            <button type="button" onClick={onApplyEngine}>
              Check engine
            </button>
          </div>
        )}
        {panel === "about" && (
          <div className="start-panel">
            <button type="button" className="start-back" onClick={() => setPanel("menu")}>
              Back
            </button>
            <p className="credit">
              Shale {version} — a desktop slicer. Slicing is done by an unmodified
              CuraEngine process; printer definitions, quality profiles and materials
              are Cura's, read as data, and the application's own licence is still
              undecided (see LICENSING.md).
            </p>
            <p className="credit">
              {andromeda.name} ({andromeda.object}), {andromeda.credit}. Released {andromeda.release}.{" "}
              {andromeda.licence}.
            </p>
            <p className="credit">
              <a href={andromeda.page}>{andromeda.page}</a>
            </p>
            <p className="credit">
              Space Grotesk, Geist Mono, and Inter are used under the SIL Open Font License 1.1.
            </p>
          </div>
        )}
      </nav>
      <p className="start-credit">
        {andromeda.name}, {andromeda.object}. {andromeda.credit}. {andromeda.licence}.
      </p>
    </div>
  );
}

function menuItems({
  panel,
  canReturn,
  recents,
  printers,
  printerQuery,
  onReturn,
  onNew,
  onOpen,
  onImport,
  onOpenRecent,
  onChoosePrinter,
  onQuit,
  onPanel,
}: {
  panel: Panel;
  canReturn: boolean;
  recents: RecentProject[];
  printers: PrinterInfo[];
  printerQuery: string;
  onReturn: () => void;
  onNew: () => void;
  onOpen: () => void;
  onImport: () => void;
  onOpenRecent: (path: string) => void;
  onChoosePrinter: (id: string) => void;
  onQuit: () => void;
  onPanel: (panel: Panel) => void;
}): Item[] {
  if (panel === "printers") {
    const query = printerQuery.trim().toLowerCase();
    return printers
      .filter((printer) =>
        query
          ? `${printer.manufacturer} ${printer.name} ${printer.id}`.toLowerCase().includes(query)
          : true,
      )
      .slice(0, 40)
      .map((printer) => ({
        id: `printer-${printer.id}`,
        label: `${printer.manufacturer ? `${printer.manufacturer} ` : ""}${printer.name}${
          printer.source === "bambu" ? " · Bambu" : ""
        }`,
        hint: "",
        run: () => {
          onChoosePrinter(printer.id);
          onPanel("menu");
        },
      }));
  }
  const items: Item[] = [];
  if (canReturn) {
    items.push({ id: "return", label: "Return", hint: "esc", run: onReturn });
  }
  items.push(
    { id: "new", label: "New project", hint: "n", run: onNew },
    { id: "open", label: "Open project...", hint: "o", run: onOpen },
    { id: "import", label: "Import model...", hint: "i", run: onImport },
  );
  // Five, not eight: eight recents plus the commands and the printers row make
  // a menu taller than the window, and the last row was clipped by the edge.
  for (const recent of recents.slice(0, 5)) {
    items.push({
      id: `recent-${recent.path}`,
      label: recent.name,
      hint: "",
      detail: recent.missing ? "missing" : formatWhen(recent.saved_at_ms),
      machine: recent.missing ? "" : shortMachine(recent.printer),
      dim: recent.missing,
      run: () => onOpenRecent(recent.path),
    });
  }
  items.push(
    { id: "printers", label: "Printers", hint: "p", run: () => onPanel("printers") },
    { id: "settings", label: "Settings", hint: "s", run: () => onPanel("settings") },
    { id: "about", label: "About and credits", hint: "a", run: () => onPanel("about") },
    { id: "quit", label: "Quit", hint: "q", run: onQuit },
  );
  return items;
}

/// The name a machine is printed under in the recent list. Cura definitions
/// carry a vendor and often a whole family in one entry —
/// `Creality3D Creality Ender-3 / Ender-3 v2` — where the vendor repeats the
/// name and the family lists machines the project was not saved on. The first
/// name is the machine, and a vendor token that repeats the brand is dropped;
/// "Bambu Lab A1" keeps its vendor, because that is what the machine is called.
function shortMachine(name: string): string {
  const first = name.split("/")[0].trim();
  const words = first.split(/\s+/).filter(Boolean);
  if (words.length < 2) return first || name;
  const vendorish = /3d|ultimaker|prusa/i.test(words[0]);
  const rest = words.slice(1).join(" ");
  return vendorish && rest ? rest : first;
}

function formatWhen(ms: number): string {
  if (!ms) return "";
  return new Date(ms).toLocaleDateString("en-GB", {
    day: "numeric",
    month: "short",
    year: "numeric",
  });
}
