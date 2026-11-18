import { useEffect, useMemo, useState } from "react";
import { SETTING_GROUPS, type SettingField } from "./settings";
import type { QualityInfo } from "./api";
import { QUICK_SETTING_KEYS, quickSettingsView } from "./engine-settings";
import {
  ADVANCED,
  BAMBU_LABEL,
  GROUP_SECTION,
  NO_BAMBU_EQUIVALENT,
  SECTIONS,
  describe,
  type Section,
} from "./settings-meta";

export interface QuickFallbacks {
  layerHeight: number;
  walls: number;
  infill: number;
  pattern: string;
  supports: boolean;
  speed: number;
  nozzle: number;
  bed: number;
}

interface Props {
  fallbacks: QuickFallbacks;
  overrides: Record<string, string>;
  profileValues?: Record<string, string>;
  onOverride: (key: string, value: string) => void;
  onResetOverrides: () => void;
  qualities: QualityInfo[];
  qualityType: string | null;
  onQuality: (qualityType: string) => void;
  printerLabel: string;
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

  const applyPreset = (qualityType: string) => {
    props.onQuality(qualityType);
    props.onResetOverrides();
  };

  const profile = props.profileValues ?? {};
  const quick = quickSettingsView(profile, props.overrides, props.fallbacks);

  const quickReset = (key: string) =>
    key in props.overrides ? (
      <button type="button" className="setting-revert" onClick={() => props.onOverride(key, "")} title="Use profile value">
        profile
      </button>
    ) : null;

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
        <span className="process-presets-label">{props.printerLabel} process</span>
        {props.qualities.map((preset) => (
          <button
            key={preset.quality_type}
            type="button"
            className={preset.quality_type === props.qualityType ? "process-preset on" : "process-preset"}
            onClick={() => applyPreset(preset.quality_type)}
            title={preset.name}
          >
            {preset.name}
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
            <div
              className="menu-row setting-row"
              data-changed={QUICK_SETTING_KEYS.layerHeight in props.overrides ? "yes" : "no"}
            >
              <span className="setting-name">
                Layer height
                <span className="setting-q" title="Height of every layer after the first.">?</span>
              </span>
              <span className="setting-number">
                <input
                  type="number"
                  value={quick.layerHeight}
                  min={0.02}
                  max={1}
                  step={0.02}
                  onChange={(event) => props.onOverride(QUICK_SETTING_KEYS.layerHeight, event.target.value)}
                />
                <em>mm</em>
                {quickReset(QUICK_SETTING_KEYS.layerHeight)}
              </span>
            </div>
          )}
          {section === "Strength" && (
            <>
              <div
                className="menu-row setting-row"
                data-changed={QUICK_SETTING_KEYS.walls in props.overrides ? "yes" : "no"}
              >
                <span className="setting-name">
                  Wall loops
                  <span className="setting-q" title="How many perimeters are printed around each layer.">?</span>
                </span>
                <span className="setting-number">
                  <input
                    type="number"
                    value={quick.walls}
                    min={1}
                    max={12}
                    step={1}
                    onChange={(event) => props.onOverride(QUICK_SETTING_KEYS.walls, event.target.value)}
                  />
                  {quickReset(QUICK_SETTING_KEYS.walls)}
                </span>
              </div>
              <div
                className="menu-row setting-row"
                data-changed={QUICK_SETTING_KEYS.infill in props.overrides ? "yes" : "no"}
              >
                <span className="setting-name">
                  Sparse infill density
                  <span className="setting-q" title="How much material is laid inside the part.">?</span>
                </span>
                <span className="setting-number">
                  <input
                    type="number"
                    value={quick.infill}
                    min={0}
                    max={100}
                    step={5}
                    onChange={(event) => props.onOverride(QUICK_SETTING_KEYS.infill, event.target.value)}
                  />
                  <em>%</em>
                  {quickReset(QUICK_SETTING_KEYS.infill)}
                </span>
              </div>
              <div
                className="menu-row setting-row"
                data-changed={QUICK_SETTING_KEYS.pattern in props.overrides ? "yes" : "no"}
              >
                <span className="setting-name">
                  Sparse infill pattern
                  <span className="setting-q" title="The shape the sparse infill is drawn in.">?</span>
                </span>
                <select
                  value={quick.pattern}
                  onChange={(event) => props.onOverride(QUICK_SETTING_KEYS.pattern, event.target.value)}
                >
                  {["grid", "lines", "triangles", "gyroid", "cubic", "honeycomb", "lightning", "concentric", "zigzag", "cross"].map((pattern) => (
                    <option key={pattern} value={pattern}>{pattern}</option>
                  ))}
                </select>
                {quickReset(QUICK_SETTING_KEYS.pattern)}
              </div>
            </>
          )}
          {section === "Speed" && (
            <div
              className="menu-row setting-row"
              data-changed={QUICK_SETTING_KEYS.speed in props.overrides ? "yes" : "no"}
            >
              <span className="setting-name">
                Print speed
                <span className="setting-q" title="The speed every feature falls back to.">?</span>
              </span>
              <span className="setting-number">
                <input
                  type="number"
                  value={quick.speed}
                  min={10}
                  max={500}
                  step={5}
                  onChange={(event) => props.onOverride(QUICK_SETTING_KEYS.speed, event.target.value)}
                />
                <em>mm/s</em>
                {quickReset(QUICK_SETTING_KEYS.speed)}
              </span>
            </div>
          )}
          {section === "Support" && (
            <div
              className="menu-row setting-row"
              data-changed={QUICK_SETTING_KEYS.supports in props.overrides ? "yes" : "no"}
            >
              <span className="setting-name">
                Enable support
                <span className="setting-q" title="Print support under overhangs.">?</span>
              </span>
              <label className="menu-toggle">
                <input
                  type="checkbox"
                  checked={quick.supports}
                  onChange={(event) =>
                    props.onOverride(QUICK_SETTING_KEYS.supports, event.target.checked ? "1" : "0")
                  }
                />
                <em>{quick.supports ? "on" : "off"}</em>
              </label>
              {quickReset(QUICK_SETTING_KEYS.supports)}
            </div>
          )}
          {section === "Others" && (
            <>
              <div
                className="menu-row setting-row"
                data-changed={QUICK_SETTING_KEYS.nozzle in props.overrides ? "yes" : "no"}
              >
                <span className="setting-name">
                  Nozzle temperature
                  <span className="setting-q" title="Hotend temperature for the first layer.">?</span>
                </span>
                <span className="setting-number">
                  <input
                    type="number"
                    value={quick.nozzle}
                    min={150}
                    max={320}
                    step={5}
                    onChange={(event) => props.onOverride(QUICK_SETTING_KEYS.nozzle, event.target.value)}
                  />
                  <em>°C</em>
                  {quickReset(QUICK_SETTING_KEYS.nozzle)}
                </span>
              </div>
              <div
                className="menu-row setting-row"
                data-changed={QUICK_SETTING_KEYS.bed in props.overrides ? "yes" : "no"}
              >
                <span className="setting-name">
                  Bed temperature
                  <span className="setting-q" title="Bed temperature for the first layer.">?</span>
                </span>
                <span className="setting-number">
                  <input
                    type="number"
                    value={quick.bed}
                    min={0}
                    max={120}
                    step={5}
                    onChange={(event) => props.onOverride(QUICK_SETTING_KEYS.bed, event.target.value)}
                  />
                  <em>°C</em>
                  {quickReset(QUICK_SETTING_KEYS.bed)}
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
