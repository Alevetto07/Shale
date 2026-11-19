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
  loading: boolean;
  plateMm: number;
  printerId: string;
  layer: number;
  playing: boolean;
  showTravel: boolean;
  layerHeight: number;
  infill: number;
  walls: number;
  setLayer: React.Dispatch<React.SetStateAction<number>>;
  setPlaying: (v: boolean) => void;
  setShowTravel: (v: boolean) => void;
  setLayerHeight: (v: number) => void;
  setInfill: (v: number) => void;
  setWalls: (v: number) => void;
  selectPrinter: (p: PrinterRow) => void;
  visibleLayerCount: number;
  estimates: { layers: number; filament_m: number; time_s: number };
  loadSlice: (url?: string) => void;
};

const DemoContext = createContext<DemoState | null>(null);

const DEFAULT_SLICE = "/demo/demo-slice.json";

let sliceCache: Promise<SliceData> | null = null;

function fetchSliceData(url: string): Promise<SliceData> {
  if (!sliceCache) {
    sliceCache = fetch(url).then((r) => {
      if (!r.ok) throw new Error("slice fetch failed");
      return r.json();
    });
  }
  return sliceCache;
}

export function DemoProvider({
  children,
  eager = false,
  sliceUrl = DEFAULT_SLICE,
  syncTimeline = false,
}: {
  children: ReactNode;
  eager?: boolean;
  sliceUrl?: string;
  /** When true, follow the activity timeline's window scrub events.
   *  Only the app-window preview should opt in: the workbench and hero
   *  keep their own layer, otherwise the timeline resets them to layer 0. */
  syncTimeline?: boolean;
}) {
  const [data, setData] = useState<SliceData | null>(null);
  const [loading, setLoading] = useState(false);
  const [plateMm, setPlateMm] = useState(256);
  const [printerId, setPrinterId] = useState("bambu_lab_a1");
  const [layer, setLayer] = useState(0);
  const [playing, setPlaying] = useState(false);
  const [showTravel, setShowTravel] = useState(false);
  const [layerHeight, setLayerHeight] = useState(0.2);
  const [infill, setInfill] = useState(20);
  const [walls, setWalls] = useState(2);
  const [armed, setArmed] = useState(eager);

  const loadSlice = useCallback(
    (url = sliceUrl) => {
      if (!armed && !eager) return;
      setLoading(true);
      fetchSliceData(url)
        .then((json: SliceData) => {
          setData(json);
          setLayer(json.layers.length - 1);
        })
        .catch(() => setData(null))
        .finally(() => setLoading(false));
    },
    [armed, eager, sliceUrl],
  );

  useEffect(() => {
    if (eager) loadSlice(sliceUrl);
  }, [eager, loadSlice, sliceUrl]);

  useEffect(() => {
    if (!syncTimeline) return;
    const onScrub = (e: Event) => {
      const { layer } = (e as CustomEvent<{ layer: number }>).detail ?? {};
      if (typeof layer === "number" && data) {
        setLayer(Math.min(layer, data.layers.length - 1));
      }
    };
    window.addEventListener("shale-timeline-scrub", onScrub);
    return () => window.removeEventListener("shale-timeline-scrub", onScrub);
  }, [data, syncTimeline]);

  const baseMeta = data?.meta ?? {};
  const baseLayers = Number(baseMeta.layers ?? 120);
  const baseFil = Number(baseMeta.filament_m ?? 0.9508);
  const baseTime = Number(baseMeta.time_s ?? 863);
  const baseLh = Number(baseMeta.layer_height_mm ?? 0.2);

  const visibleLayerCount = useMemo(() => {
    const ratio = baseLh / layerHeight;
    return Math.max(1, Math.min(baseLayers, Math.round(baseLayers * ratio)));
  }, [baseLayers, baseLh, layerHeight]);

  const estimates = useMemo(() => {
    const layerScale = baseLh / layerHeight;
    const infillScale = 0.85 + (infill / 100) * 0.3;
    const wallScale = 1 + (walls - 3) * 0.06;
    return {
      layers: Math.ceil(baseLayers * layerScale),
      filament_m: baseFil * layerScale * infillScale * wallScale,
      time_s: baseTime * layerScale * infillScale * wallScale,
    };
  }, [baseLayers, baseFil, baseTime, baseLh, layerHeight, infill, walls]);

  const selectPrinter = useCallback((p: PrinterRow) => {
    setPrinterId(p.id);
    setPlateMm(p.bed_mm);
  }, []);

  const value: DemoState = {
    data,
    loading,
    plateMm,
    printerId,
    layer,
    playing,
    showTravel,
    layerHeight,
    infill,
    walls,
    setLayer,
    setPlaying,
    setShowTravel,
    setLayerHeight,
    setInfill,
    setWalls,
    selectPrinter,
    visibleLayerCount,
    estimates,
    loadSlice,
  };

  return (
    <DemoContext.Provider value={value}>
      <DemoArmOnVisible armed={armed} setArmed={setArmed} onArm={() => loadSlice(sliceUrl)} />
      {children}
    </DemoContext.Provider>
  );
}

function DemoArmOnVisible({
  armed,
  setArmed,
  onArm,
}: {
  armed: boolean;
  setArmed: (v: boolean) => void;
  onArm: () => void;
}) {
  useEffect(() => {
    if (armed) return;
    const el = document.querySelector("#demos, #app-demo, #toolpath");
    if (!el) return;
    const io = new IntersectionObserver(
      (entries) => {
        if (entries.some((e) => e.isIntersecting)) {
          setArmed(true);
          onArm();
          io.disconnect();
        }
      },
      { rootMargin: "120px" },
    );
    io.observe(el);
    return () => io.disconnect();
  }, [armed, setArmed, onArm]);
  return null;
}

export function useDemo() {
  const ctx = useContext(DemoContext);
  if (!ctx) throw new Error("useDemo outside DemoProvider");
  return ctx;
}