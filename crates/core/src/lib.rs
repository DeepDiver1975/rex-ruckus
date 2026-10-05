//! Build-inspired game core: sector maps, mesh extrusion, collision and movement.
//! Coordinates are right-handed, Z-up, in metres (x = east, y = north).

pub mod actors;
pub mod collide;
pub mod combat;
pub mod defs;
pub mod extrude;
pub mod fixtures;
pub mod geom;
pub mod health;
pub mod interact;
pub mod map;
pub mod mechanics;
pub mod movement;
pub mod pickups;
pub mod projectile;
pub mod rng;
pub mod trace;
pub mod validate;
pub mod weapons;

pub use glam;
