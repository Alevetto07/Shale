/** Which plates a "slice all" pass should visit, in plate order. */

export interface SliceTarget {
  index: number;
  name: string;
  models: number;
  sliced?: boolean;
}

/** Plates with something on them. `onlyUnsliced` skips a plate that already has G-code. */
export function platesToSlice(plates: SliceTarget[], onlyUnsliced: boolean): SliceTarget[] {
  return plates.filter((plate) => plate.models > 0 && (!onlyUnsliced || plate.sliced !== true));
}

/** `plateFraction` is this plate's own 0–1 progress. The result covers the whole batch. */
export function batchFraction(done: number, total: number, plateFraction: number): number {
  if (total <= 0) return 0;
  const span = 1 / total;
  const fraction = done * span + Math.min(1, Math.max(0, plateFraction)) * span;
  return Math.min(1, Math.max(0, fraction));
}

/** The live bar names the plate, then whatever the engine just said. */
export function batchMessage(done: number, total: number, name: string, detail: string): string {
  const head = `Plate ${done + 1} of ${total}: ${name}`;
  const tail = detail.trim();
  return tail ? `${head} — ${tail}` : head;
}
