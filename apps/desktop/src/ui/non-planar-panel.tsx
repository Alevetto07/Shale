import type { NonplanarReportView, NonplanarSettings } from "../api";

const MODES: { id: NonplanarSettings["mode"]; label: string }[] = [
  { id: "off", label: "Off" },
  { id: "curved-tops", label: "Curved tops" },
  { id: "auto", label: "Auto" },
];

export const ENVELOPE_NOTE =
  "Head-clearance envelopes are conservative estimates. They have not been measured on a printer yet.";

interface Props {
  value: NonplanarSettings;
  onChange: (next: NonplanarSettings) => void;
  report: NonplanarReportView | null;
  /** False when this printer has no head envelope. */
  supported: boolean;
  liveSlice: boolean;
  onLiveSlice: (next: boolean) => void;
}

/** Curved-layer controls. The slice itself runs in shale-core, the same path as the CLI. */
export function NonPlanarPanel(props: Props) {
  const active = props.value.mode !== "off";
  const angle = props.value.max_angle_deg;

  return (
    <div className="nonplanar-slot" data-slot="non-planar" data-disabled="0" id="nonplanar-panel">
      <p className="nonplanar-slot__title">Non-planar</p>
      <p className="nonplanar-slot__lead">
        Curved tops follow a gentle surface instead of stacking flat layers. Off leaves the slice planar.
      </p>

      <div className="nonplanar-slot__modes" role="group" aria-label="Non-planar mode">
        {MODES.map((mode) => (
          <button
            key={mode.id}
            type="button"
            className="nonplanar-slot__mode"
            data-active={props.value.mode === mode.id ? "1" : "0"}
            aria-pressed={props.value.mode === mode.id}
            onClick={() => props.onChange({ ...props.value, mode: mode.id })}
          >
            {mode.label}
          </button>
        ))}
      </div>

      {!props.supported && active && (
        <p className="nonplanar-slot__warn" role="status">
          This printer has no head envelope, so a curved slice will be refused. Choose Off, or a Bambu machine.
        </p>
      )}

      <div className="nonplanar-slot__settings settings-body">
        <div className="menu-row setting-row setting-row--range">
          <span className="setting-name">Max angle</span>
          <div className="nonplanar-slot__slider">
            <label className="nonplanar-slot__angle">
              <input
                type="range"
                aria-label="Max angle"
                min={1}
                max={10}
                step={0.5}
                value={angle}
                onChange={(event) =>
                  props.onChange({ ...props.value, max_angle_deg: Number(event.target.value) })
                }
              />
              <span>
                {angle.toFixed(1)}°
              </span>
            </label>
          </div>
        </div>
        <label className="nonplanar-slot__check">
          <input
            type="checkbox"
            checked={props.value.strict}
            onChange={(event) => props.onChange({ ...props.value, strict: event.target.checked })}
          />
          <span>Strict — stop the slice if the curved pass cannot be checked, instead of printing flat</span>
        </label>
      </div>

      <details className="nonplanar-slot__experimental">
        <summary>Experimental</summary>
        <label className="nonplanar-slot__check">
          <input
            type="checkbox"
            checked={props.value.underside_bands}
            disabled={props.value.mode !== "auto"}
            onChange={(event) =>
              props.onChange({ ...props.value, underside_bands: event.target.checked })
            }
          />
          <span>
            Underside bands. Auto may tilt overhangs, bridges and cantilevers. This is unverified:
            the part sits above the band, so the head-clearance check cannot clear it.
          </span>
        </label>
      </details>

      <p className="nonplanar-slot__note">{ENVELOPE_NOTE}</p>

      {props.value.mode === "off" ? (
        <label className="nonplanar-slot__check">
          <input
            type="checkbox"
            checked={props.liveSlice}
            onChange={(event) => props.onLiveSlice(event.target.checked)}
          />
          <span>Refresh the slice in the background when a planar setting changes</span>
        </label>
      ) : (
        <p className="nonplanar-slot__note">
          Background refresh stays off while curved layers are on. A curved slice runs the engine twice.
        </p>
      )}

      {props.report && <Report report={props.report} />}
    </div>
  );
}

function Report({ report }: { report: NonplanarReportView }) {
  const curved = report.outcome === "curved" && !report.fallback_to_planar;
  const accepted = report.surfaces.filter((surface) => surface.enabled);
  const refused = report.surfaces.filter((surface) => !surface.enabled);
  const slope =
    report.worst_slope_deg == null || !Number.isFinite(report.worst_slope_deg)
      ? "—"
      : `${report.worst_slope_deg.toFixed(2)}°`;
  return (
    <section className="nonplanar-report" aria-label="Non-planar report">
      <p className="nonplanar-report__outcome" data-outcome={curved ? "curved" : "planar"}>
        {curved ? "Curved layers" : "Fell back to a planar slice"}
      </p>
      <dl className="nonplanar-report__facts">
        <div>
          <dt>Curved moves</dt>
          <dd>{report.curved_move_count.toLocaleString("en-GB")}</dd>
        </div>
        <div>
          <dt>Worst slope</dt>
          <dd>{slope}</dd>
        </div>
      </dl>
      {!curved && report.fallback_reasons.length > 0 && (
        <div>
          <p className="nonplanar-report__heading">Why it stayed planar</p>
          <ul>
            {report.fallback_reasons.map((reason) => (
              <li key={reason}>{reason}</li>
            ))}
          </ul>
        </div>
      )}
      {report.surfaces.length > 0 && (
        <div>
          <p className="nonplanar-report__heading">Surfaces</p>
          <ul className="nonplanar-report__surfaces">
            {accepted.map((surface) => (
              <li key={`ok-${surface.id}`} data-accepted="1">
                <span>Accepted</span> {surface.kind}
                {surface.reason ? ` — ${surface.reason}` : ""}
              </li>
            ))}
            {refused.map((surface) => (
              <li key={`no-${surface.id}`} data-accepted="0">
                <span>Refused</span> {surface.kind}
                {surface.reason ? ` — ${surface.reason}` : ""}
              </li>
            ))}
          </ul>
        </div>
      )}
    </section>
  );
}
