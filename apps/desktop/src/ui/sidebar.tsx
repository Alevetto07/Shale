import { AnimatePresence, motion } from "motion/react";
import type { KeyboardEventHandler, ReactNode } from "react";
import { cn } from "./cn";
import { EASE_OUT, springTab } from "./motion";
import { useSidebarChrome } from "./shell-frame";
import { useReducedMotion } from "./use-reduced-motion";

export function Sidebar({
  className,
  children,
  collapsed,
}: {
  className?: string;
  children: ReactNode;
  collapsed?: boolean;
}) {
  const chrome = useSidebarChrome();
  const isCollapsed = chrome?.collapsed ?? collapsed ?? false;
  const settled = chrome?.settled ?? true;
  return (
    <aside
      className={cn("ui-sidebar", className)}
      data-collapsed={isCollapsed ? "1" : "0"}
      data-settled={settled ? "1" : "0"}
      aria-label="Projects"
    >
      {children}
    </aside>
  );
}

export function SidebarHeader({
  title,
  leading,
  actions,
}: {
  title: string;
  leading?: ReactNode;
  actions?: ReactNode;
}) {
  return (
    <div className="ui-sidebar__header">
      {leading}
      <h2 className="ui-sidebar__title ui-sidebar__fade">{title}</h2>
      {actions ? <div className="ui-sidebar__header-actions ui-sidebar__fade">{actions}</div> : null}
    </div>
  );
}

export function SidebarSection({
  label,
  icon,
  count,
  children,
}: {
  label: string;
  icon?: ReactNode;
  count?: number;
  children: ReactNode;
}) {
  return (
    <section className="ui-sidebar__section">
      <h3 className="ui-sidebar__section-label">
        {icon ? <span className="ui-sidebar__section-icon">{icon}</span> : null}
        <span className="ui-sidebar__fade">{label}</span>
        {count != null ? <span className="ui-sidebar__section-count ui-sidebar__fade">{count}</span> : null}
      </h3>
      <div className="ui-sidebar__scroll">{children}</div>
    </section>
  );
}

/** List wrapper so additions and removals ease their siblings instead of jumping. */
export function SidebarRows({ children }: { children: ReactNode }) {
  return (
    <AnimatePresence initial={false} mode="popLayout">
      {children}
    </AnimatePresence>
  );
}

export function SidebarRow({
  title,
  subtitle,
  selected,
  dimmed,
  leading,
  trailing,
  onClick,
  onKeyDown,
  className,
  disabled,
  hint,
  indicatorId,
  recent,
}: {
  title: string;
  subtitle?: string;
  selected?: boolean;
  dimmed?: boolean;
  leading?: ReactNode;
  trailing?: ReactNode;
  onClick?: () => void;
  onKeyDown?: KeyboardEventHandler<HTMLButtonElement>;
  className?: string;
  disabled?: boolean;
  hint?: string;
  /** Shared layout id for a travelling selection bar. One id per list. */
  indicatorId?: string;
  recent?: boolean;
}) {
  const reduce = useReducedMotion();
  return (
    <motion.button
      type="button"
      className={cn("ui-sidebar__row", className)}
      disabled={disabled}
      title={hint}
      data-selected={selected ? "1" : "0"}
      data-dimmed={dimmed ? "1" : "0"}
      data-recent={recent ? "1" : undefined}
      onClick={onClick}
      onKeyDown={onKeyDown}
      layout={reduce ? false : "position"}
      initial={false}
      exit={reduce ? { opacity: 1 } : { opacity: 0, transition: { duration: 0.15, ease: [...EASE_OUT] } }}
    >
      {selected ? (
        reduce || !indicatorId ? (
          <span className="ui-sidebar__active-bar" />
        ) : (
          <motion.span
            className="ui-sidebar__active-bar"
            layoutId={indicatorId}
            transition={springTab}
          />
        )
      ) : null}
      {leading ? <span className="ui-sidebar__row-leading">{leading}</span> : <span className="ui-sidebar__bullet" />}
      <span className="ui-sidebar__row-text ui-sidebar__fade">
        <span className="ui-sidebar__row-title">{title}</span>
        {subtitle ? <span className="ui-sidebar__row-sub">{subtitle}</span> : null}
      </span>
      {trailing ? <span className="ui-sidebar__row-trail">{trailing}</span> : null}
    </motion.button>
  );
}

export function SidebarFooter({ children }: { children: ReactNode }) {
  return <footer className="ui-sidebar__footer">{children}</footer>;
}
