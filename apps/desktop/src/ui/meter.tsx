/** Slice progress. A filled bar, not a row of block glyphs. */
export function Meter({
  fraction,
  message,
  stage,
}: {
  fraction: number;
  message: string;
  stage?: string;
}) {
  const clamped = Math.min(1, Math.max(0, fraction));
  const pct = Math.round(clamped * 100);
  const title = stage && stage !== message ? message : undefined;
  return (
    <div
      className="ui-meter"
      role="progressbar"
      aria-valuemin={0}
      aria-valuemax={100}
      aria-valuenow={pct}
      aria-label={stage ?? message}
      title={title}
    >
      <span className="ui-meter__track">
        <span className="ui-meter__fill" style={{ transform: `scaleX(${clamped})` }} />
      </span>
      <span className="ui-meter__label">
        <span className="ui-meter__pct">{pct}%</span> {stage ?? message}
      </span>
    </div>
  );
}
