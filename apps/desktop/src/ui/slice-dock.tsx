import { Button } from "./button";

export interface SliceDockProps {
  busy: boolean;
  /** Named stage while a slice is running. */
  stage: string | null;
  /** 0–1 while busy. */
  fraction: number | null;
  disabled: boolean;
  onSlice: () => void;
  time: string | null;
  filament: string | null;
  weight: string | null;
  cost: string | null;
}

/// Pinned under the settings column. The button names the stage while the
/// engine runs, then the same bar shows the estimate.
export function SliceDock(props: SliceDockProps) {
  const stats = [
    props.time ? ["Time", props.time] : null,
    props.filament ? ["Filament", props.filament] : null,
    props.weight ? ["Weight", props.weight] : null,
    props.cost ? ["Cost", props.cost] : null,
  ].filter((item): item is [string, string] => item !== null);

  return (
    <div className="slice-dock">
      {props.busy && (
        <div className="slice-dock__meter" role="progressbar" aria-valuemin={0} aria-valuemax={100} aria-valuenow={Math.round((props.fraction ?? 0) * 100)} aria-label={props.stage ?? "Slicing"}>
          <span style={{ width: `${Math.max(6, Math.min(100, (props.fraction ?? 0.08) * 100))}%` }} />
        </div>
      )}
      {stats.length > 0 && !props.busy && (
        <dl className="slice-dock__stats">
          {stats.map(([label, value]) => (
            <div key={label}>
              <dt>{label}</dt>
              <dd>{value}</dd>
            </div>
          ))}
        </dl>
      )}
      <Button variant="primary" size="lg" className="slice-dock__go" disabled={props.disabled} onClick={props.onSlice}>
        {props.busy ? props.stage || "Slicing layers" : "Slice"}
      </Button>
    </div>
  );
}
