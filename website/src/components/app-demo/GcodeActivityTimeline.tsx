import { useCallback, useEffect, useMemo, useState } from "react";
import { FEATURE_LABELS, featureColor } from "../../lib/gcode-types";

type Segment = { type: string; t0: number; t1: number; layer: number };
type TimelineData = {
  label: string;
  meta: { time_s: number; filament_m: number; moves: number; layers: number };
  layer_times: { layer: number; t: number }[];
  lanes: { id: string; count: number }[];
  segments: Segment[];
  log: string[];
};

const LANE_ORDER = ["wall-outer", "wall-inner", "skin", "infill", "support", "travel"];

function layerAtTime(times: { layer: number; t: number }[], tSec: number): number {
  let layer = 0;
  for (const row of times) {
    if (row.t <= tSec) layer = row.layer;
    else break;
  }
  return layer;
}

function formatTime(sec: number) {
  const m = Math.floor(sec / 60);
  const s = Math.floor(sec % 60);
  return `${m}:${String(s).padStart(2, "0")}`;
}

export default function GcodeActivityTimeline() {
  const [data, setData] = useState<TimelineData | null>(null);
  const [tSec, setTSec] = useState(300);
  const [playing, setPlaying] = useState(false);

  useEffect(() => {
    fetch("/demo/demo-slice-timeline.json")
      .then((r) => r.json())
      .then((json: TimelineData) => {
        setData(json);
        setTSec(json.meta.time_s * 0.35);
      })
      .catch(() => setData(null));
  }, []);

  const total = data?.meta.time_s ?? 863;

  const emitScrub = useCallback(
    (t: number) => {
      if (!data) return;
      const layer = layerAtTime(data.layer_times, t);
      window.dispatchEvent(new CustomEvent("shale-timeline-scrub", { detail: { layer, t } }));
    },
    [data],
  );

  useEffect(() => {
    emitScrub(tSec);
  }, [tSec, emitScrub]);

  useEffect(() => {
    if (!playing || !data) return;
    const id = window.setInterval(() => {
      setTSec((prev) => {
        const next = prev + total / 400;
        return next >= total ? 0 : next;
      });
    }, 35);
    return () => window.clearInterval(id);
  }, [playing, data, total]);

  const lanes = useMemo(() => {
    if (!data) return [];
    const counts = new Map(data.lanes.map((l) => [l.id, l.count]));
    return LANE_ORDER.map((id) => ({
      id,
      count: counts.get(id) ?? 0,
      segs: data.segments.filter((s) => s.type === id),
    }));
  }, [data]);

  const logLines = useMemo(() => {
    if (!data) return [];
    const cut = Math.floor((tSec / total) * (data.log.length - 1));
    return data.log.filter((_, i) => i <= cut).slice(-7);
  }, [data, tSec, total]);

  if (!data) {
    return (
      <div className="gcode-timeline" id="activity">
        <p className="dim">Loading real G-code timeline…</p>
      </div>
    );
  }

  const playheadPct = (tSec / total) * 100;

  return (
    <section className="gcode-timeline" id="activity" aria-labelledby="activity-title" data-reveal>
      <header className="gcode-timeline__head">
        <h2 id="activity-title">T-wing print timeline</h2>
        <p className="dim">{data.label}</p>
      </header>

      <div className="gcode-timeline__controls">
        <button type="button" className="btn btn--ghost" onClick={() => setPlaying((p) => !p)}>
          {playing ? "Pause" : "Play"}
        </button>
        <span className="mono dim">
          {formatTime(tSec)} / {formatTime(total)}
        </span>
        <input
          type="range"
          min={0}
          max={1000}
          value={Math.round((tSec / total) * 1000)}
          onChange={(e) => {
            setPlaying(false);
            setTSec((Number(e.target.value) / 1000) * total);
          }}
          aria-label="Scrub print time"
          className="gcode-timeline__scrub"
        />
      </div>

      <div className="gcode-timeline__swim" style={{ ["--playhead" as string]: `${playheadPct}%` }}>
        {lanes.map((lane) => (
          <div key={lane.id} className="gcode-timeline__lane">
            <span className="gcode-timeline__lane-name" style={{ color: featureColor(lane.id) }}>
              {FEATURE_LABELS[lane.id] ?? lane.id}
            </span>
            <div className="gcode-timeline__track" aria-hidden="true">
              {lane.segs.map((s, i) => {
                const left = (s.t0 / total) * 100;
                const w = Math.max(0.12, ((s.t1 - s.t0) / total) * 100);
                return (
                  <span
                    key={`${s.t0}-${i}`}
                    className="gcode-timeline__tick"
                    style={{ left: `${left}%`, width: `${w}%`, background: featureColor(lane.id) }}
                  />
                );
              })}
            </div>
            <span className="gcode-timeline__lane-count dim">{lane.count}</span>
          </div>
        ))}
        <div className="gcode-timeline__playhead-col" aria-hidden="true" />
      </div>

      <div className="gcode-timeline__log mono" aria-live="polite">
        {logLines.map((line) => (
          <div key={line}>{line}</div>
        ))}
      </div>

      <p className="gcode-timeline__footer">
        {data.meta.layers} layer markers · {data.meta.moves.toLocaleString()} moves · {data.meta.filament_m} m ·{" "}
        {data.meta.time_s} s
      </p>
    </section>
  );
}