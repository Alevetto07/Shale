import type { ReactNode } from "react";

export function DemoFrame({
  title,
  note,
  children,
}: {
  title: string;
  note?: string;
  children: ReactNode;
}) {
  return (
    <div className="demo-frame">
      <div className="demo-frame-bar">
        <span className="demo-frame-title">{title}</span>
        {note && <span className="demo-frame-note">{note}</span>}
      </div>
      <div className="demo-frame-body">{children}</div>
    </div>
  );
}