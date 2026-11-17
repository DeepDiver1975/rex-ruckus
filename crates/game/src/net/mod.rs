//! Co-op networking on the game side. `NetMode` says whether this process is a plain single-player
//! game, a host, or a client; an absent resource means `Single`.

use bevy::prelude::*;

pub mod client;
pub mod host;
pub mod lobby;

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
