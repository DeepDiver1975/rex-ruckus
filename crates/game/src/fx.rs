//! Explosion effects and screen shake. Purely presentational: reads `Explosion` events from the
//! [`FxQueue`] and never touches the simulation or the player body.

use crate::combat::{FxQueue, FxReaders, eye_of};
use crate::coords::to_bevy;
use crate::flow::LevelEntity;
use crate::mechanics::LevelMechanics;
use crate::player::{Player, PlayerBody, PlayerCamera, update_camera};
use bevy::light::NotShadowCaster;
use bevy::prelude::*;
use rr_core::combat::CombatEvent;

/// Seconds the fireball sphere takes to grow and fade.
pub const FIREBALL_SECS: f32 = 0.35;
/// Final fireball radius as a fraction of the blast radius.
pub const FIREBALL_SCALE: f32 = 0.6;
/// Seconds the flash light lives.
pub const FLASH_SECS: f32 = 0.2;
/// Trauma lost per second.
pub const TRAUMA_DECAY: f32 = 1.5;
/// Camera nudge at full trauma: metres and radians of roll.
const SHAKE_MOVE: f32 = 0.12;
const SHAKE_ROLL: f32 = 0.04;
const FLASH_INTENSITY: f32 = 4_000_000.0;

/// The expanding emissive sphere of one explosion.
#[derive(Component)]
pub struct ExplosionFx {
    age: f32,
    radius: f32,
    material: Handle<StandardMaterial>,
}

/// The short point light of one explosion; the value is its remaining seconds.
#[derive(Component)]
pub struct ExplosionLight(f32);

/// Camera shake: `trauma` in 0..=1, the nudge scales with its square. Never read by the sim.
#[derive(Resource, Default, Debug)]
pub struct ScreenShake {
    pub trauma: f32,
}

impl ScreenShake {
    /// Adds the trauma of a blast of `radius` at distance `d` from the player's eye.
    pub fn add_blast(&mut self, d: f32, radius: f32) {
        let gain = (1.0 - d / (radius * 3.0)).max(0.0);
        self.trauma = (self.trauma + gain).min(1.0);
    }

    /// A running quake keeps the shake at least this strong; never lowers it.
    pub fn hold(&mut self, level: f32) {
        self.trauma = self.trauma.max(level.clamp(0.0, 1.0));
    }

    pub fn decay(&mut self, dt: f32) {
        self.trauma = (self.trauma - TRAUMA_DECAY * dt).max(0.0);
    }

    /// Back to calm, for a level restart.
    pub fn reset(&mut self) {
        self.trauma = 0.0;
    }
}

/// Camera offset (metres in the camera's frame, roll in radians) for `trauma` at time `t`:
/// smooth pseudo-noise from sines, scaled by trauma squared.
pub fn shake_offset(trauma: f32, t: f32) -> (Vec3, f32) {
    let k = trauma * trauma;
    let n = |f: f32, p: f32| (t * f + p).sin();
    (
        Vec3::new(n(37.0, 0.3), n(41.0, 1.7), n(29.0, 2.9)) * SHAKE_MOVE * k,
        n(33.0, 4.1) * SHAKE_ROLL * k,
    )
}

/// Explosion visuals and screen shake. Needs a renderer's assets (`Assets<Mesh>`,
/// `Assets<StandardMaterial>`), `CombatSimPlugin` and the player.
pub struct FxPlugin;

impl Plugin for FxPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<ScreenShake>()
            .add_systems(Update, spawn_explosions.in_set(FxReaders))
            .add_systems(
                Update,
                (age_explosions, decay_shake).after(spawn_explosions),
            )
            .add_systems(Update, quake_shake.after(decay_shake))
            .add_systems(
                RunFixedMainLoop,
                apply_shake
                    .in_set(RunFixedMainLoopSystems::AfterFixedMainLoop)
                    .after(update_camera),
            );
    }
}

/// One fireball and flash light per `Explosion`, plus the shake it causes. An [`FxReaders`]
/// system.
fn spawn_explosions(
    mut commands: Commands,
    fx: Res<FxQueue>,
    mut shake: ResMut<ScreenShake>,
    player: Query<&PlayerBody, With<Player>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut sphere: Local<Option<Handle<Mesh>>>,
) {
    for ev in &fx.combat {
        let CombatEvent::Explosion { point, radius } = ev else {
            continue;
        };
        let mesh = sphere
            .get_or_insert_with(|| meshes.add(Sphere::new(1.0)))
            .clone();
        let material = materials.add(fireball_material(1.0));
        let at = to_bevy(*point);
        commands.spawn((
            Mesh3d(mesh),
            MeshMaterial3d(material.clone()),
            Transform::from_translation(at).with_scale(Vec3::ZERO),
            NotShadowCaster,
            ExplosionFx {
                age: 0.0,
                radius: *radius,
                material,
            },
            LevelEntity,
        ));
        commands.spawn((
            PointLight {
                color: Color::srgb(1.0, 0.65, 0.25),
                intensity: FLASH_INTENSITY,
                range: radius * 3.0,
                shadow_maps_enabled: false,
                ..default()
            },
            Transform::from_translation(at),
            ExplosionLight(FLASH_SECS),
            LevelEntity,
        ));
        for body in &player {
            shake.add_blast(eye_of(&body.0).distance(*point), *radius);
        }
    }
}

fn fireball_material(alpha: f32) -> StandardMaterial {
    let c = Color::srgba(1.0, 0.6, 0.15, alpha);
    StandardMaterial {
        base_color: c,
        emissive: c.to_linear() * 8.0,
        alpha_mode: AlphaMode::Blend,
        unlit: true,
        ..default()
    }
}

/// Fireball scale (as a fraction of its final radius) and alpha at `age` seconds: grows fast,
/// then eases out while it fades.
fn fireball_look(age: f32) -> (f32, f32) {
    let u = (age / FIREBALL_SECS).clamp(0.0, 1.0);
    (1.0 - (1.0 - u) * (1.0 - u), 1.0 - u)
}

/// Grows and fades fireballs, dims flash lights, and despawns both when their time is up.
fn age_explosions(
    mut commands: Commands,
    time: Res<Time>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut balls: Query<(Entity, &mut ExplosionFx, &mut Transform)>,
    mut lights: Query<(Entity, &mut ExplosionLight, &mut PointLight), Without<ExplosionFx>>,
) {
    let dt = time.delta_secs();
    for (e, mut ball, mut t) in &mut balls {
        ball.age += dt;
        if ball.age >= FIREBALL_SECS {
            commands.entity(e).despawn();
            continue;
        }
        let (scale, alpha) = fireball_look(ball.age);
        t.scale = Vec3::splat(ball.radius * FIREBALL_SCALE * scale);
        if let Some(mut m) = materials.get_mut(&ball.material) {
            *m = fireball_material(alpha);
        }
    }
    for (e, mut life, mut light) in &mut lights {
        life.0 -= dt;
        if life.0 <= 0.0 {
            commands.entity(e).despawn();
        } else {
            light.intensity = FLASH_INTENSITY * life.0 / FLASH_SECS;
        }
    }
}

fn decay_shake(time: Res<Time>, mut shake: ResMut<ScreenShake>) {
    shake.decay(time.delta_secs());
}

/// Keeps the screen shaking for as long as a quake runs.
fn quake_shake(mech: Option<Res<LevelMechanics>>, mut shake: ResMut<ScreenShake>) {
    if let Some(m) = mech {
        shake.hold(m.0.quake_strength());
    }
}

/// Nudges the camera by the shake, on top of the transform `update_camera` just set (which
/// is rebuilt from the sim every frame, so the offset never accumulates or reaches the player).
fn apply_shake(
    time: Res<Time>,
    shake: Res<ScreenShake>,
    mut cams: Query<&mut Transform, With<PlayerCamera>>,
) {
    if shake.trauma <= 0.0 {
        return;
    }
    let (off, roll) = shake_offset(shake.trauma, time.elapsed_secs());
    for mut t in &mut cams {
        let rot = t.rotation;
        t.translation += rot * off;
        t.rotation *= Quat::from_rotation_z(roll);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hold_keeps_trauma_at_least_the_quake() {
        let mut s = ScreenShake::default();
        s.hold(0.6);
        assert_eq!(s.trauma, 0.6);
        s.trauma = 0.9;
        s.hold(0.6);
        assert_eq!(s.trauma, 0.9, "never lowers it");
    }

    #[test]
    fn trauma_scales_with_distance_and_clamps() {
        let mut s = ScreenShake::default();
        s.add_blast(0.0, 4.0);
        assert_eq!(s.trauma, 1.0);
        s.reset();
        s.add_blast(6.0, 4.0);
        assert!((s.trauma - 0.5).abs() < 1e-6);
        s.reset();
        s.add_blast(12.0, 4.0);
        assert_eq!(s.trauma, 0.0, "beyond 3 radii: nothing");
        s.add_blast(100.0, 4.0);
        assert_eq!(s.trauma, 0.0, "never negative");
        s.trauma = 0.3;
        s.decay(0.1);
        assert!((s.trauma - 0.15).abs() < 1e-6);
        s.decay(1.0);
        assert_eq!(s.trauma, 0.0);
    }

    #[test]
    fn shake_scales_with_trauma_squared() {
        let (a, ra) = shake_offset(0.5, 1.0);
        let (b, rb) = shake_offset(1.0, 1.0);
        assert!((b - a * 4.0).length() < 1e-5);
        assert!((rb - ra * 4.0).abs() < 1e-6);
        assert_eq!(shake_offset(0.0, 1.0), (Vec3::ZERO, 0.0));
    }

    #[test]
    fn fireball_grows_and_fades() {
        assert_eq!(fireball_look(0.0), (0.0, 1.0));
        let (s, a) = fireball_look(FIREBALL_SECS);
        assert!((s - 1.0).abs() < 1e-6 && a.abs() < 1e-6);
    }
}
