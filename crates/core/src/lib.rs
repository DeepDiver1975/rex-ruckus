//! Build-inspired game core: sector maps, mesh extrusion, collision and movement.
//! Coordinates are right-handed, Z-up, in metres (x = east, y = north).

pub mod collide;
pub mod extrude;
pub mod fixtures;
pub mod geom;
pub mod interact;
pub mod map;
pub mod mechanics;
pub mod movement;
pub mod validate;

pub use glam;
