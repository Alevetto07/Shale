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
import { thumbnailJpeg } from "./thumbnail";
import { Frame, meter, track } from "./theme";
import { MenuBar, type SliceSettings } from "./MenuBar";
import { Viewport, type TransformMode, type ViewStyle } from "./Viewport";
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
  warning_count: 0,
};

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
  const [material, setMaterial] = useState<string | null>(null);
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
  const [nozzle, setNozzle] = useState(200);
  const [bedTemp, setBedTemp] = useState(60);
  const [models, setModels] = useState<ModelView[]>([]);
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
  /// The ruler: while measuring, a click on a part drops a point instead of
  /// selecting it. Two points make a span, the number comes from the core.
  const [measuring, setMeasuring] = useState(false);
  const [measureMarks, setMeasureMarks] = useState<Array<[number, number, number]>>([]);
  const [measureSpan, setMeasureSpan] = useState<MeasureSpan | null>(null);
  const [viewStyle, setViewStyle] = useState<ViewStyle>("solid");
  const [busy, setBusy] = useState(false);
  const [screen, setScreen] = useState<"start" | "slicer">("start");
  const [workspace, setWorkspace] = useState(false);
  const [projectPath, setProjectPath] = useState<string | null>(null);
  const [projectName, setProjectName] = useState("Untitled");
  const [recents, setRecents] = useState<RecentProject[]>([]);
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
  const [layerCount, setLayerCount] = useState(0);
  const [layerZ, setLayerZ] = useState<number[]>([]);
  const [paths, setPaths] = useState<PreviewPath[]>([]);
  const [printSeconds, setPrintSeconds] = useState<number | null>(null);
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
          if (existing && existing.layer_count > 0) {
            const zs = existing.layers.map((item) => item.z);
            setLayerCount(existing.layer_count);
            setLayerZ(zs);
            const marked = instructionMarks(stored, zs, 0.2);
            const focus = marked.find((index) => index >= 0 && index < existing.layer_count);
            setLayer(focus ?? Math.max(0, Math.floor(existing.layer_count / 2)));
            setPreview(true);
            setShowModel(false);
          }
          const preferred =
            list.find((printer) => printer.id === "creality_ender3") ??
            list.find((printer) => printer.id === "prusa_i3") ??
            list[0];
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
        const pending = restore.current;
        if (pending) {
          setVariant(pending.variant);
          setQuality(pending.quality);
          setMaterial(pending.material);
          return;
        }
        const printer = printers.find((item) => item.id === printerId);
        const nextVariant = nextVariants.some((item) => item.name === printer?.preferred_variant)
          ? printer?.preferred_variant ?? nextVariants[0]?.name ?? null
          : nextVariants[0]?.name ?? null;
        const nextQuality = nextQualities.some((item) => item.quality_type === printer?.preferred_quality_type)
          ? printer?.preferred_quality_type ?? nextQualities[0]?.quality_type ?? null
          : nextQualities.find((item) => item.quality_type === "draft")?.quality_type ??
            nextQualities[0]?.quality_type ??
            null;
        const nextMaterial = nextMaterials.some((item) => item.id === printer?.preferred_material)
          ? printer?.preferred_material ?? nextMaterials[0]?.id ?? null
          : nextMaterials.find((item) => item.id.startsWith("generic_pla"))?.id ?? nextMaterials[0]?.id ?? null;
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
          setLayerHeight(pending.layerHeight);
          setInfill(pending.infill);
          setSupports(pending.supports);
          setNozzle(pending.nozzle);
          setBedTemp(pending.bedTemp);
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
  const past = useRef<{ models: ModelView[]; selected: number | null }[]>([]);
  const future = useRef<{ models: ModelView[]; selected: number | null }[]>([]);
  const modelsRef = useRef<ModelView[]>(models);
  const lastEdit = useRef<{ key: string; at: number } | null>(null);
  const [historyDepth, setHistoryDepth] = useState({ past: 0, future: 0 });

  const syncHistory = () =>
    setHistoryDepth({ past: past.current.length, future: future.current.length });

  useEffect(() => {
    modelsRef.current = models;
  }, [models]);

  /** Remember the plate as it is now, before it changes. */
  function remember(key = "") {
    const now = Date.now();
    if (key && lastEdit.current && lastEdit.current.key === key && now - lastEdit.current.at < 1200) {
      lastEdit.current.at = now;
      return;
    }
    lastEdit.current = key ? { key, at: now } : null;
    past.current.push({ models: modelsRef.current, selected: selectedId });
    if (past.current.length > 80) past.current.shift();
    future.current = [];
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
    const snapshot = past.current.pop();
    if (!snapshot) return;
    future.current.push({ models: modelsRef.current, selected: selectedId });
    syncHistory();
    await restoreSnapshot(snapshot);
  }

  async function redo() {
    const snapshot = future.current.pop();
    if (!snapshot) return;
    past.current.push({ models: modelsRef.current, selected: selectedId });
    syncHistory();
    await restoreSnapshot(snapshot);
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
      if (event.key === "Escape") {
        setMeasuring(false);
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
  }, [layerCount, screen]);

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
      const model = await api.importModel(picked, printerId || null, variant, quality, chord, turn);
      remember();
      setModels((current) => [...current, model]);
      setSelectedId(model.id);
      setPreview(false);
      setShowModel(true);
      setImportProgress({ fraction: 1, message: `${model.triangles} triangles` });
    } catch (err) {
      const message = messageOf(err);
      // Only a deliberate cancel stays quiet; any other failure is shown.
      const cancelled = message.trim().toLowerCase() === "import cancelled";
      if (!cancelled) setError(message);
      // The slicer is entered before the read starts so progress has somewhere
      // to appear. On failure with nothing else loaded, go back rather than
      // leaving an empty slicer on screen.
      if (models.length === 0) setScreen("start");
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
      const halves = await api.cut(selected.id, cutAxis, Number(cutOffset) || 0, cutKeep);
      remember();
      setModels((current) => [...current.filter((model) => model.id !== selected.id), ...halves]);
      setSelectedId(halves[0]?.id ?? null);
      setPreview(false);
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
      setLayerZ(summary.layers.map((item) => item.z));
      setLayer(Math.max(0, summary.layer_count - 1));
      setPrintSeconds(summary.print_time_seconds);
      setPreview(true);
      setShowModel(false);
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

  async function setScopedOverride(key: string, value: string) {
    if (!scopedPart) {
      setOverrides((prev) => {
        const next = { ...prev };
        if (value === "") delete next[key];
        else next[key] = value;
        return next;
      });
      return;
    }
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

  async function removeInstruction(id: number) {
    try {
      setInstructions(await api.removeInstruction(id));
    } catch (err) {
      setError(messageOf(err));
    }
  }

  /** Put a temperature tower on the plate. Shale builds the geometry itself,
   * so the engine only ever sees an ordinary STL. */
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
      setPrintSeconds(null);
      setTower(null);
      setCalibration(null);
      setMeasureSpan(null);
      setMeasureMarks([]);
    } catch (failure) {
      setError(String(failure));
    }
  }

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
      setPrintSeconds(null);
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
      setPrintSeconds(null);
      setTower(null);
      setCalibration(null);
    } catch (failure) {
      setError(String(failure));
    }
  }

  async function addTemperatureTower() {
    setError(null);
    setCalibrating(true);
    try {
      const built = await api.calibrationTower(printerId || null);
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
   * temperature. Nothing else in the file moves. */
  async function stepHotend() {
    if (!tower) return;
    setError(null);
    setCalibrating(true);
    try {
      setCalibration(await api.calibrateSlice(tower.bands));
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
    const fileName = path.split(/[/\\]/).pop()?.replace(/\.shale$/i, "") || name;
    try {
      const listed = await api.saveProject({
        path,
        name: fileName,
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
      });
      setProjectPath(path);
      setProjectName(fileName);
      setRecents(listed);
      setNotice(`Saved ${fileName}.shale`);
    } catch (err) {
      setError(messageOf(err));
    }
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
    setPrintSeconds(null);
    setProgress(null);
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
    try {
      const opened = await api.openProject(picked);
      applyOpened(opened);
      setRecents(await api.recents().catch(() => []));
    } catch (err) {
      setError(messageOf(err));
      setRecents(await api.recents().catch(() => []));
    }
  }

  async function newProject() {
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
    setPrintSeconds(null);
    setProgress(null);
    setProjectPath(null);
    setProjectName("Untitled");
    enterSlicer();
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

  const minutes = printSeconds == null ? null : Math.max(1, Math.round(printSeconds / 60));
  const marks = instructionMarks(instructions, layerZ, layerHeight);
  const marksHere = instructions.filter((item) => markOf(item, layerZ, layerHeight) === layer);

  if (screen === "start") {
    return (
      <div className="app">
        <StartScreen
          live={!busy && !importing}
          canReturn={workspace}
          recents={recents}
          printers={printers}
          printerId={printerId}
          enginePath={enginePath}
          libDir={libDir}
          backend={backend}
          onReturn={() => setScreen("slicer")}
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
        {error && (
          <p className="banner error floating" role="alert">
            {error}
          </p>
        )}
        {notice && (
          <p className="banner floating" role="status">
            {notice}
          </p>
        )}
      </div>
    );
  }

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
          onStart={() => setScreen("start")}
          onNew={() => void newProject()}
          onOpen={() => void openProject()}
          onImport={() => void importModel()}
          onSave={() => void saveProject(false)}
          onSaveAs={() => void saveProject(true)}
          onExport={() => void exportGcode()}
          onExportGeometry={(format) => void exportGeometry(format)}
          onQuit={() => void quit()}
          onDuplicate={() => void duplicateSelected()}
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
          showModel={showModel}
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
                    {plate.index + 1} {plate.name} ({plate.models})
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

          <Frame title="printer">
            <label>
              filter
              <AsciiField label="filter" value={printerQuery} placeholder="Ender, Prusa…" onChange={setPrinterQuery} />
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
                options={materials.map((item) => ({ value: item.id, label: item.label }))}
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
                ? `${tower.bands.length} bands, ${tower.bands[0].temperature} to ${
                    tower.bands[tower.bands.length - 1].temperature
                  } °C, ${tower.band_height_mm} mm each`
                : "a five-band temperature tower, 5 °C apart"}
            </p>
            <div className="row">
              <button type="button" disabled={calibrating} onClick={() => void addTemperatureTower()}>
                [ temperature tower ]
              </button>
              <button type="button" disabled={!tower || calibrating} onClick={() => void stepHotend()}>
                [ step the hotend ]
              </button>
            </div>
            {calibration && (
              <p className="meta">
                {calibration.injections.length} steps:{" "}
                {calibration.injections
                  .map((step) => `L${step.layer} ${step.temperature}°C`)
                  .join(", ")}
              </p>
            )}
            <p className="hint">
              the tower alternates 20 mm and 26 mm blocks, so every band overhangs the one below by 3 mm. slice it,
              then step the hotend: one M104 per band, written into the slice, and the preview stays as it was.
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
          <div className="plate" data-show-model={showModel ? "yes" : "no"} data-models={models.length}>
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
            overlays={support?.overlays ?? []}
            showModel={showModel}
            showToolpaths={preview}
            viewStyle={viewStyle}
            mode={mode}
            frameToken={frameToken}
            measuring={measuring}
            marks={measureMarks}
            measureLabel={measureSpan ? `${measureSpan.distance_mm.toFixed(2)} mm` : null}
            onMeasure={(id, point) => void addMeasurePoint(id, point)}
            onSelect={(id) => setSelectedId(id < 0 ? null : id)}
            onTransform={(id, transform) => void commitTransform(id, transform)}
          />
          {models.length === 0 && !preview && (
            <p className="caption">no model on the plate. import an stl, 3mf, or step</p>
          )}
          </div>
          <footer className="scrubber">
            <span>
              view{" "}
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
            {preview ? (
              <>
                <button type="button" onClick={() => { setPreview(false); setShowModel(true); }}>
                  model
                </button>
                <AsciiCheck checked={showModel} onChange={setShowModel}>
                  ghost
                </AsciiCheck>
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
  layerHeight: number;
  infill: number;
  supports: boolean;
  nozzle: number;
  bedTemp: number;
}

function round3(value: number): number {
  return Math.round(value * 1000) / 1000;
}

function messageOf(err: unknown): string {
  if (typeof err === "string") return err;
  if (err instanceof Error) return err.message;
  return "Something went wrong";
}
