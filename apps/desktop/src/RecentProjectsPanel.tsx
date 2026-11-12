import {
  forwardRef,
  useEffect,
  useImperativeHandle,
  useRef,
  useState,
  type KeyboardEvent,
} from "react";
import type { RecentProject } from "./api";
import { theme } from "./theme";

const HIDDEN_KEY = "shale.start.recentsHidden";
const RULE_W = 26;

export type RecentProjectsPanelHandle = {
  reveal: () => void;
  focusFirst: () => void;
};

function readHidden(): boolean {
  try {
    return window.localStorage.getItem(HIDDEN_KEY) === "1";
  } catch {
    return false;
  }
}

function writeHidden(hidden: boolean) {
  try {
    if (hidden) window.localStorage.setItem(HIDDEN_KEY, "1");
    else window.localStorage.removeItem(HIDDEN_KEY);
  } catch {
    // Storage can be missing; the panel still toggles for this session.
  }
}

function shortMachine(name: string): string {
  const first = name.split("/")[0].trim();
  const words = first.split(/\s+/).filter(Boolean);
  if (words.length < 2) return first || name;
  const vendorish = /3d|ultimaker|prusa/i.test(words[0]);
  const rest = words.slice(1).join(" ");
  return vendorish && rest ? rest : first;
}

function formatWhen(ms: number): string {
  if (!ms) return "";
  return new Date(ms).toLocaleDateString("en-GB", {
    day: "numeric",
    month: "short",
  });
}

function clip(text: string, max: number): string {
  const trimmed = text.trim();
  if (trimmed.length <= max) return trimmed;
  return `${trimmed.slice(0, Math.max(0, max - 1))}…`;
}

export const RecentProjectsPanel = forwardRef<
  RecentProjectsPanelHandle,
  {
    recents: RecentProject[];
    recentsRead: boolean;
    onOpen: (path: string) => void;
    onForget: (path: string) => void;
  }
>(function RecentProjectsPanel({ recents, recentsRead, onOpen, onForget }, ref) {
  const [hidden, setHidden] = useState(() => readHidden());
  const rowRefs = useRef<Array<HTMLButtonElement | null>>([]);
  const shown = recents.slice(0, 5);
  const rule = theme.box.h.repeat(RULE_W);

  useImperativeHandle(ref, () => ({
    reveal() {
      if (hidden) {
        setHidden(false);
        writeHidden(false);
      }
    },
    focusFirst() {
      if (hidden) {
        setHidden(false);
        writeHidden(false);
      }
      window.requestAnimationFrame(() => rowRefs.current[0]?.focus());
    },
  }));

  useEffect(() => {
    rowRefs.current.length = shown.length;
  }, [shown.length]);

  function toggleHidden() {
    const next = !hidden;
    setHidden(next);
    writeHidden(next);
  }

  function onRowKey(event: KeyboardEvent<HTMLButtonElement>, path: string) {
    if (event.key === "Backspace" || event.key === "Delete") {
      event.preventDefault();
      onForget(path);
    }
  }

  if (hidden) {
    return (
      <div className="start-recents start-recents--collapsed">
        <button type="button" className="start-recents-tab" onClick={toggleHidden} aria-expanded={false}>
          <span aria-hidden="true">
            {theme.box.bl}
            {theme.box.h}
          </span>
          <span className="start-recents-tab-label">recent</span>
          <span aria-hidden="true">▸</span>
          <span aria-hidden="true">
            {theme.box.h}
            {theme.box.br}
          </span>
        </button>
      </div>
    );
  }

  return (
    <aside className="start-recents" aria-label="Recent projects">
      <div className="start-recents-rule" aria-hidden="true">
        <span>{theme.box.tl}</span>
        <span className="rule">{rule}</span>
        <span>{theme.box.tr}</span>
      </div>
      <div className="start-recents-head">
        <span className="start-recents-edge" aria-hidden="true">
          {theme.box.v}
        </span>
        <div className="start-recents-title">
          <span>recent</span>
          <button type="button" className="start-recents-hide" onClick={toggleHidden} aria-expanded={true}>
            hide
          </button>
        </div>
        <span className="start-recents-edge" aria-hidden="true">
          {theme.box.v}
        </span>
      </div>
      <div className="start-recents-body">
        <span className="start-recents-edge" aria-hidden="true">
          {theme.box.v}
        </span>
        <div className="start-recents-list">
          {!recentsRead && <p className="start-recents-empty">reading…</p>}
          {recentsRead && shown.length === 0 && <p className="start-recents-empty">none yet</p>}
          {shown.map((recent, index) => (
            <div key={recent.path} className="start-recents-row">
              <button
                type="button"
                ref={(node) => {
                  rowRefs.current[index] = node;
                }}
                className={recent.missing ? "recent-row dim" : "recent-row"}
                onClick={() => onOpen(recent.path)}
                onKeyDown={(event) => onRowKey(event, recent.path)}
              >
                <span className="recent-name">{clip(recent.name, 18)}</span>
                <span className="recent-meta">
                  {recent.missing ? "missing" : formatWhen(recent.saved_at_ms)}
                  {!recent.missing && recent.printer ? (
                    <>
                      <span className="start-dot" aria-hidden="true">
                        ·
                      </span>
                      <span className="recent-machine">{clip(shortMachine(recent.printer), 12)}</span>
                    </>
                  ) : null}
                </span>
              </button>
              <button
                type="button"
                className="recent-forget"
                aria-label={`Remove ${recent.name} from recents`}
                onClick={(event) => {
                  event.stopPropagation();
                  onForget(recent.path);
                }}
              >
                rm
              </button>
            </div>
          ))}
        </div>
        <span className="start-recents-edge" aria-hidden="true">
          {theme.box.v}
        </span>
      </div>
      <div className="start-recents-rule" aria-hidden="true">
        <span>{theme.box.bl}</span>
        <span className="rule">{rule}</span>
        <span>{theme.box.br}</span>
      </div>
    </aside>
  );
});
