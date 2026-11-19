import { useEffect, useState, type ReactNode } from "react";
import { DemoProvider, useDemo } from "../../lib/demo-context";
import { GcodeViewer } from "../demos/GcodeViewer";
import { downloads, checksumsUrl, primaryDownload } from "../../config/site";

function LayerPreview() {
  const { data } = useDemo();
  const layer = Math.min(95, (data?.layers.length ?? 1) - 1);
  return (
    <GcodeViewer
      data={data}
      layerIndex={layer}
      showTravel={false}
      buildProgress={1}
      showLegend={false}
      className="artifact-viewer"
    />
  );
}

export default function ArtifactSplitSection() {
  const [excerpt, setExcerpt] = useState("");
  const [preset, setPreset] = useState("");
  const [resolved, setResolved] = useState("");

  useEffect(() => {
    fetch("/demo/gcode-excerpt.txt")
      .then((r) => r.text())
      .then(setExcerpt);
    fetch("/demo/a1-preset-0.20.json")
      .then((r) => r.json())
      .then((j) => setPreset(JSON.stringify(j, null, 2)));
    fetch("/demo/shale-resolved-slice.json")
      .then((r) => r.json())
      .then((j) => setResolved(JSON.stringify(j.resolved, null, 2) + "\n\n" + JSON.stringify(j.result, null, 2)));
  }, []);

  const sums = downloads
    .map((d) => `${d.sha256}  ${d.fileName}`)
    .join("\n");

  return (
    <section className="artifact-split" id="artifacts" aria-labelledby="artifacts-title" data-reveal>
      <h2 id="artifacts-title">Raw artefacts</h2>
      <p className="dim">Real G-code, Bambu preset source, and release checksums — side by side with what you see on screen.</p>

      <SplitRow
        left={
          <>
            <h3>t-wing_972c.gcode (excerpt)</h3>
            <pre>{excerpt}</pre>
          </>
        }
        right={
          <DemoProvider eager>
            <h3>Rendered layer 96 / 120</h3>
            <LayerPreview />
          </DemoProvider>
        }
      />

      <SplitRow
        left={
          <>
            <h3>Bambu preset JSON</h3>
            <pre>{preset}</pre>
          </>
        }
        right={
          <>
            <h3>Shale resolved slice</h3>
            <pre>{resolved}</pre>
          </>
        }
      />

      <SplitRow
        left={
          <>
            <h3>SHA256SUMS.txt (excerpt)</h3>
            <pre>{sums}</pre>
            <p className="dim">
              <a href={checksumsUrl}>Full checksums</a>
            </p>
          </>
        }
        right={
          <>
            <h3>{primaryDownload.fileName}</h3>
            <p>{primaryDownload.description}</p>
            <p className="mono dim">{primaryDownload.sizeLabel}</p>
          </>
        }
      />
    </section>
  );
}

function SplitRow({ left, right }: { left: ReactNode; right: ReactNode }) {
  return (
    <div className="artifact-split__row">
      <article className="artifact-pane">{left}</article>
      <article className="artifact-pane">{right}</article>
    </div>
  );
}