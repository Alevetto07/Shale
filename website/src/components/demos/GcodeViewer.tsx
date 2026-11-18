import { useLayoutEffect, useMemo, useRef, type ReactNode } from "react";
import { Canvas, useFrame, useThree } from "@react-three/fiber";
import { OrbitControls, Line } from "@react-three/drei";
import * as THREE from "three";
import type { Layer, SliceData } from "../../lib/demo-context";
import { featureColor } from "../../lib/gcode-types";
import { FeatureLegend } from "./FeatureLegend";

type Bbox = { minX: number; maxX: number; minY: number; maxY: number; minZ: number; maxZ: number };

function sceneBbox(data: SliceData): Bbox {
  const b = data.meta.bounds as Bbox;
  return b;
}

function modelCenter(b: Bbox) {
  return { x: (b.minX + b.maxX) / 2, y: (b.minY + b.maxY) / 2, z: (b.minZ + b.maxZ) / 2 };
}

function BedPlate({ footprint, hero }: { footprint: number; hero?: boolean }) {
  const show = hero ? footprint * 1.9 : footprint * 2.4;
  return (
    <group>
      <mesh rotation={[-Math.PI / 2, 0, 0]} position={[0, 0.01, 0]} receiveShadow>
        <planeGeometry args={[show, show]} />
        <meshStandardMaterial color="#0c1018" metalness={0.55} roughness={0.42} />
      </mesh>
      <gridHelper
        args={[show, Math.round(show / 10), "#2e4a6a", "#1a2430"]}
        position={[0, 0.03, 0]}
      />
    </group>
  );
}

function LayerPaths({
  layers,
  maxLayer,
  center,
  showTravel,
  buildProgress,
}: {
  layers: Layer[];
  maxLayer: number;
  center: { x: number; y: number };
  showTravel: boolean;
  buildProgress: number;
}) {
  const visible = layers.slice(0, maxLayer + 1);
  return (
    <group>
      {visible.map((lay, li) => {
        const layerDim = li < maxLayer ? 0.55 + (li / Math.max(1, maxLayer)) * 0.35 : 1;
        return lay.segments.map((seg, si) => {
          if (seg.type === "travel" && !showTravel) return null;
          if (seg.type === "skirt") return null;
          let pts = seg.points.map(([x, y]) => new THREE.Vector3(x - center.x, lay.z, y - center.y));
          if (pts.length < 2) return null;
          if (li === maxLayer && buildProgress < 1) {
            const keep = Math.max(2, Math.floor(pts.length * buildProgress));
            pts = pts.slice(0, keep);
          }
          const col = featureColor(seg.type);
          const width =
            seg.type === "wall-outer" ? 5 : seg.type === "wall-inner" ? 4 : seg.type === "skin" ? 3.5 : 2.5;
          return (
            <Line
              key={`${li}-${si}-${seg.type}`}
              points={pts}
              color={col}
              lineWidth={width}
              transparent
              opacity={seg.type === "travel" ? 0.35 : (0.55 + 0.45 * layerDim) * (seg.type === "infill" ? 0.85 : 1)}
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
  const tip = useMemo(() => {
    const lay = layers[Math.min(layerIndex, layers.length - 1)];
    if (!lay) return new THREE.Vector3(0, 0.4, 0);
    const extrude = lay.segments.filter((s) => s.type !== "travel" && s.type !== "skirt");
    let pts: number[][] = [];
    for (const s of extrude) pts = pts.concat(s.points);
    if (!pts.length) return new THREE.Vector3(0, lay.z + 0.5, 0);
    const idx = Math.min(pts.length - 1, Math.floor(pts.length * buildProgress));
    const p = pts[idx];
    return new THREE.Vector3(p[0] - center.x, lay.z + 0.55, p[1] - center.y);
  }, [layers, layerIndex, center, buildProgress]);

  useFrame((_, dt) => {
    if (!ref.current) return;
    ref.current.position.lerp(tip, 1 - Math.pow(0.0008, dt));
  });

  const glow = featureColor("infill");
  return (
    <group>
      <mesh ref={ref} position={tip}>
        <sphereGeometry args={[0.75, 14, 14]} />
        <meshStandardMaterial emissive={glow} emissiveIntensity={2.5} color="#121212" />
      </mesh>
      <pointLight position={tip} intensity={1.2} distance={28} color={glow} />
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
}: {
  data: SliceData;
  layerIndex: number;
  showTravel: boolean;
  buildProgress: number;
  hero?: boolean;
}) {
  const bbox = sceneBbox(data);
  const center = modelCenter(bbox);
  const maxLayer = Math.min(layerIndex, data.layers.length - 1);
  const footprint = Math.max(bbox.maxX - bbox.minX, bbox.maxY - bbox.minY);

  const centeredBbox: Bbox = {
    minX: -footprint / 2,
    maxX: footprint / 2,
    minY: -((bbox.maxY - bbox.minY) / 2),
    maxY: (bbox.maxY - bbox.minY) / 2,
    minZ: bbox.minZ,
    maxZ: bbox.maxZ,
  };

  return (
    <>
      <color attach="background" args={["#04060a"]} />
      <fog attach="fog" args={["#04060a", 80, 220]} />
      <ambientLight intensity={0.42} />
      <directionalLight position={[50, 90, 35]} intensity={1.25} color="#f0f4ff" />
      <directionalLight position={[-40, 60, -50]} intensity={0.35} />
      <BedPlate footprint={footprint} hero={hero} />
      <LayerPaths
        layers={data.layers}
        maxLayer={maxLayer}
        center={center}
        showTravel={showTravel}
        buildProgress={buildProgress}
      />
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
    group.current.rotation.y = pointer.x * 0.05;
    group.current.rotation.x = pointer.y * 0.02;
  });
  return <group ref={group}>{children}</group>;
}

export function GcodeViewer({
  data,
  layerIndex,
  showTravel,
  buildProgress = 1,
  className = "",
  hero = false,
  showLegend = true,
}: {
  data: SliceData | null;
  layerIndex: number;
  showTravel: boolean;
  buildProgress?: number;
  className?: string;
  hero?: boolean;
  showLegend?: boolean;
}) {
  if (!data) {
    return <div className={`gcode-viewer gcode-viewer--loading ${className}`}>Loading G-code preview…</div>;
  }

  return (
    <div className={`gcode-viewer ${hero ? "gcode-viewer--hero" : ""} ${className}`}>
      {showLegend && !hero && <FeatureLegend showTravel={showTravel} />}
      <Canvas
        dpr={[1, 1.4]}
        camera={{ fov: 40, near: 0.5, far: 900 }}
        gl={{ antialias: true, powerPreference: "high-performance" }}
      >
        {hero ? (
          <ParallaxRig>
            <SceneInner
              data={data}
              layerIndex={layerIndex}
              showTravel={showTravel}
              buildProgress={buildProgress}
              hero
            />
          </ParallaxRig>
        ) : (
          <SceneInner
            data={data}
            layerIndex={layerIndex}
            showTravel={showTravel}
            buildProgress={buildProgress}
            hero={false}
          />
        )}
      </Canvas>
      {hero && (
        <p className="gcode-viewer__caption mono">
          Real CuraEngine G-code · Shale · layer {layerIndex + 1}/{data.layers.length}
        </p>
      )}
    </div>
  );
}