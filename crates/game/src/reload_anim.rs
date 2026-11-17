//! Per-weapon reload animations for the viewmodel: pure keyframe tracks (this module) that
//! `viewmodel` applies to the view rig, a code-built glove ([`ReloadHand`]) and a code-built
//! prop per gun ([`ReloadProp`]: a magazine, a shell, an ammo box or a rocket).
//!
//! A track runs over the reload's progress `t` in 0..1. Each key holds the gun's offset and
//! rotation (applied to the whole rig, so the hand and prop, children of the rig, move in the
//! gun's frame), and the hand and prop positions in that frame. Keys are eased with smoothstep;
//! two keys at almost the same `t` make a cut (a prop vanishing into the gun, or a fresh one
//! appearing off screen). The first and last keys are the rest pose with the hand and prop
//! below the view, so a reload starts and ends without a pop.

use bevy::prelude::*;
use rr_core::defs::WeaponId;

/// Below the view (rig space): where the hand and props wait.
const OFF: Vec3 = Vec3::new(0.02, -0.32, 0.06);

/// One key of a reload track. `rot` is (pitch, yaw, roll) in degrees: pitch tips the muzzle
/// up, roll turns the gun's top to the left.
#[derive(Clone, Copy, Debug)]
pub struct Key {
    pub t: f32,
    pub gun: Vec3,
    pub rot: Vec3,
    pub hand: Vec3,
    pub prop: Vec3,
}

const fn key(t: f32, gun: [f32; 3], rot: [f32; 3], hand: [f32; 3], prop: [f32; 3]) -> Key {
    Key {
        t,
        gun: Vec3::from_array(gun),
        rot: Vec3::from_array(rot),
        hand: Vec3::from_array(hand),
        prop: Vec3::from_array(prop),
    }
}

const O: [f32; 3] = [0.0, 0.0, 0.0];
const OFF_A: [f32; 3] = [OFF.x, OFF.y, OFF.z];

/// Pistol: raised and tilted right so the grip shows, the empty magazine drops out, the hand
/// brings a fresh one up and slaps it home, the gun jolts and settles.
const PISTOL_GRIP: [f32; 3] = [0.01, -0.055, 0.012];
const PG: [f32; 3] = [-0.04, 0.06, -0.04];
const PR: [f32; 3] = [15.0, 0.0, -25.0];
const PISTOL: &[Key] = &[
    key(0.0, O, O, OFF_A, PISTOL_GRIP),
    key(0.15, PG, PR, OFF_A, PISTOL_GRIP),
    key(0.32, PG, PR, [0.01, -0.3, 0.05], [0.01, -0.3, 0.012]),
    key(0.33, PG, PR, [0.01, -0.3, 0.05], [0.01, -0.26, 0.012]),
    key(0.62, PG, PR, [0.01, -0.13, 0.015], [0.01, -0.1, 0.012]),
    key(
        0.74,
        [-0.04, 0.07, -0.04],
        [18.0, 0.0, -22.0],
        [0.01, -0.09, 0.015],
        PISTOL_GRIP,
    ),
    key(
        0.88,
        [-0.01, 0.015, -0.01],
        [5.0, 0.0, -8.0],
        [0.01, -0.3, 0.05],
        PISTOL_GRIP,
    ),
    key(1.0, O, O, OFF_A, PISTOL_GRIP),
];

/// Shotgun: raised and rolled onto its side, the hand feeds three shells into the loading
/// port, then racks the pump along the barrel.
const PORT: [f32; 3] = [0.0, -0.03, -0.06];
const SHELL_IN: [f32; 3] = [0.0, -0.03, -0.045];
const SHELL_HAND: [f32; 3] = [0.0, -0.06, -0.06];
const SHELL_WAIT: [f32; 3] = [0.0, -0.2, -0.02];
const SG: [f32; 3] = [0.0, 0.03, -0.02];
const SR: [f32; 3] = [8.0, 0.0, 55.0];
const SHOTGUN: &[Key] = &[
    key(0.0, O, O, OFF_A, OFF_A),
    key(0.1, SG, SR, SHELL_WAIT, SHELL_WAIT),
    // Three shells, each pushed in, then the next appears in the hand below.
    key(0.2, SG, SR, SHELL_HAND, PORT),
    key(0.25, SG, SR, SHELL_HAND, SHELL_IN),
    key(0.251, SG, SR, SHELL_HAND, OFF_A),
    key(0.3, SG, SR, SHELL_WAIT, SHELL_WAIT),
    key(0.4, SG, SR, SHELL_HAND, PORT),
    key(0.45, SG, SR, SHELL_HAND, SHELL_IN),
    key(0.451, SG, SR, SHELL_HAND, OFF_A),
    key(0.5, SG, SR, SHELL_WAIT, SHELL_WAIT),
    key(0.6, SG, SR, SHELL_HAND, PORT),
    key(0.65, SG, SR, SHELL_HAND, SHELL_IN),
    key(0.651, SG, SR, SHELL_HAND, OFF_A),
    // Roll back, grab the fore-end, rack it.
    key(0.75, O, [6.0, 0.0, 0.0], [0.0, -0.06, -0.15], OFF_A),
    key(
        0.84,
        [0.0, 0.0, 0.015],
        [9.0, 0.0, 0.0],
        [0.0, -0.06, -0.1],
        OFF_A,
    ),
    key(0.92, O, [3.0, 0.0, 0.0], [0.0, -0.06, -0.15], OFF_A),
    key(1.0, O, O, OFF_A, OFF_A),
];

/// Chaingun: hefted up and turned to show its flank, the empty ammo box drops away, the hand
/// brings a full one up from below and clamps it on.
const BOX_ON: [f32; 3] = [0.05, -0.14, -0.06];
const CG: [f32; 3] = [-0.08, 0.06, -0.1];
const CR: [f32; 3] = [8.0, 35.0, -10.0];
const CHAINGUN: &[Key] = &[
    key(0.0, O, O, OFF_A, BOX_ON),
    key(0.15, CG, CR, OFF_A, BOX_ON),
    key(0.32, CG, CR, [-0.15, -0.28, -0.06], [0.05, -0.45, -0.06]),
    key(0.33, CG, CR, [-0.15, -0.28, -0.06], [-0.15, -0.24, -0.06]),
    key(0.62, CG, CR, [0.02, -0.22, -0.06], [0.03, -0.18, -0.06]),
    key(
        0.78,
        [-0.08, 0.07, -0.1],
        [11.0, 35.0, -8.0],
        [0.05, -0.18, -0.06],
        BOX_ON,
    ),
    key(
        0.9,
        [-0.03, 0.02, -0.04],
        [3.0, 10.0, -3.0],
        [0.0, -0.35, 0.03],
        BOX_ON,
    ),
    key(1.0, O, O, OFF_A, BOX_ON),
];

/// Rocket launcher: pushed out, tipped up and swung left so its rear comes into view; the hand slides a
/// rocket into the back of the tube.
const RG: [f32; 3] = [-0.06, 0.03, -0.16];
const RR: [f32; 3] = [25.0, 35.0, 10.0];
const ROCKETS: &[Key] = &[
    key(0.0, O, O, OFF_A, OFF_A),
    key(0.2, RG, RR, [0.0, -0.3, 0.2], [0.0, -0.26, 0.2]),
    key(0.45, RG, RR, [0.0, -0.02, 0.2], [0.0, 0.02, 0.16]),
    key(0.72, RG, RR, [0.0, -0.02, 0.12], [0.0, 0.02, 0.07]),
    key(0.721, RG, RR, [0.0, -0.02, 0.12], OFF_A),
    key(
        0.85,
        [-0.01, 0.01, -0.05],
        [10.0, 0.0, 3.0],
        [0.0, -0.3, 0.15],
        OFF_A,
    ),
    key(1.0, O, O, OFF_A, OFF_A),
];

/// The reload track of a gun; `None` for weapons that never reload (boot, pipe bombs).
pub fn track(w: WeaponId) -> Option<&'static [Key]> {
    match w {
        WeaponId::Pistol => Some(PISTOL),
        WeaponId::Shotgun => Some(SHOTGUN),
        WeaponId::Chaingun => Some(CHAINGUN),
        WeaponId::Rockets => Some(ROCKETS),
        WeaponId::Boot | WeaponId::PipeBombs => None,
    }
}

/// A sampled reload pose: the gun's offset and rotation (applied to the rig) and the hand and
/// prop positions in the gun's frame.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Pose {
    pub gun: Vec3,
    pub rot: Quat,
    pub hand: Vec3,
    pub prop: Vec3,
}

impl Pose {
    pub const REST: Pose = Pose {
        gun: Vec3::ZERO,
        rot: Quat::IDENTITY,
        hand: OFF,
        prop: OFF,
    };
}

fn rotation(deg: Vec3) -> Quat {
    let r = deg * (std::f32::consts::PI / 180.0);
    Quat::from_euler(EulerRot::XYZ, r.x, r.y, r.z)
}

/// The pose of `w`'s reload at progress `t` (clamped to 0..1); [`Pose::REST`] for weapons
/// without a track.
pub fn pose(w: WeaponId, t: f32) -> Pose {
    let Some(keys) = track(w) else {
        return Pose::REST;
    };
    let t = t.clamp(0.0, 1.0);
    let i = keys.iter().rposition(|k| k.t <= t).unwrap_or(0);
    let a = keys[i];
    let b = keys.get(i + 1).copied().unwrap_or(a);
    let f = if b.t > a.t {
        let x = ((t - a.t) / (b.t - a.t)).clamp(0.0, 1.0);
        x * x * (3.0 - 2.0 * x)
    } else {
        0.0
    };
    Pose {
        gun: a.gun.lerp(b.gun, f),
        rot: rotation(a.rot).slerp(rotation(b.rot), f),
        hand: a.hand.lerp(b.hand, f),
        prop: a.prop.lerp(b.prop, f),
    }
}

/// The glove that does the reloading; shown only during a reload.
#[derive(Component)]
pub struct ReloadHand;

/// A gun's reload prop; shown only while that gun reloads.
#[derive(Component)]
pub struct ReloadProp(pub WeaponId);

/// A box of a prop: size and centre.
type PropBox = ([f32; 3], [f32; 3]);

/// The prop's boxes and colour.
fn prop_shape(w: WeaponId) -> (&'static [PropBox], Color) {
    match w {
        // A pistol magazine.
        WeaponId::Pistol => (
            &[([0.016, 0.05, 0.024], [0.0, 0.0, 0.0])],
            Color::srgb(0.12, 0.12, 0.13),
        ),
        // A red shell with a brass base, lying along the gun.
        WeaponId::Shotgun => (
            &[([0.014, 0.014, 0.034], [0.0, 0.0, 0.0])],
            Color::srgb(0.7, 0.1, 0.08),
        ),
        // An olive ammo box with a belt stub on top.
        WeaponId::Chaingun => (
            &[
                ([0.07, 0.055, 0.085], [0.0, 0.0, 0.0]),
                ([0.025, 0.02, 0.03], [0.0, 0.036, -0.01]),
            ],
            Color::srgb(0.3, 0.34, 0.18),
        ),
        // A rocket along the tube: body and warhead.
        WeaponId::Rockets => (
            &[
                ([0.032, 0.032, 0.11], [0.0, 0.0, 0.0]),
                ([0.024, 0.024, 0.03], [0.0, 0.0, -0.07]),
            ],
            Color::srgb(0.32, 0.36, 0.2),
        ),
        WeaponId::Boot | WeaponId::PipeBombs => (&[], Color::BLACK),
    }
}

/// Spawns the reload hand and every gun's prop under the view rig, hidden.
pub fn spawn(
    commands: &mut Commands,
    rig: Entity,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
) {
    let glove = materials.add(StandardMaterial {
        base_color: Color::srgb(0.2, 0.15, 0.1),
        perceptual_roughness: 0.8,
        ..default()
    });
    let hand = commands
        .spawn((
            ReloadHand,
            Transform::from_translation(OFF),
            Visibility::Hidden,
            ChildOf(rig),
        ))
        .id();
    // Fist, thumb and wrist, reaching up from below.
    for (size, at) in [
        ([0.045, 0.04, 0.05], [0.0, 0.0, 0.0]),
        ([0.015, 0.02, 0.03], [-0.025, 0.01, -0.01]),
        ([0.04, 0.1, 0.04], [0.0, -0.07, 0.01]),
    ] {
        commands.spawn((
            Mesh3d(meshes.add(Cuboid::new(size[0], size[1], size[2]))),
            MeshMaterial3d(glove.clone()),
            Transform::from_translation(Vec3::from_array(at)),
            bevy::light::NotShadowCaster,
            ChildOf(hand),
        ));
    }
    for w in WeaponId::ALL {
        let (boxes, color) = prop_shape(w);
        if boxes.is_empty() {
            continue;
        }
        let mat = materials.add(StandardMaterial {
            base_color: color,
            perceptual_roughness: 0.6,
            ..default()
        });
        let prop = commands
            .spawn((
                ReloadProp(w),
                Transform::from_translation(OFF),
                Visibility::Hidden,
                ChildOf(rig),
            ))
            .id();
        for (size, at) in boxes {
            commands.spawn((
                Mesh3d(meshes.add(Cuboid::new(size[0], size[1], size[2]))),
                MeshMaterial3d(mat.clone()),
                Transform::from_translation(Vec3::from_array(*at)),
                bevy::light::NotShadowCaster,
                ChildOf(prop),
            ));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const GUNS: [WeaponId; 4] = [
        WeaponId::Pistol,
        WeaponId::Shotgun,
        WeaponId::Chaingun,
        WeaponId::Rockets,
    ];

    #[test]
    fn every_gun_has_a_track_and_the_rest_have_none() {
        for w in GUNS {
            assert!(track(w).is_some(), "{w:?}");
            assert!(!prop_shape(w).0.is_empty(), "{w:?} prop");
        }
        assert!(track(WeaponId::Boot).is_none());
        assert!(track(WeaponId::PipeBombs).is_none());
        assert_eq!(pose(WeaponId::Boot, 0.5), Pose::REST);
    }

    #[test]
    fn tracks_start_and_end_at_rest_with_the_hand_off_screen() {
        for w in GUNS {
            let keys = track(w).unwrap();
            assert_eq!(keys.first().unwrap().t, 0.0, "{w:?}");
            assert_eq!(keys.last().unwrap().t, 1.0, "{w:?}");
            assert!(
                keys.windows(2).all(|k| k[0].t < k[1].t),
                "{w:?} keys in order"
            );
            for t in [0.0, 1.0] {
                let p = pose(w, t);
                assert!(p.gun.length() < 1e-6, "{w:?} at {t}: {p:?}");
                assert!(p.rot.angle_between(Quat::IDENTITY) < 1e-5, "{w:?} at {t}");
                assert_eq!(p.hand, OFF, "{w:?} at {t}");
            }
        }
    }

    /// Where a point of the gun's frame lands on screen (-1..1 each way) at the default
    /// 75° vertical field of view, 16:9; `None` behind the eye.
    fn on_screen(p: &Pose, local: Vec3) -> Option<Vec2> {
        let c = crate::viewmodel::RIG_BASE + p.gun + p.rot * local;
        let t = (75f32.to_radians() / 2.0).tan();
        (c.z < -0.02).then(|| Vec2::new(c.x / -c.z / (t * 16.0 / 9.0), c.y / -c.z / t))
    }

    #[test]
    fn the_hand_and_the_prop_come_into_view() {
        for w in GUNS {
            let seen = |f: fn(&Pose) -> Vec3| {
                (0..=100).any(|i| {
                    let p = pose(w, i as f32 / 100.0);
                    on_screen(&p, f(&p)).is_some_and(|s| s.abs().max_element() < 0.9)
                })
            };
            assert!(seen(|p| p.hand), "{w:?}: the hand never shows");
            assert!(seen(|p| p.prop), "{w:?}: the prop never shows");
            assert!(
                (0..=100).any(|i| pose(w, i as f32 / 100.0).gun.length() > 0.01),
                "{w:?}: the gun never moves"
            );
        }
    }

    #[test]
    fn poses_are_continuous_except_at_cuts() {
        // Between neighbouring samples the gun never jumps (props may cut).
        for w in GUNS {
            let mut last = pose(w, 0.0);
            for i in 1..=400 {
                let p = pose(w, i as f32 / 400.0);
                assert!((p.gun - last.gun).length() < 0.01, "{w:?} at {i}");
                assert!(p.rot.angle_between(last.rot) < 0.05, "{w:?} at {i}");
                last = p;
            }
        }
    }
}
