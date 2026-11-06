import { useEffect, useState } from "react";

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
    <div className="tabs">
      <div className="doc-tabs" role="tablist" aria-label="Open projects">
        {tabs.map((tab) => (
          <span key={tab.id} className={tab.id === activeTab ? "doc-tab active" : "doc-tab"}>
            <button
              type="button"
              role="tab"
              aria-selected={tab.id === activeTab}
              className="doc-name"
              title={tab.path ?? "not saved yet"}
              onClick={() => onTab(tab.id)}
            >
              {tab.name}
            </button>
            {tab.dirty && <span className="doc-dirty" aria-label="unsaved changes">•</span>}
            {tabs.length > 1 && (
              <button
                type="button"
                className="doc-close"
                aria-label={`close ${tab.name}`}
                title="close this tab. the file stays where it is"
                onClick={() => onCloseTab(tab.id)}
              >
                ×
              </button>
            )}
          </span>
        ))}
        <button type="button" className="doc-add" title="a new empty plate in its own tab" onClick={onNewTab}>
          +
        </button>
        <button type="button" className="doc-open" title="open a project into a new tab" onClick={onOpenTab}>
          open…
        </button>
      </div>

      <div className="mode-bar">
        <div className="mode-chips" role="tablist" aria-label="Workspace">
          {PANES.map((item) => {
            const blocked = item.id === "preview" && !sliced;
            return (
              <button
                key={item.id}
                type="button"
                role="tab"
                aria-selected={pane === item.id}
                className={pane === item.id ? "mode-chip active" : "mode-chip"}
                disabled={blocked}
                title={blocked ? "slice the plate first — there is no toolpath to show" : item.note}
                onClick={() => onPane(item.id)}
              >
                {item.label}
              </button>
            );
          })}
        </div>
        <p className="mode-printer" title="the machine the plate is sliced for">
          {printer}
        </p>
      </div>
    </div>
  );
}

export interface Spool {
  /** 1-based bay, as the machine numbers them. */
  bay: number;
  material: string;
}

export interface JobFacts {
  layers: number;
  seconds: number | null;
  layerHeight: number;
  nozzle: number;
  bedTemp: number;
  material: string;
  sliced: boolean;
}

export interface DevicePaneProps {
  machine: string;
  source: string;
  bedWidth: number;
  bedDepth: number;
  bedHeight: number;
  nozzleSize: number;
  spools: Spool[];
  job: JobFacts;
  engineBackend: string;
  engineReady: boolean;
}

function clock(seconds: number | null): string {
  if (seconds == null) return "—";
  const whole = Math.max(0, Math.round(seconds));
  const hours = Math.floor(whole / 3600);
  const minutes = Math.round((whole % 3600) / 60);
  return hours > 0 ? `${hours} h ${String(minutes).padStart(2, "0")} m` : `${minutes} min`;
}

/// The Device pane. A printer that is not on the network yet still has a lot
/// worth reading: what machine the plate is sliced for, which spools it will
/// pull from, and what the job on the plate comes to.
export function DevicePane({
  machine,
  source,
  bedWidth,
  bedDepth,
  bedHeight,
  nozzleSize,
  spools,
  job,
  engineBackend,
  engineReady,
}: DevicePaneProps) {
  const [host, setHost] = useState(() => localStorage.getItem("shale.device.host") ?? "");
  const [serial, setSerial] = useState(() => localStorage.getItem("shale.device.serial") ?? "");

  useEffect(() => {
    localStorage.setItem("shale.device.host", host);
  }, [host]);
  useEffect(() => {
    localStorage.setItem("shale.device.serial", serial);
  }, [serial]);

  const connected = host.trim().length > 0 && serial.trim().length > 0;

  return (
    <div className="device">
      <section className="device-head">
        <p className="device-kicker">device</p>
        <h2 className="device-title">{machine}</h2>
        <p className="device-sub">
          {source === "bambu" ? "Bambu Studio profile" : "Cura profile"} ·{" "}
          {bedWidth.toFixed(0)} × {bedDepth.toFixed(0)} × {bedHeight.toFixed(0)} mm · {nozzleSize.toFixed(2)} mm nozzle
        </p>
        <p className={connected ? "device-status live" : "device-status"}>
          {connected ? `set for ${serial.trim()} at ${host.trim()} — local mqtt is not wired yet` : "no printer connected"}
        </p>
      </section>

      <section className="device-grid">
        <article className="device-card">
          <p className="device-label">plate</p>
          <p className="device-big">{job.layers > 0 ? job.layers : "—"}</p>
          <p className="device-note">{job.layers > 0 ? "layers sliced" : "nothing sliced yet"}</p>
        </article>
        <article className="device-card">
          <p className="device-label">print time</p>
          <p className="device-big">{clock(job.seconds)}</p>
          <p className="device-note">{job.layers > 0 ? `at ${job.layerHeight.toFixed(2)} mm layers` : "slice to find out"}</p>
        </article>
        <article className="device-card">
          <p className="device-label">nozzle</p>
          <p className="device-big">{job.nozzle.toFixed(0)}°</p>
          <p className="device-note">bed {job.bedTemp.toFixed(0)}° · {job.material || "no material"}</p>
        </article>
        <article className="device-card">
          <p className="device-label">engine</p>
          <p className="device-big">{engineBackend}</p>
          <p className="device-note">{engineReady ? "ready" : "not found"}</p>
        </article>
      </section>

      <section className="device-panel">
        <p className="device-label">spools</p>
        <ul className="device-spools">
          {spools.map((spool) => (
            <li key={spool.bay}>
              <span className="bay">T{spool.bay}</span>
              <span className={spool.material ? "spool" : "spool empty"}>{spool.material || "empty bay"}</span>
            </li>
          ))}
        </ul>
        <p className="device-note">
          the plate prints from T1 unless a part is pointed at another bay. each spool is resolved as its own
          material, with its own hotend temperature.
        </p>
      </section>

      <section className="device-panel">
        <p className="device-label">connection</p>
        <div className="device-form">
          <label>
            host
            <input
              type="text"
              aria-label="printer host"
              placeholder="192.168.1.40"
              value={host}
              onChange={(event) => setHost(event.target.value)}
            />
          </label>
          <label>
            serial
            <input
              type="text"
              aria-label="printer serial"
              placeholder="01P00A000000000"
              value={serial}
              onChange={(event) => setSerial(event.target.value)}
            />
          </label>
        </div>
        <p className="device-note">
          the access code stays out of this window on purpose: it is a password for the printer. local mqtt and
          the chamber camera come after the decision about whether shale talks to the machine or hands the
          g-code to a host.
        </p>
      </section>
    </div>
  );
}
