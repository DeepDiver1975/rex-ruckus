//! Build-inspired game core: sector maps, mesh extrusion, collision and movement.
//! Coordinates are right-handed, Z-up, in metres (x = east, y = north).

pub mod actors;
pub mod audio;
pub mod automap;
pub mod carry;
pub mod collide;
pub mod combat;
pub mod defs;
pub mod destruct;
pub mod difficulty;
pub mod explosion;
pub mod extrude;
pub mod fixtures;
pub mod geom;
pub mod hazard;
pub mod health;
pub mod interact;
pub mod inventory;
pub mod map;
pub mod mechanics;
pub mod models;
pub mod movement;
pub mod pickups;
pub mod projectile;
pub mod props;
pub mod rng;
pub mod stats;
pub mod trace;
pub mod validate;
pub mod vitals;
pub mod weapons;

pub use glam;
