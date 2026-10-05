//! Bullet-hole and scorch decals. Presentational only: reads `Impact` and `Explosion` events from
//! the [`FxQueue`], places small textured quads on the surface, and keeps at most [`RING_CAP`]
//! alive. A decal belongs to the sector it was placed in and is removed when that sector is
//! re-extruded (a door, lift, glass pane or crack wall that moved or broke would leave it hanging).

use crate::combat::{FxQueue, FxReaders};
use crate::coords::to_bevy;
use crate::flow::LevelEntity;
use crate::level::{CurrentMap, rebuild_dirty_sectors};
use crate::mechanics::DirtySectors;
use bevy::asset::RenderAssetUsages;
use bevy::image::{ImageAddressMode, ImageSampler, ImageSamplerDescriptor};
use bevy::light::NotShadowCaster;
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};
use rr_core::combat::CombatEvent;
use rr_core::map::{Map, SectorId};
use rr_core::rng::Rng;
use rr_core::trace::{Hit, Ray, trace_world};
use std::collections::VecDeque;

/// Most decals alive at once; the oldest goes first.
pub const RING_CAP: usize = 128;
/// Side of a bullet hole in metres.
pub const HOLE_SIZE: f32 = 0.12;
/// Side of a scorch mark in metres.
pub const SCORCH_SIZE: f32 = 1.5;
/// Lift off the surface, against z-fighting.
pub const SURFACE_OFFSET: f32 = 0.002;
const TEX: u32 = 16;

/// A decal and the sector whose rebuild removes it.
#[derive(Component, Debug, Clone, Copy)]
pub struct Decal {
    pub sector: SectorId,
}

/// Live decals, oldest first.
#[derive(Resource, Default)]
pub struct DecalRing(VecDeque<Entity>);

impl DecalRing {
    pub fn len(&self) -> usize {
        self.0.len()
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// Adds `e`; returns the oldest entity if the ring overflowed and it must be despawned.
    pub fn push(&mut self, e: Entity) -> Option<Entity> {
        self.0.push_back(e);
        if self.0.len() > RING_CAP {
            self.0.pop_front()
        } else {
            None
        }
    }

    /// Forgets every decal (the entities are despawned by the level restart itself).
    pub fn reset(&mut self) {
        self.0.clear();
    }
}

struct DecalAssets {
    quad: Handle<Mesh>,
    hole: Handle<StandardMaterial>,
    scorch: Handle<StandardMaterial>,
}

/// RGBA8 bytes of a bullet hole: a dark pit with a ragged edge.
pub fn hole_pixels() -> Vec<u8> {
    radial(|d, n| {
        let edge = 0.8 + 0.2 * n;
        if d < 0.35 * edge {
            [8, 8, 8, 255]
        } else if d < 0.9 * edge {
            [40, 38, 36, (255.0 * (1.0 - d / edge)) as u8]
        } else {
            [0, 0, 0, 0]
        }
    })
}

/// RGBA8 bytes of a scorch: soot fading out to nothing.
pub fn scorch_pixels() -> Vec<u8> {
    radial(|d, n| {
        let a = ((1.0 - d).clamp(0.0, 1.0) * (0.85 + 0.15 * n) * 235.0) as u8;
        [14, 12, 10, a]
    })
}

/// A TEX×TEX image whose texel colour is `f(distance from centre in 0..=1+, noise in 0..1)`.
fn radial(f: impl Fn(f32, f32) -> [u8; 4]) -> Vec<u8> {
    let mut rng = Rng::new(0xDECA1);
    let c = (TEX as f32 - 1.0) / 2.0;
    (0..TEX)
        .flat_map(|y| (0..TEX).map(move |x| (x, y)))
        .flat_map(|(x, y)| {
            let d = ((x as f32 - c).powi(2) + (y as f32 - c).powi(2)).sqrt() / (c + 0.5);
            f(d, rng.unit())
        })
        .collect()
}

fn decal_image(pixels: Vec<u8>) -> Image {
    let mut image = Image::new(
        Extent3d {
            width: TEX,
            height: TEX,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        pixels,
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::RENDER_WORLD,
    );
    image.sampler = ImageSampler::Descriptor(ImageSamplerDescriptor {
        address_mode_u: ImageAddressMode::ClampToEdge,
        address_mode_v: ImageAddressMode::ClampToEdge,
        ..ImageSamplerDescriptor::nearest()
    });
    image
}

fn decal_material(texture: Handle<Image>) -> StandardMaterial {
    StandardMaterial {
        base_color_texture: Some(texture),
        alpha_mode: AlphaMode::Blend,
        unlit: true,
        cull_mode: None,
        ..default()
    }
}

/// The nearest surface within `radius` of an explosion at `point` in `sector`, found by tracing
/// straight down, along the four horizontals and down the four 45° diagonals.
pub fn scorch_site(map: &Map, point: Vec3, sector: SectorId, radius: f32) -> Option<Hit> {
    let s = std::f32::consts::FRAC_1_SQRT_2;
    let dirs = [
        Vec3::NEG_Z,
        Vec3::X,
        Vec3::NEG_X,
        Vec3::Y,
        Vec3::NEG_Y,
        Vec3::new(s, 0.0, -s),
        Vec3::new(-s, 0.0, -s),
        Vec3::new(0.0, s, -s),
        Vec3::new(0.0, -s, -s),
    ];
    dirs.iter()
        .filter_map(|&dir| {
            trace_world(
                map,
                &Ray {
                    origin: point,
                    dir,
                    sector,
                    max: radius,
                },
            )
        })
        .filter(|h| {
            matches!(
                h.kind,
                rr_core::trace::HitKind::Floor(_)
                    | rr_core::trace::HitKind::Wall(_)
                    | rr_core::trace::HitKind::Ceiling(_)
            )
        })
        .min_by(|a, b| a.dist.total_cmp(&b.dist))
}

/// Orientation of a quad facing along the core `normal`, spun by `spin` about it.
fn decal_rotation(normal: Vec3, spin: f32) -> Quat {
    Quat::from_rotation_arc(Vec3::Z, to_bevy(normal).normalize()) * Quat::from_rotation_z(spin)
}

/// One decal per `Impact`, and a scorch per `Explosion` that has a surface in reach. An
/// [`FxReaders`] system.
#[allow(clippy::too_many_arguments)]
fn spawn_decals(
    mut commands: Commands,
    fx: Res<FxQueue>,
    map: Res<CurrentMap>,
    mut ring: ResMut<DecalRing>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut images: ResMut<Assets<Image>>,
    mut assets: Local<Option<DecalAssets>>,
    mut rng: Local<Option<Rng>>,
) {
    let mut places: Vec<(Vec3, Vec3, SectorId, f32, bool)> = Vec::new();
    for ev in &fx.combat {
        match ev {
            CombatEvent::Impact {
                point,
                normal,
                sector,
            } => places.push((*point, *normal, *sector, HOLE_SIZE, false)),
            CombatEvent::Explosion { point, radius } => {
                let Some(sector) = map.0.find_sector(point.truncate(), None) else {
                    continue;
                };
                if let Some(hit) = scorch_site(&map.0, *point, sector, *radius) {
                    places.push((hit.point, hit.normal, hit.sector, SCORCH_SIZE, true));
                }
            }
            _ => {}
        }
    }
    if places.is_empty() {
        return;
    }
    let assets = assets.get_or_insert_with(|| DecalAssets {
        quad: meshes.add(Rectangle::new(1.0, 1.0)),
        hole: materials.add(decal_material(images.add(decal_image(hole_pixels())))),
        scorch: materials.add(decal_material(images.add(decal_image(scorch_pixels())))),
    });
    let rng = rng.get_or_insert_with(|| Rng::new(0xB0_11E7));
    for (point, normal, sector, size, scorch) in places {
        let spin = rng.unit() * std::f32::consts::TAU;
        let at = to_bevy(point + normal * SURFACE_OFFSET);
        let e = commands
            .spawn((
                Mesh3d(assets.quad.clone()),
                MeshMaterial3d(if scorch { &assets.scorch } else { &assets.hole }.clone()),
                Transform {
                    translation: at,
                    rotation: decal_rotation(normal, spin),
                    scale: Vec3::splat(size),
                },
                NotShadowCaster,
                Decal { sector },
                LevelEntity,
            ))
            .id();
        if let Some(old) = ring.push(e) {
            commands.entity(old).try_despawn();
        }
    }
}

/// Despawns the decals of every sector queued in [`DirtySectors`]. Runs before
/// `rebuild_dirty_sectors`, which empties the set.
fn purge_dirty_decals(
    mut commands: Commands,
    dirty: Res<DirtySectors>,
    mut ring: ResMut<DecalRing>,
    decals: Query<(Entity, &Decal)>,
) {
    if dirty.0.is_empty() {
        return;
    }
    for (e, d) in &decals {
        if dirty.0.contains(&d.sector) {
            commands.entity(e).try_despawn();
            ring.0.retain(|&r| r != e);
        }
    }
}

/// Bullet-hole and scorch decals. Needs a renderer's assets, `CombatSimPlugin` and
/// `LevelRenderPlugin`.
pub struct DecalsPlugin;

impl Plugin for DecalsPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<DecalRing>()
            .add_systems(Update, spawn_decals.in_set(FxReaders))
            .add_systems(
                Update,
                purge_dirty_decals
                    .after(spawn_decals)
                    .before(rebuild_dirty_sectors),
            );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rr_core::fixtures::two_rooms;
    use rr_core::trace::HitKind;

    #[test]
    fn scorch_goes_on_the_floor_below_within_radius() {
        let map = two_rooms(0.0, 4.0);
        let p = map.player_start;
        let at = Vec3::new(p.pos.0, p.pos.1, 0.8);
        let hit = scorch_site(&map, at, 0, 2.0).expect("floor is 0.8 m down");
        assert!(matches!(hit.kind, HitKind::Floor(0)));
        assert!((hit.dist - 0.8).abs() < 1e-3);
        assert!(hit.normal.z > 0.99);
    }

    #[test]
    fn scorch_skips_when_nothing_is_in_reach() {
        let map = two_rooms(0.0, 4.0);
        let p = map.player_start;
        let at = Vec3::new(p.pos.0, p.pos.1, 3.0);
        assert!(scorch_site(&map, at, 0, 1.0).is_none());
    }

    #[test]
    fn nearest_surface_wins() {
        let map = two_rooms(0.0, 4.0);
        let at = Vec3::new(0.3, 3.0, 2.0); // 0.3 m from the west wall, 2 m above the floor
        let hit = scorch_site(&map, at, 0, 3.0).expect("wall in reach");
        assert!(matches!(hit.kind, HitKind::Wall(_)), "{:?}", hit.kind);
    }

    #[test]
    fn ring_evicts_oldest_past_cap() {
        let mut ring = DecalRing::default();
        for i in 0..RING_CAP as u32 {
            assert!(ring.push(Entity::from_raw_u32(i).unwrap()).is_none());
        }
        let evicted = ring.push(Entity::from_raw_u32(999).unwrap());
        assert_eq!(evicted, Entity::from_raw_u32(0));
        assert_eq!(ring.len(), RING_CAP);
    }

    #[test]
    fn textures_have_alpha_and_are_deterministic() {
        for p in [hole_pixels(), scorch_pixels()] {
            assert_eq!(p.len(), (TEX * TEX * 4) as usize);
        }
        assert_eq!(hole_pixels(), hole_pixels());
        let h = hole_pixels();
        assert_eq!(h[3], 0, "corner is transparent");
        let mid = ((TEX / 2 * TEX + TEX / 2) * 4) as usize;
        assert_eq!(h[mid + 3], 255, "centre is opaque");
    }
}
