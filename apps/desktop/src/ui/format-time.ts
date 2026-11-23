/** Relative time in British English for sidebar subtitles. */
export function formatRelativeTime(ms: number, now = Date.now()): string {
  if (!ms) return "";
  const diff = now - ms;
  const day = 86_400_000;
  if (diff < day) return "today";
  if (diff < 2 * day) return "yesterday";
  if (diff < 14 * day) return `${Math.floor(diff / day)} days ago`;
  if (diff < 45 * day) return "last month";
  if (diff < 365 * day) return `${Math.floor(diff / (30 * day))} months ago`;
  return `${Math.floor(diff / (365 * day))} years ago`;
}