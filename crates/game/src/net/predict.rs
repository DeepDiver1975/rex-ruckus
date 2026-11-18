//! Client-side prediction: the client's own player moves and shoots on the tick it is told to,
//! without waiting for the host. Every tick's controls go into [`InputHistory`]; when a snapshot
//! arrives, [`reconcile`] takes the host's state of the player and replays the frames the host
//! has not applied yet on top of it. The camera hides the jump a small correction makes with a
//! [`CorrectionOffset`] that fades out; the simulated body takes the host's state at once.
//!
//! Only movement and the arsenal are predicted. The jetpack, medkit and night-vision taps are
//! the host's to apply (`use_inventory`): prediction and replay step with the inventory of the
//! last snapshot, so a jetpack switched on flies here once a snapshot says it is on.

use std::collections::VecDeque;

use bevy::prelude::*;
use rr_core::collide::Body;
use rr_core::defs::Defs;
use rr_core::inventory::Inventory as CoreInventory;
use rr_core::map::Map;
use rr_core::movement::Tuning;
use rr_core::rng::Rng;
use rr_core::snapshot::PlayerSnap;
use rr_core::vitals::Vitals;
use rr_core::weapons::{Arsenal, WeaponEvent};
use rr_net::{ClientMsg, INPUT_REDUNDANCY, InputFrame, buttons};

use super::client::NetClient;
use super::{NetMode, apply_frame};
use crate::combat::{
    FxQueue, GameDefs, LevelCombat, PlayRng, PlayerArsenal, PlayerInventory, PlayerVitals, aim_dir,
    weapon_input,
};
use crate::flow::{PausedFrom, PlayState, world_running};
use crate::level::CurrentMap;
use crate::player::{
    Inventory, LocalPlayer, Look, PITCH_LIMIT, PendingInput, Player, PlayerBody, PlayerTuning,
    PrevFeet, step_body,
};

/// Most frames [`InputHistory`] keeps: two seconds at the 60 Hz tick.
pub const INPUT_HISTORY: usize = 128;

/// A correction farther than this (metres) is a teleport, not an error: the camera snaps.
pub const CORRECTION_SNAP: f32 = 1.0;

/// Rate (1/s) at which [`CorrectionOffset`] fades: under 1 % left after 92 ms.
pub const CORRECTION_EASE_RATE: f32 = 50.0;

/// The local player's frames the host has not acknowledged yet, oldest first.
#[derive(Resource, Default)]
pub struct InputHistory(pub VecDeque<InputFrame>);

/// Where the camera draws the local player relative to its simulated body (core metres): a
/// correction moves the body at once and this by the opposite, so the view slides onto the new
/// spot as it fades ([`CorrectionOffset::fade`]). Never added to the body itself.
#[derive(Resource, Default, Debug)]
pub struct CorrectionOffset {
    pub offset: Vec3,
    /// The length of the last correction (metres), for tests and debugging.
    pub last: f32,
}

impl CorrectionOffset {
    /// Takes a correction from `predicted` to `corrected`: a small one adds to the offset, one
    /// past [`CORRECTION_SNAP`] clears it.
    pub fn correct(&mut self, predicted: Vec3, corrected: Vec3) {
        let error = predicted - corrected;
        self.last = error.length();
        self.offset = if self.last > CORRECTION_SNAP {
            Vec3::ZERO
        } else {
            self.offset + error
        };
    }

    /// Fades the offset over a frame of `dt` seconds, frame-rate independent.
    pub fn fade(&mut self, dt: f32) {
        self.offset *= (-CORRECTION_EASE_RATE * dt).exp();
    }
}

/// Forgets the prediction of a session: a new one starts its frames at seq 1 again.
pub fn reset_prediction(world: &mut World) {
    world.insert_resource(InputHistory::default());
    world.insert_resource(CorrectionOffset::default());
}

/// What every predicted step reads besides the player.
struct StepCtx<'a> {
    map: &'a Map,
    tuning: &'a Tuning,
    defs: &'a Defs,
    /// The local player's pipe bombs in the world (they keep the launcher armed).
    live_bombs: u32,
    dt: f32,
}

/// The local player's state a predicted step changes or reads.
struct Predicted<'a> {
    body: &'a mut Body,
    arsenal: &'a mut Arsenal,
    vitals: &'a Vitals,
    inventory: &'a CoreInventory,
}

/// One tick of `frame` for the local player, as the host steps it: movement, then the arsenal
/// (never an attack: the host's shot is the one that hits). Use, medkit, jetpack and
/// night-vision taps are left to the host. Returns the arsenal's events.
fn step_local(ctx: &StepCtx, frame: &InputFrame, me: Predicted, rng: &mut Rng) -> Vec<WeaponEvent> {
    let mut input = PendingInput::default();
    let mut look = Look {
        angle: 0.0,
        pitch: 0.0,
    };
    apply_frame(frame, &mut input, &mut look);
    let alive = me.vitals.health.alive();
    step_body(
        ctx.map,
        ctx.tuning,
        ctx.dt,
        me.body,
        &input,
        &look,
        me.inventory.jetpack_on,
        alive,
    );
    let weapons = weapon_input(&mut input);
    if !alive {
        return Vec::new();
    }
    me.arsenal.live_bombs = ctx.live_bombs;
    let aim = aim_dir(look.angle, look.pitch);
    me.arsenal.tick(ctx.defs, &weapons, aim, rng, ctx.dt)
}

/// This tick's frame from the local controls, taking the taps the way the sim systems take
/// them; the automap toggle stays local. In range as the host would clamp it.
fn take_frame(seq: u32, input: &mut PendingInput, look: &Look) -> InputFrame {
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
    InputFrame {
        seq,
        forward: input.forward,
        strafe: input.strafe,
        angle: look.angle,
        pitch: look.pitch,
        buttons: bits,
        select: input.select.take(),
        cycle,
    }
    .clamped(PITCH_LIMIT)
}

/// What [`predict_local`] steps of the local player.
type LocalQuery = (
    &'static Player,
    &'static mut PendingInput,
    &'static Look,
    &'static mut PlayerBody,
    &'static mut PrevFeet,
    &'static mut PlayerArsenal,
    &'static PlayerVitals,
    &'static PlayerInventory,
);

/// The client's tick of its own player: this tick's frame goes into the history and to the
/// host (with the newest [`INPUT_REDUNDANCY`] frames, so one lost datagram loses nothing), and
/// the local player steps by it at once. Its weapon events play here now as the local
/// player's (viewmodel, sound).
#[allow(clippy::too_many_arguments)]
pub fn predict_local(
    mut net: ResMut<NetClient>,
    mut history: ResMut<InputHistory>,
    state: Res<PlayState>,
    from: Res<PausedFrom>,
    mode: Option<Res<NetMode>>,
    time: Res<Time<Fixed>>,
    map: Res<CurrentMap>,
    tuning: Res<PlayerTuning>,
    defs: Res<GameDefs>,
    combat: Option<Res<LevelCombat>>,
    rng: Option<ResMut<PlayRng>>,
    mut fx: ResMut<FxQueue>,
    mut q: Query<LocalQuery, With<LocalPlayer>>,
) {
    if net.welcomed.is_none() || !world_running(state, from, mode) {
        return;
    }
    let Ok((player, mut input, look, mut body, mut prev, mut arsenal, vitals, carried)) =
        q.single_mut()
    else {
        return;
    };
    net.seq += 1;
    let frame = take_frame(net.seq, &mut input, look);
    history.0.push_back(frame.clone());
    while history.0.len() > INPUT_HISTORY {
        history.0.pop_front();
    }
    let ctx = StepCtx {
        map: &map.0,
        tuning: &tuning.0,
        defs: &defs.0,
        live_bombs: combat.map_or(0, |c| c.0.live_bombs(player.id)),
        dt: time.timestep().as_secs_f32(),
    };
    prev.0 = body.0.pos;
    let me = Predicted {
        body: &mut body.0,
        arsenal: &mut arsenal.0,
        vitals: &vitals.0,
        inventory: &carried.0,
    };
    let mut scratch = Rng::new(0);
    let rng = match rng {
        Some(rng) => &mut rng.into_inner().0,
        None => &mut scratch,
    };
    let events = step_local(&ctx, &frame, me, rng);
    fx.weapon.extend(events.into_iter().map(|e| (player.id, e)));
    let newest = history.0.len().saturating_sub(INPUT_REDUNDANCY);
    let frames = history.0.iter().skip(newest).cloned().collect();
    net.client.send(&ClientMsg::Input(frames));
    net.client.flush();
}

/// Takes the host's state of the local player from `snap` (the newest snapshot, on arrival):
/// frames up to its `ack` leave the history, the rest are replayed on top of it with their
/// events discarded (they already played when predicted). The difference to the prediction
/// goes into [`CorrectionOffset`]. Inventory taps in the replayed frames are not replayed (the
/// host applies them; see the module docs).
pub fn reconcile(world: &mut World, snap: &PlayerSnap) {
    {
        let mut history = world.resource_mut::<InputHistory>();
        while history.0.front().is_some_and(|f| f.seq <= snap.ack) {
            history.0.pop_front();
        }
    }
    let Ok((e, predicted)) = world
        .query_filtered::<(Entity, &PlayerBody), With<LocalPlayer>>()
        .single(world)
        .map(|(e, b)| (e, b.0.pos))
    else {
        return;
    };
    let mut body = snap.body;
    let mut arsenal = snap.arsenal.clone();
    {
        let map = &world.resource::<CurrentMap>().0;
        // The host's body indexes this level; one that does not fit it cannot be stepped.
        if body.sector >= map.sectors.len() {
            return;
        }
        let ctx = StepCtx {
            map,
            tuning: &world.resource::<PlayerTuning>().0,
            defs: &world.resource::<GameDefs>().0,
            live_bombs: world
                .get_resource::<LevelCombat>()
                .map_or(0, |c| c.0.live_bombs(snap.id)),
            dt: world.resource::<Time<Fixed>>().timestep().as_secs_f32(),
        };
        // Spread is drawn from a scratch stream: replayed shots are never shown.
        let mut rng = Rng::new(0);
        for frame in &world.resource::<InputHistory>().0 {
            let me = Predicted {
                body: &mut body,
                arsenal: &mut arsenal,
                vitals: &snap.vitals,
                inventory: &snap.inventory,
            };
            step_local(&ctx, frame, me, &mut rng);
        }
    }
    world
        .get_resource_or_init::<CorrectionOffset>()
        .correct(predicted, body.pos);
    let mut entity = world.entity_mut(e);
    entity.insert((
        PlayerBody(body),
        PlayerVitals(snap.vitals),
        PlayerArsenal(arsenal),
        PlayerInventory(snap.inventory),
    ));
    if let Some(mut inv) = entity.get_mut::<Inventory>() {
        inv.keys = snap.keys;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn small_corrections_add_up_and_a_teleport_snaps() {
        let mut c = CorrectionOffset::default();
        c.correct(Vec3::new(0.3, 0.0, 0.0), Vec3::ZERO);
        c.correct(Vec3::new(0.0, 0.2, 0.0), Vec3::ZERO);
        assert!((c.offset - Vec3::new(0.3, 0.2, 0.0)).length() < 1e-6);
        assert!((c.last - 0.2).abs() < 1e-6);
        c.correct(Vec3::new(3.0, 0.0, 0.0), Vec3::ZERO);
        assert_eq!(c.offset, Vec3::ZERO);
        assert!((c.last - 3.0).abs() < 1e-6);
    }

    #[test]
    fn the_offset_fades_in_100_ms_at_any_frame_rate() {
        let fade = |frames: u32| {
            let mut c = CorrectionOffset {
                offset: Vec3::X,
                last: 1.0,
            };
            for _ in 0..frames {
                c.fade(0.1 / frames as f32);
            }
            c.offset.x
        };
        let (slow, fast) = (fade(6), fade(24));
        assert!(slow < 0.01, "{slow}");
        assert!((slow - fast).abs() < 1e-5, "{slow} vs {fast}");
    }
}
