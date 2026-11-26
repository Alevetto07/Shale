import { motion, useReducedMotion } from "motion/react";
import { lazy, Suspense, useEffect, useRef, useState, type ReactNode } from "react";
import { EASE_OUT } from "./ui/motion";
import type { PrinterInfo, RecentProject } from "./api";
import { PlateField } from "./field/PlateField";
import { uiStyleById, type UiStyleId } from "./ui-styles";
import type { SettingsSectionId } from "./settings/settings-layout";
import { Button, Input } from "./ui";
import { HomeLanding } from "./ui/home-landing";

const SettingsView = lazy(() => import("./SettingsView").then((module) => ({ default: module.SettingsView })));

export type StartPanel = "home" | "printers" | "settings" | "about";

export function StartScreen({
  live,
  panel,
  onPanel,
  canReturn,
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
  onChoosePrinter,
  onEnginePath,
  onLibDir,
  onBackend,
  onApplyEngine,
  onQuit,
  onRerunSetup,
  uiStyle,
  onUiStyle,
  recents,
  onOpenRecent,
  onDropModel,
  onDiscover,
  settingsSection,
}: {
  live: boolean;
  panel: StartPanel;
  onPanel: (panel: StartPanel) => void;
  uiStyle: UiStyleId;
  onUiStyle: (id: UiStyleId) => void;
  canReturn: boolean;
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
  onChoosePrinter: (id: string) => void;
  onEnginePath: (value: string) => void;
  onLibDir: (value: string) => void;
  onBackend: (value: string) => void;
  onApplyEngine: () => void;
  onQuit: () => void;
  onRerunSetup?: () => void;
  recents: RecentProject[];
  onOpenRecent: (path: string) => void;
  onDropModel: (path: string | null) => void;
  onDiscover?: () => void;
  settingsSection?: SettingsSectionId;
}) {
  const [printerQuery, setPrinterQuery] = useState("");
  const plate = uiStyleById(uiStyle);
  const plateStill =
    typeof window !== "undefined" && new URLSearchParams(window.location.search).get("plateStill") === "1";
  const fieldLive = live && !plateStill;

  useEffect(() => {
    const onKey = (event: KeyboardEvent) => {
      const target = event.target as HTMLElement | null;
      const typing = target?.tagName === "INPUT" || target?.tagName === "TEXTAREA";
      if (event.key === "Escape") {
        event.preventDefault();
        if (panel !== "home") onPanel("home");
        else if (canReturn) onReturn();
        return;
      }
      if (typing || event.metaKey || event.ctrlKey || event.altKey) return;
      const key = event.key.toLowerCase();
      const shortcut: Record<string, () => void> = {
        n: onNew,
        o: onOpen,
        i: onImport,
        p: () => onPanel("printers"),
        s: () => onPanel("settings"),
        a: () => onPanel("about"),
        q: onQuit,
        r: () => document.querySelector<HTMLElement>("[data-recent='1']")?.focus(),
      };
      const run = shortcut[key];
      if (!run) return;
      event.preventDefault();
      run();
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [panel, canReturn, onReturn, onNew, onOpen, onImport, onQuit, onPanel]);

  const filteredPrinters = printers
    .filter((printer) => {
      const query = printerQuery.trim().toLowerCase();
      if (!query) return true;
      return `${printer.manufacturer} ${printer.name} ${printer.id}`.toLowerCase().includes(query);
    })
    .slice(0, 40);

  return (
    <div className="start start--hero" aria-label="Start">
      {panel === "home" ? (
        <div className="home-hero" aria-hidden="true" style={{ backgroundImage: `url(${plate.plateImage})` }} />
      ) : null}
      <PlateField
        live={fieldLive && panel !== "home"}
        comet={fieldLive && panel !== "home"}
        imageUrl={plate.plateImage}
        backgroundRgb={plate.plateBackground}
        plateTune={plate.plateTune}
      />
      {panel === "home" ? (
        <HomeLanding
          recents={recents}
          printers={printers}
          printerId={printerId}
          onNew={onNew}
          onOpen={onOpen}
          onImport={onImport}
          onOpenRecent={onOpenRecent}
          onChoosePrinter={onChoosePrinter}
          onDropModel={onDropModel}
          onDiscover={onDiscover}
        />
      ) : null}
      {panel !== "home" ? (
        <div className="start-panel-overlay">
          <StartPanelShell key={panel}>
            {panel === "printers" ? (
              <PrintersPanel
                printers={filteredPrinters}
                printerId={printerId}
                query={printerQuery}
                onQuery={setPrinterQuery}
                onBack={() => onPanel("home")}
                onChoose={(id) => {
                  onChoosePrinter(id);
                  onPanel("home");
                }}
              />
            ) : null}
            {panel === "settings" ? (
              <SettingsOverlay
                version={version}
                printers={printers}
                printerId={printerId}
                enginePath={enginePath}
                libDir={libDir}
                backend={backend}
                uiStyle={uiStyle}
                onUiStyle={onUiStyle}
                onChoosePrinter={onChoosePrinter}
                onEnginePath={onEnginePath}
                onLibDir={onLibDir}
                onBackend={onBackend}
                onApplyEngine={onApplyEngine}
                onRerunSetup={onRerunSetup}
                settingsSection={settingsSection}
                onBack={() => onPanel("home")}
              />
            ) : null}
            {panel === "about" ? (
              <AboutPanel plate={plate} version={version} onBack={() => onPanel("home")} />
            ) : null}
          </StartPanelShell>
        </div>
      ) : null}
    </div>
  );
}

function StartPanelShell({ children }: { children: ReactNode }) {
  const reduce = useReducedMotion();
  return (
    <motion.div
      className="start-panel-sheet"
      initial={reduce ? { opacity: 0 } : { opacity: 0, y: 8 }}
      animate={{ opacity: 1, y: 0 }}
      exit={reduce ? { opacity: 0 } : { opacity: 0, y: 4 }}
      transition={reduce ? { duration: 0 } : { duration: 0.2, ease: [...EASE_OUT] }}
    >
      {children}
    </motion.div>
  );
}

function PrintersPanel({
  printers,
  printerId,
  query,
  onQuery,
  onBack,
  onChoose,
}: {
  printers: PrinterInfo[];
  printerId: string;
  query: string;
  onQuery: (value: string) => void;
  onBack: () => void;
  onChoose: (id: string) => void;
}) {
  const listRef = useRef<HTMLDivElement>(null);
  return (
    <>
      <Button variant="ghost" size="sm" onClick={onBack}>
        Back
      </Button>
      <Input aria-label="Filter printers" value={query} placeholder="Filter" onChange={(e) => onQuery(e.target.value)} />
      <div className="start-scroll" ref={listRef} role="listbox" aria-label="Printers">
        {printers.map((printer) => (
          <button
            key={printer.id}
            type="button"
            role="option"
            aria-selected={printer.id === printerId}
            className={printer.id === printerId ? "selected" : undefined}
            onClick={() => onChoose(printer.id)}
          >
            {`${printer.manufacturer ? `${printer.manufacturer} ` : ""}${printer.name}${
              printer.source === "bambu" ? " · Bambu" : ""
            }`}
          </button>
        ))}
      </div>
    </>
  );
}

function SettingsOverlay(props: {
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
  settingsSection?: SettingsSectionId;
  onBack: () => void;
}) {
  return (
    <>
      <Button variant="ghost" size="sm" onClick={props.onBack}>
        Back
      </Button>
      <Suspense fallback={<div className="ui-sidebar__skeleton" aria-hidden><i style={{ width: "40%" }} /><i style={{ width: "70%" }} /></div>}>
      <SettingsView
        compact
        version={props.version}
        printers={props.printers}
        printerId={props.printerId}
        enginePath={props.enginePath}
        libDir={props.libDir}
        backend={props.backend}
        uiStyle={props.uiStyle}
        onUiStyle={props.onUiStyle}
        onChoosePrinter={props.onChoosePrinter}
        onEnginePath={props.onEnginePath}
        onLibDir={props.onLibDir}
        onBackend={props.onBackend}
        onApplyEngine={props.onApplyEngine}
        onRerunSetup={props.onRerunSetup}
        initialSection={props.settingsSection}
      />
      </Suspense>
    </>
  );
}

function AboutPanel({
  plate,
  version,
  onBack,
}: {
  plate: ReturnType<typeof uiStyleById>;
  version: string;
  onBack: () => void;
}) {
  return (
    <>
      <Button variant="ghost" size="sm" onClick={onBack}>
        Back
      </Button>
      <p className="credit">
        Shale {version} — a desktop slicer. Slicing is done by an unmodified CuraEngine process; printer
        definitions, quality profiles and materials are Cura&apos;s, read as data, and the application is
        licensed under AGPL-3.0-or-later.
      </p>
      <p className="credit">
        What&apos;s new: search MakerWorld and Thingiverse from Models. Prepare can slice several plates
        at once, orient a part onto a hull face, drag a cut, and place a support box. Device shows printer
        messages, files on the machine, and a toast when a print finishes, fails, pauses, or runs out
        of filament. A Bambu send uploads a sliced 3MF, with the tray map in the file and a picture of
        the parts. Support boxes, the tray map, and the other plate choices are kept in the project file.
      </p>
      <p className="credit">
        Home screen: {plate.plateCredit.name} ({plate.plateCredit.object}), {plate.plateCredit.credit}. Released{" "}
        {plate.plateCredit.release}. {plate.plateCredit.licence}.
      </p>
      <p className="credit">
        <a href={plate.plateCredit.page}>{plate.plateCredit.page}</a>
      </p>
      <p className="credit">
        Space Grotesk, Geist Mono, and Inter are used under the SIL Open Font License 1.1. Motion, Radix UI,
        Sonner, cmdk, Number Flow, and clsx are MIT. Lucide is ISC.
      </p>
    </>
  );
}