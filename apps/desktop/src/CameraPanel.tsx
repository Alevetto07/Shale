import { useEffect, useRef, useState } from "react";
import * as api from "./api";
import { open } from "./platform/dialog";
import { Button, Select, Slider, SwitchRow } from "./ui";

export interface CameraPanelProps {
  printerId: string;
  host: string;
  serial: string;
  code: string;
  connected: boolean;
  /** The layer the printer last reported. Zero when it has not said. */
  layer: number;
  /** A finding that asks for a pause. The pane sends the pause; this panel does not. */
  onPause: () => void;
  /** `frames` are the pictures from just before a warning, as data URLs. */
  onNotify: (message: string, frames?: string[]) => void;
}

const H264_NAMES = ["p2s", "n2s", "x1", "x2", "h2", "h3", "n7"];
const LAPSE_LIMIT = 180;

function h264Machine(printerId: string): boolean {
  const id = printerId.toLowerCase();
  if (H264_NAMES.some((name) => id.includes(name))) return true;
  if (import.meta.env.VITE_SHALE_MOCK === "1" && typeof window !== "undefined") {
    return new URLSearchParams(window.location.search).get("codec") === "h264";
  }
  return false;
}

function protocol(printerId: string): string {
  return h264Machine(printerId)
    ? "RTSPS on port 322. The picture is H.264, decoded on this computer."
    : "JPEG over TLS on port 6000, using the LAN access code.";
}

function dataUrl(jpeg: string): string {
  return `data:image/jpeg;base64,${jpeg}`;
}

function byteLine(received: number, total: number): string {
  const count = (bytes: number) => Math.max(0, Math.round(bytes)).toLocaleString("en-GB");
  if (total > 0) return `${count(received)} of ${count(total)} bytes`;
  return `${count(received)} bytes`;
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
  layer,
  onPause,
  onNotify,
}: CameraPanelProps) {
  const h264 = h264Machine(printerId);
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
  const [sample, setSample] = useState<api.SensitivityPreview | null>(null);
  const [codec, setCodec] = useState<api.CodecStatus | null>(null);
  const [recording, setRecording] = useState(false);
  const [folder, setFolder] = useState<string | null>(null);
  const [intervalSec, setIntervalSec] = useState("5");
  const [captured, setCaptured] = useState(0);
  const [lapseNote, setLapseNote] = useState<string | null>(null);
  const [preview, setPreview] = useState<string[]>([]);
  const pauseKey = `shale.camera.pause.${serial || "printer"}`;
  const [pauseOptIn, setPauseOptIn] = useState(false);
  const pausedFor = useRef(false);
  const alerted = useRef(false);
  const recent = useRef<string[]>([]);
  const lapseFrames = useRef<string[]>([]);
  const pictureRaw = useRef<string | null>(null);
  const lastLayer = useRef(0);
  const lastCapture = useRef(0);

  useEffect(() => {
    setPauseOptIn(localStorage.getItem(pauseKey) === "1");
  }, [pauseKey]);

  useEffect(() => {
    if (!watchOn) return;
    let live = true;
    void api
      .cameraSensitivityPreview(sensitivity)
      .then((view) => {
        if (live) setSample(view);
      })
      .catch(() => undefined);
    return () => {
      live = false;
    };
  }, [watchOn, sensitivity]);

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
          if (!live || !jpeg) return;
          const url = dataUrl(jpeg);
          setPicture(url);
          pictureRaw.current = jpeg;
          recent.current = recent.current.concat(url).slice(-6);
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
            alerted.current = false;
            return;
          }
          const frames = recent.current.slice();
          const attached =
            frames.length > 0
              ? `${view.note} ${
                  frames.length === 1
                    ? "The last frame is attached."
                    : `The last ${frames.length} frames are attached.`
                }`
              : view.note;
          if (view.action === "pause" && !pausedFor.current) {
            pausedFor.current = true;
            alerted.current = true;
            onPause();
            onNotify(
              `${attached} The print was paused because you opted in for this printer.`,
              frames,
            );
          } else if (view.action === "notify" && !alerted.current) {
            alerted.current = true;
            onNotify(attached, frames);
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

  useEffect(() => {
    if (!h264) return;
    let live = true;
    const tick = () => {
      void api.cameraCodecStatus().then((status) => {
        if (live) setCodec(status);
      });
    };
    tick();
    const timer = window.setInterval(tick, codec?.phase === "downloading" ? 700 : 4000);
    return () => {
      live = false;
      window.clearInterval(timer);
    };
  }, [h264, codec?.phase]);

  useEffect(() => {
    if (!recording) return;
    const take = () => {
      const raw = pictureRaw.current;
      if (!raw || lapseFrames.current.length >= LAPSE_LIMIT) return;
      const now = Date.now();
      const layerChanged = layer > 0 && layer !== lastLayer.current;
      if (layerChanged) lastLayer.current = layer;
      else if (now - lastCapture.current < Number(intervalSec) * 1000) return;
      lastCapture.current = now;
      lapseFrames.current = lapseFrames.current.concat(raw);
      setCaptured(lapseFrames.current.length);
      setPreview((current) => current.concat(dataUrl(raw)).slice(-8));
    };
    take();
    const timer = window.setInterval(take, 500);
    return () => window.clearInterval(timer);
  }, [recording, layer, intervalSec]);

  async function start() {
    if (h264) {
      let status = codec;
      try {
        status = await api.cameraCodecStatus();
        if (!status.ready && !status.fromSource) {
          status = await api.cameraCodecDownload();
          const deadline = Date.now() + 120_000;
          while (status.phase === "downloading" && Date.now() < deadline) {
            await new Promise((resolve) => window.setTimeout(resolve, 400));
            status = await api.cameraCodecStatus();
          }
        }
        setCodec(status);
      } catch (err) {
        setFault(String(err));
      }
    }
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

  async function downloadCodec() {
    setBusy(true);
    try {
      setCodec(await api.cameraCodecDownload());
    } catch (err) {
      setFault(String(err));
    } finally {
      setBusy(false);
    }
  }

  async function chooseFolder() {
    const picked = await open({ directory: true, title: "Save the timelapse" });
    if (typeof picked === "string" && picked) setFolder(picked);
  }

  async function saveTimelapse() {
    if (!folder || lapseFrames.current.length === 0) return;
    setBusy(true);
    try {
      const saved = await api.cameraTimelapseSave({
        folder,
        name: serial || "print",
        frames: lapseFrames.current,
        intervalMs: Number(intervalSec) * 1000,
      });
      setLapseNote(saved.note);
    } catch (err) {
      setLapseNote(String(err));
    } finally {
      setBusy(false);
    }
  }

  async function toggleRecord(checked: boolean) {
    if (!checked) {
      setRecording(false);
      if (lapseFrames.current.length > 0 && folder) await saveTimelapse();
      return;
    }
    let next = folder;
    if (!next) {
      const picked = await open({ directory: true, title: "Save the timelapse" });
      if (typeof picked === "string" && picked) {
        next = picked;
        setFolder(picked);
      }
    }
    if (!next) {
      setLapseNote("Choose a folder for this print.");
      return;
    }
    lapseFrames.current = [];
    lastCapture.current = 0;
    lastLayer.current = 0;
    setCaptured(0);
    setPreview([]);
    setLapseNote(null);
    setRecording(true);
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
      {h264 ? (
        <p className="device-note camera-credit">{codec?.credit || api.OPENH264_CREDIT}</p>
      ) : null}
      {h264 && codec && !codec.ready && !codec.fromSource ? (
        <>
          <p className="device-note">{codec.message}</p>
          {codec.phase === "downloading" ? (
            <p className="device-note">{byteLine(codec.received, codec.total)}</p>
          ) : (
            <Button type="button" variant="secondary" size="sm" disabled={busy} onClick={() => void downloadCodec()}>
              Download OpenH264
            </Button>
          )}
        </>
      ) : null}
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
      <SwitchRow checked={recording} onChange={(checked) => void toggleRecord(checked)}>
        Record this print
      </SwitchRow>
      {recording || lapseNote || captured > 0 ? (
        <div className="camera-watch">
          <Select
            label="Frame interval"
            showLabel
            value={intervalSec}
            options={[
              { value: "2", label: "2 seconds" },
              { value: "5", label: "5 seconds" },
              { value: "10", label: "10 seconds" },
            ]}
            onChange={setIntervalSec}
          />
          <p className="device-note">
            One frame each time the layer changes. Otherwise, one frame on this interval.
          </p>
          <div className="device-controls">
            <Button type="button" variant="secondary" size="sm" onClick={() => void chooseFolder()}>
              {folder ? "Change folder" : "Choose a folder"}
            </Button>
            <Button
              type="button"
              variant="secondary"
              size="sm"
              disabled={busy || !folder || captured === 0}
              onClick={() => void saveTimelapse()}
            >
              Save the timelapse
            </Button>
            <span className="device-note">
              {folder ?? "No folder chosen"} · {captured} {captured === 1 ? "frame" : "frames"}
            </span>
          </div>
          {lapseNote ? <p className="device-note">{lapseNote}</p> : null}
          {preview.length > 0 ? (
            <div className="camera-snap-row" aria-label="Timelapse preview">
              {preview.map((src, index) => (
                <img key={`${index}-${src.length}`} src={src} alt={`Saved frame ${index + 1}`} />
              ))}
            </div>
          ) : null}
        </div>
      ) : null}
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
                <p className="device-note">{byteLine(model.received, model.total)}</p>
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
          {sample ? (
            <div className="camera-snaps">
              <p className="device-note">
                Test frame at this sensitivity. {sample.note} Confidence{" "}
                {Math.round(sample.confidence * 100)}%.
              </p>
              <div className="camera-snap-row" aria-label="Test frame for this sensitivity">
                <img src={dataUrl(sample.jpeg)} alt="A generated test frame at this sensitivity" />
              </div>
            </div>
          ) : null}
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
