import { KIND_COLOR } from "./Viewport";
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

export interface FeatureLegendProps {
  paths: PreviewPath[];
  hidden: Set<string>;
  onToggle: (kind: string) => void;
  onAll: () => void;
  onNone: () => void;
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
      {kinds.map((kind) => {
        const off = props.hidden.has(kind);
        return (
          <button
            type="button"
            key={kind}
            className={off ? "legend-row off" : "legend-row"}
            data-kind={kind}
            title={off ? `show ${label(kind)}` : `hide ${label(kind)}`}
            onClick={() => props.onToggle(kind)}
          >
            <i style={{ background: KIND_COLOR[kind] ?? "#9c9aab" }} />
            <span>{label(kind)}</span>
            <em>{counts.get(kind)}</em>
          </button>
        );
      })}
    </aside>
  );
}
