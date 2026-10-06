//! Enemy, barrel, projectile and impact visuals. Enemies and barrels are animated glTF models
//! from the [`ModelLibrary`]; projectiles and sparks stay procedural.
//!
//! Purely presentational: every system here only reads the simulation ([`LevelCombat`],
//! [`FxQueue`]) and writes transforms, materials, animation players and visual-only entities.
//! Every actor gets an [`ActorVisual`] root plus the marker of its kind, with its model spawned
//! under it ([`spawn_actors`]). [`humanoid`] places the walkers (Grunt, Enforcer, Slasher) and
//! attaches the Grunt's pistol, [`drone`] and [`barrel`] handle their kinds, [`anim`] picks and
//! plays clips, and [`projectiles`] holds the bolt, rocket, bomb and spark visuals.

mod anim;
mod barrel;
mod drone;
mod humanoid;
mod projectiles;

pub use anim::{AnimCmd, anim_for, pick_role};
pub use barrel::BarrelVisual;
pub use drone::DroneVisual;
pub use humanoid::{EnforcerVisual, GruntVisual, SlasherVisual};
pub use projectiles::{
    BoltVisual, BombVisual, ProjKind, RocketVisual, SPARK_SECS, Spark, proj_kind,
};

use crate::combat::{FxQueue, FxReaders, LevelCombat, spawn_combat};
use crate::coords::{core_angle_to_yaw, to_bevy};
use crate::flow::{LevelEntity, SpawnLevel};
use crate::models::{Look, ModelLibrary, ModelReady, ModelSlot, TintCache, spawn_model_with};
use bevy::light::NotShadowCaster;
use bevy::prelude::*;
use rr_core::actors::{Actor, AiState};
use rr_core::combat::CombatEvent;
use rr_core::map::ActorKind;
use rr_core::models::Tip;
use std::f32::consts::FRAC_PI_2;

/// Seconds the gun tip glows after a shot.
pub const TIP_GLOW_SECS: f32 = 0.12;
/// Seconds the body flashes after a hit that did not kill.
pub const HIT_FLASH_SECS: f32 = 0.1;
/// Radius of the gun-tip glow (m).
const TIP_RADIUS: f32 = 0.07;

/// Root of every actor's visual; the index is into `LevelCombat.actors`. Next to it the root
/// carries the marker of its kind ([`GruntVisual`], [`EnforcerVisual`], ...).
#[derive(Component)]
pub struct ActorVisual(pub usize);

/// Seconds the gun tip keeps glowing.
#[derive(Component, Default)]
struct TipGlow(f32);

/// Seconds of hit flash left on the body.
#[derive(Component, Default)]
struct HitFlash(f32);

/// The glTF model spawned under an actor's root (the [`ModelSlot`] entity).
#[derive(Component)]
struct ActorModel {
    root: Entity,
    kind: ActorKind,
}

/// The model's original mesh materials (captured once it is ready) and the look they show.
#[derive(Component, Default)]
struct ModelLook {
    originals: Option<Vec<(Entity, Handle<StandardMaterial>)>>,
    current: Option<Look>,
}

/// The gun-tip glow sphere of an actor that shoots; `None` until its tip node exists.
#[derive(Component, Default)]
struct TipSphere(Option<Entity>);

/// Keeps an entity at a fixed world scale (and its offset in world units) although its parent
/// sits inside a glTF's scaled armature: the local transform divides out the parent's scale.
#[derive(Component, Clone, Copy)]
struct KeepWorldScale {
    scale: f32,
    offset: Vec3,
}

/// Meshes and materials shared by every bolt, spark and tip glow; survives restarts.
#[derive(Resource, Clone)]
struct ActorAssets {
    tip_mesh: Handle<Mesh>,
    tip_glow: Handle<StandardMaterial>,
    proj: projectiles::ProjAssets,
}

impl ActorAssets {
    fn new(meshes: &mut Assets<Mesh>, materials: &mut Assets<StandardMaterial>) -> Self {
        ActorAssets {
            tip_mesh: meshes.add(Sphere::new(TIP_RADIUS)),
            tip_glow: materials.add(emissive(Color::srgb(1.0, 0.65, 0.18), 8.0)),
            proj: projectiles::ProjAssets::new(meshes, materials),
        }
    }
}

fn emissive(c: Color, strength: f32) -> StandardMaterial {
    StandardMaterial {
        base_color: c,
        emissive: c.to_linear() * strength,
        ..default()
    }
}

fn matte(c: Color) -> StandardMaterial {
    StandardMaterial {
        base_color: c,
        perceptual_roughness: 0.8,
        ..default()
    }
}

/// Enemy and barrel models, their animation and looks, and projectiles. Needs a renderer's
/// assets (`Assets<Mesh>`, `Assets<StandardMaterial>`) and `CombatSimPlugin`; the models need
/// [`ModelsPlugin`](crate::models::ModelsPlugin) (without it only the bare roots spawn). Not
/// part of the headless sim.
pub struct ActorVisualsPlugin;

impl Plugin for ActorVisualsPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(SpawnLevel, spawn_actors.after(spawn_combat))
            .add_systems(
                Update,
                (
                    (flash_tips, flash_hits, projectiles::spawn_sparks).in_set(FxReaders),
                    anim::animate_enemies
                        .in_set(FxReaders)
                        .run_if(resource_exists::<ModelLibrary>),
                    (
                        humanoid::place_walkers,
                        drone::place_drones,
                        barrel::hide_dead_barrels,
                        glow_tips,
                    )
                        .after(flash_tips),
                    (
                        tint_enemies.after(flash_hits),
                        humanoid::attach_scenes,
                        attach_tips,
                    )
                        .run_if(resource_exists::<ModelLibrary>),
                    // After the spawns, so Bevy's sync point applies them and a fresh attachment
                    // is corrected the same frame.
                    keep_world_scale
                        .after(humanoid::attach_scenes)
                        .after(attach_tips),
                    projectiles::sync_projectiles,
                    projectiles::age_sparks,
                )
                    .run_if(resource_exists::<ActorAssets>),
            );
    }
}

/// Death pitch-over angle (radians, 0 = upright, π/2 = flat) with `t_left` of `death_time` to
/// go. Monotonic in elapsed time and clamped to [0, π/2]; accelerates like a fall.
pub fn death_pitch(t_left: f32, death_time: f32) -> f32 {
    let done = if death_time > 0.0 {
        1.0 - t_left / death_time
    } else {
        1.0
    };
    let done = done.clamp(0.0, 1.0);
    FRAC_PI_2 * done * done
}

/// Whether an enemy kind shoots, and so gets a gun-tip glow.
fn has_gun(kind: ActorKind) -> bool {
    matches!(
        kind,
        ActorKind::Grunt | ActorKind::Enforcer | ActorKind::Drone
    )
}

/// One visual per actor, in [`SpawnLevel`] after combat has spawned: a root with the marker of
/// its kind and, with a [`ModelLibrary`], its glTF model and (for an offset tip) the tip glow.
fn spawn_actors(
    mut commands: Commands,
    existing: Option<Res<ActorAssets>>,
    lib: Option<Res<ModelLibrary>>,
    combat: Res<LevelCombat>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let a = match existing {
        Some(a) => a.clone(),
        None => {
            let a = ActorAssets::new(&mut meshes, &mut materials);
            commands.insert_resource(a.clone());
            a
        }
    };
    for (i, actor) in combat.0.actors.iter().enumerate() {
        let root = commands
            .spawn((
                ActorVisual(i),
                TipGlow::default(),
                HitFlash::default(),
                TipSphere::default(),
                Transform::from_translation(to_bevy(actor.body.pos))
                    .with_rotation(Quat::from_rotation_y(core_angle_to_yaw(actor.angle))),
                Visibility::default(),
                LevelEntity,
            ))
            .id();
        let mut e = commands.entity(root);
        match actor.kind {
            ActorKind::Grunt => e.insert(GruntVisual(i)),
            ActorKind::Enforcer => e.insert(EnforcerVisual(i)),
            ActorKind::Slasher => e.insert(SlasherVisual(i)),
            ActorKind::Drone => e.insert(DroneVisual(i)),
            ActorKind::Barrel => e.insert(BarrelVisual(i)),
        };
        let Some(lib) = lib.as_deref() else {
            continue;
        };
        let enemy = lib.enemy(actor.kind);
        let slot = ModelSlot {
            enemy: Some(actor.kind),
            no_shadows: false,
        };
        let model = spawn_model_with(
            &mut commands,
            root,
            &enemy.model.scene,
            &enemy.model.place,
            slot,
        );
        commands.entity(root).insert((
            ActorModel {
                root: model,
                kind: actor.kind,
            },
            ModelLook::default(),
            anim::EnemyAnim::default(),
        ));
        let tip = &lib.defs.enemy(actor.kind).tip;
        if has_gun(actor.kind)
            && let Tip::Offset(o) = tip
        {
            let sphere = spawn_tip(
                &mut commands,
                &a,
                root,
                Transform::from_translation(Vec3::from(*o)),
            );
            commands.entity(root).insert(TipSphere(Some(sphere)));
        }
    }
}

/// A hidden tip-glow sphere under `parent`.
fn spawn_tip(commands: &mut Commands, a: &ActorAssets, parent: Entity, t: Transform) -> Entity {
    commands
        .spawn((
            Mesh3d(a.tip_mesh.clone()),
            MeshMaterial3d(a.tip_glow.clone()),
            NotShadowCaster,
            t,
            Visibility::Hidden,
            ChildOf(parent),
        ))
        .id()
}

/// Parents the tip glow to its named node (e.g. `Muzzle`) once the model is ready.
fn attach_tips(
    mut commands: Commands,
    a: Res<ActorAssets>,
    lib: Res<ModelLibrary>,
    mut roots: Query<(&ActorModel, &mut TipSphere)>,
    ready: Query<&ModelReady>,
) {
    for (model, mut sphere) in &mut roots {
        if sphere.0.is_some() || !has_gun(model.kind) {
            continue;
        }
        let Tip::Node(name) = &lib.defs.enemy(model.kind).tip else {
            continue;
        };
        let Some(&node) = ready.get(model.root).ok().and_then(|r| r.nodes.get(name)) else {
            continue;
        };
        let e = spawn_tip(&mut commands, &a, node, Transform::default());
        commands.entity(e).insert(KeepWorldScale {
            scale: 1.0,
            offset: Vec3::ZERO,
        });
        sphere.0 = Some(e);
    }
}

/// Divides each [`KeepWorldScale`] entity's parent scale out of its local transform. Uses last
/// frame's propagated parent transform (bone scales do not animate).
fn keep_world_scale(
    mut q: Query<(&KeepWorldScale, &ChildOf, &mut Transform)>,
    globals: Query<&GlobalTransform>,
) {
    for (k, parent, mut t) in &mut q {
        let Ok(g) = globals.get(parent.parent()) else {
            continue;
        };
        let s = g.to_scale_rotation_translation().0;
        if s.min_element().abs() < 1e-6 {
            continue;
        }
        let want = Transform {
            translation: k.offset / s,
            rotation: t.rotation,
            scale: Vec3::splat(k.scale) / s,
        };
        t.set_if_neq(want);
    }
}

/// Shows each tip glow while its actor's [`TipGlow`] runs and the actor lives.
fn glow_tips(
    combat: Res<LevelCombat>,
    roots: Query<(&ActorVisual, &TipGlow, &TipSphere)>,
    mut vis: Query<&mut Visibility>,
) {
    for (g, glow, sphere) in &roots {
        let (Some(e), Some(actor)) = (sphere.0, combat.0.actors.get(g.0)) else {
            continue;
        };
        if let Ok(mut v) = vis.get_mut(e) {
            v.set_if_neq(if glow.0 > 0.0 && actor.alive() {
                Visibility::Inherited
            } else {
                Visibility::Hidden
            });
        }
    }
}

/// Counts gun-tip glows down; `ActorFired` relights the firing actor's tip for
/// [`TIP_GLOW_SECS`]. An [`FxReaders`] system.
fn flash_tips(time: Res<Time>, fx: Res<FxQueue>, mut q: Query<(&ActorVisual, &mut TipGlow)>) {
    for (_, mut glow) in &mut q {
        glow.0 = (glow.0 - time.delta_secs()).max(0.0);
    }
    for ev in &fx.combat {
        if let CombatEvent::ActorFired { actor } = ev {
            for (g, mut glow) in &mut q {
                if g.0 == *actor {
                    glow.0 = TIP_GLOW_SECS;
                }
            }
        }
    }
}

/// Counts hit flashes down; `ActorHurt` relights the hit actor's flash for
/// [`HIT_FLASH_SECS`]. An [`FxReaders`] system.
fn flash_hits(time: Res<Time>, fx: Res<FxQueue>, mut q: Query<(&ActorVisual, &mut HitFlash)>) {
    for (_, mut flash) in &mut q {
        flash.0 = (flash.0 - time.delta_secs()).max(0.0);
    }
    for ev in &fx.combat {
        if let CombatEvent::ActorHurt { actor, .. } = ev {
            for (g, mut flash) in &mut q {
                if g.0 == *actor {
                    flash.0 = HIT_FLASH_SECS;
                }
            }
        }
    }
}

/// The body look. Priority: pain, then the hit flash, then the AI state's look. The flash
/// shows only on a living actor that is not in pain, so it never fights the pain look and a
/// Dying actor keeps its death look.
fn skin_look(state: Look, alive: bool, hit_flash: f32) -> Look {
    if state != Look::Pain && alive && hit_flash > 0.0 {
        Look::Hit
    } else {
        state
    }
}

/// The look an AI state gives a kind's body: walkers dim asleep, everything that feels pain
/// glows red in it; drones never sleep-dim and barrels only ever flash.
fn pose(kind: ActorKind, state: AiState) -> Look {
    match (kind, state) {
        (ActorKind::Barrel, _) => Look::Normal,
        (_, AiState::Pain { .. }) => Look::Pain,
        (ActorKind::Drone, _) => Look::Normal,
        (_, AiState::Sleep) => Look::Dim,
        _ => Look::Normal,
    }
}

/// Swaps every mesh of each ready model to the [`TintCache`] variant of its original material
/// for the actor's look, only when the look changes. The enemy's `tint` colours `Normal`/`Dim`.
fn tint_enemies(
    combat: Res<LevelCombat>,
    lib: Res<ModelLibrary>,
    mut cache: ResMut<TintCache>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut roots: Query<(&ActorVisual, &ActorModel, &HitFlash, &mut ModelLook)>,
    ready: Query<&ModelReady>,
    mut mats: Query<&mut MeshMaterial3d<StandardMaterial>>,
) {
    for (g, model, hit, mut look) in &mut roots {
        let (Some(actor), Ok(ready)) = (combat.0.actors.get(g.0), ready.get(model.root)) else {
            continue;
        };
        let want = skin_look(pose(model.kind, actor.state), actor.alive(), hit.0);
        if look.current == Some(want) {
            continue;
        }
        let look = &mut *look;
        let originals = look.originals.get_or_insert_with(|| {
            ready
                .meshes
                .iter()
                .filter_map(|&e| mats.get(e).ok().map(|m| (e, m.0.clone())))
                .collect()
        });
        // Wait until every source material has loaded: `TintCache` hands back untinted sources
        // otherwise, and the look would be recorded as applied without ever being retried.
        if !originals
            .iter()
            .all(|(_, source)| materials.get(source).is_some())
        {
            continue;
        }
        let tint = lib.enemy(model.kind).tint;
        for (e, source) in originals.iter() {
            if let Ok(mut m) = mats.get_mut(*e) {
                let h = cache.get(&mut materials, source, want, tint);
                if m.0 != h {
                    m.0 = h;
                }
            }
        }
        look.current = Some(want);
    }
}

/// Places a walker's root between its last two ticks, facing its heading (model forward −Z).
fn place_walker(t: &mut Transform, actor: &Actor, alpha: f32) {
    let feet = actor.prev_pos.lerp(actor.body.pos, alpha);
    t.translation = to_bevy(feet);
    t.rotation = Quat::from_rotation_y(core_angle_to_yaw(actor.angle));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn death_pitch_is_monotonic_and_clamped() {
        let dt = 0.8;
        assert_eq!(death_pitch(dt, dt), 0.0, "upright at the start");
        assert!(
            (death_pitch(0.0, dt) - FRAC_PI_2).abs() < 1e-6,
            "flat at the end"
        );
        let mut last = -1.0;
        for i in 0..=100 {
            let t_left = dt * (1.0 - i as f32 / 100.0);
            let p = death_pitch(t_left, dt);
            assert!(p >= last, "monotonic at t_left {t_left}");
            assert!((0.0..=FRAC_PI_2).contains(&p));
            last = p;
        }
        // Out-of-range inputs stay clamped.
        assert_eq!(death_pitch(2.0 * dt, dt), 0.0);
        assert_eq!(death_pitch(-1.0, dt), FRAC_PI_2);
        assert_eq!(
            death_pitch(0.3, 0.0),
            FRAC_PI_2,
            "no death time: flat at once"
        );
    }

    #[test]
    fn hit_flash_never_overrides_pain_or_death() {
        use Look::*;
        assert_eq!(skin_look(Normal, true, 0.05), Hit);
        assert_eq!(skin_look(Dim, true, 0.05), Hit);
        assert_eq!(skin_look(Normal, true, 0.0), Normal);
        assert_eq!(skin_look(Pain, true, 0.05), Pain, "pain wins");
        assert_eq!(skin_look(Normal, false, 0.05), Normal, "dying: no flash");
    }

    #[test]
    fn looks_per_kind_and_state() {
        use ActorKind::*;
        let pain = AiState::Pain { t: 0.1 };
        for k in [Grunt, Enforcer, Slasher] {
            assert_eq!(pose(k, AiState::Sleep), Look::Dim, "{k:?}");
            assert_eq!(pose(k, pain), Look::Pain, "{k:?}");
            assert_eq!(pose(k, AiState::Chase), Look::Normal, "{k:?}");
            assert_eq!(pose(k, AiState::Dead), Look::Normal, "{k:?}");
        }
        assert_eq!(
            pose(Drone, AiState::Sleep),
            Look::Normal,
            "drones never dim"
        );
        assert_eq!(pose(Drone, pain), Look::Pain);
        assert_eq!(pose(Barrel, pain), Look::Normal, "barrels only flash");
    }
}
