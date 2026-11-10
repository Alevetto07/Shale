# Shale v0.1 — Batch 2 status

Branch `cursor/deep-field-75ee`. Nothing was pushed, merged, tagged, released or
published; no PR was opened or changed. The A1 at 192.168.1.11 was never
contacted and no network was scanned — the fake printer binds to 127.0.0.1.

Workspace at this commit: **173 passed, 0 failed, 3 ignored**, and
`cargo clippy --workspace --all-targets -- -D warnings` is clean. The run was at
`729b074`; everything after it changes only this page and `docs/verification.md`. The three
ignored tests are two stopwatches (`boolean_timings_…`,
`a_curved_part_does_not_become_a_chain`) and one check that takes a file
(`a_sliced_file_reaches_the_fake_printer`, which reads `SHALE_SLICED_GCODE`).

## Batch 2 items

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

1. **The boolean wall is bounded, not cracked.** `BOOLEAN_CAP` = 12 000; a convex
   body above it needs a bounding-volume hierarchy with exact clipping inside
   overlapping boxes. Until then a 30 k-triangle convex part takes minutes if it
   is attempted at all, which the cap prevents.
2. **Not proved on hardware.** Every host claim — MQTT, FTPS upload, pinning, the
   `;===== machine:` gate end to end — is proved against the loopback double.
   Nothing has been sent to a real A1, by rule.
3. **Nothing is pushed.** `git push` cannot read a username in this shell
   (`terminal prompts disabled`) and both batches forbid it. Every commit is
   local; the remote tip is still `0b2e3e4`.
4. **A webview that outlives a crash still blocks the next launch.** The
   single-instance guard covers the second *Shale process*; it cannot cover a
   surviving `msedgewebview2` child holding
   `%LOCALAPPDATA%\dev.shale.desktop\EBWebView` with no Shale running. Then the
   mutex is free, setup runs, and it panics: `failed to create webview: WebView2
   error: WindowsError(Error { code: HRESULT(0x800700AA), message: "The
   requested resource is in use." })` — no window, no sentence. Provoked twice
   tonight by `terminate`-ing probes. Catching `Builder::build()`'s error and
   saying something would fix it; the mutex cannot.
5. **Remaining Bambu parity is blocked outside Shale** — seam painting,
   per-filament flush volumes and multi-spool sliced 3MF need CuraEngine 5.13
   support it does not offer; camera and AMS mapping need a machine.
