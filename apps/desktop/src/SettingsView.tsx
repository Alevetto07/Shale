import { useEffect, useMemo, useState, type ReactNode } from "react";
import { motion } from "motion/react";
import type { PrinterInfo } from "./api";
import { CloudPanel } from "./CloudPanel";
import { SlicerImportPanel } from "./SlicerImportPanel";
import { filterSections, SETTINGS_SECTIONS, type SettingsSectionId } from "./settings/settings-layout";
import { resetSetupWizard } from "./setup-wizard";
import { UiStylePicker } from "./UiStylePicker";
import type { UiStyleId } from "./ui-styles";
import { Button, Input, Select, Tabs, TabsContent, TabsList, TabsTrigger, TextField } from "./ui";
import { springSnappy } from "./ui/springs";
import { useReducedMotion } from "./ui/use-reduced-motion";

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
  const reduce = useReducedMotion();

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
        <Input
          aria-label="Search settings"
          value={query}
          placeholder="Search settings…"
          onChange={(event) => setQuery(event.target.value)}
        />
      </div>
      <Tabs
        className="settings-tabs"
        value={active.id}
        onValueChange={(value) => setSection(value as SettingsSectionId)}
        orientation={compact ? "horizontal" : "vertical"}
      >
        <TabsList className="settings-nav" aria-label="Settings sections">
          {sections.map((item) => (
            <TabsTrigger key={item.id} value={item.id}>
              {item.label}
            </TabsTrigger>
          ))}
        </TabsList>
        <TabsContent value={active.id} className="settings-body">
          <motion.div
            key={active.id}
            initial={reduce ? false : { opacity: 0, y: 8 }}
            animate={{ opacity: 1, y: 0 }}
            transition={reduce ? { duration: 0 } : springSnappy}
          >
            <header className="settings-heading">
              <h2>{active.label}</h2>
              <p>{active.description}</p>
            </header>

            {active.id === "general" && (
              <div className="settings-group">
                <label className="ui-field">
                  <span className="ui-field__label">backend</span>
                  <Select
                    label="backend"
                    value={backend}
                    options={[
                      { value: "cli", label: "cli" },
                      { value: "arcus", label: "arcus" },
                    ]}
                    onChange={onBackend}
                  />
                </label>
                <label className="ui-field">
                  <span className="ui-field__label">binary</span>
                  <TextField label="binary" value={enginePath} onChange={onEnginePath} />
                </label>
                <label className="ui-field">
                  <span className="ui-field__label">libs</span>
                  <TextField label="libs" value={libDir} onChange={onLibDir} />
                </label>
                <Button type="button" variant="secondary" onClick={onApplyEngine}>
                  Check engine
                </Button>
              </div>
            )}

            {active.id === "appearance" && (
              <div className="settings-group">
                <UiStylePicker value={uiStyle} onChange={onUiStyle} />
              </div>
            )}

            {active.id === "printers" && (
              <div className="settings-group">
                <TextField
                  label="filter printers"
                  value={printerQuery}
                  placeholder="Filter catalogue…"
                  onChange={setPrinterQuery}
                />
                <RuntimeScroll className="settings-scroll">
                  {filteredPrinters.map((printer) => (
                    <Button
                      key={printer.id}
                      type="button"
                      variant={printer.id === printerId ? "primary" : "ghost"}
                      size="sm"
                      className="ui-btn--block"
                      onClick={() => onChoosePrinter(printer.id)}
                    >
                      {printer.manufacturer ? `${printer.manufacturer} ` : ""}
                      {printer.name}
                      {printer.source === "bambu" ? " · Bambu" : ""}
                    </Button>
                  ))}
                </RuntimeScroll>
              </div>
            )}

            {active.id === "profiles" && (
              <div className="settings-group">
                <p className="hint">
                  Machine, nozzle, process, and filament defaults live in the menu bar under{" "}
                  <strong>Printer</strong> and <strong>Slice settings</strong> while a project is open.
                </p>
              </div>
            )}

            {active.id === "slicing" && (
              <div className="settings-group">
                <p className="hint">
                  Cost and energy fields are on the Preview sidebar after a slice. Process overrides
                  are under <strong>Slice settings</strong> in the menu bar.
                </p>
              </div>
            )}

            {active.id === "accounts" && <CloudPanel taskName="settings" />}

            {active.id === "import" && (
              <div className="settings-group">
                <SlicerImportPanel compact onImported={() => undefined} />
              </div>
            )}

            {active.id === "advanced" && (
              <div className="settings-group">
                <Button
                  type="button"
                  variant="secondary"
                  onClick={() => {
                    resetSetupWizard();
                    onRerunSetup?.();
                  }}
                >
                  Re-run first-run setup
                </Button>
              </div>
            )}

            {active.id === "about" && (
              <div className="settings-group about-block">
                <p className="credit">
                  Shale {version} — AGPL-3.0-or-later desktop slicer for Bambu Lab printers. CuraEngine
                  runs as a separate process; profiles may come from Bambu Studio or Cura resources.
                </p>
              </div>
            )}
          </motion.div>
        </TabsContent>
      </Tabs>
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
