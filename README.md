<p>
  <img src="docs/shale-lockup-dark.png" alt="Shale" width="284" />
</p>

Shale is a desktop 3D-printing slicer with its own interface: a dark, monospace window, box-drawing chrome, and a WebGL plate. Slicing is done by an unmodified [CuraEngine](https://github.com/Ultimaker/CuraEngine) process. Printer definitions, quality profiles, and materials are Cura's, read as data.

This is the first MVP:

1. Import an STL or a straightforward 3MF and show it on the build plate. Move, rotate, scale, and drop it onto the plate.
2. Pick a printer from Cura's definitions, plus layer height, infill density, supports, and nozzle / bed temperature.
3. Slice with CuraEngine, follow progress, then scrub a layer-by-layer toolpath preview.
4. Export the G-code.

Two more pieces sit on the same core:

5. An MCP server, so an agent can list printers, load a model, change settings, slice, and export G-code.
6. Custom instructions (a pause, a temperature change, a beep) stored on the job and compiled into the engine's G-code. You do not edit the file by hand.

Still out of scope: camera-based print removal, and automatic non-planar placement. The overhang map is shaped so a later non-planar pass can reuse bridge and cantilever spans.

## Layout

```
crates/shale-core     job model, profiles, formulas, meshes, engine boundary
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

### Why the command-line engine, not Arcus, for now

Cura's own frontend speaks [Arcus](https://github.com/Ultimaker/libArcus): protobuf messages from `Cura.proto`, framed as `uint32` header, `uint32` size, `uint32` type id, then the protobuf bytes. The type id is libArcus's private hash of the message's `GetTypeName()`, so both sides have to register the same messages or nothing decodes. CuraEngine is the client (`CuraEngine connect host:port`); the frontend listens, sends one `Slice` (settings plus raw meshes), and reads `Progress`, layer polygons, and `GCodeLayer` until `SlicingFinished`.

That is the right long-term path, and `shale_core::engine::SlicingEngine` is the seam for it. `ArcusEngine` is a stub that returns a clear error. The MVP uses `CliEngine` instead:

* CuraEngine 5.13's command-line `sendProgress` is empty, and `-p` is a leftover no-op, so the CLI cannot stream real percentages. Shale estimates progress from the engine's log (`layer N of M` during insets, skin, and infill, then `starting export`) and from `;LAYER:` lines as they are flushed, and reads `;TIME_ELAPSED:` from the finished file. Arcus `Progress` messages are the proper stream. The engine is not forked to get that.
* A CLI-only binary is the practical one to build and to point Shale at. The AGPL boundary stays obvious: we never link the engine.
* The layer preview the MVP needs is already in the G-code CuraEngine writes (`;LAYER:` and `;TYPE:`). Arcus polygons can replace that parser later without a UI change.

The engine only reads `default_value` from definition JSON. Cura's `value` formulas are Python, evaluated by Uranium in the official app and then sent across as literals. Shale resolves the stack itself and passes a `-r` JSON file (`global`, `extruder.0`, …). `{setting}` placeholders in start and end G-code are filled in first, because the engine writes those strings through unchanged.

## Setting resolution

Shale does not depend on Uranium. The resolver walks definition inheritance (`fdmprinter` → `creality_base` → `creality_ender3`, and the extruder train), then applies the nozzle variant, a matching material from fdm_materials, the quality profile, an optional intent, and the handful of values from the window. Formulas are a Rust subset of the expression language: arithmetic, comparisons, `and` / `or` / `not`, `a if c else b`, lists, `min` / `max` / `round` / `math.*`, and the Cura helpers `extruderValue`, `extruderValues`, `resolveOrValue`, and `anyExtruderWithMaterial`.

A formula that fails falls back to `default_value` and is counted as a warning. On Cura 5.13 the Creality Ender-3 and the Prusa i3 resolve the settings a normal slice needs (bed size, line width, infill spacing, temperatures, speeds, adhesion, start G-code) with no warnings for those keys. Known gaps, left as TODOs in `crates/shale-core/src/formula.rs`:

* `min` / `max` ignore the `key=` argument. With one extruder the list has one element, so the result is that element. Multi-extruder adhesion ordering is wrong until `key=` is implemented.
* No lambdas, comprehensions, or attribute calls such as `('raft', 'brim').index`.
* Intents are loaded and can be passed on `ResolveRequest.intent_category`. The window does not expose them yet.
* Quality matching is by definition ancestry, quality type, variant name, and material id. It is not a full copy of Cura's quality-node tree.

Creality's preferred quality type in 5.13 is `standard`, and that profile does not itself set a layer height. The window reloads the resolved layer height when you change quality; set 0.2 mm yourself for a typical draft.

## Fetch the printer data

Cura 5.13.0 and fdm_materials are downloaded, not copied into git. Their licence files land next to the data.

```bash
chmod +x scripts/fetch-resources.sh
./scripts/fetch-resources.sh
```

That creates `vendor/Cura` and `vendor/fdm_materials`. Override the location with `SHALE_CURA_DIR` and `SHALE_MATERIALS_DIR` if you already have a checkout. See [LICENSING.md](LICENSING.md).

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

A release build links libArcus even though Shale's MVP does not open the socket. That is upstream's build; leave the binary unmodified.

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

`SHALE_CURAENGINE` should be the full path to `CuraEngine.exe`. Shale prepends `SHALE_CURAENGINE_LIB_DIR` to `PATH` on Windows so accompanying DLLs are found.

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

## Tests

```bash
cargo test --workspace
```

The resolver tests use a small fixture printer and, when `SHALE_CURA_DIR` / `SHALE_MATERIALS_DIR` are set, the real Ender-3 and Prusa i3 profiles. A slice test runs only when `SHALE_CURAENGINE` is set as well, so CI does not need the engine binary.

GitHub Actions builds the frontend and runs `cargo test --workspace` on Ubuntu, including the Tauri shell.

## G-code the CLI leaves unfinished

CuraEngine's command-line header sometimes still contains `;TIME:6666`. Shale replaces that with `;TIME_ELAPSED` from the same file (or the engine log, when that line is present) and fills `;Filament used: 0m` from the extrusion commands. The toolpath, temperatures, and start G-code are the engine's.

Arcs (`G2` / `G3`) are not expanded in the preview; Cura's default toolpaths are `G0` / `G1`.
