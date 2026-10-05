//! Grunt, bolt and impact visuals: procedural stand-ins built from Bevy primitives until M4's art.
//!
//! Purely presentational: every system here only reads the simulation ([`LevelCombat`],
//! [`FxQueue`]) and writes transforms, materials and visual-only entities.

use crate::combat::{FxQueue, FxReaders, GameDefs, LevelCombat, spawn_combat};
use crate::coords::{core_angle_to_yaw, to_bevy};
use crate::flow::{LevelEntity, SpawnLevel};
use bevy::light::NotShadowCaster;
use bevy::prelude::*;
use rr_core::actors::{Actor, AiState};
use rr_core::combat::CombatEvent;
use rr_core::projectile::Projectile;
use std::collections::BTreeSet;
use std::f32::consts::FRAC_PI_2;

/// Height of the Grunt model as built; the root is scaled to the enemy def's height.
const MODEL_HEIGHT: f32 = 1.75;
/// Half the torso depth: how far the back sticks out behind the pivot when lying flat.
const BACK_HALF_DEPTH: f32 = 0.15;
/// Seconds the gun tip glows after a shot.
pub const TIP_GLOW_SECS: f32 = 0.12;
/// Seconds the body flashes after a hit that did not kill.
pub const HIT_FLASH_SECS: f32 = 0.1;
/// Backward tilt while in pain (radians).
const PAIN_TILT: f32 = 0.25;
/// Head nod while asleep (radians, negative = chin down).
const SLEEP_NOD: f32 = -0.5;
/// Lifetime of an impact spark.
pub const SPARK_SECS: f32 = 0.15;

/// Root of an actor's visual; the index is into `LevelCombat.actors`.
#[derive(Component)]
pub struct GruntVisual(pub usize);

/// The animated parts of a Grunt (children of its [`GruntVisual`] root).
#[derive(Component)]
struct GruntRig {
    /// Parts that take the body material (dimmed asleep, red in pain).
    skin: [Entity; 4],
    neck: Entity,
    visor: Entity,
    arm: Entity,
    tip: Entity,
}

/// Seconds the gun tip keeps glowing.
#[derive(Component, Default)]
struct TipGlow(f32);

/// Seconds of hit flash left on the body.
#[derive(Component, Default)]
struct HitFlash(f32);

/// A flying bolt's visual; the id is a `Projectile::id`.
#[derive(Component)]
pub struct BoltVisual(pub u32);

/// An impact spark with its remaining lifetime in seconds.
#[derive(Component)]
pub struct Spark(pub f32);

/// Meshes and materials shared by every Grunt, bolt and spark; survives restarts.
#[derive(Resource, Clone)]
struct ActorAssets {
    legs: Handle<Mesh>,
    torso: Handle<Mesh>,
    head: Handle<Mesh>,
    visor: Handle<Mesh>,
    upper_arm: Handle<Mesh>,
    gun: Handle<Mesh>,
    tip: Handle<Mesh>,
    bolt: Handle<Mesh>,
    spark: Handle<Mesh>,
    skin: Handle<StandardMaterial>,
    skin_dim: Handle<StandardMaterial>,
    skin_pain: Handle<StandardMaterial>,
    skin_hit: Handle<StandardMaterial>,
    visor_on: Handle<StandardMaterial>,
    visor_off: Handle<StandardMaterial>,
    gun_mat: Handle<StandardMaterial>,
    tip_idle: Handle<StandardMaterial>,
    tip_glow: Handle<StandardMaterial>,
    bolt_mat: Handle<StandardMaterial>,
    spark_mat: Handle<StandardMaterial>,
}

const BOLT_COLOR: Color = Color::srgb(1.0, 0.35, 0.1);

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

impl ActorAssets {
    fn new(meshes: &mut Assets<Mesh>, materials: &mut Assets<StandardMaterial>) -> Self {
        ActorAssets {
            legs: meshes.add(Cuboid::new(0.36, 0.8, 0.22)),
            torso: meshes.add(Cuboid::new(0.52, 0.62, 2.0 * BACK_HALF_DEPTH)),
            head: meshes.add(Cuboid::new(0.26, 0.28, 0.26)),
            visor: meshes.add(Cuboid::new(0.2, 0.06, 0.04)),
            upper_arm: meshes.add(Cuboid::new(0.12, 0.55, 0.12)),
            gun: meshes.add(Cuboid::new(0.12, 0.12, 0.6)),
            tip: meshes.add(Sphere::new(0.06)),
            bolt: meshes.add(Capsule3d::new(0.06, 0.3)),
            spark: meshes.add(Sphere::new(0.08)),
            skin: materials.add(matte(Color::srgb(0.35, 0.42, 0.22))),
            skin_dim: materials.add(matte(Color::srgb(0.16, 0.19, 0.11))),
            skin_pain: materials.add(emissive(Color::srgb(0.85, 0.15, 0.1), 1.5)),
            skin_hit: materials.add(emissive(Color::srgb(1.0, 0.85, 0.7), 2.5)),
            visor_on: materials.add(emissive(Color::srgb(1.0, 0.1, 0.05), 4.0)),
            visor_off: materials.add(matte(Color::srgb(0.1, 0.05, 0.05))),
            gun_mat: materials.add(matte(Color::srgb(0.2, 0.2, 0.22))),
            tip_idle: materials.add(matte(Color::srgb(0.3, 0.12, 0.05))),
            tip_glow: materials.add(emissive(Color::srgb(1.0, 0.6, 0.15), 8.0)),
            bolt_mat: materials.add(emissive(BOLT_COLOR, 6.0)),
            spark_mat: materials.add(emissive(Color::srgb(1.0, 0.85, 0.4), 10.0)),
        }
    }
}

/// Grunt, bolt and spark visuals. Needs a renderer's assets (`Assets<Mesh>`,
/// `Assets<StandardMaterial>`) and `CombatSimPlugin`; not part of the headless sim.
pub struct ActorVisualsPlugin;

impl Plugin for ActorVisualsPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(SpawnLevel, spawn_grunts.after(spawn_combat))
            .add_systems(
                Update,
                (
                    (flash_tips, flash_hits, spawn_sparks).in_set(FxReaders),
                    pose_grunts.after(flash_tips).after(flash_hits),
                    sync_bolts,
                    age_sparks,
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

/// One Grunt visual per actor, in [`SpawnLevel`] after combat has spawned.
fn spawn_grunts(
    mut commands: Commands,
    existing: Option<Res<ActorAssets>>,
    combat: Res<LevelCombat>,
    defs: Res<GameDefs>,
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
        let scale = defs.0.enemy(actor.kind).height / MODEL_HEIGHT;
        let root = commands
            .spawn((
                GruntVisual(i),
                TipGlow::default(),
                HitFlash::default(),
                Transform::from_translation(to_bevy(actor.body.pos))
                    .with_rotation(Quat::from_rotation_y(core_angle_to_yaw(actor.angle)))
                    .with_scale(Vec3::splat(scale)),
                Visibility::default(),
                LevelEntity,
            ))
            .id();
        let part = |commands: &mut Commands, parent, mesh: &Handle<Mesh>, mat, at: Vec3| {
            commands
                .spawn((
                    Mesh3d(mesh.clone()),
                    MeshMaterial3d(mat),
                    Transform::from_translation(at),
                    ChildOf(parent),
                ))
                .id()
        };
        let pivot = |commands: &mut Commands, parent, at: Vec3| {
            commands
                .spawn((
                    Transform::from_translation(at),
                    Visibility::default(),
                    ChildOf(parent),
                ))
                .id()
        };
        // Model space: feet at the origin, y up, facing −Z (right hand on +X).
        let legs = part(&mut commands, root, &a.legs, a.skin.clone(), Vec3::Y * 0.4);
        let torso = part(
            &mut commands,
            root,
            &a.torso,
            a.skin.clone(),
            Vec3::Y * 1.11,
        );
        let left_arm = part(
            &mut commands,
            root,
            &a.upper_arm,
            a.skin.clone(),
            Vec3::new(-0.33, 1.1, 0.0),
        );
        let neck = pivot(&mut commands, root, Vec3::Y * 1.42);
        let head = part(&mut commands, neck, &a.head, a.skin.clone(), Vec3::Y * 0.14);
        let visor = part(
            &mut commands,
            neck,
            &a.visor,
            a.visor_on.clone(),
            Vec3::new(0.0, 0.16, -0.14),
        );
        let arm = pivot(&mut commands, root, Vec3::new(0.33, 1.32, 0.0));
        part(
            &mut commands,
            arm,
            &a.gun,
            a.gun_mat.clone(),
            Vec3::Z * -0.3,
        );
        let tip = part(
            &mut commands,
            arm,
            &a.tip,
            a.tip_idle.clone(),
            Vec3::Z * -0.62,
        );
        commands.entity(root).insert(GruntRig {
            skin: [legs, torso, left_arm, head],
            neck,
            visor,
            arm,
            tip,
        });
    }
}

/// Counts gun-tip glows down; `ActorFired` relights the firing Grunt's tip for
/// [`TIP_GLOW_SECS`]. An [`FxReaders`] system.
fn flash_tips(time: Res<Time>, fx: Res<FxQueue>, mut q: Query<(&GruntVisual, &mut TipGlow)>) {
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

/// Counts hit flashes down; `ActorHurt` relights the hit Grunt's flash for
/// [`HIT_FLASH_SECS`]. An [`FxReaders`] system.
fn flash_hits(time: Res<Time>, fx: Res<FxQueue>, mut q: Query<(&GruntVisual, &mut HitFlash)>) {
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

/// The body material. Priority: pain, then the hit flash, then the AI state's look. The flash
/// shows only on a living Grunt that is not in pain, so it never fights the pain material and a
/// Dying Grunt keeps its death look.
fn skin_look(state: SkinLook, alive: bool, hit_flash: f32) -> SkinLook {
    if state != SkinLook::Pain && alive && hit_flash > 0.0 {
        SkinLook::Hit
    } else {
        state
    }
}

/// The pose of one Grunt for its AI state.
struct Pose {
    /// Whole-body pitch about the feet, backwards (radians).
    pitch: f32,
    nod: f32,
    /// Gun arm pitch: 0 = level, negative = lowered.
    arm: f32,
    skin: SkinLook,
    visor_lit: bool,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum SkinLook {
    Normal,
    Dim,
    Pain,
    Hit,
}

fn pose(actor: &Actor, death_time: f32) -> Pose {
    let base = Pose {
        pitch: 0.0,
        nod: 0.0,
        arm: -0.6,
        skin: SkinLook::Normal,
        visor_lit: true,
    };
    match actor.state {
        AiState::Sleep => Pose {
            nod: SLEEP_NOD,
            arm: -1.3,
            skin: SkinLook::Dim,
            visor_lit: false,
            ..base
        },
        AiState::Alert { .. } | AiState::Chase => base,
        AiState::Attack { .. } => Pose { arm: 0.0, ..base },
        AiState::Pain { .. } => Pose {
            pitch: PAIN_TILT,
            arm: -0.3,
            skin: SkinLook::Pain,
            ..base
        },
        AiState::Dying { t } => Pose {
            pitch: death_pitch(t, death_time),
            arm: -1.3,
            visor_lit: false,
            ..base
        },
        AiState::Dead => Pose {
            pitch: FRAC_PI_2,
            arm: -1.3,
            visor_lit: false,
            ..base
        },
    }
}

fn set_material(
    mats: &mut Query<&mut MeshMaterial3d<StandardMaterial>>,
    e: Entity,
    want: &Handle<StandardMaterial>,
) {
    if let Ok(mut m) = mats.get_mut(e)
        && m.0 != *want
    {
        m.0 = want.clone();
    }
}

/// Places every Grunt between its last two ticks and poses it from its AI state.
fn pose_grunts(
    fixed: Res<Time<Fixed>>,
    combat: Res<LevelCombat>,
    defs: Res<GameDefs>,
    a: Res<ActorAssets>,
    mut roots: Query<(&GruntVisual, &GruntRig, &TipGlow, &HitFlash, &mut Transform)>,
    mut parts: Query<&mut Transform, Without<GruntVisual>>,
    mut mats: Query<&mut MeshMaterial3d<StandardMaterial>>,
) {
    let alpha = fixed.overstep_fraction();
    for (g, rig, glow, hit, mut t) in &mut roots {
        let Some(actor) = combat.0.actors.get(g.0) else {
            continue;
        };
        let def = defs.0.enemy(actor.kind);
        let p = pose(actor, def.death_time);
        let feet = actor.prev_pos.lerp(actor.body.pos, alpha);
        // Lying back, the torso's depth would sink into the floor: lift by the back's extent.
        let lift = BACK_HALF_DEPTH * t.scale.y * p.pitch.sin();
        t.translation = to_bevy(feet) + Vec3::Y * lift;
        t.rotation =
            Quat::from_rotation_y(core_angle_to_yaw(actor.angle)) * Quat::from_rotation_x(p.pitch);
        if let Ok(mut n) = parts.get_mut(rig.neck) {
            n.rotation = Quat::from_rotation_x(p.nod);
        }
        if let Ok(mut arm) = parts.get_mut(rig.arm) {
            arm.rotation = Quat::from_rotation_x(p.arm);
        }
        let skin = match skin_look(p.skin, actor.alive(), hit.0) {
            SkinLook::Normal => &a.skin,
            SkinLook::Dim => &a.skin_dim,
            SkinLook::Pain => &a.skin_pain,
            SkinLook::Hit => &a.skin_hit,
        };
        for e in rig.skin {
            set_material(&mut mats, e, skin);
        }
        let visor = if p.visor_lit {
            &a.visor_on
        } else {
            &a.visor_off
        };
        set_material(&mut mats, rig.visor, visor);
        let tip = if glow.0 > 0.0 && actor.alive() {
            &a.tip_glow
        } else {
            &a.tip_idle
        };
        set_material(&mut mats, rig.tip, tip);
    }
}

/// A bolt's transform between its last two ticks, the capsule's long axis along its flight.
fn bolt_transform(p: &Projectile, alpha: f32) -> Transform {
    let dir = to_bevy(p.vel).normalize_or_zero();
    let rot = if dir == Vec3::ZERO {
        Quat::IDENTITY
    } else {
        Quat::from_rotation_arc(Vec3::Y, dir)
    };
    Transform::from_translation(to_bevy(p.prev.lerp(p.pos, alpha))).with_rotation(rot)
}

/// Keeps one [`BoltVisual`] per live projectile: spawns missing ones, moves the rest and
/// despawns those whose projectile is gone (covers `ProjectileGone` without queue timing).
fn sync_bolts(
    mut commands: Commands,
    fixed: Res<Time<Fixed>>,
    combat: Res<LevelCombat>,
    a: Res<ActorAssets>,
    mut q: Query<(Entity, &BoltVisual, &mut Transform)>,
) {
    let alpha = fixed.overstep_fraction();
    let mut shown = BTreeSet::new();
    for (e, bolt, mut t) in &mut q {
        match combat.0.projectiles.iter().find(|p| p.id == bolt.0) {
            Some(p) => {
                *t = bolt_transform(p, alpha);
                shown.insert(bolt.0);
            }
            None => commands.entity(e).despawn(),
        }
    }
    for p in &combat.0.projectiles {
        if shown.contains(&p.id) {
            continue;
        }
        commands.spawn((
            Mesh3d(a.bolt.clone()),
            MeshMaterial3d(a.bolt_mat.clone()),
            bolt_transform(p, alpha),
            PointLight {
                color: BOLT_COLOR,
                intensity: 15_000.0,
                range: 2.5,
                shadow_maps_enabled: false,
                ..default()
            },
            NotShadowCaster,
            BoltVisual(p.id),
            LevelEntity,
        ));
    }
}

/// One short-lived spark per `Impact`. An [`FxReaders`] system.
fn spawn_sparks(mut commands: Commands, fx: Res<FxQueue>, a: Res<ActorAssets>) {
    for ev in &fx.combat {
        if let CombatEvent::Impact { point, normal } = ev {
            commands.spawn((
                Mesh3d(a.spark.clone()),
                MeshMaterial3d(a.spark_mat.clone()),
                Transform::from_translation(to_bevy(*point + *normal * 0.05)),
                NotShadowCaster,
                Spark(SPARK_SECS),
                LevelEntity,
            ));
        }
    }
}

/// Shrinks sparks over their lifetime and despawns them when it runs out.
fn age_sparks(
    mut commands: Commands,
    time: Res<Time>,
    mut q: Query<(Entity, &mut Spark, &mut Transform)>,
) {
    for (e, mut s, mut t) in &mut q {
        s.0 -= time.delta_secs();
        if s.0 <= 0.0 {
            commands.entity(e).despawn();
        } else {
            t.scale = Vec3::splat(s.0 / SPARK_SECS);
        }
    }
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
        use SkinLook::*;
        assert_eq!(skin_look(Normal, true, 0.05), Hit);
        assert_eq!(skin_look(Dim, true, 0.05), Hit);
        assert_eq!(skin_look(Normal, true, 0.0), Normal);
        assert_eq!(skin_look(Pain, true, 0.05), Pain, "pain wins");
        assert_eq!(skin_look(Normal, false, 0.05), Normal, "dying: no flash");
    }

    #[test]
    fn bolt_points_along_its_flight() {
        let p = Projectile {
            id: 1,
            pos: Vec3::new(1.0, 0.0, 1.0),
            prev: Vec3::new(0.0, 0.0, 1.0),
            vel: Vec3::X * 15.0,
            sector: 0,
            radius: 0.12,
            damage: 8,
            owner: rr_core::projectile::Shooter::Actor(0),
            targets: rr_core::projectile::Targets::Player,
            life: 1.0,
            gravity: 0.0,
            bounce: None,
            remote: false,
            splash: None,
            resting: false,
        };
        let t = bolt_transform(&p, 0.5);
        assert!(t.translation.distance(Vec3::new(0.5, 1.0, 0.0)) < 1e-6);
        assert!((t.rotation * Vec3::Y).distance(Vec3::X) < 1e-5);
    }
}
