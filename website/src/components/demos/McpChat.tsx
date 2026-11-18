import { useEffect, useState } from "react";
import { DemoFrame } from "./DemoFrame";

type Msg = { role: "user" | "assistant" | "tool"; text: string };

export function McpChat() {
  const [label, setLabel] = useState("");
  const [messages, setMessages] = useState<Msg[]>([]);
  const [shown, setShown] = useState(0);

  useEffect(() => {
    fetch("/demo/mcp-transcript.json")
      .then((r) => r.json())
      .then((json: { label: string; messages: Msg[] }) => {
        setLabel(json.label);
        setMessages(json.messages);
      })
      .catch(() => setMessages([]));
  }, []);

  useEffect(() => {
    if (!messages.length) return;
    const reduced = window.matchMedia("(prefers-reduced-motion: reduce)").matches;
    if (reduced) {
      setShown(messages.length);
      return;
    }
    if (shown >= messages.length) return;
    const t = window.setTimeout(() => setShown((s) => s + 1), 900);
    return () => window.clearTimeout(t);
  }, [messages, shown]);

  return (
    <DemoFrame title="shale-mcp" note="Recorded exchange">
      <div className="mcp-thread" aria-live="polite">
        {messages.slice(0, shown).map((m, i) => (
          <p key={i} className={`mcp-bubble mcp-${m.role}`}>
            <span className="mcp-role">{m.role}</span>
            {m.text}
          </p>
        ))}
      </div>
      {label && <p className="demo-footnote dim">{label}</p>}
      <button type="button" className="demo-btn" onClick={() => setShown(0)}>
        Replay
      </button>
    </DemoFrame>
  );
}