//! Menu building blocks: the full-screen root, text rows and buttons.

use super::{MenuAction, MenuRoot};
use crate::hud::{UiFont, font};
use bevy::prelude::*;

/// Title colour.
pub const AMBER: Color = Color::srgb(1.0, 0.75, 0.1);
/// Button fill when not selected.
pub const IDLE: Color = Color::srgba(0.1, 0.1, 0.15, 0.9);
/// Button fill when selected (keyboard or hover).
pub const SELECTED: Color = Color::srgb(0.8, 0.35, 0.05);
/// Muted text for hints.
pub const DIM: Color = Color::srgb(0.6, 0.6, 0.6);
/// Dimmed backdrop over the frozen level.
pub const BACKDROP: Color = Color::srgba(0.0, 0.0, 0.05, 0.8);
/// Draw order of a menu: above every HUD layer (which stay below 20).
pub const MENU_Z: i32 = 100;

/// Title size of the sub-screens; the main menu uses a bigger one.
pub const TITLE_PX: f32 = 44.0;

/// Spawns a full-screen menu root with a title and returns it; add rows with
/// `commands.entity(root).with_children(..)`. Buttons are found in spawn order.
pub fn spawn_screen(commands: &mut Commands, ui: &UiFont, title: &str) -> Entity {
    spawn_screen_sized(commands, ui, title, TITLE_PX)
}

/// [`spawn_screen`] with a title of `title_px` pixels.
pub fn spawn_screen_sized(
    commands: &mut Commands,
    ui: &UiFont,
    title: &str,
    title_px: f32,
) -> Entity {
    let mut root = commands.spawn((
        Node {
            position_type: PositionType::Absolute,
            left: Val::Px(0.0),
            top: Val::Px(0.0),
            width: Val::Percent(100.0),
            height: Val::Percent(100.0),
            flex_direction: FlexDirection::Column,
            align_items: AlignItems::Center,
            justify_content: JustifyContent::Center,
            row_gap: Val::Px(8.0),
            ..default()
        },
        BackgroundColor(BACKDROP),
        GlobalZIndex(MENU_Z),
        MenuRoot,
    ));
    root.with_children(|p| label(p, ui, title, title_px, AMBER));
    root.id()
}

/// A line of text.
pub fn label(parent: &mut ChildSpawnerCommands, ui: &UiFont, text: &str, px: f32, color: Color) {
    parent.spawn((Text::new(text), font(ui, px), TextColor(color)));
}

/// A clickable row that performs `action`.
pub fn button(parent: &mut ChildSpawnerCommands, ui: &UiFont, text: &str, action: MenuAction) {
    parent
        .spawn((
            Button,
            Node {
                padding: UiRect::axes(Val::Px(24.0), Val::Px(6.0)),
                ..default()
            },
            BackgroundColor(IDLE),
            action,
        ))
        .with_children(|b| label(b, ui, text, 26.0, Color::WHITE));
}

/// Buttons side by side (they stay in spawn order for navigation).
pub fn button_row(parent: &mut ChildSpawnerCommands, rows: impl FnOnce(&mut ChildSpawnerCommands)) {
    parent
        .spawn(Node {
            column_gap: Val::Px(16.0),
            ..default()
        })
        .with_children(rows);
}
