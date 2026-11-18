import { useEffect, useRef, useState } from "react";
import { useInView } from "motion/react";

type Line = { type: "prompt" | "out" | "cmd" | "err"; text: string };

function colorClass(line: Line) {
  if (line.type === "prompt") return "term-line term-prompt";
  if (line.type === "cmd") return "term-line term-cmd";
  if (line.type === "err") return "term-line term-err";
  return "term-line term-out";
}

export function CliTerminal({ autoStart = false }: { autoStart?: boolean }) {
  const root = useRef<HTMLDivElement>(null);
  const inView = useInView(root, { margin: "-15% 0px" });
  const [lines, setLines] = useState<Line[]>([]);
  const [label, setLabel] = useState("");
  const [index, setIndex] = useState(0);
  const [armed, setArmed] = useState(!autoStart);

  useEffect(() => {
    fetch("/demo/cli-transcript.json")
      .then((r) => r.json())
      .then((json: { label: string; lines: Line[] }) => {
        setLabel(json.label);
        setLines(json.lines);
      })
      .catch(() => setLines([]));
  }, []);

  useEffect(() => {
    if (autoStart && inView) setArmed(true);
  }, [autoStart, inView]);

  useEffect(() => {
    if (!armed || !lines.length) return;
    const reduced = window.matchMedia("(prefers-reduced-motion: reduce)").matches;
    if (reduced) {
      setIndex(lines.length);
      return;
    }
    if (index >= lines.length) return;
    const delay = lines[index].type === "prompt" || lines[index].type === "cmd" ? 650 : 320;
    const t = window.setTimeout(() => setIndex((i) => i + 1), delay);
    return () => window.clearTimeout(t);
  }, [lines, index, armed]);

  const visible = lines.slice(0, armed ? index : lines.length);

  return (
    <section ref={root} className="demo-panel demo-panel--terminal" id="cli">
      <header className="demo-panel__head">
        <div>
          <p className="demo-panel__eyebrow">CLI</p>
          <h2 className="demo-panel__title">shale-cli</h2>
          <p className="demo-panel__note">{label || "Recorded session — synthetic replay"}</p>
        </div>
        <motionDots />
      </header>
      <div className="rich-terminal" role="log" aria-live="polite">
        <div className="rich-terminal__titlebar">
          <span className="dot dot--r" />
          <span className="dot dot--y" />
          <span className="dot dot--g" />
          <span className="rich-terminal__title">shale-cli — illustrative</span>
        </div>
        <pre className="rich-terminal__body">
          {visible.map((line, i) => (
            <code key={i} className={colorClass(line)}>
              {line.text}
              {"\n"}
            </code>
          ))}
          {armed && index < lines.length && <span className="term-cursor">▌</span>}
        </pre>
      </div>
      <button
        type="button"
        className="btn btn--ghost btn--sm"
        onClick={() => {
          setIndex(0);
          setArmed(true);
        }}
      >
        Replay
      </button>
    </section>
  );
}

function motionDots() {
  return (
    <span className="demo-live-pill" aria-hidden>
      LIVE REPLAY
    </span>
  );
}