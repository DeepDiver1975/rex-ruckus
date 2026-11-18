//! Co-op networking on the game side. `NetMode` says whether this process is a plain single-player
//! game, a host, or a client; an absent resource means `Single`.

use bevy::prelude::*;
use rr_net::{InputFrame, buttons};

use crate::player::{Look, PendingInput};

pub mod client;
pub mod host;
pub mod lobby;
pub mod predict;

/// A co-op session the command line starts at once (`--host`, `--join ADDR`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NetStart {
    Host,
    Join(std::net::SocketAddr),
}

#[derive(Resource, Default, Clone, Copy, PartialEq, Eq, Debug)]
pub enum NetMode {
    #[default]
    Single,
    Host,
    Client,
}

impl NetMode {
    /// Host or client: a net game never stops for a menu.
    pub fn is_net(self) -> bool {
        self != NetMode::Single
    }
}

/// Run condition: this process is not a co-op client (a client never decides restarts).
pub fn not_client(mode: Option<Res<NetMode>>) -> bool {
    mode.as_deref() != Some(&NetMode::Client)
}

/// Drives a player's controls from `frame`, the same way on the host (a seat's frame) and on a
/// client (its own predicted and replayed frames): held controls and the view are set, taps and
/// the weapon choice are latched on top of what `input` already holds.
pub fn apply_frame(frame: &InputFrame, input: &mut PendingInput, look: &mut Look) {
    let held = |bit: u16| frame.buttons & bit != 0;
    look.angle = frame.angle;
    look.pitch = frame.pitch;
    input.forward = frame.forward;
    input.strafe = frame.strafe;
    input.jump = held(buttons::JUMP);
    input.crouch = held(buttons::CROUCH);
    input.fire = held(buttons::FIRE);
    input.use_pressed |= held(buttons::USE);
    input.fire_pressed |= held(buttons::FIRE_PRESSED);
    input.reload |= held(buttons::RELOAD);
    input.kick |= held(buttons::KICK);
    input.use_medkit |= held(buttons::MEDKIT);
    input.toggle_jetpack |= held(buttons::JETPACK);
    input.toggle_nv |= held(buttons::NIGHT_VISION);
    if frame.select.is_some() {
        input.select = frame.select;
    }
    input.cycle += i32::from(frame.cycle);
}
