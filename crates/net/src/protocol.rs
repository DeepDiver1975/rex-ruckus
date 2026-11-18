//! Wire protocol: messages, encoding and the host's handshake check.

use rr_core::combat::CombatEvent;
use rr_core::defs::WeaponId;
use rr_core::difficulty::Difficulty;
use rr_core::events::{
    combat_event_finite, combat_event_players_ok, mech_event_finite, weapon_event_fits,
};
use rr_core::mechanics::MechEvent;
use rr_core::players::{MAX_PLAYERS, PlayerId};
use rr_core::snapshot::Snapshot;
use rr_core::weapons::WeaponEvent;
use serde::{Deserialize, Serialize};

/// Bump whenever any wire type or snapshot type changes shape.
pub const PROTOCOL_VERSION: u16 = 3;
/// Input frames per datagram: the newest plus the ones before it, so a lost datagram costs
/// nothing.
pub const INPUT_REDUNDANCY: usize = 4;
/// Most events of one kind in one [`TickEvents`] (and of combat, weapon and mech together).
pub const MAX_EVENTS: usize = 256;
/// Longest HUD line in bytes.
pub const MAX_HUD: usize = 120;
/// Netcode protocol id, "RRNCOOP1". Never changes, so a version mismatch always reaches
/// [`check_hello`] and gets a clear refusal instead of a silent netcode deny.
pub const NETCODE_PROTOCOL_ID: u64 = 0x5252_4E43_4F4F_5031;
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

    /// A copy with every field in range: `forward`/`strafe` in [-1, 1], `pitch` within
    /// ±`pitch_limit`, `angle` in [0, TAU), `cycle` in [-1, 1]. Call it on finite frames only.
    pub fn clamped(&self, pitch_limit: f32) -> InputFrame {
        InputFrame {
            forward: self.forward.clamp(-1.0, 1.0),
            strafe: self.strafe.clamp(-1.0, 1.0),
            pitch: self.pitch.clamp(-pitch_limit, pitch_limit),
            angle: self.angle.rem_euclid(std::f32::consts::TAU),
            cycle: self.cycle.clamp(-1, 1),
            ..self.clone()
        }
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
    /// The newest frame last, at most [`INPUT_REDUNDANCY`], seqs strictly increasing.
    Input(Vec<InputFrame>),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Welcome {
    pub player: PlayerId,
    pub level: String,
    pub skill: Difficulty,
}

/// The host (re)started `level`; snapshots with `tick <= Load.tick` belong to the old run.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Load {
    pub level: String,
    pub skill: Difficulty,
    pub tick: u32,
}

/// A player name: 1 to [`MAX_NAME`] bytes, no control characters.
pub fn valid_player_name(name: &str) -> bool {
    !name.is_empty() && name.len() <= MAX_NAME && !name.chars().any(char::is_control)
}

/// Whether `name` is a level file name a peer may ask for: 1-64 bytes of `[a-z0-9_]` then `.ron`.
pub fn valid_level_name(name: &str) -> bool {
    match name.strip_suffix(".ron") {
        Some(stem) => {
            (1..=64).contains(&stem.len())
                && stem
                    .bytes()
                    .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_')
        }
        None => false,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Refusal {
    Protocol { host: u16 },
    GameVersion,
    Assets,
    Full,
    BadName,
}

/// What the host's simulation reported in one tick, for the client's sounds, effects and HUD.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TickEvents {
    pub tick: u32,
    pub combat: Vec<CombatEvent>,
    /// Weapon events with the player they belong to.
    pub weapon: Vec<(PlayerId, WeaponEvent)>,
    pub mech: Vec<MechEvent>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[allow(clippy::large_enum_variant)] // snapshots dominate by design; messages are short-lived
pub enum ServerMsg {
    Welcome(Welcome),
    Refused(Refusal),
    /// `you` is the receiver's own seat, so a client always knows its id.
    Snapshot {
        you: PlayerId,
        snap: Snapshot,
    },
    /// Every player's name, index = `PlayerId`.
    Roster(Vec<String>),
    Load(Load),
    /// What happened in one host tick (reliable).
    Events(TickEvents),
    /// A HUD line for this client only (reliable): at most [`MAX_HUD`] bytes, no control
    /// characters.
    Hud(String),
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

    /// Ours, and fit for the game list: a player name, a level file name and a player count
    /// within [`MAX_PLAYERS`].
    pub fn is_listable(&self) -> bool {
        self.is_ours()
            && valid_player_name(&self.name)
            && valid_level_name(&self.level)
            && self.players <= self.max
            && usize::from(self.max) <= MAX_PLAYERS
    }
}

#[derive(Debug, thiserror::Error)]
pub enum NetError {
    #[error("bad message: {0}")]
    Decode(#[from] postcard::Error),
    #[error("snapshot holds a non-finite number")]
    NonFinite,
    #[error("bytes left over after the message")]
    Trailing,
    #[error("snapshot is out of bounds (too many players or actors, or a bad player id)")]
    Invalid,
}

pub fn encode<T: Serialize>(msg: &T) -> Vec<u8> {
    postcard::to_stdvec(msg).expect("wire types serialise to a Vec")
}

/// Decodes one whole message: bytes left over after it are an error. Never panics, but checks
/// only the encoding: it is for trusted or local data (tests, tools). Hosts decode what they
/// receive with [`decode_client`], clients with [`decode_server`].
pub fn decode<'a, T: Deserialize<'a>>(bytes: &'a [u8]) -> Result<T, NetError> {
    let (msg, rest) = postcard::take_from_bytes(bytes)?;
    if rest.is_empty() {
        Ok(msg)
    } else {
        Err(NetError::Trailing)
    }
}

/// [`decode`] for what clients receive: a snapshot with a NaN or infinity, or one out of
/// bounds ([`Snapshot::is_sane`], or `you` not a seat in it), is rejected; so are an `Events` stream over [`MAX_EVENTS`], with a non-finite float or a
/// player id past `MAX_PLAYERS`, a `Hud` line over [`MAX_HUD`] bytes or with control
/// characters, a roster that
/// is too long or has an over-long name, a `Welcome` to a seat past `MAX_PLAYERS`, and a
/// `Welcome`/`Load` with a bad level name.
pub fn decode_server(bytes: &[u8]) -> Result<ServerMsg, NetError> {
    let msg: ServerMsg = decode(bytes)?;
    match &msg {
        ServerMsg::Snapshot { you, snap }
            if !snap.is_sane() || you.0 as usize >= snap.players.len() =>
        {
            Err(NetError::Invalid)
        }
        ServerMsg::Snapshot { snap, .. } if !snap.is_finite() => Err(NetError::NonFinite),
        ServerMsg::Roster(names)
            if names.len() > MAX_PLAYERS || names.iter().any(|n| n.len() > MAX_NAME) =>
        {
            Err(NetError::Invalid)
        }
        ServerMsg::Welcome(Welcome { player, .. }) if usize::from(player.0) >= MAX_PLAYERS => {
            Err(NetError::Invalid)
        }
        ServerMsg::Welcome(Welcome { level, .. }) | ServerMsg::Load(Load { level, .. })
            if !valid_level_name(level) =>
        {
            Err(NetError::Invalid)
        }
        ServerMsg::Events(ev) => check_events(ev).map(|()| msg),
        ServerMsg::Hud(line) if line.len() > MAX_HUD || line.chars().any(char::is_control) => {
            Err(NetError::Invalid)
        }
        _ => Ok(msg),
    }
}

/// Caps, player ids and finiteness of one tick's events. Indices into the world are the
/// client's to check.
fn check_events(ev: &TickEvents) -> Result<(), NetError> {
    if ev.combat.len() > MAX_EVENTS || ev.weapon.len() > MAX_EVENTS || ev.mech.len() > MAX_EVENTS {
        return Err(NetError::Invalid);
    }
    if !ev.combat.iter().all(combat_event_players_ok)
        || ev
            .weapon
            .iter()
            .any(|(p, _)| usize::from(p.0) >= MAX_PLAYERS)
    {
        return Err(NetError::Invalid);
    }
    // `weapon_event_fits` is world-free: finite directions and a pellet cap.
    if !ev.combat.iter().all(combat_event_finite)
        || !ev.mech.iter().all(mech_event_finite)
        || !ev.weapon.iter().all(|(_, e)| weapon_event_fits(e))
    {
        return Err(NetError::NonFinite);
    }
    Ok(())
}

/// [`decode`] for what the host receives: the host uses it for every datagram. An `Input` with
/// more than [`INPUT_REDUNDANCY`] frames or seqs that do not strictly increase is `Invalid`,
/// one with a NaN or infinity `NonFinite`. Clamping to range lives in [`InputFrame::clamped`], which the
/// host calls where it applies the input, not here.
pub fn decode_client(bytes: &[u8]) -> Result<ClientMsg, NetError> {
    let msg: ClientMsg = decode(bytes)?;
    match &msg {
        ClientMsg::Input(frames)
            if frames.len() > INPUT_REDUNDANCY
                || frames.windows(2).any(|w| w[0].seq >= w[1].seq) =>
        {
            Err(NetError::Invalid)
        }
        ClientMsg::Input(frames) if !frames.iter().all(InputFrame::is_finite) => {
            Err(NetError::NonFinite)
        }
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
    if !valid_player_name(&hello.name) {
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
    use rr_core::weapons::WeaponEvent;

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

    fn snap_msg(snap: Snapshot) -> ServerMsg {
        ServerMsg::Snapshot {
            you: PlayerId(0),
            snap,
        }
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
        let msg = snap_msg(s);
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
        for m in [ClientMsg::Hello(hello()), ClientMsg::Input(vec![input])] {
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
            snap_msg(arena_snapshot()),
            ServerMsg::Roster(vec!["Rex".into(), "Pal".into()]),
            ServerMsg::Load(Load {
                level: "arsenal_depot.ron".into(),
                skill: Difficulty::Easy,
                tick: 77,
            }),
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
        let good = encode(&snap_msg(arena_snapshot()));
        assert!(decode::<ServerMsg>(&good[..good.len() / 2]).is_err());
        assert!(decode_server(&good[..good.len() - 1]).is_err());
    }

    #[test]
    fn decode_rejects_non_finite_snapshot() {
        let mut s = arena_snapshot();
        s.actors[0].angle = f32::NAN;
        let bytes = encode(&snap_msg(s));
        assert!(matches!(decode_server(&bytes), Err(NetError::NonFinite)));
        // The generic decode does not look inside.
        assert!(decode::<ServerMsg>(&bytes).is_ok());
    }

    #[test]
    fn decode_rejects_trailing_bytes() {
        let mut bytes = encode(&ClientMsg::Hello(hello()));
        assert!(decode::<ClientMsg>(&bytes).is_ok());
        bytes.push(0);
        assert!(matches!(
            decode::<ClientMsg>(&bytes),
            Err(NetError::Trailing)
        ));
        assert!(matches!(decode_client(&bytes), Err(NetError::Trailing)));
        let mut bytes = encode(&ServerMsg::Refused(Refusal::Full));
        bytes.extend([1, 2, 3]);
        assert!(matches!(decode_server(&bytes), Err(NetError::Trailing)));
    }

    #[test]
    fn decode_server_rejects_insane_snapshot() {
        let good = arena_snapshot();
        assert!(decode_server(&encode(&snap_msg(good.clone()))).is_ok());
        let mut s = good.clone();
        s.players[1].id = s.players[0].id;
        assert!(matches!(
            decode_server(&encode(&snap_msg(s))),
            Err(NetError::Invalid)
        ));
        let mut s = good;
        let last = s.actors.last().unwrap().clone();
        s.actors.resize(rr_core::snapshot::MAX_ACTORS + 1, last);
        assert!(matches!(
            decode_server(&encode(&snap_msg(s))),
            Err(NetError::Invalid)
        ));
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
        let good = ClientMsg::Input(vec![frame.clone()]);
        assert_eq!(decode_client(&encode(&good)).unwrap(), good);
        let bad = ClientMsg::Input(vec![InputFrame {
            forward: f32::NAN,
            ..frame
        }]);
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
        let n = encode(&snap_msg(s.clone())).len();
        eprintln!("undertow snapshot: {} actors, {n} bytes", s.actors.len());
        assert!(n <= SNAPSHOT_BUDGET, "{n} bytes");
    }

    fn frames(seqs: &[u32]) -> Vec<InputFrame> {
        seqs.iter()
            .map(|&seq| InputFrame {
                seq,
                forward: 1.0,
                strafe: 0.0,
                angle: 0.0,
                pitch: 0.0,
                buttons: 0,
                select: None,
                cycle: 0,
            })
            .collect()
    }

    #[test]
    fn input_bounds() {
        let enc = |f: Vec<InputFrame>| encode(&ClientMsg::Input(f));
        let ok = ClientMsg::Input(frames(&[1, 2, 3, 4]));
        assert_eq!(decode_client(&encode(&ok)).unwrap(), ok);
        assert!(matches!(
            decode_client(&enc(frames(&[1, 2, 3, 4, 5]))),
            Err(NetError::Invalid)
        ));
        assert!(matches!(
            decode_client(&enc(frames(&[3, 3]))),
            Err(NetError::Invalid)
        ));
        assert!(matches!(
            decode_client(&enc(frames(&[5, 4]))),
            Err(NetError::Invalid)
        ));
        let mut f = frames(&[1, 2]);
        f[0].forward = f32::NAN;
        assert!(matches!(decode_client(&enc(f)), Err(NetError::NonFinite)));
    }

    fn events(combat: Vec<CombatEvent>) -> ServerMsg {
        ServerMsg::Events(TickEvents {
            tick: 5,
            combat,
            weapon: vec![],
            mech: vec![],
        })
    }

    fn impact() -> CombatEvent {
        CombatEvent::Impact {
            point: Vec3::ONE,
            normal: Vec3::Z,
            sector: 0,
            wall: None,
        }
    }

    #[test]
    fn events_bounds() {
        let ok = events(vec![impact(); MAX_EVENTS]);
        assert_eq!(decode_server(&encode(&ok)).unwrap(), ok);
        assert!(encode(&ok).len() < 8192);
        assert!(matches!(
            decode_server(&encode(&events(vec![impact(); MAX_EVENTS + 1]))),
            Err(NetError::Invalid)
        ));
        let nan = events(vec![CombatEvent::Explosion {
            point: Vec3::new(f32::NAN, 0.0, 0.0),
            radius: 1.0,
        }]);
        assert!(matches!(
            decode_server(&encode(&nan)),
            Err(NetError::NonFinite)
        ));
        let hurt = events(vec![CombatEvent::PlayerHurt {
            player: PlayerId(4),
            amount: 1,
            from: Vec3::ZERO,
        }]);
        assert!(matches!(
            decode_server(&encode(&hurt)),
            Err(NetError::Invalid)
        ));
        let weapon = |id, dir: Vec3| {
            ServerMsg::Events(TickEvents {
                tick: 1,
                combat: vec![],
                weapon: vec![(id, WeaponEvent::Kick { dir })],
                mech: vec![MechEvent::Exit],
            })
        };
        assert!(decode_server(&encode(&weapon(PlayerId(3), Vec3::X))).is_ok());
        assert!(matches!(
            decode_server(&encode(&weapon(PlayerId(4), Vec3::X))),
            Err(NetError::Invalid)
        ));
        assert!(matches!(
            decode_server(&encode(&weapon(PlayerId(0), Vec3::NAN))),
            Err(NetError::NonFinite)
        ));
    }

    #[test]
    fn hud_bounds() {
        let hud = |t: &str| encode(&ServerMsg::Hud(t.into()));
        assert!(decode_server(&hud(&"a".repeat(MAX_HUD))).is_ok());
        assert!(matches!(
            decode_server(&hud(&"a".repeat(MAX_HUD + 1))),
            Err(NetError::Invalid)
        ));
        assert!(matches!(
            decode_server(&hud("a\nb")),
            Err(NetError::Invalid)
        ));
    }

    #[test]
    fn level_names() {
        assert!(valid_level_name("undertow.ron"));
        assert!(valid_level_name("arsenal_depot.ron"));
        for bad in [
            "",
            ".ron",
            "../x.ron",
            "a/b.ron",
            "a\\b.ron",
            "Undertow.ron",
            "x.txt",
            "x.ron\0",
        ] {
            assert!(!valid_level_name(bad), "{bad:?}");
        }
        assert!(valid_level_name(&format!("{}.ron", "a".repeat(64))));
        assert!(!valid_level_name(&format!("{}.ron", "a".repeat(65))));
    }

    #[test]
    fn clamped_input() {
        let f = InputFrame {
            seq: 1,
            forward: 3.0,
            strafe: -9.0,
            angle: -0.5,
            pitch: 2.0,
            buttons: 0,
            select: None,
            cycle: 5,
        };
        let c = f.clamped(1.45);
        assert_eq!(c.forward, 1.0);
        assert_eq!(c.strafe, -1.0);
        assert_eq!(c.pitch, 1.45);
        assert!((c.angle - (std::f32::consts::TAU - 0.5)).abs() < 1e-5);
        assert_eq!(c.cycle, 1);
        let low = InputFrame {
            pitch: -2.0,
            cycle: -7,
            ..f
        };
        let c = low.clamped(1.45);
        assert_eq!(c.pitch, -1.45);
        assert_eq!(c.cycle, -1);
    }

    #[test]
    fn snapshot_you_out_of_range_is_invalid() {
        let snap = arena_snapshot();
        assert_eq!(snap.players.len(), 2);
        let bad = ServerMsg::Snapshot {
            you: PlayerId(2),
            snap: snap.clone(),
        };
        assert!(matches!(
            decode_server(&encode(&bad)),
            Err(NetError::Invalid)
        ));
        let ok = ServerMsg::Snapshot {
            you: PlayerId(1),
            snap,
        };
        assert!(decode_server(&encode(&ok)).is_ok());
    }

    #[test]
    fn bad_level_in_welcome_or_load_is_invalid() {
        for level in ["../x.ron", ""] {
            let w = ServerMsg::Welcome(Welcome {
                player: PlayerId(1),
                level: level.into(),
                skill: Difficulty::Normal,
            });
            assert!(matches!(decode_server(&encode(&w)), Err(NetError::Invalid)));
            let l = ServerMsg::Load(Load {
                level: level.into(),
                skill: Difficulty::Normal,
                tick: 0,
            });
            assert!(matches!(decode_server(&encode(&l)), Err(NetError::Invalid)));
        }
    }

    #[test]
    fn welcome_to_a_seat_past_the_last_is_invalid() {
        let w = |id: usize| {
            encode(&ServerMsg::Welcome(Welcome {
                player: PlayerId(id as u8),
                level: "test_yard.ron".into(),
                skill: Difficulty::Normal,
            }))
        };
        assert!(decode_server(&w(MAX_PLAYERS - 1)).is_ok());
        for id in [MAX_PLAYERS, 255] {
            assert!(
                matches!(decode_server(&w(id)), Err(NetError::Invalid)),
                "seat {id}"
            );
        }
    }

    #[test]
    fn oversized_roster_is_invalid() {
        let names = |n: usize| vec!["Rex".to_string(); n];
        assert!(decode_server(&encode(&ServerMsg::Roster(names(MAX_PLAYERS)))).is_ok());
        assert!(matches!(
            decode_server(&encode(&ServerMsg::Roster(names(MAX_PLAYERS + 1)))),
            Err(NetError::Invalid)
        ));
        let long = ServerMsg::Roster(vec!["x".repeat(MAX_NAME + 1)]);
        assert!(matches!(
            decode_server(&encode(&long)),
            Err(NetError::Invalid)
        ));
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
        for bad in ["Rex\n", "\u{1b}[2J", "A\u{7f}"] {
            h.name = bad.into();
            assert_eq!(
                check_hello(&h, "0.2.0", 99, 1),
                Err(Refusal::BadName),
                "{bad:?}"
            );
        }
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

    #[test]
    fn only_sane_beacons_are_listable() {
        let b = Beacon {
            magic: BEACON_MAGIC,
            protocol: PROTOCOL_VERSION,
            name: "Rex".into(),
            level: "undertow.ron".into(),
            players: 4,
            max: 4,
            port: GAME_PORT,
        };
        assert!(b.is_listable());
        let bad = [
            Beacon {
                magic: 1,
                ..b.clone()
            },
            Beacon {
                name: "x".repeat(MAX_NAME + 1),
                ..b.clone()
            },
            Beacon {
                name: String::new(),
                ..b.clone()
            },
            Beacon {
                name: "R\nex".into(),
                ..b.clone()
            },
            Beacon {
                level: "../x.ron".into(),
                ..b.clone()
            },
            Beacon {
                players: 5,
                max: 4,
                ..b.clone()
            },
            Beacon {
                players: 2,
                max: MAX_PLAYERS as u8 + 1,
                ..b.clone()
            },
        ];
        for bad in bad {
            assert!(!bad.is_listable(), "{bad:?}");
        }
    }
}
