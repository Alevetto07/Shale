import { useEffect, useMemo, useState } from "react";
import { DemoFrame } from "./DemoFrame";

type Printer = {
  id: string;
  name: string;
  manufacturer: string;
  bed_mm: number;
};

export function PrinterCatalog() {
  const [printers, setPrinters] = useState<Printer[]>([]);
  const [query, setQuery] = useState("");
  const [active, setActive] = useState<string>("bambu_lab_a1");

  useEffect(() => {
    fetch("/demo/printers.json")
      .then((r) => r.json())
      .then((json: Printer[]) => setPrinters(json))
      .catch(() => setPrinters([]));
  }, []);

  const filtered = useMemo(() => {
    const q = query.trim().toLowerCase();
    if (!q) return printers;
    return printers.filter(
      (p) => p.name.toLowerCase().includes(q) || p.id.toLowerCase().includes(q),
    );
  }, [printers, query]);

  const selected = printers.find((p) => p.id === active);

  return (
    <DemoFrame title="Printer catalog" note="Selectable list from Shale v0.1 (Bambu only)">
      <input
        className="demo-input"
        type="search"
        placeholder="Search catalogue"
        value={query}
        onChange={(e) => setQuery(e.target.value)}
        aria-label="Search Bambu printers"
      />
      <ul className="printer-list" role="listbox" aria-label="Bambu printers">
        {filtered.map((p) => (
          <li key={p.id}>
            <button
              type="button"
              role="option"
              aria-selected={active === p.id}
              className={active === p.id ? "printer-row is-active" : "printer-row"}
              onClick={() => setActive(p.id)}
            >
              <span className="printer-name">{p.name}</span>
              <span className="mono dim">{p.id}</span>
            </button>
          </li>
        ))}
      </ul>
      {selected && (
        <p className="demo-readout">
          {selected.name} · {selected.bed_mm}×{selected.bed_mm} mm bed
        </p>
      )}
    </DemoFrame>
  );
}