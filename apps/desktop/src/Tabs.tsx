import { useEffect, useRef, useState } from "react";

import * as api from "./api";

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
        <span className="doc-rule" aria-hidden="true" />
        <button
          type="button"
          className="doc-add"
          aria-label="new project"
          title="a new empty plate in its own tab"
          onClick={onNewTab}
        >
          +
        </button>
        <button type="button" className="doc-open" title="open a project into a new tab" onClick={onOpenTab}>
          Open…
        </button>
      </div>

      <div className="mode-bar">
        <div className="mode-chips" role="tablist" aria-label={`Workspace — ${printer}`}>
          {PANES.map((item) => {
            // Dimmed, not disabled: a disabled chip swallows the click, so the
            // one place a user needs an explanation is the one place that
            // cannot give one.
            const waiting = item.id === "preview" && !sliced;
            return (
              <button
                key={item.id}
                type="button"
                role="tab"
                aria-selected={pane === item.id}
                aria-disabled={waiting || undefined}
                className={
                  pane === item.id ? "mode-chip active" : waiting ? "mode-chip waiting" : "mode-chip"
                }
                title={waiting ? "slice the plate first — there is no toolpath to show" : item.note}
                onClick={() => onPane(item.id)}
              >
                {item.label}
              </button>
            );
          })}
        </div>
        {/* The menu bar names the machine 34 px above this row; printing it
            twice made the strip look like it was repeating itself. */}
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
  /// Which bays a part on the plate is pointed at. A bay whose material was
  /// never chosen is not the same as a bay nothing prints from.
  baysInUse: number[];
  job: JobFacts;
  engineBackend: string;
  engineReady: boolean;
  /** True when the plate has a G-code file behind it, so there is something to send. */
  gcodeReady: boolean;
  /** The name of the document on screen, so a queued job is called something. */
  plate: string;
}

function clock(seconds: number | null): string {
  if (seconds == null) return "—";
  const whole = Math.max(0, Math.round(seconds));
  const hours = Math.floor(whole / 3600);
  const minutes = Math.round((whole % 3600) / 60);
  return hours > 0 ? `${hours} h ${String(minutes).padStart(2, "0")} m` : `${minutes} min`;
}

/// The Device pane, with a live printer behind it.
///
/// The link is the app's own: MQTT over TLS to the machine, FTPS for the file.
/// Every action that moves the printer is a press here — the pane never starts
/// a print on its own — and the send button asks first, naming the file, the
/// plate and the time the slice predicted.
export function DevicePane({
  machine,
  source,
  bedWidth,
  bedDepth,
  bedHeight,
  nozzleSize,
  spools,
  baysInUse,
  job,
  engineBackend,
  engineReady,
  gcodeReady,
  plate,
}: DevicePaneProps) {
  const [host, setHost] = useState(() => localStorage.getItem("shale.device.host") ?? "");
  const [serial, setSerial] = useState(() => localStorage.getItem("shale.device.serial") ?? "");
  const [code, setCode] = useState(() => localStorage.getItem("shale.device.code") ?? "");
  const [bedType, setBedType] = useState(
    () => localStorage.getItem("shale.device.bed") ?? "textured_plate",
  );
  const [status, setStatus] = useState<api.HostStatus | null>(null);
  const [sending, setSending] = useState(false);
  const [asking, setAsking] = useState(false);
  const [tools, setTools] = useState<api.SliceTools | null>(null);
  const [note, setNote] = useState<string | null>(null);
  const [queue, setQueue] = useState<api.QueueJob[]>([]);
  const [queueAsking, setQueueAsking] = useState(false);
  /// Set only by the one press that names every queued job, and cleared when the
  /// queue empties. Nothing else may start a print.
  const [queueRunning, setQueueRunning] = useState(false);
  /// A job counts as finished only after the machine has reported RUNNING for
  /// it: the IDLE report that precedes a start would otherwise advance the
  /// queue before the first job laid a layer.
  const queueSeenRunning = useRef(false);

  useEffect(() => {
    localStorage.setItem("shale.device.host", host);
  }, [host]);
  useEffect(() => {
    localStorage.setItem("shale.device.serial", serial);
  }, [serial]);
  // The access code is a password for the machine. It stays in this profile's
  // storage and never in the repository or a log line.
  useEffect(() => {
    localStorage.setItem("shale.device.code", code);
  }, [code]);
  useEffect(() => {
    localStorage.setItem("shale.device.bed", bedType);
  }, [bedType]);

  // Fill the form from the environment once, if it was launched with a printer.
  useEffect(() => {
    let live = true;
    api
      .hostDefaults()
      .then((defaults) => {
        if (!live) return;
        setHost((current) => current || defaults.host);
        setSerial((current) => current || defaults.serial);
        setCode((current) => current || defaults.code);
        setBedType((current) => current || defaults.bedType);
      })
      .catch(() => undefined);
    return () => {
      live = false;
    };
  }, []);

  // One poll a second while the pane is open. The link itself runs in the
  // backend and reconnects on its own; this only reads the latest report.
  useEffect(() => {
    let live = true;
    const read = () =>
      api
        .hostStatus()
        .then((next) => {
          if (live) setStatus(next);
        })
        .catch(() => undefined);
    read();
    const timer = window.setInterval(read, 1000);
    return () => {
      live = false;
      window.clearInterval(timer);
    };
  }, []);

  useEffect(() => {
    let live = true;
    api
      .queueList()
      .then((rows) => {
        if (live) setQueue(rows);
      })
      .catch(() => undefined);
    return () => {
      live = false;
    };
  }, []);

  const statusOf = status ?? null;
  // A report can arrive before the machine has filled every field, so every
  // number goes through this rather than trusting `undefined` to be a number.
  const deg = (value: number | undefined) => (Number.isFinite(value) ? (value as number).toFixed(0) : "—");
  const connected = Boolean(statusOf?.connected);
  const printing = statusOf?.state === "RUNNING" || statusOf?.state === "PAUSE";
  const state = statusOf?.state || "offline";

  async function connect() {
    setNote(null);
    try {
      const fresh = await api.hostConnect({
        host: host.trim(),
        serial: serial.trim(),
        code: code.trim(),
        bedType,
        remoteDir: "cache",
      });
      setStatus(fresh);
    } catch (err) {
      setNote(String(err));
    }
  }

  async function disconnect() {
    setNote(null);
    try {
      await api.hostDisconnect();
      setStatus(null);
    } catch (err) {
      setNote(String(err));
    }
  }

  // The confirmation is built from the file itself: how many spools the slice
  // really uses, not how many the plate was configured with.
  async function ask() {
    setNote(null);
    try {
      setTools(await api.sliceTools());
    } catch {
      setTools(null);
    }
    setAsking(true);
  }

  async function send() {
    setAsking(false);
    setSending(true);
    setNote(null);
    try {
      const sent = await api.hostSend(bedType);
      setNote(`sent ${Math.round(sent.bytes / 1024)} kB as ${sent.remote}`);
    } catch (err) {
      setNote(String(err));
    } finally {
      setSending(false);
    }
  }

  // The queue moves itself forward — but only because one press named every job
  // in it. `advance` is reached from that press and from this effect, nowhere
  // else.
  async function advance() {
    if (queue.length === 0) {
      setQueueRunning(false);
      setNote("queue done — nothing left to print");
      try {
        setQueue(await api.queueRelease());
      } catch {
        undefined;
      }
      return;
    }
    setSending(true);
    setNote(null);
    try {
      const next = await api.queueStartNext(bedType);
      setQueue(next.queue);
      setNote(
        next.remaining
          ? `printing ${next.name} — ${next.remaining} still queued`
          : `printing ${next.name} — last in the queue`,
      );
    } catch (err) {
      setNote(String(err));
      setQueueRunning(false);
    } finally {
      setSending(false);
    }
  }

  useEffect(() => {
    if (!queueRunning || sending) return;
    if (state === "RUNNING" || state === "PREPARE") {
      queueSeenRunning.current = true;
      return;
    }
    if (!queueSeenRunning.current) return;
    // A failure is not a finish: plastic is on the bed, and starting the next
    // job from a queue nobody is watching is how a bad print becomes three.
    if (state === "FAILED") {
      setQueueRunning(false);
      queueSeenRunning.current = false;
      setNote("queue stopped — the machine reported a failed print");
      return;
    }
    if (state !== "FINISH" && state !== "IDLE") return;
    queueSeenRunning.current = false;
    void advance();
    // `advance` closes over the queue it was written against, so the effect
    // lists the queue's length rather than the function.
  }, [queueRunning, sending, state, queue.length]);

  async function queueThis() {
    setNote(null);
    try {
      const size = job.layers > 0 ? `${job.layers} layers` : "unsliced";
      // Two jobs from the same plate would otherwise share a name, and a
      // confirmation listing the same words twice names nothing.
      setQueue(await api.queueAdd(`${queue.length + 1} · ${plate} · ${size}`));
    } catch (err) {
      setNote(String(err));
    }
  }

  async function dropQueued(id: number) {
    try {
      setQueue(await api.queueRemove(id));
    } catch (err) {
      setNote(String(err));
    }
  }

  async function clearQueue() {
    try {
      setQueue(await api.queueClear());
      setQueueRunning(false);
    } catch (err) {
      setNote(String(err));
    }
  }

  async function beginQueue() {
    setQueueAsking(false);
    queueSeenRunning.current = false;
    setQueueRunning(true);
    await advance();
  }

  async function control(command: "pause" | "resume" | "stop") {
    setNote(null);
    try {
      await api.hostControl(command);
    } catch (err) {
      setNote(String(err));
    }
  }

  return (
    <div className="device">
      {/* Two columns, because the page was one column of sections with
          two-thirds of a wide screen left empty: the machine on the left, what
          is going to it on the right. */}
      <div className="device-col">
      <section className="device-head">
        <p className="device-kicker">device</p>
        <h2 className="device-title">{machine}</h2>
        <p className="device-sub">
          {source === "bambu" ? "Bambu Studio profile" : "Cura profile"} ·{" "}
          {bedWidth.toFixed(0)} × {bedDepth.toFixed(0)} × {bedHeight.toFixed(0)} mm ·{" "}
          {nozzleSize.toFixed(2)} mm nozzle
        </p>
        <p className={connected ? "device-status live" : "device-status"}>
          {connected
            ? `${state.toLowerCase()} · ${statusOf?.percent ?? 0}%${
                statusOf?.file ? ` · ${statusOf.file}` : ""
              }`
            : statusOf?.note
              ? `not connected — ${statusOf.note}`
              : "no printer connected"}
        </p>
      </section>

      <section className="device-live">
        <div className="device-bar" aria-label="print progress">
          <span style={{ width: `${Math.max(0, Math.min(100, statusOf?.percent ?? 0))}%` }} />
        </div>
        <div className="device-readouts">
          <span>
            layer <b>{statusOf?.layers ? `${statusOf.layer}/${statusOf.layers}` : "—"}</b>
          </span>
          <span>
            left <b>{statusOf?.remainingMin ? `${statusOf.remainingMin} min` : "—"}</b>
          </span>
          <span>
            nozzle <b>{statusOf ? `${deg(statusOf.nozzle)}° / ${deg(statusOf.nozzleTarget)}°` : "—"}</b>
          </span>
          <span>
            bed <b>{statusOf ? `${deg(statusOf.bed)}° / ${deg(statusOf.bedTarget)}°` : "—"}</b>
          </span>
        </div>

        <div className="device-controls">
          {asking ? (
            <div className="device-confirm">
              <p>
                print <b>{job.layers > 0 ? `${job.layers} layers` : "the plate"}</b> for{" "}
                <b>{clock(job.seconds)}</b> on the {bedType.replace(/_/g, " ")}?
              </p>
              {tools && tools.tools.length > 1 ? (
                <p className="device-warn">
                  this slice uses {tools.tools.length} spools ({tools.changes} tool changes)
                  {tools.usage.length
                    ? ` — ${tools.usage
                        // Bay N is extruder N-1, so the file's `T2` is the bay this
                        // window calls T3 — and the row of bays below is what the
                        // figure has to line up with. A tool and its figure also
                        // belong on one line: "T3:" alone at the end reads as a
                        // stray token.
                        .map((used) => `T${used.tool + 1}:\u00a0${used.metres.toFixed(2)}\u00a0m`)
                        .join(", ")}`
                    : ""}
                  . Bambu takes a sliced 3MF with an AMS mapping for a multi-colour plate, and Shale sends
                  plain G-code, so the machine will print this in one colour.
                </p>
              ) : null}
              <div>
                <button type="button" className="device-go" onClick={send}>
                  print it
                </button>
                <button type="button" onClick={() => setAsking(false)}>
                  cancel
                </button>
              </div>
            </div>
          ) : (
            <>
              <button
                type="button"
                className="device-go"
                disabled={!connected || !gcodeReady || sending || printing}
                onClick={ask}
              >
                {sending ? "sending…" : "send & print"}
              </button>
              <button
                type="button"
                disabled={!connected || state !== "RUNNING"}
                onClick={() => control("pause")}
              >
                pause
              </button>
              <button
                type="button"
                disabled={!connected || state !== "PAUSE"}
                onClick={() => control("resume")}
              >
                resume
              </button>
              <button
                type="button"
                disabled={!connected || !printing}
                onClick={() => control("stop")}
              >
                stop
              </button>
            </>
          )}
        </div>
        {note ? <p className="device-note">{note}</p> : null}
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
          <label>
            access code
            <input
              type="password"
              aria-label="printer access code"
              placeholder="8 characters, from the printer's LAN settings"
              value={code}
              onChange={(event) => setCode(event.target.value)}
            />
          </label>
          <label>
            plate
            <select
              aria-label="build plate"
              value={bedType}
              onChange={(event) => setBedType(event.target.value)}
            >
              <option value="textured_plate">textured PEI</option>
              <option value="smooth_plate">smooth PEI</option>
              <option value="engineering_plate">engineering</option>
              <option value="cool_plate">cool plate</option>
            </select>
          </label>
        </div>
        <div className="device-controls">
          <button type="button" disabled={connected} onClick={connect}>
            connect
          </button>
          <button type="button" disabled={!connected} onClick={disconnect}>
            disconnect
          </button>
          <span className="device-note">
            {connected
              ? `${statusOf?.reports ?? 0} reports received`
              : "the printer must be in LAN mode with the access code enabled"}
          </span>
        </div>
        <p className="device-note">
          the access code is kept in this profile's storage, never in the repository. the link is read-write:
          send, pause, resume and stop all go to the machine over its own local interface.
        </p>
      </section>
      </div>
      <div className="device-col">
      <section className="device-queue">
        <p className="device-label">queue</p>
        {queue.length === 0 ? (
          <p className="device-note">
            nothing queued — a job here starts on its own when the machine goes idle, so the
            queue is the one place that happens.
          </p>
        ) : (
          <ol className="device-queue-list">
            {queue.map((row) => (
              <li key={row.id} className={row.running ? "running" : undefined}>
                <span className="queue-name">{row.name}</span>
                <span className="queue-size">{Math.round(row.bytes / 1024)} kB</span>
                <button
                  type="button"
                  aria-label={`remove ${row.name}`}
                  onClick={() => dropQueued(row.id)}
                >
                  ×
                </button>
              </li>
            ))}
          </ol>
        )}

        {queueAsking ? (
          <div className="device-confirm">
            <p>
              print{" "}
              <b>
                {queue.length} {queue.length === 1 ? "job" : "jobs"}
              </b>
              {queue.length === 1 ? " now" : " one after another"} —{" "}
              {queue.map((row) => row.name).join(", ")}?
              {queue.length === 1
                ? " it starts on the machine as soon as you press."
                : " each one starts when the machine goes idle."}
            </p>
            <p className="device-warn">
              this is the only place a print can start without you pressing for it, and this press
              is what authorises every job in the list.
            </p>
            <div>
              <button type="button" className="device-go" onClick={beginQueue}>
                start the queue
              </button>
              <button type="button" onClick={() => setQueueAsking(false)}>
                cancel
              </button>
            </div>
          </div>
        ) : (
          <div className="device-controls">
            <button type="button" disabled={!gcodeReady} onClick={queueThis}>
              queue this slice
            </button>
            <button
              type="button"
              className="device-go"
              disabled={!connected || queue.length === 0 || queueRunning}
              onClick={() => setQueueAsking(true)}
            >
              {queueRunning ? "queue running" : "start queue"}
            </button>
            <button type="button" disabled={queue.length === 0} onClick={clearQueue}>
              clear
            </button>
          </div>
        )}
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
              <span className={spool.material ? "spool" : "spool empty"}>
                {spool.material || (baysInUse.includes(spool.bay) ? "no material set" : "empty bay")}
              </span>
            </li>
          ))}
        </ul>
        <p className="device-note">
          the plate prints from T1 unless a part is pointed at another bay. each spool is resolved as its own
          material, with its own hotend temperature.
        </p>
      </section>
      </div>
    </div>
  );
}
