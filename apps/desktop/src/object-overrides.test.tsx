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

  it("ignores a value that is not a number", () => {
    const onChange = vi.fn();
    render(<ObjectOverrides settings={{ speed_print: "80" }} onChange={onChange} />);
    const speed = screen.getByRole("textbox", { name: "Print speed for this object, in millimetres per second" });
    fireEvent.change(speed, { target: { value: "fast" } });
    expect(onChange).not.toHaveBeenCalled();
    expect((speed as HTMLInputElement).value).toBe("80");
  });
});
