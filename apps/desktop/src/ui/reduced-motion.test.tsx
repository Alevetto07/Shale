import { render, screen } from "@testing-library/react";
import { beforeEach, describe, expect, it } from "vitest";
import { ActiveIndicator } from "./active-indicator";
import { useReducedMotion } from "./use-reduced-motion";

function mockReducedMotion(reduce: boolean) {
  Object.defineProperty(window, "matchMedia", {
    writable: true,
    configurable: true,
    value: (query: string) => ({
      matches: reduce && query.includes("prefers-reduced-motion"),
      media: query,
      onchange: null,
      addEventListener: () => undefined,
      removeEventListener: () => undefined,
      addListener: () => undefined,
      removeListener: () => undefined,
      dispatchEvent: () => false,
    }),
  });
}

function Probe() {
  const reduce = useReducedMotion();
  return <span>{reduce ? "reduce" : "full"}</span>;
}

describe("reduced motion", () => {
  beforeEach(() => {
    mockReducedMotion(true);
  });

  it("follows prefers-reduced-motion", () => {
    render(<Probe />);
    expect(screen.getByText("reduce")).toBeTruthy();
  });

  it("renders a static indicator instead of a layout animation", () => {
    render(<ActiveIndicator layoutId="pane-tabs" tone="pane" />);
    const mark = document.querySelector(".ui-active-indicator");
    expect(mark?.getAttribute("data-reduced")).toBe("1");
    expect(mark?.getAttribute("data-tone")).toBe("pane");
  });
});
