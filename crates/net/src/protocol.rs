//! Wire protocol: messages, encoding and the host's handshake check.

use rr_core::defs::WeaponId;
use rr_core::difficulty::Difficulty;
use rr_core::players::{MAX_PLAYERS, PlayerId};
use rr_core::snapshot::Snapshot;
use serde::{Deserialize, Serialize};

/// Bump whenever any wire type or snapshot type changes shape.
pub const PROTOCOL_VERSION: u16 = 1;
/// Game UDP port.
pub const GAME_PORT: u16 = 27960;
/// LAN beacon UDP port.
pub const BEACON_PORT: u16 = 27961;
/// "RRN1".
pub const BEACON_MAGIC: u32 = 0x5252_4E31;
/// Largest encoded snapshot we plan for, in bytes.
pub const SNAPSHOT_BUDGET: usize = 8192;
/// Longest player name in bytes; longer names are refused (`BadName`).
pub const MAX_NAME: usize = 24;

/// One tick of a client's controls.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct InputFrame {
    pub seq: u32,
    pub forward: f32,
    pub strafe: f32,
    pub angle: f32,
    pub pitch: f32,
    /// `buttons` bits.
    pub buttons: u16,
    pub select: Option<WeaponId>,
    pub cycle: i8,
}

impl InputFrame {
    /// Whether every float is finite.
    pub fn is_finite(&self) -> bool {
        self.forward.is_finite()
            && self.strafe.is_finite()
            && self.angle.is_finite()
            && self.pitch.is_finite()
    }
}

/// Bit values of [`InputFrame::buttons`].
pub mod buttons {
    pub const JUMP: u16 = 1;
    pub const CROUCH: u16 = 2;
    pub const USE: u16 = 4;
    pub const FIRE: u16 = 8;
    pub const FIRE_PRESSED: u16 = 16;
    pub const RELOAD: u16 = 32;
    pub const KICK: u16 = 64;
    pub const MEDKIT: u16 = 128;
    pub const JETPACK: u16 = 256;
    pub const NIGHT_VISION: u16 = 512;
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Hello {
    pub protocol: u16,
    pub game_version: String,
    pub assets: u64,
    pub name: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum ClientMsg {
    Hello(Hello),
    Input(InputFrame),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Welcome {
    pub player: PlayerId,
    pub level: String,
    pub skill: Difficulty,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Refusal {
    Protocol { host: u16 },
    GameVersion,
    Assets,
    Full,
    BadName,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[allow(clippy::large_enum_variant)] // snapshots dominate by design; messages are short-lived
pub enum ServerMsg {
    Welcome(Welcome),
    Refused(Refusal),
    Snapshot(Snapshot),
}

/// A host's LAN announcement.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Beacon {
    pub magic: u32,
    pub protocol: u16,
    pub name: String,
    pub level: String,
    pub players: u8,
    pub max: u8,
    pub port: u16,
}

impl Beacon {
    /// Whether this beacon is from a compatible game; readers ignore any other.
    pub fn is_ours(&self) -> bool {
        self.magic == BEACON_MAGIC && self.protocol == PROTOCOL_VERSION
    }
}

#[derive(Debug, thiserror::Error)]
pub enum NetError {
    #[error("bad message: {0}")]
    Decode(#[from] postcard::Error),
    #[error("snapshot holds a non-finite number")]
    NonFinite,
}

pub fn encode<T: Serialize>(msg: &T) -> Vec<u8> {
    postcard::to_stdvec(msg).expect("wire types serialise to a Vec")
}

/// Decodes untrusted bytes; never panics.
pub fn decode<'a, T: Deserialize<'a>>(bytes: &'a [u8]) -> Result<T, NetError> {
    Ok(postcard::from_bytes(bytes)?)
}

/// [`decode`] for what clients receive: a snapshot with a NaN or infinity is rejected.
pub fn decode_server(bytes: &[u8]) -> Result<ServerMsg, NetError> {
    let msg: ServerMsg = decode(bytes)?;
    match &msg {
        ServerMsg::Snapshot(s) if !s.is_finite() => Err(NetError::NonFinite),
        _ => Ok(msg),
    }
}

/// [`decode`] for what the host receives: the host uses it for every datagram. An `Input` with a
/// NaN or infinity is rejected. Clamping `forward`/`strafe` to [-1, 1] and `pitch` to its range
/// happens where the host applies the input (PR 5), not here.
pub fn decode_client(bytes: &[u8]) -> Result<ClientMsg, NetError> {
    let msg: ClientMsg = decode(bytes)?;
    match &msg {
        ClientMsg::Input(i) if !i.is_finite() => Err(NetError::NonFinite),
        _ => Ok(msg),
    }
}

/// Host side of the handshake: protocol, then game version, then assets, then name, then room.
/// `players` counts those already in the game.
pub fn check_hello(
    hello: &Hello,
    game_version: &str,
    assets: u64,
    players: usize,
) -> Result<(), Refusal> {
    if hello.protocol != PROTOCOL_VERSION {
        return Err(Refusal::Protocol {
            host: PROTOCOL_VERSION,
        });
    }
    if hello.game_version != game_version {
        return Err(Refusal::GameVersion);
    }
    if hello.assets != assets {
        return Err(Refusal::Assets);
    }
    if hello.name.is_empty() || hello.name.len() > MAX_NAME {
        return Err(Refusal::BadName);
    }
    if players >= MAX_PLAYERS {
        return Err(Refusal::Full);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use rr_core::collide::Body;
    use rr_core::combat::{Combat, PlayerTarget, level_seed};
    use rr_core::defs::Defs;
    use rr_core::difficulty::Difficulty;
    use rr_core::glam::{Vec2, Vec3};
    use rr_core::inventory::Inventory;
    use rr_core::map::{KeySet, Map};
    use rr_core::mechanics::Mechanics;
    use rr_core::players::{MAX_PLAYERS, PlayerId};
    use rr_core::projectile::{Projectile, Shooter, Targets};
    use rr_core::snapshot::{PlayerSnap, Snapshot};
    use rr_core::vitals::Vitals;
    use rr_core::weapons::Arsenal;

    const DT: f32 = 1.0 / 60.0;

    fn level(name: &str) -> String {
        let p = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../assets/levels")
            .join(name);
        std::fs::read_to_string(p).expect("level file")
    }

    fn bolt(id: u32, at: Vec3) -> Projectile {
        Projectile {
            id,
            pos: at,
            prev: at,
            vel: Vec3::X * 20.0,
            sector: 0,
            radius: 0.1,
            damage: 10,
            owner: Shooter::Actor(0),
            targets: Targets::Players,
            life: 5.0,
            gravity: 0.0,
            bounce: None,
            remote: false,
            splash: None,
            resting: false,
        }
    }

    /// A mid-fight snapshot of `file` with `players` players and `shots` projectiles.
    fn snapshot_of(file: &str, at: Vec2, ticks: usize, players: usize, shots: u32) -> Snapshot {
        let defs = Defs::builtin();
        let source = Map::from_ron(&level(file)).expect("level parses");
        let mut map = source.clone();
        let mut mech = Mechanics::new(&mut map);
        let mut combat = Combat::spawn(&map, &defs, level_seed(&map.name));
        combat.skill = Difficulty::Normal;
        let mut body = Body::spawn(&map, at, 0.35, 1.8).expect("player spot");
        let mut vitals = Vitals::new();
        for _ in 0..ticks {
            let eye = body.pos + Vec3::Z * 0.9 * body.height;
            let mut p = PlayerTarget {
                body: &mut body,
                vitals: &mut vitals,
                eye,
            };
            combat.tick(&mut map, &mut mech, &defs, std::slice::from_mut(&mut p), DT);
            let (idx, mut bodies) = combat.living_bodies();
            mech.tick(&mut map, &mut bodies, DT);
            combat.write_back(&idx, &bodies);
        }
        for i in 0..shots {
            combat
                .projectiles
                .push(bolt(100 + i, Vec3::new(at.x, at.y, 1.0)));
        }
        let snaps = (0..players as u8)
            .map(|i| PlayerSnap {
                id: PlayerId(i),
                body,
                angle: 0.5 + f32::from(i),
                pitch: -0.25,
                vitals: Vitals::new(),
                arsenal: Arsenal::new(&defs),
                inventory: Inventory::new(),
                keys: KeySet::default(),
                ack: 1000 + u32::from(i),
            })
            .collect();
        Snapshot::capture(7, &combat, &mech, &map, &source, snaps)
    }

    fn arena_snapshot() -> Snapshot {
        snapshot_of("combat_arena.ron", Vec2::new(5.0, 10.0), 120, 2, 3)
    }

    fn hello() -> Hello {
        Hello {
            protocol: PROTOCOL_VERSION,
            game_version: "0.2.0".into(),
            assets: 99,
            name: "Rex".into(),
        }
    }

    #[test]
    fn snapshot_round_trips() {
        let s = arena_snapshot();
        assert_eq!(s.players.len(), 2);
        assert!(!s.projectiles.is_empty());
        let msg = ServerMsg::Snapshot(s);
        let back: ServerMsg = decode(&encode(&msg)).unwrap();
        assert_eq!(back, msg);
        assert_eq!(decode_server(&encode(&msg)).unwrap(), msg);
    }

    #[test]
    fn messages_round_trip() {
        let input = InputFrame {
            seq: 9,
            forward: 1.0,
            strafe: -0.5,
            angle: 2.0,
            pitch: 0.1,
            buttons: buttons::FIRE | buttons::JUMP,
            select: Some(rr_core::defs::WeaponId::ALL[1]),
            cycle: -1,
        };
        for m in [ClientMsg::Hello(hello()), ClientMsg::Input(input)] {
            assert_eq!(decode::<ClientMsg>(&encode(&m)).unwrap(), m);
        }
        let servers = [
            ServerMsg::Welcome(Welcome {
                player: PlayerId(2),
                level: "undertow.ron".into(),
                skill: Difficulty::Hard,
            }),
            ServerMsg::Refused(Refusal::Protocol { host: 3 }),
            ServerMsg::Refused(Refusal::Full),
            ServerMsg::Snapshot(arena_snapshot()),
        ];
        for m in servers {
            assert_eq!(decode::<ServerMsg>(&encode(&m)).unwrap(), m);
        }
        let b = Beacon {
            magic: BEACON_MAGIC,
            protocol: PROTOCOL_VERSION,
            name: "Rex's game".into(),
            level: "undertow.ron".into(),
            players: 2,
            max: 4,
            port: GAME_PORT,
        };
        assert_eq!(decode::<Beacon>(&encode(&b)).unwrap(), b);
    }

    #[test]
    fn decode_rejects_garbage() {
        assert!(decode::<ServerMsg>(&[]).is_err());
        assert!(decode::<ClientMsg>(&[]).is_err());
        // A cheap deterministic byte stream.
        let mut x = 0x1234_5678_u32;
        for len in 1..200 {
            let bytes: Vec<u8> = (0..len)
                .map(|_| {
                    x = x.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                    (x >> 24) as u8
                })
                .collect();
            let _ = decode::<ServerMsg>(&bytes);
            let _ = decode_server(&bytes);
            let _ = decode::<ClientMsg>(&bytes);
            let _ = decode::<Beacon>(&bytes);
        }
        let good = encode(&ServerMsg::Snapshot(arena_snapshot()));
        assert!(decode::<ServerMsg>(&good[..good.len() / 2]).is_err());
        assert!(decode_server(&good[..good.len() - 1]).is_err());
    }

    #[test]
    fn decode_rejects_non_finite_snapshot() {
        let mut s = arena_snapshot();
        s.actors[0].angle = f32::NAN;
        let bytes = encode(&ServerMsg::Snapshot(s));
        assert!(matches!(decode_server(&bytes), Err(NetError::NonFinite)));
        // The generic decode does not look inside.
        assert!(decode::<ServerMsg>(&bytes).is_ok());
    }

    #[test]
    fn decode_client_rejects_non_finite_input() {
        let frame = InputFrame {
            seq: 1,
            forward: 1.0,
            strafe: 0.0,
            angle: 0.0,
            pitch: 0.0,
            buttons: 0,
            select: None,
            cycle: 0,
        };
        let good = ClientMsg::Input(frame.clone());
        assert_eq!(decode_client(&encode(&good)).unwrap(), good);
        let bad = ClientMsg::Input(InputFrame {
            forward: f32::NAN,
            ..frame
        });
        assert!(matches!(
            decode_client(&encode(&bad)),
            Err(NetError::NonFinite)
        ));
        let hello = ClientMsg::Hello(hello());
        assert_eq!(decode_client(&encode(&hello)).unwrap(), hello);
    }

    #[test]
    fn snapshot_fits_budget() {
        let s = snapshot_of("undertow.ron", Vec2::new(0.0, 0.0), 0, MAX_PLAYERS, 32);
        assert_eq!(s.actors.len(), 66);
        assert_eq!(s.players.len(), 4);
        assert_eq!(s.projectiles.len(), 32);
        let n = encode(&ServerMsg::Snapshot(s.clone())).len();
        eprintln!("undertow snapshot: {} actors, {n} bytes", s.actors.len());
        assert!(n <= SNAPSHOT_BUDGET, "{n} bytes");
    }

    #[test]
    fn check_hello_refuses_in_order() {
        let ok = hello();
        assert_eq!(check_hello(&ok, "0.2.0", 99, 1), Ok(()));
        let mut h = ok.clone();
        h.protocol = 0;
        h.assets = 1;
        assert_eq!(
            check_hello(&h, "0.2.0", 99, 1),
            Err(Refusal::Protocol {
                host: PROTOCOL_VERSION
            })
        );
        let mut h = ok.clone();
        h.game_version = "9".into();
        assert_eq!(check_hello(&h, "0.2.0", 99, 1), Err(Refusal::GameVersion));
        assert_eq!(check_hello(&ok, "0.2.0", 5, 1), Err(Refusal::Assets));
        let mut h = ok.clone();
        h.name = "x".repeat(25);
        assert_eq!(check_hello(&h, "0.2.0", 99, 1), Err(Refusal::BadName));
        h.name = String::new();
        assert_eq!(check_hello(&h, "0.2.0", 99, 1), Err(Refusal::BadName));
        h.name = "x".repeat(24);
        assert_eq!(check_hello(&h, "0.2.0", 99, 1), Ok(()));
        assert_eq!(
            check_hello(&ok, "0.2.0", 99, MAX_PLAYERS),
            Err(Refusal::Full)
        );
    }

    #[test]
    fn beacon_is_ours() {
        let b = Beacon {
            magic: BEACON_MAGIC,
            protocol: PROTOCOL_VERSION,
            name: String::new(),
            level: String::new(),
            players: 1,
            max: 4,
            port: GAME_PORT,
        };
        assert!(b.is_ours());
        assert!(
            !Beacon {
                magic: 1,
                ..b.clone()
            }
            .is_ours()
        );
        assert!(
            !Beacon {
                protocol: PROTOCOL_VERSION + 1,
                ..b
            }
            .is_ours()
        );
    }
}
