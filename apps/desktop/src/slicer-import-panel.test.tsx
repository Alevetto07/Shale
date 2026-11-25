import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it } from "vitest";
import { SlicerImportPanel } from "./SlicerImportPanel";
import { resetImportJob } from "./slicer-import";
import { SetupWizard } from "./SetupWizard";

const printer = {
  id: "bambu_lab_a1",
  name: "A1",
  manufacturer: "Bambu Lab",
  preferred_quality_type: "standard",
  preferred_variant: "0.4",
  preferred_material: "pla",
  source: "bambu",
};

describe("settings import", () => {
  afterEach(() => {
    cleanup();
    resetImportJob();
  });

  it("does nothing until Check for settings", () => {
    render(<SlicerImportPanel />);
    expect(screen.getByRole("button", { name: "Check for settings" })).toBeTruthy();
    expect(screen.queryByRole("progressbar")).toBeNull();
    expect(screen.queryByText(/printers, /)).toBeNull();
  });

  it("shows a summary and keeps details collapsed", async () => {
    render(<SlicerImportPanel />);
    fireEvent.click(screen.getByRole("button", { name: "Check for settings" }));
    expect(await screen.findByText(/4 printers, 48 filaments, 88 processes/, {}, { timeout: 4000 })).toBeTruthy();
    const details = screen.getByText("Show details").closest("details");
    expect(details?.open).toBe(false);
    expect(screen.queryByRole("checkbox")).toBeNull();
  });
});

describe("setup wizard import step", () => {
  afterEach(() => {
    cleanup();
    localStorage.clear();
    resetImportJob();
  });

  it("keeps Continue visible without listing every preset", () => {
    render(
      <SetupWizard
        open
        printers={[printer]}
        printerId=""
        uiStyle="deep-field"
        onUiStyle={() => undefined}
        onChoosePrinter={() => undefined}
        onClose={() => undefined}
      />,
    );
    fireEvent.click(screen.getByRole("button", { name: "Continue" }));
    fireEvent.click(screen.getByRole("button", { name: "Continue" }));
    fireEvent.click(screen.getByRole("button", { name: "Continue" }));
    expect(screen.getByRole("button", { name: "Check for settings" })).toBeTruthy();
    expect(screen.getByRole("button", { name: "Continue" })).toBeTruthy();
    expect(screen.queryByText(/Import selected/)).toBeNull();
  });
});
