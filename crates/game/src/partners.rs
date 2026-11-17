//! Co-op partners drawn as Rex: every [`Player`] that is not the [`LocalPlayer`] gets a
//! [`PartnerVisual`] root with the player model from the [`ModelLibrary`] under it, placed between
//! its last two ticks, animated from its body, vitals and recent hurts and shots
//! ([`partner_clip`]), tinted in its slot colour ([`slot_tint`] on the [`team_material`]s) and
//! holding its current gun ([`held_for`]) in the model's hand.
//!
//! Purely presentational: these systems read the sim and write transforms, materials, animation
//! players and visual-only entities; they never touch bodies, vitals or input. Without a
//! [`ModelLibrary`] (headless tests) only the bare roots spawn and move.

use crate::combat::{FxQueue, FxReaders, PlayerArsenal, PlayerVitals};
use crate::coords::{core_angle_to_yaw, to_bevy};
use crate::flow::LevelEntity;
use crate::models::{
    AttachModels, KeepWorldScale, Look as Skin, ModelLibrary, ModelReady, ModelSlot, PlayerClip,
    Rig, TintCache, spawn_model_with,
};
use crate::player::{LocalPlayer, Look, Player, PlayerBody, PlayerTuning, PrevFeet};
use bevy::animation::RepeatAnimation;
use bevy::gltf::GltfMaterialName;
use bevy::prelude::*;
use rr_core::combat::CombatEvent;
use rr_core::defs::WeaponId;
use rr_core::models::{HeldModel, PlayerModel};
use rr_core::players::PlayerId;
use rr_core::weapons::WeaponEvent;
use std::time::Duration;

/// Root of a co-op partner's visual; `player` is the partner's [`Player`] entity.
#[derive(Component)]
pub struct PartnerVisual {
    pub player: Entity,
}

/// Horizontal speed (m/s) below which a partner stands idle.
pub const IDLE_SPEED: f32 = 0.3;
/// Horizontal speed (m/s) from which a partner runs (`max_speed` is 9).
pub const RUN_SPEED: f32 = 4.5;
/// Seconds a hurt partner plays its pain clip.
pub const PAIN_SECS: f32 = 0.3;
/// Seconds a partner plays its attack clip after a shot.
pub const FIRE_SECS: f32 = 0.25;

/// Cross-fade between two clips.
const FADE: Duration = Duration::from_millis(150);

/// Clip for a partner: dead -> Death; else crouched (body.height < stand_height) -> Crouch (Rex's
/// Attack and Pain clips are standing poses, which would put a crouched head through the ceiling,
/// so a crouched partner ignores both); else horizontal speed >= [`IDLE_SPEED`] -> Walk, or Run from
/// [`RUN_SPEED`] (moving keeps its legs when it shoots or is hurt); else pain timer > 0 -> Pain;
/// else fire timer > 0 -> Attack; else Idle.
pub fn partner_clip(alive: bool, crouched: bool, speed: f32, pain: f32, fire: f32) -> PlayerClip {
    if !alive {
        PlayerClip::Death
    } else if crouched {
        PlayerClip::Crouch
    } else if speed >= RUN_SPEED {
        PlayerClip::Run
    } else if speed >= IDLE_SPEED {
        PlayerClip::Walk
    } else if pain > 0.0 {
        PlayerClip::Pain
    } else if fire > 0.0 {
        PlayerClip::Attack
    } else {
        PlayerClip::Idle
    }
}

/// Held-gun scene for a weapon, `None` for an empty hand.
pub fn held_for(model: &PlayerModel, id: WeaponId) -> Option<&HeldModel> {
    model.held(id)
}

/// Whether a glTF material takes the slot tint.
pub fn team_material(name: &str) -> bool {
    name.starts_with("RexTeam")
}

/// The sRGB multiplier of player `id`'s slot (`ModelDefs::validate` guarantees one per slot).
pub fn slot_tint(model: &PlayerModel, id: PlayerId) -> (f32, f32, f32) {
    model.slot_tints[usize::from(id.0)]
}

/// Seconds left of the pain and attack clips, and whether a hurt or shot came in this frame.
#[derive(Component, Default)]
struct PartnerTimers {
    pain: f32,
    fire: f32,
    hurt_now: bool,
    fired_now: bool,
}

/// The Rex model root (the [`ModelSlot`] entity) under a partner's visual.
#[derive(Component)]
struct PartnerModel(Entity);

/// The clip a partner's model plays, and whether it has been seen alive since it spawned (a
/// partner first seen dead lies still instead of falling over again).
#[derive(Component, Default)]
struct PartnerAnim {
    clip: Option<PlayerClip>,
    seen_alive: bool,
}

/// The gun shown in a partner's hand: `weapon` once placed (`None` before), `scene` its root.
#[derive(Component, Default)]
struct HeldGun {
    weapon: Option<WeaponId>,
    scene: Option<Entity>,
}

/// The team materials of a partner's model wear the slot tint.
#[derive(Component)]
struct Tinted;

/// Draws co-op partners. Needs `PlayerSimPlugin` and `CombatSimPlugin`; the Rex model needs
/// [`ModelsPlugin`](crate::models::ModelsPlugin) (without it only the bare roots spawn).
pub struct PartnersPlugin;

impl Plugin for PartnersPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(
            Update,
            (
                (despawn_orphans, spawn_partner_visuals).chain(),
                place_partners.after(spawn_partner_visuals),
                partner_timers
                    .in_set(FxReaders)
                    .after(spawn_partner_visuals),
                (
                    animate_partners.after(partner_timers),
                    tint_partners,
                    hold_guns.in_set(AttachModels),
                )
                    .after(spawn_partner_visuals)
                    .run_if(resource_exists::<ModelLibrary>),
            ),
        );
    }
}

/// A co-op player this machine does not control.
type Partner = (With<Player>, Without<LocalPlayer>);

/// Drops the visual of a partner that is gone (or became the local player).
fn despawn_orphans(
    mut commands: Commands,
    roots: Query<(Entity, &PartnerVisual)>,
    partners: Query<(), Partner>,
) {
    for (root, v) in &roots {
        if !partners.contains(v.player) {
            commands.entity(root).despawn();
        }
    }
}

/// A visual root for every partner without one and, with a [`ModelLibrary`], Rex under it.
fn spawn_partner_visuals(
    mut commands: Commands,
    lib: Option<Res<ModelLibrary>>,
    partners: Query<(Entity, &PlayerBody, &Look), Partner>,
    shown: Query<&PartnerVisual>,
) {
    for (player, body, look) in &partners {
        if shown.iter().any(|v| v.player == player) {
            continue;
        }
        let root = commands
            .spawn((
                PartnerVisual { player },
                PartnerTimers::default(),
                PartnerAnim::default(),
                HeldGun::default(),
                Transform::from_translation(to_bevy(body.0.pos))
                    .with_rotation(Quat::from_rotation_y(core_angle_to_yaw(look.angle))),
                Visibility::default(),
                LevelEntity,
            ))
            .id();
        let Some(lib) = lib.as_deref() else {
            continue;
        };
        let rex = &lib.player.model;
        let slot = ModelSlot {
            rig: Some(Rig::Player),
            no_shadows: false,
        };
        let model = spawn_model_with(&mut commands, root, &rex.scene, &rex.place, slot);
        commands.entity(root).insert(PartnerModel(model));
    }
}

/// Places each partner's root between its last two ticks, facing its view heading.
fn place_partners(
    fixed: Res<Time<Fixed>>,
    mut roots: Query<(&PartnerVisual, &mut Transform)>,
    players: Query<(&PlayerBody, &PrevFeet, &Look)>,
) {
    let alpha = fixed.overstep_fraction();
    for (v, mut t) in &mut roots {
        let Ok((body, prev, look)) = players.get(v.player) else {
            continue;
        };
        t.translation = to_bevy(prev.0.lerp(body.0.pos, alpha));
        t.rotation = Quat::from_rotation_y(core_angle_to_yaw(look.angle));
    }
}

/// Counts the pain and attack clips down; the partner's `PlayerHurt` restarts pain and its
/// fire, launch or kick restarts the attack. An [`FxReaders`] system.
fn partner_timers(
    time: Res<Time>,
    fx: Res<FxQueue>,
    players: Query<&Player>,
    mut roots: Query<(&PartnerVisual, &mut PartnerTimers)>,
) {
    let dt = time.delta_secs();
    for (v, mut t) in &mut roots {
        t.pain = (t.pain - dt).max(0.0);
        t.fire = (t.fire - dt).max(0.0);
        t.hurt_now = false;
        t.fired_now = false;
        let Ok(&Player { id }) = players.get(v.player) else {
            continue;
        };
        if fx
            .combat
            .iter()
            .any(|ev| matches!(ev, CombatEvent::PlayerHurt { player, .. } if *player == id))
        {
            t.pain = PAIN_SECS;
            t.hurt_now = true;
        }
        if fx.weapon.iter().any(|(p, ev)| {
            *p == id
                && matches!(
                    ev,
                    WeaponEvent::Fire { .. }
                        | WeaponEvent::Launch { .. }
                        | WeaponEvent::Kick { .. }
                )
        }) {
            t.fire = FIRE_SECS;
            t.fired_now = true;
        }
    }
}

/// Plays each ready partner model's [`partner_clip`]: cross-fades when the clip changes, loops
/// all but Attack, Pain and Death, and replays Attack or Pain on a fresh shot or hurt. Death
/// ends on its last frame; a partner first seen dead jumps there.
#[allow(clippy::too_many_arguments)]
fn animate_partners(
    lib: Res<ModelLibrary>,
    tuning: Res<PlayerTuning>,
    clips: Option<Res<Assets<AnimationClip>>>,
    mut roots: Query<(
        &PartnerVisual,
        &PartnerModel,
        &PartnerTimers,
        &mut PartnerAnim,
    )>,
    players: Query<(&PlayerBody, Option<&PlayerVitals>)>,
    ready: Query<&ModelReady>,
    mut anim_players: Query<(&mut AnimationPlayer, &mut AnimationTransitions)>,
) {
    let Some(graph) = &lib.player.graph else {
        return;
    };
    for (v, model, timers, mut anim) in &mut roots {
        let (Ok((body, vitals)), Ok(ready)) = (players.get(v.player), ready.get(model.0)) else {
            continue;
        };
        let Some(Ok((mut player, mut transitions))) = ready.player.map(|p| anim_players.get_mut(p))
        else {
            continue;
        };
        let alive = vitals.is_none_or(|v| v.0.health.alive());
        let speed = body.0.vel.truncate().length();
        let still = speed < IDLE_SPEED;
        let clip = partner_clip(
            alive,
            body.0.height < tuning.0.stand_height,
            speed,
            timers.pain,
            timers.fire,
        );
        // A model missing a clip stands idle instead.
        let Some((clip, &node)) = [clip, PlayerClip::Idle]
            .into_iter()
            .find_map(|c| graph.nodes.get(&c).map(|n| (c, n)))
        else {
            continue;
        };
        if anim.clip != Some(clip) {
            let active = transitions.play(&mut player, node, FADE);
            active.set_repeat(if clip.once() {
                RepeatAnimation::Never
            } else {
                RepeatAnimation::Forever
            });
            if clip == PlayerClip::Death && !anim.seen_alive {
                let end = graph
                    .clips
                    .get(&clip)
                    .and_then(|h| clips.as_ref()?.get(h))
                    .map_or(0.0, |c| c.duration());
                active.seek_to(end).pause();
            }
            anim.clip = Some(clip);
        } else if ((clip == PlayerClip::Pain && timers.hurt_now)
            || (clip == PlayerClip::Attack && timers.fired_now))
            && let Some(active) = player.animation_mut(node)
        {
            active.replay();
        }
        // A crouched partner that stands still holds its pose instead of walking in place.
        if clip == PlayerClip::Crouch
            && let Some(active) = player.animation_mut(node)
        {
            active.set_speed(if still { 0.0 } else { 1.0 });
        }
        anim.seen_alive |= alive;
    }
}

/// Once a partner's model is ready and its materials have loaded, swaps every mesh whose glTF
/// material is a [`team_material`] to the [`TintCache`] variant in the partner's slot colour.
#[allow(clippy::too_many_arguments)]
fn tint_partners(
    mut commands: Commands,
    lib: Res<ModelLibrary>,
    mut cache: ResMut<TintCache>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    roots: Query<(Entity, &PartnerVisual, &PartnerModel), Without<Tinted>>,
    players: Query<&Player>,
    ready: Query<&ModelReady>,
    mut mats: Query<(&GltfMaterialName, &mut MeshMaterial3d<StandardMaterial>)>,
) {
    for (root, v, model) in &roots {
        let (Ok(player), Ok(ready)) = (players.get(v.player), ready.get(model.0)) else {
            continue;
        };
        let team: Vec<(Entity, Handle<StandardMaterial>)> = ready
            .meshes
            .iter()
            .filter_map(|&e| {
                let (name, m) = mats.get(e).ok()?;
                team_material(name).then(|| (e, m.0.clone()))
            })
            .collect();
        // Wait for the sources: `TintCache` hands back an untinted source until it has loaded.
        if !team
            .iter()
            .all(|(_, source)| materials.get(source).is_some())
        {
            continue;
        }
        let tint = Some(slot_tint(&lib.defs.player, player.id));
        for (e, source) in team {
            if let Ok((_, mut m)) = mats.get_mut(e) {
                m.0 = cache.get(&mut materials, &source, Skin::Normal, tint);
            }
        }
        commands.entity(root).insert(Tinted);
    }
}

/// Hangs each partner's current gun ([`held_for`]) from the model's hand bone once the model is
/// ready, and swaps it whenever the partner switches weapons. Like the enemies' attachments
/// the gun keeps its placement's scale and offset in world units ([`KeepWorldScale`]).
fn hold_guns(
    mut commands: Commands,
    lib: Res<ModelLibrary>,
    mut roots: Query<(&PartnerVisual, &PartnerModel, &mut HeldGun)>,
    arsenals: Query<&PlayerArsenal>,
    ready: Query<&ModelReady>,
) {
    for (v, model, mut held) in &mut roots {
        let (Ok(arsenal), Ok(ready)) = (arsenals.get(v.player), ready.get(model.0)) else {
            continue;
        };
        let current = arsenal.0.current;
        if held.weapon == Some(current) {
            continue;
        }
        held.weapon = Some(current);
        if let Some(old) = held.scene.take()
            && let Ok(mut e) = commands.get_entity(old)
        {
            e.despawn();
        }
        let Some(gun) =
            held_for(&lib.defs.player, current).and_then(|h| lib.player.held.get(&h.id))
        else {
            continue;
        };
        let Some(&hand) = ready.nodes.get(&lib.defs.player.hand) else {
            warn!("player model: no bone named {:?}", lib.defs.player.hand);
            continue;
        };
        let place = &gun.place;
        let e = spawn_model_with(&mut commands, hand, &gun.scene, place, ModelSlot::default());
        commands.entity(e).insert(KeepWorldScale {
            scale: place.scale,
            offset: Vec3::from(place.offset),
        });
        held.scene = Some(e);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use PlayerClip::*;
    use rr_core::models::ModelDefs;

    #[test]
    fn death_wins_over_everything() {
        assert_eq!(partner_clip(false, true, 8.0, 1.0, 1.0), Death);
    }

    #[test]
    fn moving_beats_fire_and_pain() {
        assert_eq!(partner_clip(true, false, 6.0, 0.1, 0.1), Run);
        assert_eq!(partner_clip(true, false, 2.0, 0.0, 0.1), Walk);
        assert_eq!(partner_clip(true, false, 2.0, 0.1, 0.0), Walk);
    }

    #[test]
    fn pain_beats_fire_when_still() {
        assert_eq!(partner_clip(true, false, 0.0, 0.1, 0.1), Pain);
        assert_eq!(partner_clip(true, false, 0.0, 0.0, 0.1), Attack);
    }

    #[test]
    fn crouched_partner_never_stands() {
        for speed in [0.0, 2.0] {
            assert_eq!(partner_clip(true, true, speed, 0.1, 0.1), Crouch);
            assert_eq!(partner_clip(true, true, speed, 0.1, 0.0), Crouch);
            assert_eq!(partner_clip(true, true, speed, 0.0, 0.1), Crouch);
        }
    }

    #[test]
    fn crouched_partner_crouches() {
        assert_eq!(partner_clip(true, true, 3.0, 0.0, 0.0), Crouch);
        assert_eq!(partner_clip(true, true, 0.0, 0.0, 0.0), Crouch);
    }

    #[test]
    fn speed_picks_idle_walk_run() {
        assert_eq!(partner_clip(true, false, 0.1, 0.0, 0.0), Idle);
        assert_eq!(partner_clip(true, false, 2.0, 0.0, 0.0), Walk);
        assert_eq!(partner_clip(true, false, 6.0, 0.0, 0.0), Run);
    }

    #[test]
    fn held_gun_follows_weapon() {
        let defs = ModelDefs::builtin();
        let held = held_for(&defs.player, WeaponId::Chaingun).expect("a held chaingun");
        assert_eq!(held.id, WeaponId::Chaingun);
        assert!(held.scene.ends_with("chaingun.glb"), "{}", held.scene);
        assert!(held_for(&defs.player, WeaponId::Boot).is_none());
        assert!(held_for(&defs.player, WeaponId::PipeBombs).is_none());
    }

    #[test]
    fn team_materials_take_the_tint() {
        assert!(team_material("RexTeamSuit"));
        assert!(!team_material("RexSkin"));
    }

    #[test]
    fn slots_have_distinct_tints() {
        let defs = ModelDefs::builtin();
        let tints: Vec<_> = (0..4)
            .map(|i| slot_tint(&defs.player, PlayerId(i)))
            .collect();
        for (i, a) in tints.iter().enumerate() {
            for b in &tints[i + 1..] {
                assert_ne!(a, b, "{tints:?}");
            }
        }
    }
}
