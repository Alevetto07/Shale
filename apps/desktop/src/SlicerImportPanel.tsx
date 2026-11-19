import { useCallback, useEffect, useMemo, useState } from "react";
import {
  detectSlicerInstalls,
  importSlicerPresets,
  listSlicerPresets,
  type PresetOffer,
  type SlicerImportReport,
  type SlicerInstall,
} from "./slicer-import";

export function SlicerImportPanel({
  defaultSlicerId,
  compact,
  onImported,
}: {
  defaultSlicerId?: string;
  compact?: boolean;
  onImported?: (report: SlicerImportReport) => void;
}) {
  const [installs, setInstalls] = useState<SlicerInstall[] | null>(null);
  const [slicerId, setSlicerId] = useState(defaultSlicerId ?? "bambu");
  const [offers, setOffers] = useState<PresetOffer[] | null>(null);
  const [selected, setSelected] = useState<Set<string>>(() => new Set());
  const [busy, setBusy] = useState(false);
  const [report, setReport] = useState<SlicerImportReport | null>(null);
  const [error, setError] = useState<string | null>(null);

  const loadInstalls = useCallback(async () => {
    try {
      const rows = await detectSlicerInstalls();
      setInstalls(rows);
      if (!defaultSlicerId) {
        const first = rows.find((row) => row.found);
        if (first) setSlicerId(first.id);
      }
    } catch (err) {
      setError(String(err));
    }
  }, [defaultSlicerId]);

  const loadOffers = useCallback(async () => {
    setBusy(true);
    setError(null);
    try {
      const rows = await listSlicerPresets(slicerId);
      setOffers(rows);
      setSelected(new Set(rows.filter((row) => row.supported).map((row) => row.path)));
    } catch (err) {
      setError(String(err));
      setOffers(null);
    } finally {
      setBusy(false);
    }
  }, [slicerId]);

  useEffect(() => {
    void loadInstalls();
  }, [loadInstalls]);

  useEffect(() => {
    if (slicerId) void loadOffers();
  }, [slicerId, loadOffers]);

  const supportedOffers = useMemo(
    () => (offers ?? []).filter((row) => row.supported),
    [offers],
  );

  async function runImport() {
    if (selected.size === 0) return;
    setBusy(true);
    setError(null);
    try {
      const result = await importSlicerPresets(slicerId, Array.from(selected));
      setReport(result);
      onImported?.(result);
      await loadOffers();
    } catch (err) {
      setError(String(err));
    } finally {
      setBusy(false);
    }
  }

  function toggle(path: string, on: boolean) {
    setSelected((current) => {
      const next = new Set(current);
      if (on) next.add(path);
      else next.delete(path);
      return next;
    });
  }

  return (
    <div className={compact ? "slicer-import slicer-import--compact" : "slicer-import"}>
      <label>
        Source slicer
        <select value={slicerId} onChange={(event) => setSlicerId(event.target.value)}>
          {(installs ?? []).map((row) => (
            <option key={row.id} value={row.id}>
              {row.label}
              {row.found ? "" : " (not found)"}
            </option>
          ))}
        </select>
      </label>
      <div className="slicer-import-actions">
        <button type="button" disabled={busy} onClick={() => void loadOffers()}>
          Refresh list
        </button>
        <button
          type="button"
          className="primary"
          disabled={busy || selected.size === 0}
          onClick={() => void runImport()}
        >
          Import selected ({selected.size})
        </button>
      </div>
      {error ? <p className="hint error">{error}</p> : null}
      {offers && (
        <ul className="slicer-import-list">
          {offers.map((row) => (
            <li key={row.id} className={row.supported ? undefined : "dim"}>
              <label>
                <input
                  type="checkbox"
                  disabled={!row.supported}
                  checked={selected.has(row.path)}
                  onChange={(event) => toggle(row.path, event.target.checked)}
                />
                <span>
                  <strong>{row.name}</strong> <span className="dim">({row.kind})</span>
                </span>
              </label>
              {!row.supported && row.note ? <p className="hint">{row.note}</p> : null}
            </li>
          ))}
          {offers.length === 0 ? <li className="dim">No user presets found in this install.</li> : null}
        </ul>
      )}
      {supportedOffers.length > 0 ? (
        <p className="hint">
          {supportedOffers.length} supported preset{supportedOffers.length === 1 ? "" : "s"}.
          Prusa/Cura process settings become Shale named presets; Bambu/Orca JSON is stored for the
          catalogue.
        </p>
      ) : null}
      {report && (
        <div className="slicer-import-report">
          <p>
            Imported {report.imported}, skipped {report.skipped}, unsupported {report.unsupported}
          </p>
          <ul>
            {report.lines.map((line) => (
              <li key={`${line.path}-${line.outcome}`}>
                <strong>{line.name}</strong> — {line.outcome}
                {line.detail ? `: ${line.detail}` : null}
              </li>
            ))}
          </ul>
        </div>
      )}
    </div>
  );
}