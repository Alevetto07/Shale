import { cleanup, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { SendReview } from "./send-review";

const spools = [
  { bay: 1, material: "PLA Basic", colour: "#3B82F6" },
  { bay: 2, material: "PETG", colour: "#EF4444" },
];

afterEach(() => cleanup());

describe("SendReview", () => {
  it("warns when the mapped spool is a different material", () => {
    render(
      <SendReview
        filament="PLA Basic"
        bedTempC={60}
        bedType="textured_plate"
        onBedType={vi.fn()}
        tools={{ tools: [0], changes: 0, file: "plate.gcode", usage: [{ tool: 0, metres: 1.2 }] }}
        spools={spools}
        mapping={[0]}
        onMapping={vi.fn()}
        plates={[{ index: 0, name: "Plate 1", active: true, sliced: true }]}
        scope="plate"
        onScope={vi.fn()}
        plateIndex={0}
        onPlate={vi.fn()}
      />,
    );
    expect(screen.getByText(/mapped to PETG/)).toBeTruthy();
    expect(screen.getByText(/PLA Basic/)).toBeTruthy();
    expect(document.body.textContent).not.toMatch(/deep field/i);
  });

  it("warns when PLA is sent to an engineering plate", () => {
    render(
      <SendReview
        filament="PLA Basic"
        bedTempC={60}
        bedType="engineering_plate"
        onBedType={vi.fn()}
        tools={null}
        spools={spools}
        mapping={[]}
        onMapping={vi.fn()}
        plates={[]}
        scope="plate"
        onScope={vi.fn()}
        plateIndex={0}
        onPlate={vi.fn()}
      />,
    );
    expect(screen.getByText(/stick too hard/)).toBeTruthy();
  });

  it("offers every plate, and holds that choice until each plate is sliced", () => {
    render(
      <SendReview
        filament="PLA Basic"
        bedTempC={60}
        bedType="textured_plate"
        onBedType={vi.fn()}
        tools={{ tools: [0], changes: 0, file: null, usage: [] }}
        spools={spools}
        mapping={[-1]}
        onMapping={vi.fn()}
        plates={[
          { index: 0, name: "Plate 1", active: true, sliced: true },
          { index: 1, name: "Plate 2", active: false, sliced: false },
        ]}
        scope="all"
        onScope={vi.fn()}
        plateIndex={0}
        onPlate={vi.fn()}
      />,
    );
    expect((screen.getByRole("radio", { name: /Every plate/ }) as HTMLInputElement).disabled).toBe(true);
    expect(screen.getByText(/Slice Plate 2/)).toBeTruthy();
  });

  it("shows Studio's send options and hides flow when the printer has none", () => {
    render(
      <SendReview
        filament="PLA Basic"
        bedTempC={60}
        bedType="textured_plate"
        onBedType={vi.fn()}
        tools={{ tools: [0], changes: 0, file: null, usage: [] }}
        spools={spools}
        mapping={[-1]}
        onMapping={vi.fn()}
        plates={[{ index: 0, name: "Plate 1", active: true, sliced: true }]}
        scope="plate"
        onScope={vi.fn()}
        plateIndex={0}
        onPlate={vi.fn()}
        options={{
          timelapse: false,
          bedLevelling: true,
          flowCali: false,
          flowAvailable: false,
          vibrationCali: false,
          vibrationAvailable: true,
          useAms: true,
          automaticBed: false,
          automaticFlow: false,
        }}
        onOptions={vi.fn()}
      />,
    );
    expect(screen.getByText("Timelapse")).toBeTruthy();
    expect(screen.getByText("Bed levelling")).toBeTruthy();
    expect(screen.getByText("Vibration compensation")).toBeTruthy();
    expect(screen.getByText("Use AMS")).toBeTruthy();
    expect(screen.queryByText("Flow dynamics calibration")).toBeNull();
    expect(document.body.textContent).not.toMatch(/deep field/i);
  });
});
