//! Wire layer for network co-op: protocol constants, messages, postcard encoding, the
//! handshake check, the LAN beacon and the asset hash. No sockets here: PR 5 adds the UDP
//! transport and the host and client loops, PR 6 the lobby and menus.

pub mod hash;
pub mod protocol;

pub use hash::assets_hash;
pub use protocol::*;
