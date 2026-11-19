import type { UiStyleId } from "./ui-styles";

export const SETUP_WIZARD_STORAGE_KEY = "shale.setup-wizard";
export const SETUP_WIZARD_VERSION = 1;

export type SetupWizardStep = "welcome" | "theme" | "printers" | "import" | "account" | "done";

export interface SetupWizardState {
  version: number;
  completed: boolean;
  skipped: boolean;
  skippedSteps: SetupWizardStep[];
  chosenTheme?: UiStyleId;
  linkedPrinterId?: string;
  importedSlicerId?: string;
  accountSkipped?: boolean;
  finishedAt?: number;
}

const DEFAULT: SetupWizardState = {
  version: SETUP_WIZARD_VERSION,
  completed: false,
  skipped: false,
  skippedSteps: [],
};

export function readSetupWizardState(): SetupWizardState {
  if (typeof window === "undefined") return { ...DEFAULT };
  try {
    const raw = window.localStorage.getItem(SETUP_WIZARD_STORAGE_KEY);
    if (!raw) return { ...DEFAULT };
    const parsed = JSON.parse(raw) as Partial<SetupWizardState>;
    if (parsed.version !== SETUP_WIZARD_VERSION) {
      return { ...DEFAULT };
    }
    return {
      ...DEFAULT,
      ...parsed,
      skippedSteps: Array.isArray(parsed.skippedSteps) ? parsed.skippedSteps : [],
    };
  } catch {
    return { ...DEFAULT };
  }
}

export function writeSetupWizardState(state: SetupWizardState): void {
  window.localStorage.setItem(SETUP_WIZARD_STORAGE_KEY, JSON.stringify(state));
}

export function shouldShowSetupWizard(state: SetupWizardState = readSetupWizardState()): boolean {
  return !state.completed;
}

export function completeSetupWizard(
  partial: Partial<SetupWizardState> & { skipped?: boolean },
): SetupWizardState {
  const next: SetupWizardState = {
    ...readSetupWizardState(),
    ...partial,
    version: SETUP_WIZARD_VERSION,
    completed: true,
    finishedAt: Date.now(),
  };
  writeSetupWizardState(next);
  return next;
}

export function resetSetupWizard(): SetupWizardState {
  const next = { ...DEFAULT };
  writeSetupWizardState(next);
  return next;
}

export function markStepSkipped(step: SetupWizardStep): SetupWizardState {
  const current = readSetupWizardState();
  const skippedSteps = current.skippedSteps.includes(step)
    ? current.skippedSteps
    : [...current.skippedSteps, step];
  const next = { ...current, skippedSteps };
  writeSetupWizardState(next);
  return next;
}