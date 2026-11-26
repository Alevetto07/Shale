/** What the printer was doing the last time we looked. */
export interface NoticeSnapshot {
  state: string;
  /** HMS codes that mean a spool has run out. */
  runout: string[];
}

export interface PrintNotice {
  kind: "finished" | "failed" | "paused" | "runout";
  title: string;
  body: string;
}

/**
 * Desktop notifications for a change in the printer's report.
 *
 * The first reading is silent: opening the pane must not announce a print that
 * already finished. A pause that arrives with a new run-out is the run-out,
 * not a second "paused". Turning notifications off yields nothing, and the
 * caller still keeps the snapshot so switching them back on does not replay it.
 */
export function printNotices(
  previous: NoticeSnapshot | null,
  next: NoticeSnapshot,
  file: string,
  enabled: boolean,
): PrintNotice[] {
  if (!enabled || previous == null) return [];
  const name = file.trim() || "The print";
  const notices: PrintNotice[] = [];
  const freshRunout = next.runout.filter((code) => !previous.runout.includes(code));
  if (freshRunout.length > 0) {
    notices.push({
      kind: "runout",
      title: "Filament has run out",
      body: `${name} needs a new spool.`,
    });
  }
  const from = previous.state;
  const to = next.state;
  if (
    to === "FINISH" &&
    from !== "FINISH" &&
    (from === "RUNNING" || from === "PAUSE" || from === "PREPARE")
  ) {
    notices.push({
      kind: "finished",
      title: "Print finished",
      body: `${name} has finished.`,
    });
  } else if (to === "FAILED" && from !== "FAILED") {
    notices.push({
      kind: "failed",
      title: "Print failed",
      body: `${name} failed.`,
    });
  } else if (
    to === "PAUSE" &&
    from !== "PAUSE" &&
    freshRunout.length === 0 &&
    (from === "RUNNING" || from === "PREPARE")
  ) {
    notices.push({
      kind: "paused",
      title: "Print paused",
      body: `${name} is paused.`,
    });
  }
  return notices;
}
