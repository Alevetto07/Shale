import { useMemo, useRef, type ReactNode } from "react";
import { Canvas, useFrame, useThree } from "@react-three/fiber";
import { OrbitControls, Line } from "@react-three/drei";
import * as THREE from "three";
import type { Layer, SliceData } from "../../lib/demo-context";

const COLORS: Record<string, string> = {
  wall: "#f0eeea",
  infill: "var(--path)",
  travel: "#5c6a7a",
  support: "#c9a227",
};

function pathColor(type: string) {
  if (type === "wall") return "#f0eeea";
  if (type === "infill") return getComputedStyle(document.documentElement).getPropertyValue("--path").trim() || "#9eb8ff";
  if (type === "support") return "#c9a227";
  return "#5c6a7a";
}

function BedPlate({ size }: { size: number }) {
  return (
    <group>
      <mesh rotation={[-Math.PI / 2, 0, 0]} position={[0, 0.02, 0]} receiveShadow>
        <planeGeometry args={[size, size]} />
        <meshStandardMaterial color="#141820" metalness={0.35} roughness={0.65} />
      </mesh>
      <gridHelper args={[size, Math.round(size / 10), "#3d4f68", "#232b38"]} position={[0, 0.04, 0]} />
    </group>
  );
}

function LayerPaths({
  layers,
  maxLayer,
  offset,
  infillScale,
}: {
  layers: Layer[];
  maxLayer: number;
  offset: [number, number];
  infillScale: number;
}) {
  const visible = layers.slice(0, maxLayer + 1);
  return (
    <group position={[offset[0], 0, offset[1]]}>
      {visible.map((lay, li) =>
        lay.segments.map((seg, si) => {
          if (seg.type === "infill" && infillScale < 0.35) return null;
          const pts = seg.points.map(([x, y]) => new THREE.Vector3(x, lay.z, y));
          if (pts.length < 2) return null;
          return (
            <Line
              key={`${li}-${si}`}
              points={pts}
              color={pathColor(seg.type)}
              lineWidth={seg.type === "wall" ? 2.2 : 1.2}
              transparent
              opacity={seg.type === "travel" ? 0.4 : 0.92}
            />
          );
        }),
      )}
    </group>
  );
}

function NozzleMarker({ layers, layerIndex }: { layers: Layer[]; layerIndex: number }) {
  const ref = useRef<THREE.Mesh>(null);
  const tip = useMemo(() => {
    const lay = layers[Math.min(layerIndex, layers.length - 1)];
    if (!lay?.segments[0]?.points[0]) return new THREE.Vector3(10, lay?.z ?? 1, 10);
    const p = lay.segments[0].points[0];
    return new THREE.Vector3(p[0], lay.z + 0.4, p[1]);
  }, [layers, layerIndex]);

  useFrame((_, dt) => {
    if (!ref.current) return;
    ref.current.position.lerp(tip, 1 - Math.pow(0.001, dt));
  });

  return (
    <mesh ref={ref} position={tip}>
      <sphereGeometry args={[0.55, 12, 12]} />
      <meshStandardMaterial emissive={pathColor("infill")} emissiveIntensity={1.4} color="#2a2a2a" />
    </mesh>
  );
}

function SceneInner({
  data,
  plateMm,
  layerIndex,
  infill,
}: {
  data: SliceData;
  plateMm: number;
  layerIndex: number;
  infill: number;
}) {
  const bounds = data.meta.bounds as { minX: number; maxX: number; minY: number; maxY: number };
  const cx = (bounds.minX + bounds.maxX) / 2 - plateMm / 2;
  const cy = (bounds.minY + bounds.maxY) / 2 - plateMm / 2;
  const maxLayer = Math.min(layerIndex, data.layers.length - 1);
  const infillScale = infill / 100;

  return (
    <>
      <ambientLight intensity={0.55} />
      <directionalLight position={[80, 120, 60]} intensity={1.1} castShadow />
      <pointLight position={[-40, 60, -30]} intensity={0.35} color={pathColor("infill")} />
      <BedPlate size={plateMm} />
      <LayerPaths
        layers={data.layers}
        maxLayer={maxLayer}
        offset={[-cx, -cy]}
        infillScale={infillScale}
      />
      <NozzleMarker layers={data.layers} layerIndex={maxLayer} />
      <OrbitControls makeDefault enablePan maxPolarAngle={Math.PI / 2.1} minDistance={40} maxDistance={320} />
    </>
  );
}

function ParallaxRig({ children, strength = 0.08 }: { children: ReactNode; strength?: number }) {
  const group = useRef<THREE.Group>(null);
  const { pointer } = useThree();
  useFrame(() => {
    if (!group.current) return;
    group.current.rotation.y = pointer.x * strength;
    group.current.rotation.x = pointer.y * strength * 0.35;
  });
  return <group ref={group}>{children}</group>;
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
    <div className={`gcode-viewer ${className}`}>
      <Canvas
        camera={{ position: hero ? [140, 110, 140] : [120, 95, 120], fov: 42, near: 0.1, far: 2000 }}
        gl={{ antialias: true, powerPreference: "high-performance" }}
      >
        {hero ? (
          <ParallaxRig>
            <SceneInner data={data} plateMm={plateMm} layerIndex={layerIndex} infill={infill} />
          </ParallaxRig>
        ) : (
          <SceneInner data={data} plateMm={plateMm} layerIndex={layerIndex} infill={infill} />
        )}
      </Canvas>
    </div>
  );
}