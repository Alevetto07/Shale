import { fireEvent, render } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { StartScreen } from "./StartScreen";
import type { UiStyleId } from "./ui-styles";

vi.mock("./field/PlateField", () => ({
  PlateField: () => <div data-testid="plate" />,
}));

function renderStart(overrides: Partial<Parameters<typeof StartScreen>[0]> = {}) {
  const onNew = vi.fn();
  const onOpen = vi.fn();
  const onImport = vi.fn();
  const onPanel = vi.fn();
  const onQuit = vi.fn();
  const onReturn = vi.fn();
  render(
    <StartScreen
      live={false}
      panel="home"
      onPanel={onPanel}
      canReturn
      printers={[]}
      printerId=""
      enginePath=""
      libDir=""
      backend="cli"
      version="0.1.4"
      onReturn={onReturn}
      onNew={onNew}
      onOpen={onOpen}
      onImport={onImport}
      onChoosePrinter={() => undefined}
      onEnginePath={() => undefined}
      onLibDir={() => undefined}
      onBackend={() => undefined}
      onApplyEngine={() => undefined}
      onQuit={onQuit}
      uiStyle={"deep-field" as UiStyleId}
      onUiStyle={() => undefined}
      {...overrides}
    />,
  );
  return { onNew, onOpen, onImport, onPanel, onQuit, onReturn };
}

describe("start screen keyboard", () => {
  it("runs n, o, i, p, s, a, q and Escape", () => {
    const handlers = renderStart();
    const press = (key: string) => fireEvent.keyDown(window, { key });
    press("n");
    press("o");
    press("i");
    press("p");
    press("s");
    press("a");
    press("q");
    expect(handlers.onNew).toHaveBeenCalledTimes(1);
    expect(handlers.onOpen).toHaveBeenCalledTimes(1);
    expect(handlers.onImport).toHaveBeenCalledTimes(1);
    expect(handlers.onPanel).toHaveBeenCalledWith("printers");
    expect(handlers.onPanel).toHaveBeenCalledWith("settings");
    expect(handlers.onPanel).toHaveBeenCalledWith("about");
    expect(handlers.onQuit).toHaveBeenCalledTimes(1);
    press("Escape");
    expect(handlers.onReturn).toHaveBeenCalledTimes(1);
  });

  it("focuses the first recent row on r", () => {
    renderStart();
    const recent = document.createElement("button");
    recent.setAttribute("data-recent", "1");
    recent.textContent = "Benchy";
    document.body.appendChild(recent);
    fireEvent.keyDown(window, { key: "r" });
    expect(document.activeElement).toBe(recent);
    recent.remove();
  });

  it("lets Escape close a start panel before leaving home", () => {
    const handlers = renderStart({ panel: "settings" });
    fireEvent.keyDown(window, { key: "Escape" });
    expect(handlers.onPanel).toHaveBeenCalledWith("home");
    expect(handlers.onReturn).not.toHaveBeenCalled();
  });
});