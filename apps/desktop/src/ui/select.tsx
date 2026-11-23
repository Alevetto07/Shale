import * as DropdownMenu from "@radix-ui/react-dropdown-menu";
import { Check, ChevronDown } from "lucide-react";

export interface SelectOption {
  value: string;
  label: string;
}

export function Select({
  label,
  value,
  options,
  onChange,
  placeholder = "none",
}: {
  label: string;
  value: string;
  options: SelectOption[];
  onChange: (value: string) => void;
  placeholder?: string;
}) {
  const current = options.find((option) => option.value === value);
  const shown = current?.label || placeholder;

  return (
    <div className="ui-select" data-ui-select="">
      <DropdownMenu.Root>
        <DropdownMenu.Trigger className="ui-select__trigger" aria-label={label}>
          <span className="ui-select__value">{shown}</span>
          <ChevronDown size={14} strokeWidth={1.5} aria-hidden />
        </DropdownMenu.Trigger>
        <DropdownMenu.Portal>
          <DropdownMenu.Content
            className="ui-dropdown ui-select__menu"
            sideOffset={6}
            align="start"
            collisionPadding={8}
          >
            {options.length === 0 ? (
              <div className="ui-dropdown__item ui-dropdown__item--muted">none</div>
            ) : (
              options.map((option) => (
                <DropdownMenu.Item
                  key={option.value}
                  className="ui-dropdown__item"
                  onSelect={() => onChange(option.value)}
                >
                  <span className="ui-select__check" aria-hidden>
                    {option.value === value ? <Check size={14} strokeWidth={1.5} /> : null}
                  </span>
                  <span>{option.label}</span>
                </DropdownMenu.Item>
              ))
            )}
          </DropdownMenu.Content>
        </DropdownMenu.Portal>
      </DropdownMenu.Root>
    </div>
  );
}
