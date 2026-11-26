import { fireEvent, render, screen } from "@testing-library/react";
import { beforeAll, describe, expect, it, vi } from "vitest";
import { Select } from "./select";

describe("Select", () => {
  beforeAll(() => {
    class MockResizeObserver {
      observe() {}
      unobserve() {}
      disconnect() {}
    }
    vi.stubGlobal("ResizeObserver", MockResizeObserver);
    if (!HTMLElement.prototype.hasPointerCapture) {
      HTMLElement.prototype.hasPointerCapture = () => false;
    }
    if (!HTMLElement.prototype.setPointerCapture) {
      HTMLElement.prototype.setPointerCapture = () => undefined;
    }
    if (!HTMLElement.prototype.releasePointerCapture) {
      HTMLElement.prototype.releasePointerCapture = () => undefined;
    }
    if (!HTMLElement.prototype.scrollIntoView) {
      HTMLElement.prototype.scrollIntoView = () => undefined;
    }
  });

  it("shows the current option and commits a choice", () => {
    const onChange = vi.fn();
    render(
      <Select
        label="backend"
        value="cli"
        options={[
          { value: "cli", label: "cli" },
          { value: "arcus", label: "arcus" },
        ]}
        onChange={onChange}
      />,
    );
    const trigger = screen.getByRole("button", { name: "backend" });
    expect(trigger.textContent).toContain("cli");
    trigger.focus();
    fireEvent.keyDown(trigger, { key: "Enter" });
    const option = screen.getByRole("menuitem", { name: "arcus" });
    fireEvent.click(option);
    expect(onChange).toHaveBeenCalledWith("arcus");
  });
});
