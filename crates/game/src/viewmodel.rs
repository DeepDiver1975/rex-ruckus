//! First-person weapon models (boot, pistol, shotgun, chaingun, rocket launcher, pipe bomb), plus a kick leg, built from Bevy
//! primitives. Rendering only; nothing here touches the simulation.
//!
//! Layout. Spawned once (when the persistent [`PlayerCamera`] appears) as
//! `PlayerCamera -> ViewRig -> { ViewModel(weapon) x6, KickLeg }`. None of it is a
//! `LevelEntity`, so it survives restarts exactly like the camera. The rig carries all the
//! motion (bob, sway, recoil, lowering, reload tilt); every part is `NotShadowCaster`.
//!
//! Which model is visible: `PlayerArsenal.current`, except during a weapon switch, where the
//! old model is shown while it sinks (first half) and the new one (`to`) while it rises
//! (second half). The dip is `1 - |2f - 1|` for `f = left / switch_time`, so it is deepest at
//! the swap and the swap itself is hidden.
//!
//! Placement. The rig sits at camera-local [`RIG_BASE`], slightly closer in than the nominal
//! `(0.12, -0.13, -0.28)` so that every part centre stays within [`MAX_EYE_DIST`] (0.3 m) of
//! the eye. Models are small; the barrel tips may reach further. Whether that clips walls is a
//! playtest question (fallback: a second camera with `RenderLayers`).

use crate::combat::{FxQueue, FxReaders, GameDefs, PlayerArsenal};
use crate::flow::PlayState;
use crate::player::{Look, Player, PlayerBody, PlayerCamera, PlayerTuning};
use bevy::light::NotShadowCaster;
use bevy::prelude::*;
use rr_core::defs::{AmmoKind, WeaponId};
use rr_core::weapons::{WeaponEvent, WeaponPhase};
use std::f32::consts::{PI, TAU};

/// Rig origin in camera space (x right, y up, -z forward).
pub const RIG_BASE: Vec3 = Vec3::new(0.08, -0.09, -0.15);
/// Part centres stay within this distance of the eye (at rest).
pub const MAX_EYE_DIST: f32 = 0.3;
/// Seconds the muzzle flash stays visible after a shot.
pub const FLASH_SECS: f32 = 0.05;
/// Duration of the kick-leg thrust and how far it reaches forward (metres).
pub const KICK_SECS: f32 = 0.25;
pub const KICK_REACH: f32 = 0.12;
/// Recoil added per shot: backwards (+z, metres) and muzzle-up pitch (radians).
pub const RECOIL_Z: f32 = 0.05;
pub const RECOIL_PITCH_DEG: f32 = 6.0;
/// Exponential recoil recovery rate (1/s).
pub const RECOIL_RATE: f32 = 18.0;
const SWAY_RATE: f32 = 9.0;
const SWAY_GAIN: f32 = 0.05;
const SWAY_MAX: f32 = 0.04;
const BOB_RATE: f32 = 10.0;
const LOWER_DEPTH: f32 = 0.25;
const RELOAD_TILT: f32 = 0.6;
/// Chaingun barrel spin: top speed (rad/s), spin-up rate (1/s of the gap) and spin-down decay
/// (1/s). The cluster keeps spinning this long after the last shot.
pub const SPIN_MAX: f32 = 30.0;
const SPIN_UP: f32 = 12.0;
const SPIN_DOWN: f32 = 3.0;
pub const SPIN_HOLD_SECS: f32 = 0.15;
/// Seconds of the detonator press and how far the box dips (metres).
pub const PRESS_SECS: f32 = 0.2;
const PRESS_DEPTH: f32 = 0.025;
/// Where the chaingun's barrel cluster pivots, relative to its model root.
pub const CLUSTER_AT: Vec3 = Vec3::new(0.0, 0.01, -0.03);

/// Root of one weapon's model. Only the shown weapon's root is visible.
#[derive(Component)]
pub struct ViewModel(pub WeaponId);

/// Carries every viewmodel motion; child of the camera.
#[derive(Component)]
pub struct ViewRig;

/// The chaingun's rotating barrel cluster; spins about the view axis.
#[derive(Component)]
pub struct BarrelCluster;

/// The pipe bomb in the hand; hidden when the player has none left.
#[derive(Component)]
pub struct HeldBomb;

/// The detonator box, shown while bombs are live; dips when the player detonates.
#[derive(Component)]
pub struct Detonator;

/// The leg that thrusts out during a quick-kick.
#[derive(Component)]
pub struct KickLeg;

/// Emissive sphere plus small point light at a muzzle, shown briefly after a shot.
#[derive(Component)]
pub struct MuzzleFlash;

/// Gun kick: backwards displacement (metres) and muzzle-up pitch (radians).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Recoil {
    pub z: f32,
    pub pitch: f32,
}

/// Frame-loop animation state of the viewmodel. `restart_level` resets it in place.
#[derive(Resource, Default, Debug, PartialEq)]
pub struct ViewState {
    pub bob_phase: f32,
    pub sway: Vec2,
    pub last_look: Option<Vec2>,
    pub recoil: Recoil,
    /// Seconds of muzzle flash left.
    pub flash: f32,
    /// Seconds of kick thrust left.
    pub kick: f32,
    /// Chaingun cluster angle (radians) and angular speed (rad/s).
    pub spin: f32,
    pub spin_rate: f32,
    /// Seconds the chaingun still counts as firing (spin target stays at top speed).
    pub spin_hold: f32,
    /// Seconds of detonator press left.
    pub press: f32,
}

/// Sideways and vertical walking bob, camera-space metres. Exactly zero at `speed_frac == 0`,
/// bounded by 0.012 on each axis.
pub fn bob_offset(phase: f32, speed_frac: f32) -> Vec2 {
    let f = speed_frac.clamp(0.0, 1.0);
    Vec2::new(phase.sin() * 0.012 * f, (phase * 2.0).sin() * 0.008 * f)
}

/// Exponential recoil recovery: `x *= exp(-18 dt)`.
pub fn decay_recoil(r: Recoil, dt: f32) -> Recoil {
    let k = (-RECOIL_RATE * dt).exp();
    Recoil {
        z: r.z * k,
        pitch: r.pitch * k,
    }
}

/// Eased lag: the model trails the look delta (radians this frame) and settles back.
pub fn sway_step(sway: Vec2, d_yaw: f32, d_pitch: f32, dt: f32) -> Vec2 {
    let kicked = sway + Vec2::new(d_yaw, -d_pitch) * SWAY_GAIN;
    (kicked * (-SWAY_RATE * dt).exp()).clamp(Vec2::splat(-SWAY_MAX), Vec2::splat(SWAY_MAX))
}

/// One step of the chaingun spin: `(angle, rate)` after `dt`, accelerating while `firing`
/// and coasting down otherwise. The rate stays within `0..=SPIN_MAX`.
pub fn spin_step(angle: f32, rate: f32, firing: bool, dt: f32) -> (f32, f32) {
    let rate = if firing {
        rate + (SPIN_MAX - rate) * (1.0 - (-SPIN_UP * dt).exp())
    } else {
        rate * (-SPIN_DOWN * dt).exp()
    };
    let rate = rate.clamp(0.0, SPIN_MAX);
    ((angle + rate * dt) % TAU, rate)
}

/// Detonator dip (metres, backwards into the hand) with `left` seconds of press to go.
pub fn press_dip(left: f32) -> f32 {
    if left <= 0.0 {
        return 0.0;
    }
    ((1.0 - (left / PRESS_SECS).clamp(0.0, 1.0)) * PI).sin() * PRESS_DEPTH
}

/// How far a switch has dipped the model, 0 (raised) to 1 (fully lowered).
pub fn switch_dip(left: f32, total: f32) -> f32 {
    if total <= 0.0 {
        return 0.0;
    }
    let f = (left / total).clamp(0.0, 1.0);
    1.0 - (2.0 * f - 1.0).abs()
}

/// Reload tilt, 0..1: eases down and back up over the reload.
pub fn reload_tilt(left: f32, total: f32) -> f32 {
    if total <= 0.0 {
        return 0.0;
    }
    ((1.0 - (left / total).clamp(0.0, 1.0)) * PI).sin()
}

/// The weapon whose model is visible for the arsenal's current phase.
pub fn shown_weapon(current: WeaponId, phase: WeaponPhase, to_switch_time: f32) -> WeaponId {
    match phase {
        WeaponPhase::Switching { to, left }
            if to_switch_time > 0.0 && left / to_switch_time <= 0.5 =>
        {
            to
        }
        WeaponPhase::Switching { to, .. } if to_switch_time <= 0.0 => to,
        _ => current,
    }
}

#[derive(Clone, Copy)]
enum Shape {
    Cuboid(f32, f32, f32),
    /// Cylinder along the view axis: radius, length.
    Barrel(f32, f32),
}

#[derive(Clone, Copy)]
enum Mat {
    Metal,
    Grip,
    Boot,
    /// Dark emissive-free red for the detonator button.
    Red,
}

/// One model part: shape, centre relative to the rig, material.
pub struct Part {
    shape: Shape,
    pub at: Vec3,
    mat: Mat,
}

const fn part(shape: Shape, x: f32, y: f32, z: f32, mat: Mat) -> Part {
    Part {
        shape,
        at: Vec3::new(x, y, z),
        mat,
    }
}

const PISTOL: &[Part] = &[
    part(Shape::Cuboid(0.035, 0.04, 0.16), 0.0, 0.0, 0.0, Mat::Metal),
    part(
        Shape::Cuboid(0.032, 0.07, 0.04),
        0.0,
        -0.05,
        0.04,
        Mat::Grip,
    ),
];
const SHOTGUN: &[Part] = &[
    part(Shape::Barrel(0.015, 0.2), 0.0, 0.01, -0.03, Mat::Metal),
    part(
        Shape::Cuboid(0.04, 0.03, 0.07),
        0.0,
        -0.02,
        -0.05,
        Mat::Grip,
    ),
    part(Shape::Cuboid(0.04, 0.05, 0.12), 0.0, -0.03, 0.07, Mat::Grip),
];
const CHAINGUN: &[Part] = &[
    // Receiver, hub behind the cluster and a grip.
    part(Shape::Cuboid(0.05, 0.05, 0.1), 0.0, 0.0, 0.05, Mat::Metal),
    part(Shape::Barrel(0.012, 0.03), 0.0, 0.01, -0.045, Mat::Grip),
    part(
        Shape::Cuboid(0.035, 0.06, 0.04),
        0.0,
        -0.05,
        0.05,
        Mat::Grip,
    ),
];
/// Six barrels around the axis, relative to [`CLUSTER_AT`].
const CLUSTER: &[Part] = &[
    part(Shape::Barrel(0.007, 0.18), 0.0, 0.018, 0.0, Mat::Metal),
    part(Shape::Barrel(0.007, 0.18), 0.0156, 0.009, 0.0, Mat::Metal),
    part(Shape::Barrel(0.007, 0.18), 0.0156, -0.009, 0.0, Mat::Metal),
    part(Shape::Barrel(0.007, 0.18), 0.0, -0.018, 0.0, Mat::Metal),
    part(Shape::Barrel(0.007, 0.18), -0.0156, -0.009, 0.0, Mat::Metal),
    part(Shape::Barrel(0.007, 0.18), -0.0156, 0.009, 0.0, Mat::Metal),
];
const ROCKETS: &[Part] = &[
    // Tube, rear flare, forward sight, front sight post and a grip underneath.
    part(Shape::Barrel(0.03, 0.26), 0.0, 0.0, -0.02, Mat::Metal),
    part(Shape::Barrel(0.04, 0.03), 0.0, 0.0, 0.1, Mat::Metal),
    part(
        Shape::Cuboid(0.01, 0.03, 0.02),
        0.0,
        0.045,
        -0.06,
        Mat::Metal,
    ),
    part(
        Shape::Cuboid(0.006, 0.02, 0.006),
        0.0,
        0.04,
        -0.13,
        Mat::Metal,
    ),
    part(
        Shape::Cuboid(0.035, 0.06, 0.04),
        0.0,
        -0.05,
        0.03,
        Mat::Grip,
    ),
];
const PIPE_HAND: &[Part] = &[
    // Fist and wrist.
    part(Shape::Cuboid(0.06, 0.05, 0.06), 0.0, -0.03, 0.0, Mat::Boot),
    part(Shape::Cuboid(0.05, 0.05, 0.1), 0.0, -0.05, 0.07, Mat::Boot),
];
const HELD_BOMB: &[Part] = &[
    part(Shape::Barrel(0.022, 0.11), 0.0, 0.005, -0.04, Mat::Metal),
    part(Shape::Barrel(0.008, 0.02), 0.0, 0.005, -0.1, Mat::Grip),
];
const DETONATOR: &[Part] = &[
    part(
        Shape::Cuboid(0.04, 0.03, 0.05),
        -0.07,
        -0.05,
        0.0,
        Mat::Metal,
    ),
    part(
        Shape::Cuboid(0.015, 0.012, 0.015),
        -0.07,
        -0.032,
        0.0,
        Mat::Red,
    ),
];
const BOOT: &[Part] = &[
    part(
        Shape::Cuboid(0.07, 0.05, 0.15),
        0.0,
        -0.03,
        -0.03,
        Mat::Boot,
    ),
    part(Shape::Cuboid(0.06, 0.09, 0.06), 0.0, 0.03, 0.03, Mat::Boot),
];
const LEG: &[Part] = &[
    part(Shape::Cuboid(0.07, 0.05, 0.12), 0.0, -0.07, 0.0, Mat::Boot),
    part(Shape::Barrel(0.035, 0.22), 0.0, -0.05, 0.14, Mat::Boot),
];

/// Muzzle flash position relative to the rig, per firearm.
const fn muzzle(w: WeaponId) -> Option<Vec3> {
    match w {
        WeaponId::Pistol => Some(Vec3::new(0.0, 0.0, -0.09)),
        WeaponId::Shotgun | WeaponId::Chaingun | WeaponId::Rockets => {
            Some(Vec3::new(0.0, 0.01, -0.12))
        }
        WeaponId::PipeBombs => None,
        WeaponId::Boot => None,
    }
}

/// The parts of a weapon's model (the kick leg is separate: [`leg_parts`]).
pub fn weapon_parts(w: WeaponId) -> &'static [Part] {
    match w {
        WeaponId::Boot => BOOT,
        WeaponId::Pistol => PISTOL,
        WeaponId::Shotgun => SHOTGUN,
        WeaponId::Chaingun => CHAINGUN,
        WeaponId::Rockets => ROCKETS,
        WeaponId::PipeBombs => PIPE_HAND,
    }
}

/// Extra model parts that move on their own, as `(centre offset of the node, parts)`: the
/// spinning cluster, the held bomb and the detonator.
pub fn extra_parts() -> [(Vec3, &'static [Part]); 3] {
    [
        (CLUSTER_AT, CLUSTER),
        (Vec3::ZERO, HELD_BOMB),
        (Vec3::ZERO, DETONATOR),
    ]
}

pub fn leg_parts() -> &'static [Part] {
    LEG
}

/// Render plugin: needs `Assets<Mesh>`, `Assets<StandardMaterial>` and the sim plugins.
pub struct ViewModelPlugin;

impl Plugin for ViewModelPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<ViewState>().add_systems(
            Update,
            (
                spawn_viewmodel,
                read_fx.in_set(FxReaders),
                advance_state.after(read_fx),
                (pose_rig, pose_weapons, pose_extras, pose_leg).after(advance_state),
            ),
        );
    }
}

struct Looks {
    metal: Handle<StandardMaterial>,
    grip: Handle<StandardMaterial>,
    boot: Handle<StandardMaterial>,
    red: Handle<StandardMaterial>,
}

fn matte(c: Color) -> StandardMaterial {
    StandardMaterial {
        base_color: c,
        perceptual_roughness: 0.8,
        ..default()
    }
}

/// Builds the viewmodel under each newly added [`PlayerCamera`].
fn spawn_viewmodel(
    mut commands: Commands,
    cameras: Query<Entity, Added<PlayerCamera>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    for cam in &cameras {
        let looks = Looks {
            metal: materials.add(matte(Color::srgb(0.22, 0.23, 0.26))),
            grip: materials.add(matte(Color::srgb(0.32, 0.2, 0.1))),
            boot: materials.add(matte(Color::srgb(0.25, 0.17, 0.1))),
            red: materials.add(StandardMaterial {
                base_color: Color::srgb(0.8, 0.1, 0.08),
                emissive: Color::srgb(0.8, 0.1, 0.08).to_linear() * 2.0,
                ..default()
            }),
        };
        let flash_mesh = meshes.add(Sphere::new(0.02));
        let flash_mat = materials.add(StandardMaterial {
            base_color: Color::srgb(1.0, 0.8, 0.3),
            emissive: Color::srgb(1.0, 0.7, 0.2).to_linear() * 12.0,
            ..default()
        });
        let rig = commands
            .spawn((
                ViewRig,
                Transform::from_translation(RIG_BASE),
                Visibility::Hidden,
                ChildOf(cam),
            ))
            .id();
        let mut spawn_parts = |commands: &mut Commands, parent: Entity, parts: &[Part]| {
            for p in parts {
                let mesh = match p.shape {
                    Shape::Cuboid(x, y, z) => meshes.add(Cuboid::new(x, y, z)),
                    Shape::Barrel(r, len) => meshes.add(Cylinder::new(r, len)),
                };
                let mat = match p.mat {
                    Mat::Metal => looks.metal.clone(),
                    Mat::Grip => looks.grip.clone(),
                    Mat::Boot => looks.boot.clone(),
                    Mat::Red => looks.red.clone(),
                };
                let rot = match p.shape {
                    // Cylinders stand along Y; lay them along the view axis.
                    Shape::Barrel(..) => Quat::from_rotation_x(PI / 2.0),
                    Shape::Cuboid(..) => Quat::IDENTITY,
                };
                commands.spawn((
                    Mesh3d(mesh),
                    MeshMaterial3d(mat),
                    Transform::from_translation(p.at).with_rotation(rot),
                    NotShadowCaster,
                    ChildOf(parent),
                ));
            }
        };
        for w in WeaponId::ALL {
            let root = commands
                .spawn((
                    ViewModel(w),
                    Transform::default(),
                    Visibility::Hidden,
                    ChildOf(rig),
                ))
                .id();
            spawn_parts(&mut commands, root, weapon_parts(w));
            let node = |commands: &mut Commands, marker: Entity, at: Vec3| {
                commands.entity(marker).insert((
                    Transform::from_translation(at),
                    Visibility::Inherited,
                    ChildOf(root),
                ));
            };
            let [(cluster_at, cluster), (_, bomb), (_, detonator)] = extra_parts();
            match w {
                WeaponId::Chaingun => {
                    let n = commands.spawn(BarrelCluster).id();
                    node(&mut commands, n, cluster_at);
                    spawn_parts(&mut commands, n, cluster);
                }
                WeaponId::PipeBombs => {
                    let b = commands.spawn(HeldBomb).id();
                    node(&mut commands, b, Vec3::ZERO);
                    spawn_parts(&mut commands, b, bomb);
                    let d = commands.spawn(Detonator).id();
                    node(&mut commands, d, Vec3::ZERO);
                    spawn_parts(&mut commands, d, detonator);
                }
                _ => {}
            }
            if let Some(at) = muzzle(w) {
                commands.spawn((
                    MuzzleFlash,
                    Mesh3d(flash_mesh.clone()),
                    MeshMaterial3d(flash_mat.clone()),
                    PointLight {
                        color: Color::srgb(1.0, 0.7, 0.3),
                        intensity: 60_000.0,
                        range: 8.0,
                        shadow_maps_enabled: false,
                        ..default()
                    },
                    Transform::from_translation(at),
                    Visibility::Hidden,
                    NotShadowCaster,
                    ChildOf(root),
                ));
            }
        }
        let leg = commands
            .spawn((
                KickLeg,
                Transform::default(),
                Visibility::Hidden,
                ChildOf(rig),
            ))
            .id();
        spawn_parts(&mut commands, leg, leg_parts());
    }
}

/// Turns this frame's weapon events into recoil, flash and kick timers. An [`FxReaders`]
/// system: it only reads the queue.
fn read_fx(fx: Res<FxQueue>, mut state: ResMut<ViewState>) {
    for ev in &fx.weapon {
        match ev {
            // Launch (rockets, bombs) kicks like a shot; the chaingun also keeps spinning.
            WeaponEvent::Fire { weapon, .. } => {
                state.recoil.z += RECOIL_Z;
                state.recoil.pitch += RECOIL_PITCH_DEG.to_radians();
                state.flash = FLASH_SECS;
                if *weapon == WeaponId::Chaingun {
                    state.spin_hold = SPIN_HOLD_SECS;
                }
            }
            WeaponEvent::Launch { .. } => {
                state.recoil.z += RECOIL_Z;
                state.recoil.pitch += RECOIL_PITCH_DEG.to_radians();
                state.flash = FLASH_SECS;
            }
            WeaponEvent::Detonate => state.press = PRESS_SECS,
            WeaponEvent::Kick { .. } => state.kick = KICK_SECS,
            _ => {}
        }
    }
}

/// Advances bob, sway, recoil and the timers by the frame time.
fn advance_state(
    time: Res<Time>,
    tuning: Res<PlayerTuning>,
    mut state: ResMut<ViewState>,
    player: Single<(&PlayerBody, &Look), With<Player>>,
) {
    let dt = time.delta_secs();
    let (body, look) = player.into_inner();
    let speed = if body.0.on_ground {
        body.0.vel.truncate().length() / tuning.0.max_speed
    } else {
        0.0
    };
    state.bob_phase = (state.bob_phase + dt * BOB_RATE * speed.clamp(0.0, 1.0)) % TAU;
    let now = Vec2::new(look.angle, look.pitch);
    let d = state.last_look.map_or(Vec2::ZERO, |last| {
        let dy = (now.x - last.x + PI).rem_euclid(TAU) - PI;
        Vec2::new(dy, now.y - last.y)
    });
    state.last_look = Some(now);
    state.sway = sway_step(state.sway, d.x, d.y, dt);
    state.recoil = decay_recoil(state.recoil, dt);
    state.flash = (state.flash - dt).max(0.0);
    state.kick = (state.kick - dt).max(0.0);
    state.press = (state.press - dt).max(0.0);
    (state.spin, state.spin_rate) =
        spin_step(state.spin, state.spin_rate, state.spin_hold > 0.0, dt);
    state.spin_hold = (state.spin_hold - dt).max(0.0);
}

fn pose_rig(
    state: Res<ViewState>,
    play: Res<PlayState>,
    defs: Res<GameDefs>,
    tuning: Res<PlayerTuning>,
    player: Single<(&PlayerBody, &PlayerArsenal), With<Player>>,
    mut rig: Single<(&mut Transform, &mut Visibility), With<ViewRig>>,
) {
    let (body, arsenal) = player.into_inner();
    let speed = (body.0.vel.truncate().length() / tuning.0.max_speed).clamp(0.0, 1.0);
    let speed = if body.0.on_ground { speed } else { 0.0 };
    let bob = bob_offset(state.bob_phase, speed);
    let (dip, tilt) = match arsenal.0.phase {
        WeaponPhase::Switching { to, left } => {
            (switch_dip(left, defs.0.weapon(to).switch_time), 0.0)
        }
        WeaponPhase::Reloading(left) => (
            0.0,
            reload_tilt(left, defs.0.weapon(arsenal.0.current).reload),
        ),
        _ => (0.0, 0.0),
    };
    let (tf, vis) = &mut *rig;
    tf.translation = RIG_BASE
        + Vec3::new(
            bob.x + state.sway.x,
            bob.y + state.sway.y - dip * LOWER_DEPTH - tilt * 0.03,
            state.recoil.z,
        );
    tf.rotation = Quat::from_rotation_x(state.recoil.pitch - tilt * RELOAD_TILT);
    **vis = if *play == PlayState::Playing {
        Visibility::Visible
    } else {
        Visibility::Hidden
    };
}

fn pose_weapons(
    state: Res<ViewState>,
    defs: Res<GameDefs>,
    player: Single<&PlayerArsenal, With<Player>>,
    mut roots: Query<(&ViewModel, &mut Visibility), Without<MuzzleFlash>>,
    mut flashes: Query<&mut Visibility, With<MuzzleFlash>>,
) {
    let a = &player.0;
    let switch_time = match a.phase {
        WeaponPhase::Switching { to, .. } => defs.0.weapon(to).switch_time,
        _ => 0.0,
    };
    let shown = shown_weapon(a.current, a.phase, switch_time);
    for (m, mut v) in &mut roots {
        *v = if m.0 == shown {
            Visibility::Inherited
        } else {
            Visibility::Hidden
        };
    }
    for mut v in &mut flashes {
        *v = if state.flash > 0.0 {
            Visibility::Inherited
        } else {
            Visibility::Hidden
        };
    }
}

/// The chaingun cluster's spin, the held bomb's visibility and the detonator's box and press.
#[allow(clippy::type_complexity)]
fn pose_extras(
    state: Res<ViewState>,
    player: Single<&PlayerArsenal, With<Player>>,
    mut parts: Query<
        (
            &mut Transform,
            &mut Visibility,
            Option<&BarrelCluster>,
            Option<&HeldBomb>,
            Option<&Detonator>,
        ),
        Or<(With<BarrelCluster>, With<HeldBomb>, With<Detonator>)>,
    >,
) {
    let a = &player.0;
    // Out of bombs but some still live: the hand is empty and only the detonator shows.
    let has_bomb = a.reserve[AmmoKind::Bombs.index()] + a.clip[WeaponId::PipeBombs.index()] > 0;
    let shown = |on: bool| {
        if on {
            Visibility::Inherited
        } else {
            Visibility::Hidden
        }
    };
    for (mut tf, mut vis, cluster, bomb, det) in &mut parts {
        if cluster.is_some() {
            tf.rotation = Quat::from_rotation_z(state.spin);
        } else if bomb.is_some() {
            *vis = shown(has_bomb);
        } else if det.is_some() {
            tf.translation = Vec3::Z * press_dip(state.press);
            *vis = shown(a.live_bombs > 0);
        }
    }
}

fn pose_leg(
    state: Res<ViewState>,
    mut leg: Single<(&mut Transform, &mut Visibility), With<KickLeg>>,
) {
    let (tf, vis) = &mut *leg;
    if state.kick > 0.0 {
        let p = 1.0 - state.kick / KICK_SECS;
        tf.translation = Vec3::new(0.0, 0.0, -KICK_REACH * (p * PI).sin());
        **vis = Visibility::Inherited;
    } else {
        tf.translation = Vec3::ZERO;
        **vis = Visibility::Hidden;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bob_is_zero_at_rest_and_bounded() {
        for i in 0..200 {
            let phase = i as f32 * 0.37;
            assert_eq!(bob_offset(phase, 0.0), Vec2::ZERO);
            for f in [0.3, 1.0, 5.0] {
                let o = bob_offset(phase, f);
                assert!(o.x.abs() <= 0.012 && o.y.abs() <= 0.012, "{o:?}");
            }
        }
        assert_ne!(bob_offset(1.0, 1.0), Vec2::ZERO);
    }

    #[test]
    fn recoil_decays() {
        let r = Recoil {
            z: RECOIL_Z,
            pitch: RECOIL_PITCH_DEG.to_radians(),
        };
        let a = decay_recoil(r, 0.05);
        assert!(a.z < r.z && a.z > 0.0 && a.pitch < r.pitch && a.pitch > 0.0);
        assert_eq!(decay_recoil(r, 0.0), r);
        let b = decay_recoil(decay_recoil(r, 0.025), 0.025);
        assert!((a.z - b.z).abs() < 1e-6, "frame-rate independent");
        assert!(decay_recoil(r, 1.0).z < 1e-6);
    }

    #[test]
    fn parts_stay_near_the_eye() {
        let all = WeaponId::ALL
            .iter()
            .flat_map(|&w| weapon_parts(w))
            .chain(leg_parts());
        for p in all {
            let d = (RIG_BASE + p.at).length();
            assert!(
                d <= MAX_EYE_DIST,
                "part at {:?} is {d} m from the eye",
                p.at
            );
        }
        for (at, parts) in extra_parts() {
            for p in parts {
                let d = (RIG_BASE + at + p.at).length();
                assert!(d <= MAX_EYE_DIST, "extra part at {:?} is {d} m away", p.at);
            }
        }
        for w in WeaponId::ALL {
            if let Some(m) = muzzle(w) {
                assert!((RIG_BASE + m).length() <= MAX_EYE_DIST);
            }
        }
    }

    #[test]
    fn chaingun_spins_up_and_coasts_down() {
        let (mut angle, mut rate) = (0.0, 0.0);
        for _ in 0..120 {
            (angle, rate) = spin_step(angle, rate, true, 1.0 / 60.0);
            assert!((0.0..TAU).contains(&angle));
        }
        assert!(rate > SPIN_MAX * 0.95 && rate <= SPIN_MAX, "{rate}");
        let top = rate;
        (_, rate) = spin_step(angle, rate, false, 0.1);
        assert!(rate < top && rate > 0.0, "coasts, not stops");
        for _ in 0..600 {
            (_, rate) = spin_step(0.0, rate, false, 1.0 / 60.0);
        }
        assert!(rate < 0.01);
    }

    #[test]
    fn detonator_press_dips_and_returns() {
        assert_eq!(press_dip(0.0), 0.0);
        assert!(press_dip(PRESS_SECS * 0.5) > 0.0);
        assert!(press_dip(PRESS_SECS * 0.5) <= PRESS_DEPTH + 1e-6);
        assert!(press_dip(PRESS_SECS).abs() < 1e-6);
    }

    #[test]
    fn switch_and_reload_poses() {
        assert_eq!(switch_dip(0.4, 0.4), 0.0);
        assert!((switch_dip(0.2, 0.4) - 1.0).abs() < 1e-6);
        assert!(switch_dip(0.0, 0.4).abs() < 1e-6);
        assert_eq!(switch_dip(0.1, 0.0), 0.0);
        assert!(reload_tilt(1.0, 1.0).abs() < 1e-6);
        assert!((reload_tilt(0.5, 1.0) - 1.0).abs() < 1e-6);
        let sw = |left| WeaponPhase::Switching {
            to: WeaponId::Shotgun,
            left,
        };
        assert_eq!(
            shown_weapon(WeaponId::Pistol, sw(0.3), 0.4),
            WeaponId::Pistol
        );
        assert_eq!(
            shown_weapon(WeaponId::Pistol, sw(0.1), 0.4),
            WeaponId::Shotgun
        );
        assert_eq!(
            shown_weapon(WeaponId::Pistol, WeaponPhase::Ready, 0.0),
            WeaponId::Pistol
        );
    }
}
