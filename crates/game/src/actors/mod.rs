//! Enemy, barrel, projectile and impact visuals: procedural stand-ins built from Bevy primitives.
//!
//! Purely presentational: every system here only reads the simulation ([`LevelCombat`],
//! [`FxQueue`]) and writes transforms, materials and visual-only entities. One module per rig
//! ([`grunt`], [`enforcer`], [`slasher`], [`drone`], [`barrel`]); [`projectiles`] holds the bolt,
//! rocket, bomb and spark visuals. Every actor gets an [`ActorVisual`] root plus the marker of
//! its kind; [`spawn_actors`] dispatches on [`ActorKind`].

mod barrel;
mod drone;
mod enforcer;
mod grunt;
mod projectiles;
mod slasher;

pub use barrel::BarrelVisual;
pub use drone::DroneVisual;
pub use enforcer::EnforcerVisual;
pub use grunt::GruntVisual;
pub use projectiles::{
    BoltVisual, BombVisual, ProjKind, RocketVisual, SPARK_SECS, Spark, proj_kind,
};
pub use slasher::SlasherVisual;

use crate::combat::{FxQueue, FxReaders, GameDefs, LevelCombat, spawn_combat};
use crate::coords::{core_angle_to_yaw, to_bevy};
use crate::flow::{LevelEntity, SpawnLevel};
use bevy::prelude::*;
use rr_core::actors::{Actor, AiState};
use rr_core::combat::CombatEvent;
use rr_core::map::ActorKind;
use std::f32::consts::FRAC_PI_2;

/// Seconds the gun tip glows after a shot.
pub const TIP_GLOW_SECS: f32 = 0.12;
/// Seconds the body flashes after a hit that did not kill.
pub const HIT_FLASH_SECS: f32 = 0.1;
/// Backward tilt while in pain (radians).
const PAIN_TILT: f32 = 0.25;
/// Head nod while asleep (radians, negative = chin down).
const SLEEP_NOD: f32 = -0.5;

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

/// Body materials for each look.
#[derive(Clone)]
struct Skin {
    normal: Handle<StandardMaterial>,
    dim: Handle<StandardMaterial>,
    pain: Handle<StandardMaterial>,
    hit: Handle<StandardMaterial>,
}

impl Skin {
    /// A skin of base colour `c` (linear-ish sRGB triple); asleep it is darker.
    fn new(materials: &mut Assets<StandardMaterial>, c: [f32; 3]) -> Self {
        let dim = c.map(|v| v * 0.45);
        Skin {
            normal: materials.add(matte(Color::srgb(c[0], c[1], c[2]))),
            dim: materials.add(matte(Color::srgb(dim[0], dim[1], dim[2]))),
            pain: materials.add(emissive(Color::srgb(0.85, 0.15, 0.1), 1.5)),
            hit: materials.add(emissive(Color::srgb(1.0, 0.85, 0.7), 2.5)),
        }
    }

    fn pick(&self, look: SkinLook) -> &Handle<StandardMaterial> {
        match look {
            SkinLook::Normal => &self.normal,
            SkinLook::Dim => &self.dim,
            SkinLook::Pain => &self.pain,
            SkinLook::Hit => &self.hit,
        }
    }
}

/// Meshes and materials shared by every actor, bolt and spark; survives restarts.
#[derive(Resource, Clone)]
struct ActorAssets {
    grunt: grunt::Assets,
    enforcer: enforcer::Assets,
    slasher: slasher::Assets,
    drone: drone::Assets,
    barrel: barrel::Assets,
    proj: projectiles::ProjAssets,
}

impl ActorAssets {
    fn new(meshes: &mut Assets<Mesh>, materials: &mut Assets<StandardMaterial>) -> Self {
        ActorAssets {
            grunt: grunt::Assets::new(meshes, materials),
            enforcer: enforcer::Assets::new(meshes, materials),
            slasher: slasher::Assets::new(meshes, materials),
            drone: drone::Assets::new(meshes, materials),
            barrel: barrel::Assets::new(meshes, materials),
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

/// Spawns a mesh child of `parent` at `at`.
fn part(
    commands: &mut Commands,
    parent: Entity,
    mesh: &Handle<Mesh>,
    mat: &Handle<StandardMaterial>,
    at: Vec3,
) -> Entity {
    commands
        .spawn((
            Mesh3d(mesh.clone()),
            MeshMaterial3d(mat.clone()),
            Transform::from_translation(at),
            ChildOf(parent),
        ))
        .id()
}

/// Spawns an invisible pivot child of `parent` at `at`, to be rotated by the pose.
fn pivot(commands: &mut Commands, parent: Entity, at: Vec3) -> Entity {
    commands
        .spawn((
            Transform::from_translation(at),
            Visibility::default(),
            ChildOf(parent),
        ))
        .id()
}

/// Grunt/Enforcer/Slasher/Drone/Barrel visuals and projectiles. Needs a renderer's assets
/// (`Assets<Mesh>`, `Assets<StandardMaterial>`) and `CombatSimPlugin`; not part of the
/// headless sim.
pub struct ActorVisualsPlugin;

impl Plugin for ActorVisualsPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(SpawnLevel, spawn_actors.after(spawn_combat))
            .add_systems(
                Update,
                (
                    (flash_tips, flash_hits, projectiles::spawn_sparks).in_set(FxReaders),
                    (
                        pose_gunners,
                        slasher::pose_slashers,
                        drone::pose_drones,
                        barrel::pose_barrels,
                    )
                        .after(flash_tips)
                        .after(flash_hits),
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

/// One visual per actor, in [`SpawnLevel`] after combat has spawned, built by the rig of its kind.
fn spawn_actors(
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
        let model_height = match actor.kind {
            ActorKind::Grunt => grunt::MODEL_HEIGHT,
            ActorKind::Enforcer => enforcer::MODEL_HEIGHT,
            ActorKind::Slasher => slasher::MODEL_HEIGHT,
            ActorKind::Drone => drone::MODEL_HEIGHT,
            ActorKind::Barrel => barrel::MODEL_HEIGHT,
        };
        let scale = defs.0.enemy(actor.kind).height / model_height;
        let root = commands
            .spawn((
                ActorVisual(i),
                TipGlow::default(),
                HitFlash::default(),
                Transform::from_translation(to_bevy(actor.body.pos))
                    .with_rotation(Quat::from_rotation_y(core_angle_to_yaw(actor.angle)))
                    .with_scale(Vec3::splat(scale)),
                Visibility::default(),
                LevelEntity,
            ))
            .id();
        match actor.kind {
            ActorKind::Grunt => grunt::spawn(&mut commands, root, i, &a.grunt),
            ActorKind::Enforcer => enforcer::spawn(&mut commands, root, i, &a.enforcer),
            ActorKind::Slasher => slasher::spawn(&mut commands, root, i, &a.slasher),
            ActorKind::Drone => drone::spawn(&mut commands, root, i, &a.drone),
            ActorKind::Barrel => barrel::spawn(&mut commands, root, i, &a.barrel),
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

/// The body material. Priority: pain, then the hit flash, then the AI state's look. The flash
/// shows only on a living actor that is not in pain, so it never fights the pain material and a
/// Dying actor keeps its death look.
fn skin_look(state: SkinLook, alive: bool, hit_flash: f32) -> SkinLook {
    if state != SkinLook::Pain && alive && hit_flash > 0.0 {
        SkinLook::Hit
    } else {
        state
    }
}

/// The pose of one walking gunner for its AI state.
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

/// Places a walker's root between its last two ticks, pitched back by `pitch`. Lying back, the
/// body's depth would sink into the floor, so the root lifts by the back's extent.
fn place_walker(t: &mut Transform, actor: &Actor, alpha: f32, pitch: f32, back_half_depth: f32) {
    let feet = actor.prev_pos.lerp(actor.body.pos, alpha);
    let lift = back_half_depth * t.scale.y * pitch.sin();
    t.translation = to_bevy(feet) + Vec3::Y * lift;
    t.rotation =
        Quat::from_rotation_y(core_angle_to_yaw(actor.angle)) * Quat::from_rotation_x(pitch);
}

/// The animated parts of a Grunt or Enforcer (children of its [`ActorVisual`] root).
#[derive(Component)]
struct GunnerRig {
    /// Parts that take the body material (dimmed asleep, red in pain).
    skin: Vec<Entity>,
    neck: Entity,
    visor: Entity,
    arm: Entity,
    tip: Entity,
    /// Half the torso depth: how far the back sticks out behind the pivot when lying flat.
    back_half_depth: f32,
    mats: GunnerMats,
}

/// The materials a [`GunnerRig`] swaps between.
#[derive(Clone)]
struct GunnerMats {
    skin: Skin,
    visor_on: Handle<StandardMaterial>,
    visor_off: Handle<StandardMaterial>,
    tip_idle: Handle<StandardMaterial>,
    tip_glow: Handle<StandardMaterial>,
}

/// Places every Grunt and Enforcer between its last two ticks and poses it from its AI state.
fn pose_gunners(
    fixed: Res<Time<Fixed>>,
    combat: Res<LevelCombat>,
    defs: Res<GameDefs>,
    mut roots: Query<(
        &ActorVisual,
        &GunnerRig,
        &TipGlow,
        &HitFlash,
        &mut Transform,
    )>,
    mut parts: Query<&mut Transform, Without<ActorVisual>>,
    mut mats: Query<&mut MeshMaterial3d<StandardMaterial>>,
) {
    let alpha = fixed.overstep_fraction();
    for (g, rig, glow, hit, mut t) in &mut roots {
        let Some(actor) = combat.0.actors.get(g.0) else {
            continue;
        };
        let def = defs.0.enemy(actor.kind);
        let p = pose(actor, def.death_time);
        place_walker(&mut t, actor, alpha, p.pitch, rig.back_half_depth);
        if let Ok(mut n) = parts.get_mut(rig.neck) {
            n.rotation = Quat::from_rotation_x(p.nod);
        }
        if let Ok(mut arm) = parts.get_mut(rig.arm) {
            arm.rotation = Quat::from_rotation_x(p.arm);
        }
        let skin = rig.mats.skin.pick(skin_look(p.skin, actor.alive(), hit.0));
        for e in &rig.skin {
            set_material(&mut mats, *e, skin);
        }
        let visor = if p.visor_lit {
            &rig.mats.visor_on
        } else {
            &rig.mats.visor_off
        };
        set_material(&mut mats, rig.visor, visor);
        let tip = if glow.0 > 0.0 && actor.alive() {
            &rig.mats.tip_glow
        } else {
            &rig.mats.tip_idle
        };
        set_material(&mut mats, rig.tip, tip);
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
}
