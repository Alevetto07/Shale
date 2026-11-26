import { useEffect, useRef, useState } from "react";
import * as api from "./api";
import { Button, Select, Slider, SwitchRow } from "./ui";

export interface CameraPanelProps {
  printerId: string;
  host: string;
  serial: string;
  code: string;
  connected: boolean;
  /** A finding that asks for a pause. The pane sends the pause; this panel does not. */
  onPause: () => void;
  onNotify: (message: string) => void;
}

function protocol(printerId: string): string {
  const id = printerId.toLowerCase();
  const rtsps = ["p2s", "n2s", "x1", "x2", "h2", "h3", "n7"].some((name) => id.includes(name));
  return rtsps
    ? "RTSPS on port 322. The picture is H.264, decoded on this computer."
    : "JPEG over TLS on port 6000, using the LAN access code.";
}

function dataUrl(jpeg: string): string {
  return `data:image/jpeg;base64,${jpeg}`;
}

function megabytes(bytes: number): string {
  return `${Math.max(0, Math.round(bytes / 1_048_576))} MB`;
}

/**
 * The chamber camera: start, stop, a still, and a local look at the picture
 * for spaghetti, a part that has come off the bed, or a blob.
 */
export function CameraPanel({
  printerId,
  host,
  serial,
  code,
  connected,
  onPause,
  onNotify,
}: CameraPanelProps) {
  const [running, setRunning] = useState(false);
  const [fps, setFps] = useState(0);
  const [frames, setFrames] = useState(0);
  const [path, setPath] = useState("");
  const [picture, setPicture] = useState<string | null>(null);
  const [fault, setFault] = useState<string | null>(null);
  const [full, setFull] = useState(false);
  const [busy, setBusy] = useState(false);
  const [watchOn, setWatchOn] = useState(false);
  const [sensitivity, setSensitivity] = useState("medium");
  const [detector, setDetector] = useState("builtin");
  const [model, setModel] = useState<api.ModelStatus | null>(null);
  const [confidence, setConfidence] = useState(0.6);
  const [sampleSeconds, setSampleSeconds] = useState("5");
  const [debounceFrames, setDebounceFrames] = useState("3");
  const [finding, setFinding] = useState<api.WatchView | null>(null);
  const pauseKey = `shale.camera.pause.${serial || "printer"}`;
  const [pauseOptIn, setPauseOptIn] = useState(false);
  const pausedFor = useRef(false);

  useEffect(() => {
    setPauseOptIn(localStorage.getItem(pauseKey) === "1");
  }, [pauseKey]);

  useEffect(() => {
    if (!running) return;
    let live = true;
    const tick = () => {
      void api
        .cameraStatus()
        .then((status) => {
          if (!live) return;
          setFps(status.fps);
          setFrames(status.frames);
          setPath(status.path);
          setFault(status.fault);
          if (!status.running && status.fault) setRunning(false);
        })
        .catch((err: unknown) => {
          if (live) setFault(String(err));
        });
      void api
        .cameraFrame()
        .then((jpeg) => {
          if (live && jpeg) setPicture(dataUrl(jpeg));
        })
        .catch(() => undefined);
    };
    tick();
    const timer = window.setInterval(tick, 500);
    return () => {
      live = false;
      window.clearInterval(timer);
    };
  }, [running]);

  useEffect(() => {
    if (!running || !watchOn) return;
    let live = true;
    const judge = () => {
      void api
        .cameraWatch({
          confidence,
          sensitivity,
          debounceFrames: Number(debounceFrames),
          pause: pauseOptIn,
          detector,
        })
        .then((view) => {
          if (!live) return;
          setFinding(view);
          if (!view.alert) {
            pausedFor.current = false;
            return;
          }
          if (view.action === "pause" && !pausedFor.current) {
            pausedFor.current = true;
            onPause();
            onNotify(`${view.note} The print was paused because you opted in for this printer.`);
          } else if (view.action === "notify") {
            onNotify(view.note);
          }
        })
        .catch((err: unknown) => {
          if (live) setFault(String(err));
        });
    };
    judge();
    const timer = window.setInterval(judge, Math.max(2, Number(sampleSeconds)) * 1000);
    return () => {
      live = false;
      window.clearInterval(timer);
    };
  }, [
    running,
    watchOn,
    confidence,
    sensitivity,
    debounceFrames,
    sampleSeconds,
    pauseOptIn,
    detector,
    onPause,
    onNotify,
  ]);

  useEffect(() => {
    if (!watchOn || detector !== "obico") return;
    let live = true;
    const tick = () => {
      void api.cameraModelStatus().then((status) => {
        if (live) setModel(status);
      });
    };
    tick();
    const timer = window.setInterval(tick, 700);
    return () => {
      live = false;
      window.clearInterval(timer);
    };
  }, [watchOn, detector, model?.phase]);

  async function start() {
    setBusy(true);
    setFault(null);
    try {
      const status = await api.cameraStart(host, printerId || "bambu_lab_a1", code);
      setRunning(status.running);
      setPath(status.path);
      setFault(status.fault);
    } catch (err) {
      setFault(String(err));
      setRunning(false);
    } finally {
      setBusy(false);
    }
  }

  async function stop() {
    setBusy(true);
    try {
      await api.cameraStop();
      setRunning(false);
      setFps(0);
    } catch (err) {
      setFault(String(err));
    } finally {
      setBusy(false);
    }
  }

  async function snapshot() {
    setBusy(true);
    setFault(null);
    try {
      const jpeg = await api.cameraSnapshot(host, printerId || "bambu_lab_a1", code);
      setPicture(dataUrl(jpeg));
    } catch (err) {
      setFault(String(err));
    } finally {
      setBusy(false);
    }
  }

  async function cloudStill() {
    setBusy(true);
    setFault(null);
    try {
      const jpeg = await api.cameraCloudStill(serial);
      setPicture(dataUrl(jpeg));
    } catch (err) {
      setFault(String(err));
    } finally {
      setBusy(false);
    }
  }

  const stage = (
    <div className={full ? "camera-stage camera-full" : "camera-stage"}>
      {picture && !fault ? (
        <img className="camera-frame" alt="The printer camera" src={picture} />
      ) : (
        <div className={fault ? "camera-fault" : "camera-empty"}>
          {fault ?? "The camera is stopped. Start it to see the chamber."}
        </div>
      )}
      {picture && finding && finding.kind !== "none" ? (
        <div className="camera-overlay" aria-hidden>
          {finding.heatmapCols > 0
            ? finding.heatmap.map((heat, index) => (
                <span
                  key={index}
                  className="camera-heat"
                  style={{
                    width: `${100 / finding.heatmapCols}%`,
                    height: `${100 / Math.max(1, finding.heatmapRows)}%`,
                    opacity: Math.min(0.55, heat),
                  }}
                />
              ))
            : null}
          {finding.boxes.map((box, index) => (
            <span
              key={`box-${index}`}
              className="camera-box"
              style={{
                left: `${box.x * 100}%`,
                top: `${box.y * 100}%`,
                width: `${box.w * 100}%`,
                height: `${box.h * 100}%`,
              }}
            >
              {box.label}
            </span>
          ))}
        </div>
      ) : null}
      {full ? (
        <Button type="button" variant="secondary" size="sm" onClick={() => setFull(false)}>
          Leave fullscreen
        </Button>
      ) : null}
    </div>
  );

  return (
    <section className="device-panel camera-panel">
      <p className="device-label">Camera</p>
      <p className="device-note">{protocol(printerId)}</p>
      {stage}
      <div className="device-controls">
        {running ? (
          <Button type="button" variant="secondary" size="sm" disabled={busy} onClick={stop}>
            Stop
          </Button>
        ) : (
          <Button
            type="button"
            variant="secondary"
            size="sm"
            disabled={busy || !connected || !code}
            onClick={start}
          >
            Start
          </Button>
        )}
        <Button type="button" variant="ghost" size="sm" disabled={busy || !code} onClick={snapshot}>
          Snapshot
        </Button>
        <Button type="button" variant="ghost" size="sm" disabled={!picture} onClick={() => setFull(true)}>
          Fullscreen
        </Button>
        <Button type="button" variant="ghost" size="sm" disabled={busy || !serial} onClick={cloudStill}>
          Try the cloud picture
        </Button>
        <span className="device-note">
          {running ? `${fps.toFixed(1)} fps · ${frames} frames${path ? ` · ${path}` : ""}` : "Stopped"}
        </span>
      </div>
      <SwitchRow
        checked={watchOn}
        onChange={(checked) => {
          setWatchOn(checked);
          if (!checked) setFinding(null);
        }}
      >
        Watch for a failed print
      </SwitchRow>
      {watchOn ? (
        <div className="camera-watch">
          <Select
            label="Detector"
            showLabel
            value={detector}
            options={[
              { value: "builtin", label: "Built-in" },
              { value: "obico", label: "Obico model" },
            ]}
            onChange={setDetector}
          />
          {detector === "obico" && model && !model.ready ? (
            <>
              <p className="device-note">{model.message}</p>
              {model.phase === "downloading" ? (
                <p className="device-note">
                  {megabytes(model.received)} of {megabytes(model.total)}
                </p>
              ) : (
                <Button type="button" variant="secondary" size="sm" onClick={() => void api.cameraModelDownload()}>
                  Download the Obico failure model
                </Button>
              )}
              <Button type="button" variant="secondary" size="sm" onClick={() => setDetector("builtin")}>
                Use the built-in detector
              </Button>
            </>
          ) : null}
          <Select
            label="Sensitivity"
            showLabel
            value={sensitivity}
            options={[
              { value: "low", label: "Low" },
              { value: "medium", label: "Medium" },
              { value: "high", label: "High" },
            ]}
            onChange={setSensitivity}
          />
          <Slider
            label="Confidence"
            value={[Math.round(confidence * 100)]}
            min={40}
            max={90}
            step={5}
            inputDigits={0}
            formatValue={(value) => `${Math.round(value)} %`}
            onValueChange={(value) => setConfidence(value / 100)}
          />
          <Select
            label="Sample interval"
            showLabel
            value={sampleSeconds}
            options={[
              { value: "2", label: "2 seconds" },
              { value: "5", label: "5 seconds" },
              { value: "10", label: "10 seconds" },
            ]}
            onChange={setSampleSeconds}
          />
          <Select
            label="Frames to confirm"
            showLabel
            value={debounceFrames}
            options={[
              { value: "2", label: "2 frames" },
              { value: "3", label: "3 frames" },
              { value: "5", label: "5 frames" },
            ]}
            onChange={setDebounceFrames}
          />
          <SwitchRow
            checked={pauseOptIn}
            onChange={(checked) => {
              setPauseOptIn(checked);
              localStorage.setItem(pauseKey, checked ? "1" : "0");
            }}
          >
            Pause this printer when a failure holds
          </SwitchRow>
          <p className="device-note">
            {finding && finding.kind !== "none"
              ? `${finding.note} Confidence ${Math.round(finding.confidence * 100)}%.`
              : "Sampling stays on this computer. Pausing is off until you opt in for this printer."}
          </p>
        </div>
      ) : null}
    </section>
  );
}
