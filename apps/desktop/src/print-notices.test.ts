import { describe, expect, it } from "vitest";
import { printNotices, type NoticeSnapshot } from "./print-notices";

const idle: NoticeSnapshot = { state: "IDLE", runout: [] };
const running: NoticeSnapshot = { state: "RUNNING", runout: [] };

describe("printNotices", () => {
  it("stays quiet on the first report and when notifications are off", () => {
    expect(printNotices(null, { state: "FINISH", runout: [] }, "benchy", true)).toEqual([]);
    expect(printNotices(running, { state: "FINISH", runout: [] }, "benchy", false)).toEqual([]);
  });

  it("announces a finish, a failure, and a pause", () => {
    expect(printNotices(running, { state: "FINISH", runout: [] }, "benchy", true)).toEqual([
      { kind: "finished", title: "Print finished", body: "benchy has finished." },
    ]);
    expect(printNotices(running, { state: "FAILED", runout: [] }, "", true)[0]).toMatchObject({
      kind: "failed",
      body: "The print failed.",
    });
    expect(printNotices(running, { state: "PAUSE", runout: [] }, "benchy", true)[0]?.kind).toBe(
      "paused",
    );
    expect(printNotices(idle, { state: "PAUSE", runout: [] }, "benchy", true)).toEqual([]);
  });

  it("prefers a filament run-out over a pause that arrived with it", () => {
    const paused: NoticeSnapshot = { state: "PAUSE", runout: ["0700-2000-0002-0001"] };
    expect(printNotices(running, paused, "benchy", true)).toEqual([
      { kind: "runout", title: "Filament has run out", body: "benchy needs a new spool." },
    ]);
    expect(printNotices(paused, paused, "benchy", true)).toEqual([]);
  });
});
