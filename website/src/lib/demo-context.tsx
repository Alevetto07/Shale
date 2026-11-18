import type React from "react";
import {
  createContext,
  useCallback,
  useContext,
  useEffect,
  useMemo,
  useState,
  type ReactNode,
} from "react";

export type Segment = { type: string; points: number[][] };
export type Layer = { z: number; segments: Segment[] };
export type SliceData = {
  source: string;
  meta: Record<string, unknown>;
  plate_mm: number;
  layers: Layer[];
};

export type PrinterRow = {
  id: string;
  name: string;
  manufacturer: string;
  bed_mm: number;
  nozzle_mm?: number;
  silhouette?: "mini" | "core" | "enclosed";
};

type DemoState = {
  data: SliceData | null;
  plateMm: number;
  printerId: string;
  layer: number;
  playing: boolean;
  layerHeight: number;
  infill: number;
  walls: number;
  setLayer: React.Dispatch<React.SetStateAction<number>>;
  setPlaying: (v: boolean) => void;
  setLayerHeight: (v: number) => void;
  setInfill: (v: number) => void;
  setWalls: (v: number) => void;
  selectPrinter: (p: PrinterRow) => void;
  visibleLayerCount: number;
  estimates: { layers: number; filament_m: number; time_s: number };
};

const DemoContext = createContext<DemoState | null>(null);

export function DemoProvider({ children }: { children: ReactNode }) {
  const [data, setData] = useState<SliceData | null>(null);
  const [plateMm, setPlateMm] = useState(256);
  const [printerId, setPrinterId] = useState("bambu_lab_a1");
  const [layer, setLayer] = useState(0);
  const [playing, setPlaying] = useState(false);
  const [layerHeight, setLayerHeight] = useState(0.2);
  const [infill, setInfill] = useState(20);
  const [walls, setWalls] = useState(3);

  useEffect(() => {
    fetch("/demo/demo-slice.json")
      .then((r) => r.json())
      .then((json: SliceData) => {
        setData(json);
        setLayer(json.layers.length - 1);
      })
      .catch(() => setData(null));
  }, []);

  const visibleLayerCount = useMemo(() => {
    if (!data) return 0;
    const base = data.meta.layer_height_mm as number;
    const ratio = base / layerHeight;
    return Math.max(1, Math.min(data.layers.length, Math.round(data.layers.length * ratio)));
  }, [data, layerHeight]);

  const estimates = useMemo(() => {
    const base = data?.meta ?? {};
    const baseLayers = Number(base.layers ?? 100);
    const baseFil = Number(base.filament_m ?? 1.46);
    const baseTime = Number(base.time_s ?? 48);
    const layerScale = 0.2 / layerHeight;
    const infillScale = infill / 20;
    const wallScale = 1 + (walls - 3) * 0.08;
    return {
      layers: Math.ceil(baseLayers * layerScale),
      filament_m: baseFil * layerScale * infillScale * wallScale,
      time_s: baseTime * layerScale * infillScale * wallScale,
    };
  }, [data, layerHeight, infill, walls]);

  const selectPrinter = useCallback((p: PrinterRow) => {
    setPrinterId(p.id);
    setPlateMm(p.bed_mm);
  }, []);

  const value: DemoState = {
    data,
    plateMm,
    printerId,
    layer,
    playing,
    layerHeight,
    infill,
    walls,
    setLayer,
    setPlaying,
    setLayerHeight,
    setInfill,
    setWalls,
    selectPrinter,
    visibleLayerCount,
    estimates,
  };

  return <DemoContext.Provider value={value}>{children}</DemoContext.Provider>;
}

export function useDemo() {
  const ctx = useContext(DemoContext);
  if (!ctx) throw new Error("useDemo outside DemoProvider");
  return ctx;
}