import { useCallback, useEffect, useState, type ReactNode } from "react";
import { DemoProvider, useDemo } from "../../lib/demo-context";
import { GcodeViewer } from "../demos/GcodeViewer";
import { FieldThemePicker } from "../demos/FieldThemePicker";
import { meter, shaleBox, WORKFLOW_STEPS, type WorkflowStepId } from "./shale-app-tokens";

const STEP_MS = 3600;

function Sidebar({ stepId }: { stepId: WorkflowStepId }) {
  const { estimates, data } = useDemo();
  const sliceTimeS = Math.round(Number(data?.meta?.time_s ?? estimates.time_s));
  const sliceFilM = Number(data?.meta?.filament_m ?? estimates.filament_m);
  return (
    <aside className="shale-app__sidebar">
      <div className="shale-app__tabs">
        <span className={stepId === "printer" || stepId === "import" || stepId === "settings" || stepId === "slice" ? "is-on" : ""}>
          Prepare
        </span>
        <span className={stepId === "preview" ? "is-on" : ""}>Preview</span>
        <span className={stepId === "send" || stepId === "cli" ? "is-on" : ""}>Device</span>
      </div>

      {stepId === "printer" && (
        <>
          <p className="sidebar-label">Printer</p>
          <p>
            <span className="accent-dot">●</span> Bambu Lab A1
          </p>
          <p className="dim">256 × 256 × 256 mm · 0.4 mm nozzle</p>
          <p className="dim">BBL PLA · chamber off</p>
        </>
      )}

      {(stepId === "import" || stepId === "settings" || stepId === "slice" || stepId === "preview" || stepId === "send" || stepId === "cli") && (
        <>
          <p className="sidebar-label">Objects</p>
          <p>t-wing.stl</p>
          <p className="dim">Plate 1 · 42 × 24 mm footprint</p>
        </>
      )}

      {(stepId === "settings" || stepId === "slice" || stepId === "preview" || stepId === "send") && (
        <>
          <p className="sidebar-label">Process</p>
          <p>0.20mm Standard @BBL A1</p>
          <p className="dim">PLA · 20 % grid infill · 2 walls</p>
        </>
      )}

      {(stepId === "slice" || stepId === "preview" || stepId === "send" || stepId === "cli") && (
        <>
          <p className="sidebar-label">Slice summary</p>
          <p>
            {estimates.layers} layers · {sliceTimeS} s
          </p>
          <p className="dim">{sliceFilM.toFixed(2)} m filament</p>
          <p className="dim">Real baseline · t-wing_972c.gcode</p>
        </>
      )}

      {stepId === "send" && (
        <>
          <p className="sidebar-label">Device</p>
          <p>
            <span className="accent-dot">●</span> My A1 · idle
          </p>
        </>
      )}
    </aside>
  );
}

function StepPrinter() {
  return (
    <div className="ascii-frame">
      <FrameTitle>Choose printer — Bambu Lab catalogue</FrameTitle>
      <p>
        <span className="accent-dot">●</span> Bambu Lab A1 · 256×256×256
      </p>
      <p className="dim">P1S · X1 Carbon · A1 mini…</p>
    </div>
  );
}

function FrameTitle({ children }: { children: ReactNode }) {
  return <div className="ascii-frame__title">{children}</div>;
}

function StepImport() {
  return (
    <div className="ascii-frame">
      <FrameTitle>examples/t-wing.stl</FrameTitle>
      <p>Import · centre on build plate</p>
    </div>
  );
}

function StepSettings() {
  return (
    <div className="ascii-frame">
      <FrameTitle>Process sheet</FrameTitle>
      <p>layer_height ━━━━━●━━━━ 0.20</p>
      <p>infill_sparse_density ━━●━━━━━━ 20 %</p>
      <p>wall_line_count ●━━━━━━━━ 2</p>
    </div>
  );
}

function StepSlice() {
  return (
    <div className="ascii-frame">
      <FrameTitle>CuraEngine</FrameTitle>
      <p>{meter(0.72)} slicing…</p>
      <p className="dim">Wall generation · infill · skin</p>
    </div>
  );
}

function StepPreviewInner() {
  const { data, layer, setLayer, showTravel } = useDemo();
  const max = (data?.layers.length ?? 1) - 1;

  useEffect(() => {
    setLayer(Math.max(0, max - 1));
  }, [max, setLayer]);

  const idx = Math.min(layer, max);

  return (
    <div className="shale-app__preview-wrap">
      <div className="shale-app__viewport">
        <GcodeViewer
          data={data}
          layerIndex={idx}
          showTravel={showTravel}
          buildProgress={1}
          hero
          showLegend
          className="gcode-viewer--app"
        />
      </div>
    </div>
  );
}

function StepPreview() {
  return <StepPreviewInner />;
}

function StepSend() {
  return (
    <div className="ascii-frame">
      <FrameTitle>Send to printer</FrameTitle>
      <p>{meter(0)} LAN upload</p>
      <p className="dim">863 s · 0.95 m — gcode ready</p>
    </div>
  );
}

function StepCli() {
  return (
    <div className="ascii-frame ascii-frame--cli">
      <p>
        <span className="accent-dot">$</span> shale-cli slice examples/t-wing.stl --printer bambu_lab_a1
      </p>
      <p className="dim">120 layers · 0.95 m · ~863 s</p>
      <p>
        MCP <span className="dim">slice_model → gcode_path</span>
      </p>
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
  return (
    <div className="shale-app">
      <div className="shale-app__titlebar">
        <span className="shale-app__title">
          <span className="shale-app__title-mark" aria-hidden="true">{`${shaleBox.tl}${shaleBox.h.repeat(3)}${shaleBox.tr}`}</span>
          <span className="shale-app__title-text">Shale</span>
          <span className="shale-app__title-step">{step.label}</span>
        </span>
        <FieldThemePicker />
      </div>
      <div className="shale-app__body">
        <Sidebar stepId={step.id} />
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
  const [step, setStep] = useState(4);
  const [paused, setPaused] = useState(true);

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