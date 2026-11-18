import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it } from "vitest";
import { UiStylePicker } from "./UiStylePicker";
import {
  UI_STYLES,
  applyUiStyle,
  readUiStyle,
  UI_STYLE_STORAGE_KEY,
  writeUiStyle,
  type UiStyleId,
} from "./ui-styles";

describe("ui styles", () => {
  beforeEach(() => {
    localStorage.clear();
    document.documentElement.dataset.uiTheme = "deep-field";
  });

  afterEach(() => {
    cleanup();
    localStorage.clear();
    applyUiStyle("deep-field");
  });

  it("exposes a plate image for every style", () => {
    for (const style of UI_STYLES) {
      expect(style.plateImage.length).toBeGreaterThan(0);
      expect(style.label).not.toMatch(/deep field/i);
    }
  });

  it("persists the chosen style", () => {
    writeUiStyle("saturn");
    expect(readUiStyle()).toBe("saturn");
    expect(localStorage.getItem(UI_STYLE_STORAGE_KEY)).toBe("saturn");
  });

  it("applyUiStyle sets data-ui-theme on html", () => {
    applyUiStyle("nebula");
    expect(document.documentElement.dataset.uiTheme).toBe("nebula");
    applyUiStyle("lunar");
    expect(document.documentElement.dataset.uiTheme).toBe("lunar");
    expect(document.documentElement.style.colorScheme).toBe("light");
  });

  it("picker switches data-ui-theme when a swatch is chosen", () => {
    const seen: UiStyleId[] = [];
    render(
      <UiStylePicker
        value="deep-field"
        onChange={(id) => {
          seen.push(id);
          applyUiStyle(id);
        }}
      />,
    );
    fireEvent.click(screen.getByRole("option", { name: /Saturn/i }));
    expect(seen).toEqual(["saturn"]);
    expect(document.documentElement.dataset.uiTheme).toBe("saturn");
  });
});