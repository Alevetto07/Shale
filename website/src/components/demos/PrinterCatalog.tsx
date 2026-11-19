import { useEffect, useMemo, useState } from "react";
import { useDemo, type PrinterRow } from "../../lib/demo-context";
import { PrinterSilhouette } from "./PrinterSilhouette";

function silhouetteFor(id: string): PrinterRow["silhouette"] {
  if (id.includes("mini")) return "mini";
  if (id.includes("p1") || id.includes("x1") || id.includes("h2") || id.includes("h3") || id.includes("p2"))
    return "enclosed";
  return "core";
}

const enriched = (rows: PrinterRow[]): PrinterRow[] =>
  rows.map((p) => ({
    ...p,
    nozzle_mm: p.nozzle_mm ?? 0.4,
    silhouette: p.silhouette ?? silhouetteFor(p.id),
  }));

export function PrinterCatalog({ autoStart: _autoStart }: { autoStart?: boolean }) {
  const { printerId, selectPrinter } = useDemo();
  const [printers, setPrinters] = useState<PrinterRow[]>([]);
  const [query, setQuery] = useState("");

  useEffect(() => {
    fetch("/demo/printers.json")
      .then((r) => r.json())
      .then((json: PrinterRow[]) => setPrinters(enriched(json)))
      .catch(() => setPrinters([]));
  }, []);

  const filtered = useMemo(() => {
    const q = query.trim().toLowerCase();
    if (!q) return printers;
    return printers.filter(
      (p) => p.name.toLowerCase().includes(q) || p.id.toLowerCase().includes(q),
    );
  }, [printers, query]);

  const selected = printers.find((p) => p.id === printerId);

  return (
    <section className="demo-panel demo-panel--printers" id="printers">
      <header className="demo-panel__head">
        <div>
          <h2 className="demo-panel__title">Bambu printers</h2>
          <p className="demo-panel__note">Bed size updates the build plate in the 3D preview.</p>
        </div>
      </header>
      <input
        className="demo-input"
        type="search"
        placeholder="Search"
        value={query}
        onChange={(e) => setQuery(e.target.value)}
        aria-label="Search Bambu printers"
      />
      <ul className="printer-list printer-list--rich" role="listbox" aria-label="Bambu printers">
        {filtered.map((p) => (
          <li key={p.id}>
            <button
              type="button"
              role="option"
              aria-selected={printerId === p.id}
              className={printerId === p.id ? "printer-row printer-row--rich is-active" : "printer-row printer-row--rich"}
              onClick={() => selectPrinter(p)}
            >
              <PrinterSilhouette kind={p.silhouette} />
              <span className="printer-row__body">
                <span className="printer-name">{p.name}</span>
                <span className="printer-specs">
                  <span className="mono">{p.bed_mm} mm bed</span>
                  <span className="mono"> · {p.nozzle_mm} mm</span>
                </span>
              </span>
            </button>
          </li>
        ))}
      </ul>
      {selected && (
        <p className="demo-readout">
          Plate {selected.bed_mm}×{selected.bed_mm} mm · nozzle {selected.nozzle_mm} mm
        </p>
      )}
    </section>
  );
}