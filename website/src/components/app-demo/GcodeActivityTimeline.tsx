import { useEffect, useMemo, useState } from "react";
import { FEATURE_LABELS, featureColor } from "../../lib/gcode-types";

type TimelineData = {
  label: string;
  meta: { time_s: number; filament_m: number; moves: number; layers: number };
  lanes: { id: string; count: number; fraction: number }[];
  log: string[];
};

export default function GcodeActivityTimeline() {
  const [data, setData] = useState<TimelineData | null>(null);
  const [t, setT] = useState(0.35);
  const [playing, setPlaying] = useState(false);

  useEffect(() => {
    fetch("/demo/demo-slice-timeline.json")
      .then((r) => r.json())
      .then(setData)
      .catch(() => setData(null));
  }, []);

  useEffect(() => {
    if (!playing) return;
    const id = window.setInterval(() => {
      setT((prev) => {
        const next = prev + 0.008;
        return next >= 1 ? 0 : next;
      });
    }, 40);
    return () => window.clearInterval(id);
  }, [playing]);

  const footer = useMemo(() => {
    if (!data) return "";
    const m = data.meta;
    return `${m.layers} layers · ${m.moves.toLocaleString()} moves · ${m.filament_m} m filament · ${m.time_s} s print`;
  }, [data]);

  if (!data) {
    return (
      <div className="gcode-timeline" id="activity">
        <p className="dim">Loading real G-code timeline…</p>
      </div>
    );
  }

  return (
    <section className="gcode-timeline" id="activity" aria-labelledby="activity-title" data-reveal>
      <header>
        <h2 id="activity-title">T-wing print timeline</h2>
        <p className="dim">{data.label}</p>
      </header>
      <div className="gcode-timeline__lanes">
        {data.lanes.map((lane) => (
          <div key={lane.id} className="gcode-timeline__lane">
            <span style={{ color: featureColor(lane.id) }}>
              {FEATURE_LABELS[lane.id] ?? lane.id}
            </span>
            <div className="gcode-timeline__bar">
              <TimelineBar fraction={lane.fraction} color={featureColor(lane.id)} playhead={t} />
            </div>
            <span className="dim">{lane.count}</span>
          </div>
        ))}
      </div>
      <div style={{ display: "flex", gap: "0.5rem", alignItems: "center" }}>
        <button type="button" className="btn btn--ghost" onClick={() => setPlaying((p) => !p)}>
          {playing ? "Pause" : "Play"}
        </button>
        <input
          type="range"
          min={0}
          max={1000}
          value={Math.round(t * 1000)}
          onChange={(e) => {
            setPlaying(false);
            setT(Number(e.target.value) / 1000);
          }}
          aria-label="Scrub print time"
          style={{ flex: 1 }}
        />
      </div>
      <div className="gcode-timeline__log mono" aria-live="polite">
        {data.log
          .filter((_, i) => i <= Math.floor(t * (data.log.length - 1)))
          .slice(-8)
          .map((line) => (
            <div key={line}>{line}</div>
          ))}
      </div>
      <p className="gcode-timeline__footer">{footer}</p>
    </section>
  );
}

function TimelineBar({
  fraction,
  color,
  playhead,
}: {
  fraction: number;
  color: string;
  playhead: number;
}) {
  const width = Math.min(100, fraction * 100 * 1.05);
  return (
    <>
      <span className="gcode-timeline__fill" style={{ width: `${width}%`, background: color }} />
      <span className="gcode-timeline__playhead" style={{ left: `${playhead * 100}%` }} />
    </>
  );
}