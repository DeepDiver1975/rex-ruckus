//! Hosting a co-op game: the host runs the only simulation, seats the clients that say a good
//! `Hello`, drives their players from their input frames (queued, one per tick) and sends
//! everyone snapshots, each tick's events and their own HUD lines.
//!
//! Seat `k` plays `PlayerId(k + 1)`; the host's own player is `PlayerId(0)`. When a seat leaves,
//! the seats (and players) above it move down one, so ids stay dense.

use std::collections::VecDeque;
use std::io;
use std::net::SocketAddr;

use bevy::prelude::*;
use rr_core::events::{
    combat_event_finite, combat_event_players_ok, mech_event_finite, weapon_event_fits,
};
use rr_core::players::{MAX_PLAYERS, PlayerId};
use rr_core::snapshot::{PlayerSnap, Snapshot};
use rr_net::{
    BEACON_MAGIC, Beacon, BeaconSender, ClientKey, ClientMsg, Hello, Host, HostEvent, InputFrame,
    Load, MAX_EVENTS, MAX_HUD, PROTOCOL_VERSION, PlayView, ServerMsg, TickEvents, Welcome,
    assets_hash, buttons, check_hello, valid_level_name,
};

use super::{NetMode, apply_frame};
use crate::combat::{
    CombatSet, FxQueue, GameDefs, LevelCombat, PlayerArsenal, PlayerInventory, PlayerVitals,
    clear_fx, spawn_combat,
};
use crate::episode::{Episode, Stats};
use crate::flow::{
    LevelDifficulty, LevelFile, LevelRuns, LevelSource, PausedFrom, PlayState, SpawnLevel,
    advance_level, check_player_death,
};
use crate::level::CurrentMap;
use crate::mechanics::{LevelMechanics, PlayerLines, forward_mech_events, pickup_items, use_key};
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
/// Most input frames a seat holds waiting for their tick; above it the oldest are dropped (their
/// taps carried into the next one), so a burst after a stall cannot keep the player seconds
/// behind.
pub const MAX_INPUT_QUEUE: usize = 8;
/// Frames a seat may keep waiting for good: deeper for [`DRAIN_AFTER`] ticks in a row and the
/// seat takes two frames a tick until it is back.
pub const QUEUE_SLACK: usize = 2;
/// Ticks in a row a seat's queue may stay deeper than [`QUEUE_SLACK`] before it drains.
pub const DRAIN_AFTER: u32 = 3;

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
    /// The newest input sequence taken from this client (queued or applied).
    pub last_seq: u32,
    /// The sequence of the frame last applied; snapshots carry it as the player's `ack`.
    pub ack: u32,
    /// Frames waiting for their tick, oldest first: one is applied per tick.
    pub queue: VecDeque<InputFrame>,
    /// The frame last applied: held controls stay while the queue is empty, its taps are
    /// cleared once applied.
    pub frame: Option<InputFrame>,
    /// Ticks in a row the queue was deeper than [`QUEUE_SLACK`].
    pub deep: u32,
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
    /// How many `combat`, `weapon` and `mech` events of [`FxQueue`] were already relayed;
    /// back to zero when the queue is cleared.
    relayed: [usize; 3],
    /// The [`PlayView`] the seats last got.
    pub view: PlayView,
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
    let view = play_view(world);
    world.insert_resource(NetHost {
        host,
        seats: Vec::new(),
        tick: 0,
        garbage: 0,
        waiting: Vec::new(),
        runs,
        relayed: [0; 3],
        view,
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
    // Back to one player: drop the seats' per-player state, highest id first.
    let n = world
        .query_filtered::<(), With<Player>>()
        .iter(world)
        .count();
    for id in (1..n.min(MAX_PLAYERS)).rev().map(|i| PlayerId(i as u8)) {
        remove_player_state(world, id);
    }
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
                    .after(pickup_items)
                    .after(forward_mech_events)
                    .after(check_player_death)
                    .after(advance_level)
                    .run_if(hosting),
            ),
        )
        .add_systems(
            Update,
            (
                host_beacon.run_if(resource_exists::<HostBeacon>),
                reset_relayed.before(clear_fx).run_if(hosting),
            ),
        )
        .add_systems(
            SpawnLevel,
            spawn_seated_players.after(spawn_combat).run_if(hosting),
        )
        .add_systems(Last, stop_on_exit.run_if(hosting));
    }
}

/// Spawns the co-op player of every seat.
pub fn spawn_seated_players(
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

/// The screen the host is on, for its clients to follow: the Lobby in the menu, under the pause
/// menu the state it was opened from.
pub fn play_view(world: &World) -> PlayView {
    view_of(
        *world.resource::<PlayState>(),
        world
            .get_resource::<PausedFrom>()
            .copied()
            .unwrap_or_default(),
        world.get_resource::<Stats>(),
        world.get_resource::<Episode>(),
    )
}

/// [`play_view`] from the resources it reads.
fn view_of(
    state: PlayState,
    from: PausedFrom,
    stats: Option<&Stats>,
    episode: Option<&Episode>,
) -> PlayView {
    match state {
        PlayState::Menu => PlayView::Lobby,
        PlayState::Playing => PlayView::Playing,
        // The pause menu never opens from itself.
        PlayState::Paused if from.0 == PlayState::Paused => PlayView::Playing,
        PlayState::Paused => view_of(from.0, from, stats, episode),
        PlayState::Dead => PlayView::Dead,
        PlayState::Complete => PlayView::Complete {
            stats: stats.map(|s| s.0).unwrap_or_default(),
            episode: episode.is_some(),
        },
        PlayState::EpisodeEnd => {
            let (name, totals) = episode
                .map(|e| (hud_line(&e.def.name), e.totals))
                .unwrap_or_default();
            PlayView::EpisodeEnd { name, totals }
        }
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
            HostEvent::Message(key, ClientMsg::Input(frames)) => {
                let mut net = world.resource_mut::<NetHost>();
                if let Some(k) = net.seat_of(key) {
                    // Oldest first; `take_frame` skips those it already has.
                    for frame in &frames {
                        take_frame(&mut net.seats[k], frame);
                    }
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

/// Queues a frame newer than any the seat has; the redundant copies a datagram repeats are
/// skipped, so each frame (and each tap in it) is applied exactly once. Past
/// [`MAX_INPUT_QUEUE`] the oldest waiting frames go, their taps carried into the next.
fn take_frame(seat: &mut Seat, frame: &InputFrame) {
    if frame.seq <= seat.last_seq {
        return;
    }
    seat.last_seq = frame.seq;
    seat.queue.push_back(frame.clamped(PITCH_LIMIT));
    while seat.queue.len() > MAX_INPUT_QUEUE {
        let old = seat.queue.pop_front().expect("over the cap");
        carry_taps(&old, seat.queue.front_mut().expect("the cap is above zero"));
    }
}

/// Folds the one-shot parts of `older` (a frame skipped unapplied) into `newer`: its tap bits,
/// its weapon choice unless `newer` makes its own, and its weapon cycling.
fn carry_taps(older: &InputFrame, newer: &mut InputFrame) {
    newer.buttons |= older.buttons & TAPS;
    if newer.select.is_none() {
        newer.select = older.select;
    }
    newer.cycle = newer.cycle.saturating_add(older.cycle);
}

/// The frame to apply this tick: the next queued one, or (when the queue stayed deeper than
/// [`QUEUE_SLACK`] for [`DRAIN_AFTER`] ticks) the one after it with the skipped frame's taps
/// carried in, so a burst drains instead of keeping the player behind. `None` when none waits.
fn next_frame(seat: &mut Seat) -> Option<InputFrame> {
    if seat.queue.len() > QUEUE_SLACK {
        seat.deep += 1;
    } else {
        seat.deep = 0;
    }
    let mut next = seat.queue.pop_front()?;
    if seat.deep >= DRAIN_AFTER
        && let Some(mut newer) = seat.queue.pop_front()
    {
        carry_taps(&next, &mut newer);
        next = newer;
    }
    Some(next)
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
        ack: 0,
        queue: VecDeque::new(),
        frame: None,
        deep: 0,
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
    // After the load, which would put the joiner in play: a death or stats screen stays up.
    let view = play_view(world);
    world
        .resource_mut::<NetHost>()
        .host
        .send(key, &ServerMsg::Play(view));
}

fn spawn_player(world: &mut World, id: PlayerId) {
    let map = world.resource::<CurrentMap>().0.clone();
    let tuning = world.resource::<PlayerTuning>().0;
    let defs = world.resource::<GameDefs>().0.clone();
    let mut commands = world.commands();
    spawn_coop_player(&mut commands, &map, &tuning, &defs, id);
    world.flush();
}

/// Drops player `gone`'s share of the level, episode and stats state (later ids move down).
fn remove_player_state(world: &mut World, gone: PlayerId) {
    if let Some(mut combat) = world.get_resource_mut::<LevelCombat>() {
        combat.0.remove_player(gone);
    }
    if let Some(mut mech) = world.get_resource_mut::<LevelMechanics>() {
        mech.0.remove_player(gone);
    }
    if let Some(mut ep) = world.get_resource_mut::<Episode>() {
        ep.remove_player(gone);
    }
    if let Some(mut stats) = world.get_resource_mut::<Stats>() {
        stats.0.remove_player(gone);
    }
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
    remove_player_state(world, gone);
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

/// Drives every seated player from the next queued frame ([`next_frame`]), one per tick. With
/// none queued the last frame's held controls repeat; taps and weapon choices act once.
pub fn apply_remote_input(
    mut net: ResMut<NetHost>,
    mut players: Query<(&Player, &mut PendingInput, &mut Look)>,
) {
    for (k, seat) in net.seats.iter_mut().enumerate() {
        if let Some(next) = next_frame(seat) {
            seat.ack = next.seq;
            seat.frame = Some(next);
        }
        let Some(frame) = seat.frame.as_mut() else {
            continue;
        };
        let id = seat_player(k);
        if let Some((_, mut input, mut look)) = players.iter_mut().find(|(p, ..)| p.id == id) {
            apply_frame(frame, &mut input, &mut look);
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

/// The [`FxQueue`] was cleared: nothing in it is relayed yet.
fn reset_relayed(mut net: ResMut<NetHost>) {
    net.relayed = [0; 3];
}

/// Slice `i` of [`MAX_EVENTS`] events of `events`.
fn chunk<T: Clone>(events: &[T], i: usize) -> Vec<T> {
    events
        .iter()
        .skip(i * MAX_EVENTS)
        .take(MAX_EVENTS)
        .cloned()
        .collect()
}

/// The events `fx` gained since `relayed`, stamped `tick`, in as many messages as it takes to
/// keep each stream within [`MAX_EVENTS`]; none when there are none. Moves `relayed` on.
/// Events a client's `decode_server` would refuse (a non-finite float, a player id past
/// `MAX_PLAYERS`) stay home: one would end every client's session.
fn new_events(fx: &FxQueue, relayed: &mut [usize; 3], tick: u32) -> Vec<TickEvents> {
    fn since<T: Clone>(events: &[T], from: &mut usize, ok: impl Fn(&T) -> bool) -> Vec<T> {
        let new = &events[(*from).min(events.len())..];
        *from = events.len();
        new.iter().filter(|e| ok(e)).cloned().collect()
    }
    let combat = since(&fx.combat, &mut relayed[0], |e| {
        combat_event_finite(e) && combat_event_players_ok(e)
    });
    let weapon = since(&fx.weapon, &mut relayed[1], |(p, e)| {
        usize::from(p.0) < MAX_PLAYERS && weapon_event_fits(e)
    });
    let mech = since(&fx.mech, &mut relayed[2], mech_event_finite);
    let parts = combat
        .len()
        .max(weapon.len())
        .max(mech.len())
        .div_ceil(MAX_EVENTS);
    (0..parts)
        .map(|i| TickEvents {
            tick,
            combat: chunk(&combat, i),
            weapon: chunk(&weapon, i),
            mech: chunk(&mech, i),
        })
        .collect()
}

/// `text` as a `Hud` line: control characters dropped, cut to [`MAX_HUD`] bytes on a character
/// boundary.
pub fn hud_line(text: &str) -> String {
    let mut out = String::new();
    for c in text.chars().filter(|c| !c.is_control()) {
        if out.len() + c.len_utf8() > MAX_HUD {
            break;
        }
        out.push(c);
    }
    out
}

/// What [`play_view`] reads, for a system.
#[derive(bevy::ecs::system::SystemParam)]
pub struct ViewParams<'w> {
    state: Res<'w, PlayState>,
    from: Res<'w, PausedFrom>,
    stats: Option<Res<'w, Stats>>,
    episode: Option<Res<'w, Episode>>,
}

impl ViewParams<'_> {
    fn view(&self) -> PlayView {
        view_of(
            *self.state,
            *self.from,
            self.stats.as_deref(),
            self.episode.as_deref(),
        )
    }
}

/// Ends the host's tick: loads after a restart, the host's screen when it changed (after the
/// load, which puts the clients in play), this tick's events, the seats' HUD lines, a snapshot
/// every [`SNAPSHOT_EVERY`] ticks, and sends it all.
#[allow(clippy::too_many_arguments)]
pub fn host_send(
    mut net: ResMut<NetHost>,
    view: ViewParams,
    fx: Res<FxQueue>,
    mut lines: ResMut<PlayerLines>,
    runs: Res<LevelRuns>,
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
    let now = view.view();
    if now != net.view {
        net.broadcast(&ServerMsg::Play(now.clone()));
        net.view = now;
    }
    net.tick += 1;
    // Stamped like the snapshot of this tick would be.
    for ev in new_events(&fx, &mut net.relayed, net.tick) {
        net.broadcast(&ServerMsg::Events(ev));
    }
    // A seat's lines go to it alone; the local player's and unseated ones stay.
    lines.0.retain(|(p, text)| {
        let Some(seat) = (p.0 as usize).checked_sub(1).and_then(|k| net.seats.get(k)) else {
            return true;
        };
        net.host.send(seat.client, &ServerMsg::Hud(hud_line(text)));
        false
    });
    if *view.state != PlayState::Menu
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
                        j => net.seats.get(j as usize - 1).map_or(0, |s| s.ack),
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

#[cfg(test)]
mod tests {
    use super::*;
    use rr_core::mechanics::MechEvent;

    #[test]
    fn hud_lines_are_cut_on_a_char_boundary() {
        assert_eq!(hud_line("Pistol ammo"), "Pistol ammo");
        assert_eq!(hud_line("a\nb\u{7}c"), "abc");
        let long = "\u{e9}".repeat(MAX_HUD);
        let cut = hud_line(&long);
        assert_eq!(cut.len(), MAX_HUD);
        assert!(cut.chars().all(|c| c == '\u{e9}'));
        let odd = format!("x{long}");
        assert_eq!(hud_line(&odd).len(), MAX_HUD - 1);
    }

    #[test]
    fn events_split_at_the_cap_and_are_sent_once() {
        let mut fx = FxQueue {
            mech: vec![MechEvent::Exit; MAX_EVENTS + 1],
            ..default()
        };
        let mut relayed = [0; 3];
        let sent = new_events(&fx, &mut relayed, 7);
        assert_eq!(sent.len(), 2);
        assert_eq!(sent[0].mech.len(), MAX_EVENTS);
        assert_eq!(sent[1].mech.len(), 1);
        assert!(sent.iter().all(|e| e.tick == 7 && e.combat.is_empty()));
        assert!(new_events(&fx, &mut relayed, 8).is_empty(), "nothing new");
        fx.mech.push(MechEvent::Exit);
        assert_eq!(new_events(&fx, &mut relayed, 9)[0].mech.len(), 1);
        // A cleared queue with a stale cursor sends nothing and does not panic.
        fx.mech.clear();
        assert!(new_events(&fx, &mut [5; 3], 10).is_empty());
    }

    #[test]
    fn events_a_client_would_refuse_stay_home() {
        use rr_core::combat::CombatEvent;
        use rr_core::weapons::WeaponEvent;
        let boom = |x: f32| CombatEvent::Explosion {
            point: Vec3::new(x, 0.0, 0.0),
            radius: 1.0,
        };
        let fx = FxQueue {
            combat: vec![
                boom(f32::NAN),
                boom(1.0),
                CombatEvent::PlayerKilled(PlayerId(MAX_PLAYERS as u8)),
            ],
            weapon: vec![
                (PlayerId(1), WeaponEvent::Kick { dir: Vec3::NAN }),
                (PlayerId(1), WeaponEvent::Kick { dir: Vec3::X }),
                (
                    PlayerId(MAX_PLAYERS as u8),
                    WeaponEvent::Kick { dir: Vec3::X },
                ),
            ],
            mech: vec![
                MechEvent::QuakeStarted {
                    strength: f32::INFINITY,
                    duration: 1.0,
                },
                MechEvent::Exit,
            ],
            ..default()
        };
        let sent = new_events(&fx, &mut [0; 3], 3);
        assert_eq!(sent.len(), 1);
        let ev = &sent[0];
        assert_eq!(ev.combat, [boom(1.0)]);
        assert_eq!(
            ev.weapon,
            [(PlayerId(1), WeaponEvent::Kick { dir: Vec3::X })]
        );
        assert_eq!(ev.mech, [MechEvent::Exit]);
        let msg = ServerMsg::Events(ev.clone());
        assert_eq!(rr_net::decode_server(&rr_net::encode(&msg)).unwrap(), msg);
    }
}
