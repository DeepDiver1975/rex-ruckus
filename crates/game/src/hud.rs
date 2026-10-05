//! Minimal M2 HUD: use prompt, message line and held keycards. M3 replaces it with the status bar.

use crate::flow::PlayState;
use crate::level::CurrentMap;
use crate::mechanics::{HudMessage, LevelMechanics, UsePrompt};
use crate::player::Inventory;
use bevy::prelude::*;
use rr_core::map::{Key, Map, MoverKind, SwitchAction};
use rr_core::mechanics::{Mechanics, UseTarget};

#[derive(Component)]
struct PromptText;
#[derive(Component)]
struct MessageText;
#[derive(Component)]
struct KeysText;

pub fn prompt_label(map: &Map, mech: &Mechanics, t: UseTarget) -> &'static str {
    match t {
        UseTarget::Mover(m) => match mech.movers[m].def.kind {
            MoverKind::Door => "[E] Door",
            MoverKind::Lift { .. } => "[E] Lift",
        },
        UseTarget::Switch(i) => match map.switches[i].action {
            SwitchAction::Exit => "[E] Exit",
            SwitchAction::Channel(_) => "[E] Switch",
        },
    }
}

pub struct HudPlugin;

impl Plugin for HudPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, spawn_hud)
            .add_systems(Update, (update_prompt, update_message, update_keys));
    }
}

fn font(px: f32) -> TextFont {
    TextFont {
        font_size: FontSize::Px(px),
        ..default()
    }
}

/// A full-width row that centres its text horizontally.
fn row(top: Val) -> Node {
    Node {
        position_type: PositionType::Absolute,
        width: Val::Percent(100.0),
        top,
        justify_content: JustifyContent::Center,
        ..default()
    }
}

fn spawn_hud(mut commands: Commands) {
    commands.spawn(row(Val::Percent(60.0))).with_children(|p| {
        p.spawn((
            Text::new(""),
            font(22.0),
            TextColor(Color::WHITE),
            PromptText,
        ));
    });
    commands.spawn(row(Val::Px(24.0))).with_children(|p| {
        p.spawn((
            Text::new(""),
            font(28.0),
            TextColor(Color::srgb(1.0, 0.85, 0.2)),
            MessageText,
        ));
    });
    commands.spawn((
        Node {
            position_type: PositionType::Absolute,
            left: Val::Px(16.0),
            bottom: Val::Px(12.0),
            ..default()
        },
        Text::new("Keys: -"),
        font(20.0),
        TextColor(Color::WHITE),
        KeysText,
    ));
}

fn set(text: &mut Text, s: &str) {
    if text.0 != s {
        text.0 = s.to_string();
    }
}

fn update_prompt(
    prompt: Res<UsePrompt>,
    map: Res<CurrentMap>,
    mech: Res<LevelMechanics>,
    state: Res<PlayState>,
    mut text: Single<&mut Text, With<PromptText>>,
) {
    let label = match prompt.0 {
        Some(t) if *state == PlayState::Playing => prompt_label(&map.0, &mech.0, t),
        _ => "",
    };
    set(&mut text, label);
}

fn update_message(
    time: Res<Time>,
    mut msg: ResMut<HudMessage>,
    state: Res<PlayState>,
    mut text: Single<&mut Text, With<MessageText>>,
) {
    msg.remaining = (msg.remaining - time.delta_secs()).max(0.0);
    let s = if *state == PlayState::Complete {
        "LEVEL COMPLETE"
    } else if msg.remaining > 0.0 {
        msg.text.as_str()
    } else {
        ""
    };
    set(&mut text, s);
}

fn update_keys(
    q: Query<&Inventory, Changed<Inventory>>,
    mut text: Single<&mut Text, With<KeysText>>,
) {
    let Ok(inv) = q.single() else { return };
    let held: Vec<&str> = Key::ALL
        .iter()
        .filter(|&&k| inv.keys.contains(k))
        .map(|k| k.name())
        .collect();
    let s = if held.is_empty() {
        "Keys: -".to_string()
    } else {
        format!("Keys: {}", held.join(" "))
    };
    set(&mut text, &s);
}

#[cfg(test)]
mod tests {
    use super::*;
    use rr_core::fixtures::{door_rooms, lift_shaft};

    #[test]
    fn prompt_names_the_target() {
        let mut map = door_rooms(
            "(kind: Door)",
            "switches: [(wall: (7, 0), action: Exit), (wall: (3, 4), action: Channel(2))],",
        );
        let mech = Mechanics::new(&mut map);
        assert_eq!(prompt_label(&map, &mech, UseTarget::Mover(0)), "[E] Door");
        assert_eq!(prompt_label(&map, &mech, UseTarget::Switch(0)), "[E] Exit");
        assert_eq!(
            prompt_label(&map, &mech, UseTarget::Switch(1)),
            "[E] Switch"
        );
        let mut map = lift_shaft("(kind: Lift(to: 2.0))", "");
        let mech = Mechanics::new(&mut map);
        assert_eq!(prompt_label(&map, &mech, UseTarget::Mover(0)), "[E] Lift");
    }
}
