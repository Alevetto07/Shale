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

## A boolean at three sizes, and the wall it moves

`boolean_timings_at_eight_thirty_and_eighty_thousand` in `csg.rs` builds a
lattice sphere at 7 800, 32 500 and 79 200 triangles, then subtracts a smaller
sphere from it. Release build, one run:

| triangles | tree built | nodes | depth | polygons in the tree | subtract | polygons out |
|---|---|---|---|---|---|---|
| 7 800 | 67 ms | 8 054 | 108 | 13 335 | 5 114 ms | 33 728 |
| 32 500 | 366 ms | 34 670 | 135 | 58 663 | 215 565 ms | 145 928 |
| 79 200 | 924 ms | 85 585 | 195 | 146 304 | not timed | — |

Two changes moved this. Neither cracks it.

**The plane chooser samples and balances.** `pick_plane` used to score every
face plane against every polygon and take the cheapest: quadratic, and on a
convex body every face plane scores the same, because a face plane of a convex
hull leaves the whole body in front of it. It now reads at most 96 polygons,
scores each candidate on that sample, and above the third of the set that a face
plane cannot resolve it tries the three axis-aligned planes through the sample's
median centroids, charging a straddling polygon to both sides because it becomes
a polygon on both sides. A plane that bisects every polygon is not a balanced
cut, it is a way to double the mesh. The tree at 7 800 triangles went from
5 090 ms and a depth of 4 264 to 67 ms and a depth of 108.

**A Boolean does not need a tree at the end.** Every operation ended in the
csg.js way: `left.build(right.all_polygons())` merges both halves into a fresh
tree, and the caller then walks that tree to collect the same polygons. On
7 800 triangles that one step was most of a seven-second subtract. `merged()`
concatenates the two polygon lists and applies the final flip, which is all
`invert()` means to a caller that only wants polygons. Subtraction and
intersection end with that flip; a union does not.

**What is still true.** The depth is no longer 4 264, but the tree is not
balanced either: at 32 500 triangles it is 135 deep against a log₂ of 15, and
the polygon count has grown by 1.8× before a single clip, because a plane
through a sphere cuts a third of its triangles in two. Clipping is charged per
polygon per level, so the subtract at 32 500 triangles takes 3½ minutes and the
subtract at 79 200 was not waited for. The wall is now polygon growth rather
than depth, and it has the same shape: on a finely tessellated convex body a
BSP-based Boolean is quadratic whatever the plane chooser does.

**So the cap stays at 12 000 triangles.** These numbers do not justify raising
it: 32 500 takes three and a half minutes, and the climb is steep enough that
12 000 is already slower than a person will sit through on a convex body. The
concave case that parts are actually made of is fast — the bracket (1 946
triangles, 0.01 s, 1 864 out) and a plate with a bore (2 554 triangles, 57 ms).
Lifting the cap needs a different algorithm, not a better tree: a bounding-volume
hierarchy plus exact clipping inside the overlapping boxes, which never cuts a
triangle that no boundary passes through. That is a piece of work on its own,
and it is under Open in `docs/STATUS-v0.1.md`.

## A plate sliced for the A1, from both ends of the app

Two surfaces, one slicer. The window and `shale-mcp` (JSON-RPC over stdio, the
headless interface) were pointed at `Arm.3mf`, printer `bambu_lab_a1`, variant
`0.4 mm`, quality `0_20mm_standard_bbl_a1`, material `bambu_lab_pla_basic`:

| surface | layers | filament | `machine_gcode` | file |
|---|---|---|---|---|
| `shale-mcp` over stdio | 54 | 2.1942 m | true | 515 971 bytes |
| the window (`import_model` → `slice_model` → `export_gcode`) | 54 | 2.1943 m | true | 522 285 bytes |

Both files open like this — comments first, then the machine's own sequence, and
nothing that heats or moves before it:

```
;FLAVOR:Marlin
;TIME:1378
;Filament used: 2.1943m
;Layer height: 0.2
;TARGET_MACHINE.NAME:Bambu Lab A1

;Generated with Cura_SteamEngine 5.13.0
;===== machine: A1 =========================
;===== date: 20250822 ==================
G392 S0
M9833.2
;M400
;M73 P1.717

;===== start to heat heatbead&hotend==========
M1002 gcode_claim_action : 2
M1002 set_filament_type:PLA
M104 S140
M140 S65
```

and end with the A1's own finish sequence, not Cura's:

```
;=====printer finish  sound=========

;M17 X0.8 Y0.8 Z0.5 ; lower motor current to 45% power
M400
M18 X Y Z
```

Neither file contains a `{`. Both pass `starts_with_machine_sequence`, and both
were uploaded byte for byte to the fake printer by

```
SHALE_SLICED_GCODE=<the file> cargo test -p shale-host --test wiring a_sliced_file -- --ignored --nocapture
```

which also checks the head for `;===== machine: A1`,
`M1002 gcode_claim_action : 2` and `M1002 set_filament_type:PLA`, and the tail
for the finish sequence above. It printed `522285 bytes of sliced G-code
arrived at the fake printer`.

**Two defects this found.** The first attempt failed the gate on Shale's own
output: CuraEngine writes `M140`/`M190`/`M104`/`M109` *before* the start G-code
it is given, so the A1's file heated to 220 °C before the machine's macro had
said how, and the old rule — "stop looking at `;FLAVOR:`" — refused the file
outright. `bambu::strip_engine_prelude` now drops the engine's heat lines from a
file that carries a machine sequence: the template heats to standby, levels, and
then heats to print temperature, which is the machine's own order. The gate was
relaxed by exactly one line's worth of meaning — comments may precede the
banner, no command may — so a Cura file that heats first is still refused, and
the test for that case is in `gate_tests`.

`MINX`/`MAXY` in the header read `2.14748e+06`, which is `f32::MAX`. Those
comments are written by CuraEngine from the model's bounds and are wrong for a
model placed by Shale; nothing reads them, and Bambu's own firmware ignores
them, but a slicer should not write a bounding box that says the part is two
million millimetres wide. It is under Open.

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

## A printer whose certificate changed

Bambu printers present a self-signed certificate, so a chain of trust cannot say
whether the machine on `192.168.x.y` is the one you bought. Two things can: trust
on first use, and the pin that trust leaves behind.

The first connection to a printer has nothing to compare against, by definition,
so it accepts what arrives — the access code is what authorises the session — and
records the certificate's SHA-256. Every later connection must present the same
fingerprint. The pin is keyed by the printer's **serial**, falling back to its
address when no serial was recorded, so two printers on one network never
overwrite each other.

Where it is stored: **the Windows keyring**, next to the access code, under
`Shale/pin/<serial>`. Not in the app config, because the pin's whole job is to
detect something interfering with the connection, and a plain file next to the
app is exactly what such a thing would rewrite. It is not a secret — a
fingerprint is public — but it is only worth anything if it cannot be edited
casually, and the keyring is the one store on this machine with a defined
boundary per entry.

Both channels are guarded, not just one: the MQTT link and the FTPS upload go
through the same verifier. A file sent to an impostor is the model, the settings
and the access code at once.

When it fires, the link stops rather than reconnecting — a changed certificate is
a question, not a flaky socket — and the Device pane shows the sentence with
both fingerprints and one press:

> The machine on this address presented a different certificate. A printer that
> was factory reset, or had its board replaced, does this; anything else
> answering in its place does too. Trust the new one only if you know why it
> changed.
>
> `trust the new certificate`

That press deletes the pin and reconnects, which is a fresh first sighting.

Proved by `cargo test -p shale-host --test pinning`, five tests, 0.33 s against
the loopback double started with either of two committed certificates:

| Test | What it proves |
|---|---|
| `the_first_connection_records_the_certificate_it_was_offered` | the recorded pin is `61acc0ae…6f11`, the SHA-256 of the fixture, and the window is told |
| `the_same_printer_connects_again_under_its_pin` | a matching certificate is let in |
| `a_changed_certificate_stops_the_link_and_says_why` | a different certificate (`633d1a33…0ebd`) leaves the link disconnected with a sentence naming both fingerprints, and it stays refused |
| `the_upload_channel_is_guarded_by_the_same_pin` | nothing is uploaded to a machine that cannot be identified |
| `a_hex_pin_survives_a_round_trip` | a pin stored with colons, as `openssl` prints it, is read back |

And once in the window, which is the half a test cannot reach. The double is a
binary as well as a module:

```
cargo run -p shale-host --features fake-printer --bin shale-fake-printer -- --cert a
cargo run -p shale-host --features fake-printer --bin shale-fake-printer -- --cert b
```

It binds 127.0.0.1 on 8883 and 990 — the ports the Device tab's `connect` uses —
so the form can be filled and pressed like a person would. Run against `a`,
then killed and restarted as `b`, the Device tab showed, in this order:

1. `offline · 0%` and the keyring holding `61acc0ae…6f11`: a first sighting
   recorded what it was offered.
2. After `b` answered: `not connected — unexpected error: the printer's
   certificate changed: this machine was pinned to 61acc0ae… and now presents
   633d1a33…. If the printer was factory reset or its board replaced, that is
   expected; otherwise something else is answering on its address.` — plus the
   block and its single press, both in the dark palette, the hashes wrapping
   inside their box rather than overflowing.
3. One press on `trust the new certificate`: the block gone, the keyring now
   `633d1a33…0ebd`, the link re-established against the machine that was there.

The pin is never auto-accepted; the only path from a refusal to a connection is
that press.


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


## Forgetting a printer

Pinning has an obvious consequence nobody wants to discover the hard way: a
machine that legitimately changed its certificate — a replaced board, a factory
reset, a printer borrowed and returned — is refused for ever, and the only way
out was to edit the Windows Credential Manager. The Device pane now has a
`forget` press beside `connect` and `disconnect`, enabled whenever an address or
serial is present. It drops the link, deletes `pin/<serial or address>` and
`device.code` from the vault, and clears the address and serial the window keeps
in `localStorage`.

The run, with the pin from an earlier session still in the vault:

1. `connect` against a printer offering a different certificate:
   `not connected — unexpected error: the printer's certificate changed: this
   machine was pinned to 633d1a33… and now presents 61acc0ae….` The pin had
   outlived the process, which is the point of a pin and the first proof that it
   lives in the credential manager rather than in memory.
2. `forget`: address, serial and code fields empty; `localStorage` empty;
   `host_pin` null; `device.code` absent from the vault; status `no printer
   connected`; note `forgotten — the address, the serial, the access code and the
   certificate pin`.
3. A reload: the Device pane comes back with empty fields, not the old address.

## A second launch comes to the front

WebView2 will not let two processes share a window profile: the second one's
`create webview` fails with `HRESULT(0x800700AA)`, "The requested resource is in
use", and Tauri unwinds setup — no window, no sentence, the process just ends.
Double-clicking the icon twice was enough to do it.

`instance.rs` claims a named mutex (`Local\dev.shale.desktop`) before anything
opens. A second process does not argue: it finds the window titled `Shale`,
restores it with `ShowWindow(SW_RESTORE)` and brings it up with
`SetForegroundWindow`, then exits 0. The message box is only the fallback for a
process that holds the profile and has no window to show.

```sh
python second_launch.py
# after the first launch:
#   39704  01:11:56.651  Shale
# after the second launch (second pid 34948):
#   39704  01:11:56.651  Shale
# second process still alive: False exit: 0
# after killing both, a new launch:
#   30984  01:12:14.284  Shale
```

The second process left instead of dying, the first kept its window, and a
launch after both had gone still opened — so the mutex is released with the
process and cannot wedge the app. What this does **not** show is the window
actually rising to the front: the probes run on a desktop the Shale windows do
not occupy, so a screen capture sees neither the app nor the box. The calls are
the documented ones and the second process's exit is the observable.

## A window that could not be made

The case above leaves one hole: a `msedgewebview2` child that outlives a crash
keeps the profile directory, and then there is no window to focus and no mutex to
lose. That refusal arrived twice tonight by accident, as `failed to create
webview: WebView2 error: WindowsError(Error { code: HRESULT(0x800700AA), message:
"The requested resource is in use." })`, and `Builder::run` turned it into a
panic: no window, no sentence, the process gone.

`run()` builds first and runs after — `builder.build(tauri::generate_context!())`
returns the error instead of unwinding inside it — so `start_failed` gets a
value it can act on. An in-use refusal earns **one** retry: the mutex is released,
a fresh process is spawned 1.5 s later with `SHALE_START_RETRIED=1`, and that
variable is what stops the child from retrying in turn, so the pair cannot loop.
If the retry also fails, `say_start_failed` puts the sentence in a native box and
the process exits 0.

The refusal itself could not be arranged on demand. Four attempts, all negative:

| attempt | result |
|---|---|
| kill the app, start again after 0.2 s / 0.6 s / 1.5 s | no orphan survived (`webviews left: 0` at each delay); the profile was free and the app opened |
| launch `msedgewebview2.exe --user-data-dir=<profile> --headless=new` directly | the browser exited at once, 0 holders — it wants a host handshake |
| hold `EBWebView\Default\LOCK` with `CreateFileW(share mode 0)` from Python | the app started normally; that file is not the lock |
| point `WEBVIEW2_USER_DATA_FOLDER` at a file instead of a folder | the app started normally |

The lock is inside WebView2, keyed to a profile a live browser process owns, and
the only way to hold it is to be that process. So the path is driven by a
diagnostic instead: `SHALE_START_FAILS=1` hands `start_failed` the exact refusal
above and `SHALE_START_FAILS=retry` takes the retry road first. Both are read at
startup, are never set by the app, and change nothing when absent — the same kind
of switch as `SHALE_TRACE`.

The proof reads the box's own controls out of the live window rather than
photographing it, which is stronger: a capture can be of the wrong desktop (it
was, twice), where `GetWindowTextW` on the dialogue's `Static` child returns the
exact characters the user is being shown.

```sh
python read_box.py                     # SHALE_START_FAILS=1
# dialogues owned by the app: 1
#   caption: 'Shale did not start'
#   Button: 'OK'
#   Static: 'Shale could not open its window because an earlier Shale is still
#            closing.\n\nWait a moment, then start Shale again. If this comes
#            back, end the\nmsedgewebview2 processes in Task Manager and start
#            Shale once more.'
# exit code after dismissing: 0
```

And the retry, watched from outside:

```sh
python read_retry.py                   # SHALE_START_FAILS=retry
# parent pid: 33660 exit at 3s: 0
# pids alive: [32128, 33660]
# dialogs: 1
#   caption: 'Shale did not start' owned by pid 32128
# parent final exit: 0
# --- trace:
#     [shale] the window profile is still held: failed to create webview: WebView2
#             error: WindowsError(Error { code: HRESULT(0x800700AA), message: "The requ…
#     [shale] retrying once in a second and a half
```

The parent gave up its mutex, the child took it, and the child — not the parent —
carried the box. No panic appears in either run, and both exits are 0.

Three unit tests hold the sentence still: a held profile is explained as a wait
and the number `0x800700AA` never reaches the reader, any other failure is quoted
rather than summarised, and a long error is cut to six lines with an ellipsis.

What is not proved: that the real lock produces this path. It cannot be made to
happen on demand here, so the branch is covered by the seam and the sentence by
the tests, and the honest statement is that the *handling* is proved while the
*provocation* is not.

## How long the catalogue takes, when nothing else is running

The first window cannot list printers until Cura's resources are read: 706
definition files, then 6 011 quality profiles, 1 741 variant files and 641
intent files under `share/cura/resources`. That read was timed for a while at
half a minute, which was wrong — those runs happened while `cargo` had every
core. The app now says which half of the wait it is in when `SHALE_TRACE` is
set:

```sh
SHALE_TRACE=1 SHALE_CURA_DIR=... SHALE_MATERIALS_DIR=... SHALE_CURAENGINE=... \
  ./target/release/shale-desktop.exe
# [shale] resources discovered: 111.6µs
# [shale] catalogue read: 1.7906967s
```

| binary | finding Cura | reading it | `app_status` ready |
|---|---|---|---|
| release | 0.1 ms | **1.79 s** | — |
| debug | 0.27 ms | **4.09 s** | **4.9 s**, 655 printers |
| `shale-mcp` (release, stdio) | — | — | 1.6 s to the `initialize` reply |

The window figure is from `catalog_window.py`, which polls `app_status` on an
isolated WebView2 profile (`WEBVIEW2_USER_DATA_FOLDER`), so no earlier instance
can confuse it. Nothing here needs a cache or a thread pool: two seconds for
655 printers is a fair price for a fresh read, and the honest fix to the
complaint was the message, not the loader.

## A project that comes back

`.shale` is the only format Shale owns, and it holds the whole plate — the
original file bytes, every transform, the settings, the painted support patches —
so a round trip is worth checking on real data rather than a fixture. The Arm was
imported into the A1 job and saved through `save_project` (26 515 bytes), then
`open_project` read it back as one model, `Arm.3mf`, 1 946 triangles, printer
`bambu_lab_a1`, material `bambu_lab_pla_basic`.

The half that matters is the window, and the recent list is the one path to a
project that needs no OS dialog. Launched fresh, the start screen listed five
projects, newest first:

```
roundtrip    10 Nov 2026 · Bambu Lab A1
Untitled      9 Nov 2026 · Bambu Lab A1
Untitled      7 Nov 2026 · Bambu Lab A1
blockers      7 Nov 2026 · Bambu Lab A1
per-object    7 Nov 2026 · Bambu Lab A1
```

Pressing `roundtrip` closed the start screen, named the tab `roundtrip`, and
brought back Plate 1 with one part: `Arm.3mf` on the plate, the A1 / 0.4 mm /
0.20 Standard / PLA Basic job intact, and the painted support patch still
attached to the underside (`1 patches`, 36 mm²). Not sliced, which is what was
saved.

What this does not cover: the Save As and Open dialogs themselves. They are the
OS's, a probe cannot press them, and the commands they hand a path to are what
was exercised.

## The bounding box the engine does not know

CuraEngine writes six header lines describing the scene: `;MINX`, `;MINY`,
`;MINZ`, `;MAXX`, `;MAXY`, `;MAXZ`. Handed bare STL files — which is how Shale
calls it — it fills them with `f32::MAX` and its negation:

```
;MINX:2.14748e+06
;MAXX:-2.14748e+06
```

A file whose every move sits inside a 256 mm plate claimed a box two million
millimetres wide. Cura's own header describes the scene in the coordinates the
moves use, and `job::slice` already bakes and shifts each mesh into exactly
those coordinates, so `bambu::rewrite_header_bounds` now replaces the six lines
with the union of the baked meshes' bounds. Nothing else in the file is touched;
a header without those lines, or a set of models that measured to infinities, is
returned unchanged.

Sliced again for the A1 through `shale-mcp`, the same model that used to say
`2.14748e+06` says:

```
;MINX:-45.392   ;MAXY:24.392
;MINY:-45.392   ;MAXZ:11
;MINZ:0         ;MAXX:24.392
```

69.8 mm square and 11 mm tall, in the bed-centre frame the moves already use —
which is the Arm. Three unit tests cover the rewrite, the untouched header and
the unmeasured box.

## The release build

```
cargo clippy --workspace --all-targets --locked -- -D warnings   # clean
cargo build --workspace --release --locked                       # finished in 4m 49s
cd apps/desktop && npm run tauri build                           # exit 0
```

| artefact | bytes | size |
|---|---|---|
| `target/release/bundle/nsis/Shale_0.1.0_x64-setup.exe` | 4 675 254 | 4.46 MiB |
| `target/release/shale-desktop.exe` | 17 856 000 | 17.0 MiB |
| `target/release/shale-mcp.exe` | 10 735 104 | 10.2 MiB |

The installer was built three times as the night went on — 4 651 432 bytes after
the lint pass, 4 657 574 once the UI pass had changed two strings in the window
and given About its version, 4 675 455 after the bounding-box and forget fixes,
and 4 675 254 once the single-instance guard was in. The last one is the one
that ships, and it is built from the branch head, not from a working tree with
notes in it.

Versions are 0.1.0 in all four crates, `apps/desktop/package.json` and
`apps/desktop/src-tauri/tauri.conf.json`. Nothing was tagged, and the installer
was not run: it exists, it is the right size, and it is not signed.

The lint pass was mechanical in the main (borrowed values, needless casts,
`sort_by(|a, b| …)` turned into `sort_by_key`, a `while let` that never looped,
an index loop over three corners, `&mut Vec<u32>` narrowed to `&mut [u32]`, a
`Default` followed by one assignment turned into struct update). Three lints are
allowed rather than obeyed, each with its reason in the source: `too_many_arguments`
and `type_complexity` in `shale-core` and the desktop crate, and
`type_complexity` in the fake printer. A slice is thirteen parameters because a
slice has thirteen inputs; the mesh code passes tuples of primitives.
