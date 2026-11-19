import { useEffect, useLayoutEffect, useMemo, useRef, useState, type ReactNode } from "react";
import { Canvas, useFrame, useThree } from "@react-three/fiber";
import { Line, OrbitControls } from "@react-three/drei";
import * as THREE from "three";
import type { Layer, SliceData } from "../../lib/demo-context";
import { featureColor } from "../../lib/gcode-types";
import {
  boxSize,
  fitToPoints,
  layerPoints,
  modelBounds,
  modelPoints,
  type Box,
  type CameraFit,
} from "../../lib/model-fit";
import { FeatureLegend } from "./FeatureLegend";
import { ExtrusionLayers } from "./ExtrusionMesh";

type Bbox = Box;

/** The handful of OrbitControls members the camera fit touches. Typing this
 *  locally keeps the drei component out of the type position (which it does
 *  not fit: `OrbitControls` is a value there). */
type Controls = {
  target: THREE.Vector3;
  minDistance: number;
  maxDistance: number;
  autoRotate: boolean;
  autoRotateSpeed: number;
  update?: () => void;
};

function sceneBbox(data: SliceData): Bbox {
  return data.meta.bounds as Bbox;
}

function modelCenter(b: Bbox) {
  return { x: (b.minX + b.maxX) / 2, y: (b.minY + b.maxY) / 2, z: (b.minZ + b.maxZ) / 2 };
}

function segmentOnModel(seg: { points: number[][] }, bbox: Bbox, pad = 8) {
  return seg.points.every(([x, y]) => {
    return (
      x >= bbox.minX - pad &&
      x <= bbox.maxX + pad &&
      y >= bbox.minY - pad &&
      y <= bbox.maxY + pad
    );
  });
}

function pathSpan(pts: THREE.Vector3[]) {
  let minX = Infinity,
    maxX = -Infinity;
  for (const p of pts) {
    minX = Math.min(minX, p.x);
    maxX = Math.max(maxX, p.x);
  }
  return maxX - minX;
}

function polylineLength(pts: THREE.Vector3[]) {
  let d = 0;
  for (let i = 1; i < pts.length; i++) d += pts[i].distanceTo(pts[i - 1]);
  return d;
}

function BedPlate({ size }: { size: number }) {
  // The plate is sized to run past the frame: the grid has to read as a surface
  // that continues beyond the edge, not as a tile the part happens to sit on.
  const show = size;
  return (
    <group>
      <mesh rotation={[-Math.PI / 2, 0, 0]} position={[0, 0.01, 0]} receiveShadow>
        <planeGeometry args={[show, show]} />
        <meshStandardMaterial color="#0c1018" metalness={0.5} roughness={0.48} />
      </mesh>
      <gridHelper args={[show, Math.round(show / 10), "#2e4a6a", "#1a2430"]} position={[0, 0.03, 0]} />
    </group>
  );
}

function GhostLayers({
  layers,
  maxLayer,
  center,
  showTravel,
  bbox,
}: {
  layers: Layer[];
  maxLayer: number;
  center: { x: number; y: number };
  showTravel: boolean;
  bbox: Bbox;
}) {
  return (
    <group>
      {layers.slice(0, maxLayer).map((lay, li) =>
        lay.segments.map((seg, si) => {
          if (seg.type === "travel") return null;
          if (seg.type === "skirt") return null;
          if (!segmentOnModel(seg, bbox)) return null;
          const pts = seg.points
            .filter((p, i, arr) => {
              if (i === 0) return true;
              return Math.hypot(p[0] - arr[i - 1][0], p[1] - arr[i - 1][1]) < 12;
            })
            .map(([x, y]) => new THREE.Vector3(x - center.x, lay.z, y - center.y));
          if (pts.length < 2) return null;
          if (pathSpan(pts) > 70) return null;
          return (
            <Line
              key={`g-${li}-${si}`}
              points={pts}
              color={featureColor(seg.type)}
              lineWidth={1}
              transparent
              opacity={0.2 + (li / Math.max(1, maxLayer)) * 0.15}
            />
          );
        }),
      )}
    </group>
  );
}

function LineLayers({
  layers,
  maxLayer,
  center,
  showTravel,
  buildProgress,
  singleLayer,
  bbox,
  footprint,
}: {
  layers: Layer[];
  maxLayer: number;
  center: { x: number; y: number };
  showTravel: boolean;
  buildProgress: number;
  singleLayer?: boolean;
  bbox: Bbox;
  footprint: number;
}) {
  return (
    <group>
      {layers.map((lay, li) => {
        if (li > maxLayer) return null;
        if (singleLayer && li !== maxLayer) return null;
        const active = li === maxLayer;
        return lay.segments.map((seg, si) => {
          if (seg.type === "travel" && !showTravel) return null;
          if (seg.type === "skirt") return null;
          if (!segmentOnModel(seg, bbox)) return null;
          let pts = seg.points;
          if (active && buildProgress < 1) {
            pts = pts.slice(0, Math.max(2, Math.floor(pts.length * buildProgress)));
          }
          const vecs = pts
            .filter((p, i, arr) => {
              if (i === 0) return true;
              return Math.hypot(p[0] - arr[i - 1][0], p[1] - arr[i - 1][1]) < 12;
            })
            .map(([x, y]) => new THREE.Vector3(x - center.x, lay.z, y - center.y));
          if (vecs.length < 2) return null;
          if (pathSpan(vecs) > 70) return null;
          if (seg.type === "travel" && polylineLength(vecs) > 50) return null;
          const limitR = Math.max(28, footprint * 0.85);
          if (vecs.some((v) => Math.hypot(v.x, v.z) > limitR)) return null;
          const width =
            seg.type === "wall-outer" ? 2.5 : seg.type === "wall-inner" ? 2.2 : seg.type === "skin" ? 2 : 1.6;
          return (
            <Line
              key={`l-${li}-${si}`}
              points={vecs}
              color={featureColor(seg.type)}
              lineWidth={active ? width : 1.5}
              transparent
              opacity={seg.type === "travel" ? 0.35 : active ? 0.95 : 0.25}
            />
          );
        });
      })}
    </group>
  );
}

function NozzleAlongLayer({
  layers,
  layerIndex,
  center,
  buildProgress,
}: {
  layers: Layer[];
  layerIndex: number;
  center: { x: number; y: number };
  buildProgress: number;
}) {
  const ref = useRef<THREE.Mesh>(null);
  const lay = layers[Math.min(layerIndex, layers.length - 1)];
  const tip = (() => {
    if (!lay) return new THREE.Vector3(0, 0.4, 0);
    const extrude = lay.segments.filter((s) => s.type !== "travel" && s.type !== "skirt");
    let pts: number[][] = [];
    for (const s of extrude) pts = pts.concat(s.points);
    if (!pts.length) return new THREE.Vector3(0, lay.z + 0.5, 0);
    const idx = Math.min(pts.length - 1, Math.floor(pts.length * buildProgress));
    const p = pts[idx];
    return new THREE.Vector3(p[0] - center.x, lay.z + 0.55, p[1] - center.y);
  })();

  useFrame((_, dt) => {
    if (!ref.current) return;
    ref.current.position.lerp(tip, 1 - Math.pow(0.0008, dt));
  });

  const glow = featureColor("skin");
  return (
    <group>
      <mesh ref={ref} position={tip}>
        <sphereGeometry args={[0.35, 12, 12]} />
        <meshStandardMaterial emissive={glow} emissiveIntensity={2} color="#1a1a1a" />
      </mesh>
      <pointLight position={tip} intensity={0.9} distance={22} color={glow} />
    </group>
  );
}

function FitCamera({ fit, hero }: { fit: CameraFit; hero?: boolean }) {
  const { camera } = useThree();
  const controls = useThree((s) => s.controls) as Controls | null;

  useLayoutEffect(() => {
    const persp = camera as THREE.PerspectiveCamera;
    persp.position.set(fit.position[0], fit.position[1], fit.position[2]);
    persp.near = Math.max(0.1, fit.distance / 200);
    persp.far = fit.distance * 40;
    persp.lookAt(fit.target[0], fit.target[1], fit.target[2]);
    persp.updateProjectionMatrix();

    if (controls) {
      controls.target.set(fit.target[0], fit.target[1], fit.target[2]);
      controls.minDistance = fit.distance * 0.35;
      controls.maxDistance = fit.distance * 3.4;
      controls.autoRotate = !!hero;
      controls.autoRotateSpeed = 0.35;
      controls.update?.();
    }
  }, [fit, camera, hero, controls]);

  return null;
}

function SceneInner({
  data,
  layerIndex,
  showTravel,
  buildProgress,
  hero,
  singleLayer,
  linesOnly,
  fill = 0.66,
}: {
  data: SliceData;
  layerIndex: number;
  showTravel: boolean;
  buildProgress: number;
  hero?: boolean;
  singleLayer?: boolean;
  linesOnly?: boolean;
  fill?: number;
}) {
  const modelBbox = sceneBbox(data);
  const center = modelCenter(modelBbox);
  const maxLayer = Math.min(layerIndex, data.layers.length - 1);

  // Frame the part, not the slicer's idea of it: modelBounds() drops the prime
  // line and anything else the renderer would not draw, so the camera can sit
  // close enough that the layer lines and the infill stay legible.
  const partBox = useMemo(() => modelBounds(data.layers, modelBbox) ?? modelBbox, [data, modelBbox]);
  const [partW, partD] = boxSize(partBox);
  const footprint = Math.max(partW, partD);

  const camera = useThree((s) => s.camera) as THREE.PerspectiveCamera;
  const gl = useThree((s) => s.gl);
  const fov = camera.fov || 40;

  // Measure the canvas here rather than trusting the size R3F reports: a panel
  // that is laid out after the first frame (a tab that was hidden, a viewer
  // that grew once its legend rendered) fits itself to the old height and the
  // part ends up cropped on one axis.
  const [aspect, setAspect] = useState(1);
  useLayoutEffect(() => {
    const el = gl.domElement;
    const measure = () => {
      const rect = el.getBoundingClientRect();
      if (rect.width > 1 && rect.height > 1) setAspect(rect.width / rect.height);
    };
    measure();
    const ro = new ResizeObserver(measure);
    ro.observe(el);
    return () => ro.disconnect();
  }, [gl]);

  // ~32 degrees of elevation, a little off the axis: high enough to see the
  // stack of layers, low enough that the walls still read as lines instead of
  // merging into a solid top. The landing hero keeps the angle it had.
  const direction = useMemo<[number, number, number]>(
    () => (hero ? [0.627, 0.384, 0.679] : [0.424, 0.53, 0.734]),
    [hero],
  );

  const partPoints = useMemo(
    () => modelPoints(data.layers, center, modelBbox),
    [data, center.x, center.y, modelBbox],
  );

  // A single-layer panel is showing one layer, so that layer is what gets
  // framed — otherwise a 13 mm cross-section of the stem sits in a frame sized
  // for the whole 40 mm wing. The floor keeps an almost empty layer from
  // pulling the camera inside the part.
  const framePoints = useMemo(
    () => (singleLayer ? layerPoints(data.layers[maxLayer], center) : partPoints),
    [singleLayer, data, maxLayer, center.x, center.y, partPoints],
  );

  const partFit = useMemo(
    () => fitToPoints(partPoints, { fov, aspect, fill: 0.66, direction }),
    [partPoints, fov, aspect, direction],
  );

  const fit = useMemo(
    () =>
      singleLayer
        ? fitToPoints(framePoints, { fov, aspect, fill, direction, minExtent: 6 })
        : partFit,
    [singleLayer, framePoints, partFit, fov, aspect, fill, direction],
  );

  // Wide panels see a wide slice of the world, so a plate sized off the part
  // alone floats in the middle of the canvas. Size it off the whole part's
  // frame (not the current layer's) so it does not change size while the layer
  // slider moves, and let the far edges fall outside.
  const plateSize = hero
    ? footprint * 2.1 // the landing hero keeps the plate it was composed with
    : Math.max(
        footprint * 2.4,
        2 * partFit.distance * Math.tan((fov * Math.PI) / 360) * Math.max(1, aspect) * 1.15,
      );

  return (
    <>
      <color attach="background" args={["#04060a"]} />
      <ambientLight intensity={0.55} />
      <directionalLight position={[50, 90, 35]} intensity={1.1} color="#f0f4ff" />
      <directionalLight position={[-40, 60, -50]} intensity={0.3} />
      <BedPlate size={plateSize} />
      {linesOnly ? (
        <LineLayers
          layers={data.layers}
          maxLayer={maxLayer}
          center={center}
          showTravel={showTravel}
          buildProgress={buildProgress}
          singleLayer={singleLayer}
          bbox={modelBbox}
          footprint={footprint}
        />
      ) : (
        <>
          {!singleLayer && (
            <GhostLayers
              layers={data.layers}
              maxLayer={maxLayer}
              center={center}
              showTravel={showTravel}
              bbox={modelBbox}
            />
          )}
          <ExtrusionLayers
            layers={data.layers}
            maxLayer={maxLayer}
            center={center}
            showTravel={showTravel}
            buildProgress={buildProgress}
            singleLayer={singleLayer}
            bbox={modelBbox}
          />
        </>
      )}
      <NozzleAlongLayer
        layers={data.layers}
        layerIndex={maxLayer}
        center={center}
        buildProgress={buildProgress}
      />
      <FitCamera fit={fit} hero={hero} />
      <OrbitControls
        makeDefault
        enablePan={!hero}
        enableDamping
        dampingFactor={0.07}
        maxPolarAngle={Math.PI / 2.05}
      />
    </>
  );
}

function ParallaxRig({ children }: { children: ReactNode }) {
  const group = useRef<THREE.Group>(null);
  const { pointer } = useThree();
  useFrame(() => {
    if (!group.current) return;
    group.current.rotation.y = pointer.x * 0.04;
    group.current.rotation.x = pointer.y * 0.015;
  });
  return <group ref={group}>{children}</group>;
}

function CanvasResize({ root }: { root: React.RefObject<HTMLDivElement | null> }) {
  const invalidate = useThree((s) => s.invalidate);
  useLayoutEffect(() => {
    const el = root.current;
    if (!el) return;
    const ro = new ResizeObserver(() => invalidate());
    ro.observe(el);
    return () => ro.disconnect();
  }, [root, invalidate]);
  return null;
}

export function GcodeViewer({
  data,
  layerIndex,
  showTravel,
  buildProgress = 1,
  className = "",
  hero = false,
  showLegend = true,
  singleLayer = false,
  linesOnly = false,
  fill = 0.66,
}: {
  data: SliceData | null;
  layerIndex: number;
  showTravel: boolean;
  buildProgress?: number;
  className?: string;
  hero?: boolean;
  showLegend?: boolean;
  /** Fraction of the tighter canvas axis the part should fill. */
  fill?: number;
  /** When true, only the active layer is drawn (slicer-style hatch view). */
  singleLayer?: boolean;
  /** Line-only preview (more reliable in nested layouts). */
  linesOnly?: boolean;
}) {
  const rootRef = useRef<HTMLDivElement>(null);
  const keepMounted = className.includes("gcode-viewer--always");
  const [inView, setInView] = useState(keepMounted);

  useLayoutEffect(() => {
    if (keepMounted) return;
    const el = rootRef.current;
    if (!el) return;
    const measure = () => {
      const r = el.getBoundingClientRect();
      const pad = 160;
      const visH = Math.min(r.bottom, window.innerHeight) - Math.max(r.top, 0);
      setInView(r.bottom > -pad && r.top < window.innerHeight + pad && visH > 24);
    };
    measure();
    requestAnimationFrame(measure);
    window.addEventListener("scroll", measure, { passive: true });
    window.addEventListener("resize", measure);
    return () => {
      window.removeEventListener("scroll", measure);
      window.removeEventListener("resize", measure);
    };
  }, [keepMounted, data]);

  const glLive = keepMounted || inView;

  useEffect(() => {
    if (!data || !glLive) return;
    const id = window.setTimeout(() => window.dispatchEvent(new Event("resize")), 80);
    return () => window.clearTimeout(id);
  }, [data, glLive, layerIndex]);

  return (
    <div
      ref={rootRef}
      className={`gcode-viewer ${hero ? "gcode-viewer--hero" : ""} ${className} ${!data ? "gcode-viewer--loading" : ""}`}
    >
      {!data ? (
        <p className="gcode-viewer__sleep dim">Loading G-code preview…</p>
      ) : (
        <>
      {showLegend && <FeatureLegend showTravel={showTravel} />}
      {!glLive ? (
        <p className="gcode-viewer__sleep dim">Preview paused off-screen</p>
      ) : (
      <Canvas
        dpr={
          typeof window !== "undefined"
            ? [1, Math.min(2, window.devicePixelRatio || 1)]
            : [1, 1]
        }
        frameloop={linesOnly ? "always" : "demand"}
        resize={{ scroll: false, debounce: { scroll: 0, resize: 0 } }}
        camera={{ fov: 40, near: 0.5, far: 900 }}
        gl={{ antialias: true, powerPreference: "high-performance" }}
        style={{ position: "absolute", inset: 0, width: "100%", height: "100%" }}
        onCreated={({ gl, invalidate }) => {
          const kick = () => {
            invalidate();
            requestAnimationFrame(invalidate);
          };
          kick();
          window.setTimeout(kick, 120);
          window.setTimeout(kick, 400);
          gl.domElement.addEventListener("webglcontextlost", (e) => e.preventDefault());
        }}
      >
        <CanvasResize root={rootRef} />
        {hero ? (
          <ParallaxRig>
            <SceneInner
              data={data}
              layerIndex={layerIndex}
              showTravel={showTravel}
              buildProgress={buildProgress}
              hero
              singleLayer={singleLayer}
              linesOnly={linesOnly}
              fill={fill}
            />
          </ParallaxRig>
        ) : (
          <SceneInner
            data={data}
            layerIndex={layerIndex}
            showTravel={showTravel}
            buildProgress={buildProgress}
            hero={false}
            singleLayer={singleLayer}
            linesOnly={linesOnly}
            fill={fill}
          />
        )}
      </Canvas>
      )}
      {hero && data && (
        <p className="gcode-viewer__caption mono">
          Real CuraEngine G-code · Shale · layer {layerIndex + 1}/{data.layers.length}
        </p>
      )}
        </>
      )}
    </div>
  );
}