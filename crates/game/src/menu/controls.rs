//! The controls screen: one row per action, click to rebind.
//!
//! Clicking a row starts a capture ([`Capture`]); the next key or mouse button becomes its
//! binding (Esc cancels). Menu navigation and the pause toggle ignore input meanwhile.

use super::widgets::{self, button, label, spawn_screen};
use super::{MenuAction, MenuScreen, Screen};
use crate::bindings::{Action, Binding};
use crate::hud::UiFont;
use crate::settings::Settings;
use bevy::prelude::*;

/// The action waiting for its new key or button, if any.
#[derive(Resource, Default, Debug, PartialEq, Eq)]
pub struct Capture(pub Option<Action>);

/// The line under the rows ("E moved from Use").
#[derive(Resource, Default, Debug)]
pub struct Notice(pub String);

/// The bindings text of a row.
#[derive(Component)]
pub struct BindingText(Action);

/// The notice line.
#[derive(Component)]
pub struct NoticeText;

const GREY: Color = Color::srgb(0.5, 0.5, 0.55);
const ROW_PX: f32 = 18.0;

/// Spawns the controls screen; the texts are filled in by [`refresh_rows`].
pub fn spawn_controls(commands: &mut Commands, ui: &UiFont) {
    let root = spawn_screen(commands, ui, "CONTROLS");
    commands.entity(root).with_children(|p| {
        p.spawn(Node {
            flex_direction: FlexDirection::Column,
            row_gap: Val::Px(2.0),
            ..default()
        })
        .with_children(|list| {
            for a in Action::ALL {
                let row = Node {
                    width: Val::Px(560.0),
                    padding: UiRect::axes(Val::Px(16.0), Val::Px(2.0)),
                    justify_content: JustifyContent::SpaceBetween,
                    ..default()
                };
                if a == Action::Pause {
                    // Escape is not rebindable.
                    list.spawn(row).with_children(|r| {
                        label(r, ui, a.label(), ROW_PX, GREY);
                        label(r, ui, "Esc", ROW_PX, GREY);
                    });
                } else {
                    list.spawn((
                        Button,
                        row,
                        BackgroundColor(widgets::IDLE),
                        MenuAction::Rebind(a),
                    ))
                    .with_children(|r| {
                        label(r, ui, a.label(), ROW_PX, Color::WHITE);
                        r.spawn((
                            Text::new(""),
                            crate::hud::font(ui, ROW_PX),
                            TextColor(widgets::AMBER),
                            BindingText(a),
                        ));
                    });
                }
            }
        });
        p.spawn((
            Text::new(""),
            crate::hud::font(ui, ROW_PX),
            TextColor(Color::WHITE),
            NoticeText,
        ));
        button(p, ui, "Reset to defaults", MenuAction::ResetBindings);
        button(p, ui, "Back", MenuAction::Back);
    });
}

/// The bindings of an action as shown: labels joined by ` / `, or a dash.
fn bindings_label(settings: &Settings, a: Action) -> String {
    let list = settings.bindings.of(a);
    if list.is_empty() {
        "—".into()
    } else {
        list.iter()
            .map(|b| b.label())
            .collect::<Vec<_>>()
            .join(" / ")
    }
}

/// Keeps the row and notice texts on the settings, the capture and the notice.
pub fn refresh_rows(
    settings: Option<Res<Settings>>,
    capture: Res<Capture>,
    notice: Res<Notice>,
    mut rows: Query<(&mut Text, &BindingText), Without<NoticeText>>,
    mut notice_text: Query<&mut Text, With<NoticeText>>,
) {
    let Some(settings) = settings else { return };
    for (mut text, BindingText(a)) in &mut rows {
        let wanted = if capture.0 == Some(*a) {
            "press a key…".to_string()
        } else {
            bindings_label(&settings, *a)
        };
        if text.0 != wanted {
            text.0 = wanted;
        }
    }
    for mut text in &mut notice_text {
        if text.0 != notice.0 {
            text.0 = notice.0.clone();
        }
    }
}

/// While capturing, the next key or mouse button becomes the binding; Esc cancels.
///
/// The frame the capture starts is skipped: the click or Enter that chose the row must not bind.
pub fn capture_input(
    keys: Res<ButtonInput<KeyCode>>,
    mouse: Res<ButtonInput<MouseButton>>,
    mut capture: ResMut<Capture>,
    settings: Option<ResMut<Settings>>,
    mut notice: ResMut<Notice>,
    screen: Res<MenuScreen>,
    mut armed: Local<bool>,
) {
    let Some(action) = capture.0 else {
        *armed = false;
        return;
    };
    if screen.0 != Some(Screen::Controls) {
        capture.0 = None;
        *armed = false;
        return;
    }
    if !*armed {
        *armed = true;
        return;
    }
    if keys.just_pressed(KeyCode::Escape) {
        capture.0 = None;
        notice.0.clear();
        *armed = false;
        return;
    }
    let Some(binding) = keys
        .get_just_pressed()
        .next()
        .map(|k| Binding::Key(*k))
        .or_else(|| mouse.get_just_pressed().next().map(|b| Binding::Mouse(*b)))
    else {
        return;
    };
    capture.0 = None;
    *armed = false;
    let Some(mut settings) = settings else { return };
    notice.0 = match settings.bindings.bind(action, binding) {
        Some(other) => format!("{} moved from {}", binding.label(), other.label()),
        None => String::new(),
    };
}
