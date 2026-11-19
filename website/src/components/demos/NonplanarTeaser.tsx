import { useEffect, useRef, useState } from "react";
import { useInView } from "motion/react";
import type { SliceData } from "../../lib/demo-context";
import { GcodeViewer } from "./GcodeViewer";

export default function NonplanarTeaser() {
  const root = useRef<HTMLElement>(null);
  const inView = useInView(root, { margin: "-15% 0px", once: true });
  const [planar, setPlanar] = useState<SliceData | null>(null);
  const [curved, setCurved] = useState<SliceData | null>(null);
  const [layer, setLayer] = useState(180);
  const [side, setSide] = useState(0);

  useEffect(() => {
    if (!inView) return;
    Promise.all([
      fetch("/demo/demo-slice-planar.json").then((r) => r.json()),
      fetch("/demo/demo-slice-curved.json").then((r) => r.json()),
    ]).then(([p, c]) => {
      setPlanar(p);
      setCurved(c);
      const maxL = Math.min(p.layers.length, c.layers.length) - 1;
      setLayer(Math.floor(maxL * 0.75));
    });
  }, [inView]);

  const max = Math.min(planar?.layers.length ?? 1, curved?.layers.length ?? 1) - 1;
  const active = side < 0.5 ? planar : curved;

  return (
    <section ref={root} id="coming-next" className="coming-next panel-path-frame">
      <div className="coming-next__head">
        <h2>Curved top surfaces</h2>
        <p className="muted">
          In development for v0.2 — same t-wing, 0.1 mm, planar G-code vs shale-nonplanar post-process (max 10°).
        </p>
      </div>
      <div className="compare-slider">
        <label className="compare-slider__control">
          <span>Planar</span>
          <input
            type="range"
            min={0}
            max={100}
            value={Math.round(side * 100)}
            onChange={(e) => setSide(Number(e.target.value) / 100)}
            aria-label="Compare planar and curved"
          />
          <span>Curved (v0.2)</span>
        </label>
        <input
          type="range"
          className="compare-layer"
          min={0}
          max={max}
          value={layer}
          onChange={(e) => setLayer(Number(e.target.value))}
          aria-label="Layer"
        />
      </div>
      <h3 className="compare-caption mono">
        {side < 0.5 ? "Planar tops · CuraEngine" : "Curved tops · in development"} · layer {layer + 1}
      </h3>
      <GcodeViewer
        data={active}
        layerIndex={layer}
        showTravel={false}
        className="coming-next__viewer"
        showLegend={false}
      />
    </section>
  );
}
