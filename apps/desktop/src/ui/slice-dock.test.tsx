import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { SliceDock } from "./slice-dock";

describe("SliceDock", () => {
  it("names the stage while a slice is running", () => {
    render(
      <SliceDock
        busy
        stage="Curving layers"
        fraction={0.4}
        disabled={false}
        onSlice={() => undefined}
        time={null}
        filament={null}
        weight={null}
        cost={null}
      />,
    );
    expect(screen.getByRole("button", { name: "Curving layers" })).toBeTruthy();
    expect(screen.getByRole("progressbar", { name: "Curving layers" }).getAttribute("aria-valuenow")).toBe("40");
  });

  it("shows the estimate once the slice has finished", () => {
    const onSlice = vi.fn();
    render(
      <SliceDock
        busy={false}
        stage={null}
        fraction={null}
        disabled={false}
        onSlice={onSlice}
        time="1 h 11 m"
        filament="12.4 m"
        weight="37 g"
        cost="£1.20"
      />,
    );
    expect(screen.getByText("Time")).toBeTruthy();
    expect(screen.getByText("1 h 11 m")).toBeTruthy();
    expect(screen.getByText("Filament")).toBeTruthy();
    expect(screen.getByText("£1.20")).toBeTruthy();
    fireEvent.click(screen.getByRole("button", { name: "Slice" }));
    expect(onSlice).toHaveBeenCalledOnce();
  });
});
