//! Loads the map and spawns its meshes, materials and lights.

use crate::coords::{to_bevy, to_bevy_arr};
use crate::flow::{LevelEntity, SpawnLevel};
use crate::mechanics::DirtySectors;
use crate::paths::assets_dir;
use crate::textures;
use bevy::asset::RenderAssetUsages;
use bevy::light::{GlobalAmbientLight, NotShadowCaster};
use bevy::mesh::Indices;
use bevy::prelude::*;
use bevy::render::render_resource::PrimitiveTopology;
use rr_core::extrude::{MeshData, extrude_sector};
use rr_core::map::{Map, SectorId};
use std::path::{Path, PathBuf};

#[derive(Resource)]
pub struct CurrentMap(pub Map);

/// Marks the render meshes of one sector (lets later milestones re-extrude moving sectors).
#[derive(Component)]
pub struct SectorMesh(pub SectorId);

/// Material handles indexed like `Map::materials`, kept so moving sectors can be re-extruded.
#[derive(Resource)]
pub struct LevelMaterials(pub Vec<Handle<StandardMaterial>>);

/// A level argument is either an existing file path or a file name under `assets/levels/`.
pub fn level_path(arg: &str, assets: &Path) -> PathBuf {
    let direct = Path::new(arg);
    if direct.is_file() {
        direct.to_path_buf()
    } else {
        assets.join("levels").join(arg)
    }
}

pub fn load_map(file: &str) -> Map {
    let path = level_path(file, &assets_dir());
    let src = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("cannot read level {}: {e}", path.display()));
    Map::from_ron(&src).unwrap_or_else(|e| panic!("invalid level {}: {e}", path.display()))
}

pub fn to_bevy_mesh(m: &MeshData) -> Mesh {
    Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::default(),
    )
    .with_inserted_attribute(
        Mesh::ATTRIBUTE_POSITION,
        m.positions
            .iter()
            .map(|&p| to_bevy_arr(p))
            .collect::<Vec<_>>(),
    )
    .with_inserted_attribute(
        Mesh::ATTRIBUTE_NORMAL,
        m.normals
            .iter()
            .map(|&n| to_bevy_arr(n))
            .collect::<Vec<_>>(),
    )
    .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, m.uvs.clone())
    .with_inserted_indices(Indices::U32(m.indices.clone()))
}

pub struct LevelRenderPlugin;

impl Plugin for LevelRenderPlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(ClearColor(Color::srgb(0.16, 0.23, 0.47)))
            .insert_resource(GlobalAmbientLight {
                brightness: 250.0,
                ..default()
            })
            .init_resource::<DirtySectors>()
            .add_systems(SpawnLevel, spawn_level)
            .add_systems(Update, rebuild_dirty_sectors);
    }
}

fn spawn_sector(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    map: &Map,
    mats: &[Handle<StandardMaterial>],
    s: SectorId,
) {
    match extrude_sector(map, s) {
        Ok(subs) => {
            for sub in subs {
                let mut e = commands.spawn((
                    Mesh3d(meshes.add(to_bevy_mesh(&sub.mesh))),
                    MeshMaterial3d(mats[sub.material].clone()),
                    SectorMesh(s),
                    LevelEntity,
                ));
                if map.materials[sub.material] == "sky" {
                    e.insert(NotShadowCaster);
                }
            }
        }
        Err(err) => error!("{}: {err}", map.name),
    }
}

/// Re-extrudes every sector whose heights changed, plus its neighbours (their step faces
/// depend on it), replacing the old mesh entities.
pub fn rebuild_dirty_sectors(
    mut commands: Commands,
    map: Res<CurrentMap>,
    mats: Res<LevelMaterials>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut dirty: ResMut<DirtySectors>,
    existing: Query<(Entity, &SectorMesh)>,
) {
    if dirty.0.is_empty() {
        return;
    }
    let mut todo = std::mem::take(&mut dirty.0);
    let around: Vec<SectorId> = todo.iter().flat_map(|&s| map.0.neighbours(s)).collect();
    todo.extend(around);
    for (e, m) in &existing {
        if todo.contains(&m.0) {
            commands.entity(e).despawn();
        }
    }
    for &s in &todo {
        spawn_sector(&mut commands, &mut meshes, &map.0, &mats.0, s);
    }
}

/// Marks the point light of `Map::lights[.0]`, so a broken fixture's light can be found.
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
pub struct LevelLight(pub usize);

/// Sector meshes and lights (in [`SpawnLevel`]). The materials are generated on the first spawn
/// and reused by restarts of the same level.
fn spawn_level(
    mut commands: Commands,
    map: Res<CurrentMap>,
    existing: Option<Res<LevelMaterials>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut images: ResMut<Assets<Image>>,
) {
    let map = &map.0;
    let mats: Vec<Handle<StandardMaterial>> = match existing {
        Some(m) => m.0.clone(),
        None => {
            let mats: Vec<_> = map
                .materials
                .iter()
                .map(|name| {
                    let texture = images.add(textures::generate(name));
                    materials.add(StandardMaterial {
                        base_color_texture: Some(texture),
                        perceptual_roughness: 0.9,
                        unlit: name == "sky",
                        ..default()
                    })
                })
                .collect();
            commands.insert_resource(LevelMaterials(mats.clone()));
            mats
        }
    };

    for s in 0..map.sectors.len() {
        spawn_sector(&mut commands, &mut meshes, map, &mats, s);
    }

    for (i, l) in map.lights.iter().enumerate() {
        commands.spawn((
            PointLight {
                color: Color::srgb(l.color.0, l.color.1, l.color.2),
                intensity: l.intensity,
                range: l.range,
                shadow_maps_enabled: true,
                ..default()
            },
            Transform::from_translation(to_bevy(Vec3::new(l.pos.0, l.pos.1, l.pos.2))),
            LevelLight(i),
            LevelEntity,
        ));
    }

    commands.spawn((
        DirectionalLight {
            illuminance: 3000.0,
            shadow_maps_enabled: true,
            ..default()
        },
        Transform::from_xyz(0.0, 10.0, 0.0).looking_at(Vec3::new(0.4, 0.0, -0.3), Vec3::Y),
        LevelEntity,
    ));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn level_arg_is_a_name_under_assets_or_an_existing_path() {
        let assets = Path::new("/nonexistent/assets");
        assert_eq!(
            level_path("test_yard.ron", assets),
            Path::new("/nonexistent/assets/levels/test_yard.ron")
        );
        let real = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/levels/test_yard.ron");
        assert!(real.is_file());
        let arg = real.to_str().unwrap();
        assert_eq!(level_path(arg, assets), real);
    }
}
