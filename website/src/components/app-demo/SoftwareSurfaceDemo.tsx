import { useState } from "react";
import DemoWorkbench from "../demos/DemoWorkbench";
import { CliTerminal } from "../demos/CliTerminal";
import { McpChat } from "../demos/McpChat";

const TABS = [
  { id: "workbench", label: "Workbench" },
  { id: "cli", label: "CLI" },
  { id: "mcp", label: "MCP" },
] as const;

export default function SoftwareSurfaceDemo() {
  const [tab, setTab] = useState<(typeof TABS)[number]["id"]>("workbench");

  return (
    <section className="software-surface" id="software" aria-labelledby="software-title" data-reveal>
      <div className="software-surface__tabs" role="tablist" aria-label="Shale surfaces">
        {TABS.map((t) => (
          <button
            key={t.id}
            type="button"
            role="tab"
            aria-selected={tab === t.id}
            className={tab === t.id ? "is-active" : ""}
            onClick={() => setTab(t.id)}
          >
            {t.label}
          </button>
        ))}
      </div>
      <div className="software-surface__body">
        {tab === "workbench" && <DemoWorkbench />}
        {tab === "cli" && (
          <div id="cli">
            <CliTerminal />
          </div>
        )}
        {tab === "mcp" && (
          <div id="mcp">
            <McpChat />
          </div>
        )}
      </div>
    </section>
  );
}