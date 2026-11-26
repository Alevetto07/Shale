/** Short stage name for the live slice bar. The engine's own sentence stays in the tooltip. */
export function sliceStage(message: string): string {
  const text = message.trim().toLowerCase();
  if (!text) return "Preparing";
  if (text.includes("curv") || text.includes("nonplanar") || text.includes("non-planar")) {
    return "Curving layers";
  }
  if (text.includes("resolv") || text.includes("profile") || text.includes("setting")) return "Resolving the profile";
  if (text.includes("support")) return "Building supports";
  if (text.includes("mesh") || text.includes("model") || text.includes("load")) return "Reading the model";
  if (text.includes("layer") || text.includes("slice") || text.includes("gcode") || text.includes("toolpath")) {
    return "Slicing layers";
  }
  if (text.includes("start")) return "Starting";
  if (text.includes("done") || text.includes("finish") || text.includes("layer")) return "Finishing";
  return message.trim();
}
