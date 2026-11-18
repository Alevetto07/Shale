import { Suspense, useEffect, useState } from "react";
import { Canvas } from "@react-three/fiber";
import { Line } from "@react-three/drei";

type Layer = {
  walls: number[][][];
  infill: number[][][];
};

function LayerLines({ layer }: { layer: Layer }) {
  return (
    <group>
      {layer.walls.map((path, i) => {
        const pts = path.map(([x, y]) => [x - 128, 0.4, 128 - y] as [number, number, number]);
        return <Line key={`w${i}`} points={pts} color="#e9e8f2" lineWidth={1} />;
      })}
      {layer.infill.map((path, i) => {
        const pts = path.map(([x, y]) => [x - 128, 0.4, 128 - y] as [number, number, number]);
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

  return (
    <div className="toolpath-3d">
      <Canvas camera={{ position: [0, 80, 120], fov: 42 }} gl={{ antialias: true }}>
        <color attach="background" args={["#06080c"]} />
        <ambientLight intensity={0.6} />
        <Suspense fallback={null}>
          {layer && <LayerLines layer={layer} />}
        </Suspense>
      </Canvas>
    </div>
  );
}