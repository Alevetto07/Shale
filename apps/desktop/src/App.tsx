import { listen } from "@tauri-apps/api/event";
import { open, save } from "@tauri-apps/plugin-dialog";
import { useEffect, useMemo, useState } from "react";
import {
  api,
  type Bed,
  type MaterialInfo,
  type ModelView,
  type PreviewPath,
  type PrinterInfo,
  type QualityInfo,
  type Status,
  type Transform,
  type VariantInfo,
} from "./api";
import mark from "./brand/shale-mark-dark.svg";
import { Frame, meter, theme, track } from "./theme";
import { Viewport, type TransformMode, type ViewStyle } from "./Viewport";

interface Progress {
  fraction: number;
  message: string;
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

  useEffect(() => {
    void (async () => {
      try {
        const next = await api.status();
        setStatus(next);
        setEnginePath(next.engine_path);
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
          if (existing && existing.layer_count > 0) {
            setLayerCount(existing.layer_count);
            setLayerZ(existing.layers.map((item) => item.z));
            setLayer(Math.max(0, Math.floor(existing.layer_count / 2)));
            setPreview(true);
            setShowModel(false);
          }
          const preferred =
            list.find((printer) => printer.id === "creality_ender3") ??
            list.find((printer) => printer.id === "prusa_i3") ??
            list[0];
          if (preferred) setPrinterId(preferred.id);
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
    const unlisten = listen<Progress>("slice-progress", (event) => {
      setProgress(event.payload);
    });
    return () => {
      void unlisten.then((stop) => stop());
    };
  }, []);

  useEffect(() => {
    function onKey(event: KeyboardEvent) {
      const tag = (event.target as HTMLElement | null)?.tagName;
      if (tag === "INPUT" || tag === "SELECT" || tag === "TEXTAREA") return;
      if (event.key === "1") setMode("translate");
      if (event.key === "2") setMode("rotate");
      if (event.key === "3") setMode("scale");
      if (event.key === "[") setLayer((current) => Math.max(0, current - 1));
      if (event.key === "]") setLayer((current) => Math.min(Math.max(0, layerCount - 1), current + 1));
    }
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [layerCount]);

  useEffect(() => {
    if (!preview) return;
    let cancelled = false;
    void api.previewUntil(layer).then((next) => {
      if (!cancelled) setPaths(next);
    }).catch((err: unknown) => {
      if (!cancelled) setError(messageOf(err));
    });
    return () => {
      cancelled = true;
    };
  }, [preview, layer]);

  const filteredPrinters = useMemo(() => {
    const query = printerQuery.trim().toLowerCase();
    if (!query) return printers;
    return printers.filter((printer) =>
      `${printer.manufacturer} ${printer.name} ${printer.id}`.toLowerCase().includes(query),
    );
  }, [printerQuery, printers]);

  const selected = models.find((model) => model.id === selectedId) ?? null;

  async function importModel() {
    setError(null);
    const picked = await open({
      multiple: false,
      filters: [{ name: "Models", extensions: ["stl", "STL", "3mf", "3MF"] }],
    });
    if (typeof picked !== "string") return;
    try {
      const model = await api.importModel(picked, printerId || null, variant, quality);
      setModels((current) => [...current, model]);
      setSelectedId(model.id);
      setPreview(false);
      setShowModel(true);
    } catch (err) {
      setError(messageOf(err));
    }
  }

  async function commitTransform(id: number, transform: Transform) {
    setModels((current) => current.map((model) => (model.id === id ? { ...model, transform } : model)));
    try {
      await api.updateTransform(id, transform);
    } catch (err) {
      setError(messageOf(err));
    }
  }

  function editNumber(axis: 0 | 1 | 2, kind: "translation" | "rotation_deg" | "scale", value: number) {
    if (!selected) return;
    const transform: Transform = {
      translation: [...selected.transform.translation],
      rotation_deg: [...selected.transform.rotation_deg],
      scale: [...selected.transform.scale],
    };
    transform[kind][axis] = value;
    void commitTransform(selected.id, transform);
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

  async function applyEngine() {
    try {
      const ok = await api.setEngine(enginePath, libDir);
      setStatus((current) => (current ? { ...current, engine_ok: ok, engine_path: enginePath } : current));
      if (!ok) setError("CuraEngine did not respond to `help` at that path.");
    } catch (err) {
      setError(messageOf(err));
    }
  }

  const minutes = printSeconds == null ? null : Math.max(1, Math.round(printSeconds / 60));

  return (
    <div className="app">
      <header className="topbar">
        <div className="brand">
          <img className="brand-mark" src={mark} alt="" />
          <p className="wordmark">Shale</p>
        </div>
        <div className="top-actions">
          <button type="button" onClick={() => void importModel()}>
            Import model
          </button>
          <button type="button" className="primary" disabled={busy || models.length === 0} onClick={() => void slice()}>
            {busy ? "Slicing…" : "Slice"}
          </button>
          <button type="button" disabled={layerCount === 0} onClick={() => void exportGcode()}>
            Export G-code
          </button>
        </div>
      </header>

      {status && !status.resources_ok && (
        <p className="banner" role="status">
          {status.resources_message} Run <code>scripts/fetch-resources.sh</code> from the repository, then restart Shale.
        </p>
      )}
      {status && status.resources_ok && !status.engine_ok && (
        <p className="banner" role="status">
          CuraEngine was not found at <code>{status.engine_path}</code>. Set the path under Engine, or export{" "}
          <code>SHALE_CURAENGINE</code>. Slicing stays disabled until it answers.
        </p>
      )}
      {error && (
        <p className="banner error" role="alert">
          {error}
        </p>
      )}

      <div className="workspace">
        <aside className="panel">
          <Frame title="printer">
            <label>
              filter
              <input
                value={printerQuery}
                onChange={(event) => setPrinterQuery(event.target.value)}
                placeholder="Ender, Prusa…"
              />
            </label>
            <label>
              machine
              <select value={printerId} onChange={(event) => setPrinterId(event.target.value)}>
                {filteredPrinters.map((printer) => (
                  <option key={printer.id} value={printer.id}>
                    {printer.manufacturer ? `${printer.manufacturer} ` : ""}
                    {printer.name}
                  </option>
                ))}
              </select>
            </label>
            <label>
              nozzle
              <select value={variant ?? ""} onChange={(event) => setVariant(event.target.value || null)}>
                {variants.map((item) => (
                  <option key={item.name} value={item.name}>
                    {item.name}
                  </option>
                ))}
              </select>
            </label>
            <label>
              quality
              <select value={quality ?? ""} onChange={(event) => setQuality(event.target.value || null)}>
                {qualities.map((item) => (
                  <option key={item.quality_type} value={item.quality_type}>
                    {item.name}
                  </option>
                ))}
              </select>
            </label>
            <label>
              material
              <select value={material ?? ""} onChange={(event) => setMaterial(event.target.value || null)}>
                {materials.map((item) => (
                  <option key={item.id} value={item.id}>
                    {item.label}
                  </option>
                ))}
              </select>
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
              <input
                type="number"
                min={0.05}
                max={0.8}
                step={0.01}
                value={layerHeight}
                onChange={(event) => setLayerHeight(Number(event.target.value))}
              />
            </label>
            <label>
              infill %
              <input
                type="number"
                min={0}
                max={100}
                step={1}
                value={infill}
                onChange={(event) => setInfill(Number(event.target.value))}
              />
            </label>
            <label className="check">
              <input type="checkbox" checked={supports} onChange={(event) => setSupports(event.target.checked)} />
              supports
            </label>
            <label>
              hotend C
              <input
                type="number"
                min={0}
                max={400}
                step={1}
                value={nozzle}
                onChange={(event) => setNozzle(Number(event.target.value))}
              />
            </label>
            <label>
              bed C
              <input
                type="number"
                min={0}
                max={150}
                step={1}
                value={bedTemp}
                onChange={(event) => setBedTemp(Number(event.target.value))}
              />
            </label>
            <p className="hint">layer height also sets the first layer. changing quality reloads these values.</p>
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
                  {(["X", "Y", "Z"] as const).map((axis, index) => (
                    <label key={axis}>
                      {mode === "rotate" ? `${axis}°` : axis}
                      <input
                        type="number"
                        step={mode === "scale" ? 0.01 : 1}
                        value={round3(
                          mode === "translate"
                            ? selected.transform.translation[index]
                            : mode === "rotate"
                              ? selected.transform.rotation_deg[index]
                              : selected.transform.scale[index],
                        )}
                        onChange={(event) =>
                          editNumber(
                            index as 0 | 1 | 2,
                            mode === "translate" ? "translation" : mode === "rotate" ? "rotation_deg" : "scale",
                            Number(event.target.value),
                          )
                        }
                      />
                    </label>
                  ))}
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
            <p className="hint">keys  1 move   2 rotate   3 scale</p>
          </Frame>

          <Frame title="engine">
            <label>
              binary
              <input value={enginePath} onChange={(event) => setEnginePath(event.target.value)} />
            </label>
            <label>
              libs
              <input
                value={libDir}
                placeholder="Only if the binary needs bundled libs"
                onChange={(event) => setLibDir(event.target.value)}
              />
            </label>
            <button type="button" onClick={() => void applyEngine()}>
              [ check engine ]
            </button>
          </Frame>
        </aside>

        <main className="stage">
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
            showModel={showModel}
            showToolpaths={preview}
            viewStyle={viewStyle}
            mode={mode}
            onSelect={(id) => setSelectedId(id < 0 ? null : id)}
            onTransform={(id, transform) => void commitTransform(id, transform)}
          />
          {models.length === 0 && !preview && (
            <pre className="empty">
              <span className="stone">{theme.stone}</span>
              <span className="caption">{"\nno model on the plate\nimport an stl or 3mf"}</span>
            </pre>
          )}
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
                <label className="check">
                  <input type="checkbox" checked={showModel} onChange={(event) => setShowModel(event.target.checked)} />
                  ghost
                </label>
                <div className="ascii-slider">
                  <div className="readout">
                    layer {String(layer + 1).padStart(3, " ")} / {layerCount}
                    {layerZ[layer] != null ? `   z ${layerZ[layer].toFixed(2)}` : ""}
                    {minutes == null ? "" : `   ~${minutes} min`}
                    {"   [ ]"}
                  </div>
                  <div className="glyphs" aria-hidden="true">
                    {track(layer, Math.max(0, layerCount - 1))}
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
              <p className="hint">drag to orbit. scroll to zoom. z is up.  1 move  2 rotate  3 scale</p>
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

function round3(value: number): number {
  return Math.round(value * 1000) / 1000;
}

function messageOf(err: unknown): string {
  if (typeof err === "string") return err;
  if (err instanceof Error) return err.message;
  return "Something went wrong";
}
