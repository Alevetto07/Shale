import { useEffect, useRef, useState } from "react";
import { useInView } from "motion/react";

type Msg = { role: "user" | "assistant" | "tool"; text: string };

export function McpChat({ autoStart = false }: { autoStart?: boolean }) {
  const root = useRef<HTMLDivElement>(null);
  const inView = useInView(root, { margin: "-15% 0px" });
  const [label, setLabel] = useState("");
  const [messages, setMessages] = useState<Msg[]>([]);
  const [shown, setShown] = useState(0);
  const [armed, setArmed] = useState(!autoStart);

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
    if (autoStart && inView) setArmed(true);
  }, [autoStart, inView]);

  useEffect(() => {
    if (!armed || !messages.length) return;
    const reduced = window.matchMedia("(prefers-reduced-motion: reduce)").matches;
    if (reduced) {
      setShown(messages.length);
      return;
    }
    if (shown >= messages.length) return;
    const t = window.setTimeout(() => setShown((s) => s + 1), 850);
    return () => window.clearTimeout(t);
  }, [messages, shown, armed]);

  const display = armed ? messages.slice(0, shown) : messages;

  return (
    <section ref={root} className="demo-panel demo-panel--mcp" id="mcp">
      <header className="demo-panel__head">
        <div>
          <h2 className="demo-panel__title">Agent chat</h2>
          <p className="demo-panel__note">{label || "Recorded exchange"}</p>
        </div>
      </header>
      <div className="mcp-chat" aria-live="polite">
        {display.map((m, i) => (
          <article key={i} className={`mcp-msg mcp-msg--${m.role}`}>
            <header className="mcp-msg__meta">{m.role === "user" ? "You" : m.role === "tool" ? "Tool" : "Shale MCP"}</header>
            <p className="mcp-msg__text">{m.text}</p>
          </article>
        ))}
        {armed && shown < messages.length && (
          <p className="mcp-msg mcp-msg--typing" aria-hidden>
            <span className="mcp-typing-dots" />
          </p>
        )}
      </div>
      <button
        type="button"
        className="btn btn--ghost btn--sm"
        onClick={() => {
          setShown(0);
          setArmed(true);
        }}
      >
        Replay
      </button>
    </section>
  );
}