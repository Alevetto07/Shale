import { useEffect, useState } from "react";
import { DemoFrame } from "./DemoFrame";

type Line = { type: "prompt" | "out"; text: string };

export function CliTerminal() {
  const [lines, setLines] = useState<Line[]>([]);
  const [label, setLabel] = useState("");
  const [index, setIndex] = useState(0);

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
    if (!lines.length) return;
    const reduced = window.matchMedia("(prefers-reduced-motion: reduce)").matches;
    if (reduced) {
      setIndex(lines.length);
      return;
    }
    if (index >= lines.length) return;
    const delay = lines[index].type === "prompt" ? 700 : 400;
    const t = window.setTimeout(() => setIndex((i) => i + 1), delay);
    return () => window.clearTimeout(t);
  }, [lines, index]);

  const visible = lines.slice(0, index);

  return (
    <DemoFrame title="shale-cli" note="Recorded session">
      <pre className="ascii-terminal" role="log" aria-live="polite">
        <span className="ascii-terminal-frame">┌ shale-cli ─────────────────────┐</span>
        {visible.map((line, i) => (
          <span key={i} className={line.type === "prompt" ? "line-prompt" : "line-out"}>
            {line.text}
            {"\n"}
          </span>
        ))}
        {index < lines.length && <span className="line-cursor">▌</span>}
        <span className="ascii-terminal-frame">└────────────────────────────────┘</span>
      </pre>
      {label && <p className="demo-footnote dim">{label}</p>}
      <button type="button" className="demo-btn" onClick={() => setIndex(0)}>
        Replay
      </button>
    </DemoFrame>
  );
}