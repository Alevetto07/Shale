import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { HomeLanding } from "./home-landing";

const printers = [
  {
    id: "bambu_lab_a1",
    name: "A1",
    manufacturer: "Bambu Lab",
    preferred_quality_type: "standard",
    preferred_variant: "0.4",
    preferred_material: "pla",
    source: "bambu",
  },
];

describe("HomeLanding", () => {
  it("offers import, a printer, and a recent project in sentence case", () => {
    const onImport = vi.fn();
    const onChoose = vi.fn();
    const onOpenRecent = vi.fn();
    render(
      <HomeLanding
        recents={[
          {
            path: "/tmp/lid.shale",
            name: "Demo lid",
            saved_at_ms: Date.now() - 86_400_000 * 1.5,
            printer: "Bambu Lab A1",
            missing: false,
          },
        ]}
        printers={printers}
        printerId=""
        onNew={() => undefined}
        onOpen={() => undefined}
        onImport={onImport}
        onOpenRecent={onOpenRecent}
        onChoosePrinter={onChoose}
        onDropModel={() => undefined}
      />,
    );
    fireEvent.click(screen.getByRole("button", { name: "Import a model" }));
    expect(onImport).toHaveBeenCalledOnce();
    fireEvent.click(screen.getByRole("option", { name: /Bambu Lab A1/ }));
    expect(onChoose).toHaveBeenCalledWith("bambu_lab_a1");
    expect(screen.getByText(/Yesterday/)).toBeTruthy();
    fireEvent.click(screen.getByRole("button", { name: /Demo lid/ }));
    expect(onOpenRecent).toHaveBeenCalledWith("/tmp/lid.shale");
  });

  it("shows a designed empty state when nothing has been saved", () => {
    render(
      <HomeLanding
        recents={[]}
        printers={[]}
        printerId=""
        onNew={() => undefined}
        onOpen={() => undefined}
        onImport={() => undefined}
        onOpenRecent={() => undefined}
        onChoosePrinter={() => undefined}
        onDropModel={() => undefined}
      />,
    );
    expect(screen.getByText(/No recent projects yet/)).toBeTruthy();
    expect(screen.getByText(/No printer yet/)).toBeTruthy();
  });
});
