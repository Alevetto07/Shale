import { describe, expect, it } from "vitest";
import {
  defaultAmsMapping,
  plateWarnings,
  slotWarnings,
  type SpoolChoice,
} from "./print-check";

const spools: SpoolChoice[] = [
  { bay: 1, material: "PLA Basic", colour: "#3B82F6" },
  { bay: 2, material: "PETG", colour: "#EF4444" },
  { bay: 3, material: "", colour: "" },
];

describe("plate warnings", () => {
  it("is quiet when PLA is on textured PEI at a usual bed temperature", () => {
    expect(plateWarnings("PLA Basic", "textured_plate", 60)).toEqual([]);
  });

  it("warns when PLA is on an engineering plate", () => {
    expect(plateWarnings("Bambu PLA", "engineering_plate", 60)[0]).toMatch(/stick too hard/);
  });

  it("warns when the bed is far from the usual band", () => {
    const notes = plateWarnings("PLA Basic", "cool_plate", 70);
    expect(notes.some((note) => note.includes("70 °C") && note.includes("35–45"))).toBe(true);
  });

  it("warns that PETG does not belong on a cool plate", () => {
    expect(plateWarnings("PETG", "cool_plate", 35)[0]).toMatch(/too cold/);
  });

  it("says nothing about a filament it does not recognise", () => {
    expect(plateWarnings("Mystery blend", "textured_plate", 60)).toEqual([]);
  });

  it("mentions a cold bed when the filament is unknown and the bed is off", () => {
    expect(plateWarnings("", "textured_plate", 0)).toEqual([
      "The bed is set to 0 °C, so this filament may not stick.",
    ]);
  });
});

describe("AMS mapping", () => {
  it("puts tool 0 on the external spool and the next tool on the first tray", () => {
    expect(defaultAmsMapping([0, 1], 3)).toEqual([-1, 0]);
  });

  it("does not invent a tray the machine does not have", () => {
    expect(defaultAmsMapping([0, 2], 1)).toEqual([-1, -1, -1]);
  });

  it("warns when the mapped spool is a different material", () => {
    const notes = slotWarnings([0], [0], spools);
    expect(notes).toHaveLength(1);
    expect(notes[0]?.message).toMatch(/mapped to PETG/);
    expect(notes[0]?.message).toMatch(/PLA Basic/);
  });

  it("warns when the slot is empty", () => {
    const notes = slotWarnings([1], [-1, 1], spools);
    expect(notes[0]?.message).toMatch(/empty slot/);
  });

  it("warns on colour when the material matches", () => {
    const painted = [
      { bay: 1, material: "PLA Basic", colour: "#3B82F6" },
      { bay: 2, material: "PLA Basic", colour: "#111111" },
    ];
    const notes = slotWarnings([0], [0], painted);
    expect(notes[0]?.message).toMatch(/colour/);
  });

  it("is quiet when the tool stays on the spool it was sliced with", () => {
    expect(slotWarnings([0, 1], [-1, 0], spools)).toEqual([]);
  });
});
