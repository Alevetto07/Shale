import { Suspense, useEffect, useLayoutEffect, useMemo, useState } from "react";
import { Canvas, useThree } from "@react-three/fiber";
import * as THREE from "three";
import { Line, OrbitControls } from "@react-three/drei";
import { fitToBox, type Box } from "../../lib/model-fit";

type Layer = {
  walls: number[][][];
  infill: number[][][];
};

/** The handful of OrbitControls members the camera fit touches. */
type Controls = {
  target: THREE.Vector3;
  minDistance: number;
  maxDistance: number;
  update?: () => void;
};

/** G-code (x, y) to scene (x, height, z) — the same mapping the lines use. */
const toScene = ([x, y]: number[]): [number, number, number] => [x - 128, 0.4, 128 - y];

/** The box this layer's lines occupy. The file is a 10 mm cube parked at
 *  (128, 128), so without this the camera would sit 144 mm out at a shape that
 *  is 10 mm across. */
function layerBox(layer: Layer): Box | null {
  let minX = Infinity;
  let maxX = -Infinity;
  let minZ = Infinity;
  let maxZ = -Infinity;
  let points = 0;

  for (const path of [...layer.walls, ...layer.infill]) {
    for (const p of path) {
      const [x, , z] = toScene(p);
      if (x < minX) minX = x;
      if (x > maxX) maxX = x;
      if (z < minZ) minZ = z;
      if (z > maxZ) maxZ = z;
      points++;
    }
  }

  if (!points) return null;
  return { minX, maxX, minY: 0.4, maxY: 0.4, minZ, maxZ };
}

function FitTo({ box }: { box: Box }) {
  const { camera, size } = useThree();
  const controls = useThree((s) => s.controls) as Controls | null;

  // Refit when the shape changes, not on every layer step: the slider moves
  // once per frame while the panel plays, and re-aiming the camera on each
  // step would fight whoever is dragging it.
  const key = [box.minX, box.maxX, box.minZ, box.maxZ].map((n) => n.toFixed(1)).join(",");

  useLayoutEffect(() => {
    const persp = camera as THREE.PerspectiveCamera;
    const fit = fitToBox(box, {
      fov: persp.fov,
      aspect: size.width / Math.max(1, size.height),
      fill: 0.66,
      // Lower than the preview's angle: this panel is about one layer seen
      // almost edge-on, which is the view where the infill pattern reads.
      direction: [0.46, 0.58, 0.75],
    });

    persp.position.set(fit.position[0], fit.position[1], fit.position[2]);
    persp.near = Math.max(0.05, fit.distance / 200);
    persp.far = fit.distance * 40;
    persp.lookAt(fit.target[0], fit.target[1], fit.target[2]);
    persp.updateProjectionMatrix();

    if (controls) {
      controls.target.set(fit.target[0], fit.target[1], fit.target[2]);
      controls.minDistance = fit.distance * 0.35;
      controls.maxDistance = fit.distance * 4;
      controls.update?.();
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [key, camera, size, controls]);

  return null;
}

function LayerLines({ layer }: { layer: Layer }) {
  return (
    <group>
      {layer.walls.map((path, i) => {
        const pts = path.map((p) => toScene(p));
        return <Line key={`w${i}`} points={pts} color="#e9e8f2" lineWidth={1} />;
      })}
      {layer.infill.map((path, i) => {
        const pts = path.map((p) => toScene(p));
        return <Line key={`i${i}`} points={pts} color="#7eb8ff" lineWidth={0.5} dashed dashSize={1} gapSize={0.6} />;
      })}
    </group>
  );
}

export function ToolpathScene3d({ layerIndex }: { layerIndex: number }) {
  const [layer, setLayer] = useState<Layer | null>(null);

  useEffect(() => {
    fetch("/demo/toolpath-cube.json")
      .then((r) => r.json())
      .then((json) => setLayer(json.layers[layerIndex] ?? json.layers[0]));
  }, [layerIndex]);

  const box = useMemo(() => (layer ? layerBox(layer) : null), [layer]);

  return (
    <div className="toolpath-3d">
      <Canvas camera={{ position: [0, 80, 120], fov: 42 }} gl={{ antialias: true }}>
        <color attach="background" args={["#06080c"]} />
        <ambientLight intensity={0.6} />
        <Suspense fallback={null}>
          {layer && <LayerLines layer={layer} />}
          {box && <FitTo box={box} />}
        </Suspense>
        <OrbitControls makeDefault enableDamping dampingFactor={0.08} maxPolarAngle={Math.PI / 2.05} />
      </Canvas>
    </div>
  );
}
