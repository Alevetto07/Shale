# Proving Shale still works

Everything below was run on this machine and produced the number next to it.
The probes live in the agent scratch directory rather than the repository: they
drive the real window over CDP (`--remote-debugging-port=9222`), which needs a
WebView2 build flag, so they are tools for a person with the app open, not CI.

Numbers are the point. "It sliced" is not a result; "55 layers, 27 min, 2.3 m,
7.1 g" is, and it either matches or something changed.

## Build and unit tests

```bash
cd /c/Users/Alessio/shale
cargo build -p shale-desktop --locked     # Finished in ~36 s
cargo test  --workspace --locked          # 128 passed, 0 failed
cd apps/desktop && npx tsc --noEmit && npm run build   # ✓ built in ~12 s
```

Run the frontend build before launching: a stale `dist/` gives a blank window
(377 bytes of HTML) and every probe then lies about the app.

Launching with the sample data:

```bash
WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS="--remote-debugging-port=9222" \
SHALE_CURA_DIR="C:/Program Files/UltiMaker Cura 5.13.0/share/cura/resources" \
SHALE_MATERIALS_DIR="C:/Program Files/UltiMaker Cura 5.13.0/share/cura/resources/materials" \
SHALE_CURAENGINE="C:/Program Files/UltiMaker Cura 5.13.0/CuraEngine.exe" \
./target/debug/shale-desktop.exe
```

A running `shale-desktop.exe` blocks `cargo build` (`Access is denied`, os error
5). Stop it first, and give the process list a beat before trusting what the
window shows — a stale instance keeps port 9222 while the new one writes logs.

## The slice

`Arm.3mf` (1946 triangles) on the Bambu Lab A1 at 0.20 mm: **55 layers, 27 min,
2.3 m of filament, 7.1 g**. Any other number means the profile resolution or the
engine call changed.

## A setting reaches the engine

Layer height 0.2 → 0.3 and re-slice: `;LAYER_COUNT` goes **55 → 37**. The sheet
shows the profile's value until you touch the field, so the number on screen and
the number in the file can be compared directly.

## Boolean between parts

Two 20 mm cubes overlapping by 10 mm, `subtract` from the model frame: **16
triangles, 4000.00 mm³, x −10.0…0.0**, which slices as **100 layers, 12 min,
0.7 m**. `union` → 24 triangles / **12000.00 mm³**; `common` → 16 triangles /
**4000.00 mm³**. With nothing overlapping the press is refused: *"No other part
overlaps this one, so there is nothing to cut with"*.

## Place on face

Select a part, press *Place on face*, click a surface. On the bracket: rotation
**0,0,0 → −180,0,−180**, exported volume **3885 mm³** before and after, plan
centre **128.00, 128.00 mm** both times, still resting at **z 0.00**. Ctrl+Z puts
the rotation back and leaves the part on the plate. The kernel test
`orient::tests::a_clicked_face_ends_up_on_the_plate` covers an arbitrary normal:
a 10 mm cube turned 30° about X, given its +Y face, comes back with that face
lowest, **1000.00 mm³** unchanged, plan centre fixed.

## Cut, and cut with pins

A 20 mm cube (**8000.000 mm³**, 12 triangles) cut through the middle: **40
triangles, 8000.00 mm³**. The same cut with two 4 mm pins 5 mm deep: **1880
triangles, 7967.74 mm³** — 8000 + pins (124.86) − holes (163.20) to within
6 mm³, which is the clearance doing its job. The pinned plate slices as 100
layers with no complaint from the engine about BSP T-junctions.

## A dovetail, and why the boolean used to be capped

A 30 mm cube cut through its middle with a dovetail of `root 6`: the lower half
goes **20 triangles / 13500.00 mm³ → 48 triangles / 13824.00 mm³**, so the tab is
**324.00 mm³** and reaches **6.00 mm** past the plane — the trapezoid exactly,
(6+12)/2 × 6 × 6. The upper half is bored by the same prism grown 0.2 mm on
every side.

At the kernel, a 70 × 70 × 11 plate cut at 5.5 with an 8 mm root gains
**432.00 mm³** and its tab stops at z **10.00**: the upper half is 5.5 mm thick,
so the tab is held a millimetre short of coming out of its top.

**The bracket, with a dovetail, now cuts in 0.01 s.** 1946 triangles in, 1864
out, sliced to **47 layers / 0 h 26 m / 2.0 m / 6.1 g**. Before the split plane
was chosen, the same cut ran five minutes and had to be killed, twice taking the
window with it.

That earlier failure had two causes, both fixed:

- **The tree was a chain.** `csg.js` starts every node with `polygons[0]`'s
  plane. A bore contributes hundreds of facets, each with its own plane, and
  none of them divides the rest — so the tree degrades to a list and every level
  re-splits what is below it. Each node now samples two dozen candidates and
  takes the lowest `splits * 8 + |front - back|`.
- **The stack.** BSP clipping recurses once per split polygon; over `main`'s two
  megabytes that killed the window with no message at all. Geometry that cuts a
  mesh runs on a thread with 256 MB.

Scaling, a plate with a bore (concave — the shape that used to explode) cut by a
cube:

| triangles | before | after |
| --- | --- | --- |
| 338 | 4 ms | 3 ms |
| 654 | killed at 300 s | 9 ms |
| 970 | — | 14 ms |
| 1286 | — | 19 ms |
| 1922 | — | 36 ms |
| 2554 | — | 57 ms |

`BOOLEAN_CAP` is **200 000** triangles now, where it was 600: what is left at
that size is memory, not time. The kernel test
`a_concave_part_with_a_bore_booleans_quickly` holds the ceiling at two seconds.

## Undo after a cut, and after a boolean

Cut the bracket, keep the lower half, press **Ctrl+Z**: the plate goes back to
`Arm.3mf` — one row, one part. Before the fix it went back to **nothing at
all**, because the cut removed the original from the model list and
`restore_models` can only restore what it still has; the command then hid every
part it could not find, and the plate was empty with no way back.

Two overlapping cubes (**30 mm** and **20 mm**, 5 mm into each other) and
*subtract*: the plate holds one part of **55 triangles**, `consumed: 1`. **Ctrl+Z**
brings back both cubes, 12 triangles each, two rows.

The cutters are not deleted any more: they are made invisible, so undo and redo
work entirely by id. A hidden part does not count towards the boolean's
triangle cap, which counts only what is on the plate.

## Painting support

A 60 mm sphere (2944 triangles) with one brush stroke: **149 faces / 2205 mm²**
painted. Sliced side by side, the unpainted plate has **0** `;TYPE:SUPPORT`
lines and the painted one **587**, 5,414,170 → 6,797,316 bytes.

## A pause or a filament change in the preview

`swap here` on layer 20 → `; SHALE filament change at layer 19` and
`M600 ; Filament change` at **line 14314**. `pause here` on layer 31 →
`; SHALE pause at layer 30` and `M0 Paused by Shale` at **line 15712**.
480,843 → 480,935 bytes.

## Split to objects

A file holding two 20 mm boxes: **one part in, two parts of 12 triangles out**,
the export keeps them **40.00 mm** apart, and the pair slices as one job of
**50 layers**.

## The host

Against the fake printer in the scratch directory: `t+10 s` the job is
**running · 30 %**, `t+50 s` it is **idle**; with `FAKE_FAIL=1` it reports
**FAILED** after 8 s. This has not been tried against a real printer, and the
printer is not on this network — `8883 open on: []` for both `192.168.1.0/24`
and `192.168.3.0/24`.

## The video

```bash
ffprobe -v error -show_entries format=duration,size \
  -show_entries stream=width,height,r_frame_rate -of default=nw=1 \
  C:/Users/Alessio/Documents/shale-demo/shale-tour.mp4
```

**2560 × 1558, 30 fps, 40.10 s, 2,092,968 bytes.** Pull frames every five
seconds and look at them: Home, Prepare with the brush armed, Preview at layer
31 of 55 with a pause mark, Device with the host and the queue, then back to
Home and Prepare. Every pane change in the tour is checked against the active
chip before the step is allowed to count, because a silent no-op is how one take
ended up labelled Prepare while showing preview controls.

## The stills

`C:/Users/Alessio/Documents/shale-demo/` holds `shale-final-{home,prepare,preview,
device,settings}.png` and `shale-empty-{prepare,preview,device}.png`. They are
taken in one run — import the bracket, slice it (**55 layers**), shoot the loaded
panes, then clear the plate and shoot the empty ones — because an earlier set was
shot against a plate that had nothing on it and every "loaded" image came out
byte-identical to its empty counterpart. Each of the eight now has its own md5.
The settings image is the sheet itself, not the window behind it.

## The queue never starts itself

A print that finishes leaves its part on the bed, so a queue that advances by
itself prints the second job on top of the first. The rule now lives in
`shale_core::queue`, as five states — `idle`, `armed`, `printing`,
`waiting_for_plate`, `stopped` — and the gate is in Rust:

```
idle ──press──▶ armed ──send ok──▶ printing ──finished──▶ waiting_for_plate
                                            └──failed/error──▶ stopped
```

`QueueRun::observe` accepts machine reports and can never start a job: there is
no method on it that starts one. `queue_start_next` refuses unless the run is
`armed`, and only `queue_arm` (the press that starts the queue) or
`queue_plate_cleared` (the press after each print) can arm it. The refusal text
is "The queue is waiting for a plate-cleared press: nothing goes out from this
bed". A failed or errored send stops the run instead of trying the next job.

Proved at the command layer, not just the state machine: `queue_gate.py` drives
the real `queue_start_next` through the window and gets that refusal back, then
arms it and watches the send fail for want of a printer and the run stop.

## The A1 starts the way the A1 starts

`Bambu Lab A1 0.4 nozzle.json` carries `machine_start_gcode` and
`machine_end_gcode` at zero length. The real text sits beside it, in
`… template machine_start_gcode.json` and `… template machine_end_gcode.json`,
as lists of lines. `bambu.rs` reads those, `Catalog` carries them per nozzle,
and a slice for a Bambu machine renders them with the values the slice resolved.

The template speaks Bambu's names, not Cura's, so `job::render_sequence` fills
the gaps: `nozzle_temperature` from `material_print_temperature`,
`bed_temperature_initial_layer_single` from the layer-0 bed temperature,
`filament_type` from the filament profile, `spiral_mode` from `magic_spiralize`,
`outer_wall_volumetric_speed` and `flush_volumetric_speeds` from the filament's
volumetric limit, and the first layer's box from the placed models.

What comes out, measured on the fixtures the A1 ships (0.4 mm, PLA, 10 mm cube):

- Line 1 is `;===== machine: A1 =========================`, not `;FLAVOR:Marlin`.
- `M620 S0A` hands the print to the AMS, `G28 Z P0 T140` homes Z against the
  bed, `M983 F…` runs the machine's own extrusion calibration, `G29.1 Z-0.02
  ; for Textured PEI Plate` scans the plate, and the purge block wipes and
  shakes the nozzle. Cura's generic Marlin start does none of these.
- The numbers are the slice's: 215 first layer, 210 after it, bed 60, nozzle
  `H0.4`, `set_filament_type:PLA`.
- The end sequence ends `M18 X Y Z`, `M400`,
  `;M17 X0.8 Y0.8 Z0.5 ; lower motor current to 45% power`, and cuts the
  motors' current before it stops.

Placeholders are not best-effort: an unresolved name fails the slice with the
name in the message (`unexpected none: nozzle_temperature_at_noon …`), and the
renderer refuses to return text with a `{` left in it. A Bambu machine also
refuses a file that does not open with its own banner — `send_file` reads the
head of the G-code and will not send one that starts with anything else.

## The theme, after a pass

Four leftovers from a light theme and a broader palette were found by looking,
and each is now measured rather than eyeballed:

| Where | Was | Is |
| --- | --- | --- |
| View menu, checked row | `✓` glyph in a `<kbd>` | 2 px accent rule at the left, `rgb(185, 179, 255)` |
| Speed legend | five thermal-camera colours | five stops in one hue, `#474a5e → #b9b3ff`, `13` to `71` mm/s |
| Settings footnote | `linear-gradient(transparent, panel)` | flat `var(--panel)` |
| Device warning | `var(--amber, #d9a441)` | `var(--accent)` |
| Active plate chip | ink behind, paper in front — a white tile | 10 % accent over the panel, accent underline |

Probed in the running window (`probe/theme_take.py`): the checked row reports
`width: 2px`, `background: rgb(185, 179, 255)`, the page contains **zero** `✓`
characters, and the legend's five swatches read back as
`rgb(71, 74, 94)`, `rgb(98, 93, 126)`, `rgb(123, 116, 163)`,
`rgb(154, 147, 210)`, `rgb(185, 179, 255)`, with the ends taken from the slice
itself: `13` to `71` mm/s on one plate, `10` to `67` on another.

The ramp was first drawn without gaps and read as one wash at a glance; the five
bands are separated by a 1 px gap showing the panel behind them, so the stops
can be told apart in a sidebar 200 px wide.

## A boolean at three sizes, and the wall it hits

`boolean_timings_at_eight_thirty_and_eighty_thousand` in `csg.rs` builds a
lattice sphere at three sizes and reports the tree before subtracting anything.
Release build, one run:

| triangles | tree built | nodes | depth |
|---|---|---|---|
| 3 000 | 575 ms | 1 792 | 1 696 |
| 4 940 | 1 653 ms | 2 803 | 2 700 |
| 7 820 | 5 090 ms | 4 449 | 4 264 |

The cost grows as the square of the triangle count and the depth grows with the
count, not with its logarithm. Both come from the same fact: on a **convex**
body every candidate split plane is a face plane, and a face plane of a convex
hull leaves the entire body in front of it, so each node peels one face and the
tree is a chain. No choice of plane among the faces fixes that, which is why
`pick_plane` did fix the concave case and cannot fix this one.

That is why the ladder stops at 7 820: the thirty- and eighty-thousand rungs do
not finish, and a tree 4 264 deep exhausts even the 256 MB thread the desktop's
Boolean runs on. What is proved is the concave case that parts are actually made
of — the bracket (1 946 triangles, 0.01 s, 1 864 out) and a plate with a bore
(2 554 triangles, 57 ms). A Boolean on a coarse convex body is fine; a finely tessellated one would sit on
a spinner for minutes, so `BOOLEAN_CAP` is 12 000 triangles and the refusal is a
sentence rather than a hang. That cap is a property of this kernel, not of the
printer: lifting it needs a build that splits on planes of its own choosing, or
an iterative one, not a bigger stack.

## The access code is not in the profile

A Bambu LAN access code is the printer's MQTT password. It used to sit in the
WebView's `localStorage`, which is a plain file in the profile directory, read by
anything running as this user and backed up with the profile.

It is now stored in the Windows Credential Manager under `Shale/<key>`, through
`apps/desktop/src-tauri/src/secrets.rs`:

- `secret_set`, `secret_get`, `secret_clear` are Tauri commands; `get` returns
  `None` when nothing is stored rather than an error, and `clear` on an absent key
  succeeds.
- The password is never logged, never written to a project file, and never
  returned by a list command.
- On first launch the code left in `localStorage` is moved to the vault once and
  the old key deleted.

Proved by a test that uses the real vault, not a mock —
`cargo test -p shale-desktop --locked secret`:

```
test secrets::tests::a_password_survives_a_round_trip_and_a_clear ... ok
test result: ok. 1 passed; 0 failed
```

Which asserts write, read-back, delete, and that the deleted entry reads as
`None`. Nothing in `apps/` or `crates/` names a real access code.

## The host, against a printer that exists only on loopback

`crates/shale-host/tests/support/fake_printer.rs` is a Bambu machine in a test:
MQTT over TLS and implicit FTPS, both bound to `127.0.0.1` on ephemeral ports, so
no administrator rights and no network. It answers CONNECT/SUBSCRIBE/PUBLISH/
PINGREQ, collects what the host publishes on `device/<serial>/request`, pushes
reports a test asks for on `device/<serial>/report`, and reads a `STOR` payload
over a second TLS channel. Its certificate is a committed fixture
(`tests/fixtures/fake-tls/`), because a certificate generated at start could not
be pinned by a test, and pinning is the point of the second fixture.

One command runs all of it:

```
cargo test -p shale-host --test wiring
```

Four tests, all driving the same code the window drives:

| Test | What it proves |
|---|---|
| `an_a1_file_uploads_and_the_start_command_arrives` | 1.2 kB of G-code reaches `/cache/plate.gcode` intact over FTPS, then `gcode_file` arrives on the request topic with the right path and bed |
| `a_cura_started_file_is_refused_before_it_is_sent` | the A1 sequence passes the gate; a Cura start fails it **even with the banner quoted in a later comment**, which the old rule accepted |
| `a_finished_print_parks_the_queue_until_the_plate_is_cleared` | RUNNING then FINISH over the wire → `WaitingForPlate`, `may_start()` false, and only a plate-cleared press arms one more job |
| `a_failed_report_stops_the_run` | FAILED with an error number → `Stopped`, the number kept, nothing revivable |

The run takes 0.30 s. Two things it forced into the open: `HostConfig` had the
protocol's port numbers written into the client, so a test could not bind them
(`mqtt_port`/`ftps_port` are fields now, defaulting to 8883/990), and the gate
lived in the window's crate and read the whole file into memory while matching
the banner anywhere in it. The rule is in `shale-host` now, reads 4 kB, and stops
looking at the first motion — one rule, one test, next to the upload it protects.

## What is still not proved

- Seams are not painted, so there is nothing to compare against Bambu's seam
  painter.
- The body of the G-code is Marlin-flavoured: CuraEngine writes the moves and
  the types, so Bambu's own flavour (`M1002`, `M981`) does not appear. What is
  Bambu's is the start and the end — the wipe, the bed scan, the AMS handover,
  the calibration and the finish — and no machine here can run any of it.
- Per-filament flush volumes are not priced; CuraEngine has no flush matrix.
- A multi-spool plate is refused rather than sent, because the sliced 3MF needs
  an `ams_mapping` this app does not write.
- Camera and timelapse need a printer; the printer is not on this network.
