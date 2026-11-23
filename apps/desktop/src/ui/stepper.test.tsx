import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeAll, describe, expect, it, vi } from "vitest";
import { Stepper } from "./stepper";
import { NumericControl } from "./numeric-control";

describe("Stepper", () => {
  afterEach(() => cleanup());
  beforeAll(() => {
    class MockResizeObserver {
      observe() {}
      unobserve() {}
      disconnect() {}
    }
    vi.stubGlobal("ResizeObserver", MockResizeObserver);
  });

  it("steps with chevrons and keeps the accessible name", () => {
    const onChange = vi.fn();
    render(<Stepper label="X" value={10} step={1} digits={0} onChange={onChange} />);
    expect(screen.getByLabelText("X")).toBeTruthy();
    fireEvent.click(screen.getByRole("button", { name: "Increase X" }));
    expect(onChange).toHaveBeenCalledWith(11);
    fireEvent.click(screen.getByRole("button", { name: "Decrease X" }));
    expect(onChange).toHaveBeenCalledWith(9);
  });
});

describe("NumericControl", () => {
  afterEach(() => cleanup());

  it("uses a slider when both ends of the range exist", () => {
    render(
      <NumericControl label="Layer height" value={0.2} min={0.05} max={0.8} step={0.01} digits={2} onChange={() => undefined} />,
    );
    expect(screen.getByRole("slider", { name: "Layer height" })).toBeTruthy();
  });

  it("uses a stepper when the value is unbounded", () => {
    render(<NumericControl label="X" value={1} step={1} digits={0} onChange={() => undefined} />);
    expect(screen.queryByRole("slider")).toBeNull();
    expect(screen.getByRole("button", { name: "Increase X" })).toBeTruthy();
  });
});
