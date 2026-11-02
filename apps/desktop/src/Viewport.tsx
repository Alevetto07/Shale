import { OrbitControls, TransformControls } from "@react-three/drei";
import { Canvas, useThree } from "@react-three/fiber";
import { useEffect, useMemo, useState } from "react";
import * as THREE from "three";
import type { PreviewPath, SupportOverlay, Transform } from "./api";

export type TransformMode = "translate" | "rotate" | "scale";
export type ViewStyle = "solid" | "wire" | "ascii";

interface SceneModel {
  id: number;
  positions: number[];
  transform: Transform;
  selected: boolean;
}

interface ViewportProps {
  width: number;
  depth: number;
  height: number;
  centerIsZero: boolean;
  models: SceneModel[];
  toolpaths: PreviewPath[];
  overlays: SupportOverlay[];
  showModel: boolean;
  showToolpaths: boolean;
  viewStyle: ViewStyle;
  mode: TransformMode;
  onSelect: (id: number) => void;
  onTransform: (id: number, transform: Transform) => void;
}

const KIND_COLOR: Record<string, string> = {
  "wall-outer": "#f2f2f2",
  "wall-inner": "#b8b8b8",
  skin: "#8c8c8c",
  infill: "#6a6a6a",
  support: "#5c5c5c",
  "support-interface": "#9a9a9a",
  skirt: "#d4d4d4",
  brim: "#e4e4e4",
  raft: "#4a4a4a",
  "prime-tower": "#3a3a3a",
  travel: "#3e3e3e",
  extrusion: "#f2f2f2",
};

export function Viewport(props: ViewportProps) {
  return (
    <Canvas
      shadows={false}
      camera={{ position: [180, -240, 160], fov: 32, up: [0, 0, 1], near: 0.1, far: 8000 }}
      onPointerMissed={() => props.onSelect(-1)}
    >
      <color attach="background" args={["#0d0d0d"]} />
      <ambientLight intensity={0.7} />
      <directionalLight position={[120, -80, 220]} intensity={1.15} color="#ffffff" />
      <directionalLight position={[-80, 140, 40]} intensity={0.28} color="#c8c8c8" />
      <FrameBed
        width={props.width}
        depth={props.depth}
        height={props.height}
        centerIsZero={props.centerIsZero}
      />
      <Bed
        width={props.width}
        depth={props.depth}
        height={props.height}
        centerIsZero={props.centerIsZero}
      />
      {props.showModel &&
        props.models.map((model) => (
          <PrintModel
            key={model.id}
            model={model}
            viewStyle={props.viewStyle}
            mode={props.mode}
            onSelect={props.onSelect}
            onTransform={props.onTransform}
          />
        ))}
      {props.overlays.map((overlay) => (
        <SupportVolume key={overlay.id} overlay={overlay} />
      ))}
      {props.showToolpaths && <Toolpaths paths={props.toolpaths} />}
      <OrbitControls
        makeDefault
        enableDamping
        dampingFactor={0.12}
        target={[
          props.centerIsZero ? 0 : props.width / 2,
          props.centerIsZero ? 0 : props.depth / 2,
          0,
        ]}
      />
    </Canvas>
  );
}

function SupportVolume({ overlay }: { overlay: SupportOverlay }) {
  const geometry = useMemo(() => {
    const geometry = new THREE.BufferGeometry();
    geometry.setAttribute("position", new THREE.Float32BufferAttribute(overlay.positions, 3));
    geometry.computeVertexNormals();
    return geometry;
  }, [overlay.positions]);
  const enforce = overlay.role === "enforce";
  return (
    <mesh geometry={geometry}>
      <meshStandardMaterial
        color={enforce ? "#cfcfcf" : "#5a5a5a"}
        wireframe={!enforce}
        roughness={1}
        metalness={0}
        polygonOffset
        polygonOffsetFactor={1}
      />
    </mesh>
  );
}

function FrameBed({
  width,
  depth,
  height,
  centerIsZero,
}: {
  width: number;
  depth: number;
  height: number;
  centerIsZero: boolean;
}) {
  const camera = useThree((state) => state.camera);
  const controls = useThree((state) => state.controls) as {
    target?: THREE.Vector3;
    update?: () => void;
  } | null;
  useEffect(() => {
    const cx = centerIsZero ? 0 : width / 2;
    const cy = centerIsZero ? 0 : depth / 2;
    const span = Math.max(width, depth, 80);
    const lookZ = height * 0.12;
    camera.up.set(0, 0, 1);
    camera.position.set(cx + span * 0.2, cy - span * 1.15, height * 0.55 + span * 0.55);
    camera.lookAt(cx, cy, lookZ);
    camera.updateProjectionMatrix();
    controls?.target?.set(cx, cy, lookZ);
    controls?.update?.();
  }, [camera, controls, width, depth, height, centerIsZero]);
  return null;
}

function Bed({
  width,
  depth,
  height,
  centerIsZero,
}: {
  width: number;
  depth: number;
  height: number;
  centerIsZero: boolean;
}) {
  const cx = centerIsZero ? 0 : width / 2;
  const cy = centerIsZero ? 0 : depth / 2;
  const divisions = Math.max(4, Math.round(Math.max(width, depth) / 10));
  const edges = useMemo(() => new THREE.EdgesGeometry(new THREE.BoxGeometry(width, depth, height)), [width, depth, height]);
  return (
    <group>
      <mesh position={[cx, cy, -1.2]} receiveShadow>
        <boxGeometry args={[width, depth, 2.4]} />
        <meshStandardMaterial color="#161616" metalness={0.2} roughness={0.72} />
      </mesh>
      <gridHelper
        args={[Math.max(width, depth), divisions, "#4a4a4a", "#2a2a2a"]}
        rotation={[Math.PI / 2, 0, 0]}
        position={[cx, cy, 0.04]}
      />
      <lineSegments geometry={edges} position={[cx, cy, height / 2]}>
        <lineBasicMaterial color="#3a3a3a" />
      </lineSegments>
    </group>
  );
}

const ASCII_VERT = `
varying vec3 vNormal;
void main() {
  vNormal = normalize(normalMatrix * normal);
  gl_Position = projectionMatrix * modelViewMatrix * vec4(position, 1.0);
}
`;

const ASCII_FRAG = `
varying vec3 vNormal;
void main() {
  vec3 n = normalize(vNormal);
  float light = clamp(dot(n, normalize(vec3(0.35, -0.25, 0.9))), 0.0, 1.0);
  vec2 cell = fract(gl_FragCoord.xy / 8.0);
  float plus = (abs(cell.x - 0.5) < 0.08 || abs(cell.y - 0.5) < 0.08) ? 1.0 : 0.0;
  float hash = max(plus, (abs(cell.x - cell.y) < 0.07 || abs(cell.x + cell.y - 1.0) < 0.07) ? 1.0 : 0.0);
  float dotg = distance(cell, vec2(0.5)) < 0.14 ? 1.0 : 0.0;
  float ink = light > 0.66 ? hash : light > 0.36 ? plus : light > 0.14 ? dotg : 0.0;
  if (ink < 0.5) discard;
  gl_FragColor = vec4(vec3(0.78 + light * 0.22), 1.0);
}
`;

function PrintModel({
  model,
  mode,
  viewStyle,
  onSelect,
  onTransform,
}: {
  model: SceneModel;
  mode: TransformMode;
  viewStyle: ViewStyle;
  onSelect: (id: number) => void;
  onTransform: (id: number, transform: Transform) => void;
}) {
  const geometry = useMemo(() => {
    const geo = new THREE.BufferGeometry();
    geo.setAttribute("position", new THREE.Float32BufferAttribute(model.positions, 3));
    geo.computeVertexNormals();
    return geo;
  }, [model.positions]);
  const [node, setNode] = useState<THREE.Group | null>(null);
  const rotation = model.transform.rotation_deg.map((deg) => (deg * Math.PI) / 180) as [
    number,
    number,
    number,
  ];

  const publish = () => {
    if (!node) return;
    onTransform(model.id, {
      translation: [node.position.x, node.position.y, node.position.z],
      rotation_deg: [
        (node.rotation.x * 180) / Math.PI,
        (node.rotation.y * 180) / Math.PI,
        (node.rotation.z * 180) / Math.PI,
      ],
      scale: [node.scale.x, node.scale.y, node.scale.z],
    });
  };

  return (
    <>
      {model.selected && node && <TransformControls object={node} mode={mode} onMouseUp={publish} />}
      <group
        ref={setNode}
        position={model.transform.translation}
        rotation={rotation}
        scale={model.transform.scale}
        onClick={(event) => {
          event.stopPropagation();
          onSelect(model.id);
        }}
      >
        <mesh geometry={geometry}>
          {viewStyle === "ascii" ? (
            <shaderMaterial vertexShader={ASCII_VERT} fragmentShader={ASCII_FRAG} side={THREE.DoubleSide} />
          ) : viewStyle === "wire" ? (
            <meshBasicMaterial color={model.selected ? "#f2f2f2" : "#bdbdbd"} wireframe />
          ) : (
            <meshStandardMaterial
              color={model.selected ? "#f2f2f2" : "#c8c8c8"}
              roughness={0.62}
              metalness={0.02}
              flatShading
            />
          )}
        </mesh>
      </group>
    </>
  );
}

function Toolpaths({ paths }: { paths: PreviewPath[] }) {
  const geometry = useMemo(() => {
    const positions: number[] = [];
    const colors: number[] = [];
    const color = new THREE.Color();
    for (const path of paths) {
      color.set(KIND_COLOR[path.kind] ?? "#f2d3a2");
      for (let index = 1; index < path.points.length; index += 1) {
        const from = path.points[index - 1];
        const to = path.points[index];
        positions.push(from[0], from[1], from[2], to[0], to[1], to[2]);
        colors.push(color.r, color.g, color.b, color.r, color.g, color.b);
      }
    }
    const geo = new THREE.BufferGeometry();
    geo.setAttribute("position", new THREE.Float32BufferAttribute(positions, 3));
    geo.setAttribute("color", new THREE.Float32BufferAttribute(colors, 3));
    return geo;
  }, [paths]);
  if (paths.length === 0) return null;
  return (
    <lineSegments geometry={geometry}>
      <lineBasicMaterial vertexColors />
    </lineSegments>
  );
}
