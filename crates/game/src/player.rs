//! Player: core movement in a 60 Hz FixedUpdate, input gathered every frame,
//! and the camera interpolated between ticks.

use crate::coords::{core_angle_to_yaw, forward_2d, to_bevy};
use crate::level::CurrentMap;
use bevy::input::mouse::AccumulatedMouseMotion;
use bevy::pbr::{DistanceFog, FogFalloff};
use bevy::prelude::*;
use bevy::window::{CursorGrabMode, CursorOptions};
use rr_core::collide::Body;
use rr_core::map::KeySet;
use rr_core::movement::{MoveInput, Tuning, step_player};

pub const PLAYER_RADIUS: f32 = 0.35;
/// Eyes sit this far below the top of the body.
pub const EYE_BELOW_TOP: f32 = 0.15;
const PITCH_LIMIT: f32 = 1.45;
/// Rate (1/s) at which the eye eases to a new height after crouching or standing up.
pub const EYE_EASE_RATE: f32 = 14.0;

/// Frame-rate independent exponential approach of `current` towards `target`.
pub fn ease_toward(current: f32, target: f32, rate: f32, dt: f32) -> f32 {
    target + (current - target) * (-rate * dt).exp()
}

/// Smoothed eye height above the feet (core metres).
#[derive(Component)]
pub struct EyeHeight(pub f32);

/// Cursor grab decision: Some(true) grab, Some(false) release, None keep.
fn grab_change(focused: bool, clicked: bool, escape: bool, grabbed: bool) -> Option<bool> {
    if grabbed && (escape || !focused) {
        Some(false)
    } else if !grabbed && clicked && focused {
        Some(true)
    } else {
        None
    }
}

#[derive(Component)]
pub struct Player;

#[derive(Component)]
pub struct PlayerBody(pub Body);

/// Feet position at the previous tick, for render interpolation (core coords).
#[derive(Component, Default)]
pub struct PrevFeet(pub Vec3);

/// View direction: `angle` is a core heading (radians, 0 = east, CCW); `pitch` is up-positive.
#[derive(Component)]
pub struct Look {
    pub angle: f32,
    pub pitch: f32,
}

/// Latest movement intent from the frame loop; held keys are re-read every frame.
#[derive(Component, Default)]
pub struct PendingInput {
    pub forward: f32,
    pub strafe: f32,
    pub jump: bool,
    pub crouch: bool,
    /// Latched by a use-key press; the next fixed tick consumes it.
    pub use_pressed: bool,
}

/// Keycards the player holds.
#[derive(Component, Default)]
pub struct Inventory {
    pub keys: KeySet,
}

/// The fixed-tick player movement step; mechanics systems order themselves around it.
#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub struct PlayerSimSet;

#[derive(Component)]
pub struct PlayerCamera;

#[derive(Resource, Default)]
pub struct PlayerTuning(pub Tuning);

#[derive(Resource)]
pub struct MouseSensitivity(pub f32);

/// Simulation only: safe to run headless.
pub struct PlayerSimPlugin;

impl Plugin for PlayerSimPlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(Time::<Fixed>::from_hz(60.0))
            .init_resource::<PlayerTuning>()
            .add_systems(Startup, spawn_player)
            .add_systems(FixedUpdate, simulate_player.in_set(PlayerSimSet));
    }
}

fn spawn_player(mut commands: Commands, map: Res<CurrentMap>, tuning: Res<PlayerTuning>) {
    let start = map.0.player_start;
    let body = Body::spawn(
        &map.0,
        Vec2::new(start.pos.0, start.pos.1),
        PLAYER_RADIUS,
        tuning.0.stand_height,
    )
    .unwrap_or_else(|| {
        panic!(
            "level '{}': player_start {:?} is outside every sector",
            map.0.name, start.pos
        )
    });
    commands.spawn((
        Player,
        PrevFeet(body.pos),
        PlayerBody(body),
        Look {
            angle: start.angle_deg.to_radians(),
            pitch: 0.0,
        },
        PendingInput::default(),
        Inventory::default(),
        EyeHeight(tuning.0.stand_height - EYE_BELOW_TOP),
    ));
}

fn simulate_player(
    map: Res<CurrentMap>,
    tuning: Res<PlayerTuning>,
    time: Res<Time<Fixed>>,
    mut q: Query<(&mut PlayerBody, &mut PrevFeet, &PendingInput, &Look)>,
) {
    // `timestep()` (not `delta`) so the system also works when FixedUpdate is run by hand in tests.
    let dt = time.timestep().as_secs_f32();
    for (mut body, mut prev, input, look) in &mut q {
        prev.0 = body.0.pos;
        let forward = forward_2d(look.angle);
        let right = Vec2::new(forward.y, -forward.x);
        let wish = forward * input.forward + right * input.strafe;
        let move_input = MoveInput {
            wish,
            jump: input.jump,
            crouch: input.crouch,
        };
        step_player(&map.0, &mut body.0, &move_input, &tuning.0, dt);
    }
}

/// Keyboard/mouse, cursor grab and the first-person camera. Needs a window.
pub struct PlayerControlPlugin;

impl Plugin for PlayerControlPlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(MouseSensitivity(0.0025))
            .add_systems(Startup, spawn_camera)
            .add_systems(Update, grab_cursor)
            .add_systems(
                RunFixedMainLoop,
                (
                    read_input.in_set(RunFixedMainLoopSystems::BeforeFixedMainLoop),
                    update_camera.in_set(RunFixedMainLoopSystems::AfterFixedMainLoop),
                ),
            );
    }
}

fn spawn_camera(mut commands: Commands) {
    commands.spawn((
        Camera3d::default(),
        Projection::Perspective(PerspectiveProjection {
            fov: 75f32.to_radians(),
            ..default()
        }),
        DistanceFog {
            color: Color::srgb(0.16, 0.23, 0.47),
            falloff: FogFalloff::Linear {
                start: 25.0,
                end: 70.0,
            },
            ..default()
        },
        PlayerCamera,
    ));
}

fn grab_cursor(
    mut cursor: Single<&mut CursorOptions>,
    window: Single<&Window>,
    mouse: Res<ButtonInput<MouseButton>>,
    keys: Res<ButtonInput<KeyCode>>,
) {
    let grabbed = cursor.grab_mode != CursorGrabMode::None;
    match grab_change(
        window.focused,
        mouse.just_pressed(MouseButton::Left),
        keys.just_pressed(KeyCode::Escape),
        grabbed,
    ) {
        Some(true) => {
            cursor.visible = false;
            cursor.grab_mode = CursorGrabMode::Locked;
        }
        Some(false) => {
            cursor.visible = true;
            cursor.grab_mode = CursorGrabMode::None;
        }
        None => {}
    }
}

fn read_input(
    keys: Res<ButtonInput<KeyCode>>,
    mouse: Res<AccumulatedMouseMotion>,
    sensitivity: Res<MouseSensitivity>,
    cursor: Single<&CursorOptions>,
    player: Single<(&mut PendingInput, &mut Look), With<Player>>,
) {
    let (mut input, mut look) = player.into_inner();
    // Any active grab counts: on X11 `Locked` may fall back to `Confined`.
    if cursor.grab_mode != CursorGrabMode::None {
        look.angle -= mouse.delta.x * sensitivity.0;
        look.pitch = (look.pitch - mouse.delta.y * sensitivity.0).clamp(-PITCH_LIMIT, PITCH_LIMIT);
    }
    let axis = |pos: KeyCode, neg: KeyCode| {
        keys.pressed(pos) as i32 as f32 - keys.pressed(neg) as i32 as f32
    };
    input.forward = axis(KeyCode::KeyW, KeyCode::KeyS);
    input.strafe = axis(KeyCode::KeyD, KeyCode::KeyA);
    input.jump = keys.pressed(KeyCode::Space);
    input.crouch = keys.pressed(KeyCode::KeyC) || keys.pressed(KeyCode::ControlLeft);
    input.use_pressed |= keys.just_pressed(KeyCode::KeyE);
}

fn update_camera(
    time: Res<Time<Fixed>>,
    frame_time: Res<Time>,
    player: Single<(&PlayerBody, &PrevFeet, &Look, &mut EyeHeight), With<Player>>,
    mut camera: Single<&mut Transform, With<PlayerCamera>>,
) {
    let (body, prev, look, mut eye_h) = player.into_inner();
    let feet = prev.0.lerp(body.0.pos, time.overstep_fraction());
    eye_h.0 = ease_toward(
        eye_h.0,
        body.0.height - EYE_BELOW_TOP,
        EYE_EASE_RATE,
        frame_time.delta_secs(),
    );
    let eye = feet + Vec3::Z * eye_h.0;
    camera.translation = to_bevy(eye);
    camera.rotation = Quat::from_euler(
        EulerRot::YXZ,
        core_angle_to_yaw(look.angle),
        look.pitch,
        0.0,
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn eye_eases_without_overshoot_and_is_frame_rate_independent() {
        assert_eq!(ease_toward(1.65, 0.95, EYE_EASE_RATE, 0.0), 1.65);
        let one = ease_toward(1.65, 0.95, EYE_EASE_RATE, 1.0 / 30.0);
        let two = ease_toward(
            ease_toward(1.65, 0.95, EYE_EASE_RATE, 1.0 / 60.0),
            0.95,
            EYE_EASE_RATE,
            1.0 / 60.0,
        );
        assert!((one - two).abs() < 1e-5);
        assert!(one < 1.65 && one > 0.95);
        assert!((ease_toward(1.65, 0.95, EYE_EASE_RATE, 1.0) - 0.95).abs() < 1e-3);
    }

    #[test]
    fn grab_follows_click_escape_and_focus() {
        assert_eq!(grab_change(true, true, false, false), Some(true));
        assert_eq!(
            grab_change(false, true, false, false),
            None,
            "no grab while unfocused"
        );
        assert_eq!(grab_change(true, false, true, true), Some(false));
        assert_eq!(
            grab_change(false, false, false, true),
            Some(false),
            "focus lost releases"
        );
        assert_eq!(grab_change(true, false, false, true), None);
    }
}
