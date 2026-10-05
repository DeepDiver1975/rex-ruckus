//! Projectile and impact visuals: bolts, rockets, pipe bombs and sparks.

use super::{ActorAssets, emissive, matte};
use crate::combat::{FxQueue, LevelCombat};
use crate::coords::to_bevy;
use crate::flow::LevelEntity;
use bevy::light::NotShadowCaster;
use bevy::prelude::*;
use rr_core::combat::CombatEvent;
use rr_core::projectile::{Projectile, Shooter};
use std::collections::BTreeSet;
use std::f32::consts::PI;

/// Lifetime of an impact spark.
pub const SPARK_SECS: f32 = 0.15;

/// A flying bolt's visual; the id is a `Projectile::id`.
#[derive(Component)]
pub struct BoltVisual(pub u32);

/// A flying player rocket's visual (capsule plus emissive tail); the id is a `Projectile::id`.
#[derive(Component)]
pub struct RocketVisual(pub u32);

/// A pipe bomb's visual (a cylinder that spins with its velocity); the id is a `Projectile::id`.
#[derive(Component)]
pub struct BombVisual(pub u32);

/// Which visual a projectile gets.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ProjKind {
    /// Anything an actor fired.
    Bolt,
    /// Player projectile that flies straight and splashes.
    Rocket,
    /// Player projectile that bounces or is remote-fused.
    Bomb,
}

/// Picks the visual for a projectile.
pub fn proj_kind(p: &Projectile) -> ProjKind {
    match p.owner {
        Shooter::Actor(_) => ProjKind::Bolt,
        Shooter::Player if p.bounce.is_some() || p.remote => ProjKind::Bomb,
        Shooter::Player => ProjKind::Rocket,
    }
}

/// Common tag of every projectile visual, so one system syncs all kinds.
#[derive(Component)]
pub(super) struct ProjVisual {
    id: u32,
    kind: ProjKind,
}

/// Roll of a bomb: fixed axis, angle grows with the distance travelled.
#[derive(Component)]
pub(super) struct BombSpin {
    axis: Vec3,
    angle: f32,
}

/// Radians of spin per metre of flight.
const BOMB_SPIN_PER_M: f32 = 8.0;

/// An impact spark with its remaining lifetime in seconds.
#[derive(Component)]
pub struct Spark(pub f32);

/// Meshes and materials of bolts, rockets, bombs and sparks.
#[derive(Clone)]
pub(super) struct ProjAssets {
    bolt: Handle<Mesh>,
    rocket: Handle<Mesh>,
    rocket_tail: Handle<Mesh>,
    bomb: Handle<Mesh>,
    spark: Handle<Mesh>,
    bolt_mat: Handle<StandardMaterial>,
    rocket_mat: Handle<StandardMaterial>,
    tail_mat: Handle<StandardMaterial>,
    bomb_mat: Handle<StandardMaterial>,
    spark_mat: Handle<StandardMaterial>,
}

const BOLT_COLOR: Color = Color::srgb(1.0, 0.35, 0.1);

impl ProjAssets {
    pub(super) fn new(meshes: &mut Assets<Mesh>, materials: &mut Assets<StandardMaterial>) -> Self {
        ProjAssets {
            bolt: meshes.add(Capsule3d::new(0.06, 0.3)),
            rocket: meshes.add(Capsule3d::new(0.07, 0.35)),
            rocket_tail: meshes.add(Cone::new(0.06, 0.35)),
            bomb: meshes.add(Cylinder::new(0.07, 0.24)),
            spark: meshes.add(Sphere::new(0.08)),
            bolt_mat: materials.add(emissive(BOLT_COLOR, 6.0)),
            rocket_mat: materials.add(matte(Color::srgb(0.55, 0.57, 0.5))),
            tail_mat: materials.add(emissive(Color::srgb(1.0, 0.55, 0.1), 10.0)),
            bomb_mat: materials.add(matte(Color::srgb(0.3, 0.32, 0.28))),
            spark_mat: materials.add(emissive(Color::srgb(1.0, 0.85, 0.4), 10.0)),
        }
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

/// Rocket transform: like a bolt (long axis along the flight), tail trailing behind.
fn rocket_transform(p: &Projectile, alpha: f32) -> Transform {
    bolt_transform(p, alpha)
}

/// Keeps one visual per live projectile, of the kind [`proj_kind`] picks: spawns missing
/// ones, moves the rest and despawns those whose projectile is gone or changed kind (covers
/// `ProjectileGone` without queue timing). Bombs also roll with their speed until resting.
pub(super) fn sync_projectiles(
    mut commands: Commands,
    fixed: Res<Time<Fixed>>,
    time: Res<Time>,
    combat: Res<LevelCombat>,
    a: Res<ActorAssets>,
    mut q: Query<(Entity, &ProjVisual, &mut Transform, Option<&mut BombSpin>)>,
) {
    let a = &a.proj;
    let alpha = fixed.overstep_fraction();
    let mut shown = BTreeSet::new();
    for (e, vis, mut t, spin) in &mut q {
        let Some(p) = combat
            .0
            .projectiles
            .iter()
            .find(|p| p.id == vis.id && proj_kind(p) == vis.kind)
        else {
            commands.entity(e).despawn();
            continue;
        };
        shown.insert(vis.id);
        match (vis.kind, spin) {
            (ProjKind::Bomb, Some(mut spin)) => {
                if !p.resting {
                    spin.angle += p.vel.length() * time.delta_secs() * BOMB_SPIN_PER_M;
                }
                *t = Transform::from_translation(to_bevy(p.prev.lerp(p.pos, alpha)))
                    .with_rotation(Quat::from_axis_angle(spin.axis, spin.angle));
            }
            (ProjKind::Rocket, _) => *t = rocket_transform(p, alpha),
            _ => *t = bolt_transform(p, alpha),
        }
    }
    for p in &combat.0.projectiles {
        if shown.contains(&p.id) {
            continue;
        }
        let kind = proj_kind(p);
        let tag = ProjVisual { id: p.id, kind };
        match kind {
            ProjKind::Bolt => {
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
                    tag,
                    LevelEntity,
                ));
            }
            ProjKind::Rocket => {
                let root = commands
                    .spawn((
                        Mesh3d(a.rocket.clone()),
                        MeshMaterial3d(a.rocket_mat.clone()),
                        rocket_transform(p, alpha),
                        PointLight {
                            color: Color::srgb(1.0, 0.6, 0.2),
                            intensity: 40_000.0,
                            range: 5.0,
                            shadow_maps_enabled: false,
                            ..default()
                        },
                        NotShadowCaster,
                        RocketVisual(p.id),
                        tag,
                        LevelEntity,
                    ))
                    .id();
                // The cone's tip points +Y (forward); the flip turns the tip backwards, so the wide base
                // meets the rocket body and the glow tapers to a point trailing behind it.
                commands.spawn((
                    Mesh3d(a.rocket_tail.clone()),
                    MeshMaterial3d(a.tail_mat.clone()),
                    Transform::from_xyz(0.0, -0.3, 0.0).with_rotation(Quat::from_rotation_x(PI)),
                    NotShadowCaster,
                    ChildOf(root),
                ));
            }
            ProjKind::Bomb => {
                let dir = to_bevy(p.vel).normalize_or_zero();
                let axis = Vec3::Y.cross(dir).try_normalize().unwrap_or(Vec3::X);
                commands.spawn((
                    Mesh3d(a.bomb.clone()),
                    MeshMaterial3d(a.bomb_mat.clone()),
                    Transform::from_translation(to_bevy(p.prev.lerp(p.pos, alpha))),
                    NotShadowCaster,
                    BombVisual(p.id),
                    BombSpin { axis, angle: 0.0 },
                    tag,
                    LevelEntity,
                ));
            }
        }
    }
}

/// One short-lived spark per `Impact`. An [`FxReaders`] system.
pub(super) fn spawn_sparks(mut commands: Commands, fx: Res<FxQueue>, a: Res<ActorAssets>) {
    let a = &a.proj;
    for ev in &fx.combat {
        if let CombatEvent::Impact { point, normal, .. } = ev {
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
pub(super) fn age_sparks(
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
