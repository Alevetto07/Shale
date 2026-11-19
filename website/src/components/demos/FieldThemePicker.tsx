import { useEffect, useState } from "react";

const THEMES = [
  { id: "andromeda", label: "Andromeda" },
  { id: "saturn", label: "Saturn" },
  { id: "nebula", label: "Nebula" },
  { id: "lunar", label: "Lunar" },
] as const;

export function FieldThemePicker() {
  const [active, setActive] = useState("andromeda");

  useEffect(() => {
    document.documentElement.dataset.field = active;
  }, [active]);

  return (
    <div className="field-picker" role="group" aria-label="Plate field style">
      {THEMES.map((t) => (
        <button
          key={t.id}
          type="button"
          className={active === t.id ? "field-picker-btn is-active" : "field-picker-btn"}
          onClick={() => setActive(t.id)}
          aria-pressed={active === t.id}
        >
          {t.label}
        </button>
      ))}
    </div>
  );
}