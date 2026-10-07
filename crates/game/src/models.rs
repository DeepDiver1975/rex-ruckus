//! glTF models: the shared plumbing behind enemies, viewmodel weapons and pickups.
//!
//! [`ModelLibrary`] is loaded once at startup from `assets/defs/models.ron` ([`load_model_defs`]) and survives restarts:
//! one scene handle per enemy, weapon, item and extra, plus per enemy an [`AnimationGraph`] whose
//! [`ClipRole`] nodes are resolved by clip name once the enemy's `Gltf` has loaded
//! ([`build_graphs`]). [`spawn_model`] parents a scene to an entity with its [`Placement`]; when
//! the scene instance is ready, [`on_model_ready`] records the useful entities in [`ModelReady`]
//! and binds the enemy's graph to its `AnimationPlayer` (here or, if the graph is built later, in
//! [`bind_late_graphs`]). [`TintCache`] hands out per-[`Look`] variants of the glTF materials.

use crate::flow::run_spawn_level;
use crate::paths::assets_dir;
use bevy::gltf::{Gltf, GltfAssetLabel};
use bevy::light::NotShadowCaster;
use bevy::platform::collections::HashMap;
use bevy::prelude::*;
use bevy::world_serialization::{WorldAsset, WorldInstanceReady};
use rr_core::defs::WeaponId;
use rr_core::map::{ActorKind, ItemKind};
use rr_core::models::{Clips, ItemKindModel, ModelDefs, Placement};
use rr_core::props::PropKind;

/// Loads the model library, builds animation graphs and binds them to spawned enemy models.
pub struct ModelsPlugin;

impl Plugin for ModelsPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<TintCache>()
            .add_systems(Startup, load_library.before(run_spawn_level))
            .add_systems(
                Update,
                (build_graphs, bind_late_graphs)
                    .chain()
                    .run_if(resource_exists::<ModelLibrary>),
            )
            .add_observer(on_model_ready);
    }
}

/// What an enemy animation clip is for; [`Clips`] names the glTF clip of each role.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ClipRole {
    Idle,
    Walk,
    Run,
    Attack,
    Pain,
    Death,
}

impl ClipRole {
    pub const ALL: [ClipRole; 6] = [
        ClipRole::Idle,
        ClipRole::Walk,
        ClipRole::Run,
        ClipRole::Attack,
        ClipRole::Pain,
        ClipRole::Death,
    ];

    /// The glTF clip name for this role, if the model has one.
    pub fn name(self, clips: &Clips) -> Option<&str> {
        match self {
            ClipRole::Idle => &clips.idle,
            ClipRole::Walk => &clips.walk,
            ClipRole::Run => &clips.run,
            ClipRole::Attack => &clips.attack,
            ClipRole::Pain => &clips.pain,
            ClipRole::Death => &clips.death,
        }
        .as_deref()
    }
}

/// Graph node of each clip role an enemy model has.
pub type ClipNodes = HashMap<ClipRole, AnimationNodeIndex>;

/// A loaded scene and where it sits relative to its parent.
#[derive(Clone, Debug)]
pub struct ModelScene {
    pub scene: Handle<WorldAsset>,
    pub place: Placement,
}

/// A second scene to parent to a named bone of an enemy (e.g. the grunt's pistol).
#[derive(Clone, Debug)]
pub struct AttachScene {
    pub bone: String,
    pub model: ModelScene,
}

/// An enemy's animation graph and the node (and clip) of each clip role it has.
#[derive(Clone, Debug)]
pub struct EnemyGraph {
    pub graph: Handle<AnimationGraph>,
    pub nodes: ClipNodes,
    /// The clip behind each node, e.g. to read its duration from `Assets<AnimationClip>`.
    pub clips: HashMap<ClipRole, Handle<AnimationClip>>,
}

/// Everything loaded for one enemy kind.
#[derive(Clone, Debug)]
pub struct EnemyAssets {
    pub gltf: Handle<Gltf>,
    pub model: ModelScene,
    pub attach: Option<AttachScene>,
    /// sRGB multiplier for the `Normal`/`Dim` looks (see [`TintCache::get`]).
    pub tint: Option<(f32, f32, f32)>,
    /// `None` until the `Gltf` has loaded and [`build_graphs`] has run.
    pub graph: Option<EnemyGraph>,
}

/// Handles of every model in [`ModelDefs`]; built at startup, survives restarts.
#[derive(Resource)]
pub struct ModelLibrary {
    pub defs: ModelDefs,
    enemies: HashMap<ActorKind, EnemyAssets>,
    weapons: HashMap<WeaponId, ModelScene>,
    items: HashMap<ItemKindModel, ModelScene>,
    props: HashMap<PropKind, ModelScene>,
    pub held_bomb: ModelScene,
    pub detonator: ModelScene,
}

impl ModelLibrary {
    /// Starts loading every model in `defs`. Panics if `defs` fails [`ModelDefs::validate`]:
    /// the accessors below rely on its coverage guarantees.
    pub fn load(defs: ModelDefs, server: &AssetServer) -> Self {
        if let Err(e) = defs.validate() {
            panic!("invalid model definitions: {e}");
        }
        let scene = |path: &str, place: &Placement| ModelScene {
            scene: server.load(GltfAssetLabel::Scene(0).from_asset(path.to_owned())),
            place: *place,
        };
        let enemies = defs
            .enemies
            .iter()
            .map(|e| {
                let assets = EnemyAssets {
                    gltf: server.load(e.scene.clone()),
                    model: scene(&e.scene, &e.place),
                    attach: e.attach.as_ref().map(|a| AttachScene {
                        bone: a.bone.clone(),
                        model: scene(&a.scene, &a.place),
                    }),
                    tint: e.tint,
                    graph: None,
                };
                (e.kind, assets)
            })
            .collect();
        let weapons = defs
            .weapons
            .iter()
            .map(|w| (w.id, scene(&w.scene, &w.place)))
            .collect();
        let items = defs
            .items
            .iter()
            .map(|i| (i.kind, scene(&i.scene, &i.place)))
            .collect();
        let props = defs
            .props
            .iter()
            .map(|p| (p.kind, scene(&p.scene, &p.place)))
            .collect();
        ModelLibrary {
            held_bomb: scene(&defs.extras.held_bomb.scene, &defs.extras.held_bomb.place),
            detonator: scene(&defs.extras.detonator.scene, &defs.extras.detonator.place),
            enemies,
            weapons,
            items,
            props,
            defs,
        }
    }

    pub fn enemy(&self, kind: ActorKind) -> &EnemyAssets {
        &self.enemies[&kind]
    }

    /// Mutable access, e.g. for tests that install a hand-built graph.
    pub fn enemy_mut(&mut self, kind: ActorKind) -> &mut EnemyAssets {
        self.enemies
            .get_mut(&kind)
            .expect("every enemy kind is loaded")
    }

    /// `None` for weapons that stay code-built (the boot).
    pub fn weapon(&self, id: WeaponId) -> Option<&ModelScene> {
        self.weapons.get(&id)
    }

    /// Every keycard colour shares one scene.
    pub fn item(&self, kind: ItemKind) -> &ModelScene {
        &self.items[&ItemKindModel::of(kind)]
    }

    /// `None` for props that stay code-built.
    pub fn prop(&self, kind: PropKind) -> Option<&ModelScene> {
        self.props.get(&kind)
    }

    /// The graph node playing `role` for `kind`; `None` until the graph is built or if the
    /// model has no such clip.
    pub fn clip_node(&self, kind: ActorKind, role: ClipRole) -> Option<AnimationNodeIndex> {
        self.enemy(kind).graph.as_ref()?.nodes.get(&role).copied()
    }
}

/// Loads `assets/defs/models.ron`, panicking with the path on failure (the library then
/// validates it).
pub fn load_model_defs() -> ModelDefs {
    let path = assets_dir().join("defs").join("models.ron");
    let text = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("cannot read defs {}: {e}", path.display()));
    ModelDefs::from_ron(&text)
        .unwrap_or_else(|e| panic!("invalid defs in {}: {e:?}", path.display()))
}

fn load_library(mut commands: Commands, server: Res<AssetServer>) {
    commands.insert_resource(ModelLibrary::load(load_model_defs(), &server));
}

/// Builds each enemy's animation graph once its `Gltf` is loaded, resolving clips by name.
/// Tolerates apps without glTF support (headless tests): the graphs then stay unbuilt.
pub fn build_graphs(
    mut lib: ResMut<ModelLibrary>,
    gltfs: Option<Res<Assets<Gltf>>>,
    graphs: Option<ResMut<Assets<AnimationGraph>>>,
) {
    let (Some(gltfs), Some(mut graphs)) = (gltfs, graphs) else {
        return;
    };
    if lib.enemies.values().all(|e| e.graph.is_some()) {
        return;
    }
    let lib = &mut *lib;
    for model in &lib.defs.enemies {
        let enemy = lib.enemies.get_mut(&model.kind).expect("loaded from defs");
        if enemy.graph.is_some() {
            continue;
        }
        let Some(gltf) = gltfs.get(&enemy.gltf) else {
            continue;
        };
        let mut roles = Vec::new();
        let mut clips = Vec::new();
        for role in ClipRole::ALL {
            let Some(name) = role.name(&model.clips) else {
                continue;
            };
            match gltf.named_animations.get(name) {
                Some(clip) => {
                    roles.push(role);
                    clips.push(clip.clone());
                }
                None => warn!("{}: no animation clip named {name:?}", model.scene),
            }
        }
        let (graph, nodes) = AnimationGraph::from_clips(clips.iter().cloned());
        enemy.graph = Some(EnemyGraph {
            graph: graphs.add(graph),
            nodes: roles.iter().copied().zip(nodes).collect(),
            clips: roles.into_iter().zip(clips).collect(),
        });
    }
}

/// Marks a scene spawned by [`spawn_model`]; set `enemy` to have its animation graph bound.
#[derive(Component, Clone, Copy, Debug, Default)]
pub struct ModelSlot {
    /// Bind this enemy kind's animation graph to the scene's `AnimationPlayer`.
    pub enemy: Option<ActorKind>,
    /// Meshes cast no shadows (the viewmodel).
    pub no_shadows: bool,
}

/// Added to a [`ModelSlot`] root once its scene instance has spawned.
#[derive(Component, Clone, Debug, Default)]
pub struct ModelReady {
    /// The entity with the `AnimationPlayer`, if the scene is animated. Once the enemy's graph
    /// is bound it also carries `AnimationGraphHandle` and `AnimationTransitions`.
    pub player: Option<Entity>,
    /// Every entity with a `MeshMaterial3d<StandardMaterial>` (for [`TintCache`] swaps).
    pub meshes: Vec<Entity>,
    /// Descendants by glTF node `Name` (bones, `Muzzle`, ...).
    pub nodes: HashMap<String, Entity>,
}

/// Spawns `scene` as a child of `parent` at `place` and returns the model root. The root gets
/// a default [`ModelSlot`]; use [`spawn_model_with`] to bind an enemy graph or drop shadows.
pub fn spawn_model(
    commands: &mut Commands,
    parent: Entity,
    scene: &Handle<WorldAsset>,
    place: &Placement,
) -> Entity {
    spawn_model_with(commands, parent, scene, place, ModelSlot::default())
}

/// [`spawn_model`] with an explicit [`ModelSlot`].
pub fn spawn_model_with(
    commands: &mut Commands,
    parent: Entity,
    scene: &Handle<WorldAsset>,
    place: &Placement,
    slot: ModelSlot,
) -> Entity {
    commands
        .spawn((
            WorldAssetRoot(scene.clone()),
            placement_transform(place),
            slot,
            ChildOf(parent),
        ))
        .id()
}

/// Scale, then yaw (about Y), pitch (X), roll (Z) in degrees, then translate by `offset`.
pub fn placement_transform(p: &Placement) -> Transform {
    Transform {
        translation: Vec3::from(p.offset),
        rotation: Quat::from_euler(
            EulerRot::YXZ,
            p.yaw_deg.to_radians(),
            p.pitch_deg.to_radians(),
            p.roll_deg.to_radians(),
        ),
        scale: Vec3::splat(p.scale),
    }
}

/// Any punctual light a glTF can bring along.
type AnyLight = Or<(With<DirectionalLight>, With<PointLight>, With<SpotLight>)>;

/// Collects a ready model's player, meshes and named nodes; binds the enemy graph if built.
/// Lights exported with a model (the drone ships two suns) are despawned: the level lights it.
#[allow(clippy::too_many_arguments)]
fn on_model_ready(
    ev: On<WorldInstanceReady>,
    mut commands: Commands,
    slots: Query<&ModelSlot>,
    lib: Option<Res<ModelLibrary>>,
    children: Query<&Children>,
    players: Query<(), With<AnimationPlayer>>,
    meshes: Query<(), With<MeshMaterial3d<StandardMaterial>>>,
    names: Query<&Name>,
    lights: Query<(), AnyLight>,
) {
    let root = ev.entity;
    let Ok(slot) = slots.get(root) else {
        return;
    };
    let mut ready = ModelReady::default();
    for e in children.iter_descendants(root) {
        if lights.contains(e) {
            commands.entity(e).despawn();
            continue;
        }
        if ready.player.is_none() && players.contains(e) {
            ready.player = Some(e);
        }
        if meshes.contains(e) {
            ready.meshes.push(e);
            if slot.no_shadows {
                commands.entity(e).insert(NotShadowCaster);
            }
        }
        if let Ok(name) = names.get(e) {
            ready.nodes.insert(name.as_str().to_owned(), e);
        }
    }
    if let (Some(kind), Some(player), Some(lib)) = (slot.enemy, ready.player, lib)
        && let Some(graph) = &lib.enemy(kind).graph
    {
        bind_graph(&mut commands, player, graph);
    }
    commands.entity(root).insert(ready);
}

/// Binds graphs to enemy models that became ready before their graph was built.
fn bind_late_graphs(
    mut commands: Commands,
    lib: Res<ModelLibrary>,
    models: Query<(&ModelSlot, &ModelReady)>,
    unbound: Query<(), (With<AnimationPlayer>, Without<AnimationGraphHandle>)>,
) {
    for (slot, ready) in &models {
        if let (Some(kind), Some(player)) = (slot.enemy, ready.player)
            && unbound.contains(player)
            && let Some(graph) = &lib.enemy(kind).graph
        {
            bind_graph(&mut commands, player, graph);
        }
    }
}

fn bind_graph(commands: &mut Commands, player: Entity, graph: &EnemyGraph) {
    commands.entity(player).insert((
        AnimationGraphHandle(graph.graph.clone()),
        AnimationTransitions::new(),
    ));
}

/// How an enemy's body is drawn; mirrors the procedural rigs' skins.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Look {
    Normal,
    /// Asleep: darker.
    Dim,
    /// Reddened, with a soft red glow.
    Pain,
    /// Brightened, with a soft warm-white glow, after a hit.
    Hit,
    /// A phase-2 boss's pulse: the `Normal` skin (texture and tint kept) plus a red glow.
    Pulse,
}

/// Brightness of the `Dim` look relative to `Normal`.
const DIM: f32 = 0.45;

/// Colour and strength of the `Pulse` look's emissive.
const PULSE_GLOW: (f32, f32, f32) = (1.0, 0.1, 0.05);
const PULSE_STRENGTH: f32 = 0.8;

/// A flash look: sRGB colour multiplier, glow colour, glow strength.
type Flash = ((f32, f32, f32), (f32, f32, f32), f32);

/// Pain and Hit multiply the colour (the texture stays) and add a soft glow on top.
const PAIN: Flash = ((1.4, 0.35, 0.3), (0.85, 0.15, 0.1), 0.35);
const HIT: Flash = ((1.5, 1.4, 1.3), (1.0, 0.85, 0.7), 0.4);

/// `base` as drawn with `look`; `tint` (sRGB multiplier) applies to every look.
/// Textures, roughness and alpha are kept. `Pain` and `Hit` multiply the colour (red, or
/// bright warm-white) and add a soft emissive on top, so the model's detail still reads;
/// `Pulse` keeps the `Normal` colour and only adds a red emissive.
pub fn tinted(
    base: &StandardMaterial,
    look: Look,
    tint: Option<(f32, f32, f32)>,
) -> StandardMaterial {
    let mut m = base.clone();
    let scale = |m: &mut StandardMaterial, k: (f32, f32, f32)| {
        let c = m.base_color.to_srgba();
        m.base_color = Color::srgba(c.red * k.0, c.green * k.1, c.blue * k.2, c.alpha);
    };
    let (r, g, b) = tint.unwrap_or((1.0, 1.0, 1.0));
    let flash = |m: &mut StandardMaterial, (k, glow, strength): Flash| {
        scale(m, (r * k.0, g * k.1, b * k.2));
        m.emissive = Color::srgb(glow.0, glow.1, glow.2).to_linear() * strength;
    };
    match look {
        Look::Normal => scale(&mut m, (r, g, b)),
        Look::Dim => scale(&mut m, (r * DIM, g * DIM, b * DIM)),
        Look::Pain => flash(&mut m, PAIN),
        Look::Hit => flash(&mut m, HIT),
        Look::Pulse => {
            scale(&mut m, (r, g, b));
            let (pr, pg, pb) = PULSE_GLOW;
            m.emissive = Color::srgb(pr, pg, pb).to_linear() * PULSE_STRENGTH;
            m.emissive_texture = None;
        }
    }
    m
}

/// Look variants of glTF materials, created on first use and kept across restarts.
#[derive(Resource, Default)]
pub struct TintCache {
    variants: HashMap<(AssetId<StandardMaterial>, Look, [u32; 3]), Handle<StandardMaterial>>,
}

impl TintCache {
    /// The `look` variant of `source` (see [`tinted`]). An untinted `Normal` is `source`
    /// itself; so is every look while `source` has not loaded yet.
    pub fn get(
        &mut self,
        materials: &mut Assets<StandardMaterial>,
        source: &Handle<StandardMaterial>,
        look: Look,
        tint: Option<(f32, f32, f32)>,
    ) -> Handle<StandardMaterial> {
        if look == Look::Normal && tint.is_none() {
            return source.clone();
        }
        let (r, g, b) = tint.unwrap_or((1.0, 1.0, 1.0));
        let key = (source.id(), look, [r.to_bits(), g.to_bits(), b.to_bits()]);
        if let Some(h) = self.variants.get(&key) {
            return h.clone();
        }
        let Some(base) = materials.get(source) else {
            return source.clone();
        };
        let h = materials.add(tinted(base, look, tint));
        self.variants.insert(key, h.clone());
        h
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: Color, b: Color) -> bool {
        let (a, b) = (a.to_srgba(), b.to_srgba());
        (a.red - b.red).abs() < 1e-5
            && (a.green - b.green).abs() < 1e-5
            && (a.blue - b.blue).abs() < 1e-5
            && (a.alpha - b.alpha).abs() < 1e-5
    }

    fn base() -> StandardMaterial {
        StandardMaterial {
            base_color: Color::srgba(0.5, 0.4, 0.2, 0.9),
            perceptual_roughness: 0.3,
            ..default()
        }
    }

    #[test]
    fn normal_without_tint_is_unchanged() {
        let m = tinted(&base(), Look::Normal, None);
        assert!(close(m.base_color, base().base_color));
        assert_eq!(m.perceptual_roughness, 0.3);
    }

    #[test]
    fn pulse_keeps_the_skin_and_adds_a_red_glow() {
        let tex = Handle::<Image>::default();
        let src = StandardMaterial {
            base_color_texture: Some(tex.clone()),
            ..base()
        };
        let p = tinted(&src, Look::Pulse, None);
        assert!(close(p.base_color, base().base_color), "base colour kept");
        assert_eq!(p.base_color_texture, Some(tex), "texture kept");
        assert_eq!(p.perceptual_roughness, 0.3);
        let e = p.emissive;
        assert!(e.red > 0.5, "glows: {e:?}");
        assert!(e.red > 5.0 * e.green && e.red > 5.0 * e.blue, "red: {e:?}");
        // The enemy tint still applies, as for `Normal`.
        let t = tinted(&src, Look::Pulse, Some((1.0, 0.5, 2.0)));
        assert!(close(t.base_color, Color::srgba(0.5, 0.2, 0.4, 0.9)));
    }

    #[test]
    fn tint_multiplies_srgb_and_dim_darkens() {
        let t = Some((1.0, 0.5, 2.0));
        let n = tinted(&base(), Look::Normal, t);
        assert!(close(n.base_color, Color::srgba(0.5, 0.2, 0.4, 0.9)));
        let d = tinted(&base(), Look::Dim, t);
        assert!(close(
            d.base_color,
            Color::srgba(0.5 * DIM, 0.2 * DIM, 0.4 * DIM, 0.9)
        ));
        assert_eq!(d.emissive, base().emissive);
    }

    #[test]
    fn pain_and_hit_tint_the_texture_and_glow_softly() {
        let b = StandardMaterial {
            base_color_texture: Some(Handle::<Image>::default()),
            ..base()
        };
        for look in [Look::Pain, Look::Hit] {
            let m = tinted(&b, look, Some((0.5, 0.5, 0.5)));
            // The texture stays, so the model's detail still reads.
            assert_eq!(m.base_color_texture, b.base_color_texture);
            assert_eq!(m.emissive_texture, b.emissive_texture);
            // The glow is a hint, not a flood.
            let e = m.emissive;
            assert!(e.red.max(e.green).max(e.blue) <= 0.6, "{look:?}: {e:?}");
            assert_eq!(m.perceptual_roughness, 0.3);
        }
        // Pain reads red: red dominates the multiplied colour.
        let p = tinted(&b, Look::Pain, None).base_color.to_srgba();
        assert!(p.red > 2.0 * p.green && p.red > 2.0 * p.blue, "{p:?}");
        // The tint still applies to Hit.
        let a = tinted(&b, Look::Hit, Some((0.5, 0.5, 0.5)))
            .base_color
            .to_srgba();
        let n = tinted(&b, Look::Hit, None).base_color.to_srgba();
        assert!(a.red < n.red);
    }

    #[test]
    fn placement_scales_rotates_then_offsets() {
        let p = Placement {
            scale: 2.0,
            yaw_deg: 90.0,
            pitch_deg: 0.0,
            roll_deg: 0.0,
            offset: (1.0, 0.0, 0.0),
        };
        let t = placement_transform(&p);
        // Model-space -Z (forward) becomes -X after a 90° yaw, scaled then offset.
        let v = t.transform_point(Vec3::new(0.0, 0.0, -1.0));
        assert!(v.abs_diff_eq(Vec3::new(-1.0, 0.0, 0.0), 1e-5), "{v}");
    }

    #[test]
    fn every_enemy_role_maps_to_its_clip_field() {
        let clips = Clips {
            idle: Some("i".into()),
            death: Some("d".into()),
            ..default()
        };
        let named: Vec<_> = ClipRole::ALL
            .iter()
            .filter_map(|r| r.name(&clips).map(|n| (*r, n)))
            .collect();
        assert_eq!(named, [(ClipRole::Idle, "i"), (ClipRole::Death, "d")]);
    }

    #[test]
    fn tint_cache_reuses_variants() {
        let mut materials = Assets::<StandardMaterial>::default();
        let src = materials.add(base());
        let mut cache = TintCache::default();
        assert_eq!(
            cache.get(&mut materials, &src, Look::Normal, None).id(),
            src.id()
        );
        let a = cache.get(&mut materials, &src, Look::Pain, None);
        let b = cache.get(&mut materials, &src, Look::Pain, None);
        assert_eq!(a.id(), b.id());
        assert_ne!(a.id(), src.id());
        let c = cache.get(&mut materials, &src, Look::Normal, Some((0.5, 0.5, 0.5)));
        assert_ne!(c.id(), src.id());
        assert_ne!(c.id(), a.id());
    }
}
