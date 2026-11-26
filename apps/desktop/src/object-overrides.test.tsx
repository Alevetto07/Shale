import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { useState } from "react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { ObjectOverrides } from "./object-overrides";

afterEach(() => cleanup());

function Harness() {
  const [settings, setSettings] = useState<Record<string, string>>({});
  return (
    <ObjectOverrides
      settings={settings}
      onChange={(key, value) =>
        setSettings((current) => {
          const next = { ...current };
          if (value === "") delete next[key];
          else next[key] = value;
          return next;
        })
      }
    />
  );
}

describe("ObjectOverrides", () => {
  it("writes a wall count and clears it back to the plate", () => {
    render(<Harness />);
    const walls = screen.getByRole("textbox", { name: "Wall line count for this object" });
    fireEvent.change(walls, { target: { value: "4" } });
    expect((walls as HTMLInputElement).value).toBe("4");
    fireEvent.change(walls, { target: { value: "" } });
    expect((walls as HTMLInputElement).value).toBe("");
    expect(screen.getByText(/follow the plate/)).toBeTruthy();
    expect(document.body.textContent).not.toMatch(/deep field/i);
  });

  it("turns support off for this object and can follow the plate again", () => {
    render(<Harness />);
    const support = screen.getByRole("combobox", { name: "Support for this object" });
    fireEvent.change(support, { target: { value: "0" } });
    expect((support as HTMLSelectElement).value).toBe("0");
    fireEvent.change(support, { target: { value: "" } });
    expect((support as HTMLSelectElement).value).toBe("");
  });

  it("offers brim type and seam position", () => {
    render(<Harness />);
    expect(screen.getByRole("combobox", { name: "Brim type for this object" })).toBeTruthy();
    expect(screen.getByRole("combobox", { name: "Seam position for this object" })).toBeTruthy();
    expect(screen.getByRole("textbox", { name: "Brim width for this object, in millimetres" })).toBeTruthy();
    expect(document.body.textContent).not.toMatch(/deep field/i);
  });

  it("ignores a value that is not a number", () => {
    const onChange = vi.fn();
    render(<ObjectOverrides settings={{ speed_print: "80" }} onChange={onChange} />);
    const speed = screen.getByRole("textbox", { name: "Print speed for this object, in millimetres per second" });
    fireEvent.change(speed, { target: { value: "fast" } });
    expect(onChange).not.toHaveBeenCalled();
    expect((speed as HTMLInputElement).value).toBe("80");
  });
});
