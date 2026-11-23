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
    expect(screen.getByRole("slider", { name: "Layer height" })).toBeTruthy();
    fireEvent.click(screen.getByRole("button", { name: "Layer height exact value" }));
    const input = screen.getByLabelText("Layer height exact value");
    fireEvent.change(input, { target: { value: "0.25" } });
    expect(onValueChange).toHaveBeenCalledWith(0.25);
  });

  it("shows a single value+unit label away from the thumb", () => {
    const { rerender } = render(
      <Slider label="Max angle" value={[45]} min={0} max={90} step={1} unit="°" inputDigits={0} onValueChange={() => undefined} />,
    );
    expect(screen.getByRole("button", { name: "Max angle exact value" }).textContent).toBe("45°");

    rerender(
      <Slider
        label="Layer height"
        value={[0.2]}
        min={0.08}
        max={0.4}
        step={0.01}
        unit="mm"
        inputDigits={2}
        onValueChange={() => undefined}
      />,
    );
    expect(screen.getByRole("button", { name: "Layer height exact value" }).textContent).toBe("0.20 mm");
  });
});