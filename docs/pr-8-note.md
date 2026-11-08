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
- **A pause or a filament change at the layer in hand**, from the preview's own
  control row (`pause here`, `swap here`) — the instruction machinery was there,
  but only behind a sidebar frame nobody opens while scrubbing. The next slice
  carries `; SHALE pause at layer 30` then `M0 Paused by Shale`, or
  `; SHALE filament change at layer 19` then `M600 ; Filament change`.

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
- Marks, end to end through the preview: `swap here` at layer 20 then `pause
  here` at layer 31 → one `M600` at line 14314 and one `M0` at 15712, each with
  its `; SHALE` comment, and the file otherwise unchanged (+92 bytes for one
  mark).
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
- **Ear-shaped cuts** — the cut is a plane, plus pegs, plus a dovetail. Bambu's
  ear is a pair of hooks; Shale has the dovetail, which does the same job with
  one trapezoid: it is wider at its tip than at its root, so the halves press
  together and stay. What Shale does not have is Bambu's hook shape itself.
  The mesh boolean behind both joints used to be capped at 600 triangles; the
  split plane is chosen now and the cap is a memory guard at 200 000, see
  [verification.md](verification.md).
- **Camera** — not implemented. `SHALE_PRINT_REMOVAL` stays off, sends no motion,
  and `will_move_printer` is false.
- **Bambu-flavoured G-code** — the output is Marlin-flavoured, which the A1
  accepts over the same `project_file` path, but it is not Bambu's own flavour.
- **Multi-spool send** — correctly refused rather than silently wrong.
- **The video is not in the repo.** It is 1.6 MB of screen recording; it belongs
  in the PR description, not in a git object.

## Since this note was first written

- **A dovetail, and a cap on the boolean.** `cut_with_dovetail` puts a
  trapezoid tab across the seam and bores the same shape out of the other half;
  on a 30 mm cube with a 6 mm root that is **324.00 mm³** of tab reaching
  **6.00 mm** past the plane. The BSP boolean behind it took `polygons[0]`'s
  plane at every node, which on a bored part builds a chain: the 1946-triangle
  bracket did not finish in five minutes. Every node now samples two dozen
  candidate planes and takes the one that splits least and balances most, and
  the same bracket cuts in **0.01 s** (1864 triangles out, 47 layers). The cap
  is a memory guard at 200 000 triangles now, and the geometry runs on a 256 MB
  stack after a stack overflow killed the window outright.
- **A boolean between parts** — *subtract*, *union*, *common* in the model frame,
  both sides baked into world space, cutters consumed, and only parts whose
  bounds actually meet the chosen one taking part. Measured: two 20 mm cubes
  overlapping 10 mm → subtract exports 16 triangles / 4000.00 mm³ spanning
  x −10.0…0.0 and slices as 100 layers; union 12000.00 mm³; common 4000.00 mm³.
  Nothing overlapping is refused in words rather than performed.
- **Place on face** — a click names the surface that goes down; the turn is the
  shortest arc from the clicked normal to straight down, composed with the
  rotation the part already had, then dropped and kept where it stood in plan.
  On the bracket: rot 0,0,0 → −180,0,−180, volume 3885 mm³ unchanged, plan centre
  128.00, 128.00, resting at z 0.00, and Ctrl+Z puts it back.
- **The delete key**, undoably, and undo/redo now clear the notice that was
  describing something no longer true.
- **An idle document tab reads as a tab** — border, darker seat, close cross
  always visible.
- **`docs/verification.md`** — every headline number in one place with the
  command that produces it, and the list of what is still not proved.
- **The video was re-recorded** on this build: 2560 × 1558, 30 fps, 39.90 s,
  1,864,360 bytes, every pane change checked against the active chip.
