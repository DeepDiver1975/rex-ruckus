//! Wire layer for network co-op: protocol constants, messages, postcard encoding, the
//! handshake check, the LAN beacon and the asset hash. Sockets live in `session` (renet
//! host and client) and `lan` (beacons); PR 5 adds the menus and the lobby, PR 6 client prediction, interpolation and
//! the event relay.

pub mod hash;
pub mod lan;
pub mod protocol;
pub mod session;

pub use hash::assets_hash;
pub use lan::{BeaconSender, LanListener};
pub use protocol::*;
pub use session::*;
