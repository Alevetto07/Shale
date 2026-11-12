import { useCallback, useEffect, useState } from "react";
import { api, type RecentProject } from "./api";

const HIDDEN_KEY = "shale.recents.hidden";

/// The recent-projects list is shared data: the Home screen draws it, but what
/// it contains — and whether it is shown at all — is not a drawing concern.
/// Keeping it here means the panel can be restyled without touching the rules,
/// and the rules can change without touching the panel.
export interface RecentsApi {
  /// What the panel should draw: empty while the panel is hidden.
  shown: RecentProject[];
  /// Every entry, even while hidden, for a menu that wants to say how many.
  all: RecentProject[];
  read: boolean;
  hidden: boolean;
  setHidden: (value: boolean) => void;
  toggle: () => void;
  remove: (path: string) => void;
  refresh: () => void;
}

export function useRecents(): RecentsApi {
  const [all, setAll] = useState<RecentProject[]>([]);
  const [read, setRead] = useState(false);
  const [hidden, setHiddenState] = useState(() => {
    try {
      return window.localStorage.getItem(HIDDEN_KEY) === "1";
    } catch {
      return false;
    }
  });

  const refresh = useCallback(() => {
    void api
      .recents()
      .then((list) => {
        setAll(list);
        setRead(true);
      })
      .catch(() => setRead(true));
  }, []);

  useEffect(() => {
    refresh();
  }, [refresh]);

  const setHidden = useCallback((value: boolean) => {
    setHiddenState(value);
    try {
      window.localStorage.setItem(HIDDEN_KEY, value ? "1" : "0");
    } catch {
      // Storage is a convenience; the choice still holds for this session.
    }
  }, []);

  const remove = useCallback((path: string) => {
    // Optimistic: the row disappears at once, and the backend's answer is the
    // truth that replaces it.
    setAll((list) => list.filter((entry) => entry.path !== path));
    void api
      .forgetRecent(path)
      .then((list) => setAll(list))
      .catch(() => refresh());
  }, [refresh]);

  return {
    shown: hidden ? [] : all,
    all,
    read,
    hidden,
    setHidden,
    toggle: () => setHidden(!hidden),
    remove,
    refresh,
  };
}
