<p>
  <img src="docs/shale-lockup-dark.png" alt="Shale" width="284" />
</p>

Shale is a desktop 3D-printing slicer with its own interface: a dark, monospace window, box-drawing chrome, and a WebGL plate. Slicing is done by an unmodified [CuraEngine](https://github.com/Ultimaker/CuraEngine) process. Printer definitions, quality profiles, and materials are Cura's, read as data.

This is the first MVP:

1. Import an STL or a straightforward 3MF and show it on the build plate. Move, rotate, scale, and drop it onto the plate.
2. Pick a printer from Cura's definitions, plus layer height, infill density, supports, and nozzle / bed temperature.
3. Slice with CuraEngine, follow progress, then scrub a layer-by-layer toolpath preview.
4. Export the G-code.

Out of scope for now, but the layout leaves room for them: an MCP server on the core library, custom instructions that are not G-code, smarter supports, camera-based print removal, and automatic non-planar placement.

## Layout

```
crates/shale-core     job model, profiles, formulas, meshes, engine boundary
apps/desktop          Tauri 2 shell and the React / three.js view
scripts/fetch-resources.sh
vendor/               created by the script; not in git
```

`shale-core` is an ordinary Rust library. The Tauri app is a client of it. A later MCP server can call the same library without going through the window.

```
UI  →  shale-core  →  CuraEngine (separate process)
                 ↘  Cura resources + fdm_materials (data files)
```

### Why the command-line engine, not Arcus, for now

Cura's own frontend speaks [Arcus](https://github.com/Ultimaker/libArcus): protobuf messages from `Cura.proto`, framed as `uint32` header, `uint32` size, `uint32` type id, then the protobuf bytes. The type id is libArcus's private hash of the message's `GetTypeName()`, so both sides have to register the same messages or nothing decodes. CuraEngine is the client (`CuraEngine connect host:port`); the frontend listens, sends one `Slice` (settings plus raw meshes), and reads `Progress`, layer polygons, and `GCodeLayer` until `SlicingFinished`.

That is the right long-term path, and `shale_core::engine::SlicingEngine` is the seam for it. `ArcusEngine` is a stub that returns a clear error. The MVP uses `CliEngine` instead:

* CuraEngine 5.13's command-line `sendProgress` is empty, and `-p` is a leftover no-op, so the CLI cannot stream real percentages. Shale estimates progress by counting `;LAYER:` lines as they are flushed, and reads `;TIME_ELAPSED:` from the finished file. Arcus `Progress` messages are the proper stream.
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

`SHALE_DEMO_SLICE=1` slices before the window opens, so the layer scrubber is already filled.

## Tests

```bash
cargo test --workspace
```

The resolver tests use a small fixture printer and, when `SHALE_CURA_DIR` / `SHALE_MATERIALS_DIR` are set, the real Ender-3 and Prusa i3 profiles. A slice test runs only when `SHALE_CURAENGINE` is set as well, so CI does not need the engine binary.

GitHub Actions builds the frontend and runs `cargo test --workspace` on Ubuntu, including the Tauri shell.

## G-code the CLI leaves unfinished

CuraEngine's command-line header sometimes still contains `;TIME:6666`. Shale replaces that with `;TIME_ELAPSED` from the same file (or the engine log, when that line is present) and fills `;Filament used: 0m` from the extrusion commands. The toolpath, temperatures, and start G-code are the engine's.

Arcs (`G2` / `G3`) are not expanded in the preview; Cura's default toolpaths are `G0` / `G1`.
