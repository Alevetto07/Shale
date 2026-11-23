import { describe, expect, it } from "vitest";
import { formatRelativeTime } from "./format-time";

describe("formatRelativeTime", () => {
  const now = Date.UTC(2026, 10, 23, 12, 0, 0);

  it("uses British-friendly phrases", () => {
    expect(formatRelativeTime(now - 86_400_000, now)).toBe("yesterday");
    expect(formatRelativeTime(now - 8 * 86_400_000, now)).toBe("8 days ago");
    expect(formatRelativeTime(now - 40 * 86_400_000, now)).toBe("last month");
  });
});