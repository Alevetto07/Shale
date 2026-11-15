<p>
  <img src="docs/shale-lockup-dark.png" alt="Shale" width="284" />
</p>

Shale is a desktop 3D-printing slicer with its own interface: a dark, monospace window, box-drawing chrome, and a WebGL plate. Slicing is done by an unmodified [CuraEngine](https://github.com/Ultimaker/CuraEngine) process. Printer definitions, quality profiles, and materials come from an installed Cura or Bambu Studio, read as data.

This is the first MVP:

1. Import an STL, a STEP part, or a 3MF package — including the multi-part packages Bambu Studio and Orca write — and show it on the build plate. Move, rotate, scale, and drop it onto the plate.
2. Pick a printer from Cura's definitions or from an installed Bambu Studio profile library, then set the machine, nozzle, process, and filament from the menu bar.
3. Slice with CuraEngine, follow progress, then scrub a layer-by-layer toolpath preview.
4. Export the G-code — or the plate itself, as STL or 3MF.

The window is a top strip and a plate below it. The strip carries the **workspace switch** — Home, Prepare, Preview, Device — and beside it a **tab per open project**, each with a dot while it has unsaved changes; leaving a tab parks its plate in a scratch file so nothing is lost, and the file it came from is only rewritten when you save the tab. Home is the start screen — in the main area, with the strip still overhead, because a tab strip that disappears is not a tab strip. Prepare and Preview set the viewport (Preview greys out until there is a slice), and Device is the machine: its four spools, a live link to the printer on the LAN — state, percentage, layer, remaining time, nozzle and bed actual against target — and the job itself, where **send & print** uploads the plate that was just sliced and then asks before it starts, with **pause**, **resume** and **stop** beside it. Below it sits the menu bar: **File** (new, open, import, save, export), **Edit** (duplicate, drop to plate, arrange on plate, delete), **View** (solid, wireframe, ASCII, toolpaths, frame), **Printer** (machine, nozzle, process, filament), and **Slice settings** — eleven tabs (quality, strength, support, build plate, speed, cooling, multi, retraction, ironing, special modes, material) covering 88 engine settings; anything you leave alone comes from the profile. The sheet's **Applies to** menu points those settings at one part instead of the whole plate.

The model frame has **mirror X / Y / Z**, **turn 90°**, **Auto orient**, and **Upright**. Turns and mirrors happen about the part's centre, and a mirror reverses the triangle winding as it bakes, because a negative scale otherwise turns the solid inside out and the engine drops its walls. Below them the cut row splits a part with a plane: pick `cut X`, `cut Y` or `cut Z`, slide the plane with the offset, and take *both halves*, the *upper only* or the *lower only*. Each half is capped where the plane went through, so both arrive as closed solids that slice on their own, and each is set down on the plate rather than left floating. Above the cut row sits **Measure**: arm it, click two points on a part, and the distance is drawn on a line between them and printed in the panel with its x/y/z deltas. A click snaps to the nearest vertex within 2 mm — about a nozzle width and a little more — so clicking near a corner gives the corner exactly, while a click in the middle of a face stays where the ray put it. Escape or *Clear* puts the ruler away. Above the transform rows sits the **ams** frame: three more spools beside the plate's own material, and a **spool** picker on any selected part. Give a part a spool and the slice carries a tool change, that spool's hotend temperature and its own filament — a two-cube plate cuts over to the second spool **301 times** and prints it at **205 °C** instead of the plate's 220 °C, while the same file with every part on spool 1 has no `T` line at all. The **Multi** tab grows a prime tower for those changes: off, the two-spool file carries no tower; on at 45 mm it prints one on all **300 layers**, **44.6 mm** square on a **60.6 mm** base, holding **3590.2 mm** of filament. Spool choice is saved with the plate and survives a reopen. Per-filament flush volumes are still missing, so the tower purges a fixed amount rather than one worked out from the colour change. Shale also proposes its own support regions: [propose] turns every cantilever past the angle into an enforcer column, [compare] slices the plate twice and reports the engine's own filament figure, **2.4464 m with the plan against 3.8505 m with support everywhere** on the bracket, and switching one column off takes the plan to **1.2235 m**. The Support tab also chooses the **shape**: `normal` or `tree`, with branch angle, branch diameter, flare, tip and top density. The upright T-bracket needs **3276.1 mm** of normal support and **2069.9 mm** of tree support, 36.8 % less, for 14.9 % less plastic in the part at the same 170 layers. The **paint support** frame paints supports instead of guessing at them: it lists every ceiling the part has — on the gantry **387, 387 and 6 mm²** — and a painted ceiling becomes a column of prisms down to the bed, handed to the engine as a `support_mesh` volume. Painting turns the plate's own support off, so what is painted is the whole of it: one arm painted puts **1008.6 mm** of support under that arm alone, x **−69.3 to −5.7**, climbing to **z 29.4 mm**, against **2663.7 mm** across the plate with nothing painted, and clearing the paint gives back exactly **2663.7 mm**. Painted faces are saved with the part and index the part's own triangles, so moving or turning it keeps the paint; it paints patches rather than free-hand, and it cannot paint seams. A project holds **several build plates**. The `plates` frame is a row of chips — **+** adds one, **×** drops it — and each plate keeps its own parts and its own support plan, so switching plates swaps what is on the bed without re-reading anything. A slice belongs to the plate it was made on, and saving writes every plate: two plates with a tower and a cube come back as two archive entries marked `plate: 0` and `plate: 1` under named plates. The two plates slice on their own: the tower gives 250 layers, the cube 300. A **calibration** frame builds the temperature tower itself and puts it on the plate: five 10 mm bands, alternating 20 mm and 26 mm blocks so every band overhangs the one below by 3 mm, **250 layers** once sliced. *Step the hotend* rewrites that slice so band *n* prints 5 °C cooler than band *n−1* — five `M104` lines at layers 0, 48, 98, 148 and 198, whose heights read back out of the file as **0.2, 10, 20, 30 and 40 mm** — and leaves every `G` line where it was, so the preview still matches the plate. The same frame builds two more coupons: a **flow coupon**, a 30 mm single-wall tube of five bands (240 triangles) that *step it* writes **five `M221` lines** down — 105, 102.5, 100, 97.5, 95 % — and a **pressure tower** that takes **five `M900 K` lines** from 0.000 to 0.040 K. Flow is measured with calipers on a wall rather than judged by eye, which is why the coupon is a tube and not a tower. The scoreboard against Bambu Studio, including what is still missing, is [docs/bambu-parity.md](docs/bambu-parity.md). New, open, import, save, export, duplicate, arrange, undo and redo are on
Ctrl+N / O / I / S / E / D / Shift+A / Z / Shift+Z; the rest of the bar is a menu away. Each menu edits the same job the side panel does.

The plate carries a **job card** in its top-left corner once a slice exists: time, filament, weight and layer count, in tabular figures so two slices can be compared at a glance. Time and filament are the file's own figures, read back out of the header, and weight appears only when the filament profile carries a density — a weight invented from a default would be worse than no weight at all. In **Preview** the same plate carries the **feature list** down its right-hand side: every kind of line the slice is made of, in the colour it is drawn, with a switch for each. Turning the infill off leaves the walls standing, which is the only way to answer "is the infill right?". The solid part is not drawn in Preview: it would stand in front of its own infill.

Two more things the plate can do without a file: **File → Add a part** puts a cube, cylinder, cone or sphere on the first free patch of the bed, and **Edit → Fill the plate** steps the selected part's own footprint across the bed until it runs out, skipping anything already standing there. **Edit → Split to objects** is the other half of a cut: a part that is really several bodies is separated into the parts it always had, each keeping its place on the plate.

Two more pieces sit on the same core:

5. An MCP server, so an agent can list printers, load a model, change settings, slice, and export G-code.
6. Custom instructions (a pause, a temperature change, a beep) stored on the job and compiled into the engine's G-code. You do not edit the file by hand.
7. The host: the same window talks to the printer itself over its LAN interface — MQTT over TLS for state, implicit FTPS for the upload, and start/pause/resume/stop as requests. It never starts a print by itself; every actuation is a press, and the press names what will be printed. [docs/printer-host.md](docs/printer-host.md).

Still out of scope: camera-based print removal (the camera stream itself is not implemented either), a job queue, and automatic non-planar placement. The overhang map is shaped so a later non-planar pass can reuse bridge and cantilever spans.

## Layout

```
crates/shale-core     job model, profiles, formulas, meshes, engine boundary
crates/shale-host     the LAN link: MQTT state, FTPS upload, job control
crates/shale-mcp      MCP server over that same job API
apps/desktop          Tauri 2 shell and the React / three.js view
scripts/fetch-resources.sh
vendor/               created by the script; not in git
```

`shale-core` is an ordinary Rust library. The window and `shale-mcp` both call it. `Session` is the job the MCP tools use (load, transform, settings, instructions, slice, export). The window calls the same `slice` function, including the instruction pass, and keeps the mesh preview the viewport needs.

```
UI  ↘
      shale-core  →  CuraEngine (separate process)
MCP ↗            ↘  Cura resources + fdm_materials (data files)
```

### CLI and Arcus

Cura's own frontend speaks [Arcus](https://github.com/Ultimaker/libArcus): protobuf messages from `Cura.proto`, framed as a big-endian `uint32` header (`0x2BAD0100`), `uint32` size, `uint32` type id, then the protobuf bytes. The type id is FNV-1a of protobuf's `GetTypeName()` (`cura.proto.Slice`, `cura.proto.Progress`, and the rest). A `uint32` of zero is a keepalive. CuraEngine is the client (`CuraEngine connect host:port`); Shale listens, sends one `Slice` (settings plus a triangle soup, with `extruder_nr` on each mesh), and reads `Progress`, layer polygons, and `GCodeLayer` until `SlicingFinished`.

CuraEngine treats the bed centre as the origin and, when `machine_center_is_zero` is false, adds half the bed size itself. Shale's scene uses the front-left origin, so the mesh written for the engine is shifted back by half the bed. The G-code that comes back is in machine coordinates and lines up with the part.

`shale_core::engine::Engine` picks the backend at runtime:

* `SHALE_ENGINE=arcus` (or the Engine panel's `arcus` choice) uses that socket. Progress percentages come from `Progress` messages. Each layer's polygons are drawn in the viewport as they arrive.
* `SHALE_ENGINE=cli`, and the default when the variable is unset, runs `CuraEngine slice`. CuraEngine 5.13's command-line `sendProgress` is empty, and `-p` is a leftover no-op, so the CLI estimates progress from the log (`layer N of M`, then `starting export`) and from `;LAYER:` lines. The layer preview is parsed from the finished G-code.

Both backends are the same unmodified binary. Shale does not link libArcus. See [LICENSING.md](LICENSING.md).

The engine only reads `default_value` from definition JSON. Cura's `value` formulas are Python, evaluated by Uranium in the official app and then sent across as literals. Shale resolves the stack itself and passes a `-r` JSON file (`global`, `extruder.0`, …). `{setting}` placeholders in start and end G-code are filled in first, because the engine writes those strings through unchanged.

## Setting resolution

Shale does not depend on Uranium. The resolver walks definition inheritance (`fdmprinter` → `creality_base` → `creality_ender3`, and the extruder train), then applies the nozzle variant, a matching material from fdm_materials, the quality profile, an optional intent, and the handful of values from the window. Formulas cover the expression language that actually appears in the 5.13 definitions: arithmetic, comparisons, `and` / `or` / `not`, `a if c else b`, lists and subscripts, `min` / `max` (including `key=some_list.index`), `round`, `math.*` (including `math.log` and `math.pi`), `all(x for x in ...)`, and the Cura helpers `extruderValue`, `extruderValues`, `extruderValueFromContainer`, `resolveOrValue`, and `anyExtruderWithMaterial`.

Every `value`, `resolve`, limit, and `[values]` formula in the fetched 5.13 tree parses. A formula that fails at evaluation falls back to `default_value` and is counted as a warning.

`scripts/formula_oracle.py` loads Uranium's `SettingFunction` from an extracted Cura AppImage (`SHALE_URANIUM_ROOT`) without importing the `UM` package, because that import pulls in PyQt6 and aborts. It is a test harness only. Uranium is not shipped. On the three sample printers the oracle agrees with Shale on the fdmprinter formulas that the quality profile did not replace. `crates/shale-core/tests/golden/resolved.json` locks the resolved values for the Creality Ender-3 (draft), the Prusa i3 (draft), and the Ultimaker 2 (normal), with a 0.4 mm nozzle and `generic_pla`, and with no window overrides.

Still rough:

* No lambdas, list comprehensions, f-strings, or slices. None of those are used as formulas in the 5.13 definitions; generator expressions are only the `all(... for ... in ...)` form, which is supported.
* `extruderValueFromContainer` skips the window's overrides when the container index is above zero. Shale does not model Cura's quality-changes container, so index 2 and index 1 are the same here. One Ultimaker S8 setting uses it.
* Intents are loaded and can be passed on `ResolveRequest.intent_category`. The window does not expose them yet.
* Quality matching is by definition ancestry, quality type, variant name, and material id. It is not a full copy of Cura's quality-node tree.

Creality's preferred quality type in 5.13 is `standard`, and that profile does not itself set a layer height. The window reloads the resolved layer height when you change quality; set 0.2 mm yourself for a typical draft.

## CAD import

STL is a mesh already. 3MF mesh import reads the triangles in the package (units, build items, and component transforms) and does not tessellate anything. A package holding several objects is imported as several parts — each keeps the name and the place the file gave it, so a MakerWorld model made of two halves stays two halves you can move, paint or spool separately, and the assembly is dropped to the bed rather than pulled to the middle. STEP is a boundary representation. Shale reads it with truck (Apache-2.0) and tessellates the surfaces here. OpenCASCADE is not linked; see [LICENSING.md](LICENSING.md).

The chordal tolerance is the largest gap, in millimetres, between a triangle edge and the curve it stands in for. It starts at half the layer height and is never coarser than half the nozzle, clamped to 0.02–0.5 mm. The angular tolerance is the largest turn, in degrees, between two neighbouring chords of a curved edge. If the first mesh turns more sharply than that, the chord is tightened and the shape is meshed again. Both controls sit in the Slice panel. Planes ignore them. A file whose length unit is centimetres, metres, or inches is scaled to millimetres; a file with no unit is treated as millimetres.

`truck-stepio` 0.3 reads the analytic surfaces and NURBS shells used by the sample parts. A solid that exists only as the result of a boolean, or an entity the reader does not know, is rejected with the parser's error rather than replaced by a coarser STL.

Measured on the three sample parts at 0.1 mm chord and 15° turn. Triangle count and deviation are exact for that tolerance. The times are one debug build, so they move by a few tens of milliseconds:

| Part | Triangles | Max deviation | Max turn | Import |
| --- | --- | --- | --- | --- |
| `examples/block-20.step` | 12 | 0.000 mm | 0° | about 10 ms |
| `examples/cylinder-r10-h20.step` | 1788 | 0.066 mm | 10.2° | about 150 ms |
| `examples/sphere-r10.step` | 2139 | 0.077 mm | 14.3° | about 110 ms |

The cylinder is the curved solid that is sliced end to end. The sphere is a spun semicircle, so it is a shell rather than a closed solid, and it is still tessellated from the surface.

## Fetch the printer data

Cura 5.13.0 and fdm_materials are downloaded, not copied into git. Their licence files land next to the data.

```bash
chmod +x scripts/fetch-resources.sh
./scripts/fetch-resources.sh
```

That creates `vendor/Cura` and `vendor/fdm_materials`. Override the location with `SHALE_CURA_DIR` and `SHALE_MATERIALS_DIR` if you already have a checkout. See [LICENSING.md](LICENSING.md).

## Profiles from Bambu Studio

Bambu Lab printers, processes, and filaments are read from an installed Bambu
Studio resource tree, exactly as Cura's are:

```
<Bambu Studio>/resources/profiles/<VENDOR>/{machine,filament,process}/*.json
```

`SHALE_ORCA_DIR` points at a different tree. Every vendor directory that ships
with Bambu Studio is read (BBL, Creality, Anycubic, Elegoo, Anker); a machine
file becomes an entry in the printer list, its `inherits` chain is resolved
before conversion, and the window marks it `Bambu`. The library is read once
per launch and takes well under a second.

These files are data, like Cura's: Shale converts them into its own
definition, instance, and material values and does not link or ship Bambu
Studio. The moves and the feature types are still CuraEngine's Marlin flavour,
but a Bambu machine's own start and end sequence is read from its profile and
filled in, so the wipe, the bed scan, the AMS handover and the finish are the
machine's own. AMS mapping for a multi-spool sliced 3MF is not written, so such
a plate is refused rather than sent. Treat the entries as machines and materials
Shale can reason about, not as a Bambu Studio replacement.

## Build CuraEngine

Do not commit the binary. Point Shale at it with `SHALE_CURAENGINE`, or put `CuraEngine` on `PATH`. If the binary needs libraries that sit beside it (for example after extracting the Cura AppImage), set `SHALE_CURAENGINE_LIB_DIR` as well. The Engine section of the window can set both without restarting.

The versions below match the resources pin: **CuraEngine 5.13.0**.

### Linux

CuraEngine 5.x is built with [Conan](https://conan.io/) and CMake. From the [upstream instructions](https://github.com/Ultimaker/CuraEngine/wiki/Building-CuraEngine-From-Source):

```bash
git clone --branch 5.13.0 --depth 1 https://github.com/Ultimaker/CuraEngine.git
cd CuraEngine
pip install conan
conan profile detect --force
conan install . --build=missing -s build_type=Release
cmake --preset conan-release
cmake --build --preset conan-release
export SHALE_CURAENGINE="$PWD/build/Release/CuraEngine"
```

A release build links libArcus. That is upstream's build; leave the binary unmodified. Shale talks to it over the socket or the command line and does not link that library itself.

On Linux you can also unpack the official AppImage and point at the `CuraEngine` inside it. The AppImage's dynamic linker path is relative, so the extracted binary often needs `patchelf --set-interpreter /lib64/ld-linux-x86-64.so.2` and `SHALE_CURAENGINE_LIB_DIR` set to the extracted directory. Building from source is the cleaner route.

### macOS

Same Conan flow as Linux, from the 5.13.0 tag. The binary lands under `build/Release/CuraEngine`. Universal builds are a CMake/Conan preset question upstream; an arm64 build is enough on Apple silicon. Grant the terminal whatever Conan needs to compile (Xcode command-line tools, CMake, Ninja).

### Windows

Install Visual Studio 2022 with the C++ workload, CMake, and Python. In a `x64 Native Tools` prompt, from a 5.13.0 checkout:

```bat
pip install conan
conan profile detect --force
conan install . --build=missing -s build_type=Release
cmake --preset conan-release
cmake --build --preset conan-release
```

`SHALE_CURAENGINE` should be the full path to `CuraEngine.exe`. If it is unset, Shale looks for `CuraEngine.exe` in an `UltiMaker Cura*` folder under Program Files and loads DLLs from that folder. Paths are passed as separate arguments, so a space in `C:\Program Files` does not need extra quotes.

Fetch the printer data from PowerShell (Git for Windows is required):

```powershell
.\scripts\fetch-resources.ps1
```

Cura 5.13 does not include a Bambu Lab A1 definition. The only `BambuLab` text in that tree is a g-code flavour name on the base printer. A1 profiles arrived in a later Cura release, so they are not in this pin.

`CuraEngine help` should print the AGPL banner and the `slice` usage before you bother with the window.

## Build and run Shale

Requirements: Rust stable (1.85 or newer is plenty), Node 22, and a C compiler. The desktop shell is Tauri 2, so the usual GTK / WebKit packages are required on Linux:

```bash
sudo apt install libwebkit2gtk-4.1-dev libgtk-3-dev libayatana-appindicator3-dev \
  librsvg2-dev patchelf libsoup-3.0-dev build-essential
```

```bash
./scripts/fetch-resources.sh
cargo test --workspace          # core tests; the shell compiles too
cd apps/desktop
npm install
npm run build                   # or: npm run tauri dev
```

On Windows the same fetch is `scripts\fetch-resources.ps1`. With no `SHALE_CURA_DIR`, Shale also looks next to the executable, in `vendor/` above the working directory, and in an installed UltiMaker Cura 5.x tree. If nothing matches, the window lists every path it tried instead of showing an empty printer menu.

Large STL files are read on a background thread. A binary STL is a straight scan of 50-byte records, and the window only receives a decimated preview (at most 48,000 triangles, as one flat buffer). A banner shows progress and a cancel button. Measured on this machine: 1,000,000 triangles in 0.61 s (48 MB of mesh, resident memory 6 MB to 53 MB) and 5,000,000 triangles in 3.31 s (240 MB of mesh, resident memory 7 MB to 236 MB). A sphere STEP at a 0.01 mm chord (tighter than the default) produced 5,224 triangles in 258 ms, with the surface 0.020 mm from the chords.

`npm run tauri dev` starts Vite and the window. Import `examples/cube.stl` if you want a known-good 20 mm cube.

An optional launch for screenshots and smoke tests, not required for normal use:

```bash
SHALE_CURAENGINE=/path/to/CuraEngine \
SHALE_DEMO_MODEL=examples/cube.stl \
SHALE_DEMO_SLICE=1 \
  ./target/debug/shale-desktop
```

`SHALE_DEMO_SLICE=1` slices before the window opens, so the layer scrubber is already filled. That launch also stores a sample filament-change pause at layer 40, which shows up as a `+` on the layer track.

## MCP server

`shale-mcp` speaks the Model Context Protocol. Stdio is the default. `--http` serves streamable HTTP and refuses any address that is not loopback.

```bash
cargo build -p shale-mcp
./target/debug/shale-mcp
# or
./target/debug/shale-mcp --http 127.0.0.1:7337
```

The HTTP endpoint is `http://127.0.0.1:7337/mcp`. A client that is not on this machine cannot be bound.

Tools call `shale_core::Session`: `list_printers`, `list_materials`, `list_profiles`, `load_model`, `transform_model`, `arrange_model`, `get_settings`, `set_settings`, `list_instructions`, `add_instruction`, `remove_instruction`, `slice`, `slice_summary`, `export_gcode`. `get_settings` returns the resolved value plus the definition's label and description when Cura wrote them. A `shale://job` resource reads the current models, instructions, and last summary. The `slice_a_model` prompt is a short checklist for an agent.

### Claude Desktop

In Claude Desktop's config (`claude_desktop_config.json`):

```json
{
  "mcpServers": {
    "shale": {
      "command": "/absolute/path/to/shale-mcp",
      "env": {
        "SHALE_CURAENGINE": "/absolute/path/to/CuraEngine",
        "SHALE_CURA_DIR": "/absolute/path/to/vendor/Cura",
        "SHALE_MATERIALS_DIR": "/absolute/path/to/vendor/fdm_materials"
      }
    }
  }
}
```

Restart Claude Desktop after saving. The server has no arguments for stdio; do not add `--http` here.

### Any other MCP client

A client that launches a subprocess and speaks MCP on stdin/stdout uses the same command and environment. Hermes Agent can use that stdio shape. A client that wants HTTP instead runs `shale-mcp --http 127.0.0.1:7337` itself and connects to `http://127.0.0.1:7337/mcp`. Binding `0.0.0.0` is rejected.

## Custom instructions

Instructions are data on the job, not hand-edited G-code. The window edits them in the instructions panel. The layer track draws a `+` where one applies. The MCP server adds the same objects with `add_instruction`.

Kinds:

| Kind | What you say | G-code |
| --- | --- | --- |
| pause | layer 40, or 12.4 mm, optional filament change | Marlin `M600` or `M0`. RepRap `M226` (there is no standard `M600`). Klipper `PAUSE`, or `M600` when it is a filament change |
| temperature | from layer N, nozzle and/or bed, optional wait | `M104` / `M140`, plus `M109` / `M190` when waiting. Later engine setpoints on those heaters are rewritten |
| fan | X% from height Y | `M106` / `M107`. Later engine fan commands are rewritten |
| beep | at a layer | `M300` |
| message | at a layer | `M117` |
| park and wait | at a layer | travel to the park point (default 5, 5), pause, travel back |

Layer numbers are Cura's `;LAYER:` index, which starts at 0. A height picks the first layer whose Z reaches it. The block is inserted at the start of that layer, after CuraEngine has finished. The engine binary is not modified, and its plugin slot is not used.

Klipper's `PAUSE` and `M600` only do what the printer's macros say. RepRap gets a comment that a filament change is a plain pause.

## Smarter supports

Shale decides where support is worth printing, then hands CuraEngine ordinary meshes. The engine binary is not modified and its plugin slot is not used.

The analysis rasterises the mesh. A face needs support when its angle from the vertical is past the overhang angle (Cura's `support_angle`: 0 supports every underside, 90 supports none). Spans held at both ends, up to a bridge length, are bridges and are left alone. Spans held at one end are cantilevers. Floating pieces are islands. A large face only just past the angle is treated as cosmetic. `analyse` returns that map; a later non-planar placement can fit a plane to the same samples.

`propose` turns the map into volumes:

* Enforcers (`support_mesh=true`, `support_mesh_drop_down=false`) are columns under cantilevers, islands, and other real overhangs.
* Blockers (`anti_overhang_mesh=true`) cover bridges and cosmetic slopes.

The 5.13 CLI applies `-s` to the object loaded by the previous `-l`, so each volume is a separate STL with those two settings. Automatic everywhere support is turned off for that slice, otherwise Cura would fill the bridges back in.

The window draws enforcers as flat grey and blockers as wire boxes. Each region can be switched off. The panel compares the proposal with supporting every overhang: volume, contact area, and an estimated time. **Compare** slices both ways and replaces the estimate with the engine's time and support filament.

Cura 5.14 stores paint-on-support in a 3MF PNG, with a 2-bit `support` channel in the image metadata (enforce, block, or automatic). That is the right long-term way to round-trip a painted region, and it wants the Arcus slice path plus a 3MF reader that keeps UV and the paint image. The 5.13 CLI does not read that channel, so this version stays with support meshes and blockers.

MCP tools: `analyse_overhangs`, `propose_supports`, `apply_supports`.

## Print removal

Not built. `docs/print-removal.md` is the note for a later camera check (Moonraker or OctoPrint webcam) and a user-written sweep. `shale_core::removal` is a stub: it stays off unless `SHALE_PRINT_REMOVAL=1`, and even then it does not open a camera or send a move. `will_move_printer` is false.

## Tests

```bash
cargo test --workspace
```

The resolver tests use a small fixture printer and, when `SHALE_CURA_DIR` / `SHALE_MATERIALS_DIR` are set, the real Ender-3 and Prusa i3 profiles, plus a parse of every formula in the 5.13 tree. `tests/golden/resolved.json` is the committed comparison for three printers. `SHALE_URANIUM_ROOT` (an extracted Cura AppImage, not a copy in this repo) also runs those formulas through Uranium's `SettingFunction`. A slice test, including the Arcus socket, runs only when `SHALE_CURAENGINE` is set, so CI does not need the engine binary.

GitHub Actions builds the frontend and runs `cargo test --workspace` on Ubuntu, including the Tauri shell.

## G-code the CLI leaves unfinished

CuraEngine's command-line header sometimes still contains `;TIME:6666`. Shale replaces that with `;TIME_ELAPSED` from the same file (or the engine log, when that line is present) and fills `;Filament used: 0m` from the extrusion commands. The toolpath, temperatures, and start G-code are the engine's.

Arcs (`G2` / `G3`) are not expanded in the preview; Cura's default toolpaths are `G0` / `G1`.
