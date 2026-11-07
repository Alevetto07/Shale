# Bambu Studio parity

Shale slices with an unmodified CuraEngine process and reads Bambu Studio's
installed profile library as data. Nothing here links, embeds or reimplements
Bambu Studio; "parity" means the same job the user came to do can be done in
Shale, not that the code is the same.

This file is the scoreboard. It is generated from
`apps/desktop/src/settings.ts` (the field list is the UI) and from the union of
keys in Bambu Studio's own `resources/profiles/BBL/process/*.json`, so the
numbers move when the schema does.

## Slicing settings

Bambu Studio's BBL process profiles carry **188 distinct setting keys**. Shale's
settings menu exposes **88 fields** across eleven tabs, of which **64** name a key
that Bambu actually ships somewhere in its BBL library (62 distinct keys). The rest are Cura
settings with no Bambu equivalent (wall widths, combing, ironing inset) that the
engine needs to be told anything at all.

The count is strict on purpose: it only credits a field when its `bambu` mapping
is a key that appears in `BBL/{process,filament,machine}/*.json`. Five earlier
credits were matched on Bambu's UI label rather than a key (`print speed`,
`sparse_infill_anchor`, `support_density`, `brim_type`, `internal_bridge_speed`)
and are now blank, and `ironing` points at `ironing_type`, which is real.

| Group | Fields | Bambu group it answers to |
| --- | --- | --- |
| Quality | 6 | Quality (widths, initial layer) |
| Strength | 7 | Strength (walls, shells, sparse infill) |
| Support | 13 | Support (normal and tree) |
| Build plate | 6 | Others → Bed adhesion, prime tower |
| Speed | 13 | Speed |
| Cooling | 7 | Cooling |
| Retraction | 7 | Others → retraction |
| Ironing | 6 | Others → ironing |
| Special modes | 9 | Others → spiral vase, fuzzy skin, seam, sequence |
| Material | 4 | Filament settings (first-layer temperatures, flow) |

Everything left alone is not sent at all: the selected profile decides, and the
field shows `auto`/`profile`. Only touched settings travel to the engine as
`extra` overrides, which is why a fresh project slices exactly like the profile
says it should.

### What is still missing from the settings surface

144 Bambu process keys have no field yet. They cluster:

| Cluster | Keys | Examples |
| --- | --- | --- |
| Other process settings | 64 | `bottom_surface_pattern`, `top_surface_density`, `brim_object_gap`, `circle_compensation_manual_offset` |
| Overhang, bridge, support detail | 35 | `bridge_flow`, `detect_overhang_wall`, `enable_overhang_speed`, `tree_support_*` |
| Speed, acceleration, jerk | 17 | `internal_solid_infill_speed`, `gap_infill_speed`, `initial_layer_acceleration`, `ironing_speed` |
| Seams and walls | 12 | `seam_slope_*`, `only_one_wall_top`, `infill_wall_overlap`, `inner_wall_acceleration` |
| AMS and multi-filament | 9 | per-filament flush volumes, scarf-seam overrides |
| G-code and machine | 4 | `print_extruder_id`, `reduce_infill_retraction` |
| Cooling and thermal | 2 | `pre_start_fan_time`, `standby_temperature_delta` |
| Print layout | 1 | `enable_arc_fitting` |

Most of the 64 "other" keys are per-surface overrides (a separate pattern,
density or line width for top, bottom and internal-solid skins). They are worth
having, but they are a second axis on top of the schema rather than new fields:
the schema needs a notion of "surface" before it can carry them honestly.

## Application features

| Feature | Bambu Studio | Shale | Note |
| --- | --- | --- | --- |
| Printer library | yes | yes | 655 machines: Cura's 642 plus 13 Bambu, with real beds |
| Filament library | yes | yes | Bambu's per-machine compatible filaments, plus Cura materials |
| Process/quality picker | yes | yes | per-printer processes |
| Menu-bar slicing settings | yes | yes | File / Edit / View / Printer / Slice settings, 88 fields |
| Top bar: tabs and four workspaces | yes | yes | The shell is a strip across the top with one tab per open project and a workspace switch — **Home, Prepare, Preview, Device** — the same four Bambu has. A tab carries the project's name and a dot while it has unsaved changes. Leaving a plate saves it to a scratch file instead of the recents list, because a parked plate is still a draft; the file it came from is untouched until the tab is saved. Opening a file into a blank untouched plate turns that plate into the file rather than adding a second one, two blank plates get distinct names, and *new project* on a plate nobody has touched gives one blank plate rather than two. **Home is a workspace like the others**: the start screen sits in the main area with the tab strip still overhead, so a plate is never hidden behind a whole different window, and the four chips move between Home, Prepare, Preview and Device without the strip ever going away. Preview is greyed out until there is a slice to look at, the way Bambu does it. A draft plate is put away to a scratch file when you leave its tab and read back when you return — that is the whole point of a tab strip, and a plate that was written but never read back was a plate lost by clicking its own tab. Device shows the machine, the four bays and whether the engine is where it should be — and says plainly that no printer is connected, because none is |
| Settings search and per-field reset | yes | yes | The sheet carries a search field above its eleven tabs: typing `wall` leaves **8 settings** on screen from Quality, Strength and Speed, each labelled with the tab it lives on, and every one of them really is about a wall. `ironing` finds **4** settings that live two tabs away; `zzz` says so plainly rather than showing a sheet that is quietly empty, and clearing the field brings the tabs back. A setting that disagrees with the profile carries a **×** that hands the profile's own value back, so a sheet of edits can be walked back one field at a time — Bambu has the same search, and a reset arrow on each changed field |
| Slice and preview toolpaths | yes | yes | layer slider, G-code parsed back into paths |
| Job card on the plate | yes | yes | Top-left of the plate once a slice exists, in tabular figures: **time, filament, weight, layers**. The numbers are the file's own — **5 h 02 m** against the engine's `;TIME:18096`, **24.5 m** against `;Filament used: 24.5084m`, **300 layers**. Weight is length × π(d/2)² × density and appears only when the filament profile carries a density, which Bambu's do and Cura's do not: a weight invented from a default would be worse than no weight. A plate that changes forgets all four together, and a plate reopened from disk reads its `;TIME:` and `;Filament used:` back out of the header |
| Preview feature list | yes | yes | The preview lists what the slice is made of down the right-hand side, with the colour each kind is drawn in and a switch per kind. On the 20 mm cube at layer 150 of 300 the list reads **wall-outer 600, wall-inner 600, skin 1552, infill 11834, skirt 5, travel 14592**. Hiding the infill takes the plate to **65.7 %** of its pixels while the walls stay standing; *none* takes the toolpath off and leaves only the bed; *all* puts back exactly the **60 404** pixels it started with. The preview also stops drawing the solid part now — a model standing in front of its own infill hides the one thing the preview exists to show |
| Preview coloured by speed | yes | yes | The preview's colour switch has **type** and **speed**: the second colours every extrusion by the feedrate the file asked for, slow outer walls blue through green and yellow to red at full clip, with travel left dim because it is not a speed anything is printed at. The ramp is scaled to the plate on screen over extrusion only — this plate reads **13 to 67 mm/s** — and the legend swaps its kind swatches for that scale and its two numbers, rather than leaving colours on the plate with nothing to explain them |
| Move / rotate / scale / mirror | yes | yes | typed, dragged, and one-click: *mirror X/Y/Z*, *turn 90°*, *Upright*. Every one of them turns about the part's centre, and a mirror flips the triangle winding when it bakes, so the solid does not come out inside out |
| Duplicate | yes | yes | `Ctrl+D` |
| Add a part | yes (right-click) | yes (File menu) | Four solids the core builds itself, 20 mm across and standing on z = 0: cube **12 triangles**, cylinder **256**, cone **128**, sphere **2944**. Each one lands on the first free patch rather than inside whatever is already there, so four in a row give four parts with four places. The volumes are pinned by unit tests — 20 × 30 × 10 mm gives 6000 mm³, and the round ones match πr²h, πr²h/3 and 4/3πr³ to within 2 %, which is what "closed and wound outward" means in practice |
| Fill the plate | yes | yes | *Edit → Fill the plate* steps the selected part's own footprint across the bed from the corner inwards, 4 mm apart, skipping the patch the original stands on and anything else already there. On a 220 mm bed with the 20 mm cube it lays **40 copies** beside the original — 41 parts in all, x 10.0 to 202.0 and y 10.0 to 117.5 before the 40-copy stop — with **no two copies overlapping**, every one of them inside the bed and standing on z = 0. The plate then slices as one job: **100 layers** and **75.3303 m** of filament against the single cube's **1.4447 m** |
| Undo / redo | yes | yes | Ctrl+Z / Ctrl+Shift+Z over a stack of plate snapshots; delete only hides a part, so undo never re-reads a file |
| Auto-arrange | yes | yes | `Edit → Arrange on plate` (Ctrl+Shift+A): biggest footprint first, shelf by shelf, 4 mm apart |
| Auto-orient | yes | yes | *Auto orient* in the model frame tries all 64 quarter-turn combinations and keeps the one whose downward facing area is smallest, then the shortest, then the steadiest. It scores geometry rather than slicing, so it is instant — but it only considers face-down orientations, not the finer angles a search over the sphere would find. On the bracket standing on its post that is **3256.3 mm** of support across 170 layers; after auto orient, **365.8 mm** across 30 layers, 89 % less |
| Measure | yes | yes | *Measure* in the model frame turns the pointer into a ruler: two clicks on a part and the distance appears on a line between them and in the panel, with the x/y/z deltas. A click snaps to the nearest mesh vertex within 2 mm, about a nozzle width and a little more, so "from that corner" lands on the corner; a click in the middle of a face has nothing to snap to and is kept where the ray put it. The arithmetic lives in the core (`shale_core::measure`) rather than the panel, so the number on screen is the number the tests check |
| Cut / split | yes | yes (plane, plus pins) | The model frame has `cut X/Y/Z`, an offset, and *both halves* / *upper only* / *lower only*. Every cut is capped: crossed triangles leave cut segments, the segments are stitched into loops and each loop is triangulated and welded on with outward winding, because a bare clipper leaves two open shells and an engine that fills by winding reads an open shell as a void. Both halves are set down on the plate. Bambu's cut also does booleans between two parts and ear-shaped cuts; Shale has the plane. **Pins** make the cut reversible: `cut_with_pins` stands 32-sided cylinders up out of the lower half's cut face and bores the same pin, wider by 0.2 mm clearance and deeper by 0.4, out of the upper — a real BSP-tree boolean, since a hole is not something a plane can do. Placement is on the cut face itself: candidate points come from the loops the cut stitched, and a pin is only kept where the whole disc fits inside the polygon with 0.6 mm to spare, so a 10 mm face takes one pin and a 60 mm face takes four. Measured on a 20 mm cube (8000.000 mm3, 12 triangles): plane cut → **40 triangles, 8000.00 mm3**; the same cut with two 4 mm pins 5 mm deep → **1880 triangles, 7967.74 mm3**, which is 8000 + 2 pins (124.86) − 2 holes (163.20) to within 6 mm3 — the material the clearance removes is material the pin does not replace. The pinned plate then slices: 100 layers, 6 feature kinds, no complaint from the engine about the T-junctions a BSP leaves behind |
| Split to objects | yes | yes | *Edit → Split to objects* is the other half of the cut: a file whose one part is really several bodies is separated into the parts it always had. Corners are welded on a 0.1 µm grid first (an STL writes every corner once per facet), the shells are found by union-find, and each is rebuilt with its own welded vertex table, biggest first. A 24-triangle file holding two 20 mm boxes arrives as **one part**, split gives **two parts of 12 triangles**, the export keeps them **40.00 mm** apart, and the pair slices as one job of **50 layers**. Undo puts the single part back and redo splits it again |
| Per-object settings | yes | yes | Slice settings → *Applies to*: pick a part and the sheet writes that part's overrides; they ride beside its mesh to the engine and survive save/open |
| Variable layer height | yes | yes | Quality → *Variable layer height*; the engine re-plans the whole plate (0.10–0.28 mm gaps on the A1) — it is not painted per object the way Bambu's height-range modifier is |
| Supports: normal and tree | yes | yes | Support → *Support shape* picks `normal` or `tree`, with the branch knobs Bambu exposes (angle, diameter) plus Cura's flare, tip and top density. On the upright T-bracket, whose arm hangs over nothing, normal support lays **3276.1 mm** of filament and tree support **2069.9 mm** — 36.8 % less support and 14.9 % less plastic in the whole part, same 170 layers. Painting is its own frame, below |
| Support regions: enforcers and blockers | yes | yes, proposed rather than painted | [propose] reads the overhangs past the angle and turns each cantilever into an enforcer volume, one per column, which the panel lists with its volume and contact area and you can switch off one at a time. On the bracket it finds **2 cantilever columns** (17 mm span, 102 mm² contact, 2989 mm³ each); [compare] then slices twice — plan against support switched on everywhere — and the engine's own filament figure comes back **2.4464 m against 3.8505 m**, 36.5 % less. Switching one column off drops the plan to **1.2235 m**. The suggestions are regions, not a brush |
| Support painting | yes | yes, patch or free-hand brush | Bambu's brush is free-hand over the mesh; Shale offers the patches the geometry already has — and, since the brush pass, a brush of its own. The `paint support` frame lists every ceiling below the support angle — on the gantry that is **387, 387 and 6 mm²** — and each button paints one. A painted face becomes a closed prism from the face down to the bed, handed to the engine as a `support_mesh` volume, and painting turns the plate's own support off: what is painted is the whole of it. One arm painted puts **1008.6 mm** of support under that arm alone — x **−69.3 to −5.7**, climbing to **z 29.4 mm**, the painted ceiling — against **2663.7 mm** across the whole plate with nothing painted; the plate's own support comes back at exactly **2663.7 mm** when the paint is cleared. Paint is saved with the part (`painted`, 2 faces here) and indices are the part's own, so moving or turning it does not unpaint it. *brush* arms a free-hand stroke: drag over the part and every face under the cursor is painted, at a reach you set in millimetres. A stroke is a list of points in the part's own frame — no view direction, no screen-space guess — and `shale_core::paint::brush_stroke` picks the faces by distance to their centres, so the same stroke paints the same faces from any camera. On a 60 mm sphere with `reach 8`, one left-to-right drag painted **149 faces (2205 mm²)** and the slice grew **587** `;TYPE:SUPPORT` lines (0 → 587, 5.41 MB → 6.80 MB, same 600 layers); clearing the paint put the file back to 0. A stroke and a patch are the same edit: one undo entry, one history step, and the preview takes the stroke back with [ take back ]. Paint has undo with the rest of the plate |
| Seam painting | yes | **no** | a seam is a per-layer point, not a face; painting it needs the z-seam exposed and a brush that writes to the engine's seam list, which CuraEngine does not take |
| Pause or filament change at a layer | yes | yes | from the layer in hand while scrubbing the preview — `pause here` and `swap here` — or from the instructions frame in the sidebar. The next slice carries a `; SHALE … at layer N` comment and then `M0 Paused by Shale` or `M600 ; Filament change`. Measured on the Arm bracket: a change at layer 19 and a pause at layer 30, each emitted once, in order, `M600` at line 14314 and `M0` at 15712 |
| AMS / multi-material | yes | yes (four spools) | The `ams` frame holds three more spools beside the plate material, and any part can name one in the model frame — the list tags it `T2`, `T3`, `T4` and the spool picker sets it. Each part reaches the engine as `-eN` in front of its `-l`, and the plate sends one material per spool, so a mixed plate slices with a real tool change: its own hotend temperature, its own flow, its own filament. On a two-cube plate the control file carries no `T` line at all, and pointing the copy at spool 2 (Generic PETG) gives **301 tool changes**, hotends at **220 and 205 °C**, and the same **300 layers**. Bambu flushes into a prime tower and counts the flushed grams; Shale has the tower (next row) but not the accounting |
| Prime tower | yes | yes | The *Multi* tab carries `prime_tower_enable`, the tower's width and its corner, and the purge it holds per layer. With it off, a two-spool plate carries **no prime tower at all**; with it on at 45 mm the same plate grows a tower on **300 layers** holding **3590.2 mm** of filament — **44.6 mm** square on a **60.6 mm** base, sitting at x 137.2–197.8 and y 182.2–242.8 inside the 256 mm bed — and keeps its 301 tool changes. CuraEngine tags the sections `;TYPE:PRIME-TOWER`, hyphen and all, which is not the name of the setting |
| Plate management | yes | yes | The `plates` frame is a row of chips, one per build plate, with **+** to add and **×** to drop; each plate keeps its own parts and its own support plan, and a slice belongs to the plate it was made on. Each plate slices on its own — the tower on plate 1 gives 250 layers, the cube on plate 2 gives 300. A project saves every plate: two plates, a 60-triangle tower and a 12-triangle cube, save as two archive entries carrying `plate: 0` and `plate: 1` under `plate_names: ["plate 1", "plate 2"]`, and reopening puts each part back on its own plate. No per-plate printer or process settings yet |
| Send to printer / network | yes | yes | The Device workspace holds a real link over the LAN: MQTT over TLS on 8883 for state, implicit FTPS on 990 for the upload, and `gcode_file` / `pause` / `resume` / `stop` on the request topic. Credentials come from the environment, never the repo. Proved against a fake printer speaking the same wire format — a **1,492,553-byte** plate uploaded and accepted, then paused, resumed and stopped — and not yet against hardware. See `docs/printer-host.md` |
| Device tab, camera, monitoring | yes | monitoring yes, camera no | The Device pane is live: state, percent, layer, remaining time, nozzle and bed actual against target, and a bar that reads `idle` / `running` / `pause` from the machine's own reports. The camera is not implemented — Bambu's stream is a protocol of its own (MPEG-TS on X1-class, JPEG on 6000), and nothing in this tree speaks it |
| Calibration utilities | yes | one, proved | Calibration → *temperature tower* builds the test part itself and puts it on the plate: five 10 mm bands, alternating 20 mm and 26 mm blocks so every band overhangs the one below by 3 mm, and every band is asked the same question. Slicing it gives **250 layers**; *step the hotend* then rewrites that slice so band *n* prints 5 °C cooler than band *n−1* — five `M104` lines at layers 0, 48, 98, 148 and 198, whose heights read back out of the file as **0.2, 10.0, 20.0, 30.0 and 40.0 mm**, with every `G` line in the file unchanged and the layer count still 250. Three, all proved. The *flow coupon* is a 30 mm single-wall tube (four 0.4 mm walls, five bands, 240 triangles): flow is measured with calipers, not judged, so the wall is the instrument. *Step it* writes **five `M221` lines** down the tower — 105, 102.5, 100, 97.5 and 95 % — and the file keeps every `G` line and every layer it had. The *pressure tower* is the alternating tower again with **five `M900 K` lines** climbing 0.000 → 0.040 K, one per band, because pressure advance shows at corners and each band has four of the same. A band is decided by height, not by layer number, and each band's line is written once, at the first layer that reaches it |
| Timelapse | yes | **no** | |
| 3MF import | yes | yes | A 3MF is read as a package of objects, not as one blob: every build item comes back as its own part, with the name the file gave it and the place the file gave it, and the parts are dropped to the bed rather than pulled to the middle — an assembly that is re-centred comes apart. A file of two 20 mm blocks 60 mm apart arrives as **two parts of 12 triangles**, named `left block` and `right block`, still 60 mm apart, and the plate slices as one job of **100 layers**; exporting that plate writes the same two objects, whose x centres read back as **−30.0 and +30.0**. A single-object file is still centred on the plate as before |
| Project save | 3MF | `.shale` zip | round-trips models and settings |
| STL export | yes | yes | `File → Export plate as STL…` merges the visible parts, world transforms baked |
| 3MF export | yes | yes | `File → Export plate as 3MF…` keeps one named object per part |
| G-code flavour | Bambu/Marlin | Marlin | see the caveat in README |

## Verified, not assumed

Settings are only "supported" here if the engine obeyed them. Evidence from a
Bambu Lab A1 with `Arm.3mf`:

All of these are on a Bambu Lab A1, `Arm.3mf`, 0.2 mm and 55 layers unless
stated, reset to the profile between runs:

| Control | Kind | Result |
| --- | --- | --- |
| Wall loops 2 → 6 | number | 506,719 B → 622,187 B, still 55 layers |
| Enable support | toggle | 0 → 46 `;TYPE:SUPPORT` runs, 559,702 B |
| Build plate adhesion brim → none | choice | 506,661 B → 482,869 B, skirt 1 → 0 |
| Outer wall 40 mm/s | number | feed `F2400` ×260 |
| Travel 300 mm/s | number | `F7200` ×2214 → `F18000` ×2214 |
| Layer height 0.2 → 0.3 | number | `;LAYER_COUNT` 55 → 37 |
| Temperature tower, stepped | command | 250 layers, 5 `M104` at layers 0/48/98/148/198 → z 0.2/10/20/30/40 mm, every `G` line unchanged |
| Two plates, saved | command | 2 archive entries, `plate` 0 and 1, `plate_names` ["plate 1", "plate 2"]; reopening restores 1 part per plate |
| Part cooling off | toggle | fan commands 85/170/255 → one opening burst plus `M106 S0`; the print runs unfanned |
| Fan speed 40 | number | ramp 85/170/255 → 34/68/102; the profile's half speed (170) gone |
| Z hop 0.4 | number | `;LAYER_COUNT` unchanged, 34 → 34 lift moves, `z_heights` gains the hop |
| Filament weight | data | job card 3.7 m → 11 g on generic PLA (density read from the material's `<properties><density>`) |

**A material is the material, not a label.** Three faults sat between the
material menu and the nozzle, and any one of them made the choice cosmetic.
`select_material_id` ran an exact id through the fuzzy "nearest diameter" pass,
whose tie-break keeps the first material it meets — a 1.75 mm PLA — so every
choice resolved to PLA. Cura's own definition links the machine's temperatures to
the material's with a formula (`material_print_temperature` reads
`default_material_print_temperature`); definitions are read here as data, so the
link is now made explicit, or a material contributed its "print temperature"
under a key nothing read. And a material used to overwrite `material_diameter`
with its own, so a 2.85 mm spool on a 1.75 mm machine stretched every flow figure
computed from it; the machine keeps its filament size, and the picker offers only
spools that fit.

Read back from `printer_bed` on the Ender-3: PLA **200 / 60**, PETG **240 / 85**,
ABS **245 / 80** — three different materials, three different answers. Sliced:
the PLA file's only tool temperature is 200 C, the PETG file's is 240 C.

**A plate keeps its own machine and material, and its own name.** Bambu lets two
tabs be two jobs — one plate on PETG, one on PLA. Shale was already most of the
way there without anyone checking: a tab is parked to a file when you leave it
(`writeProjectFile`) and read back when you return (`openProject` →
`applyOpened`), and that file carries `printer_id`, `variant_name`,
`quality_type`, `material_id` and the AMS slots, so the recipe comes back with the
plate. What did not survive was the tab's *name*: a park wrote its scratch file's
sanitised base ("Untitled 2" → "Untitled-2") into the project's `name`, and the
read-back then renamed the tab after the file. A park records the tab's own name
now; only a real save adopts the file's.

Proved with two tabs: plate 1 switched to Generic PETG and sliced to a file whose
only tool temperature is **240 °C**; a second tab opened on the same window, set
to Generic PLA, sliced to **200 °C**; back on plate 1 the sheet read PETG again,
the tab still read "Untitled", and the re-slice was the same **240 °C** file.
14 checks, `recipe_v1.py`.

**The sheet's numbers reach the engine, and Ctrl+Z takes them back.** Two bugs
shared one symptom: a field moved, was remembered, and the slice ignored it.

The first was in the job builder. Every value typed into the sheet travels to the
core as `extra`, and the only entries the core ever read were the handful of keys
with hand-written handling — `cool_fan_enabled` and its fan speeds. Every other
field in the sheet was a facade: `sparse infill density` at 40 % on a 20 mm cube
produced a file identical to 20 %. The extras are now merged into the resolved
settings last, so an override beats the profile whatever key it names, and an
overridden `layer_height` is read back out of the merged map because the layer
estimate is computed from it. The proof is the file: 4,884.7 mm of infill and a
5,422 s estimate at the profile's 20 %, 6,423.9 mm and 6,896 s at 40 %.

The second was the history. Undo knew about models and, after the paint work,
about paint — but a number typed into the sheet was not a step, so Ctrl+Z after
changing the density quietly undid the last unrelated model move instead. The
history is one typed log now (a model snapshot, a paint stroke, or the sheet's
numbers), pushed in the order the hand acts, so Ctrl+Z always takes back the last
thing. Proven end to end: the number goes back to the profile's placeholder, the
slice returns to 4,884.7 mm exactly, and Ctrl+Y puts both back.

**A paint stroke can be taken back.** Painting used to sit outside the undo
stack: Ctrl+Z restored the models and the painted columns stayed, because paint
is triangle indices, not geometry, and the model snapshot knew nothing about it.
The backend keeps its own small history of strokes now (sixty deep, each a map of
part to faces — cheap, because a stroke is a list of indices). Ctrl+Z takes the
stroke back before it touches geometry, Ctrl+Y puts it back, and the paint panel
grows a `[ take back ]` for the mouse. Proven end to end rather than by
inspection: a painted underside puts `;TYPE:SUPPORT` in the file 146 times, one
Ctrl+Z takes the paint off the part without touching the part, and the next slice
has none.

**The sheet shows the profile's own numbers.** A field nobody has touched used
to be a blank; now `profile_settings` resolves the chosen printer, quality and
material with nothing overridden, and every number shows that value as a grey
placeholder, every switch shows the state the machine would run, and every
choice shows the profile's option. Bambu Studio's sheet behaves this way, and so
does Cura's — the number you see before you touch anything is the number the
slice will use. The "profile" option in a list and the × beside a moved field
both put the field back.

**Part cooling is a switch in Cura's UI and two numbers in the engine.** Turning
it off leaves `cool_fan_speed` at 100, so the engine keeps writing `M106 S255`
and the switch changes nothing. `resolver.rs` now carries the switch down to
`cool_fan_speed`, `_min` and `_max`, and `job.rs` does the same for a switch set
per object, because a per-object `cool_fan_enabled` never reaches the resolver at
all. Proved in the G-code: the profile's ramp of `M106` at 85/170/255 becomes a
single opening burst and an explicit `M106 S0`, and the rest of the print runs
with the fan off.

One caveat worth knowing: on a Bambu process the single "Print speed" field
barely moves the G-code, because the profile pins a speed per feature
(`outer_wall_speed`, `sparse_infill_speed`, and so on) and CuraEngine honours
those over `speed_print`. The per-feature speeds in the Speed tab are the knobs
that matter, which is also how Bambu Studio's own speed page behaves.

## The honest summary

Shale now covers the settings a person actually turns on a Bambu machine, and
proves it in the G-code. The workflows that make Bambu Studio feel like Bambu
Studio have mostly arrived too: undo, arrange, per-object settings, the AMS,
tree supports, support regions, a prime tower, several build plates and painted
supports are all in, and each one was proved from the file rather than from a
screenshot — the painting by the column it puts under one arm and under nothing
else.

What is left is the free-hand brush. Painting paints the patches the analysis
found, not any triangle a mouse crosses, it has no undo of its own, and seams
are not painted at all. Then ear-shaped cuts, a native Bambu
G-code flavour, and the flush accounting behind the tower.

## The host half

The last paragraph used to say that Shale does not open a socket to a machine.
It does now, deliberately: the Device workspace holds a live link to the printer
over its own LAN interface — MQTT/TLS on 8883 for state, implicit FTPS on 990 for
the upload, and `pause`/`resume`/`stop` as requests. See
[printer-host.md](printer-host.md) for the protocol, the safety line (nothing
starts without a press, and the press asks first) and the offline proof.

What that closes and what it does not:

| Bambu Studio | Shale |
|---|---|
| live state, temperatures, layer, remaining time | yes, one poll a second |
| upload the sliced plate and start it | yes — plain G-code, `use_ams: false` |
| pause, resume, stop | yes |
| sliced 3MF with `ams_mapping` for a multi-spool job | **no** — needs a sliced-3MF writer |
| camera | **no** — a separate stream protocol |
| firmware and account calls | **no** — those are cloud, not LAN |
| a job queue | **yes** — a job is a copy of the slice, so the next slice cannot overwrite a queued one; `start queue` names every job and is the only press that lets a print start without you pressing for it. The queue advances on `FINISH`/`IDLE` and stops on `FAILED` |
