import { useEffect, useState } from "react";
import * as DropdownMenu from "@radix-ui/react-dropdown-menu";
import * as Popover from "@radix-ui/react-popover";
import { Check } from "lucide-react";

import type { MaterialInfo, NonplanarReportView, NonplanarSettings, PrinterInfo, QualityInfo, VariantInfo } from "./api";
import type { Section } from "./settings-meta";
import { variantOptionLabel } from "./profile-labels";
import { ProcessSheet, type QuickFallbacks } from "./ProcessSheet";
import type { ViewStyle } from "./viewport-style";
import { UiStylePicker } from "./UiStylePicker";
import type { UiStyleId } from "./ui-styles";
import type { Pane } from "./Tabs";
import { Button, Select, Sheet } from "./ui";

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
  /** Stage name while a slice is running, so the button is not a spinner. */
  sliceStage?: string | null;
  uiStyle: UiStyleId;
  onUiStyle: (id: UiStyleId) => void;
  nonplanar: NonplanarSettings;
  onNonplanar: (next: NonplanarSettings) => void;
  nonplanarReport: NonplanarReportView | null;
  nonplanarSupported: boolean;
  liveSlice: boolean;
  onLiveSlice: (next: boolean) => void;
  /** Bumps when the command palette asks the sheet to open on a tab. */
  settingsNonce: number;
  settingsSection: Section | null;
  pane: Pane;
  onPane: (pane: Pane) => void;
  onOpenPalette: () => void;
}

function Item({
  onSelect,
  disabled,
  kbd,
  checked,
  children,
}: {
  onSelect: () => void;
  disabled?: boolean;
  kbd?: string;
  checked?: boolean;
  children: string;
}) {
  return (
    <DropdownMenu.Item className="ui-dropdown__item" disabled={disabled} onSelect={onSelect}>
      <span>{children}</span>
      {kbd ? <kbd>{kbd}</kbd> : null}
      {checked ? <Check size={14} strokeWidth={1.5} aria-hidden /> : null}
    </DropdownMenu.Item>
  );
}

export function MenuBar(props: MenuBarProps) {
  const [settingsOpen, setSettingsOpen] = useState(props.demo === "presets");

  useEffect(() => {
    if (props.demo === "presets") setSettingsOpen(true);
  }, [props.demo]);

  useEffect(() => {
    if (props.settingsNonce > 0) setSettingsOpen(true);
  }, [props.settingsNonce]);

  useEffect(() => {
    if (props.demo !== "presets" || !settingsOpen) return;
    const timer = window.setTimeout(() => {
      document.querySelector(".presets")?.scrollIntoView({ block: "center" });
    }, 1200);
    return () => window.clearTimeout(timer);
  }, [props.demo, settingsOpen]);

  const printer = props.printers.find((entry) => entry.id === props.printerId);
  const panes: { id: Pane; label: string }[] = [
    { id: "home", label: "Home" },
    { id: "prepare", label: "Prepare" },
    { id: "preview", label: "Preview" },
    { id: "device", label: "Device" },
  ];

  return (
    <div className="menubar">
      <nav className="studio-nav" aria-label="Workspace">
        {panes.map((item) => {
          const waiting = item.id === "preview" && !props.sliced;
          return (
            <button
              key={item.id}
              type="button"
              className="studio-nav__item"
              aria-pressed={props.pane === item.id}
              data-waiting={waiting ? "1" : undefined}
              title={waiting ? "Slice the plate first. There is no toolpath to show yet." : undefined}
              onClick={() => props.onPane(item.id)}
            >
              {item.label}
            </button>
          );
        })}
      </nav>
      <span className="studio-spacer" data-tauri-drag-region />
      <Popover.Root>
        <Popover.Trigger className="studio-printer">
          {printer ? `${printer.manufacturer} ${printer.name}` : "No printer"}
        </Popover.Trigger>
        <Popover.Portal>
          <Popover.Content
            className="ui-dropdown ui-popover"
            sideOffset={6}
            align="end"
            collisionPadding={8}
            onInteractOutside={(event) => {
              const target = event.target as HTMLElement | null;
              if (target?.closest(".ui-dropdown, .ui-select__menu")) event.preventDefault();
            }}
          >
            <label className="ui-field">
              <span className="ui-field__label">Machine</span>
              <Select
                label="Machine"
                value={props.printerId}
                options={props.printers.map((entry) => ({
                  value: entry.id,
                  label: `${entry.manufacturer} ${entry.name}${entry.source === "bambu" ? " · Bambu" : ""}`,
                }))}
                onChange={props.onPrinter}
              />
            </label>
            <label className="ui-field">
              <span className="ui-field__label">Nozzle</span>
              <Select
                label="Nozzle"
                value={props.variant ?? ""}
                options={[
                  { value: "", label: "Default" },
                  ...props.variants.map((entry) => ({ value: entry.name, label: variantOptionLabel(entry) })),
                ]}
                onChange={(next) => props.onVariant(next || null)}
              />
            </label>
            <label className="ui-field">
              <span className="ui-field__label">Process</span>
              <Select
                label="Process"
                value={props.quality ?? ""}
                options={props.qualities.map((entry) => ({ value: entry.quality_type, label: entry.name }))}
                onChange={(next) => props.onQuality(next || null)}
              />
            </label>
            <label className="ui-field">
              <span className="ui-field__label">Filament</span>
              <Select
                label="Filament"
                value={props.material ?? ""}
                options={[
                  { value: "", label: "Default" },
                  ...props.materials.map((entry) => ({ value: entry.id, label: entry.label })),
                ]}
                onChange={(next) => props.onMaterial(next || null)}
              />
            </label>
            {props.materials.length > 0 && (
              <p className="menu-note">{props.materials.length} filaments for this machine</p>
            )}
          </Popover.Content>
        </Popover.Portal>
      </Popover.Root>
      <button type="button" className="studio-search" onClick={props.onOpenPalette}>
        Search
        <kbd>Ctrl K</kbd>
      </button>
      <Button
        type="button"
        variant="primary"
        size="sm"
        className="menu-slice"
        disabled={props.busy || !props.hasModels}
        onClick={props.onSlice}
      >
        {props.busy ? props.sliceStage || "Slicing layers" : "Slice"}
      </Button>
      <DropdownMenu.Root>
        <DropdownMenu.Trigger className="menu-title">Menu</DropdownMenu.Trigger>
        <DropdownMenu.Portal>
          <DropdownMenu.Content className="ui-dropdown" sideOffset={6} align="end" collisionPadding={8}>
            <Item onSelect={props.onNew} kbd="Ctrl N">New project</Item>
            <Item onSelect={props.onOpen} kbd="Ctrl O">Open project…</Item>
            <Item onSelect={props.onImport} kbd="Ctrl I">Import model…</Item>
            <DropdownMenu.Separator className="ui-dropdown__sep" />
            <p className="menu-label">Add a part</p>
            {(
              [
                ["cube", "Cube"],
                ["cylinder", "Cylinder"],
                ["cone", "Cone"],
                ["sphere", "Sphere"],
              ] as const
            ).map(([kind, title]) => (
              <Item key={kind} onSelect={() => props.onAddPart(kind)} kbd="20 mm">
                {title}
              </Item>
            ))}
            <DropdownMenu.Separator className="ui-dropdown__sep" />
            <Item onSelect={props.onSave} kbd="Ctrl S">Save</Item>
            <Item onSelect={props.onSaveAs} kbd="Ctrl ⇧ S">Save as…</Item>
            <Item onSelect={() => props.onExportGeometry("stl")} disabled={!props.modelCount}>
              Export plate as STL…
            </Item>
            <Item onSelect={() => props.onExportGeometry("3mf")} disabled={!props.modelCount}>
              Export plate as 3MF…
            </Item>
            <Item onSelect={props.onExport} disabled={!props.sliced} kbd="Ctrl E">
              Export G-code…
            </Item>
            <DropdownMenu.Separator className="ui-dropdown__sep" />
            <Item onSelect={props.onUndo} disabled={!props.canUndo} kbd="Ctrl Z">Undo</Item>
            <Item onSelect={props.onRedo} disabled={!props.canRedo} kbd="Ctrl ⇧ Z">Redo</Item>
            <Item onSelect={props.onDuplicate} disabled={!props.hasSelection} kbd="Ctrl D">Duplicate</Item>
            <Item onSelect={props.onSplit} disabled={!props.hasSelection}>Split to objects</Item>
            <Item onSelect={props.onFillPlate} disabled={!props.hasSelection}>Fill the plate</Item>
            <Item onSelect={props.onDrop} disabled={!props.hasSelection} kbd="D">Drop to plate</Item>
            <Item onSelect={props.onArrange} disabled={props.modelCount < 2} kbd="Ctrl ⇧ A">
              Arrange on plate
            </Item>
            <Item onSelect={props.onDelete} disabled={!props.hasSelection} kbd="Del">Delete</Item>
            <DropdownMenu.Separator className="ui-dropdown__sep" />
            {(
              [
                ["solid", "Solid"],
                ["wire", "Wireframe"],
              ] as [ViewStyle, string][]
            ).map(([value, label]) => (
              <Item key={value} checked={props.viewStyle === value} onSelect={() => props.onViewStyle(value)}>
                {label}
              </Item>
            ))}
            <Item checked={props.showModel} onSelect={() => props.onShowModel(!props.showModel)}>
              Show model
            </Item>
            <Item checked={props.preview} disabled={!props.sliced} onSelect={() => props.onPreview(!props.preview)}>
              Show toolpaths
            </Item>
            <Item onSelect={props.onFrame} kbd="F">Frame model</Item>
            <DropdownMenu.Separator className="ui-dropdown__sep" />
            <Item onSelect={() => setSettingsOpen(true)}>Slice settings</Item>
            <Item onSelect={props.onStart}>Home</Item>
            <Item onSelect={props.onQuit}>Quit</Item>
            <div className="ui-dropdown__block">
              <UiStylePicker compact value={props.uiStyle} onChange={props.onUiStyle} />
            </div>
          </DropdownMenu.Content>
        </DropdownMenu.Portal>
      </DropdownMenu.Root>
      <Sheet open={settingsOpen} onOpenChange={setSettingsOpen} title="Slice settings">
        <div className="settings-scope">
          <span>Applies to</span>
          <Select
            label="Applies to"
            value={props.scope}
            options={[
              { value: "all", label: "Every part" },
              ...props.parts.map((part) => ({ value: String(part.id), label: part.name })),
            ]}
            onChange={props.onScope}
          />
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
          printerLabel={printer ? `${printer.manufacturer} ${printer.name}` : "machine"}
          nonplanar={props.nonplanar}
          onNonplanar={props.onNonplanar}
          nonplanarReport={props.nonplanarReport}
          nonplanarSupported={props.nonplanarSupported}
          liveSlice={props.liveSlice}
          onLiveSlice={props.onLiveSlice}
          sectionRequest={props.settingsSection}
          focusNonce={props.settingsNonce}
        />
      </Sheet>
    </div>
  );
}
