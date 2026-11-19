import { useEffect, useState, type ReactNode } from "react";
import type { PrinterInfo } from "./api";
import { CloudPanel } from "./CloudPanel";
import { SlicerImportPanel } from "./SlicerImportPanel";
import {
  completeSetupWizard,
  markStepSkipped,
  type SetupWizardStep,
} from "./setup-wizard";
import { UiStylePicker } from "./UiStylePicker";
import type { UiStyleId } from "./ui-styles";

const STEPS: { id: SetupWizardStep; title: string; blurb: string }[] = [
  { id: "welcome", title: "Welcome", blurb: "Set up Shale in a minute — or skip and finish later in Settings." },
  { id: "theme", title: "Theme", blurb: "Pick an interface style with a live plate preview." },
  { id: "printers", title: "Printers", blurb: "Choose your default Bambu machine from the catalogue." },
  { id: "import", title: "Import", blurb: "See presets from Bambu Studio, Orca, Prusa, or Cura on this PC." },
  { id: "account", title: "Bambu account", blurb: "Sign in for cloud printers — optional." },
  { id: "done", title: "Done", blurb: "You are ready to slice." },
];

export function SetupWizard({
  open,
  printers,
  printerId,
  uiStyle,
  onUiStyle,
  onChoosePrinter,
  onClose,
}: {
  open: boolean;
  printers: PrinterInfo[];
  printerId: string;
  uiStyle: UiStyleId;
  onUiStyle: (id: UiStyleId) => void;
  onChoosePrinter: (id: string) => void;
  onClose: () => void;
}) {
  const [index, setIndex] = useState(0);
  const [printerQuery, setPrinterQuery] = useState("");

  const step = STEPS[index] ?? STEPS[0];

  useEffect(() => {
    if (!open) {
      setIndex(0);
    }
  }, [open]);

  if (!open) return null;

  const bambuPrinters = printers
    .filter((p) => !printerQuery.trim() || `${p.manufacturer} ${p.name} ${p.id}`.toLowerCase().includes(printerQuery.trim().toLowerCase()))
    .filter((p) => p.source === "bambu" || `${p.manufacturer}`.toLowerCase().includes("bambu"))
    .slice(0, 40);

  function finish(skipped: boolean) {
    completeSetupWizard({
      skipped,
      chosenTheme: uiStyle,
      linkedPrinterId: printerId,
    });
    onClose();
  }

  function skipStep() {
    markStepSkipped(step.id);
    if (index >= STEPS.length - 1) {
      finish(true);
    } else {
      setIndex((value) => value + 1);
    }
  }

  function next() {
    if (index >= STEPS.length - 1) {
      finish(false);
    } else {
      setIndex((value) => value + 1);
    }
  }

  return (
    <div className="setup-wizard-backdrop" role="presentation">
      <div className="setup-wizard" role="dialog" aria-modal="true" aria-labelledby="setup-wizard-title">
        <header className="setup-wizard-header">
          <p className="setup-wizard-kicker">First-run setup</p>
          <h2 id="setup-wizard-title">{step.title}</h2>
          <p>{step.blurb}</p>
          <ol className="setup-wizard-steps" aria-label="Steps">
            {STEPS.map((item, stepIndex) => (
              <li key={item.id} className={stepIndex === index ? "active" : stepIndex < index ? "done" : undefined}>
                {item.title}
              </li>
            ))}
          </ol>
        </header>

        <div className="setup-wizard-body">{bodyForStep(step.id)}</div>

        <footer className="setup-wizard-footer">
          <button type="button" className="quiet" onClick={() => finish(true)}>
            Skip setup
          </button>
          <span className="setup-wizard-spacer" />
          <button type="button" className="quiet" onClick={skipStep}>
            Skip step
          </button>
          <button type="button" className="primary" onClick={next}>
            {index >= STEPS.length - 1 ? "Finish" : "Continue"}
          </button>
        </footer>
      </div>
    </div>
  );

  function bodyForStep(id: SetupWizardStep): ReactNode {
    switch (id) {
      case "welcome":
        return <p>You can change everything later under Settings on the start screen or Home tab.</p>;
      case "theme":
        return <UiStylePicker value={uiStyle} onChange={onUiStyle} />;
      case "printers":
        return (
          <>
            <label className="setup-filter">
              <span>Filter</span>
              <input value={printerQuery} onChange={(e) => setPrinterQuery(e.target.value)} />
            </label>
            <div className="setup-printer-list">
              {bambuPrinters.map((printer) => (
                <button
                  key={printer.id}
                  type="button"
                  className={printer.id === printerId ? "selected" : undefined}
                  onClick={() => onChoosePrinter(printer.id)}
                >
                  {printer.manufacturer} {printer.name}
                </button>
              ))}
            </div>
          </>
        );
      case "import":
        return <SlicerImportPanel compact defaultSlicerId="bambu" />;
      case "account":
        return <CloudPanel taskName="setup" />;
      case "done":
        return <p>Press Finish to open Shale. Happy printing.</p>;
      default:
        return null;
    }
  }
}