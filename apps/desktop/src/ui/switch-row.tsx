import { useId, type ReactNode } from "react";
import { Switch } from "./switch";

export function SwitchRow({
  checked,
  onChange,
  children,
}: {
  checked: boolean;
  onChange: (checked: boolean) => void;
  children: ReactNode;
}) {
  const id = useId();
  return (
    <div className="ui-switch-row">
      <span id={id}>{children}</span>
      <Switch checked={checked} onCheckedChange={onChange} aria-labelledby={id} />
    </div>
  );
}
