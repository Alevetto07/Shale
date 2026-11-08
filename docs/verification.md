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

## A dovetail, and the cap on the boolean

A 30 mm cube cut through its middle with a dovetail of `root 6`: the lower half
goes **20 triangles / 13500.00 mm³ → 48 triangles / 13824.00 mm³**, so the tab is
**324.00 mm³** and reaches **6.00 mm** past the plane — the trapezoid exactly,
(6+12)/2 × 6 × 6. The upper half is bored by the same prism grown 0.2 mm on
every side.

At the kernel, a 70 × 70 × 11 plate cut at 5.5 with an 8 mm root gains
**432.00 mm³** and its tab stops at z **10.00**: the upper half is 5.5 mm thick,
so the tab is held a millimetre short of coming out of its top.

The 1946-triangle bracket is **refused**, not attempted:

> A joint needs a mesh boolean, and the boolean is only proven on small parts:
> this part has 1946 triangles and the cap is 600. A plain cut still works.

That is a limit, not a bug to be papered over. `csg` is a BSP tree and its cost
climbs faster than the triangle count: before the cap existed, the same cut ran
for five minutes and had to be killed, twice taking the whole window with it —
BSP recursion over `main`'s two megabytes overflows the stack, so the geometry
now runs on a thread with 256 MB.

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

**2560 × 1558, 30 fps, 39.90 s, 1,864,360 bytes.** Pull frames every five
seconds and look at them: Home, Prepare with the brush armed, Preview at layer
31 of 55 with a pause mark, Device with the host and the queue, then back to
Home and Prepare. Every pane change in the tour is checked against the active
chip before the step is allowed to count, because a silent no-op is how one take
ended up labelled Prepare while showing preview controls.

## What is still not proved

- Seams are not painted, so there is nothing to compare against Bambu's seam
  painter.
- The G-code is Marlin-flavoured. Bambu's own flavour (`M1002`, `M981`, the
  AMS print sequence) is not emitted, and no machine here can run it.
- Per-filament flush volumes are not priced; CuraEngine has no flush matrix.
- A multi-spool plate is refused rather than sent, because the sliced 3MF needs
  an `ams_mapping` this app does not write.
- Camera and timelapse need a printer; the printer is not on this network.
