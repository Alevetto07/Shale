import { render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { Dialog } from "./dialog";

describe("Dialog", () => {
  it("shows the title when open and hides it when closed", () => {
    const { rerender } = render(
      <Dialog open title="First-run setup" onOpenChange={() => undefined}>
        <p>Welcome</p>
      </Dialog>,
    );
    expect(screen.getByRole("dialog", { name: "First-run setup" })).toBeTruthy();
    expect(screen.getByText("Welcome")).toBeTruthy();
    rerender(
      <Dialog open={false} title="First-run setup" onOpenChange={() => undefined}>
        <p>Welcome</p>
      </Dialog>,
    );
    expect(screen.queryByRole("dialog")).toBeNull();
  });

  it("calls onOpenChange when Escape is pressed", () => {
    const onOpenChange = vi.fn();
    render(
      <Dialog open title="Confirm" onOpenChange={onOpenChange}>
        <p>Body</p>
      </Dialog>,
    );
    screen.getByRole("dialog").dispatchEvent(new KeyboardEvent("keydown", { key: "Escape", bubbles: true }));
    // Radix listens on document.
    document.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape", bubbles: true }));
    expect(onOpenChange).toHaveBeenCalled();
  });
});
