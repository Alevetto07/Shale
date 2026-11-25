import { describe, expect, it } from "vitest";
import capabilities from "../src-tauri/capabilities/default.json";

const permissions = capabilities.permissions;

describe("window capabilities", () => {
  it("allows the custom caption buttons and dragging", () => {
    for (const permission of [
      "core:window:allow-minimize",
      "core:window:allow-toggle-maximize",
      "core:window:allow-close",
      "core:window:allow-start-dragging",
      "core:window:allow-is-maximized",
    ]) {
      expect(permissions).toContain(permission);
    }
  });
});
