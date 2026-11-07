# PR #8 — what this stack adds, and what it does not

Branch `cursor/deep-field-75ee`. Nothing here is merged, nothing is pushed from
this machine (no git credentials), and no release is built. This note is the
handover for the reviewer.

## The shape of it

Bambu Studio's shell, on top of a CuraEngine slice:

- **A top bar with document tabs** — several projects open at once, each with its
  own plate, and a parked tab keeps its name.
- **Four workspaces** — Home, Prepare, Preview, Device — as one segmented
  control in the same bar.
- **The settings sheet is Cura's whole catalogue**, searchable (`ironing` finds
  six rows), with per-node help text, a reset per field, and every change on the
  undo log.
- **A printer host** (`crates/shale-host`): MQTT over TLS 8883 to
  `device/<serial>/request`, implicit-FTPS 990 upload, then `project_file`. It
  speaks to a fake printer in `scratch/` on this machine; it has not been tried
  against hardware, and the printer is not on this network.
- **Paint** for support enforcement and support blocking — patches and a
  free-hand brush — plus **plane cuts with pins**, a **job queue**, **per-spool
  filament metres**, **per-object settings**, **variable layer height**,
  **auto-arrange**, **fill plate**, **17 solids**, **multi-plate**, **3MF in and
  out**, **temperature and flow calibration**.

## What the last pass fixed, because a slicer that lies is worse than one that
## lacks a feature

- **The job card described a plate it was not about.** It read `LAYERS 400`
  beside a dash for time and filament, from a slice of a cube that had left the
  plate an hour earlier. A slice now carries the ids of the parts it was made
  from, the shell only shows its numbers when those parts are on the plate, and
  a part arriving, leaving or moving puts the card back to `NOT SLICED`.
- **The printer frame read `NOZZLE none` and `QUALITY none`** while the resolver
  sliced at 0.40 mm and the machine's standard quality. A remembered profile was
  restored wholesale, and a null written into it once — by a window that saved
  before the catalogue had loaded — won on every start after. A remembered value
  now has to be one the machine offers.
- **The Device page was one column of twelve sections**, leaving two-thirds of a
  wide screen empty and the spools below the fold. It is two columns now.

## Verification

- `cargo test --workspace --locked`: **123 passed, 0 failed**.
- `npx tsc --noEmit` clean; `npm run build` clean; `cargo build -p
  shale-desktop --locked` clean.
- Brush, end to end through the real UI: one drag across a 60 mm sphere painted
  **149 faces / 2205 mm²**; the slice went from **0** to **587** `;TYPE:SUPPORT`
  lines and back to 0 when the paint was cleared.
- Pins: a 20 mm cube cut with pins measures **1880 triangles / 7967.74 mm³**,
  bbox 20 × 20 × 15 — two halves that hold together.
- Queue: advances on its own (`1.gcode` 30 % → `2.gcode` 24 % → idle); on
  `FAILED` it stops and the second job stays queued.
- Materials: choosing one changes the engine's nozzle and bed temperature, and
  the difference shows in the G-code.

## Screenshots and video

`C:\Users\Alessio\Documents\shale-demo\` — `shale-tour.mp4` (41 s, 2560 × 1558)
and the frames pulled from it every four seconds.

The video is a **page screencast**, not a screen grab: `gdigrab` by window title
comes back black on a WebView2 window (the surface is composited by the GPU) and
the desktop capture drags the taskbar and whatever notification card Windows has
decided to show into the frame.

## The theme, and what is not themed

The app is dark throughout: no light-theme leftovers in `styles.css`, none in
`Viewport.tsx`, and the toolpath colours are chosen to stay distinguishable

  - travels dimmed and thin, walls and infill and skin in separate hues,
  - the same colours again when the preview is coloured by speed.

**Not themed, and cannot be:** the OS file dialogs (Open, Save As) and the
window's title bar and frame belong to Windows. They will be light on a light
system theme whatever the app does.

## What is still missing, plainly

- **Seam painting** — Bambu paints where the z-seam goes; Shale cannot, because
  CuraEngine has no per-face seam input. `z_seam_type` is a setting, not a brush.
- **Per-filament flush volumes** — the AMS volumes are not exposed as a control.
- **Ear-shaped cuts** — the cut is a plane, plus pins. `csg::subtract` exists but
  no UI path cuts part A with part B.
- **Camera** — not implemented. `SHALE_PRINT_REMOVAL` stays off, sends no motion,
  and `will_move_printer` is false.
- **Bambu-flavoured G-code** — the output is Marlin-flavoured, which the A1
  accepts over the same `project_file` path, but it is not Bambu's own flavour.
- **Multi-spool send** — correctly refused rather than silently wrong.
- **The video is not in the repo.** It is 1.6 MB of screen recording; it belongs
  in the PR description, not in a git object.
