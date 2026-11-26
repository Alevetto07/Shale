import { KIND_COLOR, SPEED_STOPS, type ColourMode } from "./viewport-style";
import type { PreviewPath } from "./api";

/**
 * What the plate is made of, and a switch for each kind.
 *
 * Bambu Studio's preview carries the same list down the right-hand side, and it
 * earns its place: a slice is walls, skin, infill, support and a prime tower
 * drawn in one breath, and the only way to see whether the infill is right is
 * to turn the walls off. The switches live here rather than in the backend —
 * the toolpath is already in memory, so hiding a kind is a filter, not a job.
 */

export interface FeatureUse {
  kind: string;
  seconds: number;
  metres: number;
}

export interface FeatureLegendProps {
  paths: PreviewPath[];
  hidden: Set<string>;
  onToggle: (kind: string) => void;
  onAll: () => void;
  onNone: () => void;
  /// When the plate is coloured by speed, the swatches stop describing it; the
  /// legend says so and shows the scale the ramp is using instead of pretending.
  colour: ColourMode;
  /// Time and filament from the G-code, one row per feature kind.
  features?: FeatureUse[];
}

function featureClock(seconds: number): string {
  if (seconds < 1) return "<1 s";
  if (seconds < 60) return `${Math.round(seconds)} s`;
  const minutes = Math.round(seconds / 60);
  if (minutes < 60) return `${minutes} min`;
  const hours = Math.floor(minutes / 60);
  const rest = minutes % 60;
  return rest ? `${hours} h ${rest} min` : `${hours} h`;
}

function featureLength(metres: number): string {
  if (metres < 0.005) return "";
  if (metres < 1) return `${Math.round(metres * 1000)} mm`;
  return `${metres.toFixed(metres < 10 ? 2 : 1)} m`;
}

function featureCost(feature: FeatureUse | undefined): string {
  if (!feature) return "";
  const time = feature.seconds > 0 ? featureClock(feature.seconds) : "";
  const length = featureLength(feature.metres);
  return [time, length].filter(Boolean).join(" · ");
}

const PREFERRED = [
  "wall-outer",
  "wall-inner",
  "skin",
  "infill",
  "support",
  "support-interface",
  "brim",
  "raft",
  "skirt",
  "prime-tower",
  "travel",
];

/** `support-interface` reads better as "support interface". */
function label(kind: string): string {
  return kind.replace(/-/g, " ");
}

export function FeatureLegend(props: FeatureLegendProps) {
  const counts = new Map<string, number>();
  for (const path of props.paths) {
    counts.set(path.kind, (counts.get(path.kind) ?? 0) + 1);
  }
  const kinds = [...counts.keys()].sort((a, b) => {
    const left = PREFERRED.indexOf(a);
    const right = PREFERRED.indexOf(b);
    return (left < 0 ? 99 : left) - (right < 0 ? 99 : right) || a.localeCompare(b);
  });
  if (kinds.length === 0) return null;

  const shown = kinds.filter((kind) => !props.hidden.has(kind)).length;

  let low = Infinity;
  let high = -Infinity;
  if (props.colour === "speed") {
    for (const path of props.paths) {
      // Same rule as the viewport: the scale is for extrusion, not travel.
      if (path.kind === "travel") continue;
      const feed = path.feed ?? 0;
      if (feed > 0) {
        low = Math.min(low, feed);
        high = Math.max(high, feed);
      }
    }
  }
  const speeds = props.colour === "speed" && Number.isFinite(low) && high > low;
  const bySpeed = props.colour === "speed";
  let slopeLow = Infinity;
  let slopeHigh = 0;
  for (const path of props.paths) {
    for (const slope of path.slopes ?? []) {
      if (slope > 0.05) {
        slopeLow = Math.min(slopeLow, slope);
        slopeHigh = Math.max(slopeHigh, slope);
      }
    }
  }
  const showSlope = props.colour === "slope" || (Number.isFinite(slopeLow) && slopeHigh > 0.05);

  return (
    <aside className="legend" aria-label="Features in the toolpath">
      <header>
        <span>
          features <em>{shown}/{kinds.length}</em>
        </span>
        <button type="button" onClick={props.onAll} disabled={shown === kinds.length}>
          all
        </button>
        <button type="button" onClick={props.onNone} disabled={shown === 0}>
          none
        </button>
      </header>
      {showSlope && (
        <div className="legend-scale">
          <span className="legend-scale-title">curved slope °</span>
          <div className="legend-ramp legend-ramp--slope" />
          <div className="legend-scale-ends">
            <span>{Number.isFinite(slopeLow) ? slopeLow.toFixed(1) : "0.1"}</span>
            <span>{slopeHigh > 0 ? slopeHigh.toFixed(1) : "10"}</span>
          </div>
        </div>
      )}
      {bySpeed && (
        <div className="legend-scale">
          <span className="legend-scale-title">speed mm/s</span>
          <div className="legend-ramp">
            {SPEED_STOPS.map((stop) => (
              <i key={stop} style={{ background: stop }} />
            ))}
          </div>
          <div className="legend-scale-ends">
            <span>{speeds ? low.toFixed(0) : "?"}</span>
            <span>{speeds ? high.toFixed(0) : "?"}</span>
          </div>
        </div>
      )}
      {kinds.map((kind) => {
        const off = props.hidden.has(kind);
        const cost = featureCost(props.features?.find((item) => item.kind === kind));
        const paths = counts.get(kind) ?? 0;
        return (
          <button
            type="button"
            key={kind}
            className={off ? "legend-row off" : "legend-row"}
            data-kind={kind}
            title={
              cost
                ? `${off ? "show" : "hide"} ${label(kind)}, ${paths} paths, ${cost}`
                : `${off ? "show" : "hide"} ${label(kind)}`
            }
            onClick={() => props.onToggle(kind)}
          >
            <i
              className={bySpeed ? "muted" : undefined}
              style={{ background: KIND_COLOR[kind] ?? "#9c9aab" }}
            />
            <span>{label(kind)}</span>
            {cost ? <em className="legend-cost">{cost}</em> : <em>{paths}</em>}
          </button>
        );
      })}
    </aside>
  );
}
