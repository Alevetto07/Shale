# Calibrating the Bambu A1 head envelope

Shale ships a **conservative** toolhead envelope in
`crates/shale-nonplanar/data/bambu_a1_head.json`. Collision flattening and
travel checks use this model. Until you measure your printer, expect occasional
over-flattening on tall skinny features beside low travel moves.

## What to measure (on your A1, 0.4 mm nozzle)

All distances are millimetres, nozzle tip at **Z = 0** (touching the bed), XY
origin at the nozzle tip.

| JSON field | What to measure |
| --- | --- |
| `sock_outer_radius_mm` | Horizontal reach of the silicone sock at its widest point, from the nozzle centreline. |
| `sock_top_z_mm` | Height of the top of the sock above the nozzle tip. |
| `body_radius_mm` | Radius of the metal heatblock / carriage body at fan height (whichever is wider). |
| `duct_x_offset_mm[1]` | Furthest **+X** reach of the part-cooling duct shell from the nozzle centre (carriage at home orientation). |
| `duct_z_mm` | Height of the duct bottom edge above the nozzle tip when the duct is level. |

Already taken from BambuStudio and usually fine without re-measure:

- `nozzle_height_mm` (4.76 for A1 0.4)
- `tip_land_radius_mm` (land diameter / 2)
- `nozzle_cone_half_angle_deg` (stock cone)

## Procedure

1. **Home the machine**, remove filament, and install the sock you print with.
2. **Sock radius and height** — With a caliper or scale photo, measure sock OD/2
   and the vertical distance from the nozzle tip to the top of the sock at the
   same X/Y as the nozzle.
3. **Duct sweep** — Command a slow `G1` to place the nozzle at bed centre
   (`X128 Y128` on the A1 bed). Without moving Z, measure how far the duct
   extends in **+X** (`duct_x_offset_mm[1]`) and how high the duct lip sits
   (`duct_z_mm`). Repeat for **−X** if your prints use mirrored clearance; the
   JSON models the larger reach only.
4. **Heatblock body** — At maximum X reach used in step 3, measure body radius
   at the same height as the sock measurement.
5. **Update JSON** — Edit `bambu_a1_head.json` with measured numbers. Set
   `"calibration_tier": "measured"` and clear fields from `needs_measurement`
   as you verify them.
6. **Verify** — Slice a model with a tall pillar beside low travel (e.g.
   `examples/cube.stl` with a thin peg) using `--nonplanar curved-tops`. Check
   CLI output: `nonplanar_safety ok=true`, `equiv=true`, and review
   `collision_adjustments` in the safety JSON.

## Optional finer check

Print Shale's synthetic pillar scene (see `shale-nonplanar` collision tests).
If travel still collides on the physical print, shrink `body_radius_mm` or
`sock_outer_radius_mm` by 0.5 mm and re-slice until clears cleanly without
flattening all curved tops.