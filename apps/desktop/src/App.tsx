import { listen } from "./platform/event";
import { getCurrentWindow } from "./platform/window";
import { open, save } from "./platform/dialog";
import { lazy, Suspense, useEffect, useMemo, useRef, useState } from "react";
import {
  api,
  type Bed,
  type CostEstimate,
  type CostSettings,
  type CalibrationApplied,
  type CustomInstruction,
  type MaterialInfo,
  type MeasureSpan,
  type ModelView,
  type SavedModel,
  type OverhangPatch,
  type OpenedProject,
  type PlateSwitch,
  type FitWarning,
  type PlateView,
  type ProjectSettingsImport,
  type FeatureUse,
  type PreviewPath,
  type PrinterInfo,
  type QualityInfo,
  type Status,
  type StoredInstruction,
  type SupportComparison,
  type SupportView,
  type TowerBuilt,
  type Transform,
  type VariantInfo,
  DEFAULT_NONPLANAR,
  type NonplanarReportView,
  type NonplanarSettings,
  type Suggestion,
} from "./api";
import { Suggestions } from "./Suggestions";
import { translationForCentre, worldBox, worldCentre } from "./coords";
import { ModelsView } from "./models/ModelsView";
import { StartScreen, type StartPanel } from "./StartScreen";
import { AppSidebar, readSidebarCollapsed, writeSidebarCollapsed } from "./AppSidebar";
import { TitleBarNav } from "./TitleBarNav";
import { TabBar, type DocTab, type Pane, type Spool } from "./Tabs";
import { FeatureLegend } from "./Legend";
import { ObjectOverrides } from "./object-overrides";
import { plateWarnings } from "./print-check";
import { batchFraction, batchMessage, platesToSlice } from "./slice-all";
import { thumbnailJpeg } from "./thumbnail";
import { variantOptionLabel } from "./profile-labels";
import { toast } from "sonner";
import { Frame } from "./theme";
import { MenuBar } from "./MenuBar";
import { Dialog } from "./ui/dialog";
import { SliceDock } from "./ui/slice-dock";
import { applyUiStyle, readUiStyle, writeUiStyle, type UiStyleId } from "./ui-styles";
import {
  nonplanarFromSliceSettings,
  overridesFromSliceSettings,
  QUICK_SETTING_KEYS,
  quickSettingsView,
  userSettingsForEngine,
} from "./engine-settings";
import type { QuickFallbacks } from "./ProcessSheet";
import { pickQualityType } from "./profile-pick";
import type { Section } from "./settings-meta";
import type { SettingsSectionId } from "./settings/settings-layout";
import { NonPlanarPanel } from "./ui/non-planar-panel";
import { sliceStage } from "./ui/slice-stage";
import { TitleBar } from "./TitleBar";
import {
  CommandPalette,
  Meter,
  NumericControl,
  Select,
  Slider,
  SwitchRow,
  TextField,
  ThemedToaster,
  TooltipProvider,
  toastUndo,
} from "./ui";
import { buildCommandPaletteItems, nextUiStyle } from "./command-palette-items";
import { isPaletteShortcut } from "./ui/motion";
import { ShellFrame } from "./ui/shell-frame";
import { PaneFade, ViewSwap } from "./ui/view-swap";
import { UiShowcase } from "./UiShowcase";
import { shouldShowSetupWizard } from "./setup-wizard";
import { useRecents } from "./recents";
import { slopeRampFromDocument, type TransformMode, type ViewStyle, type ColourMode } from "./viewport-style";

const Viewport = lazy(() =>
  import("./Viewport").then((module) => ({ default: module.Viewport })),
);
const DevicePane = lazy(() =>
  import("./DevicePane").then((module) => ({ default: module.DevicePane })),
);
const SetupWizard = lazy(() =>
  import("./SetupWizard").then((module) => ({ default: module.SetupWizard })),
);

interface Progress {
  fraction: number;
  message: string;
  layer?: {
    index: number;
    z: number;
    paths: PreviewPath[];
  } | null;
}

const FALLBACK_BED: Bed = {
  width: 220,
  depth: 220,
  height: 250,
  center_is_zero: false,
  layer_height: 0.2,
  infill_sparse_density: 20,
  support_enable: false,
  nozzle_temperature: 200,
  bed_temperature: 60,
  nozzle_size: 0.4,
  line_width: 0.4,
  adhesion_type: "skirt",
  speed_print: 50,
  material_diameter: 1.75,
  warning_count: 0,
};

/// The plate a window opens with. A window always has one tab, so a plate
/// always has somewhere to be put away.
/// The machine and material the window was last left on. Bambu Studio comes
/// back to what you were printing on; a slicer that resets to an Ender-3 every
/// time it starts makes you choose again for no reason. Storage can be missing
/// or full (a private window, a locked profile), so the read is total.
type BootProfile = { printerId: string; variant: string | null; quality: string | null; material: string | null };

function readBootProfile(): BootProfile | null {
  try {
    const raw = window.localStorage.getItem("shale.profile");
    if (!raw) return null;
    const parsed = JSON.parse(raw) as Partial<BootProfile>;
    if (typeof parsed.printerId !== "string" || !parsed.printerId) return null;
    return {
      printerId: parsed.printerId,
      variant: typeof parsed.variant === "string" ? parsed.variant : null,
      quality: typeof parsed.quality === "string" ? parsed.quality : null,
      material: typeof parsed.material === "string" ? parsed.material : null,
    };
  } catch {
    return null;
  }
}

const BOOT_PROFILE = readBootProfile();

const FIRST_TAB: DocTab[] = [{ id: 1, name: "Untitled", path: null, scratch: null, dirty: false }];

/// One slice, kept. A person compares outcomes — this plate, that infill — so
/// the result of a slice outlives the next one instead of being overwritten by
/// it. The toolpath is kept with it, because "show me slice 2" has to draw
/// slice 2.
interface SliceRecord {
  n: number;
  seconds: number | null;
  metres: number | null;
  grams: number | null;
  layers: number;
  total: number | null;
  /// The plate and the settings that produced it, so a record can say how far
  /// it is from the settings on screen now.
  plate: string;
  settings: string;
  paths: PreviewPath[];
}

/// Which of the recorded settings differ from the ones on screen. The picker
/// says "3 changed", and the names are there for a title.
function settingsDiff(before: string, now: string): string[] {
  try {
    const a = JSON.parse(before) as Record<string, unknown>;
    const b = JSON.parse(now) as Record<string, unknown>;
    return Object.keys(a).filter((key) => JSON.stringify(a[key]) !== JSON.stringify(b[key]));
  } catch {
    return [];
  }
}

export function App() {
  const [status, setStatus] = useState<Status | null>(null);
  const [printers, setPrinters] = useState<PrinterInfo[]>([]);
  const [printerQuery, setPrinterQuery] = useState("");
  const [printerId, setPrinterId] = useState("");
  const [variants, setVariants] = useState<VariantInfo[]>([]);
  const [qualities, setQualities] = useState<QualityInfo[]>([]);
  const [materials, setMaterials] = useState<MaterialInfo[]>([]);
  const [variant, setVariant] = useState<string | null>(null);
  const [quality, setQuality] = useState<string | null>(null);
  // The numbers the chosen profile carries, fetched once per printer/quality/
  // material: the sheet shows them, so a field you have not touched is not a
  // blank and a switch you have not touched is not lying about the machine.
  const [profileValues, setProfileValues] = useState<Record<string, string>>({});
  const [material, setMaterial] = useState<string | null>(null);
  useEffect(() => {
    if (!printerId) {
      setProfileValues({});
      return;
    }
    let live = true;
    api
      .profileSettings(printerId, variant, quality, material)
      .then((values) => {
        if (!live) return;
        const profile = values ?? {};
        setProfileValues(profile);
      })
      .catch(() => {
        if (live) setProfileValues({});
      });
    return () => {
      live = false;
    };
  }, [printerId, variant, quality, material]);

  // The AMS holds three more spools beside the plate's own material. An empty
  // string is an empty bay, so the slot count is derived instead of stored.
  const [ams, setAms] = useState<string[]>(["", "", ""]);
  const [amsMapping, setAmsMapping] = useState<number[]>([]);
  const [bed, setBed] = useState<Bed>(FALLBACK_BED);
  // Cura settings the user changed in the menu; anything absent is the profile's.
  const [overrides, setOverrides] = useState<Record<string, string>>({});
  const [suggestions, setSuggestions] = useState<Suggestion[]>([]);
  const [dismissedSuggestions, setDismissedSuggestions] = useState<string[]>([]);
  const [suggestionsShown, setSuggestionsShown] = useState<string[]>([]);
  const suggestionsSeen = useRef<Set<string>>(new Set());
  useEffect(() => {
    overridesRef.current = overrides;
  }, [overrides]);
  const lastMaterial = useRef<string | null | undefined>(undefined);
  useEffect(() => {
    const prev = lastMaterial.current;
    lastMaterial.current = material;
    if (prev === undefined || prev === material) return;
    setOverrides((current) => {
      const next = { ...current };
      delete next[QUICK_SETTING_KEYS.nozzle];
      delete next[QUICK_SETTING_KEYS.bed];
      return next;
    });
  }, [material]);
  const [models, setModels] = useState<ModelView[]>([]);
  /// The part being painted, the patches it offers and how many of its
  /// triangles are painted. Painting is per part, keyed on the part's own
  /// triangles, so it survives a move or a turn.
  const [paintTarget, setPaintTarget] = useState<number | null>(null);
  const [patches, setPatches] = useState<OverhangPatch[]>([]);
  const [painted, setPainted] = useState(0);
  /// Faces painted as needing no support. The two strokes are counted apart
  /// because they ask for opposite things.
  const [blocked, setBlocked] = useState(0);
  /// Which brush the next patch press uses.
  const [paintMode, setPaintMode] = useState<"enforce" | "block">("enforce");
  /// The brush is a mode: while it is in hand a drag paints the surface under
  /// the cursor, and a click is not a selection.
  const [paintArmed, setPaintArmed] = useState(false);
  const [boxArmed, setBoxArmed] = useState(false);
  const [boxHeight, setBoxHeight] = useState("8");
  const [supportBoxes, setSupportBoxes] = useState<
    Array<{ min: [number, number, number]; max: [number, number, number]; block: boolean }>
  >([]);
  const [faceArmed, setFaceArmed] = useState(false);
  const [paintRadius, setPaintRadius] = useState("6");
  /// The plates in this project. The one flagged `active` is the one `models`
  /// holds; the rest live in the backend until they are turned to.
  const [plates, setPlates] = useState<PlateView[]>([]);
  const [fit, setFit] = useState<FitWarning | null>(null);
  const [embedded, setEmbedded] = useState<ProjectSettingsImport | null>(null);
  const [ignoreEmbedded, setIgnoreEmbedded] = useState(false);
  const [confirmExport, setConfirmExport] = useState<null | "gcode" | "stl" | "3mf">(null);
  // Part counts are the backend's, because it holds the plates that are not in
  // front. A part arriving or leaving is the only thing that changes them.
  useEffect(() => {
    void api.listPlates().then((next) => setPlates(next ?? [])).catch(() => setPlates([]));
  }, [models.length]);

  /// Which part the settings sheet writes to: "all" for the plate, or a part id.
  const [scope, setScope] = useState<string>("all");
  const [selectedId, setSelectedId] = useState<number | null>(null);
  /// What the selected part measures right now: triangles, volume, and whether
  /// its surface closes. Re-measured whenever the plate changes, because a cut
  /// or a boolean changes all three.
  const [health, setHealth] = useState<{
    triangles: number;
    openEdges: number;
    volumeMm3: number;
  } | null>(null);
  const [mode, setMode] = useState<TransformMode>("translate");
  /// The cut plane: which axis it faces, how far from the part's centre, and
  /// whether to keep both halves or one.
  const [cutAxis, setCutAxis] = useState(2);
  const [cutOffset, setCutOffset] = useState("0");
  const [cutKeep, setCutKeep] = useState("both");
  /// Lay each kept half so the face the plane made sits on the bed.
  const [cutFaceDown, setCutFaceDown] = useState(true);
  /// Pins turn the two halves back into one part: a round pin on the lower
  /// half, a hole of the same size plus clearance in the upper.
  const [cutPins, setCutPins] = useState(0);
  /// How wide a dovetail's root is, in millimetres; 0 means the cut has no dovetail.
  const [cutDovetail, setCutDovetail] = useState(0);
  const [pinRadius, setPinRadius] = useState("2");
  const [pinHeight, setPinHeight] = useState("5");
  /// The ruler: while measuring, a click on a part drops a point instead of
  /// selecting it. Two points make a span, the number comes from the core.
  const [measuring, setMeasuring] = useState(false);
  const [measureMarks, setMeasureMarks] = useState<Array<[number, number, number]>>([]);
  const [measureSpan, setMeasureSpan] = useState<MeasureSpan | null>(null);
  const [viewStyle, setViewStyle] = useState<ViewStyle>("solid");
  const [cameraPreset, setCameraPreset] = useState<"iso" | "top" | "front">("iso");
  const [cameraToken, setCameraToken] = useState(0);
  const [uiStyle, setUiStyle] = useState<UiStyleId>(() => readUiStyle());
  const [setupWizardOpen, setSetupWizardOpen] = useState(() => {
    if (import.meta.env.VITE_SHALE_MOCK === "1") {
      return new URLSearchParams(window.location.search).get("setup") === "1";
    }
    return shouldShowSetupWizard();
  });
  const chooseUiStyle = (id: UiStyleId) => {
    setUiStyle(id);
    writeUiStyle(id);
    applyUiStyle(id);
  };
  const [slopeRamp, setSlopeRamp] = useState(slopeRampFromDocument);
  useEffect(() => {
    setSlopeRamp(slopeRampFromDocument());
  }, [uiStyle]);
  const toggleUiStyle = () => chooseUiStyle(nextUiStyle(uiStyle));
  /// What the toolpath is coloured by, as Bambu's preview lets you choose.
  const [colourMode, setColourMode] = useState<ColourMode>("type");
  const [busy, setBusy] = useState(false);
  const [screen, setScreen] = useState<"start" | "slicer">("start");
  const [workspace, setWorkspace] = useState(false);
  /// Which pane the window is pointed at. Home is the start screen; the others
  /// are the plate, the toolpath, and the machine.
  const [pane, setPane] = useState<Pane>("home");
  /// Open projects. A tab holds a name, a file, and whether the file is behind
  /// the plate; switching saves the tab you leave so nothing is lost.
  // A window always has a plate in it, so it always has a tab: the tab strip is
  // the only thing that can say where a plate went, and a plate with no tab has
  // nowhere to be put away.
  const [docTabs, setDocTabs] = useState<DocTab[]>(FIRST_TAB);
  const [activeTab, setActiveTab] = useState(FIRST_TAB[0].id);
  const tabSeq = useRef(2);
  const [projectPath, setProjectPath] = useState<string | null>(null);
  const [projectName, setProjectName] = useState("Untitled");
  // The recent-projects list is data, not drawing: the Home panel is restyled
  // elsewhere and only asks this for what to show.
  const recents = useRecents();
  const [sidebarCollapsed, setSidebarCollapsed] = useState(readSidebarCollapsed);
  const [commandPaletteOpen, setCommandPaletteOpen] = useState(false);
  const plateUndoRef = useRef<{ plates: PlateView[]; models: ModelView[]; selected: number | null } | null>(null);
  const [startPanel, setStartPanel] = useState<StartPanel>("home");
  const [startSettingsSection, setStartSettingsSection] = useState<SettingsSectionId | undefined>();
  const toggleSidebar = () => {
    setSidebarCollapsed((value) => {
      const next = !value;
      writeSidebarCollapsed(next);
      return next;
    });
  };
  const [notice, setNotice] = useState<string | null>(null);
  const restore = useRef<Restore | null>(null);
  const catalogLoadingRef = useRef(false);
  const catalogAttemptsRef = useRef(0);
  const saveRef = useRef<(asNew: boolean) => void>(() => {});
  const openRef = useRef<(path?: string) => void>(() => {});
  const [progress, setProgress] = useState<Progress | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [preview, setPreview] = useState(false);
  // The toolpath is what you came to look at; the ghosted model is one click
  // away. Starting with both on made the first preview after a slice read as a
  // wash of grey.
  const [showModel, setShowModel] = useState(false);
  const [layer, setLayer] = useState(0);
  /// Preview plays the toolpath a layer at a time, the way a print would.
  const [playing, setPlaying] = useState(false);
  const [layerCount, setLayerCount] = useState(0);

  // A demo that is about the toolpath has to end up on Preview once the slice
  // has arrived: boot can run before the preload is installed, and the pane
  // would otherwise stay on Prepare for the whole clip.
  useEffect(() => {
    if (!status?.demo) return;
    if (status.demo !== "preview" && status.demo !== "multi") return;
    if (layerCount > 0) setPane("preview");
  }, [status?.demo, layerCount]);
  const [layerZ, setLayerZ] = useState<number[]>([]);
  const [paths, setPaths] = useState<PreviewPath[]>([]);
  const [features, setFeatures] = useState<FeatureUse[]>([]);
  /// Kinds turned off in the legend. The toolpath stays loaded; this only
  /// decides what is drawn, so switching a kind back on is instant.
  // Travel is not something a person wants to see by default: it is the
  // nozzle going nowhere. The legend still offers it.
  const [hiddenKinds, setHiddenKinds] = useState<Set<string>>(new Set(["travel"]));
  const [printSeconds, setPrintSeconds] = useState<number | null>(null);
  const [filamentMetres, setFilamentMetres] = useState<number | null>(null);
  // Price, power and tariff are the user's numbers, not the engine's; they are
  // read once and written whenever a field changes, so a reinstall keeps them.
  const [cost, setCost] = useState<CostSettings>({
    currency: "EUR",
    price_per_kg: 20,
    watts: 100,
    electricity_per_kwh: 0.3,
  });
  const [estimate, setEstimate] = useState<CostEstimate | null>(null);
  /// Every slice taken this session, newest last, and which one is on screen.
  const [history, setHistory] = useState<SliceRecord[]>([]);
  const [viewing, setViewing] = useState<number | null>(null);

  // What the plate *is*, near enough to tell whether a slice still describes
  // it: every part, where it sits and how big it is. A part leaving, arriving
  // or moving makes the last slice a description of some other plate.
  const plateKey = models
    .map(
      (model) =>
        `${model.id}:${model.transform.translation.join(",")}:${model.transform.rotation_deg.join(",")}:${model.transform.scale.join(",")}`,
    )
    .join("|");
  const slicedPlate = useRef<string | null>(null);

  const plateOverrides = useMemo(() => {
    if (!embedded || ignoreEmbedded) return overrides;
    const next = { ...overrides };
    for (const setting of embedded.applied) {
      if (!(setting.key in next)) next[setting.key] = setting.value;
    }
    return next;
  }, [overrides, embedded, ignoreEmbedded]);

  const scopedOverrides = useMemo(
    () =>
      scope === "all"
        ? plateOverrides
        : (models.find((m) => m.id === Number(scope))?.settings ?? {}),
    [scope, plateOverrides, models],
  );

  const quickFallbacks = useMemo(
    (): QuickFallbacks => ({
      layerHeight: bed.layer_height,
      walls: Number(profileValues.wall_line_count) || 2,
      infill: bed.infill_sparse_density,
      pattern: profileValues.infill_pattern || "grid",
      supports: bed.support_enable,
      speed: bed.speed_print,
      nozzle: bed.nozzle_temperature,
      bed: bed.bed_temperature,
    }),
    [bed, profileValues],
  );

  const quick = useMemo(
    () => quickSettingsView(profileValues, scopedOverrides, quickFallbacks),
    [profileValues, scopedOverrides, quickFallbacks],
  );

  const [nonplanar, setNonplanar] = useState<NonplanarSettings>(DEFAULT_NONPLANAR);
  const [nonplanarReport, setNonplanarReport] = useState<NonplanarReportView | null>(null);
  const [nonplanarOk, setNonplanarOk] = useState(true);
  const [liveSlice, setLiveSliceState] = useState(() => {
    try {
      return window.localStorage.getItem("shale.live-slice") !== "0";
    } catch {
      return true;
    }
  });
  const [settingsNonce, setSettingsNonce] = useState(0);
  const [settingsSection, setSettingsSection] = useState<Section | null>(null);
  const sliceQuiet = useRef<() => void>(() => undefined);

  function setLiveSlice(next: boolean) {
    setLiveSliceState(next);
    try {
      window.localStorage.setItem("shale.live-slice", next ? "1" : "0");
    } catch {
      // The preference is session-only when storage is blocked.
    }
  }

  const engineSettings = useMemo(
    () => userSettingsForEngine(scopedOverrides, nonplanar),
    [scopedOverrides, nonplanar],
  );

  const { layerHeight, infill, supports, nozzle, bed: bedTemp } = quick;

  /// What a slice was made from. The plate says where the parts are, this says
  /// what the engine was told to do with them; a change to either makes the
  /// last slice a description of something else.
  const settingsKey = JSON.stringify({
    quick,
    material,
    quality,
    variant,
    scopedOverrides,
    nonplanar,
  });
  const lastSlice = history.length > 0 ? history[history.length - 1] : null;
  const stale =
    layerCount > 0 &&
    lastSlice != null &&
    (lastSlice.plate !== plateKey || lastSlice.settings !== settingsKey);

  useEffect(() => {
    // Nothing was sliced, or the plate is still the one that was sliced.
    if (slicedPlate.current === null || slicedPlate.current === plateKey) return;
    slicedPlate.current = null;
    setLayerCount(0);
    setLayerZ([]);
    setPrintSeconds(null);
    setFilamentMetres(null);
  }, [plateKey]);

  useEffect(() => {
    if (!printerId) return;
    let cancelled = false;
    void api
      .nonplanarSupported(printerId)
      .then((ok) => {
        if (!cancelled) setNonplanarOk(ok);
      })
      .catch(() => {
        if (!cancelled) setNonplanarOk(false);
      });
    return () => {
      cancelled = true;
    };
  }, [printerId]);

  useEffect(() => {
    if (!liveSlice || !stale || nonplanar.mode !== "off" || busy) return;
    const id = window.setTimeout(() => sliceQuiet.current(), 1100);
    return () => window.clearTimeout(id);
  }, [liveSlice, stale, nonplanar.mode, settingsKey, busy]);

  const [enginePath, setEnginePath] = useState("");
  const [libDir, setLibDir] = useState("");
  const [backend, setBackend] = useState("cli");
  const [instructions, setInstructions] = useState<StoredInstruction[]>([]);
  const [kind, setKind] = useState<InstructionKind>("pause");
  const [anchor, setAnchor] = useState<"layer" | "height">("layer");
  const [at, setAt] = useState(40);
  const [note, setNote] = useState("");
  const [filament, setFilament] = useState(true);
  const [tempNozzle, setTempNozzle] = useState(200);
  const [tempBed, setTempBed] = useState(60);
  const [tempWait, setTempWait] = useState(false);
  const [fanPercent, setFanPercent] = useState(50);
  const [support, setSupport] = useState<SupportView | null>(null);
  const [supportAngle, setSupportAngle] = useState(50);
  const [bridgeMm, setBridgeMm] = useState(20);
  const [supportBusy, setSupportBusy] = useState(false);
  const [tower, setTower] = useState<TowerBuilt | null>(null);
  const [calibration, setCalibration] = useState<CalibrationApplied | null>(null);
  const [calibrating, setCalibrating] = useState(false);
  const [frameToken, setFrameToken] = useState(0);
  const [chord, setChord] = useState(0.1);
  const [turn, setTurn] = useState(15);
  const [chordTied, setChordTied] = useState(true);
  const [importing, setImporting] = useState(false);
  const [importProgress, setImportProgress] = useState<Progress | null>(null);

  useEffect(() => {
    let stopped = false;
    let retry: number | undefined;
    let catalogPoll: number | undefined;
    // A demo scenario slices before the window opens, so the first boot can
    // legitimately see an empty plate. Ask again instead of filming the start
    // screen.
    let demoPolls = 0;
    /** The window opens before the profile library is read, so the first few
     * attempts legitimately come back empty. Ask again rather than showing an
     * error for a disk that is simply slow. */
    const boot = async () => {
      try {
        const next = await api.status();
        if (stopped) return;
        setStatus(next);
        catalogLoadingRef.current =
          !!next.catalog_loading || (next.resources_ok && next.catalog_full === false);
        setEnginePath(next.engine_path);
        setLibDir(next.engine_lib_dir || "");
        setBackend(next.engine_backend || "cli");
        if (!next.resources_ok && next.catalog_loading) {
          catalogAttemptsRef.current += 1;
          if (catalogAttemptsRef.current > 180) {
            setError(
              "Reading the printer and material profiles timed out — close other heavy builds and relaunch, or check SHALE_CURA_DIR.",
            );
            return;
          }
          window.clearTimeout(retry);
          retry = window.setTimeout(() => void boot(), 700);
          return;
        }
        catalogAttemptsRef.current = 0;
        if (!next.resources_ok) {
          setError(next.resources_message);
        }
        if (
          next.resources_ok &&
          next.engine_checked &&
          !next.engine_ok &&
          next.engine_message
        ) {
          setError(next.engine_message);
        }
        if (next.resources_ok) {
          const [list, loaded, existing] = await Promise.all([
            api.printers(),
            api.loadedModels(),
            api.existingSlice(),
          ]);
          setPrinters(list);
          if (loaded.length > 0) {
            setModels(loaded);
            setSelectedId(loaded[0].id);
          }
          const stored = await api.instructions();
          setInstructions(stored ?? []);
          setSupport(await api.supportPlan());
          // A slice describes the plate it was taken from. If the plate has
          // changed since — a part added, a part taken away — the old numbers
          // are about some other plate and the job card must not wear them.
          const samePlate =
            existing !== null &&
            existing.model_ids.length === loaded.length &&
            existing.model_ids.every((id) => loaded.some((model) => model.id === id));
          if (existing && samePlate && existing.layer_count > 0) {
            const zs = existing.layers.map((item) => item.z);
            setLayerCount(existing.layer_count);
            setLayerZ(zs);
            // The card's other two figures came back as dashes on a reload,
            // because only the layers were put back.
            setPrintSeconds(existing.print_time_seconds);
            setFilamentMetres(existing.filament_metres);
            const marked = instructionMarks(stored, zs, 0.2);
            const focus = marked.find((index) => index >= 0 && index < existing.layer_count);
            setLayer(focus ?? Math.max(0, Math.floor(existing.layer_count / 2)));
            setPreview(true);
            setShowModel(false);
          }
          const remembered = BOOT_PROFILE
            ? list.find((printer) => printer.id === BOOT_PROFILE.printerId)
            : undefined;
          const preferred =
            remembered ??
            list.find((printer) => printer.id === "bambu_lab_a1") ??
            list[0];
          // The printer effect below picks the machine's own preferred variant,
          // quality and material unless something is waiting in `restore`; a
          // remembered choice has to be there before the printer id is set.
          if (remembered && BOOT_PROFILE) {
            restore.current = {
              variant: BOOT_PROFILE.variant,
              quality: BOOT_PROFILE.quality,
              material: BOOT_PROFILE.material,
            };
          }
          if (preferred) setPrinterId(preferred.id);
          if (loaded.length > 0 || next.demo_model) {
            setScreen("slicer");
            setWorkspace(true);
            // The workspace is chosen by the pane, not by the screen: a demo
            // that only set the screen still opened on Home.
            if (next.demo) {
              // The clips say what they are about: "preview" and "multi" are
              // about the toolpath, the rest about the plate.
              setPane(
                next.demo === "preview" || next.demo === "multi"
                  ? "preview"
                  : "prepare",
              );
            }
          }
          if (next.demo && loaded.length === 0 && demoPolls < 60) {
            demoPolls += 1;
            window.clearTimeout(retry);
            retry = window.setTimeout(() => void boot(), 500);
          }
          // A demo scenario is a state, not a script: the queue video starts
          // with the Device pane already open rather than hunting for a tab.
          if (next.demo === "queue") setPane("device");
          if (import.meta.env.VITE_SHALE_MOCK === "1") {
            const mockPane = new URLSearchParams(window.location.search).get("pane");
            if (mockPane === "prepare" || mockPane === "preview" || mockPane === "device" || mockPane === "models") {
              setScreen("slicer");
              setWorkspace(true);
              setPane(mockPane);
              if (mockPane === "preview") {
                setPreview(true);
                setShowModel(false);
              }
              if (mockPane === "prepare") {
                setPreview(false);
                setShowModel(true);
              }
            } else if (mockPane === "home") {
              setScreen("start");
              setPane("home");
            }
          }
          recents.refresh();
        }
      } catch (err) {
        if (!stopped) setError(messageOf(err));
      }
    };
    let offCatalogue: (() => void) | undefined;
    const onCatalogue = () => {
      window.clearTimeout(retry);
      void boot();
    };
    void listen("catalogue", onCatalogue).then((off) => {
      offCatalogue = off;
      if (!stopped) void boot();
    });
    void listen("engine-ready", () => {
      if (!stopped) void boot();
    }).catch(() => {});
    catalogPoll = window.setInterval(() => {
      if (stopped) return;
      if (catalogLoadingRef.current) void boot();
    }, 1500);
    const onFocus = () => {
      if (!stopped) void boot();
    };
    window.addEventListener("focus", onFocus);
    return () => {
      stopped = true;
      window.clearTimeout(retry);
      window.clearInterval(catalogPoll);
      window.removeEventListener("focus", onFocus);
      offCatalogue?.();
    };
  }, []);

  useEffect(() => {
    if (!printerId || !variant || !quality) return;
    try {
      window.localStorage.setItem(
        "shale.profile",
        JSON.stringify({ printerId, variant, quality, material }),
      );
    } catch {
      // A window that cannot write storage still slices; remembering is a
      // convenience, not a precondition.
    }
  }, [printerId, variant, quality, material]);

  useEffect(() => {
    if (!printerId || printers.length === 0) return;
    if (printers.some((printer) => printer.id === printerId)) return;
    const fallback =
      printers.find((printer) => printer.id === "bambu_lab_a1")?.id ?? printers[0]?.id;
    if (fallback) setPrinterId(fallback);
  }, [printers, printerId]);

  const pendingQuality = useRef<string | null | undefined>(undefined);
  const lastVariant = useRef<string | null>(null);

  useEffect(() => {
    if (!printerId) return;
    let cancelled = false;
    void (async () => {
      try {
        const [nextVariants, nextMaterials] = await Promise.all([
          api.variants(printerId),
          api.materials(printerId),
        ]);
        if (cancelled) return;
        setVariants(nextVariants);
        setMaterials(nextMaterials);
        const pending = restore.current;
        restore.current = null;
        const known = <T,>(wanted: T | null | undefined, offers: (item: T) => boolean) =>
          wanted != null && offers(wanted);
        const printer = printers.find((item) => item.id === printerId);
        const nextVariant = known(pending?.variant ?? null, (name) =>
          nextVariants.some((item) => item.name === name),
        )
          ? (pending?.variant as string)
          : nextVariants.some((item) => item.name === printer?.preferred_variant)
            ? (printer?.preferred_variant ?? nextVariants[0]?.name ?? null)
            : (nextVariants[0]?.name ?? null);
        pendingQuality.current = pending?.quality ?? null;
        const nextMaterial = known(pending?.material ?? null, (id) =>
          nextMaterials.some((item) => item.id === id),
        )
          ? (pending?.material as string)
          : nextMaterials.some((item) => item.id === printer?.preferred_material)
            ? (printer?.preferred_material ?? nextMaterials[0]?.id ?? null)
            : (nextMaterials.find((item) => item.id.startsWith("generic_pla"))?.id ??
              nextMaterials[0]?.id ??
              null);
        lastVariant.current = null;
        setVariant(nextVariant);
        setMaterial(nextMaterial);
      } catch (err) {
        if (!cancelled) setError(messageOf(err));
      }
    })();
    return () => {
      cancelled = true;
    };
  }, [printerId, printers]);

  useEffect(() => {
    if (!printerId || !variant) {
      setQualities([]);
      return;
    }
    let cancelled = false;
    const nozzleChanged = lastVariant.current !== null && lastVariant.current !== variant;
    lastVariant.current = variant;
    void (async () => {
      try {
        const nextQualities = await api.qualities(printerId, variant);
        if (cancelled) return;
        setQualities(nextQualities);
        const printer = printers.find((item) => item.id === printerId);
        const remembered = pendingQuality.current;
        pendingQuality.current = undefined;
        setQuality((current) =>
          pickQualityType(nextQualities, {
            pending: remembered,
            current,
            printer,
            nozzleChanged,
          }),
        );
      } catch (err) {
        if (!cancelled) setError(messageOf(err));
      }
    })();
    return () => {
      cancelled = true;
    };
  }, [printerId, variant, printers]);

  useEffect(() => {
    if (!printerId) return;
    let cancelled = false;
    void (async () => {
      try {
        const next = await api.bed(printerId, variant, quality, material);
        if (cancelled) return;
        setBed(next);
        setError((current) => (current && current.includes("unknown quality") ? null : current));
      } catch (err) {
        if (!cancelled) setError(messageOf(err));
      }
    })();
    return () => {
      cancelled = true;
    };
  }, [printerId, variant, quality, material]);

  useEffect(() => {
    const unlistenImport = listen<Progress>("import-progress", (event) => {
      setImportProgress(event.payload);
    });
    const unlisten = listen<Progress>("slice-progress", (event) => {
      const job = batch.current;
      setProgress(
        job && job.total > 0
          ? {
              fraction: batchFraction(job.done, job.total, event.payload.fraction),
              message: batchMessage(job.done, job.total, job.name, event.payload.message),
              layer: event.payload.layer,
            }
          : event.payload,
      );
      const live = event.payload.layer;
      if (live && live.paths.length > 0) {
        setPreview(true);
        setPaths(live.paths);
        setLayer(live.index);
        setLayerCount((count) => Math.max(count, live.index + 1));
        setLayerZ((zs) => {
          const next = zs.slice();
          next[live.index] = live.z;
          return next;
        });
      }
    });
    return () => {
      void unlistenImport.then((stop) => stop());
      void unlisten.then((stop) => stop());
    };
  }, []);

  // Undo is a stack of plate snapshots. Deleting a part only hides it in the
  // backend, so a snapshot is a list of ids and transforms: no geometry is
  // copied, and undoing a delete does not re-read a file.
  // One history for everything a hand can change, in the order it happened:
  // moving a part, painting a face, and typing a number into the sheet are all
  // steps in the same past, so Ctrl+Z always takes back the last thing rather
  // than whichever stack happened to be non-empty.
  type HistoryEntry =
    | { kind: "models"; models: ModelView[]; selected: number | null }
    | { kind: "settings"; overrides: Record<string, string> }
    | { kind: "partSettings"; id: number; settings: Record<string, string> }
    | { kind: "paint" };
  const past = useRef<HistoryEntry[]>([]);
  const future = useRef<HistoryEntry[]>([]);
  // The sheet's numbers as they are right now, for a snapshot taken inside a
  // handler that would otherwise capture an older render's copy.
  const overridesRef = useRef<Record<string, string>>({});
  const partSettingsTimer = useRef<number | null>(null);
  const partSettingsPending = useRef<{ id: number; settings: Record<string, string> } | null>(null);
  const modelsRef = useRef<ModelView[]>(models);
  const platesRef = useRef<PlateView[]>(plates);
  /// Set while "slice all" walks the plates, so one engine fraction becomes a
  /// fraction of the whole batch and the bar can name the plate.
  const batch = useRef<{ done: number; total: number; name: string } | null>(null);
  const lastEdit = useRef<{ key: string; at: number } | null>(null);
  const [historyDepth, setHistoryDepth] = useState({ past: 0, future: 0 });

  const syncHistory = () =>
    setHistoryDepth({ past: past.current.length, future: future.current.length });

  useEffect(() => {
    modelsRef.current = models;
    platesRef.current = plates;
  }, [models, plates]);

  /// The tab bar's bookkeeping. `markTabDirty` is called from `remember`, so
  /// every edit that can be undone also marks the file as behind — one place,
  /// and no handler can forget it.
  // The tab strip is read and written inside one async command at a time
  // (write this plate, then open that one), and React's state copy is a render
  // behind. A ref beside it is the only way to read the tab you just wrote.
  const activeTabRef = useRef(1);
  // The ref has to start where the state starts, or the first actions of a
  // session see a window with no tabs at all.
  const tabsRef = useRef<DocTab[]>(FIRST_TAB);

  function setTabs(next: DocTab[] | ((current: DocTab[]) => DocTab[])) {
    const value = typeof next === "function" ? next(tabsRef.current) : next;
    tabsRef.current = value;
    setDocTabs(value);
  }

  function activate(id: number) {
    activeTabRef.current = id;
    setActiveTab(id);
  }

  function markTabDirty() {
    setTabs((current) =>
      current.map((tab) => (tab.id === activeTabRef.current && !tab.dirty ? { ...tab, dirty: true } : tab)),
    );
  }

  function addTab(name: string, path: string | null): number {
    const id = tabSeq.current++;
    setTabs((current) => [...current, { id, name, path, scratch: null, dirty: false }]);
    activate(id);
    return id;
  }

  /// Give a tab its file. A tab that was opened from a file takes the file's
  /// name; one that was only put away quietly keeps the name it had, because
  /// "Untitled 2" is what the person called it.
  function nameTab(path: string | null, name: string, id?: number) {
    setTabs((current) =>
      current.map((tab) => {
        const mine = id != null ? tab.id === id : tab.path === path || tab.id === activeTabRef.current;
        if (!mine) return tab;
        return { ...tab, name: tab.path === path && tab.path != null ? tab.name : name, path: path ?? tab.path, dirty: false };
      }),
    );
  }

  /** Remember the plate as it is now, before it changes. */
  function remember(key = "") {
    const now = Date.now();
    if (key && lastEdit.current && lastEdit.current.key === key && now - lastEdit.current.at < 1200) {
      lastEdit.current.at = now;
      return;
    }
    lastEdit.current = key ? { key, at: now } : null;
    markTabDirty();
    past.current.push({ kind: "models", models: modelsRef.current, selected: selectedId });
    if (past.current.length > 80) past.current.shift();
    future.current = [];
    syncHistory();
  }

  /** One step in the sheet, so a number typed there is a step Ctrl+Z can take
   * back. The scope's own settings are not in this history yet; the sheet's
   * global numbers are, and they are the ones the slice reads first. */
  function rememberSettings() {
    past.current.push({ kind: "settings", overrides: { ...overridesRef.current } });
    if (past.current.length > 80) past.current.shift();
    future.current = [];
    markTabDirty();
    syncHistory();
  }

  async function restoreSnapshot(snapshot: { models: ModelView[]; selected: number | null }) {
    try {
      const views = await api.restoreModels(
        snapshot.models.map((model) => ({ id: model.id, transform: model.transform })),
      );
      const shown = new Map(views.map((view) => [view.id, view]));
      setModels(
        snapshot.models.map((model) => {
          const view = shown.get(model.id);
          return view ? { ...model, transform: view.transform } : model;
        }),
      );
      setSelectedId(snapshot.selected && shown.has(snapshot.selected) ? snapshot.selected : null);
      setSupport(null);
    } catch (err) {
      setError(messageOf(err));
    }
    syncHistory();
  }

  async function undo() {
    const entry = past.current.pop();
    if (!entry) return;
    // Whatever the notice was describing has just been taken back.
    setNotice(null);
    if (entry.kind === "settings") {
      future.current.push({ kind: "settings", overrides: { ...overridesRef.current } });
      setOverrides(entry.overrides);
      markTabDirty();
      syncHistory();
      setNote("setting taken back");
      return;
    }
    if (entry.kind === "partSettings") {
      const now = modelsRef.current.find((model) => model.id === entry.id)?.settings ?? {};
      future.current.push({ kind: "partSettings", id: entry.id, settings: { ...now } });
      syncHistory();
      await applyPartSettings(entry.id, entry.settings);
      return;
    }
    if (entry.kind === "paint") {
      try {
        const view = await api.undoPaint();
        future.current.push({ kind: "paint" });
        setPainted(view.painted);
        setBlocked(view.blocked);
        setPaths([]);
        setLayer(0);
        setLayerCount(0);
        setLayerZ([]);
        forgetSlice();
        setPreview(false);
        setNote("paint taken back");
      } catch (failure) {
        setError(String(failure));
      }
      syncHistory();
      return;
    }
    future.current.push({ kind: "models", models: modelsRef.current, selected: selectedId });
    syncHistory();
    await restoreSnapshot(entry);
  }

  async function redo() {
    const entry = future.current.pop();
    if (!entry) return;
    setNotice(null);
    if (entry.kind === "settings") {
      past.current.push({ kind: "settings", overrides: { ...overridesRef.current } });
      setOverrides(entry.overrides);
      markTabDirty();
      syncHistory();
      setNote("setting put back");
      return;
    }
    if (entry.kind === "partSettings") {
      const now = modelsRef.current.find((model) => model.id === entry.id)?.settings ?? {};
      past.current.push({ kind: "partSettings", id: entry.id, settings: { ...now } });
      syncHistory();
      await applyPartSettings(entry.id, entry.settings);
      return;
    }
    if (entry.kind === "paint") {
      try {
        const view = await api.redoPaint();
        past.current.push({ kind: "paint" });
        setPainted(view.painted);
        setBlocked(view.blocked);
        setPaths([]);
        setLayer(0);
        setLayerCount(0);
        setLayerZ([]);
        forgetSlice();
        setPreview(false);
        setNote("paint put back");
      } catch (failure) {
        setError(String(failure));
      }
      syncHistory();
      return;
    }
    past.current.push({ kind: "models", models: modelsRef.current, selected: selectedId });
    syncHistory();
    await restoreSnapshot(entry);
  }

  // The keyboard effect is installed once, so it reaches the current handlers
  // through a ref instead of capturing the first render's closures.
  const actions = useRef({
    newProject: async () => {},
    importModel: async () => {},
    exportGcode: async () => {},
    duplicateSelected: async () => {},
    arrangeAll: async () => {},
    undo: async () => {},
    redo: async () => {},
  });

  useEffect(() => {
    const onKey = (event: KeyboardEvent) => {
      if (!(event.metaKey || event.ctrlKey)) return;
      const key = event.key.toLowerCase();
      if (key === "s") {
        event.preventDefault();
        saveRef.current(event.shiftKey);
      } else if (key === "o") {
        event.preventDefault();
        void openRef.current();
      } else if (key === "n") {
        event.preventDefault();
        void actions.current.newProject();
      } else if (key === "i") {
        event.preventDefault();
        void actions.current.importModel();
      } else if (key === "e") {
        event.preventDefault();
        void actions.current.exportGcode();
      } else if (key === "d") {
        event.preventDefault();
        void actions.current.duplicateSelected();
      } else if (key === "a" && event.shiftKey) {
        event.preventDefault();
        void actions.current.arrangeAll();
      } else if (key === "z") {
        event.preventDefault();
        if (event.shiftKey) {
          void actions.current.redo();
        } else {
          void actions.current.undo();
        }
      } else if (key === "y") {
        event.preventDefault();
        void actions.current.redo();
      } else if (isPaletteShortcut(event)) {
        event.preventDefault();
        setCommandPaletteOpen((open) => !open);
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, []);

  useEffect(() => {
    function onKey(event: KeyboardEvent) {
      const target = event.target as HTMLElement | null;
      const tag = target?.tagName;
      if (tag === "INPUT" || tag === "SELECT" || tag === "TEXTAREA") return;
      if (target?.closest(".ui-select, .ui-stepper, .ui-dropdown, [role='menu'], .start")) return;
      if (screen !== "slicer") return;
      if (event.key === "f") {
        event.preventDefault();
        setFrameToken((token) => token + 1);
      }
      if (event.key === "Delete" || event.key === "Backspace") {
        // The object list has a bin beside every row; the key that empties a
        // plate should not need the mouse to find it.
        if (selectedId != null) {
          event.preventDefault();
          void removeSelected();
        }
      }
      if (event.key === "Escape") {
        setMeasuring(false);
        setFaceArmed(false);
        clearMeasure();
      }
      if (event.key === "1") setMode("translate");
      if (event.key === "2") setMode("rotate");
      if (event.key === "3") setMode("scale");
      if (event.key === "[") setLayer((current) => Math.max(0, current - 1));
      if (event.key === "]") setLayer((current) => Math.min(Math.max(0, layerCount - 1), current + 1));
    }
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [layerCount, screen, selectedId]);

  useEffect(() => {
    if (!chordTied) return;
    const layer = layerHeight > 0 ? layerHeight : 0.2;
    const nozzleMm = bed.nozzle_size > 0 ? bed.nozzle_size : 0.4;
    const suggested = Math.min(layer * 0.5, nozzleMm * 0.5);
    setChord(Math.round(Math.min(0.5, Math.max(0.02, suggested)) * 1000) / 1000);
  }, [chordTied, layerHeight, bed.nozzle_size]);

  // The whole toolpath is fetched once per slice, not once per slider step:
  // `layer` used to be a dependency here, so every pixel of the scrubber
  // serialised the whole prefix of the job over IPC and rebuilt the buffers.
  useEffect(() => {
    if (!preview || busy) return;
    let cancelled = false;
    void api.previewAll().then((next) => {
      if (!cancelled) setPaths(next);
    }).catch((err: unknown) => {
      if (!cancelled) setError(messageOf(err));
    });
    return () => {
      cancelled = true;
    };
  }, [preview, busy, layerCount]);

  useEffect(() => {
    if (layerCount <= 0) {
      setFeatures([]);
      return;
    }
    let cancelled = false;
    void api
      .featureBreakdown()
      .then((rows) => {
        if (!cancelled) setFeatures(rows);
      })
      .catch(() => {
        if (!cancelled) setFeatures([]);
      });
    return () => {
      cancelled = true;
    };
  }, [layerCount]);

  // A slice restored at boot (or mock data) fills the plate but not the history
  // list until the user presses slice again — keep the sidebar in sync with the
  // job card.
  useEffect(() => {
    if (!preview || layerCount <= 0 || paths.length === 0) return;
    setHistory((records) => {
      if (records.length > 0) return records;
      return [
        {
          n: 1,
          seconds: printSeconds,
          metres: filamentMetres,
          grams: estimate?.grams ?? null,
          layers: layerCount,
          total: estimate?.total ?? null,
          plate: plateKey,
          settings: settingsKey,
          paths,
        },
      ];
    });
    setViewing((current) => (current == null ? 1 : current));
  }, [
    preview,
    layerCount,
    paths,
    printSeconds,
    filamentMetres,
    plateKey,
    settingsKey,
    estimate,
  ]);

  /** Cura ships every material in a 1.75 mm and a 2.85 mm cut, and the list used
   * to offer both: picking the 2.85 mm PETG on a 1.75 mm machine left the sheet
   * saying PETG while the resolver, unable to match a spool at that diameter,
   * sliced the machine's own PLA. Only spools that fit the hotend are offered. */
  const spools = materials.filter(
    (item) => Math.abs(item.diameter - bed.material_diameter) < 0.1,
  );

  const filteredPrinters = useMemo(() => {
    const query = printerQuery.trim().toLowerCase();
    if (!query) return printers;
    return printers.filter((printer) =>
      `${printer.manufacturer} ${printer.name} ${printer.id}`.toLowerCase().includes(query),
    );
  }, [printerQuery, printers]);

  // Geometry travels outside the model list (it belongs in bytes, not JSON), so
  // whatever arrives without it — a fresh import, an opened project, a restored
  // session — is filled in here, once, wherever the model came from.
  useEffect(() => {
    const missing = models.filter((model) => model.positions.length === 0);
    if (missing.length === 0) return;
    let cancelled = false;
    void Promise.all(
      missing.map(async (model) => {
        try {
          return { id: model.id, positions: await api.geometry(model.id) };
        } catch {
          return null;
        }
      }),
    ).then((results) => {
      if (cancelled) return;
      const filled = new Map(
        results.filter((item) => item !== null).map((item) => [item!.id, item!.positions]),
      );
      if (filled.size === 0) return;
      setModels((current) =>
        current.map((model) =>
          filled.has(model.id) ? { ...model, positions: filled.get(model.id)! } : model,
        ),
      );
    });
    return () => {
      cancelled = true;
    };
  }, [models]);

  const selected = models.find((model) => model.id === selectedId) ?? null;

  useEffect(() => {
    if (selectedId == null) {
      setHealth(null);
      return;
    }
    let live = true;
    void api
      .meshHealth(selectedId)
      .then((measured) => {
        if (live) setHealth(measured);
      })
      .catch(() => {
        if (live) setHealth(null);
      });
    return () => {
      live = false;
    };
  }, [selectedId, models]);

  async function importFromPath(picked: string): Promise<boolean> {
    enterSlicer();
    setError(null);
    setImporting(true);
    setImportProgress({ fraction: 0.01, message: "Reading model" });
    try {
      const added = await api.importModel(picked, printerId || null, variant, quality, chord, turn);
      remember();
      setModels(added);
      try {
        setPlates(await api.listPlates());
        setEmbedded(await api.importedSettings());
        setIgnoreEmbedded(false);
      } catch {
        // The models are on the plate even if the plate list did not refresh.
      }
      const last = added[added.length - 1];
      setSelectedId(last.id);
      setPreview(false);
      setShowModel(true);
      const triangles = added.reduce((total, model) => total + model.triangles, 0);
      setImportProgress({
        fraction: 1,
        message: added.length > 1 ? `${added.length} parts, ${triangles} triangles` : `${triangles} triangles`,
      });
      setFrameToken((token) => token + 1);
      return true;
    } catch (err) {
      const message = messageOf(err);
      // Only a deliberate cancel stays quiet; any other failure is shown.
      const cancelled = message.trim().toLowerCase() === "import cancelled";
      if (!cancelled) setError(message);
      // The slicer is entered before the read starts so progress has somewhere
      // to appear. On failure with nothing else loaded, go back rather than
      // leaving an empty slicer on screen.
      if (models.length === 0) {
        setScreen("start");
        setPane("home");
      }
      return false;
    } finally {
      setImportProgress(null);
      setImporting(false);
    }
  }

  async function importModel() {
    const picked = await open({
      multiple: false,
      filters: [{ name: "Models", extensions: ["stl", "STL", "3mf", "3MF", "step", "stp", "STEP", "STP"] }],
    });
    if (typeof picked !== "string") return;
    await importFromPath(picked);
  }

  useEffect(() => {
    if (models.length === 0) {
      setFit(null);
      return;
    }
    let live = true;
    api
      .plateFit(printerId || null)
      .then((next) => {
        if (live) setFit(next);
      })
      .catch(() => {
        if (live) setFit(null);
      });
    return () => {
      live = false;
    };
  }, [models, printerId, plates]);

  const importPathRef = useRef(importFromPath);
  importPathRef.current = importFromPath;
  useEffect(() => {
    if (import.meta.env.VITE_SHALE_MOCK === "1") return;
    const win = getCurrentWindow() as {
      onDragDropEvent?: (
        cb: (event: { payload: { type: string; paths?: string[] } }) => void,
      ) => Promise<() => void>;
    };
    if (typeof win.onDragDropEvent !== "function") return;
    let stop: (() => void) | undefined;
    let alive = true;
    void win
      .onDragDropEvent((event) => {
        if (event.payload.type !== "drop") return;
        const path = event.payload.paths?.[0];
        if (path && /\.(stl|3mf|step|stp)$/i.test(path)) void importPathRef.current(path);
      })
      .then((off) => {
        if (!alive) off();
        else stop = off;
      });
    return () => {
      alive = false;
      stop?.();
    };
  }, []);

  async function cancelImport() {
    try {
      await api.cancelImport();
    } catch (err) {
      setError(messageOf(err));
    }
  }

  async function commitTransform(id: number, transform: Transform) {
    remember(`transform:${id}`);
    setModels((current) => current.map((model) => (model.id === id ? { ...model, transform } : model)));
    setSupport(null);
    try {
      await api.updateTransform(id, transform);
    } catch (err) {
      setError(messageOf(err));
    }
  }

  /** Rotating or scaling happens about the mesh's own origin, not the part's
   * centre, so the part slides across the plate whenever you turn it. Measure
   * the centre before and after, and put it back where it was. */
  function commitAroundCentre(model: ModelView, patch: Partial<Transform>) {
    const before = worldCentre(model.positions, model.transform);
    const next: Transform = { ...model.transform, ...patch };
    const after = worldCentre(model.positions, next);
    const translation = next.translation.map((value, index) => value + (before[index] - after[index])) as [
      number,
      number,
      number,
    ];
    void commitTransform(model.id, { ...next, translation });
  }

  function editNumber(axis: 0 | 1 | 2, kind: "rotation_deg" | "scale", value: number) {
    if (!selected) return;
    const transform: Transform = {
      translation: [...selected.transform.translation],
      rotation_deg: [...selected.transform.rotation_deg],
      scale: [...selected.transform.scale],
    };
    transform[kind][axis] = value;
    commitAroundCentre(selected, transform);
  }

  function editCentre(axis: 0 | 1 | 2, value: number) {
    if (!selected) return;
    const centre = worldCentre(selected.positions, selected.transform);
    centre[axis] = value;
    const translation = translationForCentre(selected.positions, selected.transform, centre);
    void commitTransform(selected.id, {
      ...selected.transform,
      translation,
    });
  }

  /** Mirror a part across one axis. A negative scale is a mirror, and the core
   * flips the winding when it bakes so the solid stays right way out. */
  function mirrorAxis(axis: 0 | 1 | 2) {
    if (!selected) return;
    const scale = [...selected.transform.scale] as [number, number, number];
    scale[axis] = -scale[axis];
    commitAroundCentre(selected, { scale });
  }

  /** Turn a part by a quarter, which is how most parts actually get placed. */
  function turnQuarter(axis: 0 | 1 | 2, quarters: number) {
    if (!selected) return;
    const rotation_deg = [...selected.transform.rotation_deg] as [number, number, number];
    rotation_deg[axis] = Math.round((rotation_deg[axis] + 90 * quarters) * 1000) / 1000;
    commitAroundCentre(selected, { rotation_deg });
  }

  async function resetTransform() {
    if (!selected) return;
    try {
      const transform = await api.dropModel(selected.id);
      const upright: Transform = {
        translation: transform.translation,
        rotation_deg: [0, 0, 0],
        scale: [1, 1, 1],
      };
      await api.updateTransform(selected.id, upright);
      remember();
      setModels((current) =>
        current.map((model) => (model.id === selected.id ? { ...model, transform: upright } : model)),
      );
    } catch (err) {
      setError(messageOf(err));
    }
  }

  /** Stand the part on whichever face needs the least support. */
  async function autoOrient() {
    if (!selected) return;
    try {
      const transform = await api.autoOrient(selected.id);
      remember();
      setModels((current) => current.map((model) => (model.id === selected.id ? { ...model, transform } : model)));
      toastUndo("Orientation changed", () => void undo(), { id: "orient" });
    } catch (err) {
      setError(messageOf(err));
    }
  }

  /** Same choice, for every part on the plate. Undo puts them all back. */
  async function autoOrientAll() {
    if (modelsRef.current.length === 0) return;
    try {
      const views = await api.autoOrientAll();
      remember();
      setModels(views);
      setPaths([]);
      setLayerCount(0);
      forgetSlice();
      toastUndo("Orientations changed", () => void undo(), { id: "orient-all" });
    } catch (err) {
      setError(messageOf(err));
    }
  }

  /// Put the face under the click on the plate.
  async function placeOnFace(id: number, normal: [number, number, number]) {
    if (!selected) return;
    try {
      const transform = await api.placeOnFace(id, normal);
      remember();
      setModels((current) => current.map((model) => (model.id === id ? { ...model, transform } : model)));
      setFaceArmed(false);
      setNotice("resting on the face you clicked");
    } catch (err) {
      setError(messageOf(err));
    }
  }

  async function dropSelected() {
    if (!selected) return;
    try {
      const transform = await api.dropModel(selected.id);
      remember();
      setModels((current) => current.map((model) => (model.id === selected.id ? { ...model, transform } : model)));
    } catch (err) {
      setError(messageOf(err));
    }
  }

  /** Lay every model out on the plate, biggest first. */
  async function arrangeAll() {
    if (models.length < 2) return;
    setError(null);
    try {
      const placed = await api.arrange(printerId, variant, quality);
      remember();
      setModels((current) =>
        current.map((model) => {
          const moved = placed.find((entry) => entry.id === model.id);
          return moved ? { ...model, transform: moved.transform } : model;
        }),
      );
    } catch (err) {
      setError(messageOf(err));
    }
  }

  /** Drop a point on a part, snapped onto the nearest vertex when there is one. */
  async function addMeasurePoint(id: number, point: [number, number, number]) {
    try {
      const snapped = (await api.snapMeasure(id, point)) ?? point;
      setMeasureMarks((current) => (current.length === 1 ? [current[0], snapped] : [snapped]));
    } catch (err) {
      setError(messageOf(err));
    }
  }

  function clearMeasure() {
    setMeasureMarks([]);
    setMeasureSpan(null);
  }

  useEffect(() => {
    if (measureMarks.length !== 2) {
      setMeasureSpan(null);
      return;
    }
    let cancelled = false;
    void api
      .measureSpan(measureMarks[0], measureMarks[1])
      .then((measured) => {
        if (!cancelled) setMeasureSpan(measured);
      })
      .catch(() => {});
    return () => {
      cancelled = true;
    };
  }, [measureMarks]);

  /** Split the selected part with a plane, the way Bambu's Cut does. */
  async function cutSelected() {
    if (!selected) return;
    try {
      const { views: halves, joint } = await api.cut(
        selected.id,
        cutAxis,
        Number(cutOffset) || 0,
        cutKeep,
        cutPins,
        Number(pinRadius) || 2,
        Number(pinHeight) || 5,
        cutDovetail,
        cutFaceDown,
      );
      remember();
      setModels((current) => [...current.filter((model) => model.id !== selected.id), ...halves]);
      setSelectedId(halves[0]?.id ?? null);
      setPreview(false);
      // Say what the cut did, not just that it happened: a dovetail that could
      // not be fitted leaves a plain cut, and the control that asked for it has
      // to admit that rather than stay silent.
      const shapes = `${halves.length} ${halves.length === 1 ? "part" : "parts"}`;
      if (joint) {
        setNotice(`cut: ${shapes}, with ${joint}`);
      } else if (cutDovetail > 0) {
        setNotice(`cut: ${shapes} — no room for a ${cutDovetail} mm tab on that face, so it was cut plain`);
      } else {
        setNotice(`cut: ${shapes}`);
      }
    } catch (err) {
      setError(messageOf(err));
    }
  }

  /// Bambu's mesh boolean: the chosen part against every other part on the
  /// plate. Subtract is the one that earns its place in a slicer — it is how a
  /// pocket, a window, or a clearance hole gets made without a CAD program.
  async function booleanSelected(mode: "subtract" | "union" | "common") {
    if (!selected) return;
    if (models.length < 2) {
      setNotice("a boolean needs a second part on the plate");
      return;
    }
    try {
      remember();
      const result = await api.boolean(selected.id, mode);
      setModels(result.views);
      setSelectedId(result.views[0]?.id ?? null);
      setPreview(false);
      setPaths([]);
      setLayerCount(0);
      forgetSlice();
      // Only the parts that overlapped took part; saying "against 2 parts" when
      // one of them was nowhere near would be a number the app did not measure.
      setNotice(
        `boolean: ${mode}, ${result.consumed} ${result.consumed === 1 ? "part" : "parts"} folded in`,
      );
    } catch (err) {
      setError(messageOf(err));
    }
  }

  async function duplicateSelected() {
    if (!selected) return;
    try {
      const copy = await api.duplicateModel(selected.id);
      remember();
      setModels((current) => [...current, copy]);
      setSelectedId(copy.id);
    } catch (err) {
      setError(messageOf(err));
    }
  }

  /// As many copies of the selected part as the plate holds, laid out the way
  /// Bambu's "Fill plate" does — a grid with a small gap, skipping anything
  /// already standing there.
  async function fillPlate() {
    if (!selected) return;
    try {
      remember();
      const copies = await api.fillPlate(selected.id, {
        width: bed.width,
        depth: bed.depth,
        centerIsZero: bed.center_is_zero,
      });
      setModels((current) => [...current, ...copies]);
      setSelectedId(copies[0]?.id ?? selected.id);
      setPaths([]);
      setLayerCount(0);
      forgetSlice();
      setNotice(`${copies.length + 1} copies on the plate`);
    } catch (err) {
      setError(messageOf(err));
    }
  }

  /// A primitive, the way Bambu's right-click menu adds one: 20 mm across,
  /// standing on the bed, on the first free patch.
  async function addPart(kind: "cube" | "cylinder" | "cone" | "sphere") {
    try {
      remember();
      const wanted = await api.addPrimitive(kind, 20, 20, {
        width: bed.width,
        depth: bed.depth,
        centerIsZero: bed.center_is_zero,
      });
      setModels((current) => [...current, wanted]);
      setSelectedId(wanted.id);
      setPaths([]);
      setLayerCount(0);
      forgetSlice();
    } catch (err) {
      setError(messageOf(err));
    }
  }

  async function splitSelected() {
    if (!selected) return;
    try {
      remember();
      const pieces = await api.splitModel(selected.id);
      setModels((current) => [...current.filter((model) => model.id !== selected.id), ...pieces]);
      setSelectedId(pieces[0].id);
      setNotice(`Split into ${pieces.length} parts, each with its own place on the plate`);
    } catch (err) {
      setError(messageOf(err));
    }
  }

  async function removeSelected() {
    if (!selected) return;
    try {
      remember();
      await api.removeModel(selected.id);
      setModels((current) => current.filter((model) => model.id !== selected.id));
      setSelectedId(null);
      toastUndo("Part removed", () => void undo(), { id: "remove-part" });
    } catch (err) {
      setError(messageOf(err));
    }
  }

  /// Delete by id, not by selection: a list of objects must be able to drop a
  /// row you have not clicked, which is most of the point of having the list.
  async function removeModel(id: number) {
    try {
      remember();
      await api.removeModel(id);
      setModels((current) => current.filter((model) => model.id !== id));
      if (selectedId === id) setSelectedId(null);
      toastUndo("Part removed", () => void undo(), { id: "remove-part" });
    } catch (err) {
      setError(messageOf(err));
    }
  }

  async function assignSlot(slot: number | null) {
    if (!selectedId) return;
    try {
      const updated = await api.setModelSlot(selectedId, slot);
      // Spool choice is a plate setting, like the settings sheet: it is not on
      // the undo stack, because undo restores transforms from the backend and
      // the backend is the one holding the slot.
      setModels((list) => list.map((item) => (item.id === updated.id ? updated : item)));
      setNotice(slot == null || slot === 0 ? "Part prints from the plate material" : `Part prints from spool ${slot + 1}`);
    } catch (err) {
      setError(messageOf(err));
    }
  }

  function reportProgress(fraction: number, message: string) {
    const job = batch.current;
    if (!job || job.total <= 0) {
      setProgress({ fraction, message });
      return;
    }
    setProgress({
      fraction: batchFraction(job.done, job.total, fraction),
      message: batchMessage(job.done, job.total, job.name, message),
    });
  }

  async function slice(options?: { quiet?: boolean; hold?: boolean }) {
    const quiet = options?.quiet === true;
    const hold = options?.hold === true;
    if (!printerId) {
      if (!quiet) setError("Choose a printer first.");
      if (hold) throw new Error("Choose a printer first.");
      return;
    }
    setError(null);
    if (!quiet) setBusy(true);
    setCalibration(null);
    reportProgress(0.02, "Starting");
    try {
      const summary = await api.slice(
        printerId,
        variant,
        quality,
        material,
        ams,
        engineSettings,
        ignoreEmbedded,
      );
      setLayerCount(summary.layer_count);
      setPlates((current) => {
        const next = current.map((item) => (item.active ? { ...item, sliced: true } : item));
        platesRef.current = next;
        return next;
      });
      slicedPlate.current = plateKey;
      setLayerZ(summary.layers.map((item) => item.z));
      setLayer(Math.max(0, summary.layer_count - 1));
      setPrintSeconds(summary.print_time_seconds);
      setFilamentMetres(summary.filament_metres);
      // The record keeps its own price: the cost panel can be edited later, and
      // "slice 2 cost 0.11" must not change when the tariff does.
      const density = materialInfo?.density;
      let grams: number | null = null;
      let total: number | null = null;
      if (summary.filament_metres != null && density) {
        try {
          const priced = await api.costEstimate(
            summary.filament_metres,
            summary.print_time_seconds,
            materialInfo?.diameter ?? 1.75,
            density,
          );
          grams = priced.grams;
          total = priced.total;
        } catch {
          // A missing price is not a reason to lose a slice.
        }
      }
      const all = await api.previewAll();
      setPaths(all);
      setNonplanarReport(summary.nonplanar ?? null);
      const record: SliceRecord = {
        n: history.length + 1,
        seconds: summary.print_time_seconds,
        metres: summary.filament_metres,
        grams,
        layers: summary.layer_count,
        total,
        plate: plateKey,
        settings: settingsKey,
        paths: all,
      };
      setHistory((records) => [...records, record]);
      setViewing(record.n);
      setPreview(true);
      // A finished slice is a result to look at: the app turns to Preview by
      // itself. A background refresh keeps the person on the pane they are editing.
      if (!quiet) {
        setPane("preview");
        setShowModel(false);
        setFrameToken((token) => token + 1);
      }
      reportProgress(1, `${summary.layer_count} layers`);
    } catch (err) {
      if (hold) throw err;
      if (!quiet) setError(messageOf(err));
      setProgress(null);
    } finally {
      if (!quiet && !hold) setBusy(false);
    }
  }

  /// Slice every plate that has a part on it, one after another. `onlyUnsliced`
  /// is what "send every plate" uses: a plate that already has G-code is left.
  async function sliceAll(options?: { onlyUnsliced?: boolean }): Promise<boolean> {
    if (!printerId) {
      setError("Choose a printer first.");
      return false;
    }
    const onlyUnsliced = options?.onlyUnsliced === true;
    const targets = platesToSlice(platesRef.current, onlyUnsliced);
    if (targets.length === 0) {
      if (!onlyUnsliced) setError("Nothing on any plate to slice.");
      return true;
    }
    const start = platesRef.current.find((plate) => plate.active)?.index ?? targets[0].index;
    setBusy(true);
    setError(null);
    batch.current = { done: 0, total: targets.length, name: targets[0].name };
    try {
      for (let index = 0; index < targets.length; index += 1) {
        const plate = targets[index];
        batch.current = { done: index, total: targets.length, name: plate.name };
        reportProgress(0.02, "Starting");
        const active = platesRef.current.find((item) => item.active)?.index;
        if (active !== plate.index) await showPlate(plate.index);
        await slice({ quiet: true, hold: true });
      }
      const back = platesRef.current.find((item) => item.active)?.index;
      if (back !== start) await showPlate(start);
      setNotice(targets.length === 1 ? `Sliced ${targets[0].name}` : `Sliced ${targets.length} plates`);
      return true;
    } catch (err) {
      setError(messageOf(err));
      setProgress(null);
      return false;
    } finally {
      batch.current = null;
      setBusy(false);
    }
  }

  sliceQuiet.current = () => {
    void slice({ quiet: true });
  };

  function plateOverflows() {
    return fit != null && Math.max(fit.x, fit.y, fit.z) > 0.5;
  }

  async function exportGcode() {
    if (plateOverflows()) {
      setConfirmExport("gcode");
      return;
    }
    await writeGcode();
  }

  async function writeGcode() {
    setError(null);
    const destination = await save({
      defaultPath: "shale.gcode",
      filters: [{ name: "G-code", extensions: ["gcode"] }],
    });
    if (typeof destination !== "string") return;
    try {
      await api.exportGcode(destination);
    } catch (err) {
      setError(messageOf(err));
    }
  }

  const scopedPart =
    scope === "all" ? null : models.find((model) => model.id === Number(scope)) ?? null;

  /** One part's own numbers, snapshotted before they change. `restore_models`
   * only carries transforms, so a per-part override needs its own kind of step. */
  function rememberPartSettings(part: { id: number; settings: Record<string, string> }) {
    past.current.push({ kind: "partSettings", id: part.id, settings: { ...part.settings } });
    if (past.current.length > 80) past.current.shift();
    future.current = [];
    markTabDirty();
    syncHistory();
  }

  function queuePartSettings(id: number, settings: Record<string, string>) {
    partSettingsPending.current = { id, settings };
    if (partSettingsTimer.current != null) window.clearTimeout(partSettingsTimer.current);
    partSettingsTimer.current = window.setTimeout(() => {
      partSettingsTimer.current = null;
      const pending = partSettingsPending.current;
      partSettingsPending.current = null;
      if (!pending) return;
      void api
        .setModelSettings(pending.id, pending.settings)
        .then((updated) => {
          setModels((current) => current.map((model) => (model.id === updated.id ? updated : model)));
        })
        .catch((err: unknown) => setError(messageOf(err)));
    }, 160);
  }

  function writeObjectOverride(part: ModelView, key: string, value: string) {
    if ((part.settings[key] ?? "") !== value) rememberPartSettings(part);
    const next = { ...part.settings };
    if (value === "") delete next[key];
    else next[key] = value;
    setModels((current) => current.map((model) => (model.id === part.id ? { ...model, settings: next } : model)));
    queuePartSettings(part.id, next);
    setPreview(false);
    setPaths([]);
    setLayerCount(0);
    forgetSlice();
  }

  async function applyPartSettings(id: number, settings: Record<string, string>) {
    try {
      const updated = await api.setModelSettings(id, settings);
      setModels((current) => current.map((model) => (model.id === updated.id ? updated : model)));
      forgetSlice();
      setNote("that part's setting changed");
    } catch (err) {
      setError(messageOf(err));
    }
  }

  async function setScopedOverride(key: string, value: string) {
    if (!scopedPart) {
      if ((overridesRef.current[key] ?? "") !== value) rememberSettings();
      setOverrides((prev) => {
        const next = { ...prev };
        if (value === "") delete next[key];
        else next[key] = value;
        return next;
      });
      return;
    }
    if ((scopedPart.settings[key] ?? "") !== value) rememberPartSettings(scopedPart);
    const next = { ...scopedPart.settings };
    if (value === "") delete next[key];
    else next[key] = value;
    setModels((current) =>
      current.map((model) => (model.id === scopedPart.id ? { ...model, settings: next } : model)),
    );
    // Sliders report every pointer move. Keep the plate state immediate and
    // send the part settings once the drag settles, still as set_model_settings.
    queuePartSettings(scopedPart.id, next);
  }

  async function resetScopedOverrides() {
    if (!scopedPart) {
      if (Object.keys(overridesRef.current).length > 0) rememberSettings();
      setOverrides({});
      return;
    }
    setModels((current) =>
      current.map((model) => (model.id === scopedPart.id ? { ...model, settings: {} } : model)),
    );
    try {
      const updated = await api.setModelSettings(scopedPart.id, {});
      setModels((current) => current.map((model) => (model.id === updated.id ? updated : model)));
    } catch (err) {
      setError(messageOf(err));
    }
  }

  type ProfileSnap = { printerId: string; variant: string | null; quality: string | null; material: string | null };
  const snapProfile = (): ProfileSnap => ({ printerId, variant, quality, material });
  const restoreProfile = (snap: ProfileSnap) => {
    setPrinterId(snap.printerId);
    setVariant(snap.variant);
    setQuality(snap.quality);
    setMaterial(snap.material);
  };
  const setPrinterWithUndo = (next: string) => {
    if (next === printerId) return;
    const before = snapProfile();
    setPrinterId(next);
    toastUndo("Printer profile changed", () => restoreProfile(before), { id: "profile-undo" });
  };
  const setQualityWithUndo = (next: string | null) => {
    if (next === quality) return;
    const before = snapProfile();
    setQuality(next);
    void resetScopedOverrides();
    toastUndo("Quality preset changed", () => restoreProfile(before), { id: "profile-undo" });
  };
  const setMaterialWithUndo = (next: string | null) => {
    if (next === material) return;
    const before = snapProfile();
    setMaterial(next);
    toastUndo("Material changed", () => restoreProfile(before), { id: "profile-undo" });
  };

  // A part that gets deleted while its settings are on screen hands the sheet
  // back to the whole plate.
  useEffect(() => {
    if (scope !== "all" && !models.some((model) => model.id === Number(scope))) {
      setScope("all");
    }
  }, [models, scope]);

  async function exportGeometry(format: "stl" | "3mf") {
    if (plateOverflows()) {
      setConfirmExport(format);
      return;
    }
    await writeGeometry(format);
  }

  async function writeGeometry(format: "stl" | "3mf") {
    setError(null);
    setNotice(null);
    const destination = await save({
      defaultPath: `shale-plate.${format}`,
      filters: [
        format === "stl"
          ? { name: "STL mesh", extensions: ["stl"] }
          : { name: "3MF model", extensions: ["3mf"] },
      ],
    });
    if (typeof destination !== "string") return;
    try {
      const triangles = await api.exportPlate(destination, format);
      setNotice(`Wrote ${triangles.toLocaleString("en-US")} triangles to ${destination}`);
    } catch (err) {
      setError(messageOf(err));
    }
  }

  async function addInstruction() {
    setError(null);
    const text = note.trim();
    let instruction: CustomInstruction;
    if (kind === "pause") {
      instruction = {
        kind: "pause",
        layer: anchor === "layer" ? at : null,
        height_mm: anchor === "height" ? at : null,
        filament_change: filament,
        message: text || null,
      };
    } else if (kind === "temperature") {
      instruction = {
        kind: "temperature",
        layer: at,
        nozzle_c: tempNozzle,
        bed_c: tempBed,
        wait: tempWait,
      };
    } else if (kind === "fan") {
      instruction = { kind: "fan", height_mm: at, percent: fanPercent };
    } else if (kind === "beep") {
      instruction = { kind: "beep", layer: at };
    } else if (kind === "message") {
      if (!text) {
        setError("A message needs some text.");
        return;
      }
      instruction = { kind: "message", layer: at, text };
    } else {
      instruction = {
        kind: "park_and_wait",
        layer: at,
        x: 5,
        y: 5,
        message: text || null,
      };
    }
    try {
      const stored = await api.addInstruction(instruction);
      setInstructions((current) => [...current, stored]);
      setNote("");
    } catch (err) {
      setError(messageOf(err));
    }
  }

  // Bambu adds a pause by right-clicking a layer in the preview. The same
  // instruction machinery already existed here, but only behind the sidebar,
  // where nobody looks while they are scrubbing through the print.
  async function addMarkAt(at: number, filamentChange: boolean) {
    setError(null);
    try {
      const stored = await api.addInstruction({
        kind: "pause",
        layer: at,
        height_mm: null,
        filament_change: filamentChange,
        message: null,
      });
      setInstructions((current) => [...current, stored]);
    } catch (err) {
      setError(messageOf(err));
    }
  }

  async function removeInstruction(id: number) {
    try {
      setInstructions(await api.removeInstruction(id));
    } catch (err) {
      setError(messageOf(err));
    }
  }

  /** Put a temperature tower on the plate. Shale builds the geometry itself,
   * so the engine only ever sees an ordinary STL. */
  /// The patches of underside the part on the plate offers. A patch is what a
  /// brush stroke would cover, so listing them is the nearest thing to
  /// painting without a pointer on the mesh.
  async function loadPatches(id: number | null) {
    if (id === null) {
      setPatches([]);
      setPainted(0);
      return;
    }
    try {
      setPatches((await api.overhangPatches(id)) ?? []);
      setPainted(0);
    } catch (failure) {
      setPatches([]);
      setError(String(failure));
    }
  }

  /** Paint one patch. Anything painted adds a column under that part on the
   * next slice, so the toolpath on screen is no longer what the plate would
   * print. */
  /// What a paint edit costs the plate on screen, whichever way it was made.
  function paintLanded(state: { painted: number; blocked: number }, block: boolean) {
    setPainted(state.painted);
    setBlocked(state.blocked);
    // The plate would print differently now, so the toolpath on screen is
    // stale: drop it rather than show support that is not there.
    setPaths([]);
    setLayer(0);
    setLayerCount(0);
    setLayerZ([]);
    forgetSlice();
    setPreview(false);
    setNote(
      block
        ? `${state.blocked} faces kept clear: no support will grow under them`
        : `${state.painted} faces held up: a column will grow under them`,
    );
  }

  async function paintPatch(triangles: number[], target: number | null = paintTarget) {
    if (target === null) {
      return;
    }
    const block = paintMode === "block";
    try {
      past.current.push({ kind: "paint" });
      future.current = [];
      syncHistory();
      paintLanded(await api.paintSupport(target, triangles, block), block);
    } catch (failure) {
      setError(String(failure));
    }
  }

  /// One drag of the brush. The whole path goes over the boundary together, so a
  /// stroke is one edit on the undo stack and not thirty: taking a pen stroke
  /// back should Take back the stroke.
  async function paintStroke(target: number, points: Array<[number, number, number]>) {
    const block = paintMode === "block";
    try {
      past.current.push({ kind: "paint" });
      future.current = [];
      syncHistory();
      const state = await api.paintBrush(
        target,
        points,
        Math.max(0.5, Number(paintRadius) || 6),
        block,
      );
      paintLanded(state, block);
    } catch (failure) {
      setError(String(failure));
    }
  }

  /** Take one stroke back, from the panel rather than the keyboard: a mouse
   * user has no Ctrl habit, and paint is the one edit with its own stack. */
  async function takeBackPaint() {
    try {
      const view = await api.undoPaint();
      setPainted(view.painted);
      setPaths([]);
      setLayerCount(0);
      forgetSlice();
      setPreview(false);
      setNote("paint taken back");
    } catch (failure) {
      setError(String(failure));
    }
  }

  async function unpaint() {
    if (paintTarget === null) {
      return;
    }
    try {
      past.current.push({ kind: "paint" });
      future.current = [];
      syncHistory();
      const state = await api.clearPaint(paintTarget);
      setPainted(state.painted);
      setPaths([]);
      setLayer(0);
      setLayerCount(0);
      setLayerZ([]);
      forgetSlice();
      setPreview(false);
      setNote("paint cleared");
    } catch (failure) {
      setError(String(failure));
    }
  }

  /// Turn to a plate. The backend swaps that plate's models in and parks the
  /// one in front, so anything on screen that belonged to the old plate —
  /// selection, preview, the calibration readout — has to go.
  async function showPlate(index: number) {
    try {
      const turned: PlateSwitch = await api.switchPlate(index);
      setModels(turned.models);
      platesRef.current = turned.plates;
      setPlates(turned.plates);
      setSupportBoxes(await api.supportBoxes().catch(() => []));
      setSelectedId(null);
      setScope("all");
      setPreview(false);
      // The model is drawn only while `showModel` is on, and the slice that was
      // on screen belonged to the plate we just left: its layer count and its
      // toolpaths have to go with it. A plate that was sliced before keeps its
      // file, so the numbers come back when that plate is in front again.
      setShowModel(turned.models.length > 0);
      setPaths([]);
      setLayer(0);
      setLayerCount(0);
      setLayerZ([]);
      forgetSlice();
      try {
        const existing = await api.existingSlice();
        if (existing && existing.layer_count > 0) {
          setLayerCount(existing.layer_count);
          setLayerZ(existing.layers.map((item) => item.z));
          setPrintSeconds(existing.print_time_seconds);
          setFilamentMetres(existing.filament_metres);
        }
      } catch {
        // A plate with no slice is the ordinary case.
      }
      setTower(null);
      setCalibration(null);
      setMeasureSpan(null);
      setMeasureMarks([]);
    } catch (failure) {
      setError(String(failure));
    }
  }

  // The part being painted is the selected one, else the first part on the
  // plate. A plate switch or a part that went away has to reload the list.
  useEffect(() => {
    const wanted =
      selectedId !== null && models.some((model) => model.id === selectedId)
        ? selectedId
        : models.length > 0
          ? models[0].id
          : null;
    if (wanted !== paintTarget) {
      setPaintTarget(wanted);
    }
  }, [models, selectedId, paintTarget]);

  useEffect(() => {
    void loadPatches(paintTarget);
  }, [paintTarget]);

  async function newPlate() {
    try {
      const added: PlateSwitch = await api.addPlate();
      setModels([]);
      setPlates(added.plates);
      setSelectedId(null);
      setScope("all");
      setPreview(false);
      setShowModel(false);
      setPaths([]);
      setLayer(0);
      setLayerCount(0);
      setLayerZ([]);
      forgetSlice();
      setTower(null);
      setCalibration(null);
      setMeasureSpan(null);
      setMeasureMarks([]);
    } catch (failure) {
      setError(String(failure));
    }
  }

  async function dropPlate(index: number) {
    plateUndoRef.current = { plates: [...plates], models: [...modelsRef.current], selected: selectedId };
    try {
      const left = await api.deletePlate(index);
      const turned = left.find((plate) => plate.active);
      if (turned) {
        const swapped = await api.switchPlate(turned.index);
        setModels(swapped.models);
        setPlates(swapped.plates);
        setSupportBoxes(await api.supportBoxes().catch(() => []));
        setShowModel(swapped.models.length > 0);
      } else {
        setPlates(left);
      }
      setSelectedId(null);
      setPreview(false);
      setPaths([]);
      setLayer(0);
      setLayerCount(0);
      setLayerZ([]);
      forgetSlice();
      setTower(null);
      setCalibration(null);
      const snap = plateUndoRef.current;
      if (snap) {
        toastUndo("Plate deleted", () => {
          setPlates(snap.plates);
          void restoreSnapshot({ models: snap.models, selected: snap.selected });
        }, { id: "drop-plate" });
      }
    } catch (failure) {
      setError(String(failure));
    }
  }

  async function clearWorkplate() {
    if (models.length === 0) return;
    remember();
    await emptyPlate();
    toastUndo("Plate cleared", () => void undo(), { id: "clear-plate" });
  }

  /** Put one of the three calibration prints on the plate. */
  async function addCalibration(kind: "temperature" | "flow" | "pressure") {
    setError(null);
    setCalibrating(true);
    try {
      const built =
        kind === "flow"
          ? await api.flowCoupon(printerId || null)
          : kind === "pressure"
            ? await api.pressureTower(printerId || null)
            : await api.calibrationTower(printerId || null);
      remember();
      setModels((current) => [...current, built.model]);
      setSelectedId(built.model.id);
      setPreview(false);
      setShowModel(true);
      setTower(built);
      setCalibration(null);
    } catch (err) {
      setError(messageOf(err));
    } finally {
      setCalibrating(false);
    }
  }

  /** Rewrite the slice already on screen so each band is printed at its own
   * value — temperature, flow or pressure advance. Nothing else moves. */
  async function stepCalibration() {
    if (!tower) return;
    setError(null);
    setCalibrating(true);
    try {
      setCalibration(await api.calibrateSlice(tower.bands, tower.kind));
    } catch (err) {
      setError(messageOf(err));
    } finally {
      setCalibrating(false);
    }
  }

  async function proposeSupports() {
    if (selectedId == null) {
      setError("Select a model before proposing supports.");
      return;
    }
    setError(null);
    setSupportBusy(true);
    try {
      setSupport(await api.proposeSupports(selectedId, supportAngle, bridgeMm));
    } catch (err) {
      setError(messageOf(err));
    } finally {
      setSupportBusy(false);
    }
  }

  async function toggleSupport(id: number) {
    if (!support) return;
    const ids = support.plan.regions
      .filter((region) => (region.id === id ? !region.enabled : region.enabled))
      .map((region) => region.id);
    try {
      setSupport(await api.applySupports(ids));
    } catch (err) {
      setError(messageOf(err));
    }
  }

  async function compareSupports() {
    if (!printerId || !support) return;
    setError(null);
    setSupportBusy(true);
    try {
      const comparison = await api.compareSupports(printerId, variant, quality, material, {
        ...engineSettings,
        support_enable: true,
      });
      setSupport((current) =>
        current ? { ...current, plan: { ...current.plan, comparison } } : current,
      );
    } catch (err) {
      setError(messageOf(err));
    } finally {
      setSupportBusy(false);
    }
  }

  async function applyEngine() {
    try {
      const ok = await api.setEngine(enginePath, libDir, backend);
      setStatus((current) =>
        current
          ? { ...current, engine_ok: ok, engine_path: enginePath, engine_backend: backend }
          : current,
      );
      if (!ok) {
        const fresh = await api.status();
        setError(fresh.engine_message || "CuraEngine did not respond to `help` at that path.");
      }
    } catch (err) {
      setError(messageOf(err));
    }
  }

  function enterSlicer() {
    setWorkspace(true);
    setScreen("slicer");
    // Opening or importing a part is a Prepare act: landing on whatever pane
    // was last open made the import look like it had failed.
    setPane("prepare");
  }

  // A play that ends holds the last layer instead of snapping back: the point
  // of watching is the top of the print.
  useEffect(() => {
    if (!playing) return;
    if (layer >= layerCount - 1) {
      setPlaying(false);
      return;
    }
    const tick = window.setTimeout(() => setLayer((current) => Math.min(layerCount - 1, current + 1)), 90);
    return () => window.clearTimeout(tick);
  }, [playing, layer, layerCount]);

  function currentSettings() {
    return { ...engineSettings };
  }

  function printerLabel() {
    const printer = printers.find((item) => item.id === printerId);
    if (!printer) return printerId || "No printer";
    return `${printer.manufacturer ? `${printer.manufacturer} ` : ""}${printer.name}`;
  }

  function materialLabel(id: string | null): string {
    if (!id) return "";
    return materials.find((item) => item.id === id)?.label ?? id;
  }

  function materialColour(id: string | null): string {
    if (!id) return "";
    return materials.find((item) => item.id === id)?.color ?? "";
  }

  function dismissSuggestion(item: Suggestion) {
    setDismissedSuggestions((current) => (current.includes(item.id) ? current : [...current, item.id]));
    setSuggestions((current) => current.filter((entry) => entry.id !== item.id));
    markTabDirty();
  }

  function neverSuggestion(item: Suggestion) {
    const raw = window.localStorage.getItem("shale.suggestions.never");
    const list = raw ? (JSON.parse(raw) as string[]) : [];
    if (!list.includes(item.rule_id)) {
      list.push(item.rule_id);
      window.localStorage.setItem("shale.suggestions.never", JSON.stringify(list));
    }
    dismissSuggestion(item);
  }

  async function applySuggestion(item: Suggestion) {
    const fix = item.fix;
    if (!fix) return;
    markTabDirty();
    if (Object.keys(fix.settings).length > 0) {
      rememberSettings();
      setOverrides((current) => ({ ...current, ...fix.settings }));
    }
    if (fix.nonplanar_mode === "off" || fix.nonplanar_mode === "auto" || fix.nonplanar_mode === "curved-tops") {
      setNonplanar((current) => ({ ...current, mode: fix.nonplanar_mode as NonplanarSettings["mode"] }));
    }
    for (const [id, patch] of Object.entries(fix.object_settings)) {
      const model = models.find((entry) => String(entry.id) === id);
      if (!model) continue;
      const updated = await api.setModelSettings(model.id, { ...model.settings, ...patch });
      setModels((current) => current.map((entry) => (entry.id === updated.id ? updated : entry)));
    }
    for (const [id, rotation] of Object.entries(fix.rotations_deg)) {
      const model = models.find((entry) => String(entry.id) === id);
      if (model) await commitAroundCentre(model, { rotation_deg: rotation });
    }
    for (const [id, slot] of Object.entries(fix.slots)) {
      const updated = await api.setModelSlot(Number(id), slot);
      setModels((current) => current.map((entry) => (entry.id === updated.id ? updated : entry)));
    }
    for (const [id, plate] of Object.entries(fix.plates)) {
      const turned = await api.moveModelToPlate(Number(id), plate);
      setModels(turned.models);
      setPlates(turned.plates);
    }
    dismissSuggestion(item);
    forgetSlice();
  }

  useEffect(() => {
    if (pane !== "prepare" || !printerId) {
      setSuggestions([]);
      return;
    }
    let live = true;
    const timer = window.setTimeout(() => {
      const settings: Record<string, string> = {};
      for (const [key, value] of Object.entries(overrides)) {
        if (value) settings[key] = value;
      }
      const slots = [
        { index: 0, material_id: material ?? "", filament_type: materialLabel(material) },
        ...ams.map((id, index) => ({
          index: index + 1,
          material_id: id,
          filament_type: materialLabel(id || null),
        })),
      ].filter((slot) => slot.material_id);
      let never: string[] = [];
      try {
        const raw = window.localStorage.getItem("shale.suggestions.never");
        const parsed = raw ? (JSON.parse(raw) as string[]) : [];
        if (Array.isArray(parsed)) never = parsed;
      } catch {
        never = [];
      }
      void api
        .suggestions({
          printer_id: printerId,
          variant_name: variant,
          quality_type: quality,
          material_id: material,
          settings,
          slots,
          tags: [],
          dismissed: dismissedSuggestions,
          global_dismissed: never,
          shown_once: suggestionsShown.filter((id) => !suggestionsSeen.current.has(id)),
          nonplanar_mode: nonplanar.mode,
        })
        .then((items) => {
          if (!live) return;
          for (const item of items) {
            if (item.once) suggestionsSeen.current.add(item.rule_id);
          }
          setSuggestions(items);
          const fresh = items.filter((item) => item.once && !suggestionsShown.includes(item.rule_id));
          if (fresh.length > 0) {
            setSuggestionsShown((current) => [...current, ...fresh.map((item) => item.rule_id)]);
          }
        })
        .catch(() => {
          if (live) setSuggestions([]);
        });
    }, 250);
    return () => {
      live = false;
      window.clearTimeout(timer);
    };
  }, [pane, printerId, variant, quality, material, ams, overrides, models, nonplanar.mode, dismissedSuggestions, suggestionsShown]);

  function materialName(): string {
    return materialLabel(material);
  }

  /// The bays as the machine numbers them: T1 is the plate's own material, and
  /// T2..T4 are the AMS. An empty string is an empty bay, not a blank label.
  function spoolsForDevice(): Spool[] {
    const bays: Spool[] = [{ bay: 1, material: materialLabel(material), colour: materialColour(material) }];
    ams.forEach((slot, index) =>
      bays.push({ bay: index + 2, material: materialLabel(slot || null), colour: materialColour(slot || null) }),
    );
    return bays;
  }

  /// Write the plate to a path, with no dialog: the tab bar uses this to put a
  /// tab's own file away before switching away from it.
  async function writeProjectFile(path: string, tabId = activeTabRef.current, adopt = true): Promise<boolean> {
    const fileName = path.split(/[/\\]/).pop()?.replace(/\.shale$/i, "") || "Untitled";
    // A park writes to a scratch file whose name was sanitised for the disk
    // ("Untitled 2" became "Untitled-2"), and the file's `name` is what the tab
    // is called when it is read back. So a park records the tab's own name;
    // only a real save adopts the file's.
    const parkedName = tabsRef.current.find((tab) => tab.id === tabId)?.name ?? fileName;
    try {
      await api.saveProject({
        path,
        name: adopt ? fileName : parkedName,
        printer_id: printerId,
        printer_label: printerLabel(),
        variant_name: variant,
        quality_type: quality,
        material_id: material,
        ams,
        settings: currentSettings(),
        chord_mm: chord,
        turn_deg: turn,
        thumbnail: thumbnailJpeg(models),
        quiet: !adopt,
        dismissed_suggestions: dismissedSuggestions,
        suggestions_shown: suggestionsShown,
        ams_mapping: amsMapping,
      });
      // A quiet save is a place to put a plate down, not a file to adopt: the
      // tab keeps its name and its (empty) path, and the project does not start
      // claiming to live in the temp folder.
      if (adopt) {
        setProjectPath(path);
        recents.refresh();
        setProjectName(fileName);
      }
      setTabs((current) =>
        current.map((tab) =>
          tab.id === tabId
            ? adopt
              ? { ...tab, path, dirty: false, name: fileName }
              : { ...tab, scratch: path, dirty: false }
            : tab,
        ),
      );
      return true;
    } catch (err) {
      setError(messageOf(err));
      return false;
    }
  }

  async function saveProject(asNew: boolean) {
    setError(null);
    let path = asNew ? null : projectPath;
    const name = projectName.trim() || "Untitled";
    if (!path) {
      const picked = await save({
        defaultPath: `${name}.shale`,
        filters: [{ name: "Shale project", extensions: ["shale"] }],
      });
      if (typeof picked !== "string") return;
      path = picked.toLowerCase().endsWith(".shale") ? picked : `${picked}.shale`;
    }
    if (await writeProjectFile(path)) setNotice(`Saved ${projectName.trim() || name}.shale`);
  }

  /// Switch to another open project. The tab you leave is written first — to
  /// its own file, or to a scratch copy when it has never been saved — because
  /// the backend holds one plate at a time and the plate is the thing being
  /// put away.
  /// Put the front plate away. A draft is parked whether or not the dot is
  /// showing: the tab strip is a promise that the plate is still there, and a
  /// plate that was never marked dirty would otherwise be dropped without a
  /// word. Every way of leaving a tab goes through here — clicking another tab,
  /// pressing +, opening a file into a new tab.
  async function parkActive() {
    const leaving = tabsRef.current.find((tab) => tab.id === activeTabRef.current);
    if (!leaving || (leaving.path != null && !leaving.dirty)) return;
    const target = leaving.path ?? leaving.scratch ?? (await api.scratchPath(leaving.name));
    await writeProjectFile(target, leaving.id, leaving.path != null);
  }

  async function selectTab(id: number) {
    if (id === activeTabRef.current) {
      setPane("prepare");
      return;
    }
    // Park the plate you are leaving before the other one is read: the file
    // is the only thing that survives a switch.
    await parkActive();
    const next = tabsRef.current.find((tab) => tab.id === id);
    if (!next) return;
    activate(id);
    const parked = next.path ?? next.scratch;
    if (parked) {
      // The plate in the core still belongs to the tab you just left, and
      // restoring a parked plate puts its models *on top of* what is loaded. So
      // the plate is emptied before the file is read, or switching tabs stacks
      // both plates and the old parts come back with the new one.
      await emptyPlate();
      if (next.path != null) {
        await openProject(next.path);
      } else {
        // A parked plate is a draft: read it back without letting the scratch
        // file become the project, or the tab would be renamed after it and
        // saving would land in the temp folder.
        try {
          const opened = await api.openProject(parked);
          applyOpened(opened);
          setProjectPath(null);
          setProjectName(next.name);
          recents.refresh();
        } catch (err) {
          setError(messageOf(err));
        }
      }
    } else {
      await resetPlate();
    }
    setPane("prepare");
  }

  async function closeTab(id: number) {
    const left = tabsRef.current.filter((tab) => tab.id !== id);
    setTabs(left);
    if (id !== activeTabRef.current) return;
    if (left.length === 0) {
      await resetPlate();
      addTab("Untitled", null);
      return;
    }
    const next = left[0];
    activate(next.id);
    if (next.path) await openProject(next.path);
    else await resetPlate();
  }

  async function openInNewTab() {
    const chosen = await open({
      multiple: false,
      filters: [{ name: "Shale project", extensions: ["shale"] }],
    });
    if (typeof chosen !== "string") return;
    const existing = tabsRef.current.find((tab) => tab.path === chosen);
    if (existing) {
      await selectTab(existing.id);
      return;
    }
    // Opening into a new tab is also leaving the plate you were on.
    await parkActive();
    addTab(chosen.split(/[/\\]/).pop()?.replace(/\.shale$/i, "") || "Untitled", chosen);
    await openProject(chosen);
  }

  function applyOpened(opened: OpenedProject) {
    restore.current = {
      variant: opened.variant_name,
      quality: opened.quality_type,
      material: opened.material_id,
    };
    setProjectPath(opened.path);
    setProjectName(opened.name || "Untitled");
    setPrinterId(opened.printer_id);
    setVariant(opened.variant_name);
    pendingQuality.current = opened.quality_type;
    setQuality(opened.quality_type);
    setMaterial(opened.material_id);
    setAms([0, 1, 2].map((index) => opened.ams?.[index] ?? ""));
    setAmsMapping(opened.ams_mapping ?? []);
    setOverrides(overridesFromSliceSettings(opened.settings));
    setNonplanar(nonplanarFromSliceSettings(opened.settings));
    setNonplanarReport(null);
    if (opened.chord_mm > 0) {
      setChord(opened.chord_mm);
      setChordTied(false);
    }
    if (opened.turn_deg > 0) setTurn(opened.turn_deg);
    remember();
    setModels(opened.models);
    setSelectedId(opened.models[0]?.id ?? null);
    setInstructions(opened.instructions);
    setSupport(opened.support);
    setDismissedSuggestions(opened.dismissed_suggestions ?? []);
    setSuggestionsShown(opened.suggestions_shown ?? []);
    suggestionsSeen.current = new Set();
    setPreview(false);
    setShowModel(true);
    setPaths([]);
    setLayerCount(0);
    setLayer(0);
    forgetSlice();
    setProgress(null);
    nameTab(opened.path, opened.name);
    setPane("prepare");
    if (opened.warnings.length > 0) setNotice(opened.warnings.join("\n"));
    enterSlicer();
  }

  async function openProject(path?: string) {
    setError(null);
    let picked = path ?? null;
    if (!picked) {
      const chosen = await open({
        multiple: false,
        filters: [{ name: "Shale project", extensions: ["shale"] }],
      });
      if (typeof chosen !== "string") return;
      picked = chosen;
    }
    // A file opened from a dialog, a recent, or a double-click needs a tab of
    // its own; a file already open gets its tab brought forward instead.
    const fileLabel = picked.split(/[/\\]/).pop()?.replace(/\.shale$/i, "") || "Untitled";
    const already = tabsRef.current.find((tab) => tab.path === picked);
    const front = tabsRef.current.find((tab) => tab.id === activeTabRef.current);
    // A blank plate nobody has touched is not worth a second tab: it becomes
    // the file. That is what every editor does with an empty window.
    const blank = front && front.path == null && !front.dirty && models.length === 0 ? front : null;
    if (already) activate(already.id);
    else if (blank) {
      setTabs((current) => current.map((tab) => (tab.id === blank.id ? { ...tab, name: fileLabel, path: picked } : tab)));
    } else addTab(fileLabel, picked);
    try {
      const opened = await api.openProject(picked);
      applyOpened(opened);
      setSupportBoxes(await api.supportBoxes().catch(() => []));
      recents.refresh();
    } catch (err) {
      setError(messageOf(err));
      recents.refresh();
    }
  }

  /// Empty the plate without touching the tab bar: used when a tab is closed
  /// and when a tab that has never been saved is switched back to.
  /** Empty the core's plate without touching the window's recipe. A tab switch
   * needs the plate gone before the parked one is read, but it must not reset
   * the printer, the material, or the name the incoming tab is about to adopt. */
  async function emptyPlate() {
    try {
      await api.clearJob();
    } catch (err) {
      setError(messageOf(err));
      return;
    }
    past.current = [];
    future.current = [];
    syncHistory();
    setModels([]);
    setSelectedId(null);
    setInstructions([]);
    setSupport(null);
    setPreview(false);
    setPaths([]);
    setLayerCount(0);
    setLayer(0);
    forgetSlice();
    setProgress(null);
  }

  async function resetPlate() {
    setError(null);
    try {
      await api.clearJob();
    } catch (err) {
      setError(messageOf(err));
      return;
    }
    past.current = [];
    future.current = [];
    syncHistory();
    setModels([]);
    setSelectedId(null);
    setInstructions([]);
    setSupport(null);
    setPreview(false);
    setShowModel(true);
    setPaths([]);
    setLayerCount(0);
    setLayer(0);
    forgetSlice();
    setProgress(null);
    setProjectPath(null);
    setProjectName("Untitled");
    setNonplanar(DEFAULT_NONPLANAR);
    setNonplanarReport(null);
    setPane("prepare");
    enterSlicer();
  }

  async function newProject() {
    const front = tabsRef.current.find((tab) => tab.id === activeTabRef.current);
    // A blank plate nobody has touched is not worth a second tab: "new project"
    // in a window that is already an untouched blank should give one blank, the
    // way every editor does.
    const untouched =
      front != null && front.path == null && front.scratch == null && !front.dirty && modelsRef.current.length === 0;
    // + clears the plate, so the plate has to be put away first.
    if (!untouched) await parkActive();
    await resetPlate();
    if (untouched) {
      setPane("prepare");
      return;
    }
    // Two blank plates in one window still need two names: the tab strip is
    // the only thing telling them apart.
    const used = new Set(tabsRef.current.map((tab) => tab.name));
    let name = "Untitled";
    for (let index = 2; used.has(name); index += 1) name = `Untitled ${index}`;
    addTab(name, null);
    setPane("prepare");
  }

  async function openSavedModel(saved: SavedModel) {
    await newProject();
    const opened = await importFromPath(saved.path);
    if (opened) {
      setNotice(`Opened ${saved.title} by ${saved.creator}. Licence: ${saved.licence}.`);
    }
  }

  async function quit() {
    try {
      await getCurrentWindow().close();
    } catch (err) {
      setError(messageOf(err));
    }
  }
  actions.current = {
    newProject,
    importModel,
    exportGcode,
    duplicateSelected,
    arrangeAll,
    undo,
    redo,
  };
  saveRef.current = (asNew) => {
    void saveProject(asNew);
  };
  openRef.current = (path) => {
    void openProject(path);
  };

  /// A slice decides four numbers together, and a plate that changed decides
  /// none of them: they are dropped as one so a stale time never sits beside a
  /// fresh layer count.
  function forgetSlice() {
    setPrintSeconds(null);
    setFilamentMetres(null);
  }

  const minutes = printSeconds == null ? null : Math.max(1, Math.round(printSeconds / 60));
  /// Preview draws the toolpath and nothing else. A solid part in front of it
  /// hides exactly the thing the preview exists to show — the infill inside.
  const solidShown = showModel && !preview;
  const materialInfo = materials.find((item) => item.id === material);
  // CuraEngine counts filament as length. Weight is length times the strand's
  // cross-section times the profile's density, and the profile is the only
  // place a density can honestly come from: Cura's materials do not carry one.
  const grams = (() => {
    if (filamentMetres == null) return null;
    const density = materialInfo?.density;
    if (!density) return null;
    const diameter = materialInfo?.diameter ?? 1.75;
    const area = Math.PI * (diameter / 2) ** 2;
    return filamentMetres * area * density;
  })();
  useEffect(() => {
    void api
      .costSettings()
      .then(setCost)
      .catch(() => {
        /* defaults are already in state */
      });
  }, []);

  // The card's cost figures are the core's arithmetic over the file on screen.
  // Re-run whenever the slice, the material or a price changes; the backend
  // reads the saved settings, so a price edit lands here as soon as it is
  // written and never makes the card disagree with the panel.
  useEffect(() => {
    const density = materialInfo?.density;
    if (filamentMetres == null || !density) {
      setEstimate(null);
      return;
    }
    let live = true;
    void api
      .costEstimate(filamentMetres, printSeconds, materialInfo?.diameter ?? 1.75, density)
      .then((next) => {
        if (live) setEstimate(next);
      })
      .catch(() => {
        if (live) setEstimate(null);
      });
    return () => {
      live = false;
    };
  }, [filamentMetres, printSeconds, materialInfo?.density, materialInfo?.diameter, cost]);

  // A file handed over on the command line: at launch, or by a second launch
  // that wrote its path down and raised this window. The poll is 800 ms, which
  // is invisible next to a model import and costs one file read when idle.
  useEffect(() => {
    let stopped = false;
    const openRequested = (path: string | null) => {
      if (!path || stopped) return;
      if (path.toLowerCase().endsWith(".shale")) void openProject(path);
      else void importFromPath(path);
    };
    void api.startupOpen().then(openRequested).catch(() => {});
    const timer = window.setInterval(() => {
      void api.takePendingOpen().then(openRequested).catch(() => {});
    }, 800);
    return () => {
      stopped = true;
      window.clearInterval(timer);
    };
    // The two openers are stable for the life of the window in practice; this
    // effect is about the handover, not about re-running when a model loads.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  const changeCost = (patch: Partial<CostSettings>) => {
    const next = { ...cost, ...patch };
    setCost(next);
    void api
      .setCostSettings(next)
      .then(setCost)
      .catch(() => {
        /* the panel keeps what was typed; the file is rewritten next time */
      });
  };
  const money = (value: number) => `${value.toFixed(2)} ${estimate?.currency ?? cost.currency}`;
  const clockTime = (seconds: number) =>
    `${Math.floor(seconds / 3600)} h ${String(Math.round((seconds % 3600) / 60)).padStart(2, "0")} m`;
  const marks = instructionMarks(instructions, layerZ, layerHeight);
  const marksHere = instructions.filter((item) => markOf(item, layerZ, layerHeight) === layer);

  function forgetRecentWithUndo(path: string) {
    const entry = recents.all.find((row) => row.path === path);
    recents.remove(path);
    if (!entry) return;
    toastUndo("Removed from recents", () => recents.restore(entry), { id: `recent-${path}` });
  }

  function selectPane(next: Pane) {
    if (next === "home") {
      setPane("home");
      setScreen("start");
      return;
    }
    if (next === "models") {
      setNotice(null);
      setScreen("slicer");
      setPane("models");
      return;
    }
    if (next === "preview" && layerCount === 0) {
      setNotice("nothing to preview yet — press slice first");
      return;
    }
    setNotice(null);
    setScreen("slicer");
    setPane(next);
    if (next === "preview") {
      setPreview(true);
      setShowModel(false);
      setFrameToken((token) => token + 1);
    }
    if (next === "prepare") {
      setPreview(false);
      setShowModel(true);
    }
  }

  const commandItems = useMemo(
    () =>
      buildCommandPaletteItems({
        pane,
        sliced: layerCount > 0,
        recents: recents.all,
        printers,
        printerId: printerId || null,
        qualities,
        quality,
        materials,
        material,
        uiStyle,
        hasModels: models.length > 0,
        onNewProject: () => void newProject(),
        onOpenProject: () => void openProject(),
        onImport: () => void importModel(),
        onSlice: () => void slice(),
        onOpenSliceSettings: () => {
          setScreen("slicer");
          setSettingsSection(null);
          setSettingsNonce((nonce) => nonce + 1);
        },
        onClearPlate: () => void clearWorkplate(),
        onPane: selectPane,
        onPrinter: setPrinterWithUndo,
        onQuality: setQualityWithUndo,
        onMaterial: setMaterialWithUndo,
        onToggleTheme: toggleUiStyle,
        onOpenSettings: () => {
          setPane("home");
          setScreen("start");
          setStartPanel("settings");
        },
        onOpenRecent: (path) => void openProject(path),
        nonplanarMode: nonplanar.mode,
        onNonplanarMode: (mode) => setNonplanar((current) => ({ ...current, mode })),
        onToggleStrict: () => setNonplanar((current) => ({ ...current, strict: !current.strict })),
        onOpenNonplanar: () => {
          setScreen("slicer");
          setPane("prepare");
          setSettingsSection("Non-planar");
          setSettingsNonce((nonce) => nonce + 1);
        },
        onExportGcode: () => void exportGcode(),
        onArrange: () => void arrangeAll(),
        onUndo: () => void undo(),
        onRedo: () => void redo(),
        canUndo: historyDepth.past > 0,
        canRedo: historyDepth.future > 0,
        onUiStyle: chooseUiStyle,
        liveSlice,
        onToggleLiveSlice: () => setLiveSlice(!liveSlice),
      }),
    [
      pane,
      layerCount,
      recents.all,
      printers,
      printerId,
      qualities,
      quality,
      materials,
      material,
      uiStyle,
      models.length,
      nonplanar.mode,
      liveSlice,
      historyDepth.past,
      historyDepth.future,
    ],
  );

  const qualityName =
    qualities.find((item) => item.quality_type === quality)?.name ?? quality ?? "Quality preset";
  const activeVariant = variants.find((item) => item.name === variant);
  const profileVariantLabel = activeVariant ? variantOptionLabel(activeVariant) : variant;

  useEffect(() => {
    if (!notice) return;
    const [title, ...rest] = notice.split("\n");
    toast(title, {
      id: "shale-notice",
      description: rest.length > 0 ? rest.join("\n") : undefined,
    });
  }, [notice]);

  if (import.meta.env.VITE_SHALE_MOCK === "1" && new URLSearchParams(window.location.search).get("showcase") === "ui") {
    return (
      <TooltipProvider>
        <div className="app app--showcase">
          <UiShowcase />
        </div>
      </TooltipProvider>
    );
  }

  return (
    <TooltipProvider>
    <div className="app">
      <TitleBar
        projectName={projectPath ? projectName : ""}
        leading={
          <TitleBarNav
            sidebarCollapsed={sidebarCollapsed}
            onToggleSidebar={toggleSidebar}
            canBack={pane === "home" ? startPanel !== "home" || workspace : false}
            canForward={false}
            onBack={() => {
              if (pane === "home") {
                if (startPanel !== "home") setStartPanel("home");
                else if (workspace) {
                  setScreen("slicer");
                  setPane("prepare");
                }
              }
            }}
            onForward={() => undefined}
          />
        }
      >
        <MenuBar
          screen={screen}
          projectName={projectName}
          busy={busy}
          sliced={layerCount > 0}
          hasModels={models.length > 0}
          hasSelection={selectedId !== null}
          importing={importing}
          onStart={() => {
            setScreen("start");
            setPane("home");
          }}
          onNew={() => void newProject()}
          onOpen={() => void openProject()}
          onImport={() => void importModel()}
          onSave={() => void saveProject(false)}
          onSaveAs={() => void saveProject(true)}
          onExport={() => void exportGcode()}
          onExportGeometry={(format) => void exportGeometry(format)}
          onQuit={() => void quit()}
          onDuplicate={() => void duplicateSelected()}
          onSplit={() => void splitSelected()}
          onAddPart={(kind) => void addPart(kind)}
          onFillPlate={() => void fillPlate()}
          onDelete={() => void removeSelected()}
          onDrop={() => void dropSelected()}
          onArrange={() => void arrangeAll()}
          onUndo={() => void undo()}
          onRedo={() => void redo()}
          canUndo={historyDepth.past > 0}
          canRedo={historyDepth.future > 0}
          modelCount={models.length}
          viewStyle={viewStyle}
          onViewStyle={setViewStyle}
          preview={preview}
          onPreview={setPreview}
          showModel={solidShown}
          onShowModel={setShowModel}
          onFrame={() => setFrameToken((token) => token + 1)}
          printers={printers}
          printerId={printerId}
          onPrinter={setPrinterWithUndo}
          variants={variants}
          variant={variant}
          onVariant={setVariant}
          qualities={qualities}
          quality={quality}
          onQuality={(next) => {
            setQualityWithUndo(next);
          }}
          materials={materials}
          material={material}
          onMaterial={setMaterialWithUndo}
          quickFallbacks={quickFallbacks}
          demo={status?.demo ?? null}
          overrides={scopedOverrides}
          profileValues={profileValues}
          onOverride={(key, value) => void setScopedOverride(key, value)}
          onResetOverrides={() => void resetScopedOverrides()}
          scope={scope}
          parts={models.map((model) => ({ id: model.id, name: model.name }))}
          onScope={setScope}
          onSlice={() => void slice()}
          sliceStage={busy && progress ? sliceStage(progress.message) : null}
          uiStyle={uiStyle}
          onUiStyle={chooseUiStyle}
          nonplanar={nonplanar}
          onNonplanar={setNonplanar}
          nonplanarReport={nonplanarReport}
          nonplanarSupported={nonplanarOk}
          liveSlice={liveSlice}
          onLiveSlice={setLiveSlice}
          settingsNonce={settingsNonce}
          settingsSection={settingsSection}
          pane={pane}
          onPane={selectPane}
          onOpenPalette={() => setCommandPaletteOpen(true)}
        />
      </TitleBar>

      {status && !status.resources_ok && (
        <p className="banner error" role="status">
          {status.resources_message}
        </p>
      )}
      {status && status.resources_ok && status.catalog_loading && !status.catalog_full && (
        <p className="banner subtle" role="status">
          {status.resources_message}
        </p>
      )}
      {status && status.resources_ok && !status.engine_ok && (
        <p className="banner error" role="status">
          {status.engine_message || `CuraEngine was not found at ${status.engine_path}.`}
        </p>
      )}
      {importing && (
        <div className="banner" role="status">
          <Meter
            fraction={importProgress?.fraction ?? 0.08}
            message={importProgress?.message ?? "Reading the model"}
            stage={sliceStage(importProgress?.message ?? "Reading the model")}
          />
          <button type="button" onClick={() => void cancelImport()}>
            Cancel
          </button>
        </div>
      )}
      {error && (
        <p className="banner error" role="alert">
          {error}
          {(error.includes("profiles") || error.includes("timed out")) && (
            <button
              type="button"
              className="banner-action"
              onClick={() => {
                setError(null);
                catalogAttemptsRef.current = 0;
                void api.retryCatalogLoad();
              }}
            >
              retry
            </button>
          )}
        </p>
      )}

      <TabBar
        pane={pane}
        onPane={selectPane}
        tabs={docTabs}
        activeTab={activeTab}
        onTab={(id) => void selectTab(id)}
        onCloseTab={(id) => void closeTab(id)}
        onNewTab={() => void newProject()}
        onOpenTab={() => void openInNewTab()}
        printer={printerLabel()}
        sliced={layerCount > 0}
      />

      <ShellFrame
        collapsed={sidebarCollapsed}
        sidebar={
        <AppSidebar
          collapsed={sidebarCollapsed}
          recents={recents.all}
          recentsRead={recents.read}
          onNew={() => void newProject()}
          onOpen={() => void openProject()}
          onImport={() => void importModel()}
          onOpenRecent={(path) => void openProject(path)}
          onForgetRecent={forgetRecentWithUndo}
          onOpenCommandPalette={() => setCommandPaletteOpen(true)}
          discovering={pane === "models"}
          onDiscover={() => selectPane("models")}
          plates={plates}
          activePlate={plates.find((plate) => plate.active)?.index}
          onShowPlate={(index) => void showPlate(index)}
          profileQuality={qualityName}
          profileMaterial={materialName() || "Material not set"}
          profileVariant={profileVariantLabel}
          onProfiles={() => {
            setPane("home");
            setScreen("start");
            setStartPanel("settings");
          }}
          printer={printers.find((entry) => entry.id === printerId) ?? null}
          printerLabel={variant ?? undefined}
          onPrinters={() => {
            setPane("home");
            setScreen("start");
            setStartPanel("printers");
          }}
          onSettings={() => {
            setPane("home");
            setScreen("start");
            setStartPanel("settings");
          }}
          onAbout={() => {
            setPane("home");
            setScreen("start");
            setStartPanel("about");
          }}
          onQuit={() => void quit()}
          onDeviceQueue={
            pane !== "device"
              ? () => {
                  setPane("device");
                  setScreen("slicer");
                }
              : undefined
          }
          uiStyle={uiStyle}
          onUiStyle={chooseUiStyle}
          doneSlices={history.slice(-6).reverse().map((record) => ({
            id: String(record.n),
            title: `Slice ${record.n}`,
            subtitle: `${record.layers} layers${record.seconds != null ? ` · ${clockTime(record.seconds)}` : ""}`,
          }))}
        />
        }
      >
        <ViewSwap view={pane === "prepare" || pane === "preview" ? "workspace" : pane}>
      {pane === "home" ? (
        <div className="home-pane">
          <StartScreen
            live={!busy && !importing}
            panel={startPanel}
            onPanel={setStartPanel}
            canReturn={workspace}
            printers={printers}
            printerId={printerId}
            enginePath={enginePath}
            libDir={libDir}
            backend={backend}
            version={status?.version ?? "0.1.4"}
            onReturn={() => {
              setScreen("slicer");
              setPane("prepare");
            }}
            onNew={() => void newProject()}
            onOpen={() => void openProject()}
            onImport={() => void importModel()}
            onChoosePrinter={setPrinterId}
            onEnginePath={setEnginePath}
            onLibDir={setLibDir}
            onBackend={setBackend}
            onApplyEngine={() => void applyEngine()}
            onQuit={() => void quit()}
            uiStyle={uiStyle}
            onUiStyle={chooseUiStyle}
            onRerunSetup={() => setSetupWizardOpen(true)}
            recents={recents.all}
            onOpenRecent={(path) => void openProject(path)}
            onDropModel={(path) => {
              if (path) void importFromPath(path);
              else void importModel();
            }}
            onDiscover={() => selectPane("models")}
            settingsSection={startSettingsSection}
          />
        </div>
      ) : pane === "models" ? (
        <ModelsView
          onOpened={(saved) => openSavedModel(saved)}
          onOpenAccounts={() => {
            setStartSettingsSection("accounts");
            setPane("home");
            setScreen("start");
            setStartPanel("settings");
          }}
        />
      ) : pane === "device" ? (
        <Suspense fallback={<div className="workspace-skeleton" aria-hidden><div className="ui-sidebar__skeleton"><i style={{ width: "46%" }} /><i style={{ width: "72%" }} /><i style={{ width: "58%" }} /></div></div>}>
        <DevicePane
          machine={printerLabel()}
          source={printers.find((item) => item.id === printerId)?.source ?? "cura"}
          bedWidth={bed.width}
          bedDepth={bed.depth}
          bedHeight={bed.height}
          nozzleSize={bed.nozzle_size}
          spools={spoolsForDevice()}
          baysInUse={models.map((model) => (model.slot ?? 0) + 1)}
          job={{
            layers: layerCount,
            seconds: printSeconds,
            layerHeight,
            nozzle,
            bedTemp,
            material: materialName(),
            sliced: layerCount > 0,
          }}
          engineBackend={backend}
          engineReady={enginePath.length > 0}
          gcodeReady={layerCount > 0}
          plate={docTabs.find((tab) => tab.id === activeTab)?.name ?? "plate"}
          plates={plates.map((item) => (item.active ? { ...item, sliced: layerCount > 0 } : item))}
          onSliceMissing={() => sliceAll({ onlyUnsliced: true })}
          mapping={amsMapping}
          onMapping={setAmsMapping}
        />
        </Suspense>
      ) : (
      <div className="workspace">
        <aside className="panel">
        <div className="panel-scroll">
        <PaneFade pane={pane}>
          {pane === "prepare" && (
          <>
          <Frame title="Plates">
            <div className="row plates">
              {plates.map((plate) => (
                <span key={plate.index} className="plate-chip">
                  <button
                    type="button"
                    className={plate.active ? "plate-tab active" : "plate-tab"}
                    title={plate.name}
                    onClick={() => void showPlate(plate.index)}
                  >
                    <span className="plate-name">{plate.name}</span>
                    <span className="plate-count">
                      {plate.models === 0
                        ? "Empty"
                        : plate.models === 1
                          ? "1 part"
                          : `${plate.models} parts`}
                    </span>
                  </button>
                  {plates.length > 1 && (
                    <button
                      type="button"
                      className="plate-drop"
                      title={`remove ${plate.name}`}
                      onClick={() => void dropPlate(plate.index)}
                    >
                      ×
                    </button>
                  )}
                </span>
              ))}
              <button type="button" title="Add a plate" onClick={() => void newPlate()}>
                +
              </button>
              <button
                type="button"
                title="Slice every plate that has a part on it"
                disabled={busy || models.length + plates.reduce((sum, plate) => sum + (plate.active ? 0 : plate.models), 0) === 0}
                onClick={() => void sliceAll()}
              >
                Slice all
              </button>
            </div>
            <p className="hint">
              Each plate holds its own parts. Slice all walks them in order and shows which plate it is on.
            </p>
          </Frame>
          <ProjectSummary
            embedded={embedded}
            ignored={ignoreEmbedded}
            onIgnore={setIgnoreEmbedded}
          />

          <Frame title="Objects">
            {models.length > 0 && (
              <div className="row">
                <button
                  type="button"
                  title="Stand every part on the face that needs the least support. Undo puts them back."
                  onClick={() => void autoOrientAll()}
                >
                  Orient all
                </button>
              </div>
            )}
            {models.length === 0 ? (
              <p className="hint">Nothing on this plate yet. Import a model and it is listed here.</p>
            ) : (
              <ul className="objects" role="listbox" aria-label="Objects on this plate">
                {models.map((model, index) => (
                  <li key={model.id} className="object-line">
                    <button
                      type="button"
                      role="option"
                      aria-selected={model.id === selectedId}
                      className={model.id === selectedId ? "object-row active" : "object-row"}
                      title={`${model.name} — ${model.triangles.toLocaleString()} triangles`}
                      onClick={() => setSelectedId(model.id)}
                    >
                      <span className="object-index">{index + 1}</span>
                      <span className="object-name">{model.name}</span>
                      {model.slot != null && (
                        <span className="object-slot">AMS {model.slot + 1}</span>
                      )}
                    </button>
                    <button
                      type="button"
                      className="object-drop"
                      aria-label={`delete ${model.name}`}
                      title={`delete ${model.name}`}
                      onClick={() => void removeModel(model.id)}
                    >
                      ×
                    </button>
                    {model.id === selectedId && (
                      <ObjectOverrides
                        settings={model.settings}
                        onChange={(key, value) => writeObjectOverride(model, key, value)}
                      />
                    )}
                  </li>
                ))}
              </ul>
            )}
          </Frame>

          <Frame title="Paint support">
            {paintTarget === null ? (
              <p className="hint">Import a part and its undersides can be painted here.</p>
            ) : (
              <>
                <p className="hint">
                  {models.find((model) => model.id === paintTarget)?.name ?? "part"} —{" "}
                  {painted > 0 || blocked > 0
                    ? [
                        painted > 0 ? `${painted} held up` : null,
                        blocked > 0 ? `${blocked} kept clear` : null,
                      ]
                        .filter(Boolean)
                        .join(", ")
                    : patches.length > 0
                      ? `${patches.length} patch${patches.length === 1 ? "" : "es"} of underside`
                      : "nothing underneath to hold up"}
                </p>
                {/* Two brushes, because they ask for opposite things. A patch is
                    just triangles; what you press decides whether support grows
                    under them or is kept out. */}
                <div className="row paint-brush" role="group" aria-label="Brush">
                  {(
                    [
                      ["enforce", "Hold up"],
                      ["block", "Keep out"],
                    ] as const
                  ).map(([value, label]) => (
                    <button
                      key={value}
                      type="button"
                      className={`paint-brush-button${paintMode === value ? " on" : ""}`}
                      aria-pressed={paintMode === value}
                      onClick={() => setPaintMode(value)}
                    >
                      {label}
                    </button>
                  ))}
                </div>
                <div className="row paint-brush" role="group" aria-label="Brush reach">
                  <button
                    type="button"
                    className={`paint-brush-button${paintArmed ? " on" : ""}`}
                    aria-pressed={paintArmed}
                    title="Drag over the part to paint the surface under the cursor"
                    onClick={() => {
                      setBoxArmed(false);
                      setPaintArmed((armed) => !armed);
                    }}
                  >
                    Brush
                  </button>
                  <button
                    type="button"
                    className={`paint-brush-button${boxArmed ? " on" : ""}`}
                    aria-pressed={boxArmed}
                    title="Drag on the plate to place a support box"
                    onClick={() => {
                      setPaintArmed(false);
                      setBoxArmed((armed) => !armed);
                    }}
                  >
                    Box
                  </button>
                  <label className="paint-radius ui-field">
                    <span className="ui-field__label">Reach</span>
                    <Slider
                      label="Brush reach in millimetres"
                      value={[Math.max(1, Math.min(40, Number(paintRadius) || 1))]}
                      min={1}
                      max={40}
                      step={1}
                      unit="mm"
                      inputDigits={0}
                      onValueChange={(value) => setPaintRadius(String(Math.round(value)))}
                    />
                  </label>
                </div>
                {boxArmed && (
                  <>
                    <div className="row paint-brush">
                      <label className="paint-radius ui-field">
                        <span className="ui-field__label">Height</span>
                        <Slider
                          label="Box height in millimetres"
                          value={[Math.max(1, Math.min(80, Number(boxHeight) || 8))]}
                          min={1}
                          max={80}
                          step={1}
                          unit="mm"
                          inputDigits={0}
                          onValueChange={(value) => setBoxHeight(String(Math.round(value)))}
                        />
                      </label>
                    </div>
                    <p className="paint-hint">
                      drag on the plate — {paintMode === "block" ? "kept clear" : "held up"},{" "}
                      {Math.max(1, Number(boxHeight) || 8)} mm tall
                    </p>
                  </>
                )}
                {supportBoxes.length > 0 && (
                  <div className="row paint-brush">
                    <button type="button" className="paint-back" onClick={() => void api.undoSupportBox().then(setSupportBoxes)}>
                      Undo box
                    </button>
                    <button type="button" className="paint-clear" onClick={() => void api.clearSupportBoxes().then(setSupportBoxes)}>
                      Clear boxes
                    </button>
                  </div>
                )}
                {(paintArmed || patches.length > 0 || painted > 0 || blocked > 0) && (
                <div className="row paint-patches">
                  {paintArmed && (
                    <p className="paint-hint">
                      drag over the part — {paintMode === "block" ? "kept clear" : "held up"}
                    </p>
                  )}
                  {patches.map((patch, index) => (
                    <button
                      key={index}
                      type="button"
                      className="paint-patch"
                      title={`under ${patch.min[0].toFixed(0)},${patch.min[1].toFixed(0)} to ${patch.max[0].toFixed(0)},${patch.max[1].toFixed(0)}`}
                      onClick={() => void paintPatch(patch.triangles)}
                    >
                      {paintMode === "block" ? "keep out" : "hold up"} {Math.round(patch.area_mm2)} mm²
                    </button>
                  ))}
                  {(painted > 0 || blocked > 0) && (
                    <>
                      <button type="button" className="paint-back" onClick={() => void takeBackPaint()}>
                        take back
                      </button>
                      <button type="button" className="paint-clear" onClick={() => void unpaint()}>
                        clear
                      </button>
                    </>
                  )}
                </div>
                )}
                <p className="hint">
                  A painted face is held up by a column to the bed, and painting turns the plate's own
                  support off. What you paint is the whole of it.
                </p>
              </>
            )}
          </Frame>
          </>
          )}

          {/* Prepare owns the machine and the process: these are the things a
              slice is made from. Preview is a reading of the result and offers
              none of them. */}
          {pane === "prepare" && (<>
          <Suggestions
            items={suggestions}
            onApply={(item) => void applySuggestion(item)}
            onDismiss={dismissSuggestion}
            onNever={neverSuggestion}
          />
          <Frame title="Printer">
            <label>
              Filter
              <TextField label="Filter" value={printerQuery} placeholder="Name…" onChange={setPrinterQuery} />
            </label>
            <label>
              Machine
              <Select
                label="Machine"
                value={printerId}
                options={filteredPrinters.map((printer) => ({
                  value: printer.id,
                  label: `${printer.manufacturer ? `${printer.manufacturer} ` : ""}${printer.name}`,
                }))}
                onChange={setPrinterId}
              />
            </label>
            <label>
              Nozzle
              <Select
                label="Nozzle"
                value={variant ?? ""}
                options={variants.map((item) => ({
                  value: item.name,
                  label: variantOptionLabel(item),
                }))}
                onChange={(next) => setVariant(next || null)}
              />
            </label>
            <label>
              Quality
              <Select
                label="Quality"
                value={quality ?? ""}
                options={qualities.map((item) => ({ value: item.quality_type, label: item.name }))}
                onChange={(next) => {
                  setQuality(next || null);
                  void resetScopedOverrides();
                }}
              />
            </label>
            <label>
              Material
              <Select
                label="Material"
                value={material ?? ""}
                options={spools.map((item) => ({ value: item.id, label: item.label }))}
                onChange={(next) => setMaterial(next || null)}
              />
            </label>
            <p className="value-chips">
              <span>Bed {bed.width.toFixed(0)} × {bed.depth.toFixed(0)} × {bed.height.toFixed(0)} mm</span>
              <span>Nozzle {bed.nozzle_size.toFixed(2)} mm</span>
              <span>Line {bed.line_width.toFixed(2)} mm</span>
              <span>{bed.speed_print.toFixed(0)} mm/s</span>
              {bed.warning_count > 0 ? <span>{bed.warning_count} resolver warnings</span> : null}
            </p>
          </Frame>

          <Frame title="Non-planar">
            <NonPlanarPanel
              value={nonplanar}
              onChange={setNonplanar}
              report={nonplanarReport}
              supported={nonplanarOk}
              liveSlice={liveSlice}
              onLiveSlice={setLiveSlice}
            />
          </Frame>

          <Frame title="AMS">
            <p className="meta">
              {ams.filter(Boolean).length === 0
                ? "One spool: the plate material"
                : `plate plus ${ams.filter(Boolean).length} spool${ams.filter(Boolean).length === 1 ? "" : "s"}`}
            </p>
            {[0, 1, 2].map((index) => (
              <label key={index}>
                Spool {index + 2}
                <Select
                  label={`Spool ${index + 2}`}
                  value={ams[index] ?? ""}
                  options={[
                    { value: "", label: "Empty" },
                    ...materials.map((item) => ({ value: item.id, label: item.label })),
                  ]}
                  onChange={(next) =>
                    setAms((list) => list.map((item, position) => (position === index ? next : item)))
                  }
                />
              </label>
            ))}
            <p className="hint">
              The plate material is Spool 1. Give a part a spool in the model frame and the slice gets a tool change,
              its own hotend temperature, and its own filament.
            </p>
          </Frame>

          <Frame title="Calibration" advanced>
            <p className="meta">
              {tower
                ? `${tower.bands.length} bands, ${tower.bands[0].value} to ${
                    tower.bands[tower.bands.length - 1].value
                  } ${tower.unit}, ${tower.band_height_mm} mm each — ${tower.kind}`
                : "A five-band temperature tower, 5 °C apart"}
            </p>
            <div className="row">
              <button type="button" disabled={calibrating} onClick={() => void addCalibration("temperature")}>
                Temperature tower
              </button>
              <button type="button" disabled={calibrating} onClick={() => void addCalibration("flow")}>
                Flow coupon
              </button>
              <button type="button" disabled={calibrating} onClick={() => void addCalibration("pressure")}>
                Pressure tower
              </button>
              <button type="button" disabled={!tower || calibrating} onClick={() => void stepCalibration()}>
                Step it
              </button>
            </div>
            {calibration && (
              <p className="meta">
                {calibration.injections.length} steps:{" "}
                {calibration.injections
                  .map((step) => `L${step.layer} ${step.value}${tower ? tower.unit : ""}`)
                  .join(", ")}
              </p>
            )}
            <p className="hint">
              The tower alternates 20 mm and 26 mm blocks, so every band overhangs the one below by 3 mm. The flow
              coupon is a 30 mm single-wall tube, because flow is measured with calipers, not judged. Slice it, then
              step it: one M104, M221 or M900 per band, written into the slice, and the preview stays as it was.
            </p>
          </Frame>

          <Frame title="Slice">
            <label>
              Layer height
              <NumericControl
                label="Layer height"
                value={layerHeight}
                min={0.05}
                max={0.8}
                step={0.01}
                digits={2}
                onChange={(value) => void setScopedOverride(QUICK_SETTING_KEYS.layerHeight, String(round3(value)))}
              />
            </label>
            <label>
              Infill
              <NumericControl
                label="Infill"
                value={infill}
                min={0}
                max={100}
                step={1}
                digits={0}
                onChange={(value) =>
                  void setScopedOverride(QUICK_SETTING_KEYS.infill, String(Math.round(value)))
                }
              />
            </label>
            <SwitchRow
              checked={supports}
              onChange={(value) => void setScopedOverride(QUICK_SETTING_KEYS.supports, value ? "1" : "0")}
            >
              Supports
            </SwitchRow>
            <label>
              Nozzle
              <NumericControl
                label="Nozzle temperature"
                value={nozzle}
                min={0}
                max={400}
                step={1}
                digits={0}
                onChange={(value) =>
                  void setScopedOverride(QUICK_SETTING_KEYS.nozzle, String(Math.round(value)))
                }
              />
            </label>
            <label>
              Bed
              <NumericControl
                label="Bed temperature"
                value={bedTemp}
                min={0}
                max={150}
                step={1}
                digits={0}
                onChange={(value) =>
                  void setScopedOverride(QUICK_SETTING_KEYS.bed, String(Math.round(value)))
                }
              />
            </label>
            {plateWarnings(materialName(), localStorage.getItem("shale.device.bed") ?? "textured_plate", bedTemp).map(
              (note) => (
                <p key={note} className="plate-advice" role="status">
                  {note}
                </p>
              ),
            )}
            <p className="hint">Layer height also sets the first layer. Changing quality reloads these values.</p>
            <label>
              Chord
              <NumericControl
                label="Chord"
                value={chord}
                min={0.02}
                max={0.5}
                step={0.01}
                digits={3}
                onChange={(value) => {
                  setChordTied(false);
                  setChord(value);
                }}
              />
            </label>
            <label>
              turn °
              <NumericControl
                label="Turn"
                value={turn}
                min={2}
                max={45}
                step={1}
                digits={0}
                onChange={setTurn}
              />
            </label>
            <button type="button" onClick={() => setChordTied(true)}>
              Tie chord to the layer
            </button>
            <p className="hint">
              step files are tessellated here. chord is half the layer height, and no coarser than half the nozzle.
              turn is the largest bend between two segments of a curve. stl and 3mf are already meshes.
            </p>
          </Frame>

          <Frame title="Supports">
            <label>
              angle
              <NumericControl label="Angle" value={supportAngle} min={0} max={90} step={1} digits={0} onChange={setSupportAngle} />
            </label>
            <label>
              bridge mm
              <NumericControl label="Bridge" value={bridgeMm} min={0} max={80} step={1} digits={0} onChange={setBridgeMm} />
            </label>
            <div className="row">
              <button type="button" disabled={selectedId == null || supportBusy} onClick={() => void proposeSupports()}>
                propose
              </button>
              <button type="button" disabled={!support || supportBusy} onClick={() => void compareSupports()}>
                compare
              </button>
            </div>
            {support && <ComparisonLines comparison={support.plan.comparison} />}
            {support && support.plan.regions.length === 0 && (
              <p className="hint">no overhang past the angle.</p>
            )}
            <ul className="instructions">
              {support?.plan.regions.map((region) => (
                <li key={region.id}>
                  <SwitchRow checked={region.enabled} onChange={() => void toggleSupport(region.id)}>
                    {`${region.role === "block" ? "block" : "support"}  ${region.summary}`}
                  </SwitchRow>
                </li>
              ))}
            </ul>
            <p className="hint">
              grey columns are support meshes. wire boxes block bridges and cosmetic slopes. the engine is not modified.
            </p>
          </Frame>

          <Frame title="Instructions">
            <label>
              kind
              <Select
                label="Kind"
                value={kind}
                options={[
                  { value: "pause", label: "pause" },
                  { value: "temperature", label: "temperature" },
                  { value: "fan", label: "fan" },
                  { value: "beep", label: "beep" },
                  { value: "message", label: "message" },
                  { value: "park_and_wait", label: "park and wait" },
                ]}
                onChange={(next) => setKind(next as InstructionKind)}
              />
            </label>
            {kind === "pause" && (
              <label>
                by
                <Select
                  label="By"
                  value={anchor}
                  options={[
                    { value: "layer", label: "layer" },
                    { value: "height", label: "height mm" },
                  ]}
                  onChange={(next) => setAnchor(next as "layer" | "height")}
                />
              </label>
            )}
            <label>
              {kind === "fan" || (kind === "pause" && anchor === "height") ? "height" : "layer"}
              <NumericControl
                label={kind === "fan" || (kind === "pause" && anchor === "height") ? "height" : "layer"}
                value={at}
                min={0}
                step={kind === "fan" || (kind === "pause" && anchor === "height") ? 0.1 : 1}
                digits={kind === "fan" || (kind === "pause" && anchor === "height") ? 1 : 0}
                onChange={setAt}
              />
            </label>
            {kind === "fan" && (
              <label>
                percent
                <NumericControl label="Percent" value={fanPercent} min={0} max={100} step={1} digits={0} onChange={setFanPercent} />
              </label>
            )}
            {kind === "temperature" && (
              <>
                <label>
                  Nozzle
                  <NumericControl label="Nozzle temperature" value={tempNozzle} min={0} max={400} step={1} digits={0} onChange={setTempNozzle} />
                </label>
                <label>
                  Bed
                  <NumericControl label="Bed temperature" value={tempBed} min={0} max={150} step={1} digits={0} onChange={setTempBed} />
                </label>
                <SwitchRow checked={tempWait} onChange={setTempWait}>
                  wait until reached
                </SwitchRow>
              </>
            )}
            {kind === "pause" && (
              <SwitchRow checked={filament} onChange={setFilament}>
                filament change
              </SwitchRow>
            )}
            {(kind === "pause" || kind === "message" || kind === "park_and_wait") && (
              <label>
                note
                <TextField label="Note" value={note} onChange={setNote} />
              </label>
            )}
            <button type="button" onClick={() => void addInstruction()}>
              add
            </button>
            {instructions.length === 0 && <p className="hint">none yet. they are stored on this job.</p>}
            <ul className="instructions">
              {instructions.map((item) => (
                <li key={item.id}>
                  <span>{item.summary}</span>
                  <button type="button" onClick={() => void removeInstruction(item.id)} aria-label="Remove instruction">
                    x
                  </button>
                </li>
              ))}
            </ul>
            <p className="hint">Marlin gets M600 / M0. RepRap pauses with M226. Klipper uses PAUSE, or M600 for a filament change.</p>
          </Frame>

          <Frame title="Model">
            {models.length === 0 && <p className="hint">Import an STL or a simple 3MF. It is centred on the bed.</p>}
            <ul className="models">
              {models.map((model) => (
                <li key={model.id}>
                  <button
                    type="button"
                    className={model.id === selectedId ? "model selected" : "model"}
                    onClick={() => setSelectedId(model.id)}
                  >
                    {model.name}
                    {model.slot ? `  T${model.slot + 1}` : ""}
                  </button>
                </li>
              ))}
            </ul>
            {selected && (
              <>
                <label>
                  spool
                  <Select
                    label="Spool"
                    value={String(selected.slot ?? 0)}
                    options={[
                      { value: "0", label: "plate (T1)" },
                      ...ams
                        .map((spool, index) => ({ spool, index }))
                        .filter((item) => item.spool)
                        .map((item) => ({ value: String(item.index + 1), label: `spool ${item.index + 2} (T${item.index + 2})` })),
                    ]}
                    onChange={(next) => void assignSlot(Number(next))}
                  />
                </label>
                <div className="modes" role="group" aria-label="Transform">
                  {(["translate", "rotate", "scale"] as const).map((item) => (
                    <button
                      key={item}
                      type="button"
                      className={mode === item ? "selected" : ""}
                      onClick={() => setMode(item)}
                    >
                      {item === "translate" ? "Move" : item === "rotate" ? "Rotate" : "Scale"}
                    </button>
                  ))}
                </div>
                <div className="axes">
                  {(["X", "Y", "Z"] as const).map((axis, index) => {
                    const axisIndex = index as 0 | 1 | 2;
                    const centre = worldCentre(selected.positions, selected.transform);
                    const shown =
                      mode === "translate"
                        ? centre[axisIndex]
                        : mode === "rotate"
                          ? selected.transform.rotation_deg[axisIndex]
                          : selected.transform.scale[axisIndex];
                    return (
                      <label key={axis}>
                        {mode === "rotate" ? `${axis}°` : axis}
                        <NumericControl
                          label={axis}
                          value={shown}
                          step={mode === "scale" ? 0.01 : mode === "translate" ? 1 : 1}
                          digits={mode === "scale" ? 2 : mode === "translate" ? 1 : 1}
                          onChange={(value) =>
                            mode === "translate"
                              ? editCentre(axisIndex, value)
                              : editNumber(axisIndex, mode === "rotate" ? "rotation_deg" : "scale", value)
                          }
                        />
                      </label>
                    );
                  })}
                </div>
                <div className="row" role="group" aria-label="Mirror and turn">
                  {(["X", "Y", "Z"] as const).map((axis, index) => (
                    <button
                      key={axis}
                      type="button"
                      title={`Mirror across ${axis}`}
                      onClick={() => mirrorAxis(index as 0 | 1 | 2)}
                    >
                      mirror {axis}
                    </button>
                  ))}
                  <button type="button" title="Turn a quarter turn about Z" onClick={() => turnQuarter(2, 1)}>
                    turn 90°
                  </button>
                </div>
                <div className="row">
                  <button type="button" onClick={() => void dropSelected()}>
                    Drop to plate
                  </button>
                  <button type="button" title="Stand the part on the face that needs least support. Undo puts it back." onClick={() => void autoOrient()}>
                    Auto orient
                  </button>
                  <button
                    type="button"
                    className={`face-arm${faceArmed ? " on" : ""}`}
                    aria-pressed={faceArmed}
                    title="Then click the face you want on the plate"
                    onClick={() => setFaceArmed((armed) => !armed)}
                  >
                    Place on face
                  </button>
                  <button type="button" onClick={() => void resetTransform()}>
                    Upright
                  </button>
                  <button type="button" onClick={() => void removeSelected()}>
                    Remove
                  </button>
                </div>
                <div className="row cut-row">
                  <select aria-label="Cut plane axis" value={cutAxis} onChange={(event) => setCutAxis(Number(event.target.value))}>
                    <option value={0}>cut X</option>
                    <option value={1}>cut Y</option>
                    <option value={2}>cut Z</option>
                  </select>
                  <input
                    aria-label="Cut plane offset"
                    type="number"
                    step={1}
                    value={cutOffset}
                    onChange={(event) => setCutOffset(event.target.value)}
                  />
                  <select aria-label="Keep after cutting" value={cutKeep} onChange={(event) => setCutKeep(event.target.value)}>
                    <option value="both">both halves</option>
                    <option value="above">upper only</option>
                    <option value="below">lower only</option>
                  </select>
                  <button type="button" title="Split the part with the plane. Drag the plane in the view to move it." onClick={() => void cutSelected()}>
                    Cut
                  </button>
                  <label className="cut-pin-toggle">
                    <input
                      type="checkbox"
                      checked={cutFaceDown}
                      onChange={(event) => setCutFaceDown(event.target.checked)}
                    />
                    cut face down
                  </label>
                </div>
                <div className="row cut-pins">
                  <label className="cut-pin-toggle">
                    <input
                      type="checkbox"
                      checked={cutPins > 0}
                      onChange={(event) => {
                        setCutPins(event.target.checked ? 2 : 0);
                        if (event.target.checked) setCutDovetail(0);
                      }}
                    />
                    pins
                  </label>
                  {cutPins > 0 && (
                    <>
                      <label className="cut-pin-field">
                        <span>⌀</span>
                        <input
                          aria-label="Pin diameter"
                          type="number"
                          min={1}
                          max={20}
                          step={0.5}
                          value={pinRadius}
                          onChange={(event) => setPinRadius(event.target.value)}
                        />
                      </label>
                      <label className="cut-pin-field">
                        <span>deep</span>
                        <input
                          aria-label="Pin depth"
                          type="number"
                          min={1}
                          max={30}
                          step={0.5}
                          value={pinHeight}
                          onChange={(event) => setPinHeight(event.target.value)}
                        />
                      </label>
                      <button
                        type="button"
                        className="cut-pin-count"
                        title="How many pins to try for — as many as fit inside the cut face"
                        onClick={() => setCutPins((current) => (current >= 4 ? 1 : current + 1))}
                      >
                        {cutPins} {cutPins === 1 ? "pin" : "pins"}
                      </button>
                    </>
                  )}
                </div>
                <div className="row cut-pins cut-joint">
                  <label className="cut-pin-toggle">
                    <input
                      type="checkbox"
                      checked={cutDovetail > 0}
                      onChange={(event) => {
                        setCutDovetail(event.target.checked ? 6 : 0);
                        if (event.target.checked) setCutPins(0);
                      }}
                    />
                    dovetail
                  </label>
                  {cutDovetail > 0 && (
                    <label className="cut-pin-field">
                      <span>root</span>
                      <input
                        aria-label="Dovetail root width"
                        type="number"
                        min={2}
                        max={40}
                        step={1}
                        value={cutDovetail}
                        onChange={(event) => setCutDovetail(Number(event.target.value) || 0)}
                      />
                    </label>
                  )}
                  {cutDovetail > 0 && (
                    <span className="cut-joint-note" title="The tab is twice the root wide at its tip, so the halves cannot be pulled apart along the cut">
                      tip {cutDovetail * 2} · deep {cutDovetail}
                    </span>
                  )}
                </div>
                <div className="row boolean-row">
                  <span className="boolean-label" title="One part against every other part on the plate">
                    boolean
                  </span>
                  <button
                    type="button"
                    disabled={models.length < 2}
                    title="Cut the chosen part with the others: keep only what they do not cover"
                    onClick={() => void booleanSelected("subtract")}
                  >
                    subtract
                  </button>
                  <button
                    type="button"
                    disabled={models.length < 2}
                    title="Fuse the chosen part and the others into one solid"
                    onClick={() => void booleanSelected("union")}
                  >
                    union
                  </button>
                  <button
                    type="button"
                    disabled={models.length < 2}
                    title="Keep only where the chosen part and the others overlap"
                    onClick={() => void booleanSelected("common")}
                  >
                    common
                  </button>
                </div>
              </>
            )}
            <div className="row measure-row">
              <button
                type="button"
                aria-pressed={measuring}
                title="Click two points on a part to measure between them"
                onClick={() => {
                  setMeasuring((current) => !current);
                  clearMeasure();
                }}
              >
                {measuring ? "measuring…" : "Measure"}
              </button>
              <button type="button" onClick={clearMeasure} disabled={measureMarks.length === 0}>
                Clear
              </button>
              <span className="measure-readout">
                {measureSpan
                  ? `${measureSpan.distance_mm.toFixed(2)} mm`
                  : measureMarks.length === 1
                    ? "click a second point"
                    : measuring
                      ? "click a point"
                      : "two points, one distance"}
              </span>
            </div>
            {measureSpan && (
              <p className="hint measure-deltas">
                {`Δx ${measureSpan.delta_mm[0].toFixed(2)}  Δy ${measureSpan.delta_mm[1].toFixed(2)}  Δz ${measureSpan.delta_mm[2].toFixed(2)} mm`}
              </p>
            )}
            {selected && (
              <p className="hint">
                {selected.source}  {selected.triangles} tris
                {health && selected.id === selectedId
                  ? `  ${(health.volumeMm3 / 1000).toFixed(2)} cm³  ${
                      health.openEdges === 0 ? "closed" : `${health.openEdges} seams`
                    }`
                  : ""}
                {selected.max_deviation_mm != null
                  ? `  ${selected.max_deviation_mm.toFixed(3)} mm off the curve`
                  : ""}
                {selected.max_turning_deg != null ? `  turn ${selected.max_turning_deg.toFixed(1)}°` : ""}
                {`  ${selected.import_ms} ms`}
              </p>
            )}
            <p className="hint">xyz is the part centre.  keys  1 move  2 rotate  3 scale  f frame</p>
          </Frame>

          </>)}

          {/* Every slice is kept, so two outcomes can be put side by side
              instead of one overwriting the other. */}
          {pane === "preview" && (
            <Frame title="Sliced">
              <JobSummary
                layerCount={layerCount}
                printSeconds={printSeconds}
                filamentMetres={filamentMetres}
                grams={grams}
                estimate={estimate}
                formatMoney={money}
                formatClock={clockTime}
              />
            </Frame>
          )}

          {pane === "preview" && (
            <Frame title="Slices">
              {history.length === 0 ? (
                <p className="hint">No slices yet</p>
              ) : (
                <div className="slice-list">
                  {history.map((record) => {
                    const changed = settingsDiff(record.settings, settingsKey);
                    return (
                      <button
                        key={record.n}
                        type="button"
                        className={viewing === record.n ? "slice-row on" : "slice-row"}
                        title={changed.length > 0 ? `changed: ${changed.join(", ")}` : "the settings on screen"}
                        onClick={() => {
                          setViewing(record.n);
                          setPaths(record.paths);
                          setLayerCount(record.layers);
                          setPrintSeconds(record.seconds);
                          setFilamentMetres(record.metres);
                          setLayer(Math.max(0, record.layers - 1));
                        }}
                      >
                        <span className="slice-name">Slice {record.n}</span>
                        <span className="slice-data">
                          {record.seconds == null ? "—" : clockTime(record.seconds)}
                          {" · "}
                          {record.metres == null ? "—" : `${record.metres.toFixed(1)} m`}
                          {" · "}
                          {record.total == null ? "—" : money(record.total)}
                        </span>
                        <span className="slice-diff">
                          {changed.length === 0 && record.plate === plateKey
                            ? "current settings"
                            : changed.length > 0
                              ? `${changed.length} changed`
                              : "plate changed"}
                        </span>
                      </button>
                    );
                  })}
                </div>
              )}
            </Frame>
          )}

          {/* Money and energy belong to a slice, not to a part: the numbers only
              mean anything once there is a result to price. */}
          {pane === "preview" && plates.length > 0 && (
            <Frame title="Plates">
              <div className="row plates">
                {plates.map((plate) => (
                  <span key={plate.index} className="plate-chip">
                    <button
                      type="button"
                      className={plate.active ? "plate-tab active" : "plate-tab"}
                      title={plate.name}
                      onClick={() => void showPlate(plate.index)}
                    >
                      <span className="plate-name">{plate.name}</span>
                      <span className="plate-count">
                        {plate.models === 0 ? "Empty" : plate.models === 1 ? "1 part" : `${plate.models} parts`}
                      </span>
                    </button>
                  </span>
                ))}
              </div>
            </Frame>
          )}
          {pane === "preview" && (<>
          <ProjectSummary
            embedded={embedded}
            ignored={ignoreEmbedded}
            onIgnore={setIgnoreEmbedded}
          />
          <Frame title="Cost">
            <label>
              Currency
              <TextField
                label="Currency"
                value={cost.currency}
                onChange={(value) => changeCost({ currency: value })}
              />
            </label>
            <label>
              Price per kg
              <TextField
                label="Price per kg"
                value={String(cost.price_per_kg)}
                onChange={(value) => {
                  const next = Number(value.replace(",", "."));
                  if (Number.isFinite(next)) changeCost({ price_per_kg: next });
                }}
              />
            </label>
            <label>
              Power
              <TextField
                label="Printer power"
                value={String(cost.watts)}
                onChange={(value) => {
                  const next = Number(value.replace(",", "."));
                  if (Number.isFinite(next)) changeCost({ watts: next });
                }}
              />
            </label>
            <label>
              Energy per kWh
              <TextField
                label="Electricity price"
                value={String(cost.electricity_per_kwh)}
                onChange={(value) => {
                  const next = Number(value.replace(",", "."));
                  if (Number.isFinite(next)) changeCost({ electricity_per_kwh: next });
                }}
              />
            </label>
            <p className="hint">
              Filament weight is the slice's metres through the material's strand. Energy is
              printer power times the estimate. both figures are only ever as good as the three
              numbers above.
            </p>
          </Frame>
          </>)}

          {pane === "prepare" && (<>
          <Frame title="Engine" advanced>
            <label>
              Backend
              <Select
                label="Backend"
                value={backend}
                options={[
                  { value: "cli", label: "CLI, log progress" },
                  { value: "arcus", label: "Arcus, live layers" },
                ]}
                onChange={(next) => {
                  setBackend(next);
                  void api.setEngine(enginePath, libDir, next).then((ok) => {
                    setStatus((current) =>
                      current ? { ...current, engine_ok: ok, engine_backend: next } : current,
                    );
                  });
                }}
              />
            </label>
            <label>
              Binary
              <TextField label="Binary" value={enginePath} onChange={setEnginePath} />
            </label>
            <label>
              Libraries
              <TextField
                label="Libraries"
                value={libDir}
                placeholder="bundled libs, if needed"
                onChange={setLibDir}
              />
            </label>
            <button type="button" onClick={() => void applyEngine()}>
              Check engine
            </button>
          </Frame>
          </>)}

        </PaneFade>
        </div>
        {(pane === "prepare" || pane === "preview") && (
          <SliceDock
            busy={busy}
            stage={busy && progress ? sliceStage(progress.message) : null}
            fraction={busy && progress ? progress.fraction : null}
            disabled={busy || models.length === 0}
            onSlice={() => void slice()}
            time={layerCount > 0 && printSeconds != null ? clockTime(printSeconds) : null}
            filament={layerCount > 0 && filamentMetres != null ? `${filamentMetres.toFixed(1)} m` : null}
            weight={
              layerCount > 0 && grams != null ? `${grams < 10 ? grams.toFixed(1) : grams.toFixed(0)} g` : null
            }
            cost={layerCount > 0 && estimate ? money(estimate.total) : null}
          />
        )}
        </aside>

        <main className="stage">
          {/* Prepare never shows a result, so when the plate or the process has
              moved on from the last slice it says so and offers the press. */}
          {(pane === "prepare" || pane === "preview") &&
            fit != null &&
            Math.max(fit.x, fit.y, fit.z) > 0.5 && (
            <p className="fit-warning" role="status">
              {fit.plate_name} extends outside the build volume (X {fit.x.toFixed(1)} mm, Y {fit.y.toFixed(1)} mm, Z {fit.z.toFixed(1)} mm past the plate).
            </p>
          )}
          <Dialog
            open={confirmExport !== null}
            onOpenChange={(open) => {
              if (!open) setConfirmExport(null);
            }}
            title="This plate extends past the bed"
            description={
              fit
                ? `${fit.plate_name} is outside the build volume by X ${fit.x.toFixed(1)} mm, Y ${fit.y.toFixed(1)} mm, Z ${fit.z.toFixed(1)} mm. Exporting writes that geometry as it is.`
                : "This plate extends past the bed."
            }
          >
            <div className="row">
              <button type="button" onClick={() => setConfirmExport(null)}>
                Stay
              </button>
              <button
                type="button"
                className="primary"
                onClick={() => {
                  const kind = confirmExport;
                  setConfirmExport(null);
                  if (kind === "gcode") void writeGcode();
                  else if (kind === "stl" || kind === "3mf") void writeGeometry(kind);
                }}
              >
                Export anyway
              </button>
            </div>
          </Dialog>
          {pane === "prepare" && stale && (
            <p className="slice-again" role="status">
              Changed since slice {lastSlice?.n}
              <button type="button" onClick={() => void slice()}>
                Slice again
              </button>
            </p>
          )}
          {/* `data-show-model` is what the viewport is actually told to draw:
              the parts list can be right and the bed still empty. */}
          <div
            className="plate"
            data-preview={preview ? "yes" : "no"}
            data-show-model={solidShown ? "yes" : "no"}
            data-models={models.length}
          >
          {/* What the plate will cost to print, the way a slicer's corner card
              reads: time, filament, weight, layers. Every figure comes from the
              slice that is on screen, and a plate that changed shows none of
              them rather than the last job's. */}
          {preview && paths.length > 0 && (
            <FeatureLegend
              paths={paths}
              colour={colourMode}
              hidden={hiddenKinds}
              onToggle={(kind) =>
                setHiddenKinds((current) => {
                  const next = new Set(current);
                  if (next.has(kind)) next.delete(kind);
                  else next.add(kind);
                  return next;
                })
              }
              onAll={() => setHiddenKinds(new Set())}
              onNone={() => setHiddenKinds(new Set(paths.map((path) => path.kind)))}
              features={features}
            />
          )}
          {preview && status?.demo === "cost" && (
            <JobSummary
              variant="demo"
              layerCount={layerCount}
              printSeconds={printSeconds}
              filamentMetres={filamentMetres}
              grams={grams}
              estimate={estimate}
              formatMoney={money}
              formatClock={clockTime}
            />
          )}
          {importing && models.length === 0 && (
            <div className="viewport viewport-skeleton import-skeleton" aria-hidden>
              <div className="viewport-skeleton__plate" />
            </div>
          )}
          <Suspense fallback={<div className="viewport viewport-skeleton" aria-hidden><div className="viewport-skeleton__plate" /></div>}>
          <Viewport
            width={bed.width}
            depth={bed.depth}
            height={bed.height}
            centerIsZero={bed.center_is_zero}
            models={models.map((model) => ({
              id: model.id,
              positions: model.positions,
              transform: model.transform,
              selected: model.id === selectedId,
            }))}
            toolpaths={paths}
            hiddenKinds={hiddenKinds}
            layer={layer}
            overlays={support?.overlays ?? []}
            showModel={solidShown}
            showToolpaths={preview}
            viewStyle={viewStyle}
            colour={colourMode}
            slopeAccent={slopeRamp.accent}
            slopeDim={slopeRamp.dim}
            mode={mode}
            frameToken={frameToken}
            cameraPreset={cameraPreset}
            cameraToken={cameraToken}
            measuring={measuring}
            marks={measureMarks}
            measureLabel={measureSpan ? `${measureSpan.distance_mm.toFixed(2)} mm` : null}
            faceArmed={faceArmed}
            onFace={(id, normal) => void placeOnFace(id, normal)}
            paintArmed={paintArmed}
            paintRadius={Math.max(0.5, Number(paintRadius) || 6)}
            onBrush={(id, points) => void paintStroke(id, points)}
            boxArmed={boxArmed}
            boxHeight={Math.max(1, Number(boxHeight) || 8)}
            supportBoxes={supportBoxes}
            onSupportBox={(min, max) => {
              void api
                .addSupportBox(min, max, paintMode === "block")
                .then((boxes) => setSupportBoxes(Array.isArray(boxes) ? boxes : []))
                .catch((failure) => setError(String(failure)));
            }}
            onMeasure={(id, point) => void addMeasurePoint(id, point)}
            onSelect={(id) => {
              // An armed pick that outlives its panel is an armed pick nobody
              // can see: the button lives in the selection's own row.
              if (id < 0) setFaceArmed(false);
              setSelectedId(id < 0 ? null : id);
            }}
            onTransform={(id, transform) => void commitTransform(id, transform)}
            cutPlane={
              selected && solidShown && !measuring
                ? {
                    axis: cutAxis === 1 ? 1 : cutAxis === 0 ? 0 : 2,
                    offset: Number(cutOffset) || 0,
                    ...worldBox(selected.positions, selected.transform),
                    onOffset: (next) => setCutOffset(String(next)),
                  }
                : null
            }
          />
          </Suspense>
          {models.length === 0 && !preview && (
            <div className="studio-empty">
              <h2>No model on this plate</h2>
              <p>Drop an STL, 3MF, or STEP file here, or import one.</p>
              <button type="button" onClick={() => void importModel()}>
                Import a model
              </button>
            </div>
          )}
          <div className="studio-float studio-float--tools" role="toolbar" aria-label="View">
            {(
              [
                ["solid", "Solid"],
                ["wire", "Wireframe"],
              ] as const
            ).map(([item, label]) => (
              <button
                key={item}
                type="button"
                aria-pressed={viewStyle === item}
                onClick={() => setViewStyle(item)}
              >
                {label}
              </button>
            ))}
            {(
              [
                ["iso", "Iso"],
                ["top", "Top"],
                ["front", "Front"],
              ] as const
            ).map(([item, label]) => (
              <button
                key={item}
                type="button"
                aria-pressed={cameraPreset === item}
                onClick={() => {
                  setCameraPreset(item);
                  setCameraToken((token) => token + 1);
                }}
              >
                {label}
              </button>
            ))}
            <button type="button" onClick={() => setFrameToken((token) => token + 1)}>
              Frame
            </button>
            {preview && (
              <>
                <button
                  type="button"
                  aria-pressed={playing}
                  onClick={() => {
                    if (!playing && layer >= layerCount - 1) setLayer(0);
                    setPlaying((on) => !on);
                  }}
                >
                  {playing ? "Pause" : "Play"}
                </button>
                <button type="button" onClick={() => { setPreview(false); setShowModel(true); }}>
                  Model
                </button>
                <button type="button" onClick={() => void addMarkAt(layer, false)}>
                  Pause here
                </button>
                <button type="button" onClick={() => void addMarkAt(layer, true)}>
                  Swap here
                </button>
                <button type="button" aria-pressed={showModel} onClick={() => setShowModel((on) => !on)}>
                  Ghost
                </button>
              </>
            )}
          </div>
          {preview && (
            <div className="studio-float studio-float--layers">
              <div className="studio-float" role="toolbar" aria-label="Toolpath colour">
                {(
                  [
                    ["type", "Feature"],
                    ["speed", "Speed"],
                    ["slope", "Slope"],
                  ] as const
                ).map(([item, label]) => (
                  <button
                    key={item}
                    type="button"
                    aria-pressed={colourMode === item}
                    onClick={() => setColourMode(item)}
                  >
                    {label}
                  </button>
                ))}
              </div>
              <div className="readout">
                {layerZ[layer] != null ? `Z ${layerZ[layer].toFixed(2)}` : `Layer ${layer + 1}`}
                {` / ${layerCount}`}
                {minutes == null ? "" : ` · ~${minutes} min`}
                {marksHere.length > 0 ? ` · ${marksHere.map((item) => item.summary).join(" / ")}` : ""}
              </div>
              <Slider
                label="Layer"
                value={[layer]}
                min={0}
                max={Math.max(0, layerCount - 1)}
                step={1}
                inputDigits={0}
                marks={marks}
                formatValue={(value) => `Layer ${value + 1}`}
                onValueChange={setLayer}
              />
            </div>
          )}
          </div>
        </main>
      </div>
      )}
        </ViewSwap>
      </ShellFrame>
      <ThemedToaster uiStyle={uiStyle} />
      <CommandPalette open={commandPaletteOpen} onOpenChange={setCommandPaletteOpen} items={commandItems} />
      <Suspense fallback={null}>
      <SetupWizard
        open={setupWizardOpen}
        printers={printers}
        printerId={printerId}
        uiStyle={uiStyle}
        onUiStyle={chooseUiStyle}
        onChoosePrinter={setPrinterId}
        onClose={() => setSetupWizardOpen(false)}
      />
      </Suspense>
    </div>
    </TooltipProvider>
  );
}

type InstructionKind = "pause" | "temperature" | "fan" | "beep" | "message" | "park_and_wait";

function ProjectSummary({
  embedded,
  ignored,
  onIgnore,
}: {
  embedded: ProjectSettingsImport | null;
  ignored: boolean;
  onIgnore: (ignored: boolean) => void;
}) {
  if (!embedded || embedded.applied.length === 0) return null;
  return (
    <details className="import-summary">
      <summary>Project settings</summary>
      <ul>
        {embedded.applied.map((setting) => (
          <li key={setting.key}>
            {setting.label} {setting.detail}
            {ignored ? " (ignored)" : ""}
          </li>
        ))}
      </ul>
      <p>{embedded.note}</p>
      <label>
        <input
          type="checkbox"
          checked={ignored}
          onChange={(event) => onIgnore(event.target.checked)}
        />
        Ignore project settings
      </label>
    </details>
  );
}

function markOf(item: StoredInstruction, layerZ: number[], layerHeight: number): number | null {
  const instruction = item.instruction;
  if ("layer" in instruction && instruction.layer != null) return instruction.layer;
  if ("height_mm" in instruction && instruction.height_mm != null) {
    const height = instruction.height_mm;
    const found = layerZ.findIndex((z) => z + 0.0001 >= height);
    if (found >= 0) return found;
    return Math.max(0, Math.ceil(height / Math.max(layerHeight, 0.05)) - 1);
  }
  return null;
}

function instructionMarks(items: StoredInstruction[], layerZ: number[], layerHeight: number): number[] {
  return items
    .map((item) => markOf(item, layerZ, layerHeight))
    .filter((value): value is number => value != null);
}

function ComparisonLines({ comparison }: { comparison: SupportComparison }) {
  const unit = comparison.times_from_engine ? "s sliced" : "s est";
  return (
    <p className="meta">
      smart {comparison.proposed_volume_mm3.toFixed(0)} mm3 {comparison.proposed_contact_mm2.toFixed(0)} mm2{" "}
      {comparison.proposed_time_seconds.toFixed(0)} {unit}
      {"\n"}
      every {comparison.everywhere_volume_mm3.toFixed(0)} mm3 {comparison.everywhere_contact_mm2.toFixed(0)} mm2{" "}
      {comparison.everywhere_time_seconds.toFixed(0)} {unit}
      {comparison.proposed_support_metres != null
        ? `\nfilament smart ${comparison.proposed_support_metres.toFixed(2)} m  every ${(comparison.everywhere_support_metres ?? 0).toFixed(2)} m`
        : ""}
    </p>
  );
}

interface Restore {
  variant: string | null;
  quality: string | null;
  material: string | null;
}

function round3(value: number): number {
  return Math.round(value * 1000) / 1000;
}

function messageOf(err: unknown): string {
  if (typeof err === "string") return err;
  if (err instanceof Error) return err.message;
  return "Something went wrong";
}

function JobSummary({
  layerCount,
  printSeconds,
  filamentMetres,
  grams,
  estimate,
  formatMoney,
  formatClock,
  variant = "panel",
}: {
  layerCount: number;
  printSeconds: number | null;
  filamentMetres: number | null;
  grams: number | null;
  estimate: CostEstimate | null;
  formatMoney: (value: number) => string;
  formatClock: (seconds: number) => string;
  variant?: "panel" | "demo";
}) {
  const className =
    variant === "demo" ? "job-card demo-cost" : "job-card job-card--panel";
  if (layerCount <= 0) {
    return <span className="job-state quiet">Not sliced</span>;
  }
  return (
    <div className={className} data-sliced="yes">
      {variant !== "panel" && <span className="job-state">Sliced</span>}
      <dl>
        <div>
          <dt>Time</dt>
          <dd>{printSeconds == null ? "—" : formatClock(printSeconds)}</dd>
        </div>
        <div>
          <dt>Filament</dt>
          <dd>{filamentMetres == null ? "—" : `${filamentMetres.toFixed(1)} m`}</dd>
        </div>
        {grams != null && (
          <div>
            <dt>Weight</dt>
            <dd>{grams < 10 ? grams.toFixed(1) : grams.toFixed(0)} g</dd>
          </div>
        )}
        <div>
          <dt>Layers</dt>
          <dd>{layerCount}</dd>
        </div>
        {estimate && (
          <>
            <div>
              <dt>Material</dt>
              <dd>{formatMoney(estimate.filament_cost)}</dd>
            </div>
            <div>
              <dt>Energy</dt>
              <dd>
                {estimate.kwh.toFixed(3)} kWh · {formatMoney(estimate.energy_cost)}
              </dd>
            </div>
            {estimate.spools.map((spool) => (
              <div key={spool.slot}>
                <dt>{`Spool ${spool.slot + 1}`}</dt>
                <dd>
                  {spool.grams.toFixed(0)} g · {formatMoney(spool.cost)}
                </dd>
              </div>
            ))}
            <div className="job-total">
              <dt>Total</dt>
              <dd>{formatMoney(estimate.total)}</dd>
            </div>
          </>
        )}
      </dl>
    </div>
  );
}
