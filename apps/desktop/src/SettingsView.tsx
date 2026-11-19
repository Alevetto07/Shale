import { useEffect, useMemo, useState, type ReactNode } from "react";
import type { PrinterInfo } from "./api";
import { CloudPanel } from "./CloudPanel";
import { SlicerImportPanel } from "./SlicerImportPanel";
import { filterSections, SETTINGS_SECTIONS, type SettingsSectionId } from "./settings/settings-layout";
import { resetSetupWizard } from "./setup-wizard";
import { UiStylePicker } from "./UiStylePicker";
import type { UiStyleId } from "./ui-styles";
import { AsciiField, AsciiSelect } from "./widgets";

export function SettingsView({
  compact,
  version,
  printers,
  printerId,
  enginePath,
  libDir,
  backend,
  uiStyle,
  onUiStyle,
  onChoosePrinter,
  onEnginePath,
  onLibDir,
  onBackend,
  onApplyEngine,
  onRerunSetup,
}: {
  compact?: boolean;
  version: string;
  printers: PrinterInfo[];
  printerId: string;
  enginePath: string;
  libDir: string;
  backend: string;
  uiStyle: UiStyleId;
  onUiStyle: (id: UiStyleId) => void;
  onChoosePrinter: (id: string) => void;
  onEnginePath: (value: string) => void;
  onLibDir: (value: string) => void;
  onBackend: (value: string) => void;
  onApplyEngine: () => void;
  onRerunSetup?: () => void;
}) {
  const [section, setSection] = useState<SettingsSectionId>("general");
  const [query, setQuery] = useState("");
  const [printerQuery, setPrinterQuery] = useState("");

  const sections = useMemo(() => filterSections(query), [query]);

  useEffect(() => {
    if (!sections.some((item) => item.id === section)) {
      setSection(sections[0]?.id ?? "general");
    }
  }, [sections, section]);

  const filteredPrinters = useMemo(() => {
    const q = printerQuery.trim().toLowerCase();
    return printers
      .filter((printer) =>
        q ? `${printer.manufacturer} ${printer.name} ${printer.id}`.toLowerCase().includes(q) : true,
      )
      .slice(0, 60);
  }, [printerQuery, printers]);

  const active = sections.find((item) => item.id === section) ?? sections[0] ?? SETTINGS_SECTIONS[0];

  return (
    <div className={compact ? "settings-shell settings-shell--compact" : "settings-shell"}>
      <div className="settings-toolbar">
        <AsciiField
          label="Search settings"
          value={query}
          placeholder="Search settings…"
          onChange={setQuery}
        />
      </div>
      <nav className="settings-nav" aria-label="Settings sections">
        {sections.map((item) => (
          <button
            key={item.id}
            type="button"
            className={item.id === active.id ? "selected" : undefined}
            onClick={() => setSection(item.id)}
          >
            {item.label}
          </button>
        ))}
      </nav>
      <div className="settings-body">
        <header className="settings-heading">
          <h2>{active.label}</h2>
          <p>{active.description}</p>
        </header>

        {section === "general" && (
          <div className="settings-group">
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

        {section === "appearance" && (
          <div className="settings-group">
            <UiStylePicker value={uiStyle} onChange={onUiStyle} />
          </div>
        )}

        {section === "printers" && (
          <div className="settings-group">
            <AsciiField
              label="filter printers"
              value={printerQuery}
              placeholder="Filter catalogue…"
              onChange={setPrinterQuery}
            />
            <RuntimeScroll className="settings-scroll">
              {filteredPrinters.map((printer) => (
                <button
                  key={printer.id}
                  type="button"
                  className={printer.id === printerId ? "selected" : undefined}
                  onClick={() => onChoosePrinter(printer.id)}
                >
                  {printer.manufacturer ? `${printer.manufacturer} ` : ""}
                  {printer.name}
                  {printer.source === "bambu" ? " · Bambu" : ""}
                </button>
              ))}
            </RuntimeScroll>
          </div>
        )}

        {section === "profiles" && (
          <div className="settings-group">
            <p className="hint">
              Machine, nozzle, process, and filament defaults live in the menu bar under{" "}
              <strong>Printer</strong> and <strong>Slice settings</strong> while a project is open.
            </p>
          </div>
        )}

        {section === "slicing" && (
          <div className="settings-group">
            <p className="hint">
              Cost and energy fields are on the Preview sidebar after a slice. Process overrides
              are under <strong>Slice settings</strong> in the menu bar.
            </p>
          </div>
        )}

        {section === "accounts" && <CloudPanel taskName="settings" />}

        {section === "import" && (
          <div className="settings-group">
            <SlicerImportPanel compact onImported={() => undefined} />
          </div>
        )}

        {section === "advanced" && (
          <div className="settings-group">
            <button
              type="button"
              onClick={() => {
                resetSetupWizard();
                onRerunSetup?.();
              }}
            >
              Re-run first-run setup
            </button>
          </div>
        )}

        {section === "about" && (
          <div className="settings-group about-block">
            <p className="credit">
              Shale {version} — AGPL-3.0-or-later desktop slicer for Bambu Lab printers. CuraEngine
              runs as a separate process; profiles may come from Bambu Studio or Cura resources.
            </p>
          </div>
        )}
      </div>
    </div>
  );
}

function RuntimeScroll({
  className,
  children,
}: {
  className?: string;
  children: ReactNode;
}) {
  return <div className={className}>{children}</div>;
}