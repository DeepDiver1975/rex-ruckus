//! Joining a co-op game: the client never ticks its own simulation. It loads the host's level,
//! applies the host's snapshots to its copy of it and sends the local player's input.

use std::io;
use std::net::SocketAddr;

use bevy::prelude::*;
use rr_core::combat::CombatEvent;
use rr_core::players::PlayerId;
use rr_core::snapshot::{PlayerSnap, Snapshot};
use rr_net::{
    Client, ClientMsg, ClientState, Hello, InputFrame, LostReason, PROTOCOL_VERSION, Refusal,
    ServerMsg, Welcome, buttons,
};

use super::NetMode;
use super::host::NetIdentity;
use crate::combat::{FxQueue, GameDefs, LevelCombat, PlayerArsenal, PlayerInventory, PlayerVitals};
use crate::episode::Episode;
use crate::flow::{
    LevelDifficulty, LevelFile, LevelRuns, LocalSeat, PlayState, freeze_interpolation, load_level,
    world_running,
};
use crate::level::{CurrentMap, try_load_map};
use crate::mechanics::{DirtySectors, LevelMechanics, use_key};
use crate::player::{
    Inventory, LocalPlayer, Look, PendingInput, Player, PlayerBody, PlayerSimSet, PlayerTuning,
    PrevFeet, spawn_coop_player,
};

/// The notice for a host's level this machine does not have.
pub const MISSING_LEVEL: &str = "The host's level is not on this machine";

/// The notice for a control message from the host that does not decode (another build's, or
/// corrupt): it is never sent again, so the session cannot go on.
pub const NOT_UNDERSTOOD: &str = "The host sent something this game does not understand";

/// The joined game.
#[derive(Resource)]
pub struct NetClient {
    pub client: Client,
    /// The sequence number of the last input frame sent.
    pub seq: u32,
    /// The tick of the last load or applied snapshot; older snapshots are dropped. `None` until
    /// the host's level is loaded.
    pub last_tick: Option<u32>,
    /// Every player's name, index = `PlayerId`.
    pub roster: Vec<String>,
    pub welcomed: Option<Welcome>,
    /// The newest snapshot received this tick and the seat it names, for [`client_apply`].
    pub pending: Option<(PlayerId, Snapshot)>,
    hello_sent: bool,
    /// The [`LevelRuns`] a misfitting snapshot was last reported in (one warning per run).
    warned_run: Option<u32>,
}

/// Why the last co-op session ended, for the menu to show; `None` when it ended by choice.
#[derive(Resource, Default, Debug, Clone, PartialEq, Eq)]
pub struct NetNotice(pub Option<String>);

/// A host's refusal in words.
pub fn refusal_text(r: Refusal) -> &'static str {
    match r {
        Refusal::Protocol { .. } | Refusal::GameVersion => {
            "The host runs another version of the game"
        }
        Refusal::Assets => "Your game files differ from the host's",
        Refusal::Full => "The game is full",
        Refusal::BadName => "Pick a name of 1-24 characters",
    }
}

/// Connects to the host at `addr`; the `Hello` goes out once the connection stands.
pub fn start_joining(world: &mut World, addr: SocketAddr, name: String) -> io::Result<()> {
    let identity = NetIdentity::local(name)?;
    let client = Client::connect(addr)?;
    world.insert_resource(NetClient {
        client,
        seq: 0,
        last_tick: None,
        roster: Vec::new(),
        welcomed: None,
        pending: None,
        hello_sent: false,
        warned_run: None,
    });
    world.insert_resource(identity);
    world.insert_resource(NetMode::Client);
    Ok(())
}

/// Leaves the joined game: drops the other players and goes back to single play in seat 0.
pub fn stop_joining(world: &mut World) {
    if let Some(mut net) = world.remove_resource::<NetClient>() {
        net.client.disconnect();
        net.client.flush();
    }
    world.remove_resource::<NetIdentity>();
    let players: Vec<(Entity, bool)> = world
        .query::<(Entity, Has<LocalPlayer>)>()
        .iter(world)
        .collect();
    for (e, local) in players {
        if local {
            if let Some(mut p) = world.get_mut::<Player>(e) {
                p.id = PlayerId(0);
            }
        } else if world.get::<Player>(e).is_some() {
            world.despawn(e);
        }
    }
    world.insert_resource(LocalSeat(PlayerId(0)));
    world.insert_resource(NetMode::Single);
}

/// Ends the session with `notice` and leaves play for the menu.
fn end_session(world: &mut World, notice: Option<String>) {
    if notice.is_some() {
        world.insert_resource(NetNotice(notice));
    }
    stop_joining(world);
    if world.contains_resource::<Episode>() {
        crate::menu::quit_to_menu(world);
    } else {
        *world.resource_mut::<PlayState>() = PlayState::Menu;
    }
}

/// Joining systems; inert until [`start_joining`] inserts a [`NetClient`].
pub struct NetClientPlugin;

impl Plugin for NetClientPlugin {
    fn build(&self, app: &mut App) {
        let joined = resource_exists::<NetClient>;
        app.init_resource::<NetNotice>()
            .add_systems(
                FixedUpdate,
                (
                    client_receive.run_if(joined),
                    client_apply.run_if(joined),
                    client_send_input.run_if(joined),
                )
                    .chain()
                    .before(use_key)
                    .before(PlayerSimSet),
            )
            .add_systems(Last, stop_on_exit.run_if(joined));
    }
}

/// Receives what the host sent and sends the `Hello` once connected.
pub fn client_receive(world: &mut World) {
    let dt = world.resource::<Time<Fixed>>().timestep();
    let msgs = world.resource_mut::<NetClient>().client.update(dt);
    for msg in msgs {
        client_handle(world, msg);
        if !world.contains_resource::<NetClient>() {
            return;
        }
    }
    if world.resource::<NetClient>().client.control_garbage() > 0 {
        end_session(world, Some(NOT_UNDERSTOOD.into()));
        return;
    }
    match world.resource::<NetClient>().client.state() {
        ClientState::Connecting => {}
        ClientState::Connected => {
            if !world.resource::<NetClient>().hello_sent {
                let id = world.resource::<NetIdentity>().clone();
                let hello = Hello {
                    protocol: PROTOCOL_VERSION,
                    game_version: id.game_version,
                    assets: id.assets,
                    name: id.name,
                };
                let mut net = world.resource_mut::<NetClient>();
                net.client.send(&ClientMsg::Hello(hello));
                net.client.flush();
                net.hello_sent = true;
            }
        }
        // Our own leaving is no news.
        ClientState::Lost(LostReason::Left) => end_session(world, None),
        ClientState::Lost(reason) => end_session(world, Some(reason.to_string())),
    }
}

/// Handles one message from the host.
pub fn client_handle(world: &mut World, msg: ServerMsg) {
    match msg {
        ServerMsg::Welcome(w) => {
            world.insert_resource(LocalSeat(w.player));
            world.resource_mut::<NetClient>().welcomed = Some(w);
        }
        ServerMsg::Refused(r) => end_session(world, Some(refusal_text(r).into())),
        ServerMsg::Roster(names) => world.resource_mut::<NetClient>().roster = names,
        ServerMsg::Load(load) => match try_load_map(&load.level) {
            Err(e) => {
                warn!("cannot load the host's level: {e}");
                end_session(world, Some(MISSING_LEVEL.into()));
            }
            Ok(map) => {
                world.insert_resource(LevelDifficulty(load.skill));
                world.insert_resource(LevelFile(load.level));
                load_level(world, map);
                let mut net = world.resource_mut::<NetClient>();
                net.last_tick = Some(load.tick);
                net.pending = None;
            }
        },
        ServerMsg::Snapshot { you, snap } => {
            let mut net = world.resource_mut::<NetClient>();
            // Nothing fits before the host's level is loaded; UDP may reorder the rest.
            let newer = |t: u32| net.last_tick.is_some_and(|last| t > last);
            let newest = net.pending.as_ref().is_none_or(|(_, p)| snap.tick > p.tick);
            if newer(snap.tick) && newest {
                net.pending = Some((you, snap));
            }
        }
    }
}

/// Applies the newest snapshot received to the level and the players. On a tick without one,
/// every render-interpolation endpoint rests on the current pose, so the frames between
/// snapshots hold still instead of replaying the last snapshot's step.
pub fn client_apply(world: &mut World) {
    if !apply_pending(world)
        && let Err(e) = world.run_system_cached(freeze_interpolation)
    {
        warn!("cannot settle interpolation: {e}");
    }
}

/// Applies the pending snapshot; false if there was none to apply.
fn apply_pending(world: &mut World) -> bool {
    let Some((you, snap)) = world.resource_mut::<NetClient>().pending.take() else {
        return false;
    };
    let last = world.resource::<NetClient>().last_tick;
    if last.is_none_or(|t| snap.tick <= t) {
        return false;
    }
    if !world.contains_resource::<LevelCombat>() || !world.contains_resource::<LevelMechanics>() {
        return false;
    }
    let result = world.resource_scope(|world, mut combat: Mut<LevelCombat>| {
        world.resource_scope(|world, mut mech: Mut<LevelMechanics>| {
            world.resource_scope(|world, mut map: Mut<CurrentMap>| {
                let defs = &world.resource::<GameDefs>().0;
                snap.apply(&mut combat.0, &mut mech.0, &mut map.0, defs)
            })
        })
    });
    let changes = match result {
        Ok(changes) => changes,
        Err(e) => {
            let run = world.resource::<LevelRuns>().0;
            let mut net = world.resource_mut::<NetClient>();
            if net.warned_run != Some(run) {
                net.warned_run = Some(run);
                warn!("dropped a snapshot that does not fit the level: {e:?}");
            }
            return false;
        }
    };
    world.resource_mut::<NetClient>().last_tick = Some(snap.tick);
    let glass_sectors: Vec<usize> = {
        let map = &world.resource::<CurrentMap>().0;
        changes
            .glass
            .iter()
            .flat_map(|&w| std::iter::once(map.walls[w].sector).chain(map.walls[w].next_sector))
            .collect()
    };
    world
        .resource_mut::<DirtySectors>()
        .0
        .extend(changes.sectors.iter().copied().chain(glass_sectors));
    // `changes.glass` lists both sides of a pane; one event per pane.
    let panes: Vec<usize> = {
        let map = &world.resource::<CurrentMap>().0;
        changes
            .glass
            .iter()
            .copied()
            .filter(|&w| {
                map.walls[w]
                    .next_wall
                    .is_none_or(|t| t > w || !changes.glass.contains(&t))
            })
            .collect()
    };
    {
        let mut fx = world.resource_mut::<FxQueue>();
        fx.combat
            .extend(panes.into_iter().map(|wall| CombatEvent::GlassBroken {
                wall,
                dirty: vec![],
            }));
        fx.combat
            .extend(changes.lights.iter().map(|&i| CombatEvent::LightBroken(i)));
        fx.combat
            .extend(changes.actors_added.clone().map(CombatEvent::ActorSpawned));
    }
    apply_players(world, you, &snap.players);
    true
}

/// Seats the local player as `you` and brings every player in line with its snap; players the
/// snapshot does not hold go.
fn apply_players(world: &mut World, you: PlayerId, snaps: &[PlayerSnap]) {
    let mut local = None;
    let mut others = Vec::new();
    for (e, p, is_local) in world
        .query::<(Entity, &Player, Has<LocalPlayer>)>()
        .iter(world)
    {
        if is_local {
            local = Some((e, p.id));
        } else {
            others.push((e, p.id));
        }
    }
    if let Some((e, id)) = local
        && id != you
    {
        world.get_mut::<Player>(e).unwrap().id = you;
        world.insert_resource(LocalSeat(you));
    }
    for (e, id) in others {
        if id == you || !snaps.iter().any(|s| s.id == id) {
            world.despawn(e);
        }
    }
    for s in snaps {
        let found = if s.id == you {
            local.map(|(e, _)| e)
        } else {
            world
                .query_filtered::<(Entity, &Player), Without<LocalPlayer>>()
                .iter(world)
                .find(|(_, p)| p.id == s.id)
                .map(|(e, _)| e)
        };
        let (e, prev) = match found {
            Some(e) => (e, world.get::<PlayerBody>(e).map(|b| b.0.pos)),
            None => (spawn_player(world, s.id), None),
        };
        let mut entity = world.entity_mut(e);
        entity.insert((
            PrevFeet(prev.unwrap_or(s.body.pos)),
            PlayerBody(s.body),
            PlayerVitals(s.vitals),
            PlayerArsenal(s.arsenal.clone()),
            PlayerInventory(s.inventory),
        ));
        if let Some(mut inv) = entity.get_mut::<Inventory>() {
            inv.keys = s.keys;
        }
        if s.id != you
            && let Some(mut look) = entity.get_mut::<Look>()
        {
            look.angle = s.angle;
            look.pitch = s.pitch;
        }
    }
}

fn spawn_player(world: &mut World, id: PlayerId) -> Entity {
    let map = world.resource::<CurrentMap>().0.clone();
    let tuning = world.resource::<PlayerTuning>().0;
    let defs = world.resource::<GameDefs>().0.clone();
    let mut commands = world.commands();
    let e = spawn_coop_player(&mut commands, &map, &tuning, &defs, id);
    world.flush();
    e
}

/// Sends the local player's input for this tick. Taps are taken the way the sim systems take
/// them; the automap toggle stays local.
fn client_send_input(
    mut net: ResMut<NetClient>,
    state: Res<PlayState>,
    from: Res<crate::flow::PausedFrom>,
    mode: Option<Res<NetMode>>,
    mut q: Query<(&mut PendingInput, &Look), With<LocalPlayer>>,
) {
    if net.welcomed.is_none() || !world_running(state, from, mode) {
        return;
    }
    let Ok((mut input, look)) = q.single_mut() else {
        return;
    };
    let mut bits = 0;
    let mut set = |bit: u16, on: bool| {
        if on {
            bits |= bit;
        }
    };
    set(buttons::JUMP, input.jump);
    set(buttons::CROUCH, input.crouch);
    set(buttons::FIRE, input.fire);
    set(buttons::USE, std::mem::take(&mut input.use_pressed));
    set(
        buttons::FIRE_PRESSED,
        std::mem::take(&mut input.fire_pressed),
    );
    set(buttons::RELOAD, std::mem::take(&mut input.reload));
    set(buttons::KICK, std::mem::take(&mut input.kick));
    set(buttons::MEDKIT, std::mem::take(&mut input.use_medkit));
    set(buttons::JETPACK, std::mem::take(&mut input.toggle_jetpack));
    set(buttons::NIGHT_VISION, std::mem::take(&mut input.toggle_nv));
    let cycle = std::mem::take(&mut input.cycle).clamp(i8::MIN.into(), i8::MAX.into()) as i8;
    net.seq += 1;
    let frame = InputFrame {
        seq: net.seq,
        forward: input.forward,
        strafe: input.strafe,
        angle: look.angle,
        pitch: look.pitch,
        buttons: bits,
        select: input.select.take(),
        cycle,
    };
    net.client.send(&ClientMsg::Input(frame));
    net.client.flush();
}

/// Closing the app leaves the game, so the host hears it now rather than times out.
fn stop_on_exit(mut exits: MessageReader<AppExit>, mut commands: Commands) {
    if exits.read().next().is_some() {
        commands.queue(stop_joining);
    }
}
