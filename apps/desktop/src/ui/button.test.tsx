import { render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import { Button } from "./button";

describe("Button", () => {
  it("renders with variant and exposes pressable role", () => {
    render(<Button variant="primary">Slice</Button>);
    const control = screen.getByRole("button", { name: "Slice" });
    expect(control.getAttribute("data-variant")).toBe("primary");
    expect(control.className).toContain("ui-btn");
  });

  it("marks disabled controls", () => {
    render(
      <Button variant="secondary" disabled>
        Save
      </Button>,
    );
    expect((screen.getByRole("button", { name: "Save" }) as HTMLButtonElement).disabled).toBe(true);
  });
});