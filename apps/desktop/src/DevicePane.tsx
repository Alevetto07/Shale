import { useCallback, useEffect, useRef, useState } from "react";
import { CameraPanel } from "./CameraPanel";
import { CloudPanel } from "./CloudPanel";
import * as api from "./api";
import { Button, Dialog, Input, Select, Slider, Tooltip } from "./ui";

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
  /** Printer definition id, such as `bambu_lab_a1`. Removal is offered for the A1 only. */
  printerId: string;
  centerIsZero: boolean;
  adhesion: string;
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
  printerId,
  centerIsZero,
  adhesion,
}: DevicePaneProps) {
  const [host, setHost] = useState(() => localStorage.getItem("shale.device.host") ?? "");
  const [serial, setSerial] = useState(() => localStorage.getItem("shale.device.serial") ?? "");
  const [code, setCode] = useState("");
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
  /// What the run is allowed to do. The rules are in `shale_core::queue` and
  /// the gate is in Rust; this is only what the pane was told last.
  const [queueRun, setQueueRun] = useState("idle");
  const [failureFrames, setFailureFrames] = useState<string[]>([]);
  const [ejectMode, setEjectMode] = useState<"off" | "in_file" | "after_print">("off");
  const [ejectAsk, setEjectAsk] = useState<"in_file" | "after_print" | null>(null);
  const [coolC, setCoolC] = useState(28);
  const [removalNote, setRemovalNote] = useState<string | null>(null);
  const [removalGcode, setRemovalGcode] = useState("");
  const readyLatch = useRef(false);
  const sweepLatch = useRef(false);
  const checkLatch = useRef(false);
  const a1 = printerId === "bambu_lab_a1";

  useEffect(() => {
    localStorage.setItem("shale.device.host", host);
  }, [host]);
  useEffect(() => {
    localStorage.setItem("shale.device.serial", serial);
  }, [serial]);
  // The access code is a password for the machine, so it is kept in the Windows
  // Credential Manager rather than the WebView profile — which is a plain file
  // that anything running as this user can read. `codeReady` keeps the first
  // render, which has not read the vault yet, from writing an empty string over
  // the stored one.
  const [codeReady, setCodeReady] = useState(false);
  useEffect(() => {
    let live = true;
    api
      .secretGet("device.code")
      .catch(() => null)
      .then((stored) => {
        if (!live) return;
        // A code left in the old place is moved and the old place cleared, once.
        const legacy = localStorage.getItem("shale.device.code");
        const value = stored ?? legacy ?? "";
        if (value) setCode(value);
        if (legacy !== null) {
          api.secretSet("device.code", legacy).catch(() => undefined);
          localStorage.removeItem("shale.device.code");
        }
        setCodeReady(true);
      });
    return () => {
      live = false;
    };
  }, []);
  useEffect(() => {
    if (!codeReady) return;
    const write = code ? api.secretSet("device.code", code) : api.secretClear("device.code");
    write.catch(() => undefined);
  }, [code, codeReady]);
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

  // The printer is answering with a certificate this machine was not pinned to.
  // Forgetting the pin is the whole action: the next connection has nothing to
  // compare against, accepts what arrives and records it.
  async function trustPin() {
    setNote(null);
    try {
      await api.hostForgetPin();
      setStatus(null);
      await connect();
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

  /// Forget the machine, not just hang up on it: the pin is a claim about who
  /// this printer is, and a person needs a way to withdraw it — after a board
  /// swap, after typing the wrong address once, after borrowing a friend's.
  /// The window's half is the address and serial; the vault's half is the pin
  /// and the access code, which Rust deletes.
  async function forget() {
    setNote(null);
    try {
      await api.hostForget(serial, host);
      setHost("");
      setSerial("");
      setCode("");
      setStatus(null);
      setNote("forgotten — the address, the serial, the access code and the certificate pin");
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

  // The queue never moves itself forward. `startNext` is reached from two
  // presses and nowhere else: the one that arms the list, and the one that says
  // the plate has been cleared.
  async function startNext() {
    if (queue.length === 0) {
      setQueueRun("stopped");
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
      // An error is not a reason to try the next job: the run stops.
      try {
        const stopped = await api.queueStop(String(err));
        setQueueRun(stopped.state);
      } catch {
        setQueueRun("stopped");
      }
    } finally {
      setSending(false);
    }
  }

  // The machine's own reports go to the run, which can park it or stop it —
  // never start anything. A finish parks it until someone says the plate is
  // clear, because the bed still holds the part that was just printed.
  useEffect(() => {
    let live = true;
    void api
      .queueObserve(state)
      .then((update) => {
        if (!live) return;
        setQueueRun(update.state);
        if (update.note) setNote(update.note);
      })
      .catch(() => {
        undefined;
      });
    return () => {
      live = false;
    };
  }, [state]);

  async function queueThis() {
    setNote(null);
    try {
      const size = job.layers > 0 ? `${job.layers} layers` : "unsliced";
      // Two jobs from the same plate would otherwise share a name, and a
      // confirmation listing the same words twice names nothing.
      setQueue(
        await api.queueAdd(`${queue.length + 1} · ${plate} · ${size}`, {
          coolC,
          centerIsZero,
          bedWidth,
          bedDepth,
          bedHeight,
          adhesion,
          printerId,
        }),
      );
    } catch (err) {
      setNote(String(err));
    }
  }

  const pauseForFailure = useCallback(() => {
    void api.hostControl("pause").catch((err: unknown) => setNote(String(err)));
  }, []);
  const notifyFailure = useCallback((message: string, frames?: string[]) => {
    setNote(message);
    if (frames && frames.length > 0) setFailureFrames(frames);
  }, []);

  function chooseEject(next: "off" | "in_file" | "after_print") {
    if (next === "off") {
      setEjectMode("off");
      setEjectAsk(null);
      void api.queueSetEject("off").catch((err: unknown) => setNote(String(err)));
      return;
    }
    if (ejectMode === "off") {
      setEjectAsk(next);
      return;
    }
    setEjectMode(next);
    void api.queueSetEject(next).catch((err: unknown) => setNote(String(err)));
  }

  async function confirmEject() {
    if (!ejectAsk) return;
    const next = ejectAsk;
    setEjectAsk(null);
    setEjectMode(next);
    try {
      const update = await api.queueSetEject(next);
      setQueueRun(update.state);
    } catch (err) {
      setEjectMode("off");
      setNote(String(err));
    }
  }

  useEffect(() => {
    if (!a1 || ejectMode === "off") {
      setRemovalNote(null);
      setRemovalGcode("");
      return;
    }
    let live = true;
    void api
      .removalPreview({
        printerId,
        coolC,
        centerIsZero,
        bedWidth,
        bedDepth,
        bedHeight,
        adhesion,
      })
      .then((view) => {
        if (!live) return;
        setRemovalGcode(view.gcode);
        setRemovalNote(view.accepted ? null : view.reasons.join(" "));
      })
      .catch(() => {
        if (live) setRemovalNote(null);
      });
    return () => {
      live = false;
    };
  }, [a1, ejectMode, printerId, coolC, centerIsZero, bedWidth, bedDepth, bedHeight, adhesion]);

  useEffect(() => {
    if (queueRun !== "cooling" || ejectMode === "off") return;
    let live = true;
    void api
      .queueCooled(status?.bed ?? 999, coolC)
      .then((update) => {
        if (!live) return;
        setQueueRun(update.state);
        if (update.note) setNote(update.note);
      })
      .catch((err: unknown) => {
        if (live) setNote(String(err));
      });
    return () => {
      live = false;
    };
  }, [queueRun, ejectMode, status?.bed, coolC]);

  useEffect(() => {
    if (queueRun !== "sweeping") {
      sweepLatch.current = false;
      return;
    }
    if (sweepLatch.current || ejectMode !== "after_print") return;
    sweepLatch.current = true;
    let live = true;
    void api
      .removalSend()
      .then((update) => {
        if (!live) return;
        setQueueRun(update.state);
        if (update.note) setNote(update.note);
      })
      .catch((err: unknown) => {
        if (!live) return;
        sweepLatch.current = false;
        setNote(String(err));
      });
    return () => {
      live = false;
    };
  }, [queueRun, ejectMode]);

  useEffect(() => {
    if (queueRun !== "checking_plate") {
      checkLatch.current = false;
      return;
    }
    if (checkLatch.current || ejectMode === "off") return;
    checkLatch.current = true;
    let live = true;
    void api
      .cameraCompare(serial)
      .then(async (verdict) => {
        const update = await api.queuePlateVerdict(verdict.clear);
        if (!live) return;
        setQueueRun(update.state);
        setNote(verdict.note);
      })
      .catch(async (err: unknown) => {
        if (!live) return;
        const update = await api.queueStop(String(err)).catch(() => null);
        setQueueRun(update?.state ?? "stopped");
        setNote(String(err));
      });
    return () => {
      live = false;
    };
  }, [queueRun, ejectMode, serial]);

  useEffect(() => {
    if (queueRun !== "ready") {
      readyLatch.current = false;
      return;
    }
    if (ejectMode === "off" || readyLatch.current) return;
    readyLatch.current = true;
    void startNext();
  }, [queueRun, ejectMode]);

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
      setQueueRun("stopped");
    } catch (err) {
      setNote(String(err));
    }
  }

  /** The press that authorises the list. It authorises the first job only. */
  async function beginQueue() {
    setQueueAsking(false);
    try {
      const armed = await api.queueArm();
      setQueueRun(armed.state);
    } catch (err) {
      setNote(String(err));
      return;
    }
    await startNext();
  }

  /** The press that says the finished print is off the bed. */
  async function plateCleared() {
    try {
      const cleared = await api.queuePlateCleared();
      setQueueRun(cleared.state);
      if (cleared.note) setNote(cleared.note);
    } catch (err) {
      setNote(String(err));
      return;
    }
    await startNext();
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
        <p className="device-kicker">Device</p>
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
              ? `Not connected. ${statusOf.note}`
              : "No printer connected"}
        </p>
        {statusOf?.note?.includes("certificate changed") ? (
          <div className="device-pin">
            <p>
              The machine on this address presented a different certificate. A printer that was
              factory reset, or had its board replaced, does this; anything else answering in its
              place does too. Trust the new one only if you know why it changed.
            </p>
            <Button type="button" variant="secondary" size="sm" onClick={trustPin}>
              Trust the new certificate
            </Button>
          </div>
        ) : null}
      </section>

      <CloudPanel taskName={plate} />

      <CameraPanel
        printerId={printerId}
        host={host}
        serial={serial}
        code={code}
        connected={connected}
        layer={statusOf?.layer ?? 0}
        onPause={pauseForFailure}
        onNotify={notifyFailure}
      />

      <section className="device-live">
        <div className="device-bar" aria-label="Print progress">
          <span style={{ width: `${Math.max(0, Math.min(100, statusOf?.percent ?? 0))}%` }} />
        </div>
        <div className="device-readouts">
          <span>
            Layer <b>{statusOf?.layers ? `${statusOf.layer}/${statusOf.layers}` : "—"}</b>
          </span>
          <span>
            Left <b>{statusOf?.remainingMin ? `${statusOf.remainingMin} min` : "—"}</b>
          </span>
          <span>
            Nozzle <b>{statusOf ? `${deg(statusOf.nozzle)}° / ${deg(statusOf.nozzleTarget)}°` : "—"}</b>
          </span>
          <span>
            Bed <b>{statusOf ? `${deg(statusOf.bed)}° / ${deg(statusOf.bedTarget)}°` : "—"}</b>
          </span>
        </div>

        <div className="device-controls">
          <Dialog
            open={asking}
            onOpenChange={setAsking}
            title="Print this plate?"
            description={`Print ${job.layers > 0 ? `${job.layers} layers` : "the plate"} for ${clock(job.seconds)} on the ${bedType.replace(/_/g, " ")}.`}
          >
            {tools && (tools.tools?.length ?? 0) > 1 ? (
              <p className="device-warn">
                This slice uses {tools.tools.length} spools ({tools.changes} tool changes)
                {(tools.usage?.length ?? 0) > 0
                  ? ` — ${tools.usage
                      .map((used) => `T${used.tool + 1}:\u00a0${used.metres.toFixed(2)}\u00a0m`)
                      .join(", ")}`
                  : ""}
                . Bambu takes a sliced 3MF with an AMS mapping for a multi-colour plate, and Shale sends
                plain G-code, so the machine will print this in one colour.
              </p>
            ) : null}
            <div className="device-controls">
              <Button type="button" variant="primary" onClick={send}>
                Print it
              </Button>
              <Button type="button" variant="ghost" onClick={() => setAsking(false)}>
                Cancel
              </Button>
            </div>
          </Dialog>
          <Button
            type="button"
            variant="primary"
            disabled={!connected || !gcodeReady || sending || printing}
            onClick={ask}
          >
            {sending ? "Sending…" : "Send and print"}
          </Button>
          <Button type="button" variant="secondary" size="sm" disabled={!connected || state !== "RUNNING"} onClick={() => control("pause")}>
            Pause
          </Button>
          <Button type="button" variant="secondary" size="sm" disabled={!connected || state !== "PAUSE"} onClick={() => control("resume")}>
            Resume
          </Button>
          <Button type="button" variant="destructive" size="sm" disabled={!connected || !printing} onClick={() => control("stop")}>
            Stop
          </Button>
        </div>
        {note ? <p className="device-note">{note}</p> : null}
      </section>

      <section className="device-panel">
        <p className="device-label">Connection</p>
        <div className="device-form">
          <label className="ui-field">
            <span className="ui-field__label">Host</span>
            <Input
              type="text"
              aria-label="Printer host"
              placeholder="Printer address"
              value={host}
              onChange={(event) => setHost(event.target.value)}
            />
          </label>
          <label className="ui-field">
            <span className="ui-field__label">Serial</span>
            <Input
              type="text"
              aria-label="Printer serial"
              placeholder="Serial number"
              value={serial}
              onChange={(event) => setSerial(event.target.value)}
            />
          </label>
          <label className="ui-field">
            <span className="ui-field__label">Access code</span>
            <Input
              type="password"
              className="access-code"
              aria-label="Printer access code"
              placeholder="8 characters from the printer"
              value={code}
              onChange={(event) => setCode(event.target.value)}
            />
          </label>
          <label className="ui-field">
            <span className="ui-field__label">Plate</span>
            <Select
              label="Build plate"
              value={bedType}
              options={[
                { value: "textured_plate", label: "Textured PEI" },
                { value: "smooth_plate", label: "Smooth PEI" },
                { value: "engineering_plate", label: "Engineering" },
                { value: "cool_plate", label: "Cool plate" },
              ]}
              onChange={setBedType}
            />
          </label>
        </div>
        <div className="device-controls">
          <Button type="button" variant="secondary" size="sm" disabled={connected} onClick={connect}>
            Connect
          </Button>
          <Button type="button" variant="secondary" size="sm" disabled={!connected} onClick={disconnect}>
            Disconnect
          </Button>
          <Tooltip label="Forget this printer: clear the address, serial, access code and pin">
            <Button type="button" variant="ghost" size="sm" disabled={!host && !serial} onClick={forget}>
              Forget
            </Button>
          </Tooltip>
          <span className="device-note">
            {connected
              ? `${statusOf?.reports ?? 0} reports received`
              : "The printer must be in LAN mode with the access code enabled"}
          </span>
        </div>
        <p className="device-note">
          The access code is kept in the Windows credential manager, never in the repository. The link is read-write:
          send, pause, resume and stop all go to the machine over its own local interface.
        </p>
      </section>
      </div>
      <div className="device-col">
      <section className="device-queue">
        <p className="device-label">Queue</p>
        {queue.length === 0 ? (
          <p className="device-note">
            Nothing queued. The first job starts when you press Start queue. The next one waits
            until you say the plate is clear.
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
        {failureFrames.length > 0 ? (
          <div className="camera-snaps">
            <p className="device-note">Frames from just before this warning.</p>
            <div className="camera-snap-row" aria-label="Frames from just before this warning">
              {failureFrames.map((src, index) => (
                <img key={index} src={src} alt={`Frame ${index + 1} from just before the warning`} />
              ))}
            </div>
          </div>
        ) : null}

        <Dialog
          open={queueAsking}
          onOpenChange={setQueueAsking}
          title="Start the queue?"
          description={
            queue.length === 1
              ? `Print ${queue.map((row) => row.name).join(", ")} now. It starts on the machine as soon as you press.`
              : `Print ${queue.length} jobs one after another — ${queue.map((row) => row.name).join(", ")}. The first starts now; after each print you will be asked to confirm the plate is clear before the next one goes.`
          }
        >
          <p className="device-warn">
            {ejectMode === "off"
              ? "This press authorises the first job. Nothing else starts on its own: a finished print stays on the bed until you say it is off."
              : "This press authorises the first job. Removal is on, so after a print the plate is checked with the camera before the next job. That path is experimental and untested on real hardware."}
          </p>
          <div className="device-controls">
            <Button type="button" variant="primary" onClick={beginQueue}>
              Start the queue
            </Button>
            <Button type="button" variant="ghost" onClick={() => setQueueAsking(false)}>
              Cancel
            </Button>
          </div>
        </Dialog>
        <div className="device-controls">
          <Button type="button" variant="secondary" size="sm" disabled={!gcodeReady} onClick={queueThis}>
            Queue this slice
          </Button>
          {queueRun === "waiting_for_plate" ? (
            <Button type="button" variant="primary" size="sm" onClick={plateCleared}>
              The plate is clear — start the next
            </Button>
          ) : (
            <Button
              type="button"
              variant="primary"
              size="sm"
              disabled={
                !connected ||
                queue.length === 0 ||
                queueRun === "armed" ||
                queueRun === "printing"
              }
              onClick={() => setQueueAsking(true)}
            >
              {queueRun === "armed" || queueRun === "printing" ? "Queue running" : "Start queue"}
            </Button>
          )}
          <Button type="button" variant="ghost" size="sm" disabled={queue.length === 0} onClick={clearQueue}>
            Clear
          </Button>
        </div>
        {a1 ? (
          <div className="camera-removal">
            <p className="device-warn">
              Experimental print removal for the A1. Untested on real hardware. Off unless you
              turn it on. The nozzle will move. Stay with the printer the first time and be ready
              to switch it off.
            </p>
            <Select
              label="Print removal"
              showLabel
              value={ejectMode}
              options={[
                { value: "off", label: "Off" },
                { value: "in_file", label: "Append the sweep to the file" },
                { value: "after_print", label: "Send the sweep after the bed cools" },
              ]}
              onChange={(value) => chooseEject(value as "off" | "in_file" | "after_print")}
            />
            {ejectMode !== "off" ? (
              <>
                <Slider
                  label="Cool the bed to"
                  value={[coolC]}
                  min={20}
                  max={40}
                  step={1}
                  unit="°C"
                  onValueChange={setCoolC}
                />
                <Button
                  type="button"
                  variant="ghost"
                  size="sm"
                  disabled={!connected}
                  onClick={() => {
                    void api.cameraSaveReference(serial).then(
                      () => setNote("Saved the empty-plate picture."),
                      (err: unknown) => setNote(String(err)),
                    );
                  }}
                >
                  Save empty-plate picture
                </Button>
                {removalNote ? <p className="device-warn">{removalNote}</p> : null}
                {removalGcode ? (
                  <details className="camera-gcode">
                    <summary>The sweep this part would get</summary>
                    <pre>{removalGcode}</pre>
                  </details>
                ) : null}
              </>
            ) : null}
            <Dialog
              open={ejectAsk !== null}
              onOpenChange={(open) => {
                if (!open) setEjectAsk(null);
              }}
              title="Turn on experimental removal?"
              description="After a print finishes, the bed cools, then the nozzle drops just above the plate and the bed moves so the part is pushed off the front. This has not been tried on a real A1. A part that is too tall, too small, too wide, or stuck too loosely is refused."
            >
              <p className="device-warn">
                Stay with the printer. Keep a hand near the power switch. The sweep uses slow
                moves and several passes, and it stops if the camera still sees a part.
              </p>
              <div className="device-controls">
                <Button type="button" variant="primary" onClick={confirmEject}>
                  I understand — turn it on
                </Button>
                <Button type="button" variant="ghost" onClick={() => setEjectAsk(null)}>
                  Leave it off
                </Button>
              </div>
            </Dialog>
          </div>
        ) : null}
      </section>

      <section className="device-grid">
        <article className="device-card">
          <p className="device-label">Plate</p>
          <p className="device-big">{job.layers > 0 ? job.layers : "—"}</p>
          <p className="device-note">{job.layers > 0 ? "Layers sliced" : "Nothing sliced yet"}</p>
        </article>
        <article className="device-card">
          <p className="device-label">Print time</p>
          <p className="device-big">{clock(job.seconds)}</p>
          <p className="device-note">{job.layers > 0 ? `At ${job.layerHeight.toFixed(2)} mm layers` : "Slice to find out"}</p>
        </article>
        <article className="device-card">
          <p className="device-label">Nozzle</p>
          <p className="device-big">{job.nozzle.toFixed(0)}°</p>
          <p className="device-note">Bed {job.bedTemp.toFixed(0)}° · {job.material || "No material"}</p>
        </article>
        <article className="device-card">
          <p className="device-label">Engine</p>
          <p className="device-big">{engineBackend === "cli" ? "CLI" : engineBackend === "arcus" ? "Arcus" : engineBackend}</p>
          <p className="device-note">{engineReady ? "Ready" : "Not found"}</p>
        </article>
      </section>

      <section className="device-panel">
        <p className="device-label">Spools</p>
        <ul className="device-spools">
          {spools.map((spool) => (
            <li key={spool.bay}>
              <span className="bay">T{spool.bay}</span>
              <span className={spool.material ? "spool" : "spool empty"}>
                {spool.material || (baysInUse.includes(spool.bay) ? "No material set" : "Empty bay")}
              </span>
            </li>
          ))}
        </ul>
        <p className="device-note">
          The plate prints from T1 unless a part is pointed at another bay. Each spool is resolved as its own
          material, with its own hotend temperature.
        </p>
      </section>
      </div>
    </div>
  );
}
