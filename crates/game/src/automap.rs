//! The automap overlay: Tab shows the walls seen so far, turned with the player (forward up),
//! over the running game (it does not pause). The sim half (`AutomapSimPlugin`) keeps the
//! reveal state and the toggle and runs headless; `AutomapRenderPlugin` draws it with gizmos on
//! their own render layer through a `Camera2d` above the 3D view and the UI.

use crate::combat::eye_of;
use crate::flow::{PlayState, SpawnLevel};
use crate::level::CurrentMap;
use crate::mechanics::LevelMechanics;
use crate::menu::MenuScreen;
use crate::player::{Look, PendingInput, Player, PlayerBody, PlayerSimSet, spawn_player};
use bevy::camera::ClearColorConfig;
use bevy::camera::visibility::RenderLayers;
use bevy::prelude::*;
use rr_core::automap::{Automap, LineKind, automap_lines, to_screen};

/// Render layer of the automap gizmos (nothing else uses it).
pub const AUTOMAP_LAYER: usize = 7;
/// Fixed ticks between reveal passes.
pub const REVEAL_EVERY: u32 = 6;
/// Automap zoom.
pub const PX_PER_M: f32 = 12.0;
/// Above the HUD, under the damage flash (z 5), the death and complete overlays and the menus.
const BACKDROP_Z: i32 = 4;

/// Whether the automap is up. Toggled by `PendingInput::toggle_map` while playing.
#[derive(Resource, Default, Debug, PartialEq, Eq)]
pub struct AutomapView {
    pub open: bool,
}

/// What this level run has revealed; rebuilt for every (re)spawn of a level.
#[derive(Resource)]
pub struct LevelAutomap(pub Automap);

/// The gizmo group the automap lines are drawn in (on [`AUTOMAP_LAYER`] only).
#[derive(Default, Reflect, GizmoConfigGroup)]
pub struct AutomapGizmos;

/// The 2D camera that draws [`AutomapGizmos`]; active only while the map is shown.
#[derive(Component)]
pub struct AutomapCamera;

/// The dimming full-screen node under the map lines.
#[derive(Component)]
pub struct AutomapBackdrop;

/// Reveal state and the toggle; headless.
pub struct AutomapSimPlugin;

impl Plugin for AutomapSimPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<AutomapView>()
            .add_systems(SpawnLevel, reset_automap.after(spawn_player))
            .add_systems(
                FixedUpdate,
                (
                    toggle_automap,
                    reveal.run_if(resource_equals(PlayState::Playing)),
                )
                    .after(PlayerSimSet),
            );
    }
}

/// A fresh, closed map for the level just spawned (start, restart or level switch), with the
/// start sector already revealed.
fn reset_automap(
    mut commands: Commands,
    map: Res<CurrentMap>,
    mut view: ResMut<AutomapView>,
    players: Query<&PlayerBody, With<Player>>,
) {
    let mut am = Automap::new(&map.0);
    for b in &players {
        am.reveal(&map.0, eye_of(&b.0), b.0.sector);
    }
    view.open = false;
    commands.insert_resource(LevelAutomap(am));
}

/// Consumes the latch every tick; only applies it while playing, so a press while paused (or
/// dead) is dropped rather than applied on resume.
fn toggle_automap(
    state: Res<PlayState>,
    mut view: ResMut<AutomapView>,
    mut q: Query<&mut PendingInput, With<Player>>,
) {
    for mut input in &mut q {
        if std::mem::take(&mut input.toggle_map) && *state == PlayState::Playing {
            view.open = !view.open;
        }
    }
}

/// Reveals what the player sees, every [`REVEAL_EVERY`] playing ticks.
fn reveal(
    mut tick: Local<u32>,
    map: Res<CurrentMap>,
    am: Option<ResMut<LevelAutomap>>,
    q: Query<&PlayerBody, With<Player>>,
) {
    *tick += 1;
    let Some(mut am) = am else { return };
    if !(*tick).is_multiple_of(REVEAL_EVERY) {
        return;
    }
    for b in &q {
        am.0.reveal(&map.0, eye_of(&b.0), b.0.sector);
    }
}

/// Draws the automap (window only; added by `GamePlugin`).
pub struct AutomapRenderPlugin;

impl Plugin for AutomapRenderPlugin {
    fn build(&self, app: &mut App) {
        app.init_gizmo_group::<AutomapGizmos>()
            .add_systems(Startup, setup_automap)
            .add_systems(
                Update,
                (
                    show_overlay,
                    draw_automap.run_if(|v: Res<AutomapView>| v.open),
                ),
            );
    }
}

fn setup_automap(mut commands: Commands, mut config_store: ResMut<GizmoConfigStore>) {
    let (config, _) = config_store.config_mut::<AutomapGizmos>();
    config.render_layers = RenderLayers::layer(AUTOMAP_LAYER);
    config.line.width = 2.0;
    // Always the window target: the low-res mode only shrinks the 3D view beneath it. Order 2
    // draws after the 3D camera (0) and the UI camera (1), without clearing them.
    commands.spawn((
        Camera2d,
        Camera {
            order: 2,
            clear_color: ClearColorConfig::None,
            is_active: false,
            ..default()
        },
        RenderLayers::layer(AUTOMAP_LAYER),
        AutomapCamera,
    ));
    // Drawn by the UI camera, over the HUD and under the damage flash.
    commands.spawn((
        Node {
            position_type: PositionType::Absolute,
            width: Val::Percent(100.0),
            height: Val::Percent(100.0),
            ..default()
        },
        BackgroundColor(Color::srgba(0.0, 0.02, 0.05, 0.7)),
        GlobalZIndex(BACKDROP_Z),
        Visibility::Hidden,
        AutomapBackdrop,
    ));
}

/// Shows the map while it is open during play, and never under a menu screen (the
/// `hide_under_menu` rule) or over the death and complete screens.
fn show_overlay(
    view: Res<AutomapView>,
    state: Res<PlayState>,
    screen: Option<Res<MenuScreen>>,
    mut cameras: Query<&mut Camera, With<AutomapCamera>>,
    mut backdrops: Query<&mut Visibility, With<AutomapBackdrop>>,
) {
    let shown = view.open && *state == PlayState::Playing && screen.is_none_or(|s| s.0.is_none());
    for mut camera in &mut cameras {
        if camera.is_active != shown {
            camera.is_active = shown;
        }
    }
    let vis = if shown {
        Visibility::Inherited
    } else {
        Visibility::Hidden
    };
    for mut v in &mut backdrops {
        v.set_if_neq(vis);
    }
}

fn draw_automap(
    map: Res<CurrentMap>,
    mech: Res<LevelMechanics>,
    am: Option<Res<LevelAutomap>>,
    player: Query<(&PlayerBody, &Look), With<Player>>,
    mut gizmos: Gizmos<AutomapGizmos>,
) {
    let (Some(am), Ok((body, look))) = (am, player.single()) else {
        return;
    };
    let me = body.0.pos.truncate();
    for l in automap_lines(&map.0, &mech.0, &am.0) {
        let color = match l.kind {
            LineKind::Solid => Color::srgb(0.9, 0.9, 0.85),
            LineKind::Step => Color::srgb(0.55, 0.55, 0.6),
            LineKind::Door(Some(k)) => crate::props::key_color(k),
            LineKind::Door(None) => Color::srgb(1.0, 0.65, 0.1),
            LineKind::Hazard(kind) => crate::hud::hazard_color(kind),
        };
        let a = to_screen(l.a, me, look.angle, PX_PER_M);
        let b = to_screen(l.b, me, look.angle, PX_PER_M);
        gizmos.line_2d(a, b, color);
    }
    // The player: an arrow at the centre, pointing up (forward).
    gizmos.linestrip_2d(
        [
            Vec2::new(-6.0, -6.0),
            Vec2::new(0.0, 10.0),
            Vec2::new(6.0, -6.0),
        ],
        Color::srgb(0.2, 1.0, 0.4),
    );
}
