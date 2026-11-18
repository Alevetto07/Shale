import { useEffect, useRef, useState } from "react";
import { useInView } from "motion/react";

function useCountUp(target: number, active: boolean, dur = 900) {
  const [val, setVal] = useState(0);
  useEffect(() => {
    if (!active) return;
    const t0 = performance.now();
    let id = 0;
    const tick = (now: number) => {
      const t = Math.min(1, (now - t0) / dur);
      const ease = 1 - Math.pow(1 - t, 3);
      setVal(target * ease);
      if (t < 1) id = requestAnimationFrame(tick);
    };
    id = requestAnimationFrame(tick);
    return () => cancelAnimationFrame(id);
  }, [target, active, dur]);
  return val;
}

export function StatCounter({
  value,
  decimals = 1,
  suffix = "",
  className = "",
}: {
  value: number;
  decimals?: number;
  suffix?: string;
  className?: string;
}) {
  const ref = useRef<HTMLSpanElement>(null);
  const inView = useInView(ref, { once: true, margin: "-20% 0px" });
  const n = useCountUp(value, inView);
  return (
    <span ref={ref} className={className}>
      {n.toFixed(decimals)}
      {suffix}
    </span>
  );
}