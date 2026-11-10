# Shale v0.1 — status

Batch 6 was the last one: what follows is the state of v0.1 as a whole, with the
batches below kept because each records a decision and the numbers behind it.

Branch `cursor/deep-field-75ee`. Nothing was pushed, merged, tagged, released or
published; no PR was opened or changed. The A1 at 192.168.1.11 was never
contacted and no network was scanned — the fake printer binds to 127.0.0.1.

Workspace at this commit: **188 passed, 0 failed, 3 ignored** — the same count
with `SHALE_CURA_DIR` set, which is the only way the resolver golden actually
runs, and it now passes — and
`cargo clippy --workspace --all-targets -- -D warnings` is clean, also with
`-p shale-host --features fake-printer`. Batch 3 closed at 182, Batch 4 at 186;
Batch 5 adds the two slicer tests in `tests/boolean_to_gcode.rs`; Batch 6
changes no code, only the golden's two stale values.
The three ignored tests are the two boolean timing ladders (they run for
minutes on purpose) and the fake-printer wiring test (it needs a running
double).

## Batch 6 item

One open question, settled from Cura's own files rather than by preference: the
resolver's golden disagreed with the resolver on `creality_ender3`
(`material_bed_temperature` 50 against 60; `retraction_amount` 5 against 6.5).

**Ground truth: 60 and 6.5.** `fdmprinter.def.json:2809-2826` defaults the bed
to 60 and `:2827-2842` makes the first layer follow it; `creality_base.def.json`
overrides neither; the draft quality for this printer
(`quality/creality/base/base_global_draft.inst.cfg`) sets `layer_height = 0.32`
and no temperature; and `materials/generic_pla.xml.fdm_material:47` carries
`heated bed temperature = 60`, which `material.rs:233` and `resolver.rs:646-647`
already translate. `retraction_amount` defaults to 6.5 at
`fdmprinter.def.json:4695`, and no Creality quality file sets it. The stale 50
and 5 match the per-Ultimaker-machine blocks in the same material file
(`:181`, `:218`, `:94`), which is not this printer. The walk is written out with
line references in `docs/verification.md`.

**The golden moved, not the resolver.** Regenerated with `SHALE_WRITE_GOLDEN=1`
against the same Cura 5.13.0 resources: two lines changed, and `prusa_i3` and
`ultimaker2` came out identical, so the other sample printers do not share the
fault. No code changed, so the installer was not rebuilt and clippy was not
re-run; `cargo test --workspace` is **188/0/3 both with and without
`SHALE_CURA_DIR`**, which is the first time the golden has actually run in the
suite.

## Batch 5 items

The last batch, and the shortest: carry the new engine's output through the real
slicer, check that the installer's notices cover what it links, and leave this
page reading as one status rather than five.

| # | Item | Commit | State |
|---|---|---|---|
| 1 | A Manifold part through CuraEngine, for the A1 | Batch 5 tip | `tests/boolean_to_gcode.rs`: 50 layers, 0.434 m, 5.9 min, `;===== machine:` at line 14, bounds −5…5 mm; the 32 448-triangle part slices to 50 layers, 0.439 m |
| 2 | The notices cover what is statically linked | Batch 5 tip | oneTBB corrected to **2022.3.0** and **Clipper2 2.0.1** (BSL-1.0) added; `Clipper2.lib` and `tbb12.lib` are inside `shale-desktop.exe` |
| 3 | This page reads as one v0.1 status | Batch 5 tip | retitled, counts at the top, a "what to test by hand" section, Open list re-read |
| 4 | Re-test only because the code changed | Batch 5 tip | the print-time fix below is code, so tests, both clippy runs, the release build and the installer were all re-run |

### 1. The part the engine made, sliced

Two tests in `crates/shale-core/tests/boolean_to_gcode.rs`, both skipping
quietly when Cura is not on the machine:

* a 10 mm cube with a 4 mm bore — the part the watertightness test already knows
  the volume of — slices to **50 layers, 0.434 m of filament, 5.9 minutes**,
  a 245 298-byte file whose `;===== machine:` line is **14**, with the plate's
  own start and end;
* the brief's **32 448-triangle** subdivided box with the same bore slices to
  **50 layers, 0.439 m, 6.0 minutes**, 208 339 bytes.

Both assert the marker is inside the first forty lines **and before any
command**, which is the gate `shale-host` applies before it will send a file —
so what the host would refuse is checked here, on the file, not assumed from the
mesh. Layer count is the height over the layer height, and the filament length
is measured from the file's own extrusion, not from the engine's claim.

Two real defects came out of writing them, both now fixed:

* **The part was sliced 128 mm off the corner.** `job::slice` bakes the model's
  transform and then subtracts half the bed; a part handed in at the origin is
  not at the plate's middle. The first run produced `;MINX:-133 ;MAXX:-123`. The
  test now places the part the way the app does (`place_on_bed`) and asserts the
  bounds are −5…5 mm, so the corner cannot come back unnoticed.
* **"No estimate" was reported for a file that had one, and then 111 minutes.**
  CuraEngine writes `;TIME:6666` when it has not worked the time out, and writes
  the real figure into the file header when it has. `job::slice` reported only
  the engine's own `Option`, which was `None` here, and a first attempt at
  reading the file back took `6666` at face value — 111.1 minutes for a 10 mm
  cube. `gcode::header_facts` now rejects the placeholder, and the summary is
  read back from the file *after* `annotate_header` has filled it from the last
  `;TIME_ELAPSED:`. The test asserts minutes, so a placeholder cannot pass
  again.

### 2. What the installer links

`manifold-csg-sys` fetches three things with CMake and links all three
statically: Manifold 3.5.3 (Apache-2.0), oneTBB (Apache-2.0) and Clipper2
(BSL-1.0). The notices page had oneTBB at **2021.13.0**, which is not what is
built — the tag is **v2022.3.0** — and did not mention Clipper2 at all, although
`Clipper2.lib` is compiled and linked (the `CrossSection` engine comes with
Manifold whether or not Shale asks for 2D work). Both are corrected in
`THIRD-PARTY-NOTICES.md`, with the BSL-1.0 text added in full, and the built
artefacts confirm it: `tbb12.lib`, `tbbmalloc.lib`, `Clipper2.lib`, `manifold.lib`
and `manifoldc.lib` are all in the build's output and inside `shale-desktop.exe`.

### 3. What to test by hand

Nothing below is claimed as done; each is something only a person at the machine
can settle.

1. **Install from `Shale_0.1.0_x64-setup.exe`** on Windows, on a machine that has
   never run Shale. Check that it starts, that `THIRD-PARTY-NOTICES.md` is beside
   `Shale.exe`, and that About shows `0.1.0`.
2. **Open a `.shale` project, slice, and look at the preview.** The plate should
   be centred, the toolpaths should be visible in all three colour modes, and
   the estimate should be a number of minutes.
3. **A supervised A1 print.** Point Shale at the real printer's address and
   access code (Windows Credential Manager), accept the certificate on first
   sighting, then start one small part from the Device tab, watching the printer.
   This is the only step that exercises the network at all — everything in this
   repository was proved against the loopback double, and nothing was sent to
   192.168.1.11 by rule.
4. **Kill Shale while a window is open, then start it again.** The second launch
   should bring the existing window forward rather than fail.

## Batch 4 items

| # | Item | Commit | State |
|---|---|---|---|
| 1 | Evaluate, then integrate a mesh-boolean library | `40850f0` | Manifold 3.5.3 (Apache-2.0) through `manifold3d` 0.4.1; builds here in 3m16s; the BSP is now the fallback |
| 2 | Watertight on the existing cases, library first | `40850f0` | **0 open edges** at 7 800 / 32 500 / 79 200 triangles, counted by index; the 30 k convex case is a test |
| 3 | Measure, raise the cap, tell the truth | `40850f0` | 37.5 / 83.8 / 180.1 ms; `MANIFOLD_CAP` 200 000, `BSP_CAP` 12 000; the refusal sentence no longer says "minutes" |
| 4 | Final check | this page | **186/0/3** at `50434eb`; installer **4 977 093 B** built from the same code |

### 1. A proven engine instead of a new one

The brief's first choice was Manifold through a maintained Rust binding, and it
was the first choice here too. `manifold3d` 0.4.1 wraps `manifold-csg` 0.4.1,
which wraps `manifold-csg-sys` 3.5.105: three crates, all **Apache-2.0 OR MIT**,
compiling the pinned upstream source of **Manifold v3.5.3** (Apache-2.0,
Copyright Emmett Lalish and the Manifold contributors), oneTBB 2022.3.0
(Apache-2.0) and Clipper2 at the commit Manifold pins, twelve after the 2.0.1
tag (BSL-1.0). All three are linked statically into `shale-desktop.exe`.

It built on this machine with the toolchain already here — `rustc 1.99.0`
(MSVC), Visual Studio 2019 Build Tools, CMake 4.3.1 — in **3 m 16 s** from a cold
target directory, well inside the half hour the brief allowed, so no pure-Rust
substitute was needed. One Windows fact is worth recording because it will bite
the next person: `manifold-csg-sys` fetches oneTBB and Clipper2 through CMake's
`FetchContent`, and the paths inside TBB's `rfcs/` directory are longer than
`MAX_PATH`. The clone fails with `Filename too long` unless git is allowed long
paths (`git config --global core.longpaths true`, or `GIT_CONFIG_*` in the
environment of the build). Nothing else was needed.

The rejected option, recorded so the decision is auditable: `csgrs` 0.20.1 is
pure Rust and permissively licensed, but it depends on `bevy_asset` and a dozen
CAD-format readers — pulling a game engine into a slicer's core to avoid a C++
dependency would have been the worse trade.

### 2. Watertight, and what the number means

Manifold guarantees 2-manifold output, and the measurements agree. The ladder is
the same one Batch 3 ran, on the same lattice spheres:

| triangles | BSP (Batch 3) | Manifold | open edges, by index | by position |
|---|---|---|---|---|
| 7 800 | 2 914 ms | **37.5 ms** | **0** | 0 |
| 32 500 | 141 181 ms | **83.8 ms** | **0** | 12 |
| 79 200 | 1 601 108 ms | **180.1 ms** | **0** | 50 |

The position column is the artefact that had to be understood before it could be
quoted. `open_edges` exists because the BSP returns a polygon soup — its
triangles carry their own copies of the corners, so position is the only handle
on "the same corner" — and it rounds to a thousandth of a millimetre. Manifold
returns an indexed mesh, where two corners are the same corner because they are
the same index, and it keeps vertices distinct that are a ten-thousandth of a
millimetre apart. Rounding those to the same key makes an edge look unshared.
`open_edges_topological` asks the index-level question, which is the one that
matters for a watertight surface, and gets **0** at every size.

The volumes are unchanged to the cubic millimetre (10 559 / 10 569 / 10 571), so
none of the speed came from a coarser answer.

The brief's case is now a test rather than a stopwatch:
`a_thirty_thousand_triangle_convex_subtract_is_watertight` builds a 32 448-
triangle subdivided box, bores a 64-sided hole through it, and asserts the
engine is Manifold, the volume is **874.538 mm³** against the arithmetic
874.538, and the open edges are **0** — in 0.10 s in a debug build.

### 3. The cap, and the sentence that replaces "minutes"

Two limits, because they bound different things:

* `MANIFOLD_CAP = 200 000` — what the window will hand the library. It is a
  guard against a mistake in the plate (a boolean asked for inside a loop, a
  plate of a thousand copies), not a polygon budget: 32 448 triangles take a
  tenth of a second.
* `BSP_CAP = 12 000` — what the fallback will take. That one is about time, and
  it is unchanged, because the BSP has not become any faster.

So the refusal sentence lost its "minutes" claim and now says what it is: a
boolean on N triangles is more than Shale will attempt at once, the engine is
given at most 200 000, that is not a memory limit, and it is the size at which a
boolean is more likely a mistake in the plate than a part. The unit test that
pins the sentence now also forbids the word "minutes", for the same reason the
Batch 3 test forbade "memory": a sentence that outlives its measurement is a
lie with a timestamp.

### 4. Final check

Every commit of Batch 4, and the numbers at the tip:

| commit | what |
|---|---|
| `40850f0` | booleans through Manifold; caps; notices; installer ships them |
| `50434eb` | the refusal says bytes, not memory; `as_chunks`; the two things the first green run caught |
| this page | the Batch 4 section, `docs/verification.md`, and the manifest facts above |

* `cargo test --workspace --locked` — **186 passed, 0 failed, 3 ignored**
  (Batch 3's 182, plus the three round-trip tests in `manifold_csg.rs` and the
  30 000-triangle case)
* `cargo clippy --workspace --all-targets --locked -- -D warnings` — clean
* the same with `-p shale-host --features fake-printer` — clean
* `npm --prefix apps/desktop run build` — clean
* `cargo build --workspace --release --locked` — clean
* `npm run tauri build` — installer **4 977 093 B** (4.75 MiB), built from
  the code now at `50434eb`; `shale-desktop.exe` **19 180 032 B**, `shale-mcp.exe` **11 016 192 B**.
  The installer was opened with 7-Zip afterwards: `THIRD-PARTY-NOTICES.md` is
  inside it, at the install root.

## Batch 3 items

| # | Item | Commit | State |
|---|---|---|---|
| 1 | The surviving WebView2 crash | `f92c3c5` | done; the box says "an earlier Shale is still closing", one retry, exit 0 |
| 2 | The boolean wall, timeboxed | `c52172c` | moved again, not cracked — 1.5× at 32 500, cap stays 12 000, refusal sentence rewritten |
| 3 | Clean-slate pass | `cc9bc4c` | done; two reachable panics and two untrue strings fixed, the rest listed with reasons |
| 4 | Final check | this page | 182/0/3 at `14b9561`; clippy clean twice; installer 4 674 836 B built from `14b9561` |

### 1. The window that could not be made

Batch 2's Open item 4 is closed. `run()` used to call `Builder::run`, which
panics with a bare HRESULT when the window cannot be made; it now calls
`builder.build(...)` and matches on the error, so the failure is a value with a
sentence attached to it. An in-use refusal (`HRESULT(0x800700AA)`, WebView2
holding `EBWebView` for a process that has not finished exiting) is retried
**once** as a fresh process 1.5 s later — the mutex is released first so the child
can take it, and the child carries `SHALE_START_RETRIED=1` so it cannot retry in
turn. If that also fails, a native box says, in plain words, that an earlier Shale
is still closing and to try again in a moment; the process then exits 0.

The refusal itself could not be provoked on demand tonight: killing the app leaves
no orphan, a directly-launched `msedgewebview2` exits at once, holding
`EBWebView\Default\LOCK` exclusive does not block it, and pointing the profile at
a file does not either. So the branch is driven by `SHALE_START_FAILS` (a
diagnostic like `SHALE_TRACE`) and the box's caption and text were read out of the
live window — not from a screenshot — with the exit code checked after a
`WM_CLOSE`: caption `Shale did not start`, the Static control carrying the closing
sentence, exit 0. Three unit tests pin the wording, and the ordinary launch was
re-run afterwards — release binary, clean profile, window `Shale` visible at 9 s,
`WM_CLOSE` → exit 0 — so the fix did not buy the sentence by breaking the road to
normal startup. `docs/verification.md` has the
transcripts and says plainly that the handling is proved while the provocation is
not.

### 2. The boolean wall, and the sentence it earns

The timebox went into the measured answer rather than the hoped-for one. The
brief's target was a bounding-volume hierarchy with exact clipping only inside
overlapping boxes, so that a 30 000-triangle convex part finishes in seconds. It
was scoped on paper and not landed: pruning the plane tests is half of a Boolean.
The other half is the cap surface — B's skin inside A — which needs a second BSP
or a ray-cast inside test, and whatever it produces has to stay watertight at the
T-junctions pruned clipping leaves behind. That is a new Boolean engine with its
own correctness proof, not a change of this one, and the tree build is already
fast (0.3 s at 32 500): every second after it is clipping and polygon growth.

What did land is a real, measured win. `split_polygon` collected a `Vec<Side>` for
every polygon at every plane — at 32 500 triangles that is a polygon through 135
levels, millions of allocations for three bytes each. It now computes the flag it
needs and re-reads a side per edge when it has to cut.

| triangles | tree (before → after) | subtract (before → after) |
|---|---|---|
| 7 800 | 67 → 45 ms | 5 114 → **2 914 ms** (1.75×) |
| 32 500 | 366 → 300 ms | 215 565 → **141 181 ms** (1.53×) |
| 79 200 | 924 → 673 ms | 1 601 108 ms (not run before; 27 minutes) |

So `BOOLEAN_CAP` stays 12 000. Two and a quarter minutes at 32 500 is not
"seconds", and the climb is steep enough that the cap is already optimistic on a
convex body. The concave parts people actually make stay fast: a 1 946-triangle
bracket booleans in 0.01 s and a 2 554-triangle plate with a bore in 57 ms.

The refusal now tells the truth in one sentence, in both places it can be raised:
the triangle count, that a boolean there would take minutes, the cap, and what
still works (a plain cut). It used to say the cap was what "the boolean can hold
in memory" and, at the other call site, that the boolean was "only proven on small
parts" — one untrue, the other useless. A unit test holds the wording and rejects
the word "memory". The item stays Open with the numbers above.

### 3. A clean-slate pass

Nothing to fix in three places, which is worth writing down: no `TODO`, `FIXME`,
`XXX`, `unimplemented!` or `todo!()` anywhere in the tree; no `unwrap`/`expect`
outside tests in the desktop crate or the MCP server; and `with_big_stack` already
answers `Err` when its thread cannot start or when the job panics, so a cut that
dies says so instead of taking the window with it.

Fixed: an `unreachable!` in `formula.rs` for `and`/`or` reaching the arithmetic
evaluator (a profile's formula is data; it now answers with an error naming the
lost short-circuit); an `unwrap()` on a positional expression while parsing a
comprehension; `self.plane.unwrap()` in `csg.rs` became an `expect` that names its
invariant; and the `App.tsx` comment that still claimed the catalogue takes
"twenty-odd seconds" (it is 1.79 s in release) now says a couple of seconds.

Added: `crates/shale-core/tests/garbage_inputs.rs`, five tests that feed empty,
truncated, ASCII-with-no-facets, non-zip and junk-STEP files to `load_stl`,
`load_mesh`, `read_project` and `import_step` and require `Err` rather than a
panic. All five passed on the first run — the loaders were already honest, and now
they cannot quietly stop being.

Left, with reasons: the `try_into().unwrap()` calls in `proto.rs` and `mesh.rs`
convert fixed-length chunks and a four-byte slice of `[0u8; 84]`, so they cannot
fail and a `match` would be noise; `template.rs`'s `expect`s are guarded three
lines above; test modules keep their unwraps because a failing assertion is the
point; and `removal.rs` still reports `will_move_printer: false` and still sends
no motion, which is a gate rather than an unfinished string.

### 4. Final check

At the tip of Batch 3, built from `14b9561`: **182 passed, 0 failed, 3 ignored**,
`cargo clippy --workspace --all-targets -- -D warnings` clean, the same for
`shale-host` with `--features fake-printer`, `npm run build` clean,
`cargo build --workspace --release --locked` clean, and
`npm run tauri build` produced `Shale_0.1.0_x64-setup.exe` at **4 674 836 bytes**
(4.46 MiB), with `shale-desktop.exe` at **17 874 944** and `shale-mcp.exe` at
**10 735 104**.

Every commit of Batch 3, in order:

| commit | what |
|---|---|
| `f92c3c5` | the window that could not be made says why, once, then exits 0 (item 1) |
| `906e03e` | the status page names that commit |
| `c52172c` | the boolean stops allocating a list of sides; refusal sentence rewritten; five garbage-input tests (item 2) |
| `cc9bc4c` | clean-slate pass: two panics and two untrue strings fixed, the rest listed (item 3) |
| `14b9561` | the `Side` enum the boolean no longer collects |
| `30d547b` and the docs commits after it | this page and `docs/verification.md` (item 4) — nothing after `14b9561` changes code |

Nothing was pushed, merged, tagged or published, and no PR was touched. The A1 at
192.168.1.11 was never contacted; no network was scanned; the fake printer binds
to 127.0.0.1. Fusion 360 was not touched.

## Batch 2 items (kept for reference)

| # | Item | Commit | State |
|---|---|---|---|
| 1 | Fake printer, rewritten and in the repo | `e739140` | done; four wiring tests, one command |
| 2 | TOFU certificate pinning | `fdd37c1`, `b16462b` | done; refusal and trust press proved in the window |
| 3 | The boolean wall | `5a54e5b` | moved, not cracked — cap stays 12 000 |
| 4 | End-to-end slice for the A1 | `905e8f1` | done on both surfaces; found and fixed a prelude defect |
| 5 | Release readiness | `26e2d75` | clippy clean, release build, installer; rebuilt five times as fixes landed, numbers below |
| 6 | Last UI pass | `626796e`, `84cc721`, `d8902a9`, `5f1236c`, `63a6a15` | two dishonest strings, a nameless About, a bounding box, an unwithdrawable pin and a silent second launch found and fixed; what is left is under Open |

### 1. A Bambu printer that exists only on loopback

The Python double Batch 1 could not find is gone for good; its replacement lives
in the repo as `crates/shale-host/tests/support/fake_printer.rs` (405 lines). It
speaks MQTT 3.1.1 over TLS for `push_status`, `project_file` (start, pause, stop)
and file upload, and implicit FTPS for `STOR`, on ephemeral ports bound to
127.0.0.1 so a test needs no administrator rights.

Three things had to move for it to be testable: `HostConfig` gained
`mqtt_port`/`ftps_port` (defaults 8883/990, tests use ephemeral ones), the
`;===== machine:` gate moved from the desktop crate into `shale-host` beside the
upload it protects, and `Machine::from_report` became the single mapping from a
`gcode_state` to the queue's five states. `cargo test -p shale-host --test wiring`
runs four tests in about 0.3 s: an A1 file uploads byte for byte and its
`gcode_file` command arrives on the request topic; a Cura-started file is
refused; RUNNING then FINISH parks the queue in `waiting_for_plate` until a
plate-cleared press; FAILED stops the run and keeps its error number.

### 2. The certificate is pinned on first sighting

Batch 1's host trusted the system roots, and a printer presents a self-signed
certificate, so nothing was checked. Now the first successful handshake records
the SHA-256 of the leaf in the same Windows keyring the access code uses
(`Shale/pin/<serial>`, falling back to the host, so two printers on one network do
not overwrite each other). A later mismatch becomes `HostError::Pin` and the link
stops rather than retrying: only a person can decide a changed certificate is
expected, and the device tab offers one press — "trust the new certificate" —
that forgets the pin so the next connection records what it is offered.

Both channels compare the same pin, because a file sent to an impostor hands over
the model, the settings and the access code at once. `upload` keeps the old
permissive behaviour for callers that hold no pin.

Proved twice. `cargo test -p shale-host --test pinning` does it in Rust against
two committed certificates, and the window does it against the same machine run
as a binary — `cargo run -p shale-host --features fake-printer --bin
shale-fake-printer -- --cert b`, on 127.0.0.1:8883 and :990, the ports the
Device tab's own connect uses. With `a` the first sighting recorded `61acc0ae…`;
restarted as `b`, the pane read "not connected — unexpected error: the printer's
certificate changed: this machine was pinned to 61acc0ae… and now presents
633d1a33…. If the printer was factory reset or its board replaced, that is
expected; otherwise something else is answering on its address.", with the block
and its one press below it; the press left the keyring at `633d1a33…` and the
link up. `docs/verification.md` carries the same run in full.

### 3. The boolean wall moved; the cap stays

`pick_plane` sampled every polygon against every candidate plane, which is
quadratic, and on a convex body every face plane scores the same: a face plane of
a convex hull leaves the whole body in front of it. It now reads at most 96
polygons, accepts a face plane only when that plane resolves a third of the set,
and otherwise cuts on an axis-aligned plane through the sample's median centroid,
charging a straddling polygon to both sides — a plane that bisects everything has
balanced nothing, it has doubled the mesh. Every operation also used to end by
rebuilding a tree from both halves and then walking it for the same polygons;
`merged()` concatenates and flips, which is all that step meant.

Measured in release:

| triangles | tree built | nodes | depth | subtract |
|---|---|---|---|---|
| 7 800 | 67 ms (was 5 090 ms) | 8 054 | 108 (was 4 264) | 5.1 s (was: never finished) |
| 32 500 | 366 ms | 34 670 | 135 | 215 s |
| 79 200 | 924 ms | 85 585 | 195 | not waited for |

**`BOOLEAN_CAP` stays at 12 000.** Three and a half minutes at 33 k is not
"seconds", and the wall changed shape rather than disappearing: it is polygon
growth now (a plane through a sphere cuts a third of its triangles, so the tree
holds 1.8x the input before a single clip), and clipping is charged per polygon
per level. Cracking it needs a bounding-volume hierarchy with exact clipping only
inside overlapping boxes, which never cuts a triangle that no boundary crosses.
That is its own piece of work and it is under Open.

### 4. A plate sliced for the A1, from both ends of the app

`Arm.3mf` (1 946 triangles) sliced for `bambu_lab_a1` / `0.4 mm` /
`0_20mm_standard_bbl_a1` / `bambu_lab_pla_basic`:

| surface | layers | filament | `machine_gcode` | bytes |
|---|---|---|---|---|
| `shale-mcp` over stdio | 54 | 2.1942 m | true | 515 971 |
| the window: `import_model` -> `slice_model` -> `export_gcode` | 54 | 2.1943 m | true | 522 285 |

Both files open with comments and then the A1's own `;===== machine: A1` block —
nothing heats or moves before it — contain no `{`, pass
`starts_with_machine_sequence`, and were uploaded byte for byte to the fake
printer (`522285 bytes of sliced G-code arrived at the fake printer`). The head
and the tail are quoted in `docs/verification.md`.

**This item found a real defect.** The first attempt was refused by Shale's own
gate: CuraEngine writes `M140`/`M190`/`M104`/`M109` *before* the start G-code it
is handed, so the file heated to 220 C before the A1's macro said how.
`bambu::strip_engine_prelude` now drops the engine's heat lines from a file that
carries a machine sequence — the template heats to standby, levels, then heats to
print temperature, which is the machine's order — and the gate's rule became
"comments may precede the banner, no command may". A Cura file that heats before
any banner is still refused, and both cases have tests.

### 5. Release readiness

- `cargo clippy --workspace --all-targets --locked -- -D warnings`: **clean**.
  Getting there was mechanical (`needless_borrow`, `unnecessary_cast`,
  `cloned_ref_to_slice_refs`, `trim_split_whitespace`,
  `chunks_exact_to_as_chunks`, `sort_by_key`, `while_let_loop`,
  `needless_range_loop`, `ptr_arg`, `field_reassign_with_default`) plus three
  allows, each with its reason beside it: `too_many_arguments` and
  `type_complexity` at the top of `shale-core` and the desktop crate, and
  `type_complexity` in the fake printer. A slice really is thirteen parameters,
  and the mesh code's tuples are `(vertex, normal, patch)`.
- Versions are **0.1.0** already everywhere (four crates, `package.json`,
  `tauri.conf.json`); nothing needed bumping, and nothing was tagged.
- The bundle and the installer path: see *Build* below.

### 6. Last UI pass

Driven page by page with CDP against a debug build, reading the DOM rather than
looking at a picture.

- **Theme.** Every element on Home, Prepare, Preview and Device was read for its
  computed `backgroundColor`. Nothing is light: the only element over a crude
  brightness test on Prepare, Preview and Device is `rgb(185, 179, 255)` — the
  accent span itself. Zero `✓` characters and zero pictographic glyphs in the
  body text; the menu hints are plain letters.
- **Star field** is `canvas.plate-field`, and it exists on Home only. Prepare and
  Preview carry an unclassed canvas — the viewport — and no field.
- **Menus** open on a real pointer press; a synthetic `.click()` does nothing,
  which is why the first probe saw nothing. File has 14 items, Edit 8, View 6,
  Printer is the machine list itself (101HERO onward, sorted), Slice settings has
  11 groups with real fields, and Slice is the accent button, not a menu.
- **The three pages behind the start menu**: Printers (655 machines, BACK, sort),
  Settings (backend `cli`, BINARY and LIBS fields, CHECK ENGINE), About. The
  recent list carried five projects with dates and machine names.

Five things were fixed tonight, the first two found by reading what the window
said, the third by reading a file header, the last two by driving the thing:

- `app_status` reported "Printer resources are loaded." for the whole of a cold
  catalogue read, with an empty printer list underneath, and `catalog()`'s
  comment promised a wait the code did not do. Both strings now describe the
  state they are in, and a command that arrives mid-read is told the profiles are
  still being read rather than that they are missing (`626796e`). The read itself
  turned out to be 4.1 s in a debug build and 1.8 s in release when the machine
  is idle; the half-minute first timed was measured while `cargo` was using every
  core. This page claimed 36 s for a while, and that was wrong.
- About credited a photograph and three fonts and never named the application or
  its version. It now reads "Shale 0.1.0 — a desktop slicer…", with the number
  taken from `env!("CARGO_PKG_VERSION")` so the window cannot drift from
  `Cargo.toml` (`84cc721`).
- Every file the slicer wrote claimed a bounding box two million millimetres
  wide: CuraEngine fills `;MINX`…`;MAXZ` with `f32::MAX` when it is handed bare
  STL files, and it is. `bambu::rewrite_header_bounds` now measures the meshes it
  just baked and replaces the six lines; a re-slice of the same model says
  `;-45.392 … 24.392` by `0 … 11`, which is the Arm (`d8902a9`). This one is
  item 4 territory as much as item 6's: it was found reading a header.
- A second launch died in the dark: WebView2 refuses to share the window
  profile, `create webview` fails with `HRESULT(0x800700AA)` and setup unwinds
  with no window and no sentence. A named mutex is claimed first now; the second
  process finds the window titled `Shale`, restores it, brings it forward and
  exits 0 (`729b074`). Measured: the second process left with exit 0, the first
  kept its window, and a launch after both had gone still opened.
- A pinned printer could never be forgotten: after a board swap or a typo the
  only way out was the Windows Credential Manager. The Device pane has a `forget`
  press now that drops the link, deletes the pin and the access code from the
  vault, and clears the address and serial the window kept (`5f1236c`).

Two things were also driven end to end rather than argued:

- **The .shale round trip**, on the Arm rather than a fixture: saved through
  `save_project` (26 515 bytes), read back as 1 946 triangles with the A1 job
  intact, and then reopened *in the window* by pressing its entry in the start
  screen's recent list — tab `roundtrip`, Plate 1, the part on the plate, the
  painted support patch still attached. That path needs no dialog, so it is the
  one a probe can take.
- **The new `forget` press**, with the pin from an earlier session still in the
  vault: the printer answered with a different certificate, was refused with
  both fingerprints named, and `forget` then emptied the three fields, both
  `localStorage` keys, `host_pin` and `device.code`; a reload came back empty.

Not re-driven tonight: the window's own import and Save As flows end in an OS file
dialog, which a probe cannot press. Calling `import_model` over CDP loads the model in
Rust and the plate stays empty, because the plate updates from the menu's own
handler — so the model-on-plate and sliced-preview screenshots remain Batch 1's,
and the files sliced through the window for item 4 were produced the same way,
through the commands the menu calls.

## Build

The release numbers are recorded in `docs/verification.md` under "The release
build", with the commands that produced them. Short version: clippy clean, the
first full `cargo build --workspace --release` in 4m 49s, and `npm run tauri build`
producing

    target/release/bundle/nsis/Shale_0.1.0_x64-setup.exe   4 675 254 B, 4.46 MiB

rebuilt after each fix, the last time at branch head. Versions are 0.1.0 in four
crates, `package.json` and `tauri.conf.json`; nothing was tagged, the installer
was never run, and it is unsigned.

Two build facts changed in Batch 4, both from compiling C++:

* **The first build now takes longer.** `manifold-csg-sys` clones the pinned
  Manifold source and oneTBB, then builds them with CMake: 3 m 16 s cold on this
  machine (Visual Studio 2019 Build Tools, CMake 4.3.1). After that it is a
  cached build script.
* **Git needs long paths on Windows.** TBB's `rfcs/` directory contains a path
  over `MAX_PATH`, and `FetchContent`'s clone fails with `Filename too long`
  without `core.longpaths`. Set it for the build
  (`git config --global core.longpaths true`) rather than for one shell, or the
  next clean checkout fails the same way.

The installer also ships `THIRD-PARTY-NOTICES.md`, which carries the versions and
licences of Manifold, its Rust bindings and the rest of the compiled-in
dependencies.

## Batch 1 (kept for reference)

Seven commits on top of `0b2e3e4`, closed at `b2e1234` with **157 passed, 0
failed, 2 ignored** — re-run and agreed by Shale Builder.

| # | Item | Commit | State |
|---|---|---|---|
| 1 | Overnight queue never starts itself | `7339374` | done |
| 2 | A1's own start and end G-code | `6ab5eb2` | done |
| 3 | Calibration: the end sequence is not a layer | `3fdc256` | done |
| 4 | Boolean timings at 8k / 33k / 80k | `33321d4` | measured where it finished; cap 12 000 |
| 5 | Docs reconciliation | `cd93b1b` | done |
| 6 | Theme leftovers | `6bc006c` | done |
| 7 | Keyring for the access code | `96b484c` | done; pinning followed in Batch 2 |
| 8 | Fake printer + wiring tests | Batch 2, item 1 | done there |

The queue refuses to start a second job without a plate-cleared press and the
refusal names the reason; the A1 profile keeps its start and end G-code as
templates rendered per nozzle with every `{name}` resolved or the slice fails
naming it; the layer-Z pass no longer reads `G1 E-2 Z0.2` from the end sequence as
a layer; the access code moved from `localStorage` into the Windows Credential
Manager (`Shale/<key>`); `LICENSING.md` names every library and carries the csg.js
attribution; the active plate chip is a 10 % accent tint with an accent underline,
measured in the window rather than by eye. OS file dialogs and the title bar are
not themed — they are the system's.

## Open

1. **The BSP fallback is still quadratic, and is held to 12 000 triangles.**
   The wall itself is gone: Manifold answers booleans at 32 500 in 84 ms and at
   79 200 in 180 ms, watertight. What remains slow is the path taken when
   Manifold refuses a mesh — holes, several shells that touch, an unresolvable
   self-intersection — because the old engine is still the old engine. Above
   12 000 triangles in that situation the window refuses rather than grind: the
   refusal names the limit. Closing this means either repairing non-manifold
   input before it reaches Manifold (its `MeshGL::merge` and `Simplify` cover
   some of it, not all) or replacing the fallback with a second exact engine,
   which is a bigger piece of work than v0.1 has room for.
2. **Not proved on hardware.** Every host claim — MQTT, FTPS upload, pinning, the
   `;===== machine:` gate end to end — is proved against the loopback double.
   Nothing has been sent to a real A1, by rule.
3. **Nothing is pushed.** `git push` cannot read a username in this shell
   (`terminal prompts disabled`) and both batches forbid it. Every commit is
   local; the remote tip is still `0b2e3e4`.
4. **The webview lock is handled but not provokable.** A `msedgewebview2` child
   that outlives a crash no longer kills the next launch silently: `build()`'s
   error becomes a box that says an earlier Shale is still closing, after one
   retry as a fresh process. What is unproved is the provocation — four ways of
   holding `%LOCALAPPDATA%\dev.shale.desktop\EBWebView` from outside all failed
   to reproduce it, so the branch is exercised through `SHALE_START_FAILS` and the
   sentence through unit tests. If the real refusal says something other than
   `0x800700AA`, the box quotes the error instead of guessing, which is the safe
   half of the same fix.
5. **Remaining Bambu parity is blocked outside Shale** — seam painting,
   per-filament flush volumes and multi-spool sliced 3MF need CuraEngine 5.13
   support it does not offer; camera and AMS mapping need a machine.
