import { useEffect, useMemo, useState } from "react";
import { motion } from "motion/react";
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
import { Button, Input, NumericControl, Select, SwitchRow, Tabs, TabsContent, TabsList, TabsTrigger, Tooltip } from "./ui";
import { springSnappy } from "./ui/springs";
import { useReducedMotion } from "./ui/use-reduced-motion";

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

const INFILL_PATTERNS = ["grid", "lines", "triangles", "gyroid", "cubic", "honeycomb", "lightning", "concentric", "zigzag", "cross"];

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

function fractionDigits(step: number | undefined): number {
  if (step == null || Number.isInteger(step)) return 0;
  const text = String(step);
  const index = text.indexOf(".");
  return index < 0 ? 0 : Math.min(3, text.length - index - 1);
}

function formatNumber(value: number, digits: number): string {
  if (digits <= 0) return String(Math.round(value));
  return value.toFixed(digits);
}

function Help({ text }: { text: string }) {
  return (
    <Tooltip label={text}>
      <button type="button" className="setting-q" aria-label={text}>
        ?
      </button>
    </Tooltip>
  );
}

export function ProcessSheet(props: Props) {
  const [section, setSection] = useState<Section>("Quality");
  const [advanced, setAdvanced] = useState(false);
  const [picking, setPicking] = useState(false);
  const [hidden, setHidden] = useState<Set<string>>(() => loadHidden());
  const [search, setSearch] = useState("");
  const reduce = useReducedMotion();

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

  const ranged = (name: string, key: string, value: number, min: number, max: number, step: number, unit?: string) => (
    <div className="menu-row setting-row setting-row--range" data-changed={key in props.overrides ? "yes" : "no"}>
      <span className="setting-name">
        {name}
        <Help text={describe(key, name)} />
        {quickReset(key)}
      </span>
      <NumericControl
        label={name}
        value={value}
        min={min}
        max={max}
        step={step}
        digits={fractionDigits(step)}
        unit={unit}
        onChange={(next) => props.onOverride(key, formatNumber(next, fractionDigits(step)))}
      />
    </div>
  );

  const control = (field: SettingField) => {
    const value = props.overrides[field.key] ?? "";
    if (field.kind === "toggle") {
      const profileOn = profile[field.key] === "1" || profile[field.key] === "true";
      const on = value === "" ? profileOn : value === "1" || value === "true";
      return (
        <SwitchRow checked={on} onChange={(next) => props.onOverride(field.key, next ? "1" : "0")}>
          {on ? "on" : "off"}
        </SwitchRow>
      );
    }
    if (field.kind === "choice") {
      return (
        <Select
          label={label(field)}
          value={value}
          options={[
            { value: "", label: "profile" },
            ...(field.options ?? []).map((option) => ({ value: option, label: option })),
          ]}
          onChange={(next) => props.onOverride(field.key, next)}
        />
      );
    }
    if (field.min != null && field.max != null) {
      const fallback = Number(profile[field.key] ?? field.min);
      const numeric = value === "" ? (Number.isFinite(fallback) ? fallback : field.min) : Number(value);
      return (
        <NumericControl
          label={label(field)}
          value={Number.isFinite(numeric) ? numeric : field.min}
          min={field.min}
          max={field.max}
          step={field.step ?? 1}
          digits={fractionDigits(field.step)}
          unit={field.unit}
          onChange={(next) => props.onOverride(field.key, formatNumber(next, fractionDigits(field.step)))}
        />
      );
    }
    return (
      <span className="setting-number">
        <input
          type="number"
          aria-label={label(field)}
          value={value}
          min={field.min}
          max={field.max}
          step={field.step ?? 1}
          placeholder={profile[field.key] ?? "profile"}
          onChange={(event) => props.onOverride(field.key, event.target.value)}
        />
        {field.unit && <em>{field.unit}</em>}
      </span>
    );
  };

  const row = (field: SettingField) => {
    const rangedField = field.kind === "number" && field.min != null && field.max != null;
    return (
      <div
        className={rangedField ? "menu-row setting-row setting-row--range" : "menu-row setting-row"}
        key={field.key}
        data-changed={field.key in props.overrides ? "yes" : "no"}
      >
        <span className="setting-name">
          {label(field)}
          <Help text={describe(field.key, label(field))} />
          {field.kind !== "toggle" ? quickReset(field.key) : null}
        </span>
        {control(field)}
        {field.kind === "toggle" ? quickReset(field.key) : null}
      </div>
    );
  };

  const nonPlanar = section === "Non-planar";
  const body = nonPlanar ? (
    // SLOT: future non-planar controls. Empty on purpose — no settings keys, no engine flags.
    <div className="nonplanar-slot" data-slot="non-planar">
      <p className="nonplanar-slot__title">Non-planar</p>
      <p>Non-planar slicing will appear here.</p>
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
      {section === "Quality" && ranged("Layer height", QUICK_SETTING_KEYS.layerHeight, quick.layerHeight, 0.02, 1, 0.02, "mm")}
      {section === "Strength" && (
        <>
          {ranged("Wall loops", QUICK_SETTING_KEYS.walls, quick.walls, 1, 12, 1)}
          {ranged("Sparse infill density", QUICK_SETTING_KEYS.infill, quick.infill, 0, 100, 5, "%")}
          <div className="menu-row setting-row" data-changed={QUICK_SETTING_KEYS.pattern in props.overrides ? "yes" : "no"}>
            <span className="setting-name">
              Sparse infill pattern
              <Help text="The shape the sparse infill is drawn in." />
              {quickReset(QUICK_SETTING_KEYS.pattern)}
            </span>
            <Select
              label="Sparse infill pattern"
              value={quick.pattern}
              options={INFILL_PATTERNS.map((pattern) => ({ value: pattern, label: pattern }))}
              onChange={(next) => props.onOverride(QUICK_SETTING_KEYS.pattern, next)}
            />
          </div>
        </>
      )}
      {section === "Speed" && ranged("Print speed", QUICK_SETTING_KEYS.speed, quick.speed, 10, 500, 5, "mm/s")}
      {section === "Support" && (
        <div className="menu-row setting-row" data-changed={QUICK_SETTING_KEYS.supports in props.overrides ? "yes" : "no"}>
          <span className="setting-name">
            Enable support
            <Help text="Print support under overhangs." />
          </span>
          <SwitchRow
            checked={quick.supports}
            onChange={(next) => props.onOverride(QUICK_SETTING_KEYS.supports, next ? "1" : "0")}
          >
            {quick.supports ? "on" : "off"}
          </SwitchRow>
          {quickReset(QUICK_SETTING_KEYS.supports)}
        </div>
      )}
      {section === "Others" && (
        <>
          {ranged("Nozzle temperature", QUICK_SETTING_KEYS.nozzle, quick.nozzle, 150, 320, 5, "°C")}
          {ranged("Bed temperature", QUICK_SETTING_KEYS.bed, quick.bed, 0, 120, 5, "°C")}
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

      {rows.length === 0 && (
        <p className="hint">Nothing in this section at this level. Turn advanced on.</p>
      )}

      {Object.keys(props.overrides).length > 0 && (
        <Button type="button" variant="ghost" size="sm" onClick={props.onResetOverrides}>
          reset {Object.keys(props.overrides).length} changed
        </Button>
      )}
    </div>
  );

  return (
    <div className="process-sheet">
      <Tabs value={section} onValueChange={(value) => setSection(value as Section)}>
        <div className="process-head">
          <TabsList className="process-tabs" aria-label="Slice settings">
            {SECTIONS.map((entry) => (
              <TabsTrigger key={entry} value={entry}>
                {entry}
              </TabsTrigger>
            ))}
          </TabsList>
          {!nonPlanar && (
            <SwitchRow checked={advanced} onChange={setAdvanced}>
              advanced
            </SwitchRow>
          )}
        </div>

        {!nonPlanar && (
          <div className="process-presets presets">
            <span className="process-presets-label">{props.printerLabel} process</span>
            {props.qualities.map((preset) => (
              <Button
                key={preset.quality_type}
                type="button"
                size="sm"
                variant={preset.quality_type === props.qualityType ? "primary" : "secondary"}
                onClick={() => applyPreset(preset.quality_type)}
                title={preset.name}
              >
                {preset.name}
              </Button>
            ))}
          </div>
        )}

        {!nonPlanar && (
          <div className="process-tools">
            <label className="process-search">
              <span className="visually-hidden">search settings</span>
              <Input
                type="search"
                value={search}
                placeholder="search settings"
                aria-label="search settings"
                onChange={(event) => setSearch(event.target.value)}
              />
            </label>
            <Button type="button" variant="secondary" size="sm" onClick={() => setPicking((v) => !v)}>
              {picking ? "done" : `settings shown ${hidden.size > 0 ? `(${hidden.size} hidden)` : ""}`}
            </Button>
          </div>
        )}

        {picking && !nonPlanar ? (
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
        ) : (
          <TabsContent value={section}>
            <motion.div
              key={search ? "search" : section}
              initial={reduce ? false : { opacity: 0, y: 6 }}
              animate={{ opacity: 1, y: 0 }}
              transition={reduce ? { duration: 0 } : springSnappy}
            >
              {body}
            </motion.div>
          </TabsContent>
        )}
      </Tabs>
    </div>
  );
}
