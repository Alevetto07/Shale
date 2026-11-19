export const FEATURE_COLORS: Record<string, string> = {
  "wall-outer": "#f3f0e8",
  "wall-inner": "#cbc4b8",
  skin: "#a8d4ff",
  infill: "__PATH__",
  support: "#d4a72c",
  travel: "#5a6578",
  skirt: "#3d4654",
  other: "#8891a0",
};

export const FEATURE_LABELS: Record<string, string> = {
  "wall-outer": "Outer wall",
  "wall-inner": "Inner wall",
  skin: "Skin",
  infill: "Infill",
  support: "Support",
  travel: "Travel",
};

export function featureColor(type: string): string {
  if (type === "infill") {
    return (
      getComputedStyle(document.documentElement).getPropertyValue("--path").trim() || "#9eb8ff"
    );
  }
  return FEATURE_COLORS[type] ?? FEATURE_COLORS.other;
}