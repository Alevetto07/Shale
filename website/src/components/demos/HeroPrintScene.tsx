import { useEffect, useState } from "react";
import { useDemo } from "../../lib/demo-context";
import { GcodeViewer } from "./GcodeViewer";

export function HeroPrintScene() {
  const { data, plateMm, layer, setLayer, infill } = useDemo();
  const [heroLayer, setHeroLayer] = useState(0);
  useEffect(() => {
    if (!data) return;
    const id = window.setInterval(() => {
      setHeroLayer((prev) => (prev >= data.layers.length - 1 ? 0 : prev + 1));
    }, 120);
    return () => window.clearInterval(id);
  }, [data]);

  useEffect(() => {
    const onScroll = () => {
      if (!data) return;
      const y = window.scrollY;
      const span = Math.min(900, window.innerHeight * 1.2);
      const t = Math.min(1, Math.max(0, y / span));
      const idx = Math.floor(t * (data.layers.length - 1));
      setHeroLayer(idx);
      setLayer(idx);
    };
    window.addEventListener("scroll", onScroll, { passive: true });
    onScroll();
    return () => window.removeEventListener("scroll", onScroll);
  }, [data, setLayer]);

  const active = data ? heroLayer : layer;

  return (
    <div className="hero-print" data-synthetic aria-label="Illustrative layer-build preview of examples/t-wing.stl">
      <GcodeViewer data={data} plateMm={plateMm} layerIndex={active} infill={infill} hero className="hero-print__canvas" />
      <div className="hero-print__chrome">
        <span className="hero-print__tag">Illustrative · t-wing.stl</span>
        <span className="hero-print__layer">
          Layer {active + 1}
          {data ? ` / ${data.layers.length}` : ""}
        </span>
      </div>
    </div>
  );
}