import { Html, OrbitControls, TransformControls } from "@react-three/drei";
import { Canvas, useThree } from "@react-three/fiber";
import { useEffect, useLayoutEffect, useMemo, useRef, useState } from "react";
import * as THREE from "three";
import type { PreviewPath, SupportOverlay, Transform } from "./api";
import { localCentre, rotateScale, translationForCentre, worldCentre } from "./coords";

export type TransformMode = "translate" | "rotate" | "scale";
export type ViewStyle = "solid" | "wire" | "ascii";

/// What the toolpath is coloured by. "type" is what the lines are (wall, skin,
/// infill); "speed" is how fast they are laid down, which is the question a
/// print that looks wrong actually asks. Bambu's preview offers the same choice.
export type ColourMode = "type" | "speed";

/// Slow to fast in one hue: the theme has a single accent, so the ramp runs
/// from a dim grey to that accent rather than through a thermal camera's
/// colours. Five stops is enough for a line to be placed by eye.
export const SPEED_STOPS = ["#474a5e", "#625d7e", "#7b74a3", "#9a93d2", "#b9b3ff"];

export function speedColour(feed: number, low: number, high: number): string {
  if (!(high > low)) return SPEED_STOPS[SPEED_STOPS.length - 1];
  const span = high - low;
  const place = Math.min(1, Math.max(0, (feed - low) / span));
  const index = Math.min(SPEED_STOPS.length - 1, Math.floor(place * SPEED_STOPS.length));
  return SPEED_STOPS[index];
}

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
  mode: TransformMode;
  frameToken: number;
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

/// The colour each kind of line is drawn in. Exported because the
/// preview's legend has to name the same colour the plate is painted with.
/// Nine kinds have to be told apart at a glance on a dark plate, which one
/// accent cannot do on its own: these are muted hues at the same lightness, so
/// they read as one palette and still differ. Travel is deliberately almost
/// invisible and is hidden until asked for.
export const KIND_COLOR: Record<string, string> = {
  "wall-outer": "#e9e8f2",
  "wall-inner": "#b9b3ff",
  skin: "#7fb0c6",
  infill: "#6f9a7c",
  support: "#c08a5a",
  "support-interface": "#e0b07a",
  skirt: "#8d7fbb",
  brim: "#b98fae",
  raft: "#6b6f7e",
  "prime-tower": "#9c9a6a",
  travel: "#23252c",
  extrusion: "#a39fb4",
};

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

export function Viewport(props: ViewportProps) {
  if (import.meta.env.VITE_SHALE_MOCK === "1") {
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
        />
      )}
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
    // A frame request frames the part, not the machine. Falling back to the
    // build volume meant `f` — and the frame the preview asks for after a
    // slice — showed a plate with a speck on it while the toolpaths sat
    // invisible at the distance that framing implies.
    if (frameToken > 0) {
      const chosen = modelsRef.current.find((model) => model.selected);
      const parts = chosen ? [chosen] : modelsRef.current.filter((model) => model.positions.length > 0);
      if (parts.length > 0) {
        const box = new THREE.Box3();
        for (const part of parts) box.union(modelBox(part));
        const size = box.getSize(new THREE.Vector3());
        // 24 mm of padding swamped a 60 mm part: framing should leave air for
        // the skirt, not a third of the viewport.
        box.expandByScalar(Math.max(10, Math.max(size.x, size.y, size.z) * 0.45));
        frameBox(camera, controls, box);
        invalidate();
        return;
      }
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
      <gridHelper
        args={[Math.max(width, depth), divisions, "#34343e", "#1e1f28"]}
        rotation={[Math.PI / 2, 0, 0]}
        position={[cx, cy, 0.04]}
      >
        <lineBasicMaterial attach="material" vertexColors transparent opacity={0.7} depthWrite={false} />
      </gridHelper>
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

function PrintModel({
  model,
  mode,
  viewStyle,
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
              color={model.selected ? "#b9b3ff" : "#a39fb4"}
              roughness={0.62}
              metalness={0.02}
              flatShading
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

function buildKinds(paths: PreviewPath[], colour: ColourMode): KindBuffer[] {
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
  const colour_of = (path: PreviewPath): string =>
    colour === "speed"
      ? path.kind === "travel"
        ? "#26282f"
        : (path.feed ?? 0) > 0
          ? speedColour(path.feed as number, low, high)
          : KIND_COLOR[path.kind] ?? "#9c9aab"
      : KIND_COLOR[path.kind] ?? "#9c9aab";

  let maxLayer = 0;
  for (const path of paths) maxLayer = Math.max(maxLayer, path.layer ?? 0);

  const groups = new Map<string, { positions: number[]; colors: number[]; counts: Int32Array }>();
  const tint = new THREE.Color();
  for (const path of paths) {
    let group = groups.get(path.kind);
    if (!group) {
      group = { positions: [], colors: [], counts: new Int32Array(maxLayer + 1) };
      groups.set(path.kind, group);
    }
    tint.set(colour_of(path));
    for (let index = 1; index < path.points.length; index += 1) {
      const from = path.points[index - 1];
      const to = path.points[index];
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
    out.push({ kind, geometry, counts: perLayer, lastLayer: maxLayer });
  }
  return out;
}

function KindLines({ buffer, layer, hidden }: { buffer: KindBuffer; layer: number; hidden: boolean }) {
  const invalidate = useThree((state) => state.invalidate);
  useLayoutEffect(() => {
    const at = Math.min(buffer.lastLayer, Math.max(0, layer));
    buffer.geometry.setDrawRange(0, buffer.counts[at] ?? 0);
    // The canvas draws on demand; a draw-range change is not a React prop, so
    // nothing would repaint without this.
    invalidate();
  }, [buffer, layer, invalidate]);
  useEffect(() => {
    invalidate();
  }, [hidden, invalidate]);
  return (
    <lineSegments geometry={buffer.geometry} visible={!hidden}>
      <lineBasicMaterial vertexColors />
    </lineSegments>
  );
}

function Toolpaths({
  paths,
  colour,
  layer,
  hidden,
}: {
  paths: PreviewPath[];
  colour: ColourMode;
  layer: number;
  hidden: Set<string>;
}) {
  const buffers = useMemo(() => buildKinds(paths, colour), [paths, colour]);
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
