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

/** How many plates to slice at once. Missing or nonsense values start at 2, and the cap is 4. */
export function sliceAtOnce(requested: number | null): number {
  if (requested == null || !Number.isFinite(requested)) return 2;
  return Math.min(4, Math.max(1, Math.round(requested)));
}

/** One line for the plates that are slicing now. Each plate keeps its own fraction. */
export function runningSummary(plates: { name: string; fraction: number; detail: string }[]): string {
  return plates
    .map((plate) => {
      const percent = Math.round(Math.min(1, Math.max(0, plate.fraction)) * 100);
      const tail = plate.detail.trim();
      return tail ? `${plate.name} ${percent}% — ${tail}` : `${plate.name} ${percent}%`;
    })
    .join(" · ");
}

/** The bar is the average of the plates in this batch, including ones still waiting. */
export function parallelFraction(fractions: number[]): number {
  if (fractions.length === 0) return 0;
  const sum = fractions.reduce((total, fraction) => total + Math.min(1, Math.max(0, fraction)), 0);
  return sum / fractions.length;
}

/** The live bar names the plate, then whatever the engine just said. */
export function batchMessage(done: number, total: number, name: string, detail: string): string {
  const head = `Plate ${done + 1} of ${total}: ${name}`;
  const tail = detail.trim();
  return tail ? `${head} — ${tail}` : head;
}
