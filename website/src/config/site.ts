/**
 * Single source for release assets, checksums, and legal copy on the marketing site.
 * Update `version` and the download entries when shipping a new release.
 */

export const version = "0.1.0";
export const releaseTag = `v${version}`;

export const repoUrl = "https://github.com/Alevetto07/Shale";

export const slogan = "The last slicer you'll ever download";

export const shaleLicense = "TBD";

export type DownloadAsset = {
  id: string;
  title: string;
  description: string;
  fileName: string;
  /** Placeholder until the release is cut */
  sizeLabel: string;
  sizeBytes: number;
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
    sizeLabel: "TBD",
    sizeBytes: 0,
    sha256:
      "0000000000000000000000000000000000000000000000000000000000000000",
    primary: true,
  },
  {
    id: "windows-cli",
    title: "Windows CLI + MCP",
    description: "Headless `shale-cli` and `shale-mcp` for terminals and agents.",
    fileName: `shale-${version}-windows-x64.zip`,
    sizeLabel: "TBD",
    sizeBytes: 0,
    sha256:
      "0000000000000000000000000000000000000000000000000000000000000000",
  },
  {
    id: "linux-cli",
    title: "Linux CLI + MCP",
    description: "Same headless tools for x64 Linux — ideal for a small board.",
    fileName: `shale-${version}-linux-x64.tar.gz`,
    sizeLabel: "TBD",
    sizeBytes: 0,
    sha256:
      "0000000000000000000000000000000000000000000000000000000000000000",
  },
];

export function downloadUrl(fileName: string): string {
  return `${releaseBase}/${fileName}`;
}

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
 * Re-measure on your machine and update these before the next site release.
 */
export const launchBenchmarks = {
  /** First paint of the window frame — not marketed on the site */
  shaleFrameMs: 50,
  /** Measured time until the UI is ready to use */
  shaleUsableSeconds: 1.7,
  shaleUsableRounded: "about two seconds",
  competitors: ["OrcaSlicer", "Bambu Studio", "Cura"] as const,
  competitorSecondsMin: 7,
  competitorSecondsMax: 11,
  competitorRangeLabel: "7–11 s",
} as const;

export function launchComparisonSentence(): string {
  const names = launchBenchmarks.competitors.join(", ");
  return `Opens in ${launchBenchmarks.shaleUsableRounded}, while ${names} measured ${launchBenchmarks.competitorRangeLabel} to their main window.`;
}

export const features = [
  {
    title: "Your Bambu account, built in",
    icon: "┌◉┐",
    body: "Sign in, see the printers on your account, and manage them from Home — the same workflow you know from Bambu Studio, without switching apps.",
  },
  {
    title: "Send when you are ready",
    icon: "╱▔╲",
    body: "Push a sliced plate over the LAN or through the cloud after sign-in. Shale never starts a print until you press send.",
  },
  {
    title: "Studio presets, day one",
    icon: "▤▥",
    body: "Machines, processes and filaments come from Bambu Studio's profiles — not a hand-rolled shortcut list.",
  },
  {
    title: "Ready in about two seconds",
    icon: "»»",
    body: launchComparisonSentence(),
  },
  {
    title: "Script it or agent it",
    icon: ">_",
    body: "The same slicer core runs headless: `shale-cli` in a terminal, `shale-mcp` for AI agents — on a full desktop or a small Linux board.",
  },
  {
    title: "Projects that stay yours",
    icon: "▣",
    body: "Plates, settings and layouts live in `.shale` files you can back up, share and reopen exactly as you left them.",
  },
] as const;

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
  },
  {
    id: "preview",
    kind: "screenshot" as const,
    alt: "Layer-by-layer toolpath preview after slicing",
    src: "/media/preview-1920x1080.png",
    caption: "Preview — scrub layers and toggle line types.",
  },
  {
    id: "device-lan",
    kind: "screenshot" as const,
    alt: "Device workspace showing a live LAN printer link",
    src: "/media/device-lan-1920x1080.png",
    caption: "Device — live state and send on the LAN.",
  },
  {
    id: "promo",
    kind: "video" as const,
    alt: "Promotional video placeholder",
    src: "",
    caption: "Short promo reel — slot reserved in public/media.",
  },
] as const;