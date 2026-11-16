import { cleanup, render, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

describe("mock surfaces", () => {
  beforeEach(() => {
    vi.stubEnv("VITE_SHALE_MOCK", "1");
  });

  afterEach(() => {
    cleanup();
    vi.resetModules();
    vi.unstubAllEnvs();
  });

  async function mount(search: string) {
    vi.resetModules();
    vi.stubEnv("VITE_SHALE_MOCK", "1");
    window.history.replaceState({}, "", search);
    const { App } = await import("./App");
    return render(<App />);
  }

  it("mounts Prepare without throwing", async () => {
    const errors: string[] = [];
    const onError = (event: ErrorEvent) => {
      errors.push(event.message);
    };
    window.addEventListener("error", onError);
    try {
      await mount("/?mock=workspace-preview&pane=prepare");
      await waitFor(
        () => {
          expect(document.querySelector(".workspace")).toBeTruthy();
          expect(document.querySelector(".viewport-mock, .plate")).toBeTruthy();
        },
        { timeout: 8000 },
      );
      expect(errors).toEqual([]);
    } finally {
      window.removeEventListener("error", onError);
    }
  });

  it("mounts Preview without throwing", async () => {
    const errors: string[] = [];
    const onError = (event: ErrorEvent) => {
      errors.push(event.message);
    };
    window.addEventListener("error", onError);
    try {
      await mount("/?mock=workspace-preview&pane=preview");
      await waitFor(
        () => {
          expect(document.querySelector(".workspace")).toBeTruthy();
          expect(document.querySelector(".job-card--panel, .job-state")).toBeTruthy();
        },
        { timeout: 8000 },
      );
      expect(errors).toEqual([]);
    } finally {
      window.removeEventListener("error", onError);
    }
  });

  it("mounts Device without throwing", async () => {
    const errors: string[] = [];
    const onError = (event: ErrorEvent) => {
      errors.push(event.message);
    };
    window.addEventListener("error", onError);
    try {
      await mount("/?mock=device-lan&pane=device");
      await waitFor(
        () => {
          expect(document.querySelector(".device")).toBeTruthy();
        },
        { timeout: 8000 },
      );
      expect(errors).toEqual([]);
    } finally {
      window.removeEventListener("error", onError);
    }
  });
});