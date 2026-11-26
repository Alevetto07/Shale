import type { SliceTools } from "./api";
import { plateWarnings, slotWarnings, spoolForTray, type SpoolChoice } from "./print-check";
import { Select } from "./ui";

export interface SendPlate {
  index: number;
  name: string;
  active: boolean;
  sliced?: boolean;
}

export interface SendReviewProps {
  filament: string;
  bedTempC: number;
  bedType: string;
  onBedType: (value: string) => void;
  tools: SliceTools | null;
  spools: SpoolChoice[];
  mapping: number[];
  onMapping: (tool: number, tray: number) => void;
  plates: SendPlate[];
  scope: "plate" | "all";
  onScope: (scope: "plate" | "all") => void;
  plateIndex: number;
  onPlate: (index: number) => void;
}

const PLATES = [
  { value: "textured_plate", label: "Textured PEI" },
  { value: "smooth_plate", label: "Smooth PEI" },
  { value: "engineering_plate", label: "Engineering" },
  { value: "cool_plate", label: "Cool plate" },
];

function trayLabel(tray: number, spools: SpoolChoice[]): string {
  const spool = spoolForTray(tray, spools);
  const material = spool?.material?.trim();
  if (tray < 0) return material ? `External spool · ${material}` : "External spool";
  return material ? `AMS ${tray + 1} · ${material}` : `AMS ${tray + 1} · empty`;
}

/**
 * What the person confirms before a print starts: which plate, which build
 * surface, and which spool each tool feeds from. Warnings sit in the dialog
 * so they are read at the press, not after the file has left.
 */
export function SendReview(props: SendReviewProps) {
  const tools = props.tools?.tools ?? [];
  const warnings = plateWarnings(props.filament, props.bedType, props.bedTempC);
  const slots = slotWarnings(tools, props.mapping, props.spools);
  const several = tools.length > 1;
  const allExternal = several && tools.every((tool) => (props.mapping[tool] ?? -1) < 0);
  const manyPlates = props.plates.length > 1;
  const unsliced = props.plates.filter((plate) => plate.sliced === false);
  const currentReady = props.plates.find((plate) => plate.index === props.plateIndex);

  const trayOptions = () => {
    const trays = props.spools.filter((spool) => spool.bay >= 2).map((spool) => spool.bay - 2);
    return [-1, ...trays].map((tray) => ({
      value: String(tray),
      label: trayLabel(tray, props.spools),
    }));
  };

  return (
    <div className="send-review">
      <label className="send-plate">
        <span>Build plate</span>
        <Select
          label="Build plate for this print"
          value={props.bedType}
          options={PLATES}
          onChange={props.onBedType}
        />
      </label>
      {warnings.map((note) => (
        <p key={note} className="device-warn" role="status">
          {note}
        </p>
      ))}
      {manyPlates && (
        <fieldset className="send-choice">
          <legend>Which plates</legend>
          <label>
            <input
              type="radio"
              name="send-scope"
              checked={props.scope === "plate"}
              onChange={() => props.onScope("plate")}
            />
            This plate
          </label>
          <label>
            <input
              type="radio"
              name="send-scope"
              checked={props.scope === "all"}
              onChange={() => props.onScope("all")}
              disabled={unsliced.length > 0}
            />
            Every plate
          </label>
          {props.scope === "plate" && (
            <Select
              label="Plate to print"
              value={String(props.plateIndex)}
              options={props.plates.map((plate) => ({
                value: String(plate.index),
                label: plate.sliced || plate.active ? plate.name : `${plate.name} (not sliced)`,
              }))}
              onChange={(value) => props.onPlate(Number(value))}
            />
          )}
          {props.scope === "all" && unsliced.length > 0 && (
            <p className="device-warn">
              Slice {unsliced.map((plate) => plate.name).join(", ")} before sending every plate.
            </p>
          )}
          {props.scope === "all" && unsliced.length === 0 && (
            <p className="device-note">
              The plate on screen prints now. The others wait in the queue until you say the bed is clear.
            </p>
          )}
          {props.scope === "plate" && currentReady?.sliced === false && (
            <p className="device-warn">Slice {currentReady.name} before sending it.</p>
          )}
        </fieldset>
      )}
      {tools.length > 0 && (
        <div className="send-map">
          <p className="device-label">Filament</p>
          {tools.map((tool) => {
            const used = props.tools?.usage?.find((row) => row.tool === tool);
            return (
              <label key={tool}>
                <span>
                  T{tool + 1}
                  {used ? ` · ${used.metres.toFixed(2)} m` : ""}
                </span>
                <Select
                  label={`Spool for T${tool + 1}`}
                  value={String(props.mapping[tool] ?? -1)}
                  options={trayOptions()}
                  onChange={(value) => props.onMapping(tool, Number(value))}
                />
              </label>
            );
          })}
        </div>
      )}
      {slots.map((note) => (
        <p key={note.tool} className="device-warn" role="status">
          {note.message}
        </p>
      ))}
      {allExternal && (
        <p className="device-warn">
          Every tool is on the external spool, so the printer uses one filament for all of them.
        </p>
      )}
    </div>
  );
}
