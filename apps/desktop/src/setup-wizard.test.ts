import { afterEach, describe, expect, it } from "vitest";
import {
  SETUP_WIZARD_STORAGE_KEY,
  completeSetupWizard,
  markStepSkipped,
  readSetupWizardState,
  resetSetupWizard,
  shouldShowSetupWizard,
} from "./setup-wizard";

describe("setup wizard persistence", () => {
  afterEach(() => {
    localStorage.clear();
  });

  it("shows on a fresh profile", () => {
    expect(shouldShowSetupWizard()).toBe(true);
  });

  it("records completion", () => {
    completeSetupWizard({ skipped: false, chosenTheme: "saturn" });
    expect(shouldShowSetupWizard()).toBe(false);
    const state = readSetupWizardState();
    expect(state.completed).toBe(true);
    expect(state.chosenTheme).toBe("saturn");
    expect(localStorage.getItem(SETUP_WIZARD_STORAGE_KEY)).toContain("saturn");
  });

  it("records skipped steps", () => {
    markStepSkipped("account");
    const state = readSetupWizardState();
    expect(state.skippedSteps).toContain("account");
  });

  it("reset clears completion", () => {
    completeSetupWizard({ skipped: true });
    resetSetupWizard();
    expect(shouldShowSetupWizard()).toBe(true);
  });
});