import { Html, OrbitControls, TransformControls } from "@react-three/drei";
import { Canvas, useThree } from "@react-three/fiber";
import { useEffect, useMemo, useState } from "react";
import * as THREE from "three";
import type { PreviewPath, SupportOverlay, Transform } from "./api";
import { localCentre, rotateScale, translationForCentre, worldCentre } from "./coords";

export type TransformMode = "translate" | "rotate" | "scale";
export type ViewStyle = "solid" | "wire" | "ascii";

interface SceneModel {
  id: number;
  positions: import("./api").Positions;
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
  frameToken: number;
  /// Measuring is a mode of its own: clicks pick points instead of selecting.
  measuring: boolean;
  marks: Array<[number, number, number]>;
  measureLabel: string | null;
  onMeasure: (id: number, point: [number, number, number]) => void;
  onSelect: (id: number) => void;
  onTransform: (id: number, transform: Transform) => void;
}

/// The colour each kind of line is drawn in. Exported because the
/// preview's legend has to name the same colour the plate is painted with.
export const KIND_COLOR: Record<string, string> = {
  "wall-outer": "#e2dff0",
  "wall-inner": "#8e8ba4",
  skin: "#6e6b80",
  infill: "#4e6570",
  support: "#7b5a3f",
  "support-interface": "#a9865c",
  skirt: "#7d8fa6",
  brim: "#a8a3ba",
  raft: "#3a3834",
  "prime-tower": "#34323c",
  travel: "#2a2c34",
  extrusion: "#a39fb4",
};

export function Viewport(props: ViewportProps) {
  const cx = props.centerIsZero ? 0 : props.width / 2;
  const cy = props.centerIsZero ? 0 : props.depth / 2;
  return (
    <Canvas
      frameloop="demand"
      shadows={false}
      camera={{ position: [cx + 80, cy - 220, 180], fov: 32, up: [0, 0, 1], near: 0.1, far: 8000 }}
      onPointerMissed={() => props.onSelect(-1)}
      onCreated={({ camera }) => {
        camera.up.set(0, 0, 1);
        camera.lookAt(cx, cy, 0);
      }}
    >
      <color attach="background" args={["#06070b"]} />
      <ambientLight intensity={0.55} />
      <directionalLight position={[120, -80, 220]} intensity={0.9} color="#e9e8f2" />
      <directionalLight position={[-80, 140, 40]} intensity={0.18} color="#8a8798" />
      <Framer
        width={props.width}
        depth={props.depth}
        height={props.height}
        centerIsZero={props.centerIsZero}
        models={props.models}
        frameToken={props.frameToken}
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
            measuring={props.measuring}
            onMeasure={props.onMeasure}
            onSelect={props.onSelect}
            onTransform={props.onTransform}
          />
        ))}
      {props.overlays.map((overlay) => (
        <SupportVolume key={overlay.id} overlay={overlay} />
      ))}
      {props.showToolpaths && <Toolpaths paths={props.toolpaths} />}
      <MeasureMarks marks={props.marks} label={props.measureLabel} />
      <OrbitControls makeDefault enableDamping dampingFactor={0.12} />
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
        color={enforce ? "#8d8a9c" : "#3a3844"}
        wireframe={!enforce}
        roughness={1}
        metalness={0}
        polygonOffset
        polygonOffsetFactor={1}
      />
    </mesh>
  );
}

function modelBox(model: SceneModel): THREE.Box3 {
  const box = new THREE.Box3();
  const point = new THREE.Vector3();
  for (let index = 0; index < model.positions.length; index += 3) {
    const moved = rotateScale(
      [model.positions[index], model.positions[index + 1], model.positions[index + 2]],
      model.transform.rotation_deg,
      model.transform.scale,
    );
    point.set(
      model.transform.translation[0] + moved[0],
      model.transform.translation[1] + moved[1],
      model.transform.translation[2] + moved[2],
    );
    box.expandByPoint(point);
  }
  if (box.isEmpty()) box.set(new THREE.Vector3(), new THREE.Vector3());
  return box;
}

function frameBox(camera: THREE.PerspectiveCamera, controls: { target: THREE.Vector3; update: () => void }, box: THREE.Box3) {
  const centre = box.getCenter(new THREE.Vector3());
  const size = box.getSize(new THREE.Vector3());
  const aspect = camera.aspect > 0 ? camera.aspect : 1.4;
  const vFov = (camera.fov * Math.PI) / 180;
  const hFov = 2 * Math.atan(Math.tan(vFov / 2) * aspect);
  const span = Math.max(size.x, size.y, 30);
  const dist = Math.max(span / 2 / Math.tan(hFov / 2), size.z / 2 / Math.tan(vFov / 2), 40) * 1.28;
  camera.up.set(0, 0, 1);
  camera.position.set(centre.x + dist * 0.28, centre.y - dist * 0.62, centre.z + dist * 0.48);
  camera.near = Math.max(0.1, dist / 400);
  camera.far = dist * 30;
  camera.updateProjectionMatrix();
  controls.target.copy(centre);
  controls.update();
}

function Framer({
  width,
  depth,
  height,
  centerIsZero,
  models,
  frameToken,
}: {
  width: number;
  depth: number;
  height: number;
  centerIsZero: boolean;
  models: SceneModel[];
  frameToken: number;
}) {
  const camera = useThree((state) => state.camera) as THREE.PerspectiveCamera;
  const invalidate = useThree((state) => state.invalidate);
  const controls = useThree((state) => state.controls) as { target: THREE.Vector3; update: () => void } | null;
  const modelsRef = useMemo(() => ({ current: models }), []);
  modelsRef.current = models;
  useEffect(() => {
    if (!controls?.target || !controls.update) return;
    const selected = modelsRef.current.find((model) => model.selected);
    if (frameToken > 0 && selected && selected.positions.length > 0) {
      const box = modelBox(selected);
      const size = box.getSize(new THREE.Vector3());
      box.expandByScalar(Math.max(24, Math.max(size.x, size.y, size.z) * 0.8));
      frameBox(camera, controls, box);
      invalidate();
      return;
    }
    const x0 = centerIsZero ? -width / 2 : 0;
    const y0 = centerIsZero ? -depth / 2 : 0;
    const box = new THREE.Box3(
      new THREE.Vector3(x0, y0, 0),
      new THREE.Vector3(x0 + width, y0 + depth, Math.min(48, Math.max(height * 0.15, 20))),
    );
    frameBox(camera, controls, box);
    invalidate();
  }, [camera, controls, width, depth, height, centerIsZero, frameToken, modelsRef, invalidate]);
  return null;
}

function Bed({
  width,
  depth,
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
  const outline = useMemo(() => new THREE.EdgesGeometry(new THREE.PlaneGeometry(width, depth)), [width, depth]);
  return (
    <group>
      <mesh position={[cx, cy, -1.2]} receiveShadow>
        <boxGeometry args={[width, depth, 2.4]} />
        <meshStandardMaterial color="#0b0d14" metalness={0} roughness={1} />
      </mesh>
      <gridHelper
        args={[Math.max(width, depth), divisions, "#34343e", "#1e1f28"]}
        rotation={[Math.PI / 2, 0, 0]}
        position={[cx, cy, 0.04]}
      />
      <lineSegments geometry={outline} position={[cx, cy, 0.08]}>
        <lineBasicMaterial color="#4a4856" />
      </lineSegments>
      <Axes width={width} depth={depth} cx={cx} cy={cy} />
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
  float mask = light > 0.66 ? hash : light > 0.36 ? plus : light > 0.14 ? dotg : 0.0;
  if (mask < 0.5) discard;
  vec3 colour = vec3(0.914, 0.910, 0.949) * (0.55 + light * 0.45);
  gl_FragColor = vec4(colour, 1.0);
}
`;

function Axes({ width, depth, cx, cy }: { width: number; depth: number; cx: number; cy: number }) {
  const length = Math.max(12, Math.min(width, depth) * 0.08);
  const origin = [cx - width / 2, cy - depth / 2, 0.35] as const;
  const geometry = useMemo(() => {
    const positions = [
      origin[0], origin[1], origin[2], origin[0] + length, origin[1], origin[2],
      origin[0], origin[1], origin[2], origin[0], origin[1] + length, origin[2],
      origin[0], origin[1], origin[2], origin[0], origin[1], origin[2] + length,
    ];
    const colors = [
      0.62, 0.6, 0.68, 0.62, 0.6, 0.68,
      0.48, 0.47, 0.54, 0.48, 0.47, 0.54,
      0.72, 0.7, 0.78, 0.72, 0.7, 0.78,
    ];
    const geo = new THREE.BufferGeometry();
    geo.setAttribute("position", new THREE.Float32BufferAttribute(positions, 3));
    geo.setAttribute("color", new THREE.Float32BufferAttribute(colors, 3));
    return geo;
  }, [origin, length]);
  return (
    <lineSegments geometry={geometry}>
      <lineBasicMaterial vertexColors />
    </lineSegments>
  );
}

function PrintModel({
  model,
  mode,
  viewStyle,
  measuring,
  onMeasure,
  onSelect,
  onTransform,
}: {
  model: SceneModel;
  mode: TransformMode;
  viewStyle: ViewStyle;
  measuring: boolean;
  onMeasure: (id: number, point: [number, number, number]) => void;
  onSelect: (id: number) => void;
  onTransform: (id: number, transform: Transform) => void;
}) {
  const geometry = useMemo(() => {
    const geo = new THREE.BufferGeometry();
    geo.setAttribute("position", new THREE.Float32BufferAttribute(model.positions, 3));
    geo.computeVertexNormals();
    return geo;
  }, [model.positions]);
  const centre = useMemo(() => localCentre(model.positions), [model.positions]);
  const [node, setNode] = useState<THREE.Group | null>(null);
  const rotation = model.transform.rotation_deg.map((deg) => (deg * Math.PI) / 180) as [
    number,
    number,
    number,
  ];
  const pivot = worldCentre(model.positions, model.transform);

  const publish = () => {
    if (!node) return;
    const rotationDeg: [number, number, number] = [
      (node.rotation.x * 180) / Math.PI,
      (node.rotation.y * 180) / Math.PI,
      (node.rotation.z * 180) / Math.PI,
    ];
    const scale: [number, number, number] = [node.scale.x, node.scale.y, node.scale.z];
    const translation = translationForCentre(
      model.positions,
      { translation: [0, 0, 0], rotation_deg: rotationDeg, scale },
      [node.position.x, node.position.y, node.position.z],
    );
    onTransform(model.id, { translation, rotation_deg: rotationDeg, scale });
  };

  return (
    <>
      {model.selected && node && !measuring && (
        <TransformControls object={node} mode={mode} onMouseUp={publish} />
      )}
      <group
        ref={setNode}
        position={pivot}
        rotation={rotation}
        scale={model.transform.scale}
        onClick={(event) => {
          // While measuring, a click is a point on the part, not a selection.
          if (measuring) return;
          event.stopPropagation();
          onSelect(model.id);
        }}
      >
        <mesh
          geometry={geometry}
          position={[-centre[0], -centre[1], -centre[2]]}
          onPointerDown={
            measuring
              ? (event) => {
                  event.stopPropagation();
                  onMeasure(model.id, [event.point.x, event.point.y, event.point.z]);
                }
              : undefined
          }
        >
          {viewStyle === "ascii" ? (
            <shaderMaterial vertexShader={ASCII_VERT} fragmentShader={ASCII_FRAG} side={THREE.DoubleSide} />
          ) : viewStyle === "wire" ? (
            <meshBasicMaterial color={model.selected ? "#b9b3ff" : "#a39fb4"} wireframe />
          ) : (
            <meshStandardMaterial
              color={model.selected ? "#b9b3ff" : "#a39fb4"}
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
      color.set(KIND_COLOR[path.kind] ?? "#9c9aab");
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

/** The picked points, and once there are two, the dimension line between them. */
function MeasureMarks({
  marks,
  label,
}: {
  marks: Array<[number, number, number]>;
  label: string | null;
}) {
  const line = useMemo(() => {
    if (marks.length < 2) return null;
    const geometry = new THREE.BufferGeometry();
    geometry.setAttribute(
      "position",
      new THREE.Float32BufferAttribute([...marks[0], ...marks[1]], 3),
    );
    return geometry;
  }, [marks]);
  const middle = useMemo<[number, number, number] | null>(() => {
    if (marks.length < 2) return null;
    return [
      (marks[0][0] + marks[1][0]) / 2,
      (marks[0][1] + marks[1][1]) / 2,
      (marks[0][2] + marks[1][2]) / 2,
    ];
  }, [marks]);
  return (
    <group>
      {marks.map((mark, index) => (
        <mesh key={index} position={mark}>
          <sphereGeometry args={[0.9, 14, 14]} />
          <meshBasicMaterial color="#f4ef9a" />
        </mesh>
      ))}
      {line && (
        <lineSegments geometry={line}>
          <lineBasicMaterial color="#f4ef9a" depthTest={false} transparent opacity={0.9} />
        </lineSegments>
      )}
      {middle && label && (
        <Html position={middle} center style={{ pointerEvents: "none" }}>
          <div className="measure-label">{label}</div>
        </Html>
      )}
    </group>
  );
}
