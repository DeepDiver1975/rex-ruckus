//! Breakable things in the world: glass shards, light fixtures that go dark, and the secret
//! message. Purely presentational: reads core events from the [`FxQueue`] (the re-meshing of a
//! broken pane is `GlassBroken.dirty` -> `DirtySectors` -> `rebuild_dirty_sectors`).

use crate::actors::{SPARK_SECS, Spark};
use crate::combat::{FxQueue, FxReaders};
use crate::coords::to_bevy;
use crate::flow::LevelEntity;
use crate::level::{CurrentMap, LevelLight};
use crate::mechanics::HudMessage;
use bevy::light::NotShadowCaster;
use bevy::prelude::*;
use rr_core::combat::CombatEvent;
use rr_core::map::Map;

/// Shards flung by one broken pane.
pub const SHARD_COUNT: usize = 12;
/// Seconds a shard lives (it shrinks to nothing over that time).
pub const SHARD_SECS: f32 = 1.0;
const SHARD_GRAVITY: f32 = 9.8;
/// Sparks thrown by a broken fixture.
const FIXTURE_SPARKS: usize = 6;

/// A flying glass shard: velocity (Bevy axes, m/s) and remaining seconds.
#[derive(Component)]
pub struct Shard {
    vel: Vec3,
    life: f32,
}

/// The visible fixture of `Map::lights[.0]`, drawn only for breakable lights.
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
pub struct LightFixture(pub usize);

/// Materials of the light fixtures: glowing while intact, dark once broken.
#[derive(Resource, Clone)]
pub struct FixtureMaterials {
    pub lit: Handle<StandardMaterial>,
    pub dark: Handle<StandardMaterial>,
    pub mesh: Handle<Mesh>,
}

/// Centre of the opening of glass wall `w`, in core coordinates.
pub fn pane_centre(map: &Map, w: usize) -> Vec3 {
    let wall = &map.walls[w];
    let mid = (wall.a + wall.b) * 0.5;
    let near = &map.sectors[wall.sector];
    let (lo, hi) = match wall.next_sector.map(|n| &map.sectors[n]) {
        Some(far) => (near.floor_z.max(far.floor_z), near.ceil_z.min(far.ceil_z)),
        None => (near.floor_z, near.ceil_z),
    };
    Vec3::new(mid.x, mid.y, (lo + hi) * 0.5)
}

/// Deterministic pseudo-random in -1..1 from an integer.
fn jitter(i: usize, salt: u32) -> f32 {
    let mut h = (i as u32).wrapping_mul(2_654_435_761) ^ salt.wrapping_mul(40_503);
    h = (h ^ (h >> 15)).wrapping_mul(2_246_822_519);
    ((h ^ (h >> 13)) & 0xffff) as f32 / 32_768.0 - 1.0
}

/// Initial velocity of shard `i`: flung both ways along the pane's normal, with spread.
pub fn shard_velocity(i: usize, normal: Vec3) -> Vec3 {
    let side = if i.is_multiple_of(2) { 1.0 } else { -1.0 };
    let n = normal * side * (1.5 + 2.0 * jitter(i, 1).abs());
    let tangent = Vec3::new(-normal.z, 0.0, normal.x);
    n + tangent * jitter(i, 2) * 2.0 + Vec3::Y * (1.0 + 2.0 * jitter(i, 3).abs())
}

pub struct BreakablesPlugin;

impl Plugin for BreakablesPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(
            Update,
            (spawn_shards, break_lights, secret_message).in_set(FxReaders),
        )
        .add_systems(Update, age_shards.after(spawn_shards));
    }
}

/// A burst of shard cuboids at the pane's centre per `GlassBroken`.
fn spawn_shards(
    mut commands: Commands,
    fx: Res<FxQueue>,
    map: Res<CurrentMap>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut assets: Local<Option<(Handle<Mesh>, Handle<StandardMaterial>)>>,
) {
    for ev in &fx.combat {
        let CombatEvent::GlassBroken { wall, .. } = ev else {
            continue;
        };
        let (mesh, mat) = assets
            .get_or_insert_with(|| {
                (
                    meshes.add(Cuboid::new(0.12, 0.12, 0.04)),
                    materials.add(StandardMaterial {
                        base_color: Color::srgba(0.7, 0.95, 1.0, 0.8),
                        alpha_mode: AlphaMode::Blend,
                        perceptual_roughness: 0.1,
                        ..default()
                    }),
                )
            })
            .clone();
        let w = &map.0.walls[*wall];
        let along = (w.b - w.a).normalize_or_zero();
        // Core normal of the pane (horizontal, perpendicular to the wall) in Bevy axes.
        let normal = to_bevy(Vec3::new(-along.y, along.x, 0.0));
        let at = to_bevy(pane_centre(&map.0, *wall));
        for i in 0..SHARD_COUNT {
            let spin = Quat::from_euler(
                EulerRot::XYZ,
                jitter(i, 4) * 3.0,
                jitter(i, 5) * 3.0,
                jitter(i, 6) * 3.0,
            );
            commands.spawn((
                Mesh3d(mesh.clone()),
                MeshMaterial3d(mat.clone()),
                Transform::from_translation(at).with_rotation(spin),
                NotShadowCaster,
                Shard {
                    vel: shard_velocity(i, normal),
                    life: SHARD_SECS,
                },
                LevelEntity,
            ));
        }
    }
}

/// Moves shards under gravity, shrinks them, and despawns them when their time is up.
fn age_shards(
    mut commands: Commands,
    time: Res<Time>,
    mut q: Query<(Entity, &mut Shard, &mut Transform)>,
) {
    let dt = time.delta_secs();
    for (e, mut s, mut t) in &mut q {
        s.life -= dt;
        if s.life <= 0.0 {
            commands.entity(e).despawn();
            continue;
        }
        s.vel.y -= SHARD_GRAVITY * dt;
        t.translation += s.vel * dt;
        t.scale = Vec3::splat(s.life / SHARD_SECS);
    }
}

/// Puts out the point light of a broken fixture, darkens the fixture and throws sparks.
#[allow(clippy::too_many_arguments)]
fn break_lights(
    mut commands: Commands,
    fx: Res<FxQueue>,
    map: Res<CurrentMap>,
    fixtures: Option<Res<FixtureMaterials>>,
    lights: Query<(Entity, &LevelLight)>,
    fixture_q: Query<(Entity, &LightFixture)>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut spark: Local<Option<(Handle<Mesh>, Handle<StandardMaterial>)>>,
) {
    for ev in &fx.combat {
        let CombatEvent::LightBroken(i) = ev else {
            continue;
        };
        for (e, l) in &lights {
            if l.0 == *i {
                commands.entity(e).despawn();
            }
        }
        if let Some(f) = &fixtures {
            for (e, fx) in &fixture_q {
                if fx.0 == *i {
                    commands.entity(e).insert(MeshMaterial3d(f.dark.clone()));
                }
            }
        }
        let Some(l) = map.0.lights.get(*i) else {
            continue;
        };
        let (mesh, mat) = spark
            .get_or_insert_with(|| {
                (
                    meshes.add(Sphere::new(0.05)),
                    materials.add(StandardMaterial {
                        base_color: Color::srgb(1.0, 0.85, 0.4),
                        emissive: LinearRgba::new(10.0, 8.0, 3.0, 1.0),
                        unlit: true,
                        ..default()
                    }),
                )
            })
            .clone();
        let at = to_bevy(Vec3::new(l.pos.0, l.pos.1, l.pos.2));
        for k in 0..FIXTURE_SPARKS {
            let off = Vec3::new(jitter(k, 7), jitter(k, 8), jitter(k, 9)) * 0.15;
            commands.spawn((
                Mesh3d(mesh.clone()),
                MeshMaterial3d(mat.clone()),
                Transform::from_translation(at + off),
                NotShadowCaster,
                Spark(SPARK_SECS * 2.0),
                LevelEntity,
            ));
        }
    }
}

/// "Secret found!" on the HUD.
fn secret_message(fx: Res<FxQueue>, mut msg: ResMut<HudMessage>) {
    if fx
        .combat
        .iter()
        .any(|e| matches!(e, CombatEvent::SecretFound))
    {
        msg.show("Secret found!");
    }
}
