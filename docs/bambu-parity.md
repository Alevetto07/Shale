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
| Slice and preview toolpaths | yes | yes | layer slider, G-code parsed back into paths |
| Move / rotate / scale / mirror | yes | yes | typed, dragged, and one-click: *mirror X/Y/Z*, *turn 90°*, *Upright*. Every one of them turns about the part's centre, and a mirror flips the triangle winding when it bakes, so the solid does not come out inside out |
| Duplicate | yes | yes | `Ctrl+D` |
| Undo / redo | yes | yes | Ctrl+Z / Ctrl+Shift+Z over a stack of plate snapshots; delete only hides a part, so undo never re-reads a file |
| Auto-arrange | yes | yes | `Edit → Arrange on plate` (Ctrl+Shift+A): biggest footprint first, shelf by shelf, 4 mm apart |
| Auto-orient | yes | yes | *Auto orient* in the model frame tries all 64 quarter-turn combinations and keeps the one whose downward facing area is smallest, then the shortest, then the steadiest. It scores geometry rather than slicing, so it is instant — but it only considers face-down orientations, not the finer angles a search over the sphere would find. On the bracket standing on its post that is **3256.3 mm** of support across 170 layers; after auto orient, **365.8 mm** across 30 layers, 89 % less |
| Measure | yes | yes | *Measure* in the model frame turns the pointer into a ruler: two clicks on a part and the distance appears on a line between them and in the panel, with the x/y/z deltas. A click snaps to the nearest mesh vertex within 2 mm, about a nozzle width and a little more, so "from that corner" lands on the corner; a click in the middle of a face has nothing to snap to and is kept where the ray put it. The arithmetic lives in the core (`shale_core::measure`) rather than the panel, so the number on screen is the number the tests check |
| Cut / split | yes | yes (plane only) | The model frame has `cut X/Y/Z`, an offset, and *both halves* / *upper only* / *lower only*. Every cut is capped: crossed triangles leave cut segments, the segments are stitched into loops and each loop is triangulated and welded on with outward winding, because a bare clipper leaves two open shells and an engine that fills by winding reads an open shell as a void. Both halves are set down on the plate. Bambu's cut also does booleans between two parts, holes and ear-shaped cuts; Shale has the plane |
| Per-object settings | yes | yes | Slice settings → *Applies to*: pick a part and the sheet writes that part's overrides; they ride beside its mesh to the engine and survive save/open |
| Variable layer height | yes | yes | Quality → *Variable layer height*; the engine re-plans the whole plate (0.10–0.28 mm gaps on the A1) — it is not painted per object the way Bambu's height-range modifier is |
| Supports: normal and tree | yes | yes (no painting) | Support → *Support shape* picks `normal` or `tree`, with the branch knobs Bambu exposes (angle, diameter) plus Cura's flare, tip and top density. On the upright T-bracket, whose arm hangs over nothing, normal support lays **3276.1 mm** of filament and tree support **2069.9 mm** — 36.8 % less support and 14.9 % less plastic in the whole part, same 170 layers. Paint-on supports are a separate subsystem and are still missing |
| Support regions: enforcers and blockers | yes | yes, proposed rather than painted | [propose] reads the overhangs past the angle and turns each cantilever into an enforcer volume, one per column, which the panel lists with its volume and contact area and you can switch off one at a time. On the bracket it finds **2 cantilever columns** (17 mm span, 102 mm² contact, 2989 mm³ each); [compare] then slices twice — plan against support switched on everywhere — and the engine's own filament figure comes back **2.4464 m against 3.8505 m**, 36.5 % less. Switching one column off drops the plan to **1.2235 m**. There is no free-hand brush: you get the regions the analysis found, not any triangle you point at |
| Support painting / seam painting | yes | **no** | needs a triangle-level brush, a large subsystem |
| AMS / multi-material | yes | yes (four spools) | The `ams` frame holds three more spools beside the plate material, and any part can name one in the model frame — the list tags it `T2`, `T3`, `T4` and the spool picker sets it. Each part reaches the engine as `-eN` in front of its `-l`, and the plate sends one material per spool, so a mixed plate slices with a real tool change: its own hotend temperature, its own flow, its own filament. On a two-cube plate the control file carries no `T` line at all, and pointing the copy at spool 2 (Generic PETG) gives **301 tool changes**, hotends at **220 and 205 °C**, and the same **300 layers**. Bambu flushes into a prime tower and counts the flushed grams; Shale has no tower yet, so the first lines after a change are the thin spot |
| Prime tower | yes | yes | The *Multi* tab carries `prime_tower_enable`, the tower's width and its corner, and the purge it holds per layer. With it off, a two-spool plate carries **no prime tower at all**; with it on at 45 mm the same plate grows a tower on **300 layers** holding **3590.2 mm** of filament — **44.6 mm** square on a **60.6 mm** base, sitting at x 137.2–197.8 and y 182.2–242.8 inside the 256 mm bed — and keeps its 301 tool changes. CuraEngine tags the sections `;TYPE:PRIME-TOWER`, hyphen and all, which is not the name of the setting |
| Send to printer / network | yes | **no** | deliberate: Shale does not talk to a printer |
| Device tab, camera, monitoring | yes | **no** | out of scope, see `docs/print-removal.md` |
| Calibration utilities | yes | **no** | |
| Timelapse | yes | **no** | |
| Project save | 3MF | `.shale` zip | round-trips models and settings |
| STL export | yes | yes | `File → Export plate as STL…` merges the visible parts, world transforms baked |
| 3MF export | yes | yes | `File → Export plate as 3MF…` keeps one named object per part |
| Measuring tool | yes | **no** | |
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

One caveat worth knowing: on a Bambu process the single "Print speed" field
barely moves the G-code, because the profile pins a speed per feature
(`outer_wall_speed`, `sparse_infill_speed`, and so on) and CuraEngine honours
those over `speed_print`. The per-feature speeds in the Speed tab are the knobs
that matter, which is also how Bambu Studio's own speed page behaves.

## The honest summary

Shale now covers the settings a person actually turns on a Bambu machine, and
proves it in the G-code. It does not yet cover the workflows that make Bambu
Studio feel like Bambu Studio: undo, arrange, paint-on supports, per-object
settings, AMS, and sending the job to the printer. Those are the next four
pieces of work, in that order, and each one is a feature rather than a field.
