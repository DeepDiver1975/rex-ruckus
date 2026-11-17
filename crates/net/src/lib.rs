//! Wire layer for network co-op: protocol constants, messages, postcard encoding, the
//! handshake check, the LAN beacon and the asset hash. No sockets here: PR 5 adds the UDP
//! transport, LAN discovery, the menus and the lobby; PR 6 client prediction, interpolation and
//! the event relay.

pub mod hash;
pub mod protocol;

pub use hash::assets_hash;
pub use protocol::*;
