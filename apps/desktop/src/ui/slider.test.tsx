import { render, screen, fireEvent } from "@testing-library/react";
import { beforeAll, describe, expect, it, vi } from "vitest";
import { Slider } from "./slider";

describe("Slider", () => {
  beforeAll(() => {
    class MockResizeObserver {
      observe() {}
      unobserve() {}
      disconnect() {}
    }
    vi.stubGlobal("ResizeObserver", MockResizeObserver);
  });
  it("exposes a slider role and forwards value changes", () => {
    const onValueChange = vi.fn();
    render(
      <Slider
        label="Layer height"
        value={[0.2]}
        min={0.1}
        max={0.4}
        step={0.05}
        onValueChange={onValueChange}
      />,
    );
    const control = screen.getByRole("slider", { name: "Layer height" });
    expect(control).toBeTruthy();
    const input = screen.getByLabelText("Layer height exact value");
    fireEvent.change(input, { target: { value: "0.25" } });
    expect(onValueChange).toHaveBeenCalledWith(0.25);
  });
});