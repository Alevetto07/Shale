import * as Dialog from "@radix-ui/react-dialog";
import { Command } from "cmdk";
import { motion, useReducedMotion } from "motion/react";
import { useEffect, useMemo, type ReactNode } from "react";
import { Search } from "lucide-react";
import { springPanel } from "./springs";
import { cn } from "./cn";

export interface CommandPaletteItem {
  id: string;
  label: string;
  /** Extra terms for fuzzy search (cmdk matches `value`). */
  keywords?: string[];
  group: string;
  shortcut?: string;
  disabled?: boolean;
  onSelect: () => void;
  icon?: ReactNode;
}

export interface CommandPaletteProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  items: CommandPaletteItem[];
  placeholder?: string;
}

function itemValue(item: CommandPaletteItem) {
  return [item.label, item.group, ...(item.keywords ?? [])].join(" ").toLowerCase();
}

export function CommandPalette({
  open,
  onOpenChange,
  items,
  placeholder = "Search commands…",
}: CommandPaletteProps) {
  const reduce = useReducedMotion();
  const groups = useMemo(() => {
    const order: string[] = [];
    const map = new Map<string, CommandPaletteItem[]>();
    for (const item of items) {
      if (!map.has(item.group)) {
        map.set(item.group, []);
        order.push(item.group);
      }
      map.get(item.group)!.push(item);
    }
    return order.map((name) => ({ name, items: map.get(name)! }));
  }, [items]);

  useEffect(() => {
    if (!open) return;
    const onKey = (event: KeyboardEvent) => {
      if (event.key === "k" && (event.metaKey || event.ctrlKey)) {
        event.preventDefault();
        onOpenChange(false);
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [open, onOpenChange]);

  return (
    <Dialog.Root open={open} onOpenChange={onOpenChange}>
      <Dialog.Portal>
        <Dialog.Overlay className="ui-command-palette__overlay" />
        <Dialog.Content className="ui-command-palette__dialog" aria-describedby={undefined}>
          <Dialog.Title className="visually-hidden">Command palette</Dialog.Title>
          <motion.div
            className="ui-command-palette__motion"
            initial={reduce ? { opacity: 0 } : { opacity: 0, scale: 0.98, y: -6 }}
            animate={{ opacity: 1, scale: 1, y: 0 }}
            exit={reduce ? { opacity: 0 } : { opacity: 0, scale: 0.98, y: -4 }}
            transition={reduce ? { duration: 0 } : springPanel}
          >
            <Command className="ui-command-palette" label="Command palette" loop>
              <motion.div
                className="ui-command-palette__search"
                initial={false}
                animate={{ opacity: 1 }}
                transition={reduce ? { duration: 0 } : springPanel}
              >
                <Search size={16} strokeWidth={1.5} aria-hidden />
                <Command.Input placeholder={placeholder} autoFocus />
              </motion.div>
              <Command.List className="ui-command-palette__list">
                <Command.Empty className="ui-command-palette__empty">No matching commands.</Command.Empty>
                {groups.map((group) => (
                  <Command.Group key={group.name} heading={group.name} className="ui-command-palette__group">
                    {group.items.map((item) => (
                      <Command.Item
                        key={item.id}
                        value={itemValue(item)}
                        disabled={item.disabled}
                        className={cn("ui-command-palette__item")}
                        onSelect={() => {
                          if (item.disabled) return;
                          item.onSelect();
                          onOpenChange(false);
                        }}
                      >
                        {item.icon ? <span className="ui-command-palette__icon">{item.icon}</span> : null}
                        <span className="ui-command-palette__label">{item.label}</span>
                        {item.shortcut ? <kbd className="ui-command-palette__kbd">{item.shortcut}</kbd> : null}
                      </Command.Item>
                    ))}
                  </Command.Group>
                ))}
              </Command.List>
            </Command>
          </motion.div>
        </Dialog.Content>
      </Dialog.Portal>
    </Dialog.Root>
  );
}