import { Html, OrbitControls, TransformControls } from "@react-three/drei";
import { Canvas, useThree } from "@react-three/fiber";
import { memo, useEffect, useLayoutEffect, useMemo, useRef, useState } from "react";
import * as THREE from "three";
import type { PreviewPath, SupportOverlay, Transform } from "./api";
import { localCentre, rotateScale, translationForCentre, worldCentre } from "./coords";
import { canFrameBed, modelsToFrame, shouldFrameModels } from "./framer";
import {
  CURVED_SLOPE_MIN_DEG,
  KIND_COLOR,
  speedColour,
  verticesThroughLayer,
  type ColourMode,
  type TransformMode,
  type ViewStyle,
} from "./viewport-style";

export type { ColourMode, TransformMode, ViewStyle } from "./viewport-style";

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
  /// Kinds the person has switched off; hiding one is a visibility flag, not a
  /// rebuild of the buffers.
  hiddenKinds: Set<string>;
  /// Highest layer to draw. The buffers hold every layer; scrubbing moves the
  /// draw range.
  layer: number;
  overlays: SupportOverlay[];
  showModel: boolean;
  showToolpaths: boolean;
  viewStyle: ViewStyle;
  colour?: ColourMode;
  /** Theme accent and ink, so a curved segment is coloured with the active theme. */
  slopeAccent?: string;
  slopeDim?: string;
  mode: TransformMode;
  frameToken: number;
  cameraPreset?: "iso" | "top" | "front";
  cameraToken?: number;
  /// Measuring is a mode of its own: clicks pick points instead of selecting.
  measuring: boolean;
  marks: Array<[number, number, number]>;
  measureLabel: string | null;
  onMeasure: (id: number, point: [number, number, number]) => void;
  /// Armed by the 'place on face' button: the next click names a face.
  faceArmed: boolean;
  onFace: (id: number, normal: [number, number, number]) => void;
  onSelect: (id: number) => void;
  onTransform: (id: number, transform: Transform) => void;
  /// While armed, dragging over a part paints the triangles under the cursor
  /// instead of selecting or moving it.
  paintArmed: boolean;
  /// How wide the brush reaches, in millimetres of model surface.
  paintRadius: number;
  onBrush: (id: number, points: Array<[number, number, number]>) => void;
}

function MockPlateFallback({
  width,
  depth,
  models,
  showToolpaths,
  layer,
}: Pick<ViewportProps, "width" | "depth" | "models" | "showToolpaths" | "layer">) {
  const bed = [
    "┌" + "─".repeat(48) + "┐",
    ...Array.from({ length: 22 }, (_, row) => {
      const y = Math.round((row / 21) * depth);
      const mark = models.length > 0 ? "│" + "·".repeat(16) + "▣" + "·".repeat(31) + "│" : "│" + "·".repeat(48) + "│";
      void y;
      return mark;
    }),
    "└" + "─".repeat(48) + "┘",
  ].join("\n");
  return (
    <div className="viewport-mock" aria-hidden>
      <p className="viewport-mock-meta">
        mock plate · {width}×{depth} mm · {models.length} part{models.length === 1 ? "" : "s"}
        {showToolpaths ? ` · layer ${layer + 1}` : ""}
      </p>
      <pre className="viewport-mock-bed">{bed}</pre>
    </div>
  );
}

function noteViewportPerf(kind: "render" | "camera") {
  if (typeof window === "undefined") return;
  if (new URLSearchParams(window.location.search).get("perf") !== "1") return;
  const host = window as Window & { __shalePerf?: { renders: number; cameraMoves: number } };
  const record = host.__shalePerf ?? { renders: 0, cameraMoves: 0 };
  if (kind === "render") record.renders += 1;
  else record.cameraMoves += 1;
  host.__shalePerf = record;
}

export const Viewport = memo(function Viewport(props: ViewportProps) {
  noteViewportPerf("render");
  const measure =
    typeof window !== "undefined" && new URLSearchParams(window.location.search).get("perf") === "1";
  if (import.meta.env.VITE_SHALE_MOCK === "1" && !measure) {
    return (
      <MockPlateFallback
        width={props.width}
        depth={props.depth}
        models={props.models}
        showToolpaths={props.showToolpaths}
        layer={props.layer}
      />
    );
  }
  const sceneBg =
    typeof document !== "undefined"
      ? getComputedStyle(document.documentElement).getPropertyValue("--bg").trim() || "#06070b"
      : "#06070b";
  const cx = props.centerIsZero ? 0 : props.width / 2;
  const cy = props.centerIsZero ? 0 : props.depth / 2;
  return (
    <div className="viewport-host">
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
      <color attach="background" args={[sceneBg]} />
      <hemisphereLight args={["#d9d6ea", "#1c1e28", 0.72]} />
      <ambientLight intensity={0.22} />
      <directionalLight position={[140, -90, 260]} intensity={1.15} color="#f6f4ff" />
      <directionalLight position={[-180, 120, 90]} intensity={0.38} color="#7d7a90" />
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
            accent={props.slopeAccent}
            mode={props.mode}
            measuring={props.measuring}
            onMeasure={props.onMeasure}
            onSelect={props.onSelect}
            onTransform={props.onTransform}
            paintArmed={props.paintArmed}
            faceArmed={props.faceArmed}
            onFace={props.onFace}
            paintRadius={props.paintRadius}
            onBrush={props.onBrush}
          />
        ))}
      {props.overlays.map((overlay) => (
        <SupportVolume key={overlay.id} overlay={overlay} />
      ))}
            {props.showToolpaths && (
        <Toolpaths
          paths={props.toolpaths}
          colour={props.colour ?? "type"}
          layer={props.layer}
          hidden={props.hiddenKinds}
          accent={props.slopeAccent ?? "#b9b3ff"}
          dim={props.slopeDim ?? "#6f7390"}
        />
      )}
      <MeasureMarks marks={props.marks} label={props.measureLabel} />
      <CameraRig />
      <CameraAim
        preset={props.cameraPreset ?? "iso"}
        token={props.cameraToken ?? 0}
        cx={cx}
        cy={cy}
      />
    </Canvas>
    </div>
  );
});

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

function boxOutlineGeometry(box: THREE.Box3): THREE.BufferGeometry {
  const { min, max } = box;
  const corners = [
    new THREE.Vector3(min.x, min.y, min.z),
    new THREE.Vector3(max.x, min.y, min.z),
    new THREE.Vector3(max.x, max.y, min.z),
    new THREE.Vector3(min.x, max.y, min.z),
    new THREE.Vector3(min.x, min.y, max.z),
    new THREE.Vector3(max.x, min.y, max.z),
    new THREE.Vector3(max.x, max.y, max.z),
    new THREE.Vector3(min.x, max.y, max.z),
  ];
  const edges = [
    [0, 1],
    [1, 2],
    [2, 3],
    [3, 0],
    [4, 5],
    [5, 6],
    [6, 7],
    [7, 4],
    [0, 4],
    [1, 5],
    [2, 6],
    [3, 7],
  ];
  const positions: number[] = [];
  for (const [a, b] of edges) {
    positions.push(corners[a].x, corners[a].y, corners[a].z, corners[b].x, corners[b].y, corners[b].z);
  }
  const geometry = new THREE.BufferGeometry();
  geometry.setAttribute("position", new THREE.Float32BufferAttribute(positions, 3));
  return geometry;
}

function SelectionOutline({ box }: { box: THREE.Box3 }) {
  const geometry = useMemo(() => boxOutlineGeometry(box), [box.min.x, box.min.y, box.min.z, box.max.x, box.max.y, box.max.z]);
  return (
    <lineSegments geometry={geometry} raycast={() => null} renderOrder={4}>
      <lineBasicMaterial color="#b9b3ff" transparent opacity={0.92} depthTest={false} depthWrite={false} />
    </lineSegments>
  );
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
  const modelSignal = useMemo(
    () => models.reduce((count, model) => count + model.positions.length, models.length),
    [models],
  );
  useEffect(() => {
    if (!controls?.target || !controls.update) return;
  // A frame request frames the part, not the machine. Falling back to the
  // build volume before the bed size is known framed (0,0) at machine scale
  // zero; after import the camera must re-aim at the geometry even when the
  // user has not pressed F yet.
    if (shouldFrameModels(models)) {
      const parts = modelsToFrame(models) as SceneModel[];
      const box = new THREE.Box3();
      for (const part of parts) box.union(modelBox(part));
      const size = box.getSize(new THREE.Vector3());
      box.expandByScalar(Math.max(10, Math.max(size.x, size.y, size.z) * 0.45));
      frameBox(camera, controls, box);
      invalidate();
      return;
    }
    if (!canFrameBed(width, depth)) return;
    const x0 = centerIsZero ? -width / 2 : 0;
    const y0 = centerIsZero ? -depth / 2 : 0;
    const box = new THREE.Box3(
      new THREE.Vector3(x0, y0, 0),
      new THREE.Vector3(x0 + width, y0 + depth, Math.min(48, Math.max(height * 0.15, 20))),
    );
    frameBox(camera, controls, box);
    invalidate();
  }, [
    camera,
    controls,
    width,
    depth,
    height,
    centerIsZero,
    frameToken,
    modelSignal,
    models,
    invalidate,
  ]);
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
  const outline = useMemo(() => new THREE.EdgesGeometry(new THREE.PlaneGeometry(width, depth)), [width, depth]);
  return (
    <group>
      {/* The plate is a surface, not a slab, and it never writes depth: a solid
          box under the part hid it completely once the camera dropped below the
          bed. Seen from underneath the plate is glass — the outline and the grid
          still say where it is, and the part stays visible through it. */}
      <mesh position={[cx, cy, 0]} renderOrder={-1}>
        <planeGeometry args={[width, depth]} />
        <meshBasicMaterial
          color="#0b0d14"
          transparent
          opacity={0.55}
          side={THREE.DoubleSide}
          depthWrite={false}
        />
      </mesh>
      <FadingGrid width={width} depth={depth} cx={cx} cy={cy} />
      <lineSegments geometry={outline} position={[cx, cy, 0.08]}>
        <lineBasicMaterial color="#4a4856" transparent opacity={0.9} depthWrite={false} />
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

function FadingGrid({ width, depth, cx, cy }: { width: number; depth: number; cx: number; cy: number }) {
  const geometry = useMemo(() => {
    const positions: number[] = [];
    const alphas: number[] = [];
    const step = 10;
    const x0 = cx - width / 2;
    const y0 = cy - depth / 2;
    const x1 = cx + width / 2;
    const y1 = cy + depth / 2;
    const maxR = Math.hypot(width, depth) * 0.62;
    const push = (ax: number, ay: number, bx: number, by: number) => {
      const fade = (x: number, y: number) => Math.max(0, 1 - Math.hypot(x - cx, y - cy) / maxR);
      positions.push(ax, ay, 0.05, bx, by, 0.05);
      alphas.push(fade(ax, ay), fade(bx, by));
    };
    for (let x = x0; x <= x1 + 0.01; x += step) push(x, y0, x, y1);
    for (let y = y0; y <= y1 + 0.01; y += step) push(x0, y, x1, y);
    const geo = new THREE.BufferGeometry();
    geo.setAttribute("position", new THREE.Float32BufferAttribute(positions, 3));
    geo.setAttribute("alpha", new THREE.Float32BufferAttribute(alphas, 1));
    return geo;
  }, [width, depth, cx, cy]);
  const light =
    typeof document !== "undefined" && document.documentElement.getAttribute("data-ui-theme") === "lunar";
  const material = useMemo(
    () =>
      new THREE.ShaderMaterial({
        transparent: true,
        depthWrite: false,
        uniforms: { uColor: { value: new THREE.Color(light ? "#7d8796" : "#8d8aa0") } },
        vertexShader: `
          attribute float alpha;
          varying float vAlpha;
          void main() {
            vAlpha = alpha;
            gl_Position = projectionMatrix * modelViewMatrix * vec4(position, 1.0);
          }
        `,
        fragmentShader: `
          uniform vec3 uColor;
          varying float vAlpha;
          void main() {
            gl_FragColor = vec4(uColor, vAlpha * 0.72);
          }
        `,
      }),
    [light],
  );
  return <lineSegments geometry={geometry} material={material} />;
}

function CameraAim({
  preset,
  token,
  cx,
  cy,
}: {
  preset: "iso" | "top" | "front";
  token: number;
  cx: number;
  cy: number;
}) {
  const camera = useThree((state) => state.camera);
  const controls = useThree((state) => state.controls);
  const invalidate = useThree((state) => state.invalidate);
  useLayoutEffect(() => {
    if (token === 0) return;
    const next =
      preset === "top"
        ? ([cx, cy - 0.01, 460] as const)
        : preset === "front"
          ? ([cx, cy - 380, 70] as const)
          : ([cx + 80, cy - 220, 180] as const);
    camera.position.set(next[0], next[1], next[2]);
    camera.up.set(0, 0, 1);
    camera.lookAt(cx, cy, 12);
    const orbit = controls as { target?: THREE.Vector3; update?: () => void } | null;
    orbit?.target?.set(cx, cy, 12);
    orbit?.update?.();
    invalidate();
  }, [token, preset, cx, cy, camera, controls, invalidate]);
  return null;
}

function PrintModel({
  model,
  mode,
  viewStyle,
  accent,
  measuring,
  onMeasure,
  onSelect,
  onTransform,
  paintArmed,
  faceArmed,
  onFace,
  paintRadius,
  onBrush,
}: {
  model: SceneModel;
  mode: TransformMode;
  viewStyle: ViewStyle;
  measuring: boolean;
  onMeasure: (id: number, point: [number, number, number]) => void;
  /// Armed by the 'place on face' button: the next click names a face.
  faceArmed: boolean;
  onFace: (id: number, normal: [number, number, number]) => void;
  onSelect: (id: number) => void;
  onTransform: (id: number, transform: Transform) => void;
  paintArmed: boolean;
  paintRadius: number;
  onBrush: (id: number, points: Array<[number, number, number]>) => void;
  accent?: string;
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
  const bounds = useMemo(() => modelBox(model), [model.positions, model.transform]);

  // A stroke is the path the cursor took over the surface, in the mesh's own
  // frame — the same frame as `model.positions`, which is the frame the core
  // counts triangles in. The faces themselves are chosen in the core, so the
  // brush and the patch buttons cannot disagree about what a face is.
  const path = useRef<Array<[number, number, number]>>([]);
  const pressing = useRef(false);
  const [brush, setBrush] = useState<[number, number, number] | null>(null);

  const keepPoint = (local: THREE.Vector3) => {
    // Points closer together than the brush itself would only repeat the same
    // discs, and a path with a thousand of them is a path nobody needs.
    const reach = Math.max(paintRadius * 0.35, 0.4);
    const tail = path.current[path.current.length - 1];
    if (tail) {
      const dx = tail[0] - local.x;
      const dy = tail[1] - local.y;
      const dz = tail[2] - local.z;
      if (dx * dx + dy * dy + dz * dz < reach * reach) return;
    }
    path.current.push([local.x, local.y, local.z]);
  };

  // A stroke ends where the button does, even if the cursor has left the part:
  // the mesh only sees the pointer while it is over it.
  useEffect(() => {
    if (!paintArmed) return;
    const finish = () => {
      pressing.current = false;
      if (path.current.length === 0) return;
      const points = path.current;
      path.current = [];
      onBrush(model.id, points);
    };
    window.addEventListener("pointerup", finish);
    window.addEventListener("pointercancel", finish);
    return () => {
      window.removeEventListener("pointerup", finish);
      window.removeEventListener("pointercancel", finish);
    };
  }, [paintArmed, model.id, onBrush]);

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

  // Dragging the body slides the part across the plate; the gizmo is for one
  // axis at a time, this is the coarse move. Z is taken from where the drag
  // started, so a part never leaves the bed by being dragged.
  const dragging = useRef<{ start: THREE.Vector3; from: THREE.Vector3 } | null>(null);
  const invalidate = useThree((state) => state.invalidate);

  useEffect(() => {
    const stop = () => {
      if (!dragging.current) return;
      dragging.current = null;
      publish();
    };
    window.addEventListener("pointerup", stop);
    window.addEventListener("pointercancel", stop);
    return () => {
      window.removeEventListener("pointerup", stop);
      window.removeEventListener("pointercancel", stop);
    };
  }, [publish]);

  return (
    <>
      {model.selected && node && !measuring && (
        <TransformControls object={node} mode={mode} onMouseUp={publish} />
      )}
      {model.selected && !measuring && viewStyle !== "wire" && <SelectionOutline box={bounds} />}
      <group
        ref={setNode}
        position={pivot}
        rotation={rotation}
        scale={model.transform.scale}
        onClick={(event) => {
          // While measuring, a click is a point on the part, not a selection.
          if (measuring) return;
          event.stopPropagation();
          if (faceArmed && event.face) {
            // The normal comes out of the geometry in its own space; the group
            // above carries the part's rotation, so the matrix takes it out to
            // the world the core measures in.
            const normal = event.face.normal.clone().transformDirection(event.object.matrixWorld);
            onFace(model.id, [normal.x, normal.y, normal.z]);
            return;
          }
          onSelect(model.id);
        }}
      >
        <mesh
          geometry={geometry}
          position={[-centre[0], -centre[1], -centre[2]]}
          onPointerDown={
            paintArmed
              ? (event) => {
                  event.stopPropagation();
                  path.current = [];
                  // The button is what keeps a stroke going, not the event's own
                  // `buttons` field: a pen, a touch and a mouse that has been
                  // captured do not all report it the same way.
                  pressing.current = true;
                  keepPoint(event.object.worldToLocal(event.point.clone()));
                  setBrush([event.point.x, event.point.y, event.point.z]);
                }
              : measuring
                ? (event) => {
                    event.stopPropagation();
                    onMeasure(model.id, [event.point.x, event.point.y, event.point.z]);
                  }
                : model.selected && mode === "translate" && node
                  ? (event) => {
                      event.stopPropagation();
                      dragging.current = {
                        start: event.point.clone(),
                        from: node.position.clone(),
                      };
                    }
                  : undefined
          }
          onPointerMove={(event) => {
            // One handler for both gestures: a drag starts after this render,
            // and a handler that only exists while `paintArmed` was chosen at
            // render time would never see it.
            if (paintArmed) {
              const local = event.object.worldToLocal(event.point.clone());
              if (pressing.current) keepPoint(local);
              setBrush([event.point.x, event.point.y, event.point.z]);
              return;
            }
            const drag = dragging.current;
            if (!drag || !node) return;
            const delta = event.point.clone().sub(drag.start);
            node.position.set(drag.from.x + delta.x, drag.from.y + delta.y, drag.from.z);
            invalidate();
          }}
          onPointerOut={paintArmed ? () => setBrush(null) : undefined}
        >
          {viewStyle === "ascii" ? (
            <shaderMaterial vertexShader={ASCII_VERT} fragmentShader={ASCII_FRAG} side={THREE.DoubleSide} />
          ) : viewStyle === "wire" ? (
            <meshBasicMaterial color={model.selected ? "#b9b3ff" : "#a39fb4"} wireframe />
          ) : (
            <meshStandardMaterial
              color={model.selected ? accent || "#b9b3ff" : "#c8c4d4"}
              roughness={0.4}
              metalness={0.14}
            />
          )}
        </mesh>
        {paintArmed && brush && (
          // The brush shows its own reach: a ring the size of the stroke, at the
          // point on the surface it would paint.
          <mesh
            position={brush}
            renderOrder={2}
            // The preview must not eat the stroke: it sits exactly where the
            // next press lands, so a solid sphere would take the press itself
            // and the part would never see it.
            raycast={() => null}
          >
            <sphereGeometry args={[Math.max(paintRadius, 0.4), 20, 12]} />
            <meshBasicMaterial
              color="#b9b3ff"
              transparent
              opacity={0.18}
              depthWrite={false}
            />
          </mesh>
        )}
      </group>
    </>
  );
}

/// One buffer per kind, built once per slice.
///
/// The old version rebuilt the whole plate whenever `paths` changed identity,
/// which was every slider step and every kind toggle. Here the vertices are
/// laid down in layer order and each kind keeps a cumulative count per layer,
/// so scrubbing is `setDrawRange` and a toggle is `visible = false`: no
/// geometry is touched after the slice arrives.
interface KindBuffer {
  kind: string;
  geometry: THREE.BufferGeometry;
  /// counts[layer] is how many vertices to draw for that layer and below.
  counts: Int32Array;
  lastLayer: number;
}

function slopeTint(slope: number, maxSlope: number, dim: THREE.Color, accent: THREE.Color, out: THREE.Color) {
  const place = maxSlope > CURVED_SLOPE_MIN_DEG ? Math.min(1, Math.max(0, slope / maxSlope)) : 1;
  out.copy(dim).lerp(accent, 0.25 + place * 0.75);
}

function buildKinds(paths: PreviewPath[], colour: ColourMode, accentHex: string, dimHex: string): KindBuffer[] {
  if (paths.length === 0) return [];
  let low = Infinity;
  let high = -Infinity;
  if (colour === "speed") {
    // Travel is not a print speed; leaving it in stretched the ramp to rapid
    // moves and pressed every real pass into one end of it.
    for (const path of paths) {
      if (path.kind === "travel") continue;
      const feed = path.feed ?? 0;
      if (feed > 0) {
        low = Math.min(low, feed);
        high = Math.max(high, feed);
      }
    }
  }
  let maxSlope = CURVED_SLOPE_MIN_DEG;
  for (const path of paths) {
    if (!path.slopes) continue;
    for (const slope of path.slopes) {
      if (slope > maxSlope) maxSlope = slope;
    }
  }
  const accent = new THREE.Color(accentHex);
  const dim = new THREE.Color(dimHex);
  const tint = new THREE.Color();
  const colourOf = (path: PreviewPath, slope: number): void => {
    const curved = slope > CURVED_SLOPE_MIN_DEG;
    if (colour === "slope" && path.kind !== "travel") {
      slopeTint(slope, maxSlope, dim, accent, tint);
      return;
    }
    if (colour === "type" && curved) {
      slopeTint(slope, maxSlope, dim, accent, tint);
      return;
    }
    if (colour === "speed") {
      tint.set(
        path.kind === "travel"
          ? "#26282f"
          : (path.feed ?? 0) > 0
            ? speedColour(path.feed as number, low, high)
            : KIND_COLOR[path.kind] ?? "#9c9aab",
      );
      return;
    }
    tint.set(KIND_COLOR[path.kind] ?? "#9c9aab");
  };

  let maxLayer = 0;
  for (const path of paths) maxLayer = Math.max(maxLayer, path.layer ?? 0);

  const groups = new Map<string, { positions: number[]; colors: number[] }>();
  for (const path of paths) {
    let group = groups.get(path.kind);
    if (!group) {
      group = { positions: [], colors: [] };
      groups.set(path.kind, group);
    }
    for (let index = 1; index < path.points.length; index += 1) {
      const from = path.points[index - 1];
      const to = path.points[index];
      // Z is the G-code's own Z. A curved move is not flattened onto the layer.
      const slope = path.slopes?.[index - 1] ?? 0;
      colourOf(path, slope);
      group.positions.push(from[0], from[1], from[2], to[0], to[1], to[2]);
      group.colors.push(tint.r, tint.g, tint.b, tint.r, tint.g, tint.b);
    }
  }

  // Cumulative vertex counts, filled per layer, so the draw range for layer N
  // is a lookup rather than a scan.
  const out: KindBuffer[] = [];
  for (const [kind, group] of groups) {
    const perLayer = new Int32Array(maxLayer + 1);
    for (const path of paths) {
      if (path.kind !== kind) continue;
      const layer = Math.min(maxLayer, Math.max(0, path.layer ?? 0));
      perLayer[layer] += Math.max(0, path.points.length - 1) * 2;
    }
    let running = 0;
    for (let layer = 0; layer <= maxLayer; layer += 1) {
      running += perLayer[layer];
      perLayer[layer] = running;
    }
    const geometry = new THREE.BufferGeometry();
    geometry.setAttribute("position", new THREE.Float32BufferAttribute(group.positions, 3));
    geometry.setAttribute("color", new THREE.Float32BufferAttribute(group.colors, 3));
    geometry.computeBoundingSphere();
    out.push({ kind, geometry, counts: perLayer, lastLayer: maxLayer });
  }
  return out;
}

function KindLines({ buffer, layer, hidden }: { buffer: KindBuffer; layer: number; hidden: boolean }) {
  const invalidate = useThree((state) => state.invalidate);
  useLayoutEffect(() => {
    const at = Math.min(buffer.lastLayer, Math.max(0, layer));
    buffer.geometry.setDrawRange(0, verticesThroughLayer(buffer.counts, at));
    // The canvas draws on demand; a draw-range change is not a React prop, so
    // nothing would repaint without this.
    invalidate();
  }, [buffer, layer, invalidate]);
  useEffect(() => {
    invalidate();
  }, [hidden, invalidate]);
  return (
    <lineSegments geometry={buffer.geometry} visible={!hidden} frustumCulled>
      <lineBasicMaterial vertexColors />
    </lineSegments>
  );
}

/** Orbit without writing the camera into React state, so a drag does not re-render the plate. */
function CameraRig() {
  const [damping, setDamping] = useState(true);
  useEffect(() => {
    const media = window.matchMedia("(prefers-reduced-motion: reduce)");
    const apply = () => setDamping(!media.matches);
    apply();
    media.addEventListener("change", apply);
    return () => media.removeEventListener("change", apply);
  }, []);
  return (
    <OrbitControls
      makeDefault
      enableDamping={damping}
      dampingFactor={0.12}
      onChange={() => noteViewportPerf("camera")}
    />
  );
}

function Toolpaths({
  paths,
  colour,
  layer,
  hidden,
  accent,
  dim,
}: {
  paths: PreviewPath[];
  colour: ColourMode;
  layer: number;
  hidden: Set<string>;
  accent: string;
  dim: string;
}) {
  const buffers = useMemo(() => buildKinds(paths, colour, accent, dim), [paths, colour, accent, dim]);
  if (buffers.length === 0) return null;
  return (
    <group>
      {buffers.map((buffer) => (
        <KindLines
          key={buffer.kind}
          buffer={buffer}
          layer={layer}
          hidden={hidden.has(buffer.kind)}
        />
      ))}
    </group>
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
          <meshBasicMaterial color="#b9b3ff" />
        </mesh>
      ))}
      {line && (
        <lineSegments geometry={line}>
          <lineBasicMaterial color="#b9b3ff" depthTest={false} transparent opacity={0.9} />
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
