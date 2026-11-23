import { ChevronLeft, ChevronRight, PanelLeft } from "lucide-react";
import { IconButton } from "./ui";

export function TitleBarNav({
  sidebarCollapsed,
  onToggleSidebar,
  canBack,
  canForward,
  onBack,
  onForward,
}: {
  sidebarCollapsed: boolean;
  onToggleSidebar: () => void;
  canBack: boolean;
  canForward: boolean;
  onBack: () => void;
  onForward: () => void;
}) {
  return (
    <div className="titlebar-nav">
      <IconButton
        label={sidebarCollapsed ? "Expand sidebar" : "Collapse sidebar"}
        onClick={onToggleSidebar}
      >
        <PanelLeft size={15} strokeWidth={1.5} />
      </IconButton>
      <IconButton label="Back" shortcut="Alt ←" disabled={!canBack} onClick={onBack}>
        <ChevronLeft size={15} strokeWidth={1.5} />
      </IconButton>
      <IconButton label="Forward" shortcut="Alt →" disabled={!canForward} onClick={onForward}>
        <ChevronRight size={15} strokeWidth={1.5} />
      </IconButton>
    </div>
  );
}