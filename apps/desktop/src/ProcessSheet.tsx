import { useEffect, useMemo, useState } from "react";
import { SETTING_GROUPS, type SettingField } from "./settings";
import { A1_PROCESS_PRESETS } from "./bambu-presets";
import {
  ADVANCED,
  BAMBU_LABEL,
  GROUP_SECTION,
  NO_BAMBU_EQUIVALENT,
  SECTIONS,
  describe,
  type Section,
} from "./settings-meta";

/// The four simple values live as their own state because the slice command
/// takes them; everything else is a Cura override. The sheet edits both, so a
/// person never has to know which is which.
export interface ProcessSettings {
  layerHeight: number;
  walls: number;
  infill: number;
  pattern: string;
  supports: boolean;
  nozzle: number;
  bed: number;
  speed: number;
}

interface Props {
  settings: ProcessSettings;
  onSettings: (patch: Partial<ProcessSettings>) => void;
  overrides: Record<string, string>;
  profileValues?: Record<string, string>;
  onOverride: (key: string, value: string) => void;
  onResetOverrides: () => void;
}

/// The keys the simple rows already own; showing them twice would let the two
/// controls disagree.
const BASE_KEYS = new Set([
  "wall_line_count",
  "infill_sparse_density",
  "infill_pattern",
  "support_enable",
  "speed_print",
]);

const VISIBLE_KEY = "shale.settings.hidden";

function loadHidden(): Set<string> {
  try {
    const raw = window.localStorage.getItem(VISIBLE_KEY);
    if (!raw) return new Set();
    const parsed = JSON.parse(raw) as unknown;
    return Array.isArray(parsed) ? new Set(parsed.filter((v): v is string => typeof v === "string")) : new Set();
  } catch {
    return new Set();
  }
}

function num(value: string | undefined, fallback: number): number {
  const n = Number(value);
  return Number.isFinite(n) ? n : fallback;
}

export function ProcessSheet(props: Props) {
  const [section, setSection] = useState<Section>("Quality");
  const [advanced, setAdvanced] = useState(false);
  const [picking, setPicking] = useState(false);
  const [hidden, setHidden] = useState<Set<string>>(() => loadHidden());
  const [search, setSearch] = useState("");

  useEffect(() => {
    try {
      window.localStorage.setItem(VISIBLE_KEY, JSON.stringify(Array.from(hidden)));
    } catch {
      // A browser that refuses storage still gets a working panel.
    }
  }, [hidden]);

  /// The rows of a section, in group order, with the ones Bambu would hide
  /// behind its advanced switch left out unless it is on.
  const rows = useMemo(() => {
    const out: { group: string; field: SettingField }[] = [];
    for (const group of SETTING_GROUPS) {
      if (GROUP_SECTION[group.id] !== section) continue;
      for (const field of group.fields) {
        if (BASE_KEYS.has(field.key)) continue;
        if (NO_BAMBU_EQUIVALENT.has(field.key)) continue;
        if (hidden.has(field.key)) continue;
        if (!advanced && ADVANCED.has(field.key)) continue;
        out.push({ group: group.label, field });
      }
    }
    return out;
  }, [section, advanced, hidden]);

  const hits = useMemo(() => {
    const needle = search.trim().toLowerCase();
    if (!needle) return [];
    const out: { group: string; field: SettingField }[] = [];
    for (const group of SETTING_GROUPS) {
      for (const field of group.fields) {
        const label = BAMBU_LABEL[field.key] ?? field.label;
        if (!label.toLowerCase().includes(needle)) continue;
        if (NO_BAMBU_EQUIVALENT.has(field.key)) continue;
        out.push({ group: group.label, field });
      }
    }
    return out;
  }, [search]);

  const presetName = useMemo(() => {
    const match = A1_PROCESS_PRESETS.find((preset) => {
      const v = preset.values;
      return (
        Math.abs(num(v.layer_height, -1) - props.settings.layerHeight) < 1e-6 &&
        num(v.wall_line_count, -1) === props.settings.walls &&
        num(v.infill_sparse_density, -1) === props.settings.infill
      );
    });
    return match?.name ?? null;
  }, [props.settings]);

  const applyPreset = (name: string) => {
    const preset = A1_PROCESS_PRESETS.find((entry) => entry.name === name);
    if (!preset) return;
    const v = preset.values;
    props.onSettings({
      layerHeight: num(v.layer_height, props.settings.layerHeight),
      walls: num(v.wall_line_count, props.settings.walls),
      infill: num(v.infill_sparse_density, props.settings.infill),
      pattern: v.infill_pattern ?? props.settings.pattern,
      supports: v.support_enable === "1",
    });
    for (const [key, value] of Object.entries(v)) {
      if (BASE_KEYS.has(key)) continue;
      props.onOverride(key, value);
    }
  };

  const label = (field: SettingField) => BAMBU_LABEL[field.key] ?? field.label;

  const control = (field: SettingField) => {
    const value = props.overrides[field.key] ?? "";
    if (field.kind === "toggle") {
      return (
        <select value={value} onChange={(event) => props.onOverride(field.key, event.target.value)}>
          <option value="">profile</option>
          <option value="1">on</option>
          <option value="0">off</option>
        </select>
      );
    }
    if (field.kind === "choice") {
      return (
        <select value={value} onChange={(event) => props.onOverride(field.key, event.target.value)}>
          <option value="">profile</option>
          {(field.options ?? []).map((option) => (
            <option key={option} value={option}>
              {option}
            </option>
          ))}
        </select>
      );
    }
    return (
      <span className="setting-number">
        <input
          type="number"
          value={value}
          min={field.min}
          max={field.max}
          step={field.step ?? 1}
          placeholder={props.profileValues?.[field.key] ?? "profile"}
          onChange={(event) => props.onOverride(field.key, event.target.value)}
        />
        {field.unit && <em>{field.unit}</em>}
      </span>
    );
  };

  const row = (field: SettingField) => (
    <div className="menu-row setting-row" key={field.key} data-changed={field.key in props.overrides ? "yes" : "no"}>
      <span className="setting-name">
        {label(field)}
        {/* The description is behind the question mark: a wall of grey prose
            under every control is not a settings panel, it is a manual. */}
        <span className="setting-q" title={describe(field.key, label(field))} aria-label={describe(field.key, label(field))}>
          ?
        </span>
      </span>
      {control(field)}
    </div>
  );

  return (
    <div className="process-sheet">
      <div className="process-head">
        <div className="process-tabs">
          {SECTIONS.map((entry) => (
            <button
              key={entry}
              type="button"
              className={entry === section ? "selected" : ""}
              onClick={() => setSection(entry)}
            >
              {entry}
            </button>
          ))}
        </div>
        <label className="process-advanced">
          <input type="checkbox" checked={advanced} onChange={(event) => setAdvanced(event.target.checked)} />
          <em>advanced</em>
        </label>
      </div>

      <div className="process-presets">
        <span className="process-presets-label">A1 process</span>
        {A1_PROCESS_PRESETS.map((preset) => (
          <button
            key={preset.name}
            type="button"
            className={preset.name === presetName ? "process-preset on" : "process-preset"}
            onClick={() => applyPreset(preset.name)}
            title={preset.name}
          >
            {preset.name.replace(" @BBL A1", "")}
          </button>
        ))}
      </div>

      <div className="process-tools">
        <label className="process-search">
          <span className="visually-hidden">search settings</span>
          <input
            type="search"
            value={search}
            placeholder="search settings"
            onChange={(event) => setSearch(event.target.value)}
          />
        </label>
        <button type="button" className="process-visibility" onClick={() => setPicking((v) => !v)}>
          {picking ? "done" : `settings shown ${hidden.size > 0 ? `(${hidden.size} hidden)` : ""}`}
        </button>
      </div>

      {picking ? (
        <div className="settings-body process-picker">
          <p className="hint">Unticked settings are hidden from the panel. Nothing is deleted.</p>
          {SETTING_GROUPS.map((group) => {
            const fields = group.fields.filter((field) => !NO_BAMBU_EQUIVALENT.has(field.key));
            if (fields.length === 0) return null;
            return (
              <div key={group.id} className="picker-group">
                <h4 className="settings-heading">{group.label}</h4>
                {fields.map((field) => (
                  <label key={field.key} className="picker-row">
                    <input
                      type="checkbox"
                      checked={!hidden.has(field.key)}
                      onChange={(event) => {
                        setHidden((current) => {
                          const next = new Set(current);
                          if (event.target.checked) next.delete(field.key);
                          else next.add(field.key);
                          return next;
                        });
                      }}
                    />
                    <span>{label(field)}</span>
                  </label>
                ))}
              </div>
            );
          })}
        </div>
      ) : search ? (
        <div className="settings-body">
          <p className="hint">
            {hits.length} {hits.length === 1 ? "setting" : "settings"}
          </p>
          {hits.map((hit) => row(hit.field))}
        </div>
      ) : (
        <div className="settings-body">
          {section === "Quality" && (
            <div className="menu-row setting-row">
              <span className="setting-name">
                Layer height
                <span className="setting-q" title="Height of every layer after the first.">?</span>
              </span>
              <span className="setting-number">
                <input
                  type="number"
                  value={props.settings.layerHeight}
                  min={0.02}
                  max={1}
                  step={0.02}
                  onChange={(event) => props.onSettings({ layerHeight: Number(event.target.value) })}
                />
                <em>mm</em>
              </span>
            </div>
          )}
          {section === "Strength" && (
            <>
              <div className="menu-row setting-row">
                <span className="setting-name">
                  Wall loops
                  <span className="setting-q" title="How many perimeters are printed around each layer.">?</span>
                </span>
                <span className="setting-number">
                  <input
                    type="number"
                    value={props.settings.walls}
                    min={1}
                    max={12}
                    step={1}
                    onChange={(event) => props.onSettings({ walls: Number(event.target.value) })}
                  />
                </span>
              </div>
              <div className="menu-row setting-row">
                <span className="setting-name">
                  Sparse infill density
                  <span className="setting-q" title="How much material is laid inside the part.">?</span>
                </span>
                <span className="setting-number">
                  <input
                    type="number"
                    value={props.settings.infill}
                    min={0}
                    max={100}
                    step={5}
                    onChange={(event) => props.onSettings({ infill: Number(event.target.value) })}
                  />
                  <em>%</em>
                </span>
              </div>
              <div className="menu-row setting-row">
                <span className="setting-name">
                  Sparse infill pattern
                  <span className="setting-q" title="The shape the sparse infill is drawn in.">?</span>
                </span>
                <select value={props.settings.pattern} onChange={(event) => props.onSettings({ pattern: event.target.value })}>
                  {["grid", "lines", "triangles", "gyroid", "cubic", "honeycomb", "lightning", "concentric", "zigzag", "cross"].map((pattern) => (
                    <option key={pattern} value={pattern}>{pattern}</option>
                  ))}
                </select>
              </div>
            </>
          )}
          {section === "Speed" && (
            <div className="menu-row setting-row">
              <span className="setting-name">
                Print speed
                <span className="setting-q" title="The speed every feature falls back to.">?</span>
              </span>
              <span className="setting-number">
                <input
                  type="number"
                  value={props.settings.speed}
                  min={10}
                  max={500}
                  step={5}
                  onChange={(event) => props.onSettings({ speed: Number(event.target.value) })}
                />
                <em>mm/s</em>
              </span>
            </div>
          )}
          {section === "Support" && (
            <div className="menu-row setting-row">
              <span className="setting-name">
                Enable support
                <span className="setting-q" title="Print support under overhangs.">?</span>
              </span>
              <label className="menu-toggle">
                <input
                  type="checkbox"
                  checked={props.settings.supports}
                  onChange={(event) => props.onSettings({ supports: event.target.checked })}
                />
                <em>{props.settings.supports ? "on" : "off"}</em>
              </label>
            </div>
          )}
          {section === "Others" && (
            <>
              <div className="menu-row setting-row">
                <span className="setting-name">
                  Nozzle temperature
                  <span className="setting-q" title="Hotend temperature for the first layer.">?</span>
                </span>
                <span className="setting-number">
                  <input
                    type="number"
                    value={props.settings.nozzle}
                    min={150}
                    max={320}
                    step={5}
                    onChange={(event) => props.onSettings({ nozzle: Number(event.target.value) })}
                  />
                  <em>°C</em>
                </span>
              </div>
              <div className="menu-row setting-row">
                <span className="setting-name">
                  Bed temperature
                  <span className="setting-q" title="Bed temperature for the first layer.">?</span>
                </span>
                <span className="setting-number">
                  <input
                    type="number"
                    value={props.settings.bed}
                    min={0}
                    max={120}
                    step={5}
                    onChange={(event) => props.onSettings({ bed: Number(event.target.value) })}
                  />
                  <em>°C</em>
                </span>
              </div>
            </>
          )}

          {rows.map((entry, index) => {
            const previous = rows[index - 1];
            const heading = previous?.group !== entry.group;
            return (
              <div key={entry.field.key}>
                {heading && <h4 className="settings-heading">{entry.group}</h4>}
                {row(entry.field)}
              </div>
            );
          })}

          {rows.length === 0 && <p className="hint">Nothing in this section at this level. Turn advanced on.</p>}

          {Object.keys(props.overrides).length > 0 && (
            <button type="button" className="settings-reset" onClick={props.onResetOverrides}>
              reset {Object.keys(props.overrides).length} changed
            </button>
          )}
        </div>
      )}
    </div>
  );
}
