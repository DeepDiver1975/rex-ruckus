//! Visible stand-ins until M4's art: glowing wall panels for switches, spinning cubes for keycards.

use crate::coords::to_bevy;
use crate::level::CurrentMap;
use crate::mechanics::LevelMechanics;
use bevy::prelude::*;
use rr_core::map::{ItemKind, Key, SwitchAction};

#[derive(Component)]
pub struct SwitchPanel(pub usize);

#[derive(Component)]
pub struct ItemProp(pub usize);

#[derive(Resource)]
struct PanelMaterials {
    off: Handle<StandardMaterial>,
    on: Handle<StandardMaterial>,
}

pub fn key_color(k: Key) -> Color {
    match k {
        Key::Red => Color::srgb(0.9, 0.15, 0.1),
        Key::Blue => Color::srgb(0.15, 0.35, 0.95),
        Key::Yellow => Color::srgb(0.95, 0.8, 0.1),
    }
}

fn glow(c: Color) -> StandardMaterial {
    StandardMaterial {
        base_color: c,
        emissive: c.to_linear() * 4.0,
        ..default()
    }
}

pub struct PropsPlugin;

impl Plugin for PropsPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, spawn_props)
            .add_systems(Update, (update_switch_panels, animate_items));
    }
}

fn spawn_props(
    mut commands: Commands,
    map: Res<CurrentMap>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let map = &map.0;
    let panel = meshes.add(Cuboid::new(0.4, 0.6, 0.06));
    let mats = PanelMaterials {
        off: materials.add(glow(Color::srgb(0.8, 0.1, 0.1))),
        on: materials.add(glow(Color::srgb(0.1, 0.8, 0.2))),
    };
    let exit = materials.add(glow(Color::srgb(1.0, 0.75, 0.0)));
    for (i, sw) in map.switches.iter().enumerate() {
        let w = &map.walls[sw.wall];
        let n = w.inward_normal();
        let at = ((w.a + w.b) * 0.5 + n * 0.03).extend(map.sectors[w.sector].floor_z + 1.3);
        let mat = if sw.action == SwitchAction::Exit {
            exit.clone()
        } else {
            mats.off.clone()
        };
        commands.spawn((
            Mesh3d(panel.clone()),
            MeshMaterial3d(mat),
            Transform::from_translation(to_bevy(at)).looking_to(to_bevy(n.extend(0.0)), Vec3::Y),
            SwitchPanel(i),
        ));
    }
    let cube = meshes.add(Cuboid::new(0.3, 0.3, 0.3));
    for (i, item) in map.items.iter().enumerate() {
        // Temporary: only keys get props until per-kind item styles land.
        let ItemKind::Key(k) = item.kind else {
            continue;
        };
        let floor = map
            .find_sector(item.pos, None)
            .map_or(0.0, |s| map.sectors[s].floor_z);
        commands.spawn((
            Mesh3d(cube.clone()),
            MeshMaterial3d(materials.add(glow(key_color(k)))),
            Transform::from_translation(to_bevy(item.pos.extend(floor + 0.6))),
            ItemProp(i),
        ));
    }
    commands.insert_resource(mats);
}

fn update_switch_panels(
    map: Res<CurrentMap>,
    mech: Res<LevelMechanics>,
    mats: Res<PanelMaterials>,
    mut q: Query<(&SwitchPanel, &mut MeshMaterial3d<StandardMaterial>)>,
) {
    for (p, mut m) in &mut q {
        if map.0.switches[p.0].action == SwitchAction::Exit {
            continue;
        }
        let want = if mech.0.switch_on[p.0] {
            &mats.on
        } else {
            &mats.off
        };
        if m.0 != *want {
            m.0 = want.clone();
        }
    }
}

fn animate_items(
    mut commands: Commands,
    time: Res<Time>,
    mech: Res<LevelMechanics>,
    mut q: Query<(Entity, &ItemProp, &mut Transform)>,
) {
    for (e, item, mut t) in &mut q {
        if mech.0.taken[item.0] {
            commands.entity(e).despawn();
        } else {
            t.rotate_y(time.delta_secs() * 2.0);
        }
    }
}
