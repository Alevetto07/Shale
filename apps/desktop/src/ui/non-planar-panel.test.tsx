import { fireEvent, render, screen } from "@testing-library/react";
import { useState } from "react";
import { describe, expect, it } from "vitest";
import { DEFAULT_NONPLANAR, type NonplanarReportView, type NonplanarSettings } from "../api";
import { ENVELOPE_NOTE, NonPlanarPanel } from "./non-planar-panel";

function Host({
  initial = DEFAULT_NONPLANAR,
  report = null,
  supported = true,
}: {
  initial?: NonplanarSettings;
  report?: NonplanarReportView | null;
  supported?: boolean;
}) {
  const [value, setValue] = useState(initial);
  const [live, setLive] = useState(true);
  return (
    <NonPlanarPanel
      value={value}
      onChange={setValue}
      report={report}
      supported={supported}
      liveSlice={live}
      onLiveSlice={setLive}
    />
  );
}

const curved: NonplanarReportView = {
  mode: "auto",
  max_angle_deg: 10,
  strict: false,
  underside_bands: false,
  outcome: "curved",
  fallback_to_planar: false,
  fallback_reasons: [],
  curved_move_count: 134862,
  worst_slope_deg: 6.06,
  surfaces: [
    { id: 1, kind: "Curved top", enabled: true, reason: "spread 1.80 mm", tilt_deg: 6 },
    { id: 2, kind: "Bridge", enabled: false, reason: "horizontal underside stays planar", tilt_deg: 0 },
  ],
  envelope_note: ENVELOPE_NOTE,
};

describe("NonPlanarPanel", () => {
  it("starts off, caps the angle, and keeps underside behind experimental", () => {
    render(<Host />);
    expect(screen.getByRole("button", { name: "Off", pressed: true })).toBeTruthy();
    const angle = screen.getByRole("slider", { name: "Max angle" }) as HTMLInputElement;
    expect(angle.max).toBe("10");
    expect(angle.value).toBe("10");
    fireEvent.change(angle, { target: { value: "8" } });
    expect(angle.value).toBe("8");
    expect(screen.getByText(ENVELOPE_NOTE)).toBeTruthy();
    const experimental = screen.getByText("Experimental").closest("details");
    expect(experimental?.open).toBe(false);
    fireEvent.click(screen.getByText("Experimental"));
    expect(experimental?.open).toBe(true);
    expect(screen.getByText(/unverified/i)).toBeTruthy();
    const underside = screen.getByRole("checkbox", { name: /underside bands/i }) as HTMLInputElement;
    expect(underside.disabled).toBe(true);
    fireEvent.click(screen.getByRole("button", { name: "Auto" }));
    expect(screen.getByRole("button", { name: "Auto", pressed: true })).toBeTruthy();
    expect(underside.disabled).toBe(false);
  });

  it("shows a curved result, the slope, and accepted and refused surfaces", () => {
    render(<Host initial={{ ...DEFAULT_NONPLANAR, mode: "auto" }} report={curved} />);
    expect(screen.getByText("Curved layers")).toBeTruthy();
    expect(screen.getByText("134,862")).toBeTruthy();
    expect(screen.getByText("6.06°")).toBeTruthy();
    expect(screen.getByText(/Accepted/)).toBeTruthy();
    expect(screen.getByText(/Refused/)).toBeTruthy();
    expect(screen.getByText(/horizontal underside stays planar/)).toBeTruthy();
  });

  it("lists every fallback reason when the slice stayed planar", () => {
    const report: NonplanarReportView = {
      ...curved,
      outcome: "planar",
      fallback_to_planar: true,
      curved_move_count: 0,
      fallback_reasons: [
        "no printable curved top surface",
        "16 travel move(s) collide with printed material (tolerance 0.1 mm)",
      ],
    };
    render(<Host initial={{ ...DEFAULT_NONPLANAR, mode: "auto" }} report={report} />);
    expect(screen.getByText("Fell back to a planar slice")).toBeTruthy();
    expect(screen.getByText("no printable curved top surface")).toBeTruthy();
    expect(screen.getByText(/16 travel move/)).toBeTruthy();
    expect(screen.getByText("0")).toBeTruthy();
  });

  it("says when the printer cannot curve", () => {
    render(<Host initial={{ ...DEFAULT_NONPLANAR, mode: "curved-tops" }} supported={false} />);
    expect(screen.getByText(/no head envelope/i)).toBeTruthy();
  });
});
