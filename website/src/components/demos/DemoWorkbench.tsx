import { useEffect, useRef } from "react";
import { useInView } from "motion/react";
import { DemoProvider, useDemo } from "../../lib/demo-context";
import { GcodeViewer } from "./GcodeViewer";
import { PrinterCatalog } from "./PrinterCatalog";
import { SliceSettingsDemo } from "./SliceSettingsDemo";
import { CliTerminal } from "./CliTerminal";
import { McpChat } from "./McpChat";
import { FieldThemePicker } from "./FieldThemePicker";
import { LaunchRace } from "./LaunchRace";

function ToolpathPanel() {
  const { data, plateMm, layer, setLayer, playing, setPlaying, infill, visibleLayerCount } = useDemo();
  const max = (data?.layers.length ?? 1) - 1;

  useEffect(() => {
    if (!playing || !data) return;
    const id = window.setInterval(() => {
      setLayer((l) => {
        if (l >= max) {
          setPlaying(false);
          return max;
        }
        return l + 1;
      });
    }, 90);
    return () => clearInterval(id);
  }, [playing, data, max, setLayer, setPlaying]);

  return (
    <section className="demo-panel demo-panel--toolpath" id="toolpath">
      <header className="demo-panel__head">
        <div>
          <p className="demo-panel__eyebrow">Toolpath</p>
          <h2 className="demo-panel__title">T-wing slice · 3D preview</h2>
          <p className="demo-panel__note">
            Plane-cut of <code>examples/t-wing.stl</code> ({visibleLayerCount} effective layers in UI). Illustrative — not live CuraEngine in-browser.
          </p>
        </div>
        <div className="toolpath-controls">
          <input
            type="range"
            min={0}
            max={max}
            value={layer}
            onChange={(e) => setLayer(Number(e.target.value))}
            aria-label="Layer"
          />
          <button type="button" className="btn btn--ghost" onClick={() => setPlaying(!playing)}>
            {playing ? "Pause" : "Play layers"}
          </button>
        </div>
      </header>
      <GcodeViewer data={data} plateMm={plateMm} layerIndex={layer} infill={infill} className="demo-panel__viewer" />
    </section>
  );
}

function DemosInner() {
  const root = useRef(null);
  const inView = useInView(root, { margin: "-10% 0px", once: true });

  return (
    <div ref={root} className={`demos-workbench ${inView ? "demos-workbench--live" : ""}`}>
      <div className="demos-workbench__grid">
        <ToolpathPanel />
        <PrinterCatalog autoStart={inView} />
        <SliceSettingsDemo />
        <CliTerminal autoStart={inView} />
        <McpChat autoStart={inView} />
        <section className="demo-panel demo-panel--launch">
          <header className="demo-panel__head">
            <p className="demo-panel__eyebrow">Performance</p>
            <h2 className="demo-panel__title">Cold vs warm launch</h2>
          </header>
          <LaunchRace />
        </section>
        <section className="demo-panel demo-panel--theme">
          <FieldThemePicker />
        </section>
      </div>
    </div>
  );
}

export default function DemoWorkbench() {
  return (
    <DemoProvider>
      <DemosInner />
    </DemoProvider>
  );
}