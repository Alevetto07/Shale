import { describe, expect, it } from "vitest";
import { isPaletteShortcut, shellShift, sidebarSpan, viewMotion, SIDEBAR_EXPANDED, SIDEBAR_RAIL } from "./motion";

describe("sidebar choreography", () => {
  it("uses the rail and expanded spans", () => {
    expect(sidebarSpan(false)).toBe(SIDEBAR_EXPANDED);
    expect(sidebarSpan(true)).toBe(SIDEBAR_RAIL);
  });

  it("offsets the main column by the difference, as a transform not a per-frame resize", () => {
    expect(shellShift(SIDEBAR_EXPANDED, SIDEBAR_RAIL)).toBe(SIDEBAR_EXPANDED - SIDEBAR_RAIL);
    expect(shellShift(SIDEBAR_RAIL, SIDEBAR_EXPANDED)).toBe(SIDEBAR_RAIL - SIDEBAR_EXPANDED);
    expect(shellShift(SIDEBAR_EXPANDED, SIDEBAR_EXPANDED)).toBe(0);
  });
});

describe("view motion", () => {
  it("cross-fades with a short translate when motion is allowed", () => {
    const motion = viewMotion(false);
    expect(motion.initial.y).toBe(10);
    expect(motion.exit.y).toBe(-8);
    expect(motion.transition.duration).toBeLessThanOrEqual(0.25);
    expect(motion.animate).toEqual({ opacity: 1, y: 0 });
  });

  it("does not move or fade when reduced motion is requested", () => {
    const motion = viewMotion(true);
    expect(motion.initial).toEqual({ opacity: 1, y: 0 });
    expect(motion.animate).toEqual({ opacity: 1, y: 0 });
    expect(motion.exit).toEqual({ opacity: 1, y: 0 });
    expect(motion.transition.duration).toBe(0);
  });
});

describe("command palette shortcut", () => {
  it("matches Ctrl+K and Cmd+K and ignores Alt", () => {
    expect(isPaletteShortcut({ key: "k", ctrlKey: true, metaKey: false, altKey: false })).toBe(true);
    expect(isPaletteShortcut({ key: "K", ctrlKey: false, metaKey: true, altKey: false })).toBe(true);
    expect(isPaletteShortcut({ key: "k", ctrlKey: true, metaKey: false, altKey: true })).toBe(false);
    expect(isPaletteShortcut({ key: "k", ctrlKey: false, metaKey: false, altKey: false })).toBe(false);
  });
});
