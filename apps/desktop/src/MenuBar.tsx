import { useEffect, useRef, useState } from "react";

import type { MaterialInfo, PrinterInfo, QualityInfo, VariantInfo } from "./api";
import { variantOptionLabel } from "./profile-labels";
import { ProcessSheet, type QuickFallbacks } from "./ProcessSheet";
import type { ViewStyle } from "./Viewport";
import { UiStylePicker } from "./UiStylePicker";
import type { UiStyleId } from "./ui-styles";

export interface MenuBarProps {
  screen: "start" | "slicer";
  /** `--demo <scenario>`, when the window was started for a recording. */
  demo?: string | null;
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
  onExportGeometry: (format: "stl" | "3mf") => void;
  onQuit: () => void;

  onDuplicate: () => void;
  onSplit: () => void;
  onAddPart: (kind: "cube" | "cylinder" | "cone" | "sphere") => void;
  onFillPlate: () => void;
  onDelete: () => void;
  onArrange: () => void;
  onUndo: () => void;
  onRedo: () => void;
  canUndo: boolean;
  canRedo: boolean;
  modelCount: number;
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

  quickFallbacks: QuickFallbacks;
  /** Cura settings the user has changed; anything absent comes from the profile. */
  overrides: Record<string, string>;
  profileValues?: Record<string, string>;
  onOverride: (key: string, value: string) => void;
  /// Which part the settings sheet writes to: "all" for the plate, or a part id.
  scope: string;
  parts: { id: number; name: string }[];
  onScope: (scope: string) => void;
  onResetOverrides: () => void;
  onSlice: () => void;
  uiStyle: UiStyleId;
  onUiStyle: (id: UiStyleId) => void;
}

type Menu = "file" | "edit" | "view" | "printer" | "settings";

export function MenuBar(props: MenuBarProps) {
  const [open, setOpen] = useState<Menu | null>(props.demo === "presets" ? "settings" : null);
  const bar = useRef<HTMLDivElement | null>(null);

  // The demo flag arrives with the status, one render after this component
  // mounts, so the sheet cannot be opened in the initial state alone.
  useEffect(() => {
    if (props.demo === "presets") setOpen("settings");
  }, [props.demo]);

  // The presets panel sits at the foot of a long sheet; the clip has to show
  // it, so the sheet scrolls itself there once it has opened.
  useEffect(() => {
    if (props.demo !== "presets" || open !== "settings") return;
    const timer = window.setTimeout(() => {
      document.querySelector(".presets")?.scrollIntoView({ block: "center" });
    }, 1200);
    return () => window.clearTimeout(timer);
  }, [props.demo, open]);

  useEffect(() => {
    if (!open) return;
    const close = (event: MouseEvent) => {
      if (bar.current && !bar.current.contains(event.target as Node)) {
        setOpen(null);
      }
    };
    const escape = (event: KeyboardEvent) => {
      if (event.key !== "Escape") return;
      setOpen(null);
      const active = document.activeElement;
      if (active instanceof HTMLElement) active.blur();
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
            <p className="menu-label">Add a part</p>
            {(
              [
                ["cube", "Cube"],
                ["cylinder", "Cylinder"],
                ["cone", "Cone"],
                ["sphere", "Sphere"],
              ] as const
            ).map(([kind, title]) => (
              <button key={kind} type="button" onClick={pick(() => props.onAddPart(kind))}>
                <span>{title}</span>
                <kbd>20 mm</kbd>
              </button>
            ))}
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
            <button type="button" disabled={!props.modelCount} onClick={() => pick(() => props.onExportGeometry("stl"))}>
              <span>Export plate as STL…</span>
            </button>
            <button type="button" disabled={!props.modelCount} onClick={() => pick(() => props.onExportGeometry("3mf"))}>
              <span>Export plate as 3MF…</span>
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
            <button type="button" disabled={!props.canUndo} onClick={pick(props.onUndo)}>
              <span>Undo</span>
              <kbd>Ctrl Z</kbd>
            </button>
            <button type="button" disabled={!props.canRedo} onClick={pick(props.onRedo)}>
              <span>Redo</span>
              <kbd>Ctrl ⇧ Z</kbd>
            </button>
            <hr />
            <button type="button" disabled={!props.hasSelection} onClick={pick(props.onDuplicate)}>
              <span>Duplicate</span>
              <kbd>Ctrl D</kbd>
            </button>
            <button type="button" disabled={!props.hasSelection} onClick={pick(props.onSplit)}>
              <span>Split to objects</span>
            </button>
            <button type="button" disabled={!props.hasSelection} onClick={pick(props.onFillPlate)}>
              <span>Fill the plate</span>
            </button>
            <button type="button" disabled={!props.hasSelection} onClick={pick(props.onDrop)}>
              <span>Drop to plate</span>
              <kbd>D</kbd>
            </button>
            <button
              type="button"
              disabled={props.modelCount < 2}
              onClick={pick(props.onArrange)}
            >
              <span>Arrange on plate</span>
              <kbd>Ctrl ⇧ A</kbd>
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
              </button>
            ))}
            <hr />
            <button
              type="button"
              className={props.showModel ? "checked" : ""}
              onClick={pick(() => props.onShowModel(!props.showModel))}
            >
              <span>Show model</span>
            </button>
            <button
              type="button"
              className={props.preview ? "checked" : ""}
              disabled={!props.sliced}
              onClick={pick(() => props.onPreview(!props.preview))}
            >
              <span>Show toolpaths</span>
            </button>
            <hr />
            <button type="button" onClick={pick(props.onFrame)}>
              <span>Frame model</span>
              <kbd>F</kbd>
            </button>
            <hr />
            <UiStylePicker compact value={props.uiStyle} onChange={props.onUiStyle} />
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
                  {variantOptionLabel(entry)}
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
          <div
            className="menu-sheet wide settings-sheet"
            role="menu"
          >
            <div className="settings-scope">
              <span>Applies to</span>
              <select
                value={props.scope}
                onChange={(event) => props.onScope(event.currentTarget.value)}
              >
                <option value="all">Every part</option>
                {props.parts.map((part) => (
                  <option key={part.id} value={String(part.id)}>
                    {part.name}
                  </option>
                ))}
              </select>
              {props.scope !== "all" && (
                <span className="settings-scope-note">
                  {Object.keys(props.overrides).length} override
                  {Object.keys(props.overrides).length === 1 ? "" : "s"} for this part
                </span>
              )}
            </div>
            <ProcessSheet
              fallbacks={props.quickFallbacks}
              overrides={props.overrides}
              profileValues={props.profileValues}
              onOverride={props.onOverride}
              onResetOverrides={props.onResetOverrides}
              qualities={props.qualities}
              qualityType={props.quality}
              onQuality={(next) => props.onQuality(next)}
              printerLabel={
                printer ? `${printer.manufacturer} ${printer.name}` : "machine"
              }
            />
          </div>
        )}
      </div>

      <div className="menu-status">
        {printer ? (
          <>
            <span className="menu-printer-name">
              {printer.manufacturer} {printer.name}
            </span>
            {/* The name already says Bambu Lab; the chip said it twice. The
                fact that the machine is a Bambu is still visible where it
                matters — the Device pane and the printer list. */}
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
