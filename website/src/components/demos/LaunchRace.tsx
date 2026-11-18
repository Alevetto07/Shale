import { useCallback, useState } from "react";
import { animate, useMotionValue, useTransform, motion } from "motion/react";
import { DemoFrame } from "./DemoFrame";
import { launchBenchmarks } from "../../config/site";

const COMPETITORS = [
  { name: "Shale", seconds: launchBenchmarks.shaleUsableWarmSeconds },
  { name: "Bambu Studio", seconds: 9 },
  { name: "OrcaSlicer", seconds: 8 },
  { name: "Cura", seconds: 11 },
];

export function LaunchRace() {
  const [running, setRunning] = useState(false);
  const progress = useMotionValue(0);
  const max = 11;

  const run = useCallback(() => {
    setRunning(true);
    progress.set(0);
    animate(progress, 1, { duration: 2.2, ease: [0.22, 1, 0.36, 1] }).then(() =>
      setRunning(false),
    );
  }, [progress]);

  return (
    <DemoFrame title="Launch race" note="Measured warm/cold vs competitor range">
      <ul className="race-list">
        {COMPETITORS.map((row) => (
          <RaceRow key={row.name} name={row.name} seconds={row.seconds} max={max} progress={progress} />
        ))}
      </ul>
      <button type="button" className="demo-btn" onClick={run} disabled={running}>
        Run comparison
      </button>
      <p className="demo-footnote dim">
        Shale {launchBenchmarks.shaleUsableWarmLabel} warm · {launchBenchmarks.shaleUsableColdLabel}{" "}
        cold. Others {launchBenchmarks.competitorRangeLabel} (author measurements).
      </p>
    </DemoFrame>
  );
}

function RaceRow({
  name,
  seconds,
  max,
  progress,
}: {
  name: string;
  seconds: number;
  max: number;
  progress: ReturnType<typeof useMotionValue<number>>;
}) {
  const width = useTransform(progress, (t) => `${Math.min(100, (seconds / max) * 100 * t)}%`);
  return (
    <li className="race-row">
      <span className="race-name">{name}</span>
      <span className="race-track" aria-hidden>
        <motion.span className="race-fill" style={{ width }} />
      </span>
      <span className="mono race-time">{seconds} s</span>
    </li>
  );
}