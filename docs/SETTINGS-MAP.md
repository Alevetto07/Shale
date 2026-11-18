# Settings map — Bambu Studio process settings over CuraEngine

Shale's process panel is Bambu Studio's: five tabs (Quality, Strength, Speed, Support, Others),
Bambu's names, and a simple/advanced switch. The engine underneath is still CuraEngine, so every
row is a Cura setting carrying a value that came from a Bambu key.

The presets in `apps/desktop/src/bambu-presets.ts` are generated from Bambu Studio's own A1
process profiles, resolved through their inheritance chain (`resources/profiles/BBL`), not typed
by hand. The table below lists the mapping and, for reference, the value in `0.20mm Standard @BBL A1`.

| Bambu Studio | CuraEngine | 0.20 Standard |
| --- | --- | --- |
| `bottom_shell_layers` | `bottom_layers` | 3 |
| `bridge_speed` | `bridge_wall_speed` | 50 |
| `brim_width` | `brim_width` | 5 |
| `default_acceleration` | `acceleration_print` | 6000 |
| `enable_prime_tower` | `prime_tower_enable` | 1 |
| `enable_support` | `support_enable` | 0 |
| `fan_max_speed` | `cool_fan_speed_max` | — |
| `fan_min_speed` | `cool_fan_speed_min` | — |
| `fuzzy_skin` | `magic_fuzzy_skin_enabled` | 0 |
| `initial_layer_print_height` | `layer_height_0` | 0.2 |
| `initial_layer_speed` | `speed_layer_0` | 50 |
| `inner_wall_line_width` | `wall_line_width_x` | 0.45 |
| `inner_wall_speed` | `speed_wall` | 300 |
| `internal_solid_infill_line_width` | `skin_line_width` | 0.42 |
| `ironing_type` | `ironing_enabled` | 0 |
| `layer_height` | `layer_height` | 0.2 |
| `outer_wall_acceleration` | `acceleration_wall_0` | 5000 |
| `outer_wall_line_width` | `wall_line_width_0` | 0.42 |
| `outer_wall_speed` | `speed_wall_0` | 200 |
| `prime_tower_width` | `prime_tower_size` | 35 |
| `reduce_crossing_wall` | `retraction_combing` | 0 |
| `retraction_length` | `retraction_amount` | — |
| `seam_position` | `z_seam_type` | back |
| `skirt_distance` | `skirt_gap` | 2 |
| `skirt_loops` | `skirt_line_count` | 0 |
| `slow_down_layer_time` | `cool_min_layer_time` | — |
| `slow_down_min_speed` | `cool_min_speed` | — |
| `sparse_infill_density` | `infill_sparse_density` | 15 |
| `sparse_infill_line_width` | `infill_line_width` | 0.45 |
| `sparse_infill_pattern` | `infill_pattern` | grid |
| `sparse_infill_speed` | `speed_infill` | 270 |
| `spiral_mode` | `magic_spiralize` | 0 |
| `support_line_width` | `support_line_width` | 0.42 |
| `support_object_xy_distance` | `support_xy_distance` | 0.35 |
| `support_speed` | `speed_support` | 150 |
| `support_threshold_angle` | `support_angle` | 30 |
| `support_top_z_distance` | `support_z_distance` | 0.2 |
| `support_type` | `support_structure` | tree |
| `top_shell_layers` | `top_layers` | 5 |
| `top_surface_speed` | `speed_topbottom` | 200 |
| `travel_speed` | `speed_travel` | 700 |
| `wall_loops` | `wall_line_count` | 2 |
| `z_hop` | `retraction_hop` | — |

## Settings Shale does not show

Bambu has no counterpart for these Cura settings, so the panel hides them rather than offering a
control that means nothing in Bambu's vocabulary:

- `speed_print`
- `bridge_skin_speed`
- `acceleration_enabled`
- `cool_fan_enabled`
- `retraction_enable`
- `retraction_hop_enabled`
- `ironing_pattern`
- `ironing_monotonic`
- `z_seam_corner`
- `bridge_settings_enabled`
- `material_flow_layer_0`
- `prime_tower_position_x`
- `prime_tower_position_y`
- `prime_tower_min_volume`
- `prime_tower_wipe_enabled`
- `adaptive_layer_height_enabled`
- `adaptive_layer_height_variation`
- `adaptive_layer_height_variation_step`
- `adaptive_layer_height_threshold`

They still exist in CuraEngine and a preset file may carry them; the panel simply does not
pretend they are Bambu settings.

## Presets

The six A1 process presets are Bambu's:

- **0.08mm Extra Fine** — `0.08mm Extra Fine @BBL A1`
- **0.12mm Fine** — `0.12mm Fine @BBL A1`
- **0.16mm Optimal** — `0.16mm Optimal @BBL A1`
- **0.20mm Standard** — `0.20mm Standard @BBL A1`
- **0.24mm Draft** — `0.24mm Draft @BBL A1`
- **0.28mm Extra Draft** — `0.28mm Extra Draft @BBL A1`

Applying one writes the four simple values (layer height, wall loops, sparse infill density and
pattern) plus every other mapped key as a Cura override, so the sheet shows exactly what changed.

## Values that are not one-to-one

- `sparse_infill_pattern` maps Bambu's names to Cura's (`line` → `lines`, `zig-zag` → `zigzag`,
  `tri-hexagon` → `trihexagon`, `cross-hatch` → `cross`, `adaptive cubic` → `cubicsubdiv`).
- `seam_position` maps `aligned` → `back`, `nearest` → `shortest`; the rest are shared words.
- `support_type` maps `tree(auto)` → `tree`, `normal(auto)` → `normal`.
- `ironing_type` becomes Cura's `ironing_enabled` toggle: `no ironing` is off, anything else on.
- Temperatures live in the filament profile, not the process, so the material tab shows Cura's
  first-layer temperatures rather than a copy of Bambu's filament settings.
