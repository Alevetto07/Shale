# Handover

> **Where this file is now out of date.** It is a snapshot of `38729ab`
> (2026-11-03) and is kept as written, because it explains how the tree got
> here. For what is true now — what is finished, what each commit is, what is
> still open — read [docs/STATUS-v0.1.md](docs/STATUS-v0.1.md) and
> [docs/verification.md](docs/verification.md).
>
> As of 2026-11-10 the branch is ninety-three commits past that snapshot and
> `docs/STATUS-v0.1.md` is the status page. Since this was written: the queue
> no longer advances itself; a Bambu machine's own start and end sequence is
> used, with the engine's heat prelude stripped so the machine's sequence is
> the first thing in the file; the Boolean kernel picks its split plane and no
> longer peels a convex body face by face (30 k triangles still exceed the
> 12 000-triangle cap); undo after a cut restores the part; parts can be
> joined with pegs or a dovetail; the access code lives in the Windows
> Credential Manager rather than in the window's storage; a printer's
> certificate is pinned on first sighting, a changed one is refused with a
> sentence and a press, and a printer can now be forgotten outright; a Bambu
> double in the repo, bound to 127.0.0.1, proves the host end to end without
> touching a printer; and every file carries the bounding box it actually has.
>
> At the head: `cargo test --workspace --locked` 173 passed / 0 failed /
> 3 ignored, `cargo clippy --workspace --all-targets -- -D warnings` clean, an
> NSIS installer at `target/release/bundle/nsis/Shale_0.1.0_x64-setup.exe`
> (4 675 455 bytes) that has never been run and is not signed. Nothing has been
> pushed, merged, tagged or published, and no real printer has been contacted.

This file is for the next agent. It describes the repository as of commit `38729ab` on `cursor/deep-field-75ee` (2026-11-03). That commit is unfinished work on top of pull request #8. Nothing in this stack is merged. Do not merge, do not push to `main`, and do not publish a release.

Shale's own source licence is still undecided. See [LICENSING.md](LICENSING.md).

## What Shale is

Shale is Alessio's desktop 3D-printing slicer. The window is a Tauri 2 shell with a React and three.js interface. Slicing is done by an unmodified CuraEngine process. Printer definitions, quality profiles, and materials are Cura's, read as data. Shale does not link Cura, Uranium, libArcus, or OpenCASCADE.

The first MVP, plus the stacked follow-ups, can:

1. Import STL, a straightforward 3MF mesh, or a STEP file Shale tessellates itself, and show it on the build plate. Move, rotate, scale, and drop it onto the plate.
2. Pick a printer from Cura's definitions, plus layer height, infill, supports, and nozzle / bed temperature.
3. Slice with CuraEngine, follow progress, then scrub a layer-by-layer toolpath preview.
4. Export G-code.
5. Drive the same job from an MCP server.
6. Store custom instructions (pause, temperature, fan, beep, message, park) on the job and compile them into the engine's G-code after the engine finishes. The engine binary is not modified.

Still out of scope, and not built: camera-based print removal, and automatic non-planar placement. The overhang map is shaped so a later non-planar pass can reuse bridge and cantilever spans. `docs/print-removal.md` is the note. `shale_core::removal` stays off unless `SHALE_PRINT_REMOVAL=1`, and even then it does not open a camera or send a move. `will_move_printer` is false.

## Layout

```
crates/shale-core     job model, profiles, formulas, meshes, engine boundary, .shale zip (in progress)
crates/shale-mcp      MCP server over the same Session API
apps/desktop          Tauri 2 shell and the React / three.js view
apps/desktop/src-tauri
scripts/fetch-resources.sh
scripts/fetch-resources.ps1
scripts/formula_oracle.py
vendor/               created by the fetch script; not in git
examples/             cube STL, bridge, t-wing, and three STEP samples
docs/                 lockup, print-removal note
```

`shale-core` is an ordinary Rust library. The window and `shale-mcp` both call it. `Session` (`crates/shale-core/src/session.rs`) is the job the MCP tools use. The window does not use `Session`. It keeps a parallel `Inner` in `apps/desktop/src-tauri/src/lib.rs` and calls the same `slice` function.

```
UI  ↘
      shale-core  →  CuraEngine (separate process)
MCP ↗            ↘  Cura resources + fdm_materials (data files)
```

## Stacked pull requests

All eight are open and unmerged. Each PR's base is the previous PR's head, except #1, whose base is `main`. Review and merge from #1 upward. Merging a later PR into `main` on its own will pull the earlier branches with it, or GitHub will refuse until the base is `main`.

| PR | Branch | Base | What it adds |
| --- | --- | --- | --- |
| [#1](https://github.com/Alevetto07/Shale/pull/1) | `cursor/shale-mvp-75ee` | `main` | First slicer. STL import, plate, Cura printer menu, CLI slice, layer preview, G-code export. `0766223`. |
| [#2](https://github.com/Alevetto07/Shale/pull/2) | `cursor/mcp-and-instructions-75ee` | PR #1 | `shale-mcp` (stdio and loopback HTTP) and custom instructions compiled into the finished G-code. `08ad3e2`. |
| [#3](https://github.com/Alevetto07/Shale/pull/3) | `cursor/smart-supports-75ee` | PR #2 | Overhang, bridge, cantilever, island, and cosmetic analysis. Enforcer and blocker meshes passed to an unmodified CuraEngine. `1beb147`. |
| [#4](https://github.com/Alevetto07/Shale/pull/4) | `cursor/ui-polish-75ee` | PR #3 | Frame the camera on the part centre. Restyle the desktop controls. `1874168`. |
| [#5](https://github.com/Alevetto07/Shale/pull/5) | `cursor/arcus-and-resolver-75ee` | PR #4 | Arcus socket backend beside the CLI, and a closer match to Cura's setting formulas, with golden files. `197bcac`. |
| [#6](https://github.com/Alevetto07/Shale/pull/6) | `cursor/step-tessellation-75ee` | PR #5 | STEP tessellation with truck. 3MF mesh import kept as a mesh. Chord and turn controls. `f61178c`. |
| [#7](https://github.com/Alevetto07/Shale/pull/7) | `cursor/windows-workflow-75ee` | PR #6 | Large STL import off the UI thread, with cancel. Find Cura and CuraEngine on Windows without environment variables. `b138a3d`. |
| [#8](https://github.com/Alevetto07/Shale/pull/8) | `cursor/deep-field-75ee` | PR #7 | Deep Field palette, then a Hubble Deep Field ASCII plate (`3a4a2b8`). Commit `38729ab` starts replacing that plate. It is not finished. |

## Architecture

### Engine

`shale_core::engine::Engine` picks the backend at runtime. Both backends are the same unmodified `CuraEngine` binary. Shale does not link libArcus. The Arcus framing (header `0x2BAD0100`, sizes, FNV-1a type id, protobuf body, zero word as keepalive) is reimplemented in Rust from the published protocol. `Cura.proto` is not vendored.

* `SHALE_ENGINE=arcus`, or the Engine panel's `arcus` choice, uses the socket. Progress comes from `Progress` messages. Layer polygons can be drawn as they arrive.
* `SHALE_ENGINE=cli`, and the default when unset, runs `CuraEngine slice`. CuraEngine 5.13's command-line progress is empty, so the CLI estimates progress from the log and from `;LAYER:` lines. The layer preview is parsed from the finished G-code.

CuraEngine treats the bed centre as the origin. Shale's scene uses the front-left origin, so the mesh written for the engine is shifted by half the bed when `machine_center_is_zero` is false. G-code comes back in machine coordinates.

The engine only reads `default_value` from definition JSON. Cura's `value` formulas are Python, evaluated by Uranium in the official app. Shale resolves the stack itself and passes a `-r` JSON file. `{setting}` placeholders in start and end G-code are filled in first.

### Setting resolver

`crates/shale-core/src/resolver.rs` and `formula.rs`. The resolver walks inheritance (`fdmprinter` → manufacturer base → machine, and the extruder train), then the nozzle variant, a matching material, the quality profile, an optional intent, and the window overrides.

The formula subset covers what actually appears in the 5.13 definitions: arithmetic, comparisons, `and` / `or` / `not`, conditionals, lists and subscripts, `min` / `max`, `round`, `math.*`, `all(x for x in ...)`, and the helpers `extruderValue`, `extruderValues`, `extruderValueFromContainer`, `resolveOrValue`, and `anyExtruderWithMaterial`. A formula that fails at evaluation falls back to `default_value` and is counted as a warning.

No lambdas, list comprehensions, f-strings, or slices. None of those are used as formulas in the pinned 5.13 tree. `extruderValueFromContainer` skips window overrides when the container index is above zero. Intents can be passed on `ResolveRequest` but the window does not expose them. Quality matching is by ancestry, quality type, variant name, and material id, not Cura's full quality-node tree.

Golden file: `crates/shale-core/tests/golden/resolved.json` for the Creality Ender-3 (draft), the Prusa i3 (draft), and the Ultimaker 2 (normal), 0.4 mm nozzle, `generic_pla`, no window overrides. `scripts/formula_oracle.py` compares with Uranium's `SettingFunction` when `SHALE_URANIUM_ROOT` points at an extracted Cura AppImage. Uranium is not shipped. On the last `cargo test -p shale-core` in this session, `sample_printers_match_the_golden_file` passed (resources were not required for that path; the Uranium oracle skips when the root is unset).

### MCP

`crates/shale-mcp`. Stdio by default. `--http` serves streamable HTTP and refuses any address that is not loopback. Tools call `Session`: list printers, materials, and profiles; load and transform a model; arrange; get and set settings; list, add, and remove instructions; analyse, propose, and apply supports; slice; slice summary; export G-code. Resource `shale://job`. Prompt `slice_a_model`.

### Custom instructions

`crates/shale-core/src/instructions.rs`. Kinds: pause (layer or height, optional filament change), temperature, fan, beep, message, park and wait. Inserted at the start of the chosen layer after CuraEngine has written the file. Marlin uses `M600` / `M0`. RepRap uses `M226` for a pause. Klipper uses `PAUSE`, or `M600` for a filament change. The window shows a `+` on the layer track.

### Smart supports

`crates/shale-core/src/supports.rs`. A face needs support when its angle from the vertical is past `support_angle`. Spans held at both ends, up to a bridge length, are bridges. Spans held at one end are cantilevers. Floating pieces are islands. A large face only just past the angle is cosmetic. `propose` builds enforcer meshes (`support_mesh`, `support_mesh_drop_down=false`) and blocker meshes (`anti_overhang_mesh`). Automatic everywhere support is turned off for that slice. The window can disable a region and can compare the proposal with supporting every overhang.

### STEP

`crates/shale-core/src/cad.rs` uses truck (`truck-stepio`, `truck-modeling`, `truck-meshalgo`, `truck-topology`, Apache-2.0). OpenCASCADE is not linked. Chordal tolerance starts at half the layer height, never coarser than half the nozzle, clamped to 0.02–0.5 mm. If the first mesh turns more sharply than the angular tolerance, the chord is tightened and the shape is meshed again. A file whose length unit is centimetres, metres, or inches is scaled to millimetres. A file with no unit is treated as millimetres. A solid that exists only as a boolean result, or an entity the reader does not know, is rejected with the parser's error.

`truck-stepio` 0.3 reads the analytic surfaces and NURBS shells used by `examples/block-20.step`, `examples/cylinder-r10-h20.step`, and `examples/sphere-r10.step`. The sphere is a spun shell, not a closed solid, and is still tessellated.

### Windows discovery

With no `SHALE_CURA_DIR`, Shale looks next to the executable, in `vendor/` above the working directory, and in an installed UltiMaker Cura 5.x tree (`share/cura/resources`, and on Windows the Program Files layout). If nothing matches, the window lists every path it tried. If `SHALE_CURAENGINE` is unset on Windows, it looks for `CuraEngine.exe` under an `UltiMaker Cura*` folder in Program Files and loads DLLs from that folder. This was not run on a Windows machine in the session that wrote this file.

Cura 5.13 does not include a Bambu Lab A1 definition. The only `BambuLab` text in that tree is a g-code flavour name. A1 profiles arrived in a later Cura release, so they are not in this pin.

### Deep Field theme

Dark only, already on this branch before `38729ab`. Tokens in `apps/desktop/src/styles.css` and `apps/desktop/src/theme.tsx`:

* Background `#06070b`, panels `#0b0d14`, ink `#e9e8f2`.
* Ink-2 is 72% ink, ink-3 is 58% ink, hairlines are 12% ink.
* One accent, `#b9b3ff`, for selection, focus, the active layer, and primary actions.
* Geist Mono for panels and controls. Labels are 11 px, 0.14 em, uppercase.
* Inter for longer text (hints, banners, credits).
* Space Grotesk 600, letter-spacing -0.02 em, for the wordmark only.
* The faceted stone mark is `apps/desktop/src/brand/shale-mark-dark.svg`.
* Fonts are SIL OFL 1.1 and live in `apps/desktop/src/fonts/`. They are bundled, not loaded from a host.

The 3D view (`apps/desktop/src/Viewport.tsx`) uses the same background and panel colour for the bed, a dim grid, muted model shading, and accent on the selected model. Toolpath colours stay distinct but are muted. `38729ab` adds short axis lines at the front-left corner in grey-lavender, not primary red/green/blue. The viewport uses `frameloop="demand"`.

Box-drawing frames (`┌ ─ │`) are the existing panel chrome from this branch. The progress meter uses block characters. Those were already in the Deep Field restyle. The new start-screen menu does not add icons.

OS-native file dialogs (Tauri `open` / `save`) are not themed. The window title bar is the operating system's.

## Licensing

Details and URLs are in [LICENSING.md](LICENSING.md). Short form:

* Shale's own code has no licence yet. Alessio has to choose one before distribution. Do not treat the code as open source.
* CuraEngine is AGPL-3.0. It is a separate unmodified process. If a binary is shipped with Shale, that binary stays under the AGPL and its source must be offered. No CuraEngine binary is committed.
* Cura definitions and profiles are LGPL-3.0. They are not vendored. `scripts/fetch-resources.sh` checks out Cura 5.13.0 (`1fb8a7610460e03ec62da950d1c173a249b23b59`) into `vendor/Cura/`. Shale reads them as data and does not link Cura or Uranium.
* libArcus is not linked. The socket framing is reimplemented. Field numbers match the 5.13.0 `Cura.proto`.
* fdm_materials is CC0-1.0, pin `82f433b2cfe0ac36c974443b80c9294f31b79224`.
* OpenCASCADE is not linked. STEP uses truck (Apache-2.0). `spade` is MIT OR Apache-2.0. `rmcp` (MCP SDK) is Apache-2.0. Cargo fetches them. Their source is not copied into this repository.
* The start-screen image in `38729ab` is NASA/JPL-Caltech, GALEX ultraviolet M31, NASA id PIA15416, from https://images.nasa.gov/details/PIA15416. The bundled file is a 640×477 downscale of the NASA medium JPEG. NASA image content is generally not subject to copyright in the United States. The previous Hubble Deep Field JPEG was removed in that commit. The credit string in code is `apps/desktop/src/field/credit.ts`.

## Build, test, and run

Rust stable new enough for the lockfile is required. On 2026-11-03 the lockfile's Tauri 2.12 crates want rustc 1.90. Rust 1.83 and 1.85 both failed to compile this lock. Rust 1.99.1 compiled `shale-core`. The README's "1.85 or newer" line is behind the lockfile. Use a current stable.

Node 22 for the desktop. A C compiler. Linux also needs the Tauri GTK / WebKit packages named in the README (`libwebkit2gtk-4.1-dev`, `libgtk-3-dev`, `libayatana-appindicator3-dev`, `librsvg2-dev`, `patchelf`, `libsoup-3.0-dev`). This environment did not have them at the start of the session. After `apt` installed them, `cargo test --workspace` was not re-run.

### Resources and the engine

```bash
./scripts/fetch-resources.sh          # Linux / macOS
```

```powershell
.\scripts\fetch-resources.ps1         # Windows. Git for Windows is required.
```

Environment variables:

| Variable | Role |
| --- | --- |
| `SHALE_CURAENGINE` | Path to the unmodified CuraEngine binary. If unset, `PATH` is searched. On Windows, an installed `UltiMaker Cura*` tree is also searched. |
| `SHALE_CURAENGINE_LIB_DIR` | Directory of libraries that must sit beside that binary (AppImage extract, or the Cura install on Windows). |
| `SHALE_CURA_DIR` | Cura checkout or resources root. Overrides the search. |
| `SHALE_MATERIALS_DIR` | fdm_materials checkout. |
| `SHALE_ENGINE` | `cli` (default) or `arcus`. |
| `SHALE_URANIUM_ROOT` | Optional. Extracted Cura AppImage for the formula oracle. Not shipped. |
| `SHALE_DEMO_MODEL` | Optional model path imported for a screenshot launch. |
| `SHALE_DEMO_SLICE` | `1` slices that model before the window opens. |
| `SHALE_DEMO_SUPPORTS` | `1` proposes supports for the preloaded model. |
| `SHALE_PRINT_REMOVAL` | `1` arms the removal stub. It still does not move a printer. |

CuraEngine 5.13.0 matches the resource pin. Build it from that tag with Conan and CMake, or point at the binary inside an official AppImage. Do not commit the binary. Leave it unmodified. `CuraEngine help` should print the AGPL banner before you bother with the window.

### Linux

```bash
./scripts/fetch-resources.sh
cargo test --workspace
cd apps/desktop
npm install
npm run build          # tsc --noEmit && vite build
npm run tauri dev      # window
```

An optional launch, from the repo root, after the shell is built:

```bash
SHALE_CURAENGINE=/path/to/CuraEngine \
SHALE_DEMO_MODEL=examples/cube.stl \
SHALE_DEMO_SLICE=1 \
  ./target/debug/shale-desktop
```

### Windows (PowerShell)

Fetch with `.\scripts\fetch-resources.ps1`. Build CuraEngine from the 5.13.0 tag in an `x64 Native Tools` prompt (Visual Studio 2022 C++ workload, CMake, Python, Conan), as in the README. `SHALE_CURAENGINE` should be the full path to `CuraEngine.exe` if it is not discovered under Program Files. Paths are passed as separate arguments, so a space in `C:\Program Files` does not need extra quotes.

Then, from the repo:

```powershell
cargo test --workspace
cd apps\desktop
npm install
npm run build
npm run tauri dev
```

The same `SHALE_*` variables apply. Windows discovery and the large-STL path were written in PR #7 and were not re-run on hardware in the session that wrote this handover.

### MCP

```bash
cargo build -p shale-mcp
./target/debug/shale-mcp
./target/debug/shale-mcp --http 127.0.0.1:7337
```

Binding `0.0.0.0` is rejected. The HTTP endpoint is `http://127.0.0.1:7337/mcp`.

### What was actually run on 2026-11-03

* `cargo +stable test --locked -p shale-core`: 38 passed, 1 failed. Failure: `project::tests::project_round_trip_keeps_step_bytes_and_choices`, assertion "the sample STEP file should be a STEP exchange", before the zip was compared. `missing_model_entry_is_an_error`, `missing_project_file_is_an_error`, and `recents_keep_eight_and_mark_missing_files` passed.
* `npx tsc --noEmit` in `apps/desktop`: exit 0.
* `npm run build` (vite) was not run.
* `cargo test --workspace` was not completed. An earlier attempt failed because `gdk-3.0` was not installed, which is before this commit's Rust compiled the shell.
* The desktop app was not launched. No screenshots and no video were taken.
* Plate-field CPU was not measured. There is no before/after number from this session. Do not treat the brief's "1.04" as a number this session reproduced.

## Current brief: done, partial, not started

The brief on PR #8, after the Deep Field restyle, asked for four product changes and a measured CPU cut. Commit `38729ab` is a partial implementation. It was not verified in the running app.

### NASA Andromeda ASCII start screen

Partial.

Done in the tree:

* Hubble JPEG removed. Bundled image is `apps/desktop/src/field/andromeda.jpg` (640×477, about 55 KB), downscaled from NASA's medium JPEG of PIA15416. Credit updated in `credit.ts`, the slicer credits panel, the start-screen line, and `LICENSING.md`.
* `StartScreen.tsx` is a full-window view shown before the slicer. Most of the window is the near-black field. The menu sits in the lower left: mark, wordmark, New project, Open project, Import model, up to eight recent rows (name, date, printer), Printers, Settings, About and credits, Quit, and Return once a job has been opened. Arrow keys, Enter, and the letter shortcuts in that file. Escape closes a sub-panel or returns to the slicer.
* `PlateField` / `field/paint.ts` sample the image onto ramp ` .:-=+*#%@`, tint glyphs from the pixel, drift the sample, and lens near the pointer (brighter, higher ramp step, offset that eases out and springs back). A short trail of `.` `+` `*` follows the pointer. The canvas is `aria-hidden`. Reduced motion paints one still frame and does not subscribe to the clock. The comet is only mounted from the start screen. The slicer does not mount `PlateField` or the old margin canvas (`MarginField.tsx` was deleted).
* Opening, importing, or New project switches to the slicer. The slicer top bar has Start, which goes back without clearing the job.

Not done:

* Nobody has looked at it running. Keyboard focus, the printer list, the about copy, and whether the galaxy actually reads as ASCII are unchecked.
* Quit calls `getCurrentWindow().close()`. The capability `core:window:allow-close` was added and not exercised.
* The old margin-star field is gone. There is no separate comet over the 3D view, by construction, but that was not watched.

### `.shale` project file

Partial. The format is in `crates/shale-core/src/project.rs`. The window calls it from `save_project`, `open_project`, `clear_job`, `list_recents`, and `forget_recent_project` in `apps/desktop/src-tauri/src/lib.rs`.

The zip contains `project.json`, `thumbnail.jpg`, and `models/<index>-<filename>`. Model bytes are the original file bytes, so a STEP entry stays STEP. The JSON stores printer id and label, variant, quality, material, `UserSettings` overrides, chord and turn, transforms, custom instructions, and the support plan with per-region `enabled` flags. Support surface samples are cleared on write. Recents are `recents.json` in the Tauri app config directory, capped at 8, with `missing` set when the path is not a file. A missing archive, a missing model entry, and an empty model entry return an error that names the file. The desktop shows that string in the error banner.

The round-trip test that would lock STEP bytes, transforms, settings, a beep instruction, a disabled support region, and the thumbnail did not pass (see above). The shell that wires save and open was not compiled after the commands were added. Thumbnail bytes are a small JPEG drawn on a canvas in `apps/desktop/src/thumbnail.ts`. Save and Save as are in the slicer top bar. Ctrl or Cmd+S and Ctrl or Cmd+Shift+S call them. There is no proof the dialog, the zip, or the recent list work on screen.

Imported models keep `source_bytes` by reading the file again after the mesh load. A model with empty source bytes cannot be saved. The demo preload path uses `fs::read(...).unwrap_or_default()`, so a demo model whose read failed would fail at save time.

### Deep Field across the whole app

Partial, and most of it was already on the branch before `38729ab`.

`38729ab` adds scrollbar colours, the start-screen menu styles, a floating error line, muted toolpath and support colours, bed grid and outline tweaks, and the axis lines. It does not prove that every dropdown, dialog, toast, and banner was checked. File dialogs and the OS title bar cannot be themed from CSS. Say that in the PR if you finish the visual pass. No screenshot of the slicer, a sliced preview, or a settings panel was taken in this session.

### CPU target

Not done. No measurement.

The code aimed at the target is in `apps/desktop/src/field/loop.ts` (shared clock skips frames closer than 33 ms) and `field/paint.ts` (glyph alpha atlas built once, full redraw only when the drift step changes, otherwise a dirty rectangle around the lens and the comet). That is the intended design. It was not timed. Do not report 0.15, and do not report 1.04, unless you measure both with the same method.

A fair measurement: one page, one canvas size, one image, one pointer path, the unoptimized full-`fillText` painter and the painter in `paint.ts`, process CPU from `/proc/<pid>/stat` over several seconds while the pointer is moving. The start screen is the only animation. It is unmounted in the slicer, so slicing, scrubbing, and orbiting do not run it. That satisfies "animation stops while slicing, scrubbing, and when the viewport is in use" only if the start screen is actually unmounted, which the React tree does, and which nobody has watched.

## Known bugs and gaps

* STEP boolean solids, and entities truck does not know, are rejected. They are not meshed by OpenCASCADE.
* A STEP file with no length unit is treated as millimetres.
* If the first tessellation turns more sharply than the angular tolerance, Shale tightens the chord and meshes again. Planes ignore both controls.
* The new project round-trip test fails. Do not trust `.shale` until that test is green and a real file has been opened again.
* The Tauri commands added in `38729ab` were not compiled. `lib.rs` may not build.
* Windows auto-discovery and the large-STL import path have not been run on a Windows machine in this session. PR #7 describes the intended behaviour. The README records earlier timings for 1e6 and 5e6 triangles on the machine that wrote that PR, not on this one.
* Cura 5.13 has no Bambu Lab A1 definition.
* Quality profiles: Creality's preferred type in 5.13 is `standard`, and that profile does not itself set a layer height. The window reloads the resolved layer height when quality changes.
* CuraEngine CLI output sometimes still contains `;TIME:6666`. Shale replaces that from `;TIME_ELAPSED` or the log, and fills `;Filament used: 0m` from extrusion commands. Arcs (`G2` / `G3`) are not expanded in the preview.
* `Session` and the desktop `Inner` are two copies of the job. Project save/load goes through `Inner` only. MCP does not read or write `.shale` files.
* Intents are not in the window.
* Paint-on-support from Cura 5.14's 3MF PNG is not implemented. Supports are meshes and blockers, which the 5.13 CLI can apply.

## What to do next

In this order. Do not start the later items while the earlier acceptance checks are red.

1. Make the tree build. `cargo test --workspace` with a current stable Rust, the GTK/WebKit packages on Linux, and `npm run build` in `apps/desktop`. Acceptance: both exit 0, including `project_round_trip_keeps_step_bytes_and_choices`. If the STEP assertion is a bad comparison rather than a bad zip, fix the assertion so it still checks the bytes that were written and read back, including the `.step` extension and identical STEP bytes.

2. Run the desktop app. Acceptance, watched in the Shale window: start screen shows ASCII Andromeda on near-black, the menu works with the keyboard, New project and Import model reach the slicer, Start returns, Quit closes the window. Save two projects, kill and relaunch, and see both on the recent list with name, date, and printer. Delete one file on disk and confirm the row is marked missing and opening it reports the missing path instead of crashing. Open a project that contains a STEP file and confirm the zip entry is still STEP, not an STL rewrite.

3. Measure the start-screen CPU the same way before and after, while the pointer is moving over the field. Acceptance: the optimized painter stays at or under about 0.15 of a core on that machine, and the note records both numbers, the canvas size, and the method. If it does not, the next levers are a coarser cell, a smaller dirty rect, and skipping `putImageData` when the spring has settled. The atlas and the 30 fps cap are already the intended first cut.

4. Theme pass. Walk every panel, menu, banner, and the layer preview. Acceptance: no leftover light-theme colours in app CSS or in `Viewport.tsx`. Toolpaths stay distinguishable. Write down, in the PR, that OS file dialogs and the title bar are not themed.

5. Only after 1–4: the verification the brief asked for. Screenshots of the start screen, the recent list with two saved projects, the slicer with a model, a sliced preview, and a settings panel. One video under 60 seconds: start screen with the pointer lens, open a recent project, slice, back to the start screen. Record the Shale window, not an empty desktop. Pull frames about every 5 seconds and look at them before claiming the video shows the app.

6. Deferred, after the brief above is actually true. Automatic non-planar placement: `analyse` already returns spans with samples a plane can be fitted to. Do not move the printer. Camera-based print removal: keep it behind `SHALE_PRINT_REMOVAL`, default off, and do not send motion until there is a real design beyond `docs/print-removal.md`. Neither of those is started beyond the stub and the support samples.

Push further work to `cursor/deep-field-75ee` so it stays on PR #8, unless Alessio asks for a new branch. Do not merge. Do not push to `main`.
