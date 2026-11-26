/**
 * Checks made before a print leaves the computer.
 *
 * The plate type and the bed temperature come from the profile and the plate
 * the person picked. The slot check compares the filament the slice was made
 * with against the spool they mapped that tool onto. Neither check blocks a
 * send: a warning is something to read, and the press is still theirs.
 */

export interface SpoolChoice {
  /** 1-based bay. Bay 1 is the external spool. */
  bay: number;
  material: string;
  colour?: string;
}

export interface SlotWarning {
  tool: number;
  message: string;
}

const PLATE_LABEL: Record<string, string> = {
  textured_plate: "a textured PEI plate",
  smooth_plate: "a smooth PEI plate",
  engineering_plate: "an engineering plate",
  cool_plate: "a cool plate",
};

/** The plate the device pane last stored, or textured PEI when none has been chosen. */
export function storedBedType(): string {
  if (typeof localStorage === "undefined") return "textured_plate";
  const stored = localStorage.getItem("shale.device.bed");
  return stored && stored in PLATE_LABEL ? stored : "textured_plate";
}

function family(filament: string): string {
  const name = filament.toLowerCase();
  if (name.includes("petg")) return "petg";
  if (name.includes("abs")) return "abs";
  if (name.includes("asa")) return "asa";
  if (name.includes("tpu") || name.includes("tpe")) return "tpu";
  if (name.includes("nylon") || name.includes("polyamide") || /\bpa[- ]?\d*\b/.test(name)) return "pa";
  if (name.includes("polycarbonate") || /\bpc\b/.test(name)) return "pc";
  if (name.includes("pla")) return "pla";
  return "";
}

function plateName(bedType: string): string {
  return PLATE_LABEL[bedType] ?? "this plate";
}

interface Band {
  /** Plates this filament is happy on, with the bed range in °C. */
  happy: Record<string, [number, number]>;
  /** Plates that are a poor match, and why. */
  avoid: Record<string, string>;
}

const BANDS: Record<string, Band> = {
  pla: {
    happy: {
      textured_plate: [55, 65],
      smooth_plate: [55, 65],
      cool_plate: [35, 45],
    },
    avoid: {
      engineering_plate: "PLA on an engineering plate can stick too hard. A textured PEI plate is the usual choice.",
    },
  },
  petg: {
    happy: {
      textured_plate: [70, 80],
      engineering_plate: [70, 80],
    },
    avoid: {
      smooth_plate: "PETG on a smooth PEI plate can bond to it. A textured PEI plate is the safer choice.",
      cool_plate: "PETG on a cool plate is usually too cold to hold. A textured PEI plate at about 70–80 °C is the usual choice.",
    },
  },
  abs: {
    happy: { engineering_plate: [90, 110] },
    avoid: {
      textured_plate: "ABS on a textured PEI plate often will not hold. An engineering plate at about 90–110 °C is the usual choice.",
      smooth_plate: "ABS on a smooth PEI plate often will not hold. An engineering plate at about 90–110 °C is the usual choice.",
      cool_plate: "ABS on a cool plate will not hold. An engineering plate at about 90–110 °C is the usual choice.",
    },
  },
  asa: {
    happy: { engineering_plate: [90, 110] },
    avoid: {
      textured_plate: "ASA on a textured PEI plate often will not hold. An engineering plate at about 90–110 °C is the usual choice.",
      smooth_plate: "ASA on a smooth PEI plate often will not hold. An engineering plate at about 90–110 °C is the usual choice.",
      cool_plate: "ASA on a cool plate will not hold. An engineering plate at about 90–110 °C is the usual choice.",
    },
  },
  tpu: {
    happy: {
      textured_plate: [30, 45],
      cool_plate: [30, 40],
    },
    avoid: {
      engineering_plate: "TPU on an engineering plate can stick too hard. A textured PEI plate is the usual choice.",
      smooth_plate: "TPU on a smooth PEI plate can stick too hard. A textured PEI plate is the usual choice.",
    },
  },
  pa: {
    happy: { engineering_plate: [80, 100] },
    avoid: {
      textured_plate: "Nylon on a textured PEI plate often will not hold. An engineering plate is the usual choice.",
      smooth_plate: "Nylon on a smooth PEI plate often will not hold. An engineering plate is the usual choice.",
      cool_plate: "Nylon on a cool plate will not hold. An engineering plate is the usual choice.",
    },
  },
  pc: {
    happy: { engineering_plate: [100, 120] },
    avoid: {
      textured_plate: "Polycarbonate on a textured PEI plate often will not hold. An engineering plate is the usual choice.",
      smooth_plate: "Polycarbonate on a smooth PEI plate often will not hold. An engineering plate is the usual choice.",
      cool_plate: "Polycarbonate on a cool plate will not hold. An engineering plate is the usual choice.",
    },
  },
};

/**
 * Warnings for the plate under this filament, and for a bed temperature that
 * sits outside the usual band. An unknown filament says nothing: inventing a
 * range would be worse than staying quiet.
 */
export function plateWarnings(filament: string, bedType: string, bedTempC: number | null): string[] {
  const kind = family(filament);
  if (!kind) {
    if (bedTempC === 0) return ["The bed is set to 0 °C, so this filament may not stick."];
    return [];
  }
  const band = BANDS[kind];
  if (!band) return [];
  const notes: string[] = [];
  const avoided = band.avoid[bedType];
  const happy = band.happy[bedType];
  if (avoided) notes.push(avoided);
  if (happy && bedTempC != null && (bedTempC < happy[0] - 8 || bedTempC > happy[1] + 8)) {
    const material = filament.trim() || kind.toUpperCase();
    notes.push(
      `The bed is set to ${Math.round(bedTempC)} °C. ${material} on ${plateName(bedType)} is usually ${happy[0]}–${happy[1]} °C.`,
    );
  }
  return notes;
}

function normalMaterial(value: string): string {
  return value
    .toLowerCase()
    .replace(/bambu\s*lab/g, "")
    .replace(/\s+/g, " ")
    .trim();
}

function normalColour(value: string | undefined): string {
  return (value ?? "").replace(/#/g, "").trim().toLowerCase();
}

/** Tray index for a bay. Bay 1 is the external spool (−1). Bay 2 is AMS tray 0. */
export function trayForBay(bay: number): number {
  return bay <= 1 ? -1 : bay - 2;
}

/** Bay for a tray. −1 is the external spool. */
export function bayForTray(tray: number): number {
  return tray < 0 ? 1 : tray + 2;
}

/**
 * One entry per tool index, including tools the slice never selects (those
 * stay on the external spool). Tool 0 is the external spool. Tool 1 is the
 * first AMS tray, when that tray exists.
 */
export function defaultAmsMapping(tools: number[], trayCount: number): number[] {
  const max = tools.reduce((highest, tool) => Math.max(highest, tool), -1);
  if (max < 0) return [];
  return Array.from({ length: max + 1 }, (_, tool) => {
    if (!tools.includes(tool) || tool <= 0) return -1;
    const tray = tool - 1;
    return tray < trayCount ? tray : -1;
  });
}

export function spoolForTray(tray: number, spools: SpoolChoice[]): SpoolChoice | undefined {
  const bay = bayForTray(tray);
  return spools.find((spool) => spool.bay === bay);
}

/**
 * The material the slice used for a tool. Tool 0 is the plate's own spool,
 * and tool N is bay N+1.
 */
export function slicedMaterial(tool: number, spools: SpoolChoice[]): SpoolChoice | undefined {
  return spools.find((spool) => spool.bay === tool + 1);
}

/** Warnings for a mapping. `mapping[tool]` is an AMS tray, or −1 for the external spool. */
export function slotWarnings(tools: number[], mapping: number[], spools: SpoolChoice[]): SlotWarning[] {
  const notes: SlotWarning[] = [];
  for (const tool of tools) {
    const chosen = spoolForTray(mapping[tool] ?? -1, spools);
    const expected = slicedMaterial(tool, spools);
    const label = `T${tool + 1}`;
    if (!chosen || !chosen.material.trim()) {
      notes.push({
        tool,
        message: `${label} is mapped to an empty slot. Choose a spool that holds filament, or the printer will have nothing to feed.`,
      });
      continue;
    }
    const want = normalMaterial(expected?.material ?? "");
    const have = normalMaterial(chosen.material);
    if (want && have && want !== have) {
      notes.push({
        tool,
        message: `${label} is mapped to ${chosen.material}, but this slice used ${expected?.material} for that tool.`,
      });
      continue;
    }
    const wantColour = normalColour(expected?.colour);
    const haveColour = normalColour(chosen.colour);
    if (want && have && wantColour && haveColour && wantColour !== haveColour) {
      notes.push({
        tool,
        message: `${label} is the right material, but the colour does not match the filament this slice used.`,
      });
    }
  }
  return notes;
}
