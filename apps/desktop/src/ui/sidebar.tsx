import type { KeyboardEventHandler, ReactNode, ButtonHTMLAttributes } from "react";
import { cn } from "./cn";

export function Sidebar({ className, children, collapsed }: { className?: string; children: ReactNode; collapsed?: boolean }) {
  return (
    <aside
      className={cn("ui-sidebar", className)}
      data-collapsed={collapsed ? "1" : "0"}
      aria-label="Projects"
    >
      {children}
    </aside>
  );
}

export function SidebarHeader({ title, actions }: { title: string; actions?: ReactNode }) {
  return (
    <div className="ui-sidebar__header">
      <h2 className="ui-sidebar__title">{title}</h2>
      {actions ? <div className="ui-sidebar__header-actions">{actions}</div> : null}
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
        <span>{label}</span>
        {count != null ? <span className="ui-sidebar__section-count">{count}</span> : null}
      </h3>
      <SnapshotScrollArea>{children}</SnapshotScrollArea>
    </section>
  );
}

function SnapshotScrollArea({ children }: { children: ReactNode }) {
  return <div className="ui-sidebar__scroll">{children}</div>;
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
  ...rest
}: {
  title: string;
  subtitle?: string;
  selected?: boolean;
  dimmed?: boolean;
  leading?: ReactNode;
  trailing?: ReactNode;
  onClick?: () => void;
  onKeyDown?: KeyboardEventHandler<HTMLButtonElement>;
} & Omit<ButtonHTMLAttributes<HTMLButtonElement>, "onClick">) {
  return (
    <button
      type="button"
      className="ui-sidebar__row"
      data-selected={selected ? "1" : "0"}
      data-dimmed={dimmed ? "1" : "0"}
      onClick={onClick}
      onKeyDown={onKeyDown}
      {...rest}
    >
      {leading ? <span className="ui-sidebar__row-leading">{leading}</span> : <span className="ui-sidebar__bullet" />}
      <span className="ui-sidebar__row-text">
        <span className="ui-sidebar__row-title">{title}</span>
        {subtitle ? <span className="ui-sidebar__row-sub">{subtitle}</span> : null}
      </span>
      {trailing ? <span className="ui-sidebar__row-trail">{trailing}</span> : null}
    </button>
  );
}

export function SidebarFooter({ children }: { children: ReactNode }) {
  return <footer className="ui-sidebar__footer">{children}</footer>;
}