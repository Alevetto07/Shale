import type { Positions } from "./api";

export interface FramableModel {
  positions: Positions;
  selected?: boolean;
}

/** Whether the viewport should aim the camera at loaded geometry instead of the bed. */
export function shouldFrameModels(models: FramableModel[]): boolean {
  return models.some((model) => model.positions.length > 0);
}

/** Parts to include in a model frame (selected part, or every part with geometry). */
export function modelsToFrame(models: FramableModel[]): FramableModel[] {
  const chosen = models.find((model) => model.selected && model.positions.length > 0);
  if (chosen) return [chosen];
  return models.filter((model) => model.positions.length > 0);
}

/** Bed framing waits until the machine profile has a real build volume. */
export function canFrameBed(width: number, depth: number): boolean {
  return width > 0 && depth > 0;
}