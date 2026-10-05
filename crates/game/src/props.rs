//! Visible stand-ins until M4's art: glowing wall panels for switches and spinning, per-kind
//! item props that ride their sector's live floor (lifts included).

use crate::coords::to_bevy;
use crate::flow::{LevelEntity, SpawnLevel};
use crate::level::CurrentMap;
use crate::mechanics::LevelMechanics;
use bevy::prelude::*;
use rr_core::map::{ItemKind, Key, Map, SectorId, SwitchAction};

#[derive(Component)]
pub struct SwitchPanel(pub usize);

/// An item's prop: `index` into `map.items`, and the sector it stands in (found once at spawn).
#[derive(Component)]
pub struct ItemProp {
    pub index: usize,
    pub sector: Option<SectorId>,
}

/// Height of an item prop's centre above its floor.
const ITEM_LIFT: f32 = 0.6;

#[derive(Resource, Clone)]
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

/// Colour and box size (Bevy axes: x, y up, z) of an item kind's prop. Colours follow the
/// `render-svg` legend: ammo yellow, shells orange, shotgun brown, health white (red cross),
/// keys in their key colour.
pub fn item_style(kind: ItemKind) -> (Color, Vec3) {
    match kind {
        ItemKind::Key(k) => (key_color(k), Vec3::new(0.3, 0.3, 0.3)),
        ItemKind::PistolAmmo => (Color::srgb(0.96, 0.82, 0.25), Vec3::new(0.2, 0.14, 0.12)),
        ItemKind::ShotgunShells => (Color::srgb(0.9, 0.49, 0.13), Vec3::new(0.3, 0.16, 0.16)),
        ItemKind::Shotgun => (Color::srgb(0.55, 0.35, 0.17), Vec3::new(0.8, 0.1, 0.12)),
        ItemKind::HealthSmall => (Color::srgb(0.95, 0.95, 0.95), Vec3::new(0.3, 0.3, 0.3)),
    }
}

/// The red cross on a health prop.
const HEALTH_CROSS: Color = Color::srgb(0.75, 0.15, 0.12);

/// Item props glow, except the white health box, which would bloom and hide its cross.
fn item_material(kind: ItemKind, c: Color) -> StandardMaterial {
    if kind == ItemKind::HealthSmall {
        StandardMaterial {
            base_color: c,
            emissive: c.to_linear() * 0.3,
            ..default()
        }
    } else {
        glow(c)
    }
}

pub fn glow(c: Color) -> StandardMaterial {
    StandardMaterial {
        base_color: c,
        emissive: c.to_linear() * 4.0,
        ..default()
    }
}

pub struct PropsPlugin;

impl Plugin for PropsPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(SpawnLevel, spawn_props)
            .add_systems(Update, (update_switch_panels, animate_items));
    }
}

/// Switch panels and item props (in [`SpawnLevel`]); the panel materials survive restarts.
fn spawn_props(
    mut commands: Commands,
    map: Res<CurrentMap>,
    existing: Option<Res<PanelMaterials>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let map = &map.0;
    let panel = meshes.add(Cuboid::new(0.4, 0.6, 0.06));
    let mats = match existing {
        Some(m) => m.clone(),
        None => {
            let mats = PanelMaterials {
                off: materials.add(glow(Color::srgb(0.8, 0.1, 0.1))),
                on: materials.add(glow(Color::srgb(0.1, 0.8, 0.2))),
            };
            commands.insert_resource(mats.clone());
            mats
        }
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
            LevelEntity,
        ));
    }
    let cross = materials.add(glow(HEALTH_CROSS));
    let cross_bars = [
        meshes.add(Cuboid::new(0.2, 0.06, 0.32)),
        meshes.add(Cuboid::new(0.06, 0.2, 0.32)),
    ];
    for (i, item) in map.items.iter().enumerate() {
        let sector = map.find_sector(item.pos, None);
        let z = sector.map_or(ITEM_LIFT, |s| item_prop_z(map, s));
        let (color, size) = item_style(item.kind);
        let mut prop = commands.spawn((
            Mesh3d(meshes.add(Cuboid::from_size(size))),
            MeshMaterial3d(materials.add(item_material(item.kind, color))),
            Transform::from_translation(to_bevy(item.pos.extend(z))),
            ItemProp { index: i, sector },
            LevelEntity,
        ));
        if item.kind == ItemKind::HealthSmall {
            prop.with_children(|c| {
                for bar in &cross_bars {
                    c.spawn((Mesh3d(bar.clone()), MeshMaterial3d(cross.clone())));
                }
            });
        }
    }
}

/// Height (core z) of an item prop's centre standing in `sector`, on the live floor.
pub fn item_prop_z(map: &Map, sector: SectorId) -> f32 {
    map.sectors[sector].floor_z + ITEM_LIFT
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

/// Spins item props, keeps them on their (possibly moving) floor and despawns taken ones.
fn animate_items(
    mut commands: Commands,
    time: Res<Time>,
    map: Res<CurrentMap>,
    mech: Res<LevelMechanics>,
    mut q: Query<(Entity, &ItemProp, &mut Transform)>,
) {
    for (e, item, mut t) in &mut q {
        if mech.0.taken[item.index] {
            commands.entity(e).despawn();
            continue;
        }
        t.rotate_y(time.delta_secs() * 2.0);
        if let Some(s) = item.sector {
            // Bevy y is core z.
            t.translation.y = item_prop_z(&map.0, s);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::math::Vec2;
    use rr_core::fixtures::lift_shaft;
    use rr_core::mechanics::Mechanics;

    #[test]
    fn item_prop_z_tracks_lift_floor() {
        let mut map = lift_shaft("(kind: Lift(to: 2.0))", "");
        let mut mech = Mechanics::new(&mut map);
        let lift = map.find_sector(Vec2::new(5.0, 2.0), None).unwrap();
        assert!((item_prop_z(&map, lift) - 0.6).abs() < 1e-6);
        mech.toggle(0);
        let mut last = item_prop_z(&map, lift);
        for _ in 0..120 {
            mech.tick(&mut map, &mut [], 1.0 / 60.0);
            let z = item_prop_z(&map, lift);
            assert!(z >= last, "rises with the floor");
            last = z;
        }
        assert_eq!(map.sectors[lift].floor_z, 2.0);
        assert!((item_prop_z(&map, lift) - 2.6).abs() < 1e-6);
    }
}
