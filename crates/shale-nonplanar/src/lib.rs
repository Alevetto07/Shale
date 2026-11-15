//! Non-planar **curved tops** for Bambu Lab A1: deform the mesh, slice planar with
//! an unmodified CuraEngine, then map G-code back with flow correction and a
//! toolhead collision pass.
//!
//! Pipeline design follows the deform → slice → map-back pattern described in
//! CurviSlicer (ACM TOG 2019) and related work; this crate contains only
//! Shale-original implementations.

pub mod mesh;
#[cfg(test)]
mod test_meshes;
pub mod collision;
pub mod config;
pub mod deform;
pub mod envelope;
pub mod extrusion;
pub mod height_field;
pub mod inverse;
pub mod pipeline;

pub use config::{NonplanarMode, NonplanarOptions, NonplanarReport};
pub use mesh::merge_meshes;
pub use pipeline::{
    apply_post_pass, gcode_respects_envelope, max_top_layer_slope_deg,
    max_top_shell_slope_across_layers_deg, max_top_shell_slope_deg, prepare_pre_pass,
    top_layer_z_variation, top_shell_z_variation, NonplanarContext,
};