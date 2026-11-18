/**
 * Single source for release assets, checksums, and legal copy on the marketing site.
 * Update `version` and the download entries when shipping a new release.
 */

export const version = "0.1.3";
export const releaseTag = `v${version}`;

export const repoUrl = "https://github.com/Alevetto07/Shale";

export const slogan = "The last slicer you'll ever download";

export const shaleLicense = "AGPL-3.0-or-later";

export type DownloadAsset = {
  id: string;
  title: string;
  description: string;
  fileName: string;
  /** Empty until filled from the release's SHA256SUMS.txt */
  sizeLabel: string;
  sizeBytes: number;
  /** Empty string = link to SHA256SUMS.txt instead of printing a hash */
  sha256: string;
  primary?: boolean;
};

const releaseBase = `${repoUrl}/releases/download/${releaseTag}`;

export const downloads: DownloadAsset[] = [
  {
    id: "windows-desktop",
    title: "Windows desktop",
    description: "Graphical slicer with the full Prepare, Preview and Device workspaces.",
    fileName: `Shale_${version}_x64-setup.exe`,
    sizeLabel: "5.1 MB",
    sizeBytes: 5324305,
    sha256:
      "9c6bd26a3223fd9dd08fe393af3ac30e6d9c04100152841edb018a420fd6b28d",
    primary: true,
  },
  {
    id: "windows-cli",
    title: "Windows CLI + MCP",
    description: "Headless `shale-cli` and `shale-mcp` for terminals and agents.",
    fileName: `shale-${version}-windows-x64.zip`,
    sizeLabel: "5.0 MB",
    sizeBytes: 5243592,
    sha256:
      "cd73ac06c0d4db860d820c865ec4f4d274312ffeb2898ea249e028adc9447e8c",
  },
  {
    id: "linux-cli",
    title: "Linux CLI + MCP",
    description: "Same headless tools for x64 Linux — ideal for a small board.",
    fileName: `shale-${version}-linux-x64.tar.gz`,
    sizeLabel: "4.7 MB",
    sizeBytes: 4909746,
    sha256:
      "edf83fcb7fff89421da2b5aa0440d943bbe22a9ba4af3dd057ca1eb858c5c6d1",
  },
];

export function downloadUrl(fileName: string): string {
  return `${releaseBase}/${fileName}`;
}

export const checksumsUrl = downloadUrl("SHA256SUMS.txt");
export const releasePageUrl = `${repoUrl}/releases/tag/${releaseTag}`;

export const primaryDownload =
  downloads.find((d) => d.primary) ?? downloads[0];

export const mcpEnvExample = {
  SHALE_CURAENGINE: "/usr/bin/CuraEngine",
  SHALE_CURA_DIR: "/usr/share/cura/resources",
  SHALE_MATERIALS_DIR: "/usr/share/cura/resources/materials",
} as const;

export const underTheHood =
  "Unmodified CuraEngine · Tauri 2 · WebGL plate · Bambu-only printer list";

/**
 * Cold-start timings to the main, usable window (seconds unless noted).
 * Warm = repeat launch on the same machine; cold = uncached / first open.
 */
export const launchBenchmarks = {
  shaleFrameMs: 50,
  shaleUsableWarmSeconds: 0.8,
  shaleUsableColdSeconds: 1.0,
  shaleUsableWarmLabel: "~0.8 s",
  shaleUsableColdLabel: "~1.0 s",
  competitors: ["OrcaSlicer", "Bambu Studio", "Cura"] as const,
  competitorSecondsMin: 7,
  competitorSecondsMax: 11,
  competitorRangeLabel: "7–11 s",
} as const;

export function launchComparisonSentence(): string {
  const names = launchBenchmarks.competitors.join(", ");
  return `Launch to a usable workspace in ${launchBenchmarks.shaleUsableWarmLabel} warm or ${launchBenchmarks.shaleUsableColdLabel} on a cold start — ${names} measured ${launchBenchmarks.competitorRangeLabel} to their main window.`;
}

export function launchStatLine(): string {
  return `${launchBenchmarks.shaleUsableWarmLabel} warm · ${launchBenchmarks.shaleUsableColdLabel} cold. Other slicers: ${launchBenchmarks.competitorRangeLabel}.`;
}

export type FeatureTile =
  | {
      id: string;
      kind: "launch";
      title: string;
      body: string;
    }
  | {
      id: string;
      kind: "visual";
      title: string;
      body: string;
      image: string;
      imageAlt: string;
    }
  | {
      id: string;
      kind: "mini";
      title: string;
      body: string;
      mini: "cli" | "shale-file";
    };

/** Fixed 3×2 grid — each tile carries a visual, stat, or mini UI. */
export const featureTiles: FeatureTile[] = [
  {
    id: "fast",
    kind: "launch",
    title: "Usable workspace",
    body: "Launch to the main window on this build — warm repeat vs uncached first open.",
  },
  {
    id: "account",
    kind: "visual",
    title: "Your Bambu account",
    body: "Sign in and manage printers from Home — the same flow as Bambu Studio.",
    image: "/media/device-lan-1920x1080.png",
    imageAlt: "Device workspace with a live LAN printer",
  },
  {
    id: "presets",
    kind: "visual",
    title: "Studio presets",
    body: "Machines, processes and filaments from Bambu Studio profiles — not a shortcut list.",
    image: "/media/prepare-1920x1080.png",
    imageAlt: "Prepare workspace with Bambu process settings",
  },
  {
    id: "send",
    kind: "visual",
    title: "Send when you are ready",
    body: "LAN or cloud after sign-in. Shale never starts a print until you press send.",
    image: "/media/preview-1920x1080.png",
    imageAlt: "Preview workspace with layer toolpaths",
  },
  {
    id: "headless",
    kind: "mini",
    title: "Script it or agent it",
    body: "`shale-cli` in a terminal, `shale-mcp` for agents — same core on desktop or a small board.",
    mini: "cli",
  },
  {
    id: "projects",
    kind: "mini",
    title: "Projects that stay yours",
    body: "Plates, settings and layouts in `.shale` files you can back up and reopen.",
    mini: "shale-file",
  },
];

/** @deprecated Use featureTiles — kept for typing in older imports */
export const features = featureTiles;

export const comingNext = [
  {
    title: "Non-planar curved-top slicing",
    status: "Planned",
    body: "Slicing that follows curved tops instead of flat layers only.",
  },
  {
    title: "Automatic printed-part removal",
    status: "Planned",
    body: "Camera-guided sequences to lift a finished part off the bed.",
  },
  {
    title: "Remote failure detection",
    status: "Planned",
    body: "Watching a job off-machine and surfacing stalls or errors early.",
  },
] as const;

export const mediaSlots = [
  {
    id: "prepare",
    kind: "screenshot" as const,
    alt: "Shale Prepare workspace with a model on the build plate",
    src: "/media/prepare-1920x1080.png",
    caption: "Prepare — model on the plate with Bambu process settings.",
    layout: "hero" as const,
  },
  {
    id: "preview",
    kind: "screenshot" as const,
    alt: "Layer-by-layer toolpath preview after slicing",
    src: "/media/preview-1920x1080.png",
    caption: "Preview — scrub layers and toggle line types.",
    layout: "tall" as const,
  },
  {
    id: "device-lan",
    kind: "screenshot" as const,
    alt: "Device workspace showing a live LAN printer link",
    src: "/media/device-lan-1920x1080.png",
    caption: "Device — live state and send on the LAN.",
    layout: "normal" as const,
  },
  {
    id: "promo",
    kind: "video" as const,
    alt: "Promotional video placeholder",
    src: "",
    caption: "Short promo reel — slot reserved in public/media.",
    layout: "normal" as const,
  },
] as const;