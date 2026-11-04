/**
 * Slicing settings, as data.
 *
 * Every entry is a Cura setting key: the engine takes settings by name, so this
 * list is the whole UI. `bambu` records the Bambu Studio / Orca setting the
 * field stands in for, which is what `docs/bambu-parity.md` counts.
 *
 * A setting with no user value is not sent: the profile decides. Only what the
 * user has actually touched travels to the engine.
 */

export type SettingKind = "number" | "choice" | "toggle";

export interface SettingField {
  /** Cura setting key, sent to CuraEngine as-is. */
  key: string;
  label: string;
  kind: SettingKind;
  unit?: string;
  min?: number;
  max?: number;
  step?: number;
  options?: string[];
  /** The Bambu Studio / Orca setting this stands in for, if there is one. */
  bambu: string;
}

export interface SettingGroup {
  id: string;
  label: string;
  fields: SettingField[];
}

export const SETTING_GROUPS: SettingGroup[] = [
  {
    id: 'quality',
    label: 'Quality',
    fields: [
      { key: "layer_height_0", label: "Initial layer height", kind: "number", bambu: "initial_layer_print_height", min: 0.05, max: 1.2, step: 0.02, unit: "mm" },
      { key: "wall_line_width_0", label: "Outer wall width", kind: "number", bambu: "outer_wall_line_width", min: 0.1, max: 2, step: 0.01, unit: "mm" },
      { key: "wall_line_width_x", label: "Inner wall width", kind: "number", bambu: "inner_wall_line_width", min: 0.1, max: 2, step: 0.01, unit: "mm" },
      { key: "infill_line_width", label: "Sparse infill width", kind: "number", bambu: "sparse_infill_line_width", min: 0.1, max: 2, step: 0.01, unit: "mm" },
      { key: "skin_line_width", label: "Skin width", kind: "number", bambu: "internal_solid_infill_line_width", min: 0.1, max: 2, step: 0.01, unit: "mm" },
      { key: "support_line_width", label: "Support width", kind: "number", bambu: "support_line_width", min: 0.1, max: 2, step: 0.01, unit: "mm" },
    ],
  },
  {
    id: 'strength',
    label: 'Strength',
    fields: [
      { key: "wall_line_count", label: "Wall loops", kind: "number", bambu: "wall_loops", min: 0, max: 12, step: 1 },
      { key: "top_layers", label: "Top shell layers", kind: "number", bambu: "top_shell_layers", min: 0, max: 20, step: 1 },
      { key: "bottom_layers", label: "Bottom shell layers", kind: "number", bambu: "bottom_shell_layers", min: 0, max: 20, step: 1 },
      { key: "top_bottom_thickness", label: "Skin thickness", kind: "number", bambu: "top_shell_thickness", min: 0, max: 5, step: 0.1, unit: "mm" },
      { key: "infill_sparse_density", label: "Sparse infill density", kind: "number", bambu: "sparse_infill_density", min: 0, max: 100, step: 5, unit: "%" },
      { key: "infill_pattern", label: "Sparse infill pattern", kind: "choice", bambu: "sparse_infill_pattern", options: ["lines", "trihexagon", "cubicsubdiv", "tetrahedral", "quarter_cubic", "concentric", "zigzag", "cross", "cross_3d", "gyroid", "lightning", "honeycomb", "octagon", "grid", "cubic", "triangles"] },
      { key: "infill_overlap", label: "Infill/wall overlap", kind: "number", bambu: "sparse_infill_anchor", min: 0, max: 60, step: 1, unit: "%" },
    ],
  },
  {
    id: 'support',
    label: 'Support',
    fields: [
      { key: "support_enable", label: "Enable support", kind: "toggle", bambu: "enable_support" },
      { key: "support_angle", label: "Overhang threshold", kind: "number", bambu: "support_threshold_angle", min: 0, max: 90, step: 1, unit: "°" },
      { key: "support_pattern", label: "Support pattern", kind: "choice", bambu: "support_base_pattern", options: ["lines", "grid", "triangles", "concentric", "zigzag", "cross", "gyroid", "lightning", "honeycomb", "octagon"] },
      { key: "support_infill_rate", label: "Support density", kind: "number", bambu: "support_density", min: 0, max: 100, step: 5, unit: "%" },
      { key: "support_z_distance", label: "Top gap", kind: "number", bambu: "support_top_z_distance", min: 0, max: 2, step: 0.05, unit: "mm" },
      { key: "support_xy_distance", label: "XY gap", kind: "number", bambu: "support_object_xy_distance", min: 0, max: 5, step: 0.1, unit: "mm" },
      { key: "support_interface_enable", label: "Interface layers", kind: "toggle", bambu: "support_interface_top_layers" },
    ],
  },
  {
    id: 'adhesion',
    label: 'Build plate',
    fields: [
      { key: "adhesion_type", label: "Build plate adhesion", kind: "choice", bambu: "brim_type", options: ["skirt", "brim", "raft", "none"] },
      { key: "brim_width", label: "Brim width", kind: "number", bambu: "brim_width", min: 0, max: 20, step: 1, unit: "mm" },
      { key: "skirt_line_count", label: "Skirt loops", kind: "number", bambu: "skirt_loops", min: 0, max: 10, step: 1 },
      { key: "skirt_gap", label: "Skirt gap", kind: "number", bambu: "skirt_distance", min: 0, max: 20, step: 0.5, unit: "mm" },
      { key: "draft_shield_enabled", label: "Draft shield", kind: "toggle", bambu: "draft_shield" },
      { key: "prime_tower_enable", label: "Prime tower", kind: "toggle", bambu: "enable_prime_tower" },
    ],
  },
  {
    id: 'speed',
    label: 'Speed',
    fields: [
      { key: "speed_print", label: "Print speed", kind: "number", bambu: "print speed", min: 10, max: 500, step: 1, unit: "mm/s" },
      { key: "speed_layer_0", label: "Initial layer", kind: "number", bambu: "initial_layer_speed", min: 5, max: 200, step: 1, unit: "mm/s" },
      { key: "speed_wall_0", label: "Outer wall", kind: "number", bambu: "outer_wall_speed", min: 5, max: 500, step: 1, unit: "mm/s" },
      { key: "speed_wall", label: "Inner wall", kind: "number", bambu: "inner_wall_speed", min: 5, max: 500, step: 1, unit: "mm/s" },
      { key: "speed_infill", label: "Sparse infill", kind: "number", bambu: "sparse_infill_speed", min: 5, max: 500, step: 1, unit: "mm/s" },
      { key: "speed_topbottom", label: "Top/bottom", kind: "number", bambu: "top_surface_speed", min: 5, max: 500, step: 1, unit: "mm/s" },
      { key: "speed_travel", label: "Travel", kind: "number", bambu: "travel_speed", min: 10, max: 900, step: 5, unit: "mm/s" },
      { key: "speed_support", label: "Support", kind: "number", bambu: "support_speed", min: 5, max: 500, step: 1, unit: "mm/s" },
      { key: "bridge_wall_speed", label: "Bridge wall", kind: "number", bambu: "bridge_speed", min: 5, max: 200, step: 1, unit: "mm/s" },
      { key: "bridge_skin_speed", label: "Bridge skin", kind: "number", bambu: "internal_bridge_speed", min: 5, max: 200, step: 1, unit: "mm/s" },
      { key: "acceleration_enabled", label: "Limit acceleration", kind: "toggle", bambu: "" },
      { key: "acceleration_print", label: "Print acceleration", kind: "number", bambu: "default_acceleration", min: 100, max: 20000, step: 100, unit: "mm/s²" },
      { key: "acceleration_wall_0", label: "Outer wall acceleration", kind: "number", bambu: "outer_wall_acceleration", min: 100, max: 20000, step: 100, unit: "mm/s²" },
    ],
  },
  {
    id: 'cooling',
    label: 'Cooling',
    fields: [
      { key: "cool_fan_enabled", label: "Part cooling", kind: "toggle", bambu: "" },
      { key: "cool_fan_speed", label: "Fan speed", kind: "number", bambu: "fan_max_speed", min: 0, max: 100, step: 5, unit: "%" },
      { key: "cool_fan_speed_min", label: "Fan minimum", kind: "number", bambu: "fan_min_speed", min: 0, max: 100, step: 5, unit: "%" },
      { key: "cool_fan_speed_max", label: "Fan maximum", kind: "number", bambu: "fan_max_speed", min: 0, max: 100, step: 5, unit: "%" },
      { key: "cool_fan_full_at_height", label: "Full fan at height", kind: "number", bambu: "close_fan_the_first_x_layers", min: 0, max: 10, step: 0.2, unit: "mm" },
      { key: "cool_min_layer_time", label: "Minimum layer time", kind: "number", bambu: "slow_down_layer_time", min: 1, max: 60, step: 1, unit: "s" },
      { key: "cool_min_speed", label: "Minimum speed", kind: "number", bambu: "slow_down_min_speed", min: 1, max: 60, step: 1, unit: "mm/s" },
    ],
  },
  {
    id: 'retraction',
    label: 'Retraction',
    fields: [
      { key: "retraction_enable", label: "Retract on travel", kind: "toggle", bambu: "" },
      { key: "retraction_amount", label: "Retraction length", kind: "number", bambu: "retraction_length", min: 0, max: 10, step: 0.1, unit: "mm" },
      { key: "retraction_speed", label: "Retraction speed", kind: "number", bambu: "retraction_speed", min: 1, max: 120, step: 1, unit: "mm/s" },
      { key: "retraction_prime_speed", label: "Prime speed", kind: "number", bambu: "deretraction_speed", min: 1, max: 120, step: 1, unit: "mm/s" },
      { key: "retraction_hop_enabled", label: "Z hop on travel", kind: "toggle", bambu: "" },
      { key: "retraction_hop", label: "Z hop height", kind: "number", bambu: "z_hop", min: 0, max: 5, step: 0.1, unit: "mm" },
      { key: "retraction_combing", label: "Combing", kind: "choice", bambu: "reduce_crossing_wall", options: ["off", "all", "no_outer_surfaces", "noskin", "infill"] },
    ],
  },
  {
    id: 'ironing',
    label: 'Ironing',
    fields: [
      { key: "ironing_enabled", label: "Iron top surfaces", kind: "toggle", bambu: "enable_ironing" },
      { key: "ironing_flow", label: "Ironing flow", kind: "number", bambu: "ironing_flow", min: 0, max: 50, step: 1, unit: "%" },
      { key: "ironing_line_spacing", label: "Ironing spacing", kind: "number", bambu: "ironing_spacing", min: 0.02, max: 1, step: 0.01, unit: "mm" },
      { key: "ironing_inset", label: "Ironing inset", kind: "number", bambu: "ironing_inset", min: 0, max: 2, step: 0.05, unit: "mm" },
      { key: "ironing_pattern", label: "Ironing pattern", kind: "choice", bambu: "", options: ["concentric", "zigzag"] },
      { key: "ironing_monotonic", label: "Monotonic ironing", kind: "toggle", bambu: "" },
    ],
  },
  {
    id: 'special',
    label: 'Special modes',
    fields: [
      { key: "magic_spiralize", label: "Spiralize outer contour", kind: "toggle", bambu: "spiral_mode" },
      { key: "magic_fuzzy_skin_enabled", label: "Fuzzy skin", kind: "toggle", bambu: "fuzzy_skin" },
      { key: "magic_fuzzy_skin_thickness", label: "Fuzz thickness", kind: "number", bambu: "fuzzy_skin_thickness", min: 0.1, max: 2, step: 0.05, unit: "mm" },
      { key: "magic_fuzzy_skin_point_dist", label: "Fuzz spacing", kind: "number", bambu: "fuzzy_skin_point_distance", min: 0.1, max: 2, step: 0.05, unit: "mm" },
      { key: "z_seam_type", label: "Seam position", kind: "choice", bambu: "seam_position", options: ["back", "shortest", "random", "sharpest_corner"] },
      { key: "z_seam_corner", label: "Seam corner", kind: "choice", bambu: "", options: ["z_seam_corner_inner", "z_seam_corner_outer", "z_seam_corner_any", "z_seam_corner_weighted"] },
      { key: "print_sequence", label: "Print order", kind: "choice", bambu: "print_sequence", options: ["all_at_once", "one_at_a_time"] },
      { key: "adaptive_layer_height_enabled", label: "Adaptive layers", kind: "toggle", bambu: "adaptive_layer_height" },
      { key: "bridge_settings_enabled", label: "Bridge tuning", kind: "toggle", bambu: "" },
    ],
  },
  {
    id: 'material',
    label: 'Material',
    fields: [
      { key: "material_flow", label: "Flow", kind: "number", bambu: "filament_flow_ratio", min: 50, max: 150, step: 1, unit: "%" },
      { key: "material_print_temperature_layer_0", label: "First layer hotend", kind: "number", bambu: "nozzle_temperature_initial_layer", min: 150, max: 320, step: 5, unit: "°C" },
      { key: "material_bed_temperature_layer_0", label: "First layer bed", kind: "number", bambu: "hot_plate_temp_initial_layer", min: 0, max: 120, step: 5, unit: "°C" },
      { key: "material_flow_layer_0", label: "First layer flow", kind: "number", bambu: "", min: 50, max: 150, step: 1, unit: "%" },
    ],
  },
];
