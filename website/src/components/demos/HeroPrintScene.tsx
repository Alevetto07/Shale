import { useEffect, useState } from "react";
import { useDemo } from "../../lib/demo-context";
import { GcodeViewer } from "./GcodeViewer";

export function HeroPrintScene() {
  const { data, layer, setLayer, showTravel } = useDemo();
  const [heroLayer, setHeroLayer] = useState(0);
  const [buildProgress, setBuildProgress] = useState(1);

  useEffect(() => {
    if (!data) return;
    const hi = Math.floor(data.layers.length * 0.92);
    setHeroLayer(hi);
    setLayer(hi);
    let prog = 0.15;
    const id = window.setInterval(() => {
      prog += 0.022;
      if (prog >= 1) prog = 0.12;
      setBuildProgress(prog);
    }, 50);
    return () => clearInterval(id);
  }, [data, setLayer]);

  useEffect(() => {
    const onScroll = () => {
      if (!data) return;
      const hi = Math.floor(data.layers.length * 0.92);
      if (window.scrollY < 48) {
        setHeroLayer(hi);
        setLayer(hi);
        return;
      }
      const t = Math.min(1, Math.max(0, window.scrollY / Math.min(800, window.innerHeight)));
      const idx = Math.floor(t * (data.layers.length - 1));
      setHeroLayer(idx);
      setLayer(idx);
      setBuildProgress(1);
    };
    window.addEventListener("scroll", onScroll, { passive: true });
    onScroll();
    return () => window.removeEventListener("scroll", onScroll);
  }, [data, setLayer]);

  const active = data ? heroLayer : layer;

  return (
    <div className="hero-print" data-synthetic aria-label="Real CuraEngine G-code preview of t-wing">
      <GcodeViewer
        data={data}
        layerIndex={active}
        showTravel={showTravel}
        buildProgress={buildProgress}
        hero
        showLegend={false}
        className="hero-print__canvas"
      />
      <div className="hero-print__chrome">
        <span className="hero-print__tag">Real CuraEngine G-code · Shale</span>
        <span className="hero-print__layer">
          Layer {active + 1}
          {data ? ` / ${data.layers.length}` : ""}
        </span>
      </div>
    </div>
  );
}