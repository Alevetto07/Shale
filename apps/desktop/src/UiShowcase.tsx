import * as DropdownMenu from "@radix-ui/react-dropdown-menu";
import { useState, type HTMLAttributes } from "react";
import { toast } from "sonner";
import { Button, IconButton, Input, Slider, Switch } from "./ui";
import { Settings } from "lucide-react";

/** Static fixture for review screenshots (`?showcase=ui`). */
export function UiShowcase() {
  const [on, setOn] = useState(true);
  const [layer, setLayer] = useState(0.2);

  return (
    <Card className="ui-showcase" aria-label="Component showcase">
      <h1 className="ui-showcase__title">UI components</h1>
      <div className="ui-showcase__grid">
        <div className="ui-showcase__block">
          <span className="ui-showcase__label">Buttons</span>
          <div className="ui-showcase__row">
            <Button variant="primary">Primary</Button>
            <Button variant="secondary">Secondary</Button>
            <Button variant="ghost">Ghost</Button>
            <Button variant="pill">Pill</Button>
            <Button variant="destructive">Destructive</Button>
          </div>
        </div>
        <div className="ui-showcase__block">
          <span className="ui-showcase__label">Icon button &amp; tooltip</span>
          <IconButton label="Settings" shortcut="S">
            <Settings size={16} strokeWidth={1.5} />
          </IconButton>
        </div>
        <div className="ui-showcase__block">
          <span className="ui-showcase__label">Input</span>
          <Input aria-label="Search" placeholder="Search settings…" defaultValue="Andromeda" />
        </div>
        <div className="ui-showcase__block">
          <span className="ui-showcase__label">Switch</span>
          <Switch checked={on} onCheckedChange={setOn} aria-label="Supports enabled" />
        </div>
        <div className="ui-showcase__block ui-showcase__block--wide">
          <span className="ui-showcase__label">Slider</span>
          <Slider
            label="Layer height"
            min={0.08}
            max={0.4}
            step={0.01}
            value={[layer]}
            inputDigits={2}
            unit="mm"
            onValueChange={setLayer}
          />
        </div>
        <div className="ui-showcase__block">
          <span className="ui-showcase__label">Dropdown</span>
          <DropdownMenu.Root>
            <DropdownMenu.Trigger asChild>
              <Button variant="secondary">Open menu</Button>
            </DropdownMenu.Trigger>
            <DropdownMenu.Portal>
              <DropdownMenu.Content className="ui-dropdown" sideOffset={6}>
                <DropdownMenu.Item className="ui-dropdown__item">New project</DropdownMenu.Item>
                <DropdownMenu.Item className="ui-dropdown__item">Import model…</DropdownMenu.Item>
              </DropdownMenu.Content>
            </DropdownMenu.Portal>
          </DropdownMenu.Root>
        </div>
        <div className="ui-showcase__block">
          <span className="ui-showcase__label">Toast</span>
          <Button variant="secondary" onClick={() => toast.success("Export finished")}>
            Show toast
          </Button>
        </div>
      </div>
    </Card>
  );
}

function Card({ className, children, ...rest }: HTMLAttributes<HTMLDivElement>) {
  return (
    <div className={className} {...rest}>
      {children}
    </div>
  );
}