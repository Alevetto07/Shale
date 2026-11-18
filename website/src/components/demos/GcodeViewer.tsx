import { useEffect, useLayoutEffect, useRef, useState, type ReactNode } from "react";
import { Canvas, useFrame, useThree } from "@react-three/fiber";
import { Line, OrbitControls } from "@react-three/drei";
import * as THREE from "three";
import type { Layer, SliceData } from "../../lib/demo-context";
import { featureColor } from "../../lib/gcode-types";
import { FeatureLegend } from "./FeatureLegend";
import { ExtrusionLayers } from "./ExtrusionMesh";

type Bbox = { minX: number; maxX: number; minY: number; maxY: number; minZ: number; maxZ: number };

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

function BedPlate({ footprint }: { footprint: number }) {
  const show = footprint * 2.1;
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
  singleLayer: boolean;
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

function FitCamera({ bbox, fill = 0.46, hero }: { bbox: Bbox; fill?: number; hero?: boolean }) {
  const { camera, size } = useThree();
  const controls = useThree((s) => s.controls) as OrbitControls | null;

  useLayoutEffect(() => {
    const persp = camera as THREE.PerspectiveCamera;
    const sx = bbox.maxX - bbox.minX;
    const sy = bbox.maxY - bbox.minY;
    const sz = bbox.maxZ - bbox.minZ;
    const half = Math.max(sx, sy, sz, 6) * 0.5;
    const targetY = sz * 0.38;

    const aspect = size.width / size.height;
    const vFov = (persp.fov * Math.PI) / 180;
    const hFov = 2 * Math.atan(Math.tan(vFov / 2) * aspect);
    const distV = half / Math.tan(vFov / 2) / fill;
    const distH = half / Math.tan(hFov / 2) / fill;
    const dist = Math.max(distV, distH) * (hero ? 1.0 : 1.06);

    persp.position.set(dist * 0.85, targetY + dist * 0.52, dist * 0.92);
    persp.near = Math.max(0.2, dist / 120);
    persp.far = dist * 35;
    persp.lookAt(0, targetY, 0);
    persp.updateProjectionMatrix();

    if (controls) {
      controls.target.set(0, targetY, 0);
      controls.minDistance = dist * 0.38;
      controls.maxDistance = dist * 3.2;
      controls.autoRotate = !!hero;
      controls.autoRotateSpeed = 0.35;
      controls.update?.();
    }
  }, [bbox, camera, size, fill, hero, controls]);

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
}: {
  data: SliceData;
  layerIndex: number;
  showTravel: boolean;
  buildProgress: number;
  hero?: boolean;
  singleLayer?: boolean;
  linesOnly?: boolean;
}) {
  const modelBbox = sceneBbox(data);
  const center = modelCenter(modelBbox);
  const maxLayer = Math.min(layerIndex, data.layers.length - 1);
  const footprint = Math.max(modelBbox.maxX - modelBbox.minX, modelBbox.maxY - modelBbox.minY);

  const centeredBbox: Bbox = {
    minX: -footprint / 2,
    maxX: footprint / 2,
    minY: -((modelBbox.maxY - modelBbox.minY) / 2),
    maxY: (modelBbox.maxY - modelBbox.minY) / 2,
    minZ: modelBbox.minZ,
    maxZ: modelBbox.maxZ,
  };

  return (
    <>
      <color attach="background" args={["#04060a"]} />
      <ambientLight intensity={0.55} />
      <directionalLight position={[50, 90, 35]} intensity={1.1} color="#f0f4ff" />
      <directionalLight position={[-40, 60, -50]} intensity={0.3} />
      <BedPlate footprint={footprint} />
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
      <FitCamera bbox={centeredBbox} fill={0.48} hero={hero} />
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
}: {
  data: SliceData | null;
  layerIndex: number;
  showTravel: boolean;
  buildProgress?: number;
  className?: string;
  hero?: boolean;
  showLegend?: boolean;
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