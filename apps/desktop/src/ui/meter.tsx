/** Slice progress. A filled bar, not a row of block glyphs. */
export function Meter({ fraction, message }: { fraction: number; message: string }) {
  const clamped = Math.min(1, Math.max(0, fraction));
  const pct = Math.round(clamped * 100);
  return (
    <div
      className="ui-meter"
      role="progressbar"
      aria-valuemin={0}
      aria-valuemax={100}
      aria-valuenow={pct}
      aria-label={message}
    >
      <span className="ui-meter__track">
        <span className="ui-meter__fill" style={{ transform: `scaleX(${clamped})` }} />
      </span>
      <span className="ui-meter__label">
        <span className="ui-meter__pct">{pct}%</span> {message}
      </span>
    </div>
  );
}
