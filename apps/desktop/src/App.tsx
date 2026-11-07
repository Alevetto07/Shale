import { listen } from "@tauri-apps/api/event";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { open, save } from "@tauri-apps/plugin-dialog";
import { useEffect, useMemo, useRef, useState } from "react";
import {
  api,
  type Bed,
  type CalibrationApplied,
  type CustomInstruction,
  type MaterialInfo,
  type MeasureSpan,
  type ModelView,
  type OverhangPatch,
  type OpenedProject,
  type PlateSwitch,
  type PlateView,
  type PreviewPath,
  type PrinterInfo,
  type QualityInfo,
  type RecentProject,
  type Status,
  type StoredInstruction,
  type SupportComparison,
  type SupportView,
  type TowerBuilt,
  type Transform,
  type VariantInfo,
} from "./api";
import mark from "./brand/shale-mark-dark.svg";
import { worldCentre, translationForCentre } from "./coords";
import { andromeda } from "./field/credit";
import { StartScreen } from "./StartScreen";
import { DevicePane, TabBar, type DocTab, type Pane, type Spool } from "./Tabs";
import { FeatureLegend } from "./Legend";
import { thumbnailJpeg } from "./thumbnail";
import { Frame, meter, track } from "./theme";
import { MenuBar, type SliceSettings } from "./MenuBar";
import { Viewport, type TransformMode, type ViewStyle , type ColourMode } from "./Viewport";
import { AsciiCheck, AsciiField, AsciiSelect, AsciiStepper } from "./widgets";

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
        if (live) setProfileValues(values ?? {});
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
  const [bed, setBed] = useState<Bed>(FALLBACK_BED);
  const [layerHeight, setLayerHeight] = useState(0.2);
  const [infill, setInfill] = useState(20);
  const [supports, setSupports] = useState(false);
  const [walls, setWalls] = useState(2);
  const [pattern, setPattern] = useState("grid");
  const [speed, setSpeed] = useState(50);
  // Cura settings the user changed in the menu; anything absent is the profile's.
  const [overrides, setOverrides] = useState<Record<string, string>>({});
  useEffect(() => {
    overridesRef.current = overrides;
  }, [overrides]);
  const [nozzle, setNozzle] = useState(200);
  const [bedTemp, setBedTemp] = useState(60);
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
  const [faceArmed, setFaceArmed] = useState(false);
  const [paintRadius, setPaintRadius] = useState("6");
  /// The plates in this project. The one flagged `active` is the one `models`
  /// holds; the rest live in the backend until they are turned to.
  const [plates, setPlates] = useState<PlateView[]>([]);
  // Part counts are the backend's, because it holds the plates that are not in
  // front. A part arriving or leaving is the only thing that changes them.
  useEffect(() => {
    void api.listPlates().then(setPlates).catch(() => {});
  }, [models.length]);

  /// Which part the settings sheet writes to: "all" for the plate, or a part id.
  const [scope, setScope] = useState<string>("all");
  const [selectedId, setSelectedId] = useState<number | null>(null);
  const [mode, setMode] = useState<TransformMode>("translate");
  /// The cut plane: which axis it faces, how far from the part's centre, and
  /// whether to keep both halves or one.
  const [cutAxis, setCutAxis] = useState(2);
  const [cutOffset, setCutOffset] = useState("0");
  const [cutKeep, setCutKeep] = useState("both");
  /// Pins turn the two halves back into one part: a round pin on the lower
  /// half, a hole of the same size plus clearance in the upper.
  const [cutPins, setCutPins] = useState(0);
  const [pinRadius, setPinRadius] = useState("2");
  const [pinHeight, setPinHeight] = useState("5");
  /// The ruler: while measuring, a click on a part drops a point instead of
  /// selecting it. Two points make a span, the number comes from the core.
  const [measuring, setMeasuring] = useState(false);
  const [measureMarks, setMeasureMarks] = useState<Array<[number, number, number]>>([]);
  const [measureSpan, setMeasureSpan] = useState<MeasureSpan | null>(null);
  const [viewStyle, setViewStyle] = useState<ViewStyle>("solid");
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
  const [recents, setRecents] = useState<RecentProject[]>([]);
  /// Whether the recent list has been read at least once. The list is fetched
  /// before the catalogue, because it does not depend on it, but it still takes
  /// a moment on a cold start — and an empty list that means "not loaded" must
  /// not be shown as "none yet".
  const [recentsRead, setRecentsRead] = useState(false);
  // The recent list does not depend on the catalogue, and the catalogue takes
  // twenty-odd seconds to read 655 printer definitions on a cold start. Reading
  // the list immediately keeps the start screen from claiming nothing is recent.
  useEffect(() => {
    let live = true;
    api
      .recents()
      .then((listed) => {
        if (live) setRecents(listed);
      })
      .catch(() => {
        // Not worth a banner: the boot pass reads them again, and a list that
        // cannot be read is not the same as no list.
      })
      .finally(() => {
        if (live) setRecentsRead(true);
      });
    return () => {
      live = false;
    };
  }, []);
  const [notice, setNotice] = useState<string | null>(null);
  const restore = useRef<Restore | null>(null);
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
  const [layerZ, setLayerZ] = useState<number[]>([]);
  const [paths, setPaths] = useState<PreviewPath[]>([]);
  /// Kinds turned off in the legend. The toolpath stays loaded; this only
  /// decides what is drawn, so switching a kind back on is instant.
  const [hiddenKinds, setHiddenKinds] = useState<Set<string>>(new Set());
  const [printSeconds, setPrintSeconds] = useState<number | null>(null);
  const [filamentMetres, setFilamentMetres] = useState<number | null>(null);

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

  useEffect(() => {
    // Nothing was sliced, or the plate is still the one that was sliced.
    if (slicedPlate.current === null || slicedPlate.current === plateKey) return;
    slicedPlate.current = null;
    setLayerCount(0);
    setLayerZ([]);
    setPrintSeconds(null);
    setFilamentMetres(null);
  }, [plateKey]);
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
    /** The window opens before the profile library is read, so the first few
     * attempts legitimately come back empty. Ask again rather than showing an
     * error for a disk that is simply slow. */
    const boot = async () => {
      try {
        const next = await api.status();
        if (stopped) return;
        setStatus(next);
        setEnginePath(next.engine_path);
        setLibDir(next.engine_lib_dir || "");
        setBackend(next.engine_backend || "cli");
        if (!next.resources_ok && next.catalog_loading) {
          window.clearTimeout(retry);
          retry = window.setTimeout(() => void boot(), 700);
          return;
        }
        if (!next.resources_ok) {
          setError(next.resources_message);
        }
        if (next.resources_ok && !next.engine_ok && next.engine_message) {
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
          setInstructions(stored);
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
            list.find((printer) => printer.id === "creality_ender3") ??
            list.find((printer) => printer.id === "prusa_i3") ??
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
          }
          if (next.demo_model && loaded.length === 0) {
            void importFromPath(next.demo_model);
          }
          setRecents(await api.recents().catch(() => []));
        }
      } catch (err) {
        if (!stopped) setError(messageOf(err));
      }
    };
    void boot();
    const unlisten = listen("catalogue", () => {
      window.clearTimeout(retry);
      void boot();
    });
    return () => {
      stopped = true;
      window.clearTimeout(retry);
      void unlisten.then((off) => off());
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
    if (!printerId) return;
    let cancelled = false;
    void (async () => {
      try {
        const [nextVariants, nextQualities, nextMaterials] = await Promise.all([
          api.variants(printerId),
          api.qualities(printerId),
          api.materials(printerId),
        ]);
        if (cancelled) return;
        setVariants(nextVariants);
        setQualities(nextQualities);
        setMaterials(nextMaterials);
        // A remembered choice is only a choice if the machine still offers it.
        // The profile used to be restored wholesale, so a null written into
        // storage once — by a window that had not loaded the catalogue yet —
        // left the nozzle and quality reading "none" for ever while the
        // resolver quietly used the machine's own defaults.
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
            ? printer?.preferred_variant ?? nextVariants[0]?.name ?? null
            : nextVariants[0]?.name ?? null;
        const nextQuality = known(pending?.quality ?? null, (type) =>
          nextQualities.some((item) => item.quality_type === type),
        )
          ? (pending?.quality as string)
          : nextQualities.some((item) => item.quality_type === printer?.preferred_quality_type)
            ? printer?.preferred_quality_type ?? nextQualities[0]?.quality_type ?? null
            : nextQualities.find((item) => item.quality_type === "draft")?.quality_type ??
              nextQualities[0]?.quality_type ??
              null;
        const nextMaterial = known(pending?.material ?? null, (id) =>
          nextMaterials.some((item) => item.id === id),
        )
          ? (pending?.material as string)
          : nextMaterials.some((item) => item.id === printer?.preferred_material)
            ? printer?.preferred_material ?? nextMaterials[0]?.id ?? null
            : nextMaterials.find((item) => item.id.startsWith("generic_pla"))?.id ??
              nextMaterials[0]?.id ??
              null;
        setVariant(nextVariant);
        setQuality(nextQuality);
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
    if (!printerId) return;
    let cancelled = false;
    void (async () => {
      try {
        const next = await api.bed(printerId, variant, quality, material);
        if (cancelled) return;
        setBed(next);
        const pending = restore.current;
        if (pending) {
          if (pending.layerHeight !== undefined) setLayerHeight(pending.layerHeight);
          if (pending.infill !== undefined) setInfill(pending.infill);
          if (pending.supports !== undefined) setSupports(pending.supports);
          if (pending.nozzle !== undefined) setNozzle(pending.nozzle);
          if (pending.bedTemp !== undefined) setBedTemp(pending.bedTemp);
          restore.current = null;
          return;
        }
        setLayerHeight(round3(next.layer_height));
        setInfill(round3(next.infill_sparse_density));
        setSupports(next.support_enable);
        setNozzle(round3(next.nozzle_temperature));
        setBedTemp(round3(next.bed_temperature));
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
      setProgress(event.payload);
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
  const modelsRef = useRef<ModelView[]>(models);
  const lastEdit = useRef<{ key: string; at: number } | null>(null);
  const [historyDepth, setHistoryDepth] = useState({ past: 0, future: 0 });

  const syncHistory = () =>
    setHistoryDepth({ past: past.current.length, future: future.current.length });

  useEffect(() => {
    modelsRef.current = models;
  }, [models]);

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
      if (target?.closest(".ascii-select, .ascii-menu, .ascii-step, .start")) return;
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

  useEffect(() => {
    if (!preview || busy) return;
    let cancelled = false;
    void api.previewUntil(layer).then((next) => {
      if (!cancelled) setPaths(next);
    }).catch((err: unknown) => {
      if (!cancelled) setError(messageOf(err));
    });
    return () => {
      cancelled = true;
    };
  }, [preview, layer, busy]);

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

  /**
   * Everything that goes to the engine as an `extra` override: the quick fields
   * the side panel mirrors, then whatever the settings menu has been told.
   * Empty strings are dropped so an untouched field stays the profile's.
   */
  function overridesFor(): [string, string][] {
    const merged: Record<string, string> = {
      wall_line_count: String(walls),
      infill_pattern: pattern,
      speed_print: String(speed),
      ...overrides,
    };
    return Object.entries(merged).filter(([, value]) => value !== "");
  }

  const sliceSettings: SliceSettings = {
    layerHeight,
    walls,
    infill,
    pattern,
    supports,
    nozzle,
    bed: bedTemp,
    speed,
  };

  function applySliceSettings(patch: Partial<SliceSettings>) {
    if (patch.layerHeight !== undefined) setLayerHeight(patch.layerHeight);
    if (patch.walls !== undefined) setWalls(patch.walls);
    if (patch.infill !== undefined) setInfill(patch.infill);
    if (patch.pattern !== undefined) setPattern(patch.pattern);
    if (patch.supports !== undefined) setSupports(patch.supports);
    if (patch.nozzle !== undefined) setNozzle(patch.nozzle);
    if (patch.bed !== undefined) setBedTemp(patch.bed);
    if (patch.speed !== undefined) setSpeed(patch.speed);
  }

  async function importFromPath(picked: string) {
    enterSlicer();
    setError(null);
    setImporting(true);
    setImportProgress({ fraction: 0.01, message: "Reading model" });
    try {
      const added = await api.importModel(picked, printerId || null, variant, quality, chord, turn);
      remember();
      setModels((current) => [...current, ...added]);
      const last = added[added.length - 1];
      setSelectedId(last.id);
      setPreview(false);
      setShowModel(true);
      const triangles = added.reduce((total, model) => total + model.triangles, 0);
      setImportProgress({
        fraction: 1,
        message: added.length > 1 ? `${added.length} parts, ${triangles} triangles` : `${triangles} triangles`,
      });
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
      const halves = await api.cut(
        selected.id,
        cutAxis,
        Number(cutOffset) || 0,
        cutKeep,
        cutPins,
        Number(pinRadius) || 2,
        Number(pinHeight) || 5,
      );
      remember();
      setModels((current) => [...current.filter((model) => model.id !== selected.id), ...halves]);
      setSelectedId(halves[0]?.id ?? null);
      setPreview(false);
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

  async function slice() {
    if (!printerId) {
      setError("Choose a printer first.");
      return;
    }
    setError(null);
    setBusy(true);
    setCalibration(null);
    setProgress({ fraction: 0.01, message: "Starting" });
    try {
      const summary = await api.slice(printerId, variant, quality, material, ams, {
        layer_height: layerHeight,
        infill_sparse_density: infill,
        support_enable: supports,
        material_print_temperature: nozzle,
        material_bed_temperature: bedTemp,
        // The menu bar edits these directly; an empty list used to mean the
        // selected profile decided and there was no way to change your mind.
        extra: overridesFor(),
      });
      setLayerCount(summary.layer_count);
      slicedPlate.current = plateKey;
      setLayerZ(summary.layers.map((item) => item.z));
      setLayer(Math.max(0, summary.layer_count - 1));
      setPrintSeconds(summary.print_time_seconds);
      setFilamentMetres(summary.filament_metres);
      setPreview(true);
      setShowModel(false);
      // The toolpaths arrive at the camera the model was left under; frame
      // them, the way pressing f would.
      setFrameToken((token) => token + 1);
      setProgress({ fraction: 1, message: `${summary.layer_count} layers` });
    } catch (err) {
      setError(messageOf(err));
      setProgress(null);
    } finally {
      setBusy(false);
    }
  }

  async function exportGcode() {
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
    try {
      const updated = await api.setModelSettings(scopedPart.id, next);
      setModels((current) => current.map((model) => (model.id === updated.id ? updated : model)));
    } catch (err) {
      setError(messageOf(err));
    }
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

  // A part that gets deleted while its settings are on screen hands the sheet
  // back to the whole plate.
  useEffect(() => {
    if (scope !== "all" && !models.some((model) => model.id === Number(scope))) {
      setScope("all");
    }
  }, [models, scope]);

  async function exportGeometry(format: "stl" | "3mf") {
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
      setPatches(await api.overhangPatches(id));
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
  /// back should take back the stroke.
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
      setPlates(turned.plates);
      setSelectedId(null);
      setScope("all");
      setPreview(false);
      // The model is drawn only while `showModel` is on, and the slice that was
      // on screen belonged to the plate we just left: its layer count and its
      // toolpaths have to go with it.
      setShowModel(turned.models.length > 0);
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
    try {
      const left = await api.deletePlate(index);
      const turned = left.find((plate) => plate.active);
      if (turned) {
        const swapped = await api.switchPlate(turned.index);
        setModels(swapped.models);
        setPlates(swapped.plates);
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
    } catch (failure) {
      setError(String(failure));
    }
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
        layer_height: layerHeight,
        infill_sparse_density: infill,
        support_enable: true,
        material_print_temperature: nozzle,
        material_bed_temperature: bedTemp,
        // The menu bar edits these directly; an empty list used to mean the
        // selected profile decided and there was no way to change your mind.
        extra: overridesFor(),
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
    return {
      layer_height: layerHeight,
      infill_sparse_density: infill,
      support_enable: supports,
      material_print_temperature: nozzle,
      material_bed_temperature: bedTemp,
      extra: [] as [string, string][],
    };
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

  function materialName(): string {
    return materialLabel(material);
  }

  /// The bays as the machine numbers them: T1 is the plate's own material, and
  /// T2..T4 are the AMS. An empty string is an empty bay, not a blank label.
  function spoolsForDevice(): Spool[] {
    const bays: Spool[] = [{ bay: 1, material: materialLabel(material) }];
    ams.forEach((slot, index) => bays.push({ bay: index + 2, material: materialLabel(slot || null) }));
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
      const listed = await api.saveProject({
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
      });
      // A quiet save is a place to put a plate down, not a file to adopt: the
      // tab keeps its name and its (empty) path, and the project does not start
      // claiming to live in the temp folder.
      if (adopt) {
        setProjectPath(path);
        setRecents(listed);
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
          setRecents((current) => current);
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
      layerHeight: opened.settings.layer_height ?? layerHeight,
      infill: opened.settings.infill_sparse_density ?? infill,
      supports: opened.settings.support_enable ?? supports,
      nozzle: opened.settings.material_print_temperature ?? nozzle,
      bedTemp: opened.settings.material_bed_temperature ?? bedTemp,
    };
    setProjectPath(opened.path);
    setProjectName(opened.name || "Untitled");
    setPrinterId(opened.printer_id);
    setVariant(opened.variant_name);
    setQuality(opened.quality_type);
    setMaterial(opened.material_id);
    setAms([0, 1, 2].map((index) => opened.ams?.[index] ?? ""));
    if (opened.settings.layer_height != null) setLayerHeight(opened.settings.layer_height);
    if (opened.settings.infill_sparse_density != null) setInfill(opened.settings.infill_sparse_density);
    if (opened.settings.support_enable != null) setSupports(opened.settings.support_enable);
    if (opened.settings.material_print_temperature != null) setNozzle(opened.settings.material_print_temperature);
    if (opened.settings.material_bed_temperature != null) setBedTemp(opened.settings.material_bed_temperature);
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
      setRecents(await api.recents().catch(() => []));
    } catch (err) {
      setError(messageOf(err));
      setRecents(await api.recents().catch(() => []));
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
  const clockTime = (seconds: number) =>
    `${Math.floor(seconds / 3600)} h ${String(Math.round((seconds % 3600) / 60)).padStart(2, "0")} m`;
  const marks = instructionMarks(instructions, layerZ, layerHeight);
  const marksHere = instructions.filter((item) => markOf(item, layerZ, layerHeight) === layer);

  return (
    <div className="app">
      <header className="topbar">
        <div className="brand">
          <img className="brand-mark" src={mark} alt="" />
          <p className="wordmark">Shale</p>
          {projectPath && <p className="project-name">{projectName}</p>}
        </div>
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
          onPrinter={setPrinterId}
          variants={variants}
          variant={variant}
          onVariant={setVariant}
          qualities={qualities}
          quality={quality}
          onQuality={setQuality}
          materials={materials}
          material={material}
          onMaterial={setMaterial}
          settings={sliceSettings}
          onSettings={applySliceSettings}
          overrides={scope === "all" ? overrides : (models.find((m) => m.id === Number(scope))?.settings ?? {})}
          profileValues={profileValues}
          onOverride={(key, value) => void setScopedOverride(key, value)}
          onResetOverrides={() => void resetScopedOverrides()}
          scope={scope}
          parts={models.map((model) => ({ id: model.id, name: model.name }))}
          onScope={setScope}
          onSlice={() => void slice()}
        />
      </header>

      {status && !status.resources_ok && (
        <p className="banner error" role="status">
          {status.resources_message}
        </p>
      )}
      {status && status.resources_ok && !status.engine_ok && (
        <p className="banner error" role="status">
          {status.engine_message || `CuraEngine was not found at ${status.engine_path}.`}
        </p>
      )}
      {importing && (
        <p className="banner" role="status">
          import {(importProgress ? importProgress.fraction * 100 : 0).toFixed(0)}%{" "}
          {importProgress?.message ?? "reading"}{" "}
          <button type="button" onClick={() => void cancelImport()}>
            [ cancel ]
          </button>
        </p>
      )}
      {error && (
        <p className="banner error" role="alert">
          {error}
        </p>
      )}
      {notice && (
        <p className="banner" role="status">
          {notice}
        </p>
      )}

      <TabBar
        pane={pane}
        onPane={(next) => {
          if (next === "home") {
            setPane("home");
            setScreen("start");
            return;
          }
          if (next === "preview" && layerCount === 0) {
            setNotice("nothing to preview yet — press slice first");
            return;
          }
          // A notice belongs to the workspace that raised it; carrying it into
          // the next one is how a stale sentence ends up under a different page.
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
        }}
        tabs={docTabs}
        activeTab={activeTab}
        onTab={(id) => void selectTab(id)}
        onCloseTab={(id) => void closeTab(id)}
        onNewTab={() => void newProject()}
        onOpenTab={() => void openInNewTab()}
        printer={printerLabel()}
        sliced={layerCount > 0}
      />

      {pane === "home" ? (
        <div className="home-pane">
          <StartScreen
            live={!busy && !importing}
            canReturn={workspace}
            recents={recents} recentsRead={recentsRead}
            printers={printers}
            printerId={printerId}
            enginePath={enginePath}
            libDir={libDir}
            backend={backend}
            onReturn={() => {
              setScreen("slicer");
              setPane("prepare");
            }}
            onNew={() => void newProject()}
            onOpen={() => void openProject()}
            onImport={() => void importModel()}
            onOpenRecent={(path) => void openProject(path)}
            onChoosePrinter={setPrinterId}
            onEnginePath={setEnginePath}
            onLibDir={setLibDir}
            onBackend={setBackend}
            onApplyEngine={() => void applyEngine()}
            onQuit={() => void quit()}
          />
        </div>
      ) : pane === "device" ? (
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
        />
      ) : (
      <div className="workspace">
        <aside className="panel">
          <Frame title="plates">
            <div className="row plates">
              {plates.map((plate) => (
                <span key={plate.index} className="plate-chip">
                  <button
                    type="button"
                    className={plate.active ? "plate-tab active" : "plate-tab"}
                    title={plate.name}
                    onClick={() => void showPlate(plate.index)}
                  >
                    <span className="plate-name">Plate {plate.index + 1}</span>
                    <span className="plate-count">
                      {plate.models === 0
                        ? "empty"
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
              <button type="button" title="add a plate" onClick={() => void newPlate()}>
                [ + ]
              </button>
            </div>
            <p className="hint">
              each plate holds its own parts and is sliced on its own. a project saves every plate with it.
            </p>
          </Frame>

          <Frame title="objects">
            {models.length === 0 ? (
              <p className="hint">nothing on this plate yet. import a model and it is listed here.</p>
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
                  </li>
                ))}
              </ul>
            )}
          </Frame>

          <Frame title="paint support">
            {paintTarget === null ? (
              <p className="hint">import a part and its undersides can be painted here.</p>
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
                      ? `${patches.length} patches of underside`
                      : "nothing underneath to hold up"}
                </p>
                {/* Two brushes, because they ask for opposite things. A patch is
                    just triangles; what you press decides whether support grows
                    under them or is kept out. */}
                <div className="row paint-brush" role="group" aria-label="brush">
                  {(
                    [
                      ["enforce", "hold up"],
                      ["block", "keep out"],
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
                <div className="row paint-brush" role="group" aria-label="brush reach">
                  <button
                    type="button"
                    className={`paint-brush-button${paintArmed ? " on" : ""}`}
                    aria-pressed={paintArmed}
                    title="Drag over the part to paint the surface under the cursor"
                    onClick={() => setPaintArmed((armed) => !armed)}
                  >
                    brush
                  </button>
                  <label className="paint-radius">
                    <span>reach</span>
                    <input
                      aria-label="Brush reach in millimetres"
                      type="number"
                      min={1}
                      max={40}
                      step={1}
                      value={paintRadius}
                      onChange={(event) => setPaintRadius(event.target.value)}
                    />
                    <span>mm</span>
                  </label>
                </div>
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
                      [ {paintMode === "block" ? "keep out" : "hold up"} {Math.round(patch.area_mm2)} mm² ]
                    </button>
                  ))}
                  {(painted > 0 || blocked > 0) && (
                    <>
                      <button type="button" className="paint-back" onClick={() => void takeBackPaint()}>
                        [ take back ]
                      </button>
                      <button type="button" className="paint-clear" onClick={() => void unpaint()}>
                        [ clear ]
                      </button>
                    </>
                  )}
                </div>
                <p className="hint">
                  a painted face is held up by a column to the bed, and painting turns the plate's own
                  support off: what you paint is the whole of it.
                </p>
              </>
            )}
          </Frame>

          <Frame title="printer">
            <label>
              filter
              <AsciiField label="filter" value={printerQuery} placeholder="name…" onChange={setPrinterQuery} />
            </label>
            <label>
              machine
              <AsciiSelect
                label="machine"
                value={printerId}
                options={filteredPrinters.map((printer) => ({
                  value: printer.id,
                  label: `${printer.manufacturer ? `${printer.manufacturer} ` : ""}${printer.name}`,
                }))}
                onChange={setPrinterId}
              />
            </label>
            <label>
              nozzle
              <AsciiSelect
                label="nozzle"
                value={variant ?? ""}
                options={variants.map((item) => ({ value: item.name, label: item.name }))}
                onChange={(next) => setVariant(next || null)}
              />
            </label>
            <label>
              quality
              <AsciiSelect
                label="quality"
                value={quality ?? ""}
                options={qualities.map((item) => ({ value: item.quality_type, label: item.name }))}
                onChange={(next) => setQuality(next || null)}
              />
            </label>
            <label>
              material
              <AsciiSelect
                label="material"
                value={material ?? ""}
                options={spools.map((item) => ({ value: item.id, label: item.label }))}
                onChange={(next) => setMaterial(next || null)}
              />
            </label>
            <p className="meta">
              bed {bed.width.toFixed(0)} x {bed.depth.toFixed(0)} x {bed.height.toFixed(0)} mm
              {"\n"}
              nozzle {bed.nozzle_size.toFixed(2)}  line {bed.line_width.toFixed(2)}  {bed.adhesion_type}  {bed.speed_print.toFixed(0)} mm/s
              {bed.warning_count > 0 ? `\n${bed.warning_count} resolver warnings` : ""}
            </p>
          </Frame>

          <Frame title="ams">
            <p className="meta">
              {ams.filter(Boolean).length === 0
                ? "one spool: the plate material"
                : `plate plus ${ams.filter(Boolean).length} spool${ams.filter(Boolean).length === 1 ? "" : "s"}`}
            </p>
            {[0, 1, 2].map((index) => (
              <label key={index}>
                spool {index + 2}
                <AsciiSelect
                  label={`spool ${index + 2}`}
                  value={ams[index] ?? ""}
                  options={[
                    { value: "", label: "empty" },
                    ...materials.map((item) => ({ value: item.id, label: item.label })),
                  ]}
                  onChange={(next) =>
                    setAms((list) => list.map((item, position) => (position === index ? next : item)))
                  }
                />
              </label>
            ))}
            <p className="hint">
              the plate material is spool 1. give a part a spool in the model frame and the slice gets a tool change,
              its own hotend temperature and its own filament.
            </p>
          </Frame>

          <Frame title="calibration">
            <p className="meta">
              {tower
                ? `${tower.bands.length} bands, ${tower.bands[0].value} to ${
                    tower.bands[tower.bands.length - 1].value
                  } ${tower.unit}, ${tower.band_height_mm} mm each — ${tower.kind}`
                : "a five-band temperature tower, 5 °C apart"}
            </p>
            <div className="row">
              <button type="button" disabled={calibrating} onClick={() => void addCalibration("temperature")}>
                [ temperature tower ]
              </button>
              <button type="button" disabled={calibrating} onClick={() => void addCalibration("flow")}>
                [ flow coupon ]
              </button>
              <button type="button" disabled={calibrating} onClick={() => void addCalibration("pressure")}>
                [ pressure tower ]
              </button>
              <button type="button" disabled={!tower || calibrating} onClick={() => void stepCalibration()}>
                [ step it ]
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
              the tower alternates 20 mm and 26 mm blocks, so every band overhangs the one below by 3 mm; the flow
              coupon is a 30 mm single-wall tube, because flow is measured with calipers, not judged. slice it, then
              step it: one M104, M221 or M900 per band, written into the slice, and the preview stays as it was.
            </p>
          </Frame>

          <Frame title="slice">
            <label>
              layer mm
              <AsciiStepper label="layer mm" value={layerHeight} min={0.05} max={0.8} step={0.01} digits={2} onChange={setLayerHeight} />
            </label>
            <label>
              infill %
              <AsciiStepper label="infill %" value={infill} min={0} max={100} step={1} digits={0} onChange={setInfill} />
            </label>
            <AsciiCheck checked={supports} onChange={setSupports}>
              supports
            </AsciiCheck>
            <label>
              hotend C
              <AsciiStepper label="hotend C" value={nozzle} min={0} max={400} step={1} digits={0} onChange={setNozzle} />
            </label>
            <label>
              bed C
              <AsciiStepper label="bed C" value={bedTemp} min={0} max={150} step={1} digits={0} onChange={setBedTemp} />
            </label>
            <p className="hint">layer height also sets the first layer. changing quality reloads these values.</p>
            <label>
              chord mm
              <AsciiStepper
                label="chord mm"
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
              <AsciiStepper
                label="turn"
                value={turn}
                min={2}
                max={45}
                step={1}
                digits={0}
                onChange={setTurn}
              />
            </label>
            <button type="button" onClick={() => setChordTied(true)}>
              [ tie chord to layer ]
            </button>
            <p className="hint">
              step files are tessellated here. chord is half the layer height, and no coarser than half the nozzle.
              turn is the largest bend between two segments of a curve. stl and 3mf are already meshes.
            </p>
          </Frame>

          <Frame title="supports">
            <label>
              angle
              <AsciiStepper label="angle" value={supportAngle} min={0} max={90} step={1} digits={0} onChange={setSupportAngle} />
            </label>
            <label>
              bridge mm
              <AsciiStepper label="bridge mm" value={bridgeMm} min={0} max={80} step={1} digits={0} onChange={setBridgeMm} />
            </label>
            <div className="row">
              <button type="button" disabled={selectedId == null || supportBusy} onClick={() => void proposeSupports()}>
                [ propose ]
              </button>
              <button type="button" disabled={!support || supportBusy} onClick={() => void compareSupports()}>
                [ compare ]
              </button>
            </div>
            {support && <ComparisonLines comparison={support.plan.comparison} />}
            {support && support.plan.regions.length === 0 && (
              <p className="hint">no overhang past the angle.</p>
            )}
            <ul className="instructions">
              {support?.plan.regions.map((region) => (
                <li key={region.id}>
                  <AsciiCheck checked={region.enabled} onChange={() => void toggleSupport(region.id)}>
                    {`${region.role === "block" ? "block" : "support"}  ${region.summary}`}
                  </AsciiCheck>
                </li>
              ))}
            </ul>
            <p className="hint">
              grey columns are support meshes. wire boxes block bridges and cosmetic slopes. the engine is not modified.
            </p>
          </Frame>

          <Frame title="instructions">
            <label>
              kind
              <AsciiSelect
                label="kind"
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
                <AsciiSelect
                  label="by"
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
              <AsciiStepper
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
                <AsciiStepper label="percent" value={fanPercent} min={0} max={100} step={1} digits={0} onChange={setFanPercent} />
              </label>
            )}
            {kind === "temperature" && (
              <>
                <label>
                  hotend C
                  <AsciiStepper label="hotend C" value={tempNozzle} min={0} max={400} step={1} digits={0} onChange={setTempNozzle} />
                </label>
                <label>
                  bed C
                  <AsciiStepper label="bed C" value={tempBed} min={0} max={150} step={1} digits={0} onChange={setTempBed} />
                </label>
                <AsciiCheck checked={tempWait} onChange={setTempWait}>
                  wait until reached
                </AsciiCheck>
              </>
            )}
            {kind === "pause" && (
              <AsciiCheck checked={filament} onChange={setFilament}>
                filament change
              </AsciiCheck>
            )}
            {(kind === "pause" || kind === "message" || kind === "park_and_wait") && (
              <label>
                note
                <AsciiField label="note" value={note} onChange={setNote} />
              </label>
            )}
            <button type="button" onClick={() => void addInstruction()}>
              [ add ]
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

          <Frame title="model">
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
                  <AsciiSelect
                    label="spool"
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
                        <AsciiStepper
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
                  <button type="button" title="Stand the part on the face that needs least support" onClick={() => void autoOrient()}>
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
                  <button type="button" title="Split the part with a plane through its centre" onClick={() => void cutSelected()}>
                    Cut
                  </button>
                </div>
                <div className="row cut-pins">
                  <label className="cut-pin-toggle">
                    <input
                      type="checkbox"
                      checked={cutPins > 0}
                      onChange={(event) => setCutPins(event.target.checked ? 2 : 0)}
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
                {selected.max_deviation_mm != null
                  ? `  ${selected.max_deviation_mm.toFixed(3)} mm off the curve`
                  : ""}
                {selected.max_turning_deg != null ? `  turn ${selected.max_turning_deg.toFixed(1)}°` : ""}
                {`  ${selected.import_ms} ms`}
              </p>
            )}
            <p className="hint">xyz is the part centre.  keys  1 move  2 rotate  3 scale  f frame</p>
          </Frame>

          <Frame title="engine">
            <label>
              backend
              <AsciiSelect
                label="backend"
                value={backend}
                options={[
                  { value: "cli", label: "cli  log progress" },
                  { value: "arcus", label: "arcus  live layers" },
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
              binary
              <AsciiField label="binary" value={enginePath} onChange={setEnginePath} />
            </label>
            <label>
              libs
              <AsciiField
                label="libs"
                value={libDir}
                placeholder="bundled libs, if needed"
                onChange={setLibDir}
              />
            </label>
            <button type="button" onClick={() => void applyEngine()}>
              [ check engine ]
            </button>
          </Frame>

          <Frame title="credits">
            <p className="credit">
              {andromeda.name} ({andromeda.object}), {andromeda.credit}. Released {andromeda.release}.{" "}
              {andromeda.licence}.
            </p>
            <p className="credit">
              <a href={andromeda.page}>{andromeda.page}</a>
            </p>
          </Frame>
        </aside>

        <main className="stage">
          {/* `data-show-model` is what the viewport is actually told to draw:
              the parts list can be right and the bed still empty. */}
          <div className="plate" data-show-model={solidShown ? "yes" : "no"} data-models={models.length}>
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
            />
          )}
          <div className="job-card" data-sliced={layerCount > 0 ? "yes" : "no"}>
            {layerCount > 0 ? (
              <>
                <span className="job-state">sliced</span>
                <dl>
                  <div>
                    <dt>time</dt>
                    <dd>{printSeconds == null ? "—" : clockTime(printSeconds)}</dd>
                  </div>
                  <div>
                    <dt>filament</dt>
                    <dd>{filamentMetres == null ? "—" : `${filamentMetres.toFixed(1)} m`}</dd>
                  </div>
                  {grams != null && (
                    <div>
                      <dt>weight</dt>
                      <dd>{grams < 10 ? grams.toFixed(1) : grams.toFixed(0)} g</dd>
                    </div>
                  )}
                  <div>
                    <dt>layers</dt>
                    <dd>{layerCount}</dd>
                  </div>
                </dl>
              </>
            ) : (
              <span className="job-state quiet">not sliced</span>
            )}
          </div>
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
            toolpaths={hiddenKinds.size === 0 ? paths : paths.filter((path) => !hiddenKinds.has(path.kind))}
            overlays={support?.overlays ?? []}
            showModel={solidShown}
            showToolpaths={preview}
            viewStyle={viewStyle}
            colour={colourMode}
            mode={mode}
            frameToken={frameToken}
            measuring={measuring}
            marks={measureMarks}
            measureLabel={measureSpan ? `${measureSpan.distance_mm.toFixed(2)} mm` : null}
            faceArmed={faceArmed}
            onFace={(id, normal) => void placeOnFace(id, normal)}
            paintArmed={paintArmed}
            paintRadius={Math.max(0.5, Number(paintRadius) || 6)}
            onBrush={(id, points) => void paintStroke(id, points)}
            onMeasure={(id, point) => void addMeasurePoint(id, point)}
            onSelect={(id) => {
              // An armed pick that outlives its panel is an armed pick nobody
              // can see: the button lives in the selection's own row.
              if (id < 0) setFaceArmed(false);
              setSelectedId(id < 0 ? null : id);
            }}
            onTransform={(id, transform) => void commitTransform(id, transform)}
          />
          {models.length === 0 && !preview && (
            <p className="caption">no model on the plate. import an stl, 3mf, or step</p>
          )}
          </div>
          <footer className="scrubber">
            <span className="scrub-group">
              <em>view</em>
              {""}
              {(["solid", "wire", "ascii"] as const).map((item) => (
                <button
                  key={item}
                  type="button"
                  className={viewStyle === item ? "selected" : ""}
                  onClick={() => setViewStyle(item)}
                >
                  {item}
                </button>
              ))}
            </span>
            {preview && (
            <span className="scrub-group">
              <em>colour</em>
              {""}
              {(["type", "speed"] as const).map((item) => (
                <button
                  key={item}
                  type="button"
                  className={colourMode === item ? "selected" : ""}
                  onClick={() => setColourMode(item)}
                >
                  {item}
                </button>
              ))}
            </span>
            )}
            {preview ? (
              <>
                <span className="scrub-group">
                  <em>preview</em>
                  {""}
                <button
                  type="button"
                  className={playing ? "selected" : ""}
                  onClick={() => {
                    // At the top of a finished preview, play means watch it
                    // again, not press a button that does nothing.
                    if (!playing && layer >= layerCount - 1) setLayer(0);
                    setPlaying((on) => !on);
                  }}
                >
                  {playing ? "pause" : "play"}
                </button>
                <button type="button" onClick={() => { setPreview(false); setShowModel(true); }}>
                  model
                </button>
                <button type="button" onClick={() => void addMarkAt(layer, false)}>
                  pause here
                </button>
                <button type="button" onClick={() => void addMarkAt(layer, true)}>
                  swap here
                </button>
                <AsciiCheck checked={showModel} onChange={setShowModel}>
                  ghost
                </AsciiCheck>
                </span>
                <div className="ascii-slider">
                  <div className="readout">
                    layer <span className="active">{String(layer + 1).padStart(3, " ")}</span> / {layerCount}
                    {layerZ[layer] != null ? `   z ${layerZ[layer].toFixed(2)}` : ""}
                    {minutes == null ? "" : `   ~${minutes} min`}
                    {marksHere.length > 0 ? `   + ${marksHere.map((item) => item.summary).join(" / ")}` : ""}
                    {"   [ ]"}
                  </div>
                  <div className="glyphs" aria-hidden="true">
                    {Array.from(track(layer, Math.max(0, layerCount - 1), 28, marks)).map((glyph, index) =>
                      glyph === "●" ? (
                        <span key={index} className="active">
                          {glyph}
                        </span>
                      ) : (
                        glyph
                      ),
                    )}
                  </div>
                  <input
                    type="range"
                    min={0}
                    max={Math.max(0, layerCount - 1)}
                    value={layer}
                    aria-label="Layer"
                    onChange={(event) => setLayer(Number(event.target.value))}
                  />
                </div>
              </>
            ) : (
              <p className="hint">drag to orbit. scroll to zoom. z is up.  1 move  2 rotate  3 scale  f frame</p>
            )}
            {progress && (
              <p className="meter" aria-live="polite">
                {meter(progress.fraction)} {String(Math.round(progress.fraction * 100)).padStart(3, " ")}%  {progress.message}
              </p>
            )}
          </footer>
        </main>
      </div>
      )}
    </div>
  );
}

type InstructionKind = "pause" | "temperature" | "fan" | "beep" | "message" | "park_and_wait";

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
  // A project carries its numbers, so opening one restores all of them. The
  // machine remembered across a restart carries only the choice of machine,
  // quality and material — the numbers are the profile's, not a project's.
  layerHeight?: number;
  infill?: number;
  supports?: boolean;
  nozzle?: number;
  bedTemp?: number;
}

function round3(value: number): number {
  return Math.round(value * 1000) / 1000;
}

function messageOf(err: unknown): string {
  if (typeof err === "string") return err;
  if (err instanceof Error) return err.message;
  return "Something went wrong";
}
