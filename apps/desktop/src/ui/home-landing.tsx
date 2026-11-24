import { useState, type DragEvent } from "react";
import type { PrinterInfo, RecentProject } from "../api";
import { formatRelativeTime } from "./format-time";
import { Button } from "./button";

export interface HomeLandingProps {
  recents: RecentProject[];
  printers: PrinterInfo[];
  printerId: string;
  onNew: () => void;
  onOpen: () => void;
  onImport: () => void;
  onOpenRecent: (path: string) => void;
  onChoosePrinter: (id: string) => void;
  /** A path from a file drop, or null when the drop did not carry one. */
  onDropModel: (path: string | null) => void;
}

function lead(text: string): string {
  const trimmed = text.trim();
  if (!trimmed) return trimmed;
  return trimmed.charAt(0).toUpperCase() + trimmed.slice(1);
}

function markHue(name: string): number {
  let hash = 0;
  for (let index = 0; index < name.length; index += 1) hash = (hash * 33 + name.charCodeAt(index)) % 360;
  return hash;
}

export function HomeLanding(props: HomeLandingProps) {
  const [over, setOver] = useState(false);
  const shown = props.recents.slice(0, 6);
  const machines = props.printers.slice(0, 6);

  function onDrop(event: DragEvent<HTMLElement>) {
    event.preventDefault();
    setOver(false);
    const file = event.dataTransfer.files[0] as (File & { path?: string }) | undefined;
    props.onDropModel(file?.path ?? null);
  }

  return (
    <div className="home-landing">
      <header className="home-landing__intro">
        <p className="home-landing__kicker">Shale</p>
        <h1>Prepare a plate</h1>
        <p>Import a model, choose a printer, and slice. The plate stays in view the whole way.</p>
      </header>

      <section
        className={over ? "home-drop home-drop--over" : "home-drop"}
        onDragEnter={(event) => {
          event.preventDefault();
          setOver(true);
        }}
        onDragOver={(event) => {
          event.preventDefault();
          setOver(true);
        }}
        onDragLeave={() => setOver(false)}
        onDrop={onDrop}
      >
        <h2>Drop a model</h2>
        <p>STL, 3MF, or STEP. Or pick a file from disk.</p>
        <div className="home-drop__actions">
          <Button variant="primary" size="lg" onClick={props.onImport}>
            Import a model
          </Button>
          <Button variant="secondary" size="lg" onClick={props.onNew}>
            New project
          </Button>
          <Button variant="ghost" size="lg" onClick={props.onOpen}>
            Open project
          </Button>
        </div>
      </section>

      <section className="home-block" aria-label="Printer">
        <h2>Printer</h2>
        {machines.length === 0 ? (
          <p className="home-empty">No printer yet. Shale will list the machines it knows once the catalogue has loaded.</p>
        ) : (
          <div className="home-printers" role="listbox" aria-label="Quick start printer">
            {machines.map((printer) => {
              const name = `${printer.manufacturer ? `${printer.manufacturer} ` : ""}${printer.name}`;
              const selected = printer.id === props.printerId;
              return (
                <button
                  key={printer.id}
                  type="button"
                  role="option"
                  aria-selected={selected}
                  className={selected ? "home-printer is-on" : "home-printer"}
                  onClick={() => props.onChoosePrinter(printer.id)}
                >
                  <span>{name}</span>
                  {printer.source === "bambu" ? <em>Bambu</em> : null}
                </button>
              );
            })}
          </div>
        )}
      </section>

      <section className="home-block" aria-label="Recent projects">
        <h2>Recent projects</h2>
        {shown.length === 0 ? (
          <p className="home-empty">No recent projects yet. A project you save will land here, with the printer it was sliced for.</p>
        ) : (
          <ul className="home-cards">
            {shown.map((entry) => (
              <li key={entry.path}>
                <button type="button" className="home-card" data-recent="1" onClick={() => props.onOpenRecent(entry.path)}>
                  <span className="home-card__mark" style={{ ["--mark" as string]: `${markHue(entry.name)}` }} aria-hidden />
                  <span className="home-card__copy">
                    <strong>{entry.name}</strong>
                    <span>
                      {entry.missing ? "Missing" : lead(formatRelativeTime(entry.saved_at_ms) || "Saved")}
                      {entry.printer ? ` · ${entry.printer}` : ""}
                    </span>
                  </span>
                </button>
              </li>
            ))}
          </ul>
        )}
      </section>
    </div>
  );
}
