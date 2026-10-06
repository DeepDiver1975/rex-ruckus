//! The options screen: one row per setting, `label  < value >`.
//!
//! Left/Right (or clicking the arrows) adjust the selected row; Enter or a click on the row itself
//! steps a slider up or flips a toggle. Edits go straight into [`Settings`], which applies them
//! live; leaving the screen asks for a save.

use super::widgets::{self, button, label, spawn_screen};
use super::{MenuAction, MenuButtons, MenuInput, MenuScreen, MenuSelection, Screen, Setting};
use crate::hud::{UiFont, font};
use crate::settings::{FOV_MAX, FOV_MIN, SENS_MAX, SENS_MIN, SaveSettings, Settings};
use bevy::prelude::*;

/// The options rows, in screen order.
const ROWS: [Setting; 8] = [
    Setting::Sensitivity,
    Setting::InvertY,
    Setting::Fov,
    Setting::Master,
    Setting::Sfx,
    Setting::Voice,
    Setting::Music,
    Setting::LowRes,
];

/// The value text of a row; kept in sync with [`Settings`] by [`refresh_values`].
#[derive(Component)]
pub struct ValueText(Setting);

/// A `<` or `>` arrow button; clicking it adjusts its setting by the direction.
#[derive(Component)]
pub struct Arrow(Setting, i8);

fn is_toggle(which: Setting) -> bool {
    matches!(which, Setting::InvertY | Setting::LowRes)
}

fn name(which: Setting) -> &'static str {
    match which {
        Setting::Sensitivity => "Mouse sensitivity",
        Setting::InvertY => "Invert Y",
        Setting::Fov => "Field of view",
        Setting::Master => "Master volume",
        Setting::Sfx => "Effects volume",
        Setting::Voice => "Voice volume",
        Setting::Music => "Music volume",
        Setting::LowRes => "Low-res mode",
    }
}

/// Steps a volume by a tenth in direction `dir`, kept on whole tenths within 0..1.
fn step_tenth(v: f32, dir: i8) -> f32 {
    ((v + 0.1 * f32::from(dir.signum())) * 10.0)
        .round()
        .clamp(0.0, 10.0)
        / 10.0
}

/// Changes `which` one step in direction `dir` (toggles flip on any non-zero `dir`), clamped.
pub fn adjust(s: &mut Settings, which: Setting, dir: i8) {
    let up = dir > 0;
    match which {
        Setting::Sensitivity => {
            let k = if up { 1.15 } else { 1.0 / 1.15 };
            s.mouse_sensitivity = (s.mouse_sensitivity * k).clamp(SENS_MIN, SENS_MAX);
        }
        Setting::Fov => {
            s.fov_deg = (s.fov_deg + 5.0 * f32::from(dir.signum())).clamp(FOV_MIN, FOV_MAX);
        }
        Setting::Master => s.master = step_tenth(s.master, dir),
        Setting::Sfx => s.sfx = step_tenth(s.sfx, dir),
        Setting::Voice => s.voice = step_tenth(s.voice, dir),
        Setting::Music => s.music = step_tenth(s.music, dir),
        Setting::InvertY if dir != 0 => s.invert_y = !s.invert_y,
        Setting::LowRes if dir != 0 => s.low_res = !s.low_res,
        Setting::InvertY | Setting::LowRes => {}
    }
}

/// The text shown between the arrows.
pub fn value_label(s: &Settings, which: Setting) -> String {
    let onoff = |b: bool| if b { "On" } else { "Off" }.to_string();
    let percent = |v: f32| format!("{}%", (v * 100.0).round());
    match which {
        Setting::Sensitivity => format!(
            "×{:.2}",
            s.mouse_sensitivity / Settings::default().mouse_sensitivity
        ),
        Setting::Fov => format!("{}°", s.fov_deg.round()),
        Setting::Master => percent(s.master),
        Setting::Sfx => percent(s.sfx),
        Setting::Voice => percent(s.voice),
        Setting::Music => percent(s.music),
        Setting::InvertY => onoff(s.invert_y),
        Setting::LowRes => onoff(s.low_res),
    }
}

/// Spawns the options screen.
pub fn spawn_options(commands: &mut Commands, ui: &UiFont) {
    let root = spawn_screen(commands, ui, "OPTIONS");
    commands.entity(root).with_children(|p| {
        for which in ROWS {
            let action = if is_toggle(which) {
                MenuAction::Toggle(which)
            } else {
                MenuAction::Adjust(which, 1)
            };
            p.spawn((
                Button,
                Node {
                    width: Val::Px(560.0),
                    padding: UiRect::axes(Val::Px(24.0), Val::Px(4.0)),
                    column_gap: Val::Px(12.0),
                    justify_content: JustifyContent::SpaceBetween,
                    align_items: AlignItems::Center,
                    ..default()
                },
                BackgroundColor(widgets::IDLE),
                action,
            ))
            .with_children(|row| {
                label(row, ui, name(which), 24.0, Color::WHITE);
                row.spawn(Node {
                    column_gap: Val::Px(12.0),
                    align_items: AlignItems::Center,
                    ..default()
                })
                .with_children(|v| {
                    arrow(v, ui, "<", Arrow(which, -1));
                    v.spawn((
                        Text::new(""),
                        font(ui, 24.0),
                        TextColor(widgets::AMBER),
                        Node {
                            min_width: Val::Px(90.0),
                            justify_content: JustifyContent::Center,
                            ..default()
                        },
                        TextLayout::justify(Justify::Center),
                        ValueText(which),
                    ));
                    arrow(v, ui, ">", Arrow(which, 1));
                });
            });
        }
        widgets::button_row(p, |r| {
            button(r, ui, "Controls", MenuAction::Controls);
            button(r, ui, "Back", MenuAction::Back);
        });
    });
}

fn arrow(parent: &mut ChildSpawnerCommands, ui: &UiFont, text: &str, arrow: Arrow) {
    parent
        .spawn((
            Button,
            Node {
                padding: UiRect::axes(Val::Px(10.0), Val::Px(0.0)),
                ..default()
            },
            BackgroundColor(Color::NONE),
            arrow,
        ))
        .with_children(|b| label(b, ui, text, 24.0, Color::WHITE));
}

/// Left/Right adjust the selected row.
pub(super) fn arrow_keys(
    keys: Res<ButtonInput<KeyCode>>,
    screen: Res<MenuScreen>,
    buttons: MenuButtons,
    selection: Res<MenuSelection>,
    mut input: ResMut<MenuInput>,
) {
    if screen.0 != Some(Screen::Options) {
        return;
    }
    let dir = if keys.just_pressed(KeyCode::ArrowLeft) {
        -1
    } else if keys.just_pressed(KeyCode::ArrowRight) {
        1
    } else {
        return;
    };
    if let Some((_, MenuAction::Adjust(which, _) | MenuAction::Toggle(which))) =
        buttons.list().get(selection.0)
    {
        input.activate = Some(MenuAction::Adjust(*which, dir));
    }
}

/// Clicking an arrow adjusts its setting.
pub fn click_arrows(
    arrows: Query<(&Interaction, &Arrow), Changed<Interaction>>,
    mut input: ResMut<MenuInput>,
) {
    for (interaction, Arrow(which, dir)) in &arrows {
        if *interaction == Interaction::Pressed {
            input.activate = Some(MenuAction::Adjust(*which, *dir));
        }
    }
}

/// Keeps the value texts on the settings.
pub fn refresh_values(settings: Option<Res<Settings>>, mut texts: Query<(&mut Text, &ValueText)>) {
    let Some(settings) = settings else { return };
    for (mut text, ValueText(which)) in &mut texts {
        let wanted = value_label(&settings, *which);
        if text.0 != wanted {
            text.0 = wanted;
        }
    }
}

/// Asks for a save when the options screen is left (for the menu it came from, or for good).
pub fn save_on_leave(
    screen: Res<MenuScreen>,
    mut previous: Local<Option<Screen>>,
    save: Option<ResMut<SaveSettings>>,
) {
    let now = screen.0;
    if *previous == Some(Screen::Options)
        && now != Some(Screen::Options)
        && let Some(mut save) = save
    {
        save.0 = true;
    }
    *previous = now;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn adjust_steps_and_clamps() {
        let mut s = Settings::default();
        adjust(&mut s, Setting::Fov, 1);
        assert_eq!(s.fov_deg, 80.0);
        for _ in 0..20 {
            adjust(&mut s, Setting::Fov, 1);
        }
        assert_eq!(s.fov_deg, FOV_MAX);
        adjust(&mut s, Setting::Music, -1);
        assert_eq!(s.music, 0.4);
        adjust(&mut s, Setting::InvertY, 1);
        assert!(s.invert_y);
        assert_eq!(value_label(&s, Setting::Music), "40%");
    }

    #[test]
    fn sensitivity_scales_and_clamps() {
        let mut s = Settings::default();
        assert_eq!(value_label(&s, Setting::Sensitivity), "×1.00");
        adjust(&mut s, Setting::Sensitivity, 1);
        assert_eq!(value_label(&s, Setting::Sensitivity), "×1.15");
        for _ in 0..60 {
            adjust(&mut s, Setting::Sensitivity, -1);
        }
        assert_eq!(s.mouse_sensitivity, SENS_MIN);
        for _ in 0..60 {
            adjust(&mut s, Setting::Sensitivity, 1);
        }
        assert_eq!(s.mouse_sensitivity, SENS_MAX);
    }

    #[test]
    fn volumes_stay_on_tenths_within_range() {
        let mut s = Settings::default();
        for _ in 0..15 {
            adjust(&mut s, Setting::Master, 1);
        }
        assert_eq!(s.master, 1.0);
        for _ in 0..15 {
            adjust(&mut s, Setting::Master, -1);
        }
        assert_eq!(s.master, 0.0);
        adjust(&mut s, Setting::LowRes, -1);
        assert!(s.low_res, "a toggle flips on any direction");
    }
}
