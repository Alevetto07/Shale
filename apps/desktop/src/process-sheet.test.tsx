import { fireEvent, render, screen } from "@testing-library/react";
import { beforeAll, describe, expect, it, vi } from "vitest";
import { ProcessSheet } from "./ProcessSheet";
import { TooltipProvider } from "./ui";

const fallbacks = {
  layerHeight: 0.2,
  walls: 3,
  infill: 15,
  pattern: "grid",
  supports: false,
  speed: 200,
  nozzle: 220,
  bed: 60,
};

describe("ProcessSheet", () => {
  beforeAll(() => {
    class MockResizeObserver {
      observe() {}
      unobserve() {}
      disconnect() {}
    }
    vi.stubGlobal("ResizeObserver", MockResizeObserver);
  });

  it("uses the filled slider for layer height and keeps a non-planar slot", async () => {
    render(
      <TooltipProvider>
      <ProcessSheet
        fallbacks={fallbacks}
        overrides={{}}
        onOverride={() => undefined}
        onResetOverrides={() => undefined}
        qualities={[]}
        qualityType={null}
        onQuality={() => undefined}
        printerLabel="Bambu Lab A1"
      />
      </TooltipProvider>,
    );
    expect(screen.getByRole("slider", { name: "Layer height" })).toBeTruthy();
    const tab = screen.getByRole("tab", { name: "Non-planar" });
    fireEvent.mouseDown(tab, { button: 0 });
    fireEvent.click(tab);
    expect(await screen.findByText(/coming in v0\.2/i)).toBeTruthy();
    expect(screen.getByRole("slider", { name: "Max angle" })).toBeTruthy();
  });
});
