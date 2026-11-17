//! Hosting a co-op game: the host runs the only simulation, seats the clients that say a good
//! `Hello`, drives their players from their input frames and sends everyone snapshots.
//!
//! Seat `k` plays `PlayerId(k + 1)`; the host's own player is `PlayerId(0)`. When a seat leaves,
//! the seats (and players) above it move down one, so ids stay dense.

use std::io;
use std::net::SocketAddr;

use bevy::prelude::*;
use rr_core::players::{MAX_PLAYERS, PlayerId};
use rr_core::snapshot::{PlayerSnap, Snapshot};
use rr_net::{
    BEACON_MAGIC, Beacon, BeaconSender, ClientKey, ClientMsg, Hello, Host, HostEvent, InputFrame,
    Load, PROTOCOL_VERSION, ServerMsg, Welcome, assets_hash, buttons, check_hello,
    valid_level_name,
};

use super::NetMode;
use crate::combat::{
    CombatSet, GameDefs, LevelCombat, PlayerArsenal, PlayerInventory, PlayerVitals, spawn_combat,
};
use crate::flow::{
    LevelDifficulty, LevelFile, LevelRuns, LevelSource, PlayState, SpawnLevel, advance_level,
    check_player_death,
};
use crate::level::CurrentMap;
use crate::mechanics::{LevelMechanics, forward_mech_events, use_key};
use crate::paths::assets_dir;
use crate::player::{
    Inventory, LocalPlayer, Look, PITCH_LIMIT, PendingInput, Player, PlayerBody, PlayerTuning,
    spawn_coop_player,
};

/// Ticks between snapshots: 30 Hz at the 60 Hz tick.
pub const SNAPSHOT_EVERY: u32 = 2;
/// Seconds a connection may stay without a `Hello` before it is dropped.
pub const HELLO_TIMEOUT: f32 = 5.0;
/// Seconds between LAN beacons.
pub const BEACON_EVERY: f32 = 1.0;

/// Tap bits of [`InputFrame::buttons`]: each press acts once, not for as long as the frame holds.
const TAPS: u16 = buttons::USE
    | buttons::FIRE_PRESSED
    | buttons::RELOAD
    | buttons::KICK
    | buttons::MEDKIT
    | buttons::JETPACK
    | buttons::NIGHT_VISION;

/// Who this process says it is in a co-op handshake.
#[derive(Resource, Clone, Debug, PartialEq, Eq)]
pub struct NetIdentity {
    pub name: String,
    pub game_version: String,
    pub assets: u64,
}

impl NetIdentity {
    /// `name` with this build's version and the hash of its assets.
    pub fn local(name: String) -> io::Result<NetIdentity> {
        Ok(NetIdentity {
            name,
            game_version: env!("CARGO_PKG_VERSION").into(),
            assets: assets_hash(&assets_dir())?,
        })
    }
}

/// A seated client.
#[derive(Debug)]
pub struct Seat {
    pub client: ClientKey,
    pub name: String,
    /// The newest input sequence taken from this client.
    pub last_seq: u32,
    /// The latest frame: held controls stay, taps are cleared once applied.
    pub frame: Option<InputFrame>,
}

/// The hosted game.
#[derive(Resource)]
pub struct NetHost {
    pub host: Host,
    /// Seat `k` plays `PlayerId(k + 1)`.
    pub seats: Vec<Seat>,
    /// Net ticks; snapshots and loads are stamped with it.
    pub tick: u32,
    /// Messages from clients that did not decode; counted and ignored.
    pub garbage: u32,
    /// Connected but not seated yet, with seconds since connecting.
    waiting: Vec<(ClientKey, f32)>,
    /// The [`LevelRuns`] the seats last got a `Load` for.
    runs: u32,
}

impl NetHost {
    fn seat_of(&self, key: ClientKey) -> Option<usize> {
        self.seats.iter().position(|s| s.client == key)
    }

    fn broadcast(&mut self, msg: &ServerMsg) {
        for s in &self.seats {
            self.host.send(s.client, msg);
        }
    }

    fn roster(&self, host_name: &str) -> ServerMsg {
        let names = std::iter::once(host_name.to_string())
            .chain(self.seats.iter().map(|s| s.name.clone()))
            .collect();
        ServerMsg::Roster(names)
    }
}

/// The beacon socket and the time since the last beacon.
#[derive(Resource)]
struct HostBeacon {
    sender: BeaconSender,
    since: f32,
}

/// Why a level cannot be hosted, if it cannot: clients load the [`LevelFile`] by name from
/// their own `assets/levels/`, so it must pass [`valid_level_name`].
pub fn unhostable_level(world: &World) -> Option<String> {
    match world.get_resource::<LevelFile>() {
        Some(file) if valid_level_name(&file.0) => None,
        Some(file) => Some(format!(
            "{:?} cannot be hosted: co-op plays levels from assets/levels",
            file.0
        )),
        None => Some("There is no level to host".into()),
    }
}

/// Opens a co-op game on `addr` and announces it on the LAN. Refuses (`InvalidInput`) a level
/// the clients could not load ([`unhostable_level`]).
pub fn start_hosting(world: &mut World, addr: SocketAddr, name: String) -> io::Result<()> {
    if let Some(why) = unhostable_level(world) {
        return Err(io::Error::new(io::ErrorKind::InvalidInput, why));
    }
    let identity = NetIdentity::local(name)?;
    let host = Host::bind(addr)?;
    let runs = world.get_resource::<LevelRuns>().map_or(0, |r| r.0);
    world.insert_resource(NetHost {
        host,
        seats: Vec::new(),
        tick: 0,
        garbage: 0,
        waiting: Vec::new(),
        runs,
    });
    world.insert_resource(identity);
    world.insert_resource(NetMode::Host);
    match BeaconSender::bind() {
        Ok(sender) => {
            world.insert_resource(HostBeacon {
                sender,
                since: BEACON_EVERY,
            });
        }
        Err(e) => warn!("no LAN beacon (the game is still open by address): {e}"),
    }
    Ok(())
}

/// Closes the hosted game: tells the clients, drops their players and goes back to single play.
pub fn stop_hosting(world: &mut World) {
    if let Some(mut net) = world.remove_resource::<NetHost>() {
        net.host.shutdown();
    }
    world.remove_resource::<NetIdentity>();
    world.remove_resource::<HostBeacon>();
    let remote: Vec<Entity> = world
        .query_filtered::<Entity, (With<Player>, Without<LocalPlayer>)>()
        .iter(world)
        .collect();
    for e in remote {
        world.despawn(e);
    }
    world.insert_resource(NetMode::Single);
}

/// Hosting systems; inert until [`start_hosting`] inserts a [`NetHost`].
pub struct NetHostPlugin;

impl Plugin for NetHostPlugin {
    fn build(&self, app: &mut App) {
        let hosting = resource_exists::<NetHost>;
        app.add_systems(
            FixedUpdate,
            (
                (host_receive, apply_remote_input)
                    .chain()
                    .before(use_key)
                    .run_if(hosting),
                host_send
                    .after(CombatSet)
                    .after(forward_mech_events)
                    .after(check_player_death)
                    .after(advance_level)
                    .run_if(hosting),
            ),
        )
        .add_systems(Update, host_beacon.run_if(resource_exists::<HostBeacon>))
        .add_systems(
            SpawnLevel,
            spawn_seated_players.after(spawn_combat).run_if(hosting),
        )
        .add_systems(Last, stop_on_exit.run_if(hosting));
    }
}

/// Spawns the co-op player of every seat.
fn spawn_seated_players(
    mut commands: Commands,
    net: Res<NetHost>,
    map: Res<CurrentMap>,
    tuning: Res<PlayerTuning>,
    defs: Res<GameDefs>,
) {
    for k in 0..net.seats.len() {
        spawn_coop_player(&mut commands, &map.0, &tuning.0, &defs.0, seat_player(k));
    }
}

fn seat_player(k: usize) -> PlayerId {
    PlayerId(k as u8 + 1)
}

/// The level and skill being played, as a `Load` stamped with `tick`.
fn current_load(world: &World, tick: u32) -> Load {
    Load {
        level: world.resource::<LevelFile>().0.clone(),
        skill: world.resource::<LevelDifficulty>().0,
        tick,
    }
}

/// Receives what the clients sent: handshakes, input frames and departures.
fn host_receive(world: &mut World) {
    let dt = world.resource::<Time<Fixed>>().timestep();
    let events = world.resource_mut::<NetHost>().host.update(dt);
    for event in events {
        match event {
            HostEvent::Connected(key) => world.resource_mut::<NetHost>().waiting.push((key, 0.0)),
            HostEvent::Message(key, ClientMsg::Hello(hello)) => greet(world, key, &hello),
            HostEvent::Message(key, ClientMsg::Input(frame)) => {
                let mut net = world.resource_mut::<NetHost>();
                if let Some(k) = net.seat_of(key) {
                    take_frame(&mut net.seats[k], &frame);
                }
            }
            HostEvent::Disconnected(key) => leave(world, key),
            HostEvent::Garbage(_) => world.resource_mut::<NetHost>().garbage += 1,
        }
    }
    // Connections that never say hello are dropped.
    let mut net = world.resource_mut::<NetHost>();
    let secs = dt.as_secs_f32();
    let mut silent = Vec::new();
    net.waiting.retain_mut(|(key, age)| {
        *age += secs;
        let keep = *age < HELLO_TIMEOUT;
        if !keep {
            silent.push(*key);
        }
        keep
    });
    for key in silent {
        net.host.disconnect(key);
    }
}

/// A newer frame replaces the seat's; taps it has not applied yet carry over, so two frames in
/// one tick cannot swallow a press.
fn take_frame(seat: &mut Seat, frame: &InputFrame) {
    if frame.seq <= seat.last_seq {
        return;
    }
    seat.last_seq = frame.seq;
    let mut next = frame.clamped(PITCH_LIMIT);
    if let Some(old) = &seat.frame {
        next.buttons |= old.buttons & TAPS;
        next.select = next.select.or(old.select);
        if next.cycle == 0 {
            next.cycle = old.cycle;
        }
    }
    seat.frame = Some(next);
}

/// Answers a `Hello`: a refusal, or a seat, its welcome, the roster and (in play) its player.
fn greet(world: &mut World, key: ClientKey, hello: &Hello) {
    let identity = world.resource::<NetIdentity>().clone();
    let welcome = Welcome {
        player: PlayerId(0),
        level: world.resource::<LevelFile>().0.clone(),
        skill: world.resource::<LevelDifficulty>().0,
    };
    let playing = *world.resource::<PlayState>() != PlayState::Menu;
    let mut net = world.resource_mut::<NetHost>();
    if net.seat_of(key).is_some() {
        return;
    }
    net.waiting.retain(|(k, _)| *k != key);
    if let Err(refusal) = check_hello(
        hello,
        &identity.game_version,
        identity.assets,
        1 + net.seats.len(),
    ) {
        net.host.send(key, &ServerMsg::Refused(refusal));
        net.host.flush();
        net.host.disconnect(key);
        return;
    }
    net.seats.push(Seat {
        client: key,
        name: hello.name.clone(),
        last_seq: 0,
        frame: None,
    });
    let player = seat_player(net.seats.len() - 1);
    net.host
        .send(key, &ServerMsg::Welcome(Welcome { player, ..welcome }));
    let roster = net.roster(&identity.name);
    net.broadcast(&roster);
    let tick = net.tick;
    if playing {
        let load = current_load(world, tick);
        world
            .resource_mut::<NetHost>()
            .host
            .send(key, &ServerMsg::Load(load));
        spawn_player(world, player);
    }
}

fn spawn_player(world: &mut World, id: PlayerId) {
    let map = world.resource::<CurrentMap>().0.clone();
    let tuning = world.resource::<PlayerTuning>().0;
    let defs = world.resource::<GameDefs>().0.clone();
    let mut commands = world.commands();
    spawn_coop_player(&mut commands, &map, &tuning, &defs, id);
    world.flush();
}

/// A client left: its seat and player go, everyone above moves down one.
fn leave(world: &mut World, key: ClientKey) {
    let identity = world.resource::<NetIdentity>().name.clone();
    let mut net = world.resource_mut::<NetHost>();
    net.waiting.retain(|(k, _)| *k != key);
    let Some(k) = net.seat_of(key) else { return };
    net.seats.remove(k);
    let roster = net.roster(&identity);
    net.broadcast(&roster);
    let gone = seat_player(k);
    if let Some(mut combat) = world.get_resource_mut::<LevelCombat>() {
        combat.0.remove_player(gone);
    }
    if let Some(mut mech) = world.get_resource_mut::<LevelMechanics>() {
        mech.0.remove_player(gone);
    }
    let mut q = world.query::<(Entity, &mut Player)>();
    let mut despawn = None;
    for (e, mut p) in q.iter_mut(world) {
        if p.id == gone {
            despawn = Some(e);
        } else if p.id > gone {
            p.id = PlayerId(p.id.0 - 1);
        }
    }
    if let Some(e) = despawn {
        world.despawn(e);
    }
}

/// Drives every seated player from its latest frame. Held controls repeat each tick; taps and
/// weapon choices are applied once.
fn apply_remote_input(
    mut net: ResMut<NetHost>,
    mut players: Query<(&Player, &mut PendingInput, &mut Look)>,
) {
    for (k, seat) in net.seats.iter_mut().enumerate() {
        let Some(frame) = seat.frame.as_mut() else {
            continue;
        };
        let id = seat_player(k);
        if let Some((_, mut input, mut look)) = players.iter_mut().find(|(p, ..)| p.id == id) {
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
        // Applied, or dropped with no player to take it: a tap never acts twice.
        frame.buttons &= !TAPS;
        frame.select = None;
        frame.cycle = 0;
    }
}

/// What a snapshot holds of each player.
type SnapQuery = (
    &'static Player,
    &'static PlayerBody,
    &'static Look,
    &'static PlayerVitals,
    &'static PlayerArsenal,
    &'static PlayerInventory,
    &'static Inventory,
);

/// Ends the host's tick: loads after a restart, a snapshot every [`SNAPSHOT_EVERY`] ticks, and
/// sends it all.
#[allow(clippy::too_many_arguments)]
fn host_send(
    mut net: ResMut<NetHost>,
    runs: Res<LevelRuns>,
    state: Res<PlayState>,
    file: Res<LevelFile>,
    skill: Res<LevelDifficulty>,
    combat: Option<Res<LevelCombat>>,
    mech: Option<Res<LevelMechanics>>,
    map: Res<CurrentMap>,
    source: Res<LevelSource>,
    players: Query<SnapQuery>,
) {
    let net = &mut *net;
    // Snapshots up to this tick belong to the old run.
    if runs.0 != net.runs {
        net.runs = runs.0;
        let load = ServerMsg::Load(Load {
            level: file.0.clone(),
            skill: skill.0,
            tick: net.tick,
        });
        net.broadcast(&load);
    }
    net.tick += 1;
    if *state != PlayState::Menu
        && net.tick.is_multiple_of(SNAPSHOT_EVERY)
        && !net.seats.is_empty()
        && let (Some(combat), Some(mech)) = (combat, mech)
    {
        let mut snaps: Vec<PlayerSnap> = players
            .iter()
            .map(
                |(p, body, look, vitals, arsenal, carried, inv)| PlayerSnap {
                    id: p.id,
                    body: body.0,
                    angle: look.angle,
                    pitch: look.pitch,
                    vitals: vitals.0,
                    arsenal: arsenal.0.clone(),
                    inventory: carried.0,
                    keys: inv.keys,
                    ack: match p.id.0 {
                        0 => 0,
                        j => net.seats.get(j as usize - 1).map_or(0, |s| s.last_seq),
                    },
                },
            )
            .collect();
        snaps.sort_by_key(|s| s.id);
        let snap = Snapshot::capture(net.tick, &combat.0, &mech.0, &map.0, &source.0, snaps);
        for (k, seat) in net.seats.iter().enumerate() {
            let msg = ServerMsg::Snapshot {
                you: seat_player(k),
                snap: snap.clone(),
            };
            net.host.send(seat.client, &msg);
        }
    }
    net.host.flush();
}

/// Announces the game on the LAN every [`BEACON_EVERY`] seconds.
fn host_beacon(
    time: Res<Time>,
    mut beacon: ResMut<HostBeacon>,
    net: Res<NetHost>,
    identity: Res<NetIdentity>,
    file: Res<LevelFile>,
) {
    beacon.since += time.delta_secs();
    if beacon.since < BEACON_EVERY {
        return;
    }
    beacon.since = 0.0;
    let msg = Beacon {
        magic: BEACON_MAGIC,
        protocol: PROTOCOL_VERSION,
        name: identity.name.clone(),
        level: file.0.clone(),
        players: (1 + net.seats.len()) as u8,
        max: MAX_PLAYERS as u8,
        port: net.host.local_addr().port(),
    };
    beacon.sender.broadcast(&msg);
}

/// Closing the app closes the game, so the clients hear it now rather than time out.
fn stop_on_exit(mut exits: MessageReader<AppExit>, mut commands: Commands) {
    if exits.read().next().is_some() {
        commands.queue(stop_hosting);
    }
}
