import { useEffect, useMemo, useState } from "react";
import { useDemo } from "../../lib/demo-context";
import { GcodeViewer } from "./GcodeViewer";

export function HeroPrintScene() {
  const { data, layer, setLayer, showTravel } = useDemo();
  const [buildProgress, setBuildProgress] = useState(0.2);
  const [scrollLayer, setScrollLayer] = useState<number | null>(null);

  const heroLayerDefault = useMemo(
    () => (data ? Math.floor(data.layers.length * 0.92) : 0),
    [data],
  );

  const displayLayer = scrollLayer ?? heroLayerDefault;

  useEffect(() => {
    if (!data) return;
    setLayer(heroLayerDefault);
    let prog = 0.2;
    const id = window.setInterval(() => {
      prog += 0.022;
      if (prog >= 1) prog = 0.12;
      setBuildProgress(prog);
    }, 50);
    return () => clearInterval(id);
  }, [data, heroLayerDefault, setLayer]);

  useEffect(() => {
    const onScroll = () => {
      if (!data) return;
      if (window.scrollY < 48) {
        setScrollLayer(null);
        setLayer(heroLayerDefault);
        return;
      }
      const t = Math.min(1, Math.max(0, window.scrollY / Math.min(800, window.innerHeight)));
      const idx = Math.floor(t * (data.layers.length - 1));
      setScrollLayer(idx);
      setLayer(idx);
      setBuildProgress(1);
    };
    window.addEventListener("scroll", onScroll, { passive: true });
    onScroll();
    return () => window.removeEventListener("scroll", onScroll);
  }, [data, setLayer, heroLayerDefault]);

  return (
    <div className="hero-print" data-synthetic aria-label="Real CuraEngine G-code preview of t-wing">
      <GcodeViewer
        data={data}
        layerIndex={displayLayer}
        showTravel={showTravel}
        buildProgress={buildProgress}
        hero
        showLegend={false}
        className="hero-print__canvas"
      />
      <div className="hero-print__chrome">
        <span className="hero-print__tag">Real CuraEngine G-code · Shale</span>
        <span className="hero-print__layer">
          Layer {displayLayer + 1}
          {data ? ` / ${data.layers.length}` : ""}
        </span>
      </div>
    </div>
  );
}