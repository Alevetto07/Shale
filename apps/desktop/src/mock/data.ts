import type {
  Bed,
  CloudDeviceRow,
  CloudSession,
  CostEstimate,
  CostSettings,
  HostStatus,
  MaterialInfo,
  ModelView,
  PreviewPath,
  PrinterInfo,
  QualityInfo,
  RecentProject,
  SliceSummary,
  Status,
  VariantInfo,
} from "../api";

export const MOCK_PRINTERS: PrinterInfo[] = [
  {
    id: "bambu_lab_a1",
    name: "A1",
    manufacturer: "Bambu Lab",
    preferred_quality_type: "normal",
    preferred_variant: "0.4mm Nozzle",
    preferred_material: "bambu_lab_pla_basic",
    source: "bambu",
  },
  {
    id: "bambu_lab_a1_mini",
    name: "A1 mini",
    manufacturer: "Bambu Lab",
    preferred_quality_type: "normal",
    preferred_variant: "0.4mm Nozzle",
    preferred_material: "bambu_lab_pla_basic",
    source: "bambu",
  },
  {
    id: "bambu_lab_p1s",
    name: "P1S",
    manufacturer: "Bambu Lab",
    preferred_quality_type: "normal",
    preferred_variant: "0.4mm Nozzle",
    preferred_material: "bambu_lab_pla_basic",
    source: "bambu",
  },
];

export const MOCK_VARIANTS: VariantInfo[] = [{ name: "0.4mm Nozzle", nozzle_size: 0.4 }];

export const MOCK_QUALITIES: QualityInfo[] = [
  { quality_type: "0.20mm Standard @BBL A1", name: "0.20mm Standard @BBL A1" },
  { quality_type: "0.28mm Extra Draft @BBL A1", name: "0.28mm Extra Draft @BBL A1" },
];

export const MOCK_MATERIALS: MaterialInfo[] = [
  {
    id: "bambu_lab_pla_basic",
    label: "Bambu PLA Basic",
    color: "#b9b3ff",
    diameter: 1.75,
    density: 1.24,
  },
  {
    id: "bambu_lab_petg_hf",
    label: "Bambu PETG HF",
    color: "#7ab8ff",
    diameter: 1.75,
    density: 1.27,
  },
];

export function mockStatus(): Status {
  return {
    resources_ok: true,
    resources_message: "",
    engine_path: "C:\\mock\\CuraEngine.exe",
    engine_lib_dir: "C:\\mock\\cura",
    engine_ok: true,
    engine_message: "",
    engine_backend: "cli",
    printer_count: MOCK_PRINTERS.length,
    version: "0.1.4",
    catalog_loading: false,
    catalog_full: true,
  };
}

export function mockBed(): Bed {
  return {
    width: 256,
    depth: 256,
    height: 250,
    center_is_zero: true,
    layer_height: 0.2,
    infill_sparse_density: 15,
    support_enable: false,
    nozzle_temperature: 220,
    bed_temperature: 60,
    nozzle_size: 0.4,
    line_width: 0.42,
    adhesion_type: "skirt",
    speed_print: 200,
    material_diameter: 1.75,
    warning_count: 0,
  };
}

export function mockPlates(modelCount = 1): import("../api").PlateView[] {
  return [{ index: 0, name: "Plate 1", models: modelCount, active: true }];
}

export function mockModel(id = 1): ModelView {
  return {
    id,
    name: "cube.stl",
    positions: [],
    transform: {
      translation: [0, 0, 5],
      rotation_deg: [0, 0, 0],
      scale: [1, 1, 1],
    },
    triangles: 12,
    max_deviation_mm: null,
    max_turning_deg: null,
    import_ms: 42,
    source: "stl",
    settings: {},
    slot: null,
  };
}

export function cubeGeometryBuffer(): ArrayBuffer {
  const positions = new Float32Array([
    -10, -10, 0, 10, -10, 0, 10, 10, 0,
    -10, -10, 0, 10, 10, 0, -10, 10, 0,
    -10, -10, 10, 10, 10, 10, 10, -10, 10,
    -10, -10, 10, -10, 10, 10, 10, -10, 10,
    -10, -10, 0, -10, -10, 10, 10, -10, 10,
    -10, -10, 0, 10, -10, 10, 10, -10, 0,
    -10, 10, 0, 10, 10, 10, -10, 10, 10,
    -10, 10, 0, -10, 10, 10, 10, 10, 0,
    -10, -10, 0, -10, 10, 10, -10, -10, 10,
    -10, -10, 0, -10, 10, 0, -10, 10, 10,
    10, -10, 0, 10, -10, 10, 10, 10, 10,
    10, -10, 0, 10, 10, 10, 10, 10, 0,
  ]);
  const buf = new ArrayBuffer(4 + positions.byteLength);
  new DataView(buf).setUint32(0, positions.length, true);
  new Float32Array(buf, 4).set(positions);
  return buf;
}

export function mockSlice(modelIds: number[]): SliceSummary {
  const layers = Array.from({ length: 54 }, (_, index) => ({
    index,
    z: (index + 1) * 0.2,
  }));
  return {
    layer_count: layers.length,
    layers,
    print_time_seconds: 29 * 60,
    filament_metres: 1.42,
    warning_count: 0,
    gcode_path: "C:\\mock\\plate.gcode",
    model_ids: modelIds,
    machine_gcode: true,
  };
}

export function mockPreviewPaths(): PreviewPath[] {
  const paths: PreviewPath[] = [];
  for (let layer = 0; layer < 20; layer += 1) {
    const z = (layer + 1) * 0.2;
    paths.push({
      kind: layer % 3 === 0 ? "wall" : "infill",
      layer,
      points: [
        [-8, -8, z],
        [8, -8, z],
        [8, 8, z],
        [-8, 8, z],
        [-8, -8, z],
      ],
    });
  }
  return paths;
}

export function mockRecents(): RecentProject[] {
  const now = Date.now();
  return [
    {
      path: "C:\\Projects\\arm-bracket.shale",
      name: "arm-bracket",
      saved_at_ms: now - 86_400_000,
      printer: "Bambu Lab A1",
      missing: false,
    },
    {
      path: "C:\\Projects\\calibration-cube.shale",
      name: "calibration-cube",
      saved_at_ms: now - 604_800_000,
      printer: "Bambu Lab A1",
      missing: false,
    },
  ];
}

export function mockCostSettings(): CostSettings {
  return {
    currency: "EUR",
    price_per_kg: 20,
    watts: 100,
    electricity_per_kwh: 0.3,
  };
}

export function mockCostEstimate(): CostEstimate {
  return {
    spools: [{ slot: 0, metres: 1.42, grams: 42, cost: 0.84 }],
    grams: 42,
    filament_cost: 0.84,
    kwh: 0.048,
    energy_cost: 0.014,
    total: 0.86,
    currency: "EUR",
  };
}

export function mockHost(connected: boolean): HostStatus {
  return {
    connected,
    state: connected ? "IDLE" : "",
    percent: connected ? 0 : 0,
    remainingMin: 0,
    layer: 0,
    layers: 0,
    nozzle: connected ? 28 : 0,
    nozzleTarget: connected ? 0 : 0,
    bed: connected ? 26 : 0,
    bedTarget: connected ? 0 : 0,
    file: "",
    error: 0,
    note: connected ? "" : "not connected",
    reports: connected ? 3 : 0,
    updatedAt: Date.now(),
  };
}

export function mockCloud(signedIn: boolean): CloudSession {
  return {
    signedIn,
    account: signedIn ? "alessio@example.com" : null,
    host: "https://api.bambulab.com",
  };
}

export function mockCloudDevices(): CloudDeviceRow[] {
  return [
    {
      devId: "mock-a1-01",
      label: "Workshop A1",
      model: "A1",
      state: "idle",
      mayPrint: true,
    },
    {
      devId: "mock-x1-01",
      label: "Office X1C",
      model: "X1 Carbon",
      state: "printing 37%",
      mayPrint: false,
    },
    {
      devId: "mock-offline",
      label: "Spare P1S",
      model: "P1S",
      state: "offline",
      mayPrint: false,
    },
  ];
}