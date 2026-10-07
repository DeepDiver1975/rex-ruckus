//! Glowing wall panels for switches and spinning glTF item props that ride their sector's live
//! floor (lifts included); keycards are tinted with their key colour.

use crate::coords::{core_angle_to_yaw, to_bevy};
use crate::flow::{LevelEntity, SpawnLevel};
use crate::level::CurrentMap;
use crate::mechanics::LevelMechanics;
use crate::models::{Look, ModelLibrary, ModelReady, TintCache, spawn_model};
use bevy::prelude::*;
use rr_core::map::{ItemKind, Key, Map, SectorId, SwitchAction};
use rr_core::props::PropKind;

#[derive(Component)]
pub struct SwitchPanel(pub usize);

/// An item's prop: `index` into `map.items`, and the sector it stands in (found once at spawn).
#[derive(Component)]
pub struct ItemProp {
    pub index: usize,
    pub sector: Option<SectorId>,
}

/// The glTF model under an [`ItemProp`]; a keycard has `tint` (sRGB multiplier) and gets it once
/// its model is ready.
#[derive(Component)]
struct ItemModel {
    tint: Option<(f32, f32, f32)>,
    tinted: bool,
}

/// A gag prop's root; the index is into `map.props`.
#[derive(Component)]
pub struct GagProp(pub usize);

/// Code-built stand-in: (size, centre, colour) boxes in model space (x across, y up, -z the
/// front). The first box is the body and spans the whole footprint.
pub fn fallback_parts(kind: PropKind) -> Vec<(Vec3, Vec3, Color)> {
    let h = kind.half_extents();
    let (w, d, ht) = (2.0 * h.y, 2.0 * h.x, kind.height());
    match kind {
        PropKind::Toilet => vec![
            (
                Vec3::new(w, 0.45, d),
                Vec3::new(0.0, 0.225, 0.0),
                Color::srgb(0.92, 0.92, 0.9),
            ),
            (
                Vec3::new(w, ht, 0.2),
                Vec3::new(0.0, ht * 0.5, d * 0.5 - 0.1),
                Color::srgb(0.85, 0.85, 0.82),
            ),
        ],
        PropKind::Vending => vec![
            (
                Vec3::new(w, ht, d),
                Vec3::new(0.0, ht * 0.5, 0.0),
                Color::srgb(0.75, 0.1, 0.1),
            ),
            (
                Vec3::new(w * 0.6, ht * 0.5, 0.02),
                Vec3::new(-w * 0.1, ht * 0.6, -d * 0.5 - 0.01),
                Color::srgb(0.9, 0.9, 1.0),
            ),
        ],
        PropKind::PoolTable => vec![
            (
                Vec3::new(w, ht, d),
                Vec3::new(0.0, ht * 0.5, 0.0),
                Color::srgb(0.35, 0.2, 0.1),
            ),
            (
                Vec3::new(w - 0.16, 0.02, d - 0.16),
                Vec3::new(0.0, ht + 0.01 - 0.02, 0.0),
                Color::srgb(0.05, 0.45, 0.15),
            ),
        ],
    }
}

/// Height of an item prop's centre above its floor.
const ITEM_LIFT: f32 = 0.6;
/// Keycard tints are the key colour times this: the card texture is mid-grey, so the plain key
/// colour comes out murky (yellow reads olive).
const KEY_TINT_GAIN: f32 = 1.35;

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
            .add_systems(Update, (update_switch_panels, animate_items, tint_keycards));
    }
}

/// Switch panels and item props (in [`SpawnLevel`]); the panel materials survive restarts.
fn spawn_props(
    mut commands: Commands,
    map: Res<CurrentMap>,
    lib: Res<ModelLibrary>,
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
    for (i, item) in map.items.iter().enumerate() {
        let sector = map.find_sector(item.pos, None);
        let z = sector.map_or(ITEM_LIFT, |s| item_prop_z(map, s));
        let prop = commands
            .spawn((
                Transform::from_translation(to_bevy(item.pos.extend(z))),
                Visibility::default(),
                ItemProp { index: i, sector },
                LevelEntity,
            ))
            .id();
        let model = lib.item(item.kind);
        let root = spawn_model(&mut commands, prop, &model.scene, &model.place);
        let tint = match item.kind {
            ItemKind::Key(k) => {
                let c = key_color(k).to_srgba();
                let g = KEY_TINT_GAIN;
                Some((c.red * g, c.green * g, c.blue * g))
            }
            _ => None,
        };
        commands.entity(root).insert(ItemModel {
            tint,
            tinted: tint.is_none(),
        });
    }
    for (i, p) in map.props.iter().enumerate() {
        let floor = map.sectors[p.sector].floor_z;
        let root = commands
            .spawn((
                Transform::from_translation(to_bevy(p.pos.extend(floor)))
                    .with_rotation(Quat::from_rotation_y(core_angle_to_yaw(p.angle))),
                Visibility::default(),
                GagProp(i),
                LevelEntity,
            ))
            .id();
        if let Some(m) = lib.prop(p.kind) {
            spawn_model(&mut commands, root, &m.scene, &m.place);
            continue;
        }
        for (size, centre, c) in fallback_parts(p.kind) {
            // The vending machine's front panel glows.
            let emissive = if p.kind == PropKind::Vending && size.z < 0.05 {
                c.to_linear() * 2.0
            } else {
                LinearRgba::BLACK
            };
            commands.spawn((
                Mesh3d(meshes.add(Cuboid::from_size(size))),
                MeshMaterial3d(materials.add(StandardMaterial {
                    base_color: c,
                    emissive,
                    perceptual_roughness: 0.7,
                    ..default()
                })),
                Transform::from_translation(centre),
                ChildOf(root),
            ));
        }
    }
}

/// Tints each keycard model with its key colour once the model is ready (the `Normal` look with
/// a tint; textures are kept).
fn tint_keycards(
    mut cache: ResMut<TintCache>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut roots: Query<(&mut ItemModel, &ModelReady)>,
    mut mats: Query<&mut MeshMaterial3d<StandardMaterial>>,
) {
    for (mut model, ready) in &mut roots {
        if model.tinted {
            continue;
        }
        // Wait until every source material has loaded: `TintCache` hands back untinted sources
        // otherwise, and we would never retry.
        let loaded = ready
            .meshes
            .iter()
            .all(|&e| mats.get(e).map_or(true, |m| materials.get(&m.0).is_some()));
        if !loaded {
            continue;
        }
        for &e in &ready.meshes {
            if let Ok(mut m) = mats.get_mut(e) {
                let h = cache.get(&mut materials, &m.0, Look::Normal, model.tint);
                if m.0 != h {
                    m.0 = h;
                }
            }
        }
        model.tinted = true;
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
    use rr_core::props::PropKind;

    #[test]
    fn fallback_footprints_match_the_core_boxes() {
        for kind in PropKind::ALL {
            let parts = fallback_parts(kind);
            let (size, _, _) = parts[0]; // the body block comes first
            let h = kind.half_extents();
            assert!(
                (size.x - 2.0 * h.y).abs() < 1e-5 && (size.z - 2.0 * h.x).abs() < 1e-5,
                "{kind:?}"
            );
            assert!(
                parts
                    .iter()
                    .all(|(s, c, _)| c.y + s.y * 0.5 <= kind.height() + 1e-5)
            );
        }
    }

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
