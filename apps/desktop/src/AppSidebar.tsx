import * as DropdownMenu from "@radix-ui/react-dropdown-menu";
import {
  CheckCircle2,
  Clock,
  Globe,
  Inbox,
  Layers,
  MoreHorizontal,
  PanelLeftClose,
  Plus,
  Printer,
  Search,
  Settings,
  SlidersHorizontal,
} from "lucide-react";
import { useState } from "react";
import type { PlateView, PrinterInfo, RecentProject } from "./api";
import {
  Button,
  IconButton,
  Sidebar,
  SidebarFooter,
  SidebarHeader,
  SidebarRow,
  SidebarRows,
  SidebarSection,
} from "./ui";
import { formatRelativeTime } from "./ui/format-time";
import { uiStyleLabel, type UiStyleId } from "./ui-styles";
import { UiStylePicker } from "./UiStylePicker";

export interface DoneSliceRow {
  id: string;
  title: string;
  subtitle: string;
}

export interface AppSidebarProps {
  collapsed: boolean;
  recents: RecentProject[];
  recentsRead: boolean;
  selectedRecentPath?: string | null;
  onNew: () => void;
  onOpen: () => void;
  onImport: () => void;
  onOpenRecent: (path: string) => void;
  onForgetRecent: (path: string) => void;
  plates: PlateView[];
  activePlate?: number;
  onShowPlate?: (index: number) => void;
  printer?: PrinterInfo | null;
  printerLabel?: string;
  onPrinters: () => void;
  onSettings: () => void;
  onAbout: () => void;
  onQuit: () => void;
  onDeviceQueue?: () => void;
  deviceAttention?: boolean;
  uiStyle: UiStyleId;
  onUiStyle: (id: UiStyleId) => void;
  doneSlices: DoneSliceRow[];
  onTogglePrepare?: () => void;
  profileQuality?: string | null;
  profileMaterial?: string | null;
  profileVariant?: string | null;
  onProfiles?: () => void;
  onOpenCommandPalette?: () => void;
}

function recentSubtitle(entry: RecentProject): string {
  const when = entry.missing ? "missing" : formatRelativeTime(entry.saved_at_ms);
  const machine = entry.printer?.trim() || "local";
  if (entry.missing) return when;
  return `${when} · ${machine}`;
}

export function AppSidebar(props: AppSidebarProps) {
  const [styleOpen, setStyleOpen] = useState(false);

  return (
    <Sidebar collapsed={props.collapsed}>
      <SidebarHeader
        title="Projects"
        actions={
          <>
            {props.onOpenCommandPalette ? (
              <IconButton label="Command palette" shortcut="Ctrl K" onClick={props.onOpenCommandPalette}>
                <Search size={16} strokeWidth={1.5} />
              </IconButton>
            ) : null}
            <DropdownMenu.Root>
              <DropdownMenu.Trigger asChild>
                <IconButton label="More actions">
                  <MoreHorizontal size={16} strokeWidth={1.5} />
                </IconButton>
              </DropdownMenu.Trigger>
              <DropdownMenu.Portal>
                <DropdownMenu.Content className="ui-dropdown" side="bottom" align="end" sideOffset={6} collisionPadding={8}>
                  <DropdownMenu.Item className="ui-dropdown__item" onSelect={props.onNew}>
                    New project
                  </DropdownMenu.Item>
                  <DropdownMenu.Item className="ui-dropdown__item" onSelect={props.onOpen}>
                    Open project…
                  </DropdownMenu.Item>
                  <DropdownMenu.Item className="ui-dropdown__item" onSelect={props.onImport}>
                    Import model…
                  </DropdownMenu.Item>
                  <DropdownMenu.Separator className="ui-dropdown__sep" />
                  <DropdownMenu.Item className="ui-dropdown__item" onSelect={props.onQuit}>
                    Quit
                  </DropdownMenu.Item>
                </DropdownMenu.Content>
              </DropdownMenu.Portal>
            </DropdownMenu.Root>
            {props.onDeviceQueue ? (
              <IconButton label="Printer queue" notify={props.deviceAttention} onClick={props.onDeviceQueue}>
                <Inbox size={16} strokeWidth={1.5} />
              </IconButton>
            ) : null}
            {props.onTogglePrepare ? (
              <IconButton label="Toggle prepare panel" onClick={props.onTogglePrepare}>
                <PanelLeftClose size={16} strokeWidth={1.5} />
              </IconButton>
            ) : null}
          </>
        }
      />

      <div className="ui-sidebar__primary">
        <Button variant="pill" size="md" className="ui-btn--block" onClick={props.onNew}>
          <Plus size={16} strokeWidth={1.5} />
          <span className="ui-sidebar__fade">New project</span>
        </Button>
      </div>

      <SidebarSection label="Recent" icon={<Clock size={14} strokeWidth={1.5} />}>
        {!props.recentsRead ? (
          <div className="ui-sidebar__skeleton" aria-hidden>
            <i style={{ width: "72%" }} />
            <i style={{ width: "54%" }} />
            <i style={{ width: "63%" }} />
          </div>
        ) : props.recents.length === 0 ? (
          <p className="ui-sidebar__empty">No recent projects yet.</p>
        ) : (
          <SidebarRows>
            {props.recents.map((entry) => (
              <SidebarRow
                key={entry.path}
                recent
                title={entry.name}
                subtitle={recentSubtitle(entry)}
                dimmed={entry.missing}
                selected={props.selectedRecentPath === entry.path}
                indicatorId="sidebar-recent"
                leading={entry.path.includes("://") ? <Globe size={14} strokeWidth={1.5} /> : undefined}
                onClick={() => props.onOpenRecent(entry.path)}
                onKeyDown={(event) => {
                  if (event.key === "Delete" || event.key === "Backspace") {
                    event.preventDefault();
                    props.onForgetRecent(entry.path);
                  }
                }}
              />
            ))}
          </SidebarRows>
        )}
      </SidebarSection>

      <SidebarSection label="Plates" icon={<Layers size={14} strokeWidth={1.5} />} count={props.plates.length || undefined}>
        {props.plates.length === 0 ? (
          <p className="ui-sidebar__empty">Open a project to see plates.</p>
        ) : (
          <SidebarRows>
            {props.plates.map((plate) => (
              <SidebarRow
                key={plate.index}
                title={plate.name || `Plate ${plate.index + 1}`}
                subtitle={
                  plate.models === 0
                    ? "empty"
                    : plate.models === 1
                      ? "1 part"
                      : `${plate.models} parts`
                }
                selected={props.activePlate === plate.index}
                indicatorId="sidebar-plate"
                onClick={() => props.onShowPlate?.(plate.index)}
              />
            ))}
          </SidebarRows>
        )}
      </SidebarSection>

      <SidebarSection label="Profiles" icon={<SlidersHorizontal size={14} strokeWidth={1.5} />}>
        <SidebarRow
          title={props.profileQuality?.trim() || "Quality preset"}
          subtitle={props.profileMaterial?.trim() || "Material not set"}
          onClick={props.onProfiles ?? props.onSettings}
        />
        {props.profileVariant ? (
          <SidebarRow title="Nozzle variant" subtitle={props.profileVariant} onClick={props.onProfiles ?? props.onSettings} />
        ) : null}
      </SidebarSection>

      <SidebarSection label="Printers" icon={<Printer size={14} strokeWidth={1.5} />}>
        <SidebarRow
          title={props.printer ? props.printer.name : "Choose a printer"}
          subtitle={
            props.printer
              ? `${props.printer.manufacturer}${props.printerLabel ? ` · ${props.printerLabel}` : ""}`
              : "Opens the printer list"
          }
          onClick={props.onPrinters}
        />
      </SidebarSection>

      <SidebarSection label="Done" icon={<CheckCircle2 size={14} strokeWidth={1.5} />}>
        {props.doneSlices.length === 0 ? (
          <p className="ui-sidebar__empty">No slices yet</p>
        ) : (
          props.doneSlices.map((row) => (
            <SidebarRow key={row.id} title={row.title} subtitle={row.subtitle} />
          ))
        )}
      </SidebarSection>

      <SidebarFooter>
        <Button variant="ghost" size="sm" onClick={props.onSettings}>
          <Settings size={16} strokeWidth={1.5} />
          <span className="ui-sidebar__footer-label ui-sidebar__fade">Settings</span>
        </Button>
        <Button variant="ghost" size="sm" onClick={() => setStyleOpen((value) => !value)}>
          <span className="ui-sidebar__footer-label ui-sidebar__fade">{uiStyleLabel(props.uiStyle)}</span>
        </Button>
        <Button variant="ghost" size="sm" onClick={props.onAbout}>
          <span className="ui-sidebar__footer-label ui-sidebar__fade">About</span>
        </Button>
      </SidebarFooter>

      {styleOpen ? (
        <div style={{ padding: "0 var(--space-1)" }}>
          <UiStylePicker value={props.uiStyle} onChange={props.onUiStyle} compact />
        </div>
      ) : null}
    </Sidebar>
  );
}

const SIDEBAR_KEY = "shale.sidebar.collapsed";

export function readSidebarCollapsed(): boolean {
  try {
    return window.localStorage.getItem(SIDEBAR_KEY) === "1";
  } catch {
    return false;
  }
}

export function writeSidebarCollapsed(collapsed: boolean) {
  try {
    window.localStorage.setItem(SIDEBAR_KEY, collapsed ? "1" : "0");
  } catch {
    // Session-only if storage fails.
  }
}