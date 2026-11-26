import { useState, useSyncExternalStore } from "react";
import {
  currentImportJob,
  resetImportJob,
  startSettingsCheck,
  startSettingsImport,
  subscribeImportJob,
} from "./slicer-import";
import { Button } from "./ui";

const SOURCES = [
  { id: "bambu", label: "Bambu Studio" },
  { id: "orca", label: "OrcaSlicer" },
  { id: "prusa", label: "PrusaSlicer" },
  { id: "cura", label: "UltiMaker Cura" },
] as const;

export function SlicerImportPanel({
  onImported,
  compact = false,
  defaultSlicerId = "bambu",
}: {
  onImported?: () => void;
  /** Kept so the wizard and Settings share this panel. */
  compact?: boolean;
  defaultSlicerId?: string;
}) {
  const job = useSyncExternalStore(subscribeImportJob, currentImportJob, currentImportJob);
  const [slicerId, setSlicerId] = useState(defaultSlicerId);
  const running = job.phase === "scanning" || job.phase === "importing";
  const summary = job.summary;
  const total = job.counts.total;
  const seen = job.counts.seen;
  const notes = summary?.warning_notes ?? job.report?.lines.map((line) => line.detail).filter(Boolean) ?? [];

  return (
    <div className={compact ? "slicer-import slicer-import--compact" : "slicer-import"}>
      <p className="slicer-import-note">
        Nothing is read until you choose a slicer and press Check for settings. You can leave this step while a check runs.
      </p>
      <label className="slicer-import-source">
        <span>Source</span>
        <select
          aria-label="Source"
          value={slicerId}
          disabled={running}
          onChange={(event) => setSlicerId(event.target.value)}
        >
          {SOURCES.map((source) => (
            <option key={source.id} value={source.id}>
              {source.label}
            </option>
          ))}
        </select>
      </label>
      <div className="slicer-import-actions">
        <Button type="button" variant="secondary" disabled={running} onClick={() => startSettingsCheck(slicerId)}>
          Check for settings
        </Button>
        {summary && summary.found && job.phase !== "done" ? (
          <Button type="button" variant="primary" disabled={running || summary.paths.length === 0} onClick={() => startSettingsImport(onImported)}>
            Import these settings
          </Button>
        ) : null}
        {job.phase !== "idle" && !running ? (
          <Button type="button" variant="ghost" onClick={() => resetImportJob()}>
            Check another
          </Button>
        ) : null}
      </div>

      {running ? (
        <div className="slicer-import-progress" aria-live="polite">
          <progress value={seen} max={Math.max(total, 1)} />
          <p>
            {total > 0 ? `${seen} of ${total} files` : "Looking through the settings folder…"}
          </p>
          <p className="dim">
            Printers {job.counts.printers} · Filaments {job.counts.filaments} · Processes {job.counts.processes}
          </p>
        </div>
      ) : null}

      {job.error ? <p className="slicer-import-error">{job.error}</p> : null}

      {summary && summary.found && !running ? (
        <div className="slicer-import-summary">
          <p>
            {summary.label}: {summary.printers} printers, {summary.filaments} filaments, {summary.processes} processes
            {summary.warnings > 0 ? `, ${summary.warnings} warnings` : ""}.
          </p>
          {job.report ? (
            <p>
              Imported {job.report.imported}
              {job.report.skipped > 0 ? `, skipped ${job.report.skipped}` : ""}.
            </p>
          ) : (
            <p className="dim">The list stays here. Continue whenever you like.</p>
          )}
          {notes.length > 0 ? (
            <details>
              <summary>Show details</summary>
              <ul>
                {notes.slice(0, 12).map((note) => (
                  <li key={note}>{note}</li>
                ))}
              </ul>
            </details>
          ) : null}
        </div>
      ) : null}
    </div>
  );
}
