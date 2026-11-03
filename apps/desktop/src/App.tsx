import { listen } from "@tauri-apps/api/event";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { open, save } from "@tauri-apps/plugin-dialog";
import { useEffect, useMemo, useRef, useState } from "react";
import {
  api,
  type Bed,
  type CustomInstruction,
  type MaterialInfo,
  type ModelView,
  type OpenedProject,
  type PreviewPath,
  type PrinterInfo,
  type QualityInfo,
  type RecentProject,
  type Status,
  type StoredInstruction,
  type SupportComparison,
  type SupportView,
  type Transform,
  type VariantInfo,
} from "./api";
import mark from "./brand/shale-mark-dark.svg";
import { worldCentre, translationForCentre } from "./coords";
import { andromeda } from "./field/credit";
import { StartScreen } from "./StartScreen";
import { thumbnailJpeg } from "./thumbnail";
import { Frame, meter, track } from "./theme";
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
  const [bed, setBed] = useState<Bed>(FALLBACK_BED);
  const [layerHeight, setLayerHeight] = useState(0.2);
  const [infill, setInfill] = useState(20);
  const [supports, setSupports] = useState(false);
  const [nozzle, setNozzle] = useState(200);
  const [bedTemp, setBedTemp] = useState(60);
  const [models, setModels] = useState<ModelView[]>([]);
  const [selectedId, setSelectedId] = useState<number | null>(null);
  const [mode, setMode] = useState<TransformMode>("translate");
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
  const [showModel, setShowModel] = useState(true);
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
  const [frameToken, setFrameToken] = useState(0);
  const [chord, setChord] = useState(0.1);
  const [turn, setTurn] = useState(15);
  const [chordTied, setChordTied] = useState(true);
  const [importing, setImporting] = useState(false);
  const [importProgress, setImportProgress] = useState<Progress | null>(null);

  useEffect(() => {
    void (async () => {
      try {
        const next = await api.status();
        setStatus(next);
        setEnginePath(next.engine_path);
        setLibDir(next.engine_lib_dir || "");
        setBackend(next.engine_backend || "cli");
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
        setError(messageOf(err));
      }
    })();
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

  async function importFromPath(picked: string) {
    enterSlicer();
    setError(null);
    setImporting(true);
    setImportProgress({ fraction: 0.01, message: "Reading model" });
    try {
      const model = await api.importModel(picked, printerId || null, variant, quality, chord, turn);
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
    setModels((current) => current.map((model) => (model.id === id ? { ...model, transform } : model)));
    setSupport(null);
    try {
      await api.updateTransform(id, transform);
    } catch (err) {
      setError(messageOf(err));
    }
  }

  function editNumber(axis: 0 | 1 | 2, kind: "rotation_deg" | "scale", value: number) {
    if (!selected) return;
    const transform: Transform = {
      translation: [...selected.transform.translation],
      rotation_deg: [...selected.transform.rotation_deg],
      scale: [...selected.transform.scale],
    };
    transform[kind][axis] = value;
    void commitTransform(selected.id, transform);
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

  async function dropSelected() {
    if (!selected) return;
    try {
      const transform = await api.dropModel(selected.id);
      setModels((current) => current.map((model) => (model.id === selected.id ? { ...model, transform } : model)));
    } catch (err) {
      setError(messageOf(err));
    }
  }

  async function removeSelected() {
    if (!selected) return;
    try {
      await api.removeModel(selected.id);
      setModels((current) => current.filter((model) => model.id !== selected.id));
      setSelectedId(null);
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
    setProgress({ fraction: 0.01, message: "Starting" });
    try {
      const summary = await api.slice(printerId, variant, quality, material, {
        layer_height: layerHeight,
        infill_sparse_density: infill,
        support_enable: supports,
        material_print_temperature: nozzle,
        material_bed_temperature: bedTemp,
        extra: [],
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
        extra: [],
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
        <div className="top-actions">
          <button type="button" onClick={() => setScreen("start")}>
            Start
          </button>
          <button type="button" onClick={() => void saveProject(false)}>
            Save
          </button>
          <button type="button" onClick={() => void saveProject(true)}>
            Save as
          </button>
          <button type="button" onClick={() => void openProject()}>
            Open
          </button>
          <button type="button" onClick={() => void importModel()}>
            Import model
          </button>
          <button type="button" className="primary" disabled={busy || models.length === 0} onClick={() => void slice()}>
            {busy ? "Slicing..." : "Slice"}
          </button>
          <button type="button" disabled={layerCount === 0} onClick={() => void exportGcode()}>
            Export G-code
          </button>
        </div>
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
                  </button>
                </li>
              ))}
            </ul>
            {selected && (
              <>
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
                <div className="row">
                  <button type="button" onClick={() => void dropSelected()}>
                    Drop to plate
                  </button>
                  <button type="button" onClick={() => void removeSelected()}>
                    Remove
                  </button>
                </div>
              </>
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
          <div className="plate">
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
