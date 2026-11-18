import { useCallback, useEffect, useState } from "react";
import { DemoProvider, useDemo } from "../../lib/demo-context";
import { GcodeViewer } from "../demos/GcodeViewer";
import { meter, shaleBox, WORKFLOW_STEPS, type WorkflowStepId } from "./shale-app-tokens";

const STEP_MS = 3600;

function Skeleton({ widths = ["w90", "w75", "w55"] as const }) {
  return (
    <div style={{ display: "flex", flexDirection: "column", gap: "0.35rem" }}>
      {widths.map((w) => (
        <div key={w} className={`skeleton-line skeleton-line--${w}`} />
      ))}
    </div>
  );
}

function StepPrinter() {
  return (
    <div className="ascii-frame">
      <div className="ascii-frame__title">Choose printer · Bambu Lab</div>
      <p>
        <span style={{ color: "var(--accent)" }}>●</span> Bambu Lab A1 · 256×256×256
      </p>
      <p className="dim">Bambu Lab P1S · 256×256×256</p>
      <p className="dim">Bambu Lab X1 Carbon · 256×256×256</p>
      <Skeleton widths={["w75", "w55"]} />
    </div>
  );
}

function StepImport() {
  return (
    <div className="ascii-frame">
      <div className="ascii-frame__title">examples/t-wing.stl</div>
      <p>t-wing · 42.3 × 24.1 × 8.6 mm</p>
      <p className="dim">Plate 1 · centred · mm</p>
      <Skeleton />
    </div>
  );
}

function StepSettings() {
  return (
    <div className="ascii-frame">
      <div className="ascii-frame__title">Process · 0.20mm Standard @BBL A1</div>
      <p>layer_height ━━━━━●━━━━ 0.20</p>
      <p>infill_sparse_density ━━●━━━━━━ 20 %</p>
      <p>wall_line_count ●━━━━━━━━ 2</p>
      <p className="dim">PLA · skirt · tree support off</p>
    </div>
  );
}

function StepSlice() {
  return (
    <div className="ascii-frame">
      <div className="ascii-frame__title">CuraEngine · slicing t-wing.stl</div>
      <p>{meter(0.72)} 72 %</p>
      <p className="dim">Layer 86 / 120 · estimating support…</p>
    </div>
  );
}

function StepPreviewInner() {
  const { data, layer, setLayer } = useDemo();
  const max = (data?.layers.length ?? 1) - 1;
  const idx = Math.min(layer, max);

  useEffect(() => {
    setLayer(max);
  }, [max, setLayer]);

  return (
    <div className="shale-app__viewport">
      <GcodeViewer
        data={data}
        layerIndex={idx}
        showTravel={false}
        buildProgress={0.65}
        hero
        className="gcode-viewer--app"
      />
      <p className="dim" style={{ position: "absolute", bottom: 6, left: 8, margin: 0, fontSize: "0.65rem" }}>
        Real CuraEngine G-code · layer {idx + 1}/{data?.layers.length ?? "—"}
      </p>
    </div>
  );
}

function StepPreview() {
  return <StepPreviewInner />;
}

function StepSend() {
  return (
    <div className="ascii-frame">
      <div className="ascii-frame__title">Device · LAN</div>
      <p>
        <span style={{ color: "var(--accent)" }}>●</span> My A1 · idle · 192.168.0.42
      </p>
      <p>{meter(0)} transfer</p>
      <p className="dim">120 layers · 863 s · 0.95 m PLA — ready to send</p>
    </div>
  );
}

function StepCli() {
  return (
    <div className="ascii-frame" style={{ fontSize: "0.65rem" }}>
      <p>
        <span style={{ color: "var(--accent)" }}>$</span> shale-cli slice examples/t-wing.stl --printer bambu_lab_a1
      </p>
      <p className="dim">Sliced 1 model · 120 layers · 0.95 m · ~863 s</p>
      <p>
        <span style={{ color: "var(--accent)" }}>MCP</span> slice_model → tool result: gcode_path
      </p>
      <p className="dim">Illustrative replay — stats match real t-wing_972c.gcode</p>
    </div>
  );
}

function StepBody({ stepId }: { stepId: WorkflowStepId }) {
  switch (stepId) {
    case "printer":
      return <StepPrinter />;
    case "import":
      return <StepImport />;
    case "settings":
      return <StepSettings />;
    case "slice":
      return <StepSlice />;
    case "preview":
      return <StepPreview />;
    case "send":
      return <StepSend />;
    case "cli":
      return <StepCli />;
    default:
      return null;
  }
}

function AppFrame({ stepIndex }: { stepIndex: number }) {
  const step = WORKFLOW_STEPS[stepIndex];
  const frame = `${shaleBox.tl}${shaleBox.h.repeat(18)}${shaleBox.tr}`;

  return (
    <div className="shale-app">
      <div className="shale-app__titlebar">
        <span>
          {frame} Shale · {step.label}
        </span>
        <span className="field-picker field-picker--mini" aria-hidden="true">
          Andromeda · Saturn · Nebula · Lunar
        </span>
      </div>
      <div className="shale-app__body">
        <aside className="shale-app__sidebar" aria-hidden="true">
          <div className="shale-app__tabs">
            <span className="is-on">Prepare</span>
            <span>Preview</span>
            <span>Device</span>
          </div>
          <p className="dim">t-wing.stl</p>
          <Skeleton widths={["w90", "w55"]} />
        </aside>
        <div className="shale-app__main">
          <StepBody stepId={step.id} />
        </div>
      </div>
    </div>
  );
}

function WorkflowTimeline({
  active,
  paused,
  onSelect,
  onTogglePause,
}: {
  active: number;
  paused: boolean;
  onSelect: (i: number) => void;
  onTogglePause: () => void;
}) {
  return (
    <nav className="workflow-timeline" aria-label="App workflow steps">
      <button type="button" className="workflow-pause" onClick={onTogglePause} aria-pressed={paused}>
        {paused ? "▶" : "⏸"}
      </button>
      {WORKFLOW_STEPS.map((s, i) => (
        <button
          key={s.id}
          type="button"
          className={i === active ? "is-active" : ""}
          onClick={() => onSelect(i)}
        >
          {s.label}
        </button>
      ))}
    </nav>
  );
}

function AppWorkflowDemoInner() {
  const [step, setStep] = useState(0);
  const [paused, setPaused] = useState(false);

  const bump = useCallback(() => {
    setStep((s) => (s + 1) % WORKFLOW_STEPS.length);
  }, []);

  useEffect(() => {
    if (paused) return;
    const id = window.setInterval(bump, STEP_MS);
    return () => window.clearInterval(id);
  }, [paused, bump]);

  return (
    <div className="app-workflow" id="app-demo" data-reveal>
      <AppFrame stepIndex={step} />
      <WorkflowTimeline
        active={step}
        paused={paused}
        onSelect={(i) => {
          setStep(i);
          setPaused(true);
        }}
        onTogglePause={() => setPaused((p) => !p)}
      />
    </div>
  );
}

export default function AppWorkflowDemo() {
  return (
    <DemoProvider eager>
      <AppWorkflowDemoInner />
    </DemoProvider>
  );
}