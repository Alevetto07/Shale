import { ActiveIndicator } from "./ui/active-indicator";
import { Tabs, TabsList, TabsTrigger, Tooltip } from "./ui";
import type { DevicePaneProps, JobFacts, Spool } from "./DevicePane";

export type { DevicePaneProps, JobFacts, Spool };

/// Where the window is pointed. Home is the start screen; the other three are
/// the three things a slicer does with a plate: set it up, look at the result,
/// and talk to the machine.
export type Pane = "home" | "prepare" | "preview" | "device";

export interface DocTab {
  id: number;
  name: string;
  /** The project file, when the tab was opened from one or saved to one. */
  path: string | null;
  /**
   * Where a plate that was never saved to a file is put away when you leave its
   * tab. Without this the plate is written and never read back, so coming back
   * to a tab would find it empty.
   */
  scratch: string | null;
  /** Something in the plate has changed since the tab was last written. */
  dirty: boolean;
}

export interface TabBarProps {
  pane: Pane;
  onPane: (pane: Pane) => void;
  tabs: DocTab[];
  activeTab: number;
  onTab: (id: number) => void;
  onCloseTab: (id: number) => void;
  onNewTab: () => void;
  onOpenTab: () => void;
  printer: string;
  sliced: boolean;
}

const PANES: { id: Pane; label: string; note: string }[] = [
  { id: "home", label: "Home", note: "projects and the machines this copy knows" },
  { id: "prepare", label: "Prepare", note: "put parts on the plate and choose how to print them" },
  { id: "preview", label: "Preview", note: "the toolpath, layer by layer" },
  { id: "device", label: "Device", note: "the machine, its spools, and the job" },
];

/// The row of open projects, then the row of panes — the shape Bambu Studio
/// puts above its plate, and the fastest way to tell where you are.
export function TabBar({
  pane,
  onPane,
  tabs,
  activeTab,
  onTab,
  onCloseTab,
  onNewTab,
  onOpenTab,
  printer,
  sliced,
}: TabBarProps) {
  return (
    <div className="tabs" data-docs={tabs.length}>
      <div className="doc-tabs">
        <Tabs value={String(activeTab)} onValueChange={(id) => onTab(Number(id))}>
          <TabsList aria-label="Open projects" className="doc-tabs-list">
            {tabs.map((tab) => (
              <span key={tab.id} className={tab.id === activeTab ? "doc-tab active" : "doc-tab"}>
                <Tooltip label={tab.path ?? "not saved yet"}>
                  <TabsTrigger value={String(tab.id)} className="doc-name">
                    {tab.name}
                  </TabsTrigger>
                </Tooltip>
                {tab.dirty && <span className="doc-dirty" aria-label="Unsaved changes">•</span>}
                {tabs.length > 1 && (
                  <button
                    type="button"
                    className="doc-close"
                    aria-label={`close ${tab.name}`}
                    onClick={() => onCloseTab(tab.id)}
                  >
                    ×
                  </button>
                )}
              </span>
            ))}
          </TabsList>
        </Tabs>
        <span className="doc-rule" aria-hidden="true" />
        <Tooltip label="A new empty plate in its own tab">
          <button type="button" className="doc-add" aria-label="New project" onClick={onNewTab}>
            +
          </button>
        </Tooltip>
        <Tooltip label="Open a project into a new tab">
          <button type="button" className="doc-open" onClick={onOpenTab}>
            Open…
          </button>
        </Tooltip>
      </div>

      <div className="mode-bar">
        <Tabs value={pane} onValueChange={(next) => onPane(next as Pane)}>
          <TabsList className="mode-chips" aria-label={`Workspace — ${printer}`}>
            {PANES.map((item) => {
              // Dimmed, not disabled: a disabled chip swallows the click, so the
              // one place a user needs an explanation is the one place that
              // cannot give one.
              const waiting = item.id === "preview" && !sliced;
              return (
                <TabsTrigger
                  key={item.id}
                  value={item.id}
                  aria-disabled={waiting || undefined}
                  className={waiting ? "mode-chip waiting" : "mode-chip"}
                  title={waiting ? "slice the plate first — there is no toolpath to show" : item.note}
                >
                  {pane === item.id ? <ActiveIndicator layoutId="pane-tabs" tone="pane" /> : null}
                  <span className="mode-chip__label">{item.label}</span>
                </TabsTrigger>
              );
            })}
          </TabsList>
        </Tabs>
      </div>
    </div>
  );
}
