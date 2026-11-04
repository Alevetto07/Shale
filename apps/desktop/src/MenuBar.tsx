import { useEffect, useRef, useState } from "react";

import type { MaterialInfo, PrinterInfo, QualityInfo, VariantInfo } from "./api";
import type { ViewStyle } from "./Viewport";

/** Slicing settings that live in the menu bar rather than a side panel. */
export interface SliceSettings {
  layerHeight: number;
  walls: number;
  infill: number;
  pattern: string;
  supports: boolean;
  nozzle: number;
  bed: number;
  speed: number;
}

export interface MenuBarProps {
  screen: "start" | "slicer";
  projectName: string;
  busy: boolean;
  sliced: boolean;
  hasModels: boolean;
  hasSelection: boolean;
  importing: boolean;

  onStart: () => void;
  onNew: () => void;
  onOpen: () => void;
  onImport: () => void;
  onSave: () => void;
  onSaveAs: () => void;
  onExport: () => void;
  onQuit: () => void;

  onDuplicate: () => void;
  onDelete: () => void;
  onDrop: () => void;

  viewStyle: ViewStyle;
  onViewStyle: (style: ViewStyle) => void;
  preview: boolean;
  onPreview: (value: boolean) => void;
  showModel: boolean;
  onShowModel: (value: boolean) => void;
  onFrame: () => void;

  printers: PrinterInfo[];
  printerId: string;
  onPrinter: (id: string) => void;
  variants: VariantInfo[];
  variant: string | null;
  onVariant: (name: string | null) => void;
  qualities: QualityInfo[];
  quality: string | null;
  onQuality: (type: string | null) => void;
  materials: MaterialInfo[];
  material: string | null;
  onMaterial: (id: string | null) => void;

  settings: SliceSettings;
  onSettings: (patch: Partial<SliceSettings>) => void;
  onSlice: () => void;
}

const INFILL_PATTERNS = [
  "grid",
  "lines",
  "triangles",
  "gyroid",
  "cubic",
  "honeycomb",
  "lightning",
  "concentric",
  "zigzag",
  "cross",
];

type Menu = "file" | "edit" | "view" | "printer" | "settings";

export function MenuBar(props: MenuBarProps) {
  const [open, setOpen] = useState<Menu | null>(null);
  const bar = useRef<HTMLDivElement | null>(null);

  useEffect(() => {
    if (!open) return;
    const close = (event: MouseEvent) => {
      if (bar.current && !bar.current.contains(event.target as Node)) {
        setOpen(null);
      }
    };
    const escape = (event: KeyboardEvent) => {
      if (event.key === "Escape") setOpen(null);
    };
    document.addEventListener("mousedown", close);
    document.addEventListener("keydown", escape);
    return () => {
      document.removeEventListener("mousedown", close);
      document.removeEventListener("keydown", escape);
    };
  }, [open]);

  const pick = (action: () => void) => () => {
    setOpen(null);
    action();
  };

  const printer = props.printers.find((entry) => entry.id === props.printerId);
  const bambu = printer?.source === "bambu";
  const settings = props.settings;

  const number = (
    key: keyof SliceSettings,
    step: number,
    min: number,
    max: number,
    suffix?: string,
  ) => (
    <span className="menu-field">
      <input
        type="number"
        value={settings[key] as number}
        step={step}
        min={min}
        max={max}
        onChange={(event) => {
          const next = Number(event.target.value);
          if (Number.isFinite(next)) {
            props.onSettings({ [key]: Math.min(max, Math.max(min, next)) });
          }
        }}
      />
      {suffix && <em>{suffix}</em>}
    </span>
  );

  return (
    <div className="menubar" ref={bar}>
      <div className="menu-group">
        <button
          type="button"
          className={open === "file" ? "menu-title open" : "menu-title"}
          onClick={() => setOpen(open === "file" ? null : "file")}
        >
          File
        </button>
        {open === "file" && (
          <div className="menu-sheet" role="menu">
            <button type="button" onClick={pick(props.onNew)}>
              <span>New project</span>
              <kbd>Ctrl N</kbd>
            </button>
            <button type="button" onClick={pick(props.onOpen)}>
              <span>Open project…</span>
              <kbd>Ctrl O</kbd>
            </button>
            <button type="button" onClick={pick(props.onImport)}>
              <span>Import model…</span>
              <kbd>Ctrl I</kbd>
            </button>
            <hr />
            <button type="button" onClick={pick(props.onSave)}>
              <span>Save</span>
              <kbd>Ctrl S</kbd>
            </button>
            <button type="button" onClick={pick(props.onSaveAs)}>
              <span>Save as…</span>
              <kbd>Ctrl ⇧ S</kbd>
            </button>
            <hr />
            <button type="button" disabled={!props.sliced} onClick={pick(props.onExport)}>
              <span>Export G-code…</span>
              <kbd>Ctrl E</kbd>
            </button>
            <hr />
            <button type="button" onClick={pick(props.onStart)}>
              <span>Start screen</span>
            </button>
            <button type="button" onClick={pick(props.onQuit)}>
              <span>Quit</span>
            </button>
          </div>
        )}
      </div>

      <div className="menu-group">
        <button
          type="button"
          className={open === "edit" ? "menu-title open" : "menu-title"}
          onClick={() => setOpen(open === "edit" ? null : "edit")}
        >
          Edit
        </button>
        {open === "edit" && (
          <div className="menu-sheet" role="menu">
            <button type="button" disabled={!props.hasSelection} onClick={pick(props.onDuplicate)}>
              <span>Duplicate</span>
              <kbd>Ctrl D</kbd>
            </button>
            <button type="button" disabled={!props.hasSelection} onClick={pick(props.onDrop)}>
              <span>Drop to plate</span>
              <kbd>D</kbd>
            </button>
            <hr />
            <button type="button" disabled={!props.hasSelection} onClick={pick(props.onDelete)}>
              <span>Delete</span>
              <kbd>Del</kbd>
            </button>
          </div>
        )}
      </div>

      <div className="menu-group">
        <button
          type="button"
          className={open === "view" ? "menu-title open" : "menu-title"}
          onClick={() => setOpen(open === "view" ? null : "view")}
        >
          View
        </button>
        {open === "view" && (
          <div className="menu-sheet" role="menu">
            {(
              [
                ["solid", "Solid"],
                ["wire", "Wireframe"],
                ["ascii", "ASCII"],
              ] as [ViewStyle, string][]
            ).map(([value, label]) => (
              <button
                type="button"
                key={value}
                className={props.viewStyle === value ? "checked" : ""}
                onClick={pick(() => props.onViewStyle(value))}
              >
                <span>{label}</span>
                {props.viewStyle === value && <kbd>✓</kbd>}
              </button>
            ))}
            <hr />
            <button
              type="button"
              className={props.showModel ? "checked" : ""}
              onClick={pick(() => props.onShowModel(!props.showModel))}
            >
              <span>Show model</span>
              {props.showModel && <kbd>✓</kbd>}
            </button>
            <button
              type="button"
              className={props.preview ? "checked" : ""}
              disabled={!props.sliced}
              onClick={pick(() => props.onPreview(!props.preview))}
            >
              <span>Show toolpaths</span>
              {props.preview && <kbd>✓</kbd>}
            </button>
            <hr />
            <button type="button" onClick={pick(props.onFrame)}>
              <span>Frame model</span>
              <kbd>F</kbd>
            </button>
          </div>
        )}
      </div>

      <div className="menu-group">
        <button
          type="button"
          className={open === "printer" ? "menu-title open" : "menu-title"}
          onClick={() => setOpen(open === "printer" ? null : "printer")}
        >
          Printer
        </button>
        {open === "printer" && (
          <div className="menu-sheet wide" role="menu">
            <label className="menu-label" htmlFor="menu-printer">
              Machine
            </label>
            <select
              id="menu-printer"
              value={props.printerId}
              onChange={(event) => props.onPrinter(event.target.value)}
            >
              {props.printers.map((entry) => (
                <option key={entry.id} value={entry.id}>
                  {entry.manufacturer} {entry.name}
                  {entry.source === "bambu" ? " · Bambu" : ""}
                </option>
              ))}
            </select>

            <label className="menu-label" htmlFor="menu-nozzle">
              Nozzle
            </label>
            <select
              id="menu-nozzle"
              value={props.variant ?? ""}
              onChange={(event) => props.onVariant(event.target.value || null)}
            >
              <option value="">Default</option>
              {props.variants.map((entry) => (
                <option key={entry.name} value={entry.name}>
                  {entry.name}
                  {entry.nozzle_size ? ` (${entry.nozzle_size} mm)` : ""}
                </option>
              ))}
            </select>

            <label className="menu-label" htmlFor="menu-quality">
              Process
            </label>
            <select
              id="menu-quality"
              value={props.quality ?? ""}
              onChange={(event) => props.onQuality(event.target.value || null)}
            >
              <option value="">Default</option>
              {props.qualities.map((entry) => (
                <option key={entry.quality_type} value={entry.quality_type}>
                  {entry.name}
                </option>
              ))}
            </select>

            <label className="menu-label" htmlFor="menu-material">
              Filament
            </label>
            <select
              id="menu-material"
              value={props.material ?? ""}
              onChange={(event) => props.onMaterial(event.target.value || null)}
            >
              <option value="">Default</option>
              {props.materials.map((entry) => (
                <option key={entry.id} value={entry.id}>
                  {entry.label}
                </option>
              ))}
            </select>
            {props.materials.length > 0 && (
              <p className="menu-note">{props.materials.length} filaments for this machine</p>
            )}
          </div>
        )}
      </div>

      <div className="menu-group">
        <button
          type="button"
          className={open === "settings" ? "menu-title open" : "menu-title"}
          onClick={() => setOpen(open === "settings" ? null : "settings")}
        >
          Slice settings
        </button>
        {open === "settings" && (
          <div className="menu-sheet wide" role="menu">
            <div className="menu-row">
              <span>Layer height</span>
              {number("layerHeight", 0.05, 0.05, 1, "mm")}
            </div>
            <div className="menu-row">
              <span>Walls</span>
              {number("walls", 1, 0, 12)}
            </div>
            <div className="menu-row">
              <span>Infill</span>
              {number("infill", 5, 0, 100, "%")}
            </div>
            <div className="menu-row">
              <span>Pattern</span>
              <select
                value={settings.pattern}
                onChange={(event) => props.onSettings({ pattern: event.target.value })}
              >
                {INFILL_PATTERNS.map((pattern) => (
                  <option key={pattern} value={pattern}>
                    {pattern}
                  </option>
                ))}
              </select>
            </div>
            <div className="menu-row">
              <span>Supports</span>
              <label className="menu-toggle">
                <input
                  type="checkbox"
                  checked={settings.supports}
                  onChange={(event) => props.onSettings({ supports: event.target.checked })}
                />
                <em>{settings.supports ? "on" : "off"}</em>
              </label>
            </div>
            <hr />
            <div className="menu-row">
              <span>Nozzle</span>
              {number("nozzle", 5, 150, 320, "°C")}
            </div>
            <div className="menu-row">
              <span>Bed</span>
              {number("bed", 5, 0, 120, "°C")}
            </div>
            <div className="menu-row">
              <span>Speed</span>
              {number("speed", 5, 10, 500, "mm/s")}
            </div>
            <p className="menu-note">
              {bambu
                ? "Bambu Studio profiles are read as data; the engine is still CuraEngine."
                : "Values override the selected Cura profile for this slice."}
            </p>
          </div>
        )}
      </div>

      <div className="menu-status">
        {printer ? (
          <>
            <span className="menu-printer-name">
              {printer.manufacturer} {printer.name}
            </span>
            {bambu && <span className="chip">Bambu</span>}
          </>
        ) : (
          <span className="menu-printer-name">No printer</span>
        )}
        {props.importing && <span className="menu-note">importing…</span>}
        <button
          type="button"
          className="primary menu-slice"
          disabled={props.busy || !props.hasModels}
          onClick={props.onSlice}
        >
          {props.busy ? "Slicing…" : "Slice"}
        </button>
      </div>
    </div>
  );
}
