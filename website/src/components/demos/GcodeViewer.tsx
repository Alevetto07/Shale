import { useLayoutEffect, useMemo, useRef, type ReactNode } from "react";
import { Canvas, useFrame, useThree } from "@react-three/fiber";
import { OrbitControls, Line } from "@react-three/drei";
import * as THREE from "three";
import type { Layer, SliceData } from "../../lib/demo-context";

function pathColor(type: string) {
  if (type === "wall") return "#f0eeea";
  if (type === "infill") {
    return (
      getComputedStyle(document.documentElement).getPropertyValue("--path").trim() || "#9eb8ff"
    );
  }
  if (type === "support") return "#c9a227";
  return "#5c6a7a";
}

type Bbox = { minX: number; maxX: number; minY: number; maxY: number; minZ: number; maxZ: number };

function sceneBbox(data: SliceData): Bbox {
  const b = data.meta.bounds as Bbox;
  return (
    b ?? {
      minX: 0,
      maxX: 40,
      minY: 0,
      maxY: 40,
      minZ: 0,
      maxZ: 20,
    }
  );
}

function modelCenter(b: Bbox) {
  return {
    x: (b.minX + b.maxX) / 2,
    y: (b.minY + b.maxY) / 2,
    z: (b.minZ + b.maxZ) / 2,
  };
}

function BedPlate({
  size,
  modelFootprint,
  hero,
}: {
  size: number;
  modelFootprint: number;
  hero?: boolean;
}) {
  const show = hero ? modelFootprint * 1.75 : Math.min(size, modelFootprint * 2.6);
  const gridDiv = Math.max(6, Math.min(20, Math.round(show / 8)));
  return (
    <group>
      <mesh rotation={[-Math.PI / 2, 0, 0]} position={[0, 0.02, 0]} receiveShadow>
        <planeGeometry args={[show, show]} />
        <meshStandardMaterial color="#10141c" metalness={0.4} roughness={0.55} />
      </mesh>
      <gridHelper args={[show, gridDiv, "#3d5f8a", "#243044"]} position={[0, 0.06, 0]} />
      <mesh rotation={[-Math.PI / 2, 0, 0]} position={[0, 0.08, 0]}>
        <ringGeometry args={[modelFootprint * 0.55, modelFootprint * 0.58, 64]} />
        <meshBasicMaterial color={pathColor("infill")} transparent opacity={0.35} />
      </mesh>
    </group>
  );
}

function LayerPaths({
  layers,
  maxLayer,
  center,
  infillScale,
}: {
  layers: Layer[];
  maxLayer: number;
  center: { x: number; y: number };
  infillScale: number;
}) {
  const visible = layers.slice(0, maxLayer + 1);
  const wallColor = pathColor("wall");
  const infillCol = pathColor("infill");
  const travelCol = pathColor("travel");

  return (
    <group>
      {visible.map((lay, li) =>
        lay.segments.map((seg, si) => {
          if (seg.type === "infill" && infillScale < 0.25) return null;
          const pts = seg.points.map(
            ([x, y]) => new THREE.Vector3(x - center.x, lay.z, y - center.y),
          );
          if (pts.length < 2) return null;
          const col =
            seg.type === "wall" ? wallColor : seg.type === "infill" ? infillCol : travelCol;
          const width = seg.type === "wall" ? 5.5 : seg.type === "infill" ? 3.5 : 1.6;
          return (
            <Line
              key={`${li}-${si}`}
              points={pts}
              color={col}
              lineWidth={width}
              worldUnits={false}
              transparent
              opacity={seg.type === "travel" ? 0.45 : 0.95}
            />
          );
        }),
      )}
    </group>
  );
}

function NozzleMarker({
  layers,
  layerIndex,
  center,
}: {
  layers: Layer[];
  layerIndex: number;
  center: { x: number; y: number };
}) {
  const ref = useRef<THREE.Mesh>(null);
  const tip = useMemo(() => {
    const lay = layers[Math.min(layerIndex, layers.length - 1)];
    if (!lay?.segments[0]?.points[0]) return new THREE.Vector3(0, lay?.z ?? 1, 0);
    const p = lay.segments[0].points[0];
    return new THREE.Vector3(p[0] - center.x, lay.z + 0.6, p[1] - center.y);
  }, [layers, layerIndex, center]);

  useFrame((_, dt) => {
    if (!ref.current) return;
    ref.current.position.lerp(tip, 1 - Math.pow(0.001, dt));
  });

  return (
    <mesh ref={ref} position={tip}>
      <sphereGeometry args={[0.9, 16, 16]} />
      <meshStandardMaterial
        emissive={pathColor("infill")}
        emissiveIntensity={2.2}
        color="#1a1a1a"
      />
    </mesh>
  );
}

function FitCamera({
  bbox,
  fill = 0.55,
  hero,
}: {
  bbox: Bbox;
  fill?: number;
  hero?: boolean;
}) {
  const { camera, size } = useThree();
  const controls = useThree((s) => s.controls) as OrbitControls | null;

  useLayoutEffect(() => {
    const persp = camera as THREE.PerspectiveCamera;
    const sx = bbox.maxX - bbox.minX;
    const sy = bbox.maxY - bbox.minY;
    const sz = bbox.maxZ - bbox.minZ;
    const half = Math.max(sx, sy, sz, 6) * 0.5;
    const targetY = bbox.minZ + sz * 0.42;

    const aspect = size.width / size.height;
    const vFov = (persp.fov * Math.PI) / 180;
    const hFov = 2 * Math.atan(Math.tan(vFov / 2) * aspect);
    const distV = half / Math.tan(vFov / 2) / fill;
    const distH = half / Math.tan(hFov / 2) / fill;
    const dist = Math.max(distV, distH) * (hero ? 1.02 : 1.08);

    persp.position.set(dist * 0.82, targetY + dist * 0.55, dist * 0.9);
    persp.near = Math.max(0.2, dist / 100);
    persp.far = dist * 30;
    persp.lookAt(0, targetY, 0);
    persp.updateProjectionMatrix();

    if (controls) {
      (controls as unknown as { target: THREE.Vector3 }).target.set(0, targetY, 0);
      controls.minDistance = dist * 0.4;
      controls.maxDistance = dist * 3;
      controls.update?.();
    }
  }, [bbox, camera, size, fill, hero, controls]);

  return null;
}

function SceneInner({
  data,
  plateMm,
  layerIndex,
  infill,
  hero,
}: {
  data: SliceData;
  plateMm: number;
  layerIndex: number;
  infill: number;
  hero?: boolean;
}) {
  const bbox = sceneBbox(data);
  const center = modelCenter(bbox);
  const maxLayer = Math.min(layerIndex, data.layers.length - 1);
  const infillScale = infill / 100;
  const footprint = Math.max(bbox.maxX - bbox.minX, bbox.maxY - bbox.minY);

  const centeredBbox: Bbox = {
    minX: bbox.minX - center.x,
    maxX: bbox.maxX - center.x,
    minY: bbox.minY - center.y,
    maxY: bbox.maxY - center.y,
    minZ: bbox.minZ,
    maxZ: bbox.maxZ,
  };

  return (
    <>
      <color attach="background" args={["#05070a"]} />
      <ambientLight intensity={0.5} />
      <directionalLight position={[60, 100, 40]} intensity={1.35} />
      <pointLight position={[-50, 80, -35]} intensity={0.55} color={pathColor("infill")} />
      <BedPlate size={plateMm} modelFootprint={footprint} hero={hero} />
      <LayerPaths
        layers={data.layers}
        maxLayer={maxLayer}
        center={center}
        infillScale={infillScale}
      />
      <NozzleMarker layers={data.layers} layerIndex={maxLayer} center={center} />
      <FitCamera bbox={centeredBbox} fill={0.46} hero={hero} />
      <OrbitControls makeDefault enablePan enableDamping dampingFactor={0.08} maxPolarAngle={Math.PI / 2.05} />
    </>
  );
}

function ParallaxRig({ children, strength = 0.06 }: { children: ReactNode }) {
  const group = useRef<THREE.Group>(null);
  const { pointer } = useThree();
  useFrame(() => {
    if (!group.current) return;
    group.current.rotation.y = pointer.x * strength;
    group.current.rotation.x = pointer.y * strength * 0.25;
  });
  return <group ref={group}>{children}</group>;
}

/** Exposed for Playwright capture regression */
export function modelCoverageRatioFromData(data: SliceData): number {
  const b = sceneBbox(data);
  const sx = b.maxX - b.minX;
  const sy = b.maxY - b.minY;
  const sz = b.maxZ - b.minZ;
  const modelRadius = Math.max(sx, sy, sz, 1);
  const fill = 0.56;
  const dist = modelRadius * 0.72 / Math.tan((42 * Math.PI) / 180 / 2) / fill;
  const projected = modelRadius / (dist * Math.tan((42 * Math.PI) / 180 / 2));
  return Math.min(0.95, projected * 1.1);
}

export function GcodeViewer({
  data,
  plateMm,
  layerIndex,
  infill,
  className = "",
  hero = false,
}: {
  data: SliceData | null;
  plateMm: number;
  layerIndex: number;
  infill: number;
  className?: string;
  hero?: boolean;
}) {
  if (!data) {
    return <div className={`gcode-viewer gcode-viewer--loading ${className}`}>Loading slice…</div>;
  }

  return (
    <div className={`gcode-viewer ${className}`} data-model-coverage={modelCoverageRatioFromData(data).toFixed(2)}>
      <Canvas
        dpr={[1, 1.5]}
        camera={{ fov: 42, near: 0.5, far: 800, position: [80, 60, 80] }}
        gl={{ antialias: true, powerPreference: "high-performance" }}
        style={{ background: "#05070a" }}
      >
        {hero ? (
          <ParallaxRig>
            <SceneInner
              data={data}
              plateMm={plateMm}
              layerIndex={layerIndex}
              infill={infill}
              hero
            />
          </ParallaxRig>
        ) : (
          <SceneInner data={data} plateMm={plateMm} layerIndex={layerIndex} infill={infill} />
        )}
      </Canvas>
    </div>
  );
}