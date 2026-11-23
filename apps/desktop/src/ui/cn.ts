import clsx, { type ClassValue } from "clsx";

/** Join conditional class names (shadcn-style, without Tailwind). */
export function cn(...parts: ClassValue[]): string {
  return clsx(parts);
}