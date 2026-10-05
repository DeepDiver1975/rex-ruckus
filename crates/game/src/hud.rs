//! HUD: bottom status bar (health, ammo, weapon slots, keycards), crosshair, damage flash,
//! death and level-complete overlays, plus the M2 use prompt and message line.
//! Spawned once at startup; it is not a `LevelEntity`, so it persists across restarts.

use crate::combat::{FxQueue, FxReaders, GameDefs, PlayerArsenal, PlayerInventory, PlayerVitals};
use crate::flow::PlayState;
use crate::level::CurrentMap;
use crate::mechanics::{HudMessage, LevelMechanics, UsePrompt};
use crate::player::{Inventory, Player};
use crate::props::key_color;
use bevy::prelude::*;
use rr_core::combat::CombatEvent;
use rr_core::defs::WeaponId;
use rr_core::inventory::{BATTERY_MAX, FUEL_MAX, Inventory as Carried, MEDKIT_MAX};
use rr_core::map::{Key, Map, MoverKind, SwitchAction};
use rr_core::mechanics::{Mechanics, UseTarget};

/// Damage that fills the flash completely.
const FLASH_FULL_DAMAGE: f32 = 25.0;
/// Flash lost per second.
const FLASH_DECAY: f32 = 2.5;
/// Opacity of a full flash.
const FLASH_ALPHA: f32 = 0.45;
const BAR_HEIGHT: f32 = 64.0;
const KEY_SIZE: f32 = 18.0;
const MISSING_KEY_ALPHA: f32 = 0.2;
const ACTIVE: Color = Color::srgb(0.4, 1.0, 0.5);
const INACTIVE: Color = Color::srgba(1.0, 1.0, 1.0, 0.45);

#[derive(Component)]
struct PromptText;
#[derive(Component)]
struct MessageText;
/// One text cell of the status bar.
#[derive(Component, Clone, Copy)]
enum StatusCell {
    Health,
    Armour,
    Ammo,
    WeaponName,
    Slot(WeaponId),
    /// The carried-item strip.
    Item(Item),
}
/// One carried item in the inventory strip.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Item {
    Medkit,
    Jetpack,
    NightVision,
}
/// A keycard square, dimmed while the key is missing.
#[derive(Component)]
struct KeyCard(Key);
#[derive(Component)]
struct FlashNode;
#[derive(Component)]
struct OverlayNode;
#[derive(Component)]
struct OverlayText;

/// Red screen flash strength in 0..=1.
#[derive(Resource, Default, Debug)]
pub struct DamageFlash(pub f32);

pub fn prompt_label(map: &Map, mech: &Mechanics, t: UseTarget) -> &'static str {
    match t {
        UseTarget::Mover(m) => match mech.movers[m].def.kind {
            MoverKind::Door => "[E] Door",
            MoverKind::Lift { .. } => "[E] Lift",
            MoverKind::Crack => "",
        },
        UseTarget::Switch(i) => match map.switches[i].action {
            SwitchAction::Exit => "[E] Exit",
            SwitchAction::Channel(_) => "[E] Switch",
        },
    }
}

/// Ammo text for the status bar: `clip / reserve`, reserve only, or a dash for the boot.
pub fn ammo_label(weapon: WeaponId, readout: (Option<u32>, Option<u32>)) -> String {
    match (weapon, readout) {
        (_, (Some(clip), Some(reserve))) => format!("{clip} / {reserve}"),
        (_, (None, Some(reserve))) => reserve.to_string(),
        (_, (Some(clip), None)) => clip.to_string(),
        (_, (None, None)) => "\u{2014}".to_string(),
    }
}

/// Inventory strip text and whether the item is active (highlighted): medkit charge, jetpack
/// fuel and night-vision battery, each as a percentage.
fn item_cell(item: Item, c: &Carried) -> (String, bool) {
    let pct = |v: f32, max: f32| (v / max * 100.0).ceil().clamp(0.0, 100.0) as i32;
    match item {
        Item::Medkit => (
            format!("MED {}%", c.medkit * 100 / MEDKIT_MAX),
            c.medkit > 0,
        ),
        Item::Jetpack => (format!("JET {}%", pct(c.fuel, FUEL_MAX)), c.jetpack_on),
        Item::NightVision => (format!("NV {}%", pct(c.battery, BATTERY_MAX)), c.nv_on),
    }
}

/// Flash after taking `amount` damage, capped at 1.
pub fn flash_add(flash: f32, amount: i32) -> f32 {
    (flash + amount.max(0) as f32 / FLASH_FULL_DAMAGE).min(1.0)
}

/// Flash after `dt` seconds of decay, never below 0.
pub fn flash_decay(flash: f32, dt: f32) -> f32 {
    (flash - FLASH_DECAY * dt).max(0.0)
}

/// Overlay message for a play state; `None` while playing.
pub fn overlay_text(state: PlayState) -> Option<&'static str> {
    match state {
        PlayState::Playing => None,
        PlayState::Dead => Some("You died \u{2014} press Use or Fire to restart"),
        PlayState::Complete => Some("Level complete \u{2014} press Use or Fire to restart"),
    }
}

fn overlay_tint(state: PlayState) -> Color {
    match state {
        PlayState::Dead => Color::srgba(0.7, 0.0, 0.0, 0.35),
        _ => Color::srgba(0.0, 0.0, 0.05, 0.6),
    }
}

pub struct HudPlugin;

impl Plugin for HudPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<DamageFlash>()
            .add_systems(Startup, spawn_hud)
            .add_systems(
                Update,
                (
                    update_prompt,
                    update_message,
                    update_status,
                    update_keycards,
                    read_hurt.in_set(FxReaders),
                    update_flash.after(read_hurt),
                    update_overlay,
                ),
            );
    }
}

fn font(px: f32) -> TextFont {
    TextFont {
        font_size: FontSize::Px(px),
        ..default()
    }
}

fn fill_screen(z: i32, color: Color) -> impl Bundle {
    (
        Node {
            position_type: PositionType::Absolute,
            width: Val::Percent(100.0),
            height: Val::Percent(100.0),
            justify_content: JustifyContent::Center,
            align_items: AlignItems::Center,
            ..default()
        },
        BackgroundColor(color),
        GlobalZIndex(z),
    )
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

    // Crosshair: two thin bars, centred.
    commands
        .spawn(Node {
            position_type: PositionType::Absolute,
            width: Val::Percent(100.0),
            height: Val::Percent(100.0),
            justify_content: JustifyContent::Center,
            align_items: AlignItems::Center,
            ..default()
        })
        .with_children(|p| {
            for (w, h) in [(14.0, 2.0), (2.0, 14.0)] {
                p.spawn((
                    Node {
                        position_type: PositionType::Absolute,
                        width: Val::Px(w),
                        height: Val::Px(h),
                        ..default()
                    },
                    BackgroundColor(Color::srgba(1.0, 1.0, 1.0, 0.8)),
                ));
            }
        });

    // Damage flash, under the overlay.
    commands.spawn((fill_screen(5, Color::srgba(0.85, 0.0, 0.0, 0.0)), FlashNode));

    // Status bar.
    commands
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                bottom: Val::Px(0.0),
                width: Val::Percent(100.0),
                height: Val::Px(BAR_HEIGHT),
                justify_content: JustifyContent::SpaceAround,
                align_items: AlignItems::Center,
                ..default()
            },
            BackgroundColor(Color::srgba(0.05, 0.05, 0.08, 0.85)),
        ))
        .with_children(|bar| {
            let cell = |p: &mut ChildSpawnerCommands, kind: StatusCell, text: &str| {
                p.spawn((Text::new(text), font(26.0), TextColor(Color::WHITE), kind));
            };
            cell(bar, StatusCell::Health, "HEALTH 100");
            cell(bar, StatusCell::Armour, "ARMOUR 0");
            cell(bar, StatusCell::Ammo, "AMMO \u{2014}");
            bar.spawn(Node {
                column_gap: Val::Px(10.0),
                align_items: AlignItems::Center,
                ..default()
            })
            .with_children(|w| {
                cell(w, StatusCell::WeaponName, "");
                for id in WeaponId::ALL {
                    cell(w, StatusCell::Slot(id), &id.slot().to_string());
                }
            });
            bar.spawn(Node {
                column_gap: Val::Px(10.0),
                ..default()
            })
            .with_children(|strip| {
                for item in [Item::Medkit, Item::Jetpack, Item::NightVision] {
                    strip.spawn((
                        Text::new(""),
                        font(18.0),
                        TextColor(INACTIVE),
                        StatusCell::Item(item),
                    ));
                }
            });
            bar.spawn(Node {
                column_gap: Val::Px(6.0),
                ..default()
            })
            .with_children(|k| {
                for key in Key::ALL {
                    k.spawn((
                        Node {
                            width: Val::Px(KEY_SIZE),
                            height: Val::Px(KEY_SIZE),
                            ..default()
                        },
                        BackgroundColor(key_color(key).with_alpha(MISSING_KEY_ALPHA)),
                        KeyCard(key),
                    ));
                }
            });
        });

    // Overlay, hidden while playing.
    commands
        .spawn((
            fill_screen(10, Color::NONE),
            Visibility::Hidden,
            OverlayNode,
        ))
        .with_children(|p| {
            p.spawn((
                Text::new(""),
                font(36.0),
                TextColor(Color::WHITE),
                OverlayText,
            ));
        });
}

fn set(text: &mut Text, s: &str) {
    if text.0 != s {
        text.0 = s.to_string();
    }
}

fn set_color(color: &mut TextColor, c: Color) {
    if color.0 != c {
        color.0 = c;
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
    mut text: Single<&mut Text, With<MessageText>>,
) {
    msg.remaining = (msg.remaining - time.delta_secs()).max(0.0);
    let s = if msg.remaining > 0.0 {
        msg.text.as_str()
    } else {
        ""
    };
    set(&mut text, s);
}

/// Health, ammo, weapon name and slot highlighting. Skips frames without a player.
fn update_status(
    defs: Res<GameDefs>,
    player: Query<(&PlayerVitals, &PlayerArsenal, &PlayerInventory), With<Player>>,
    mut cells: Query<(&StatusCell, &mut Text, &mut TextColor)>,
) {
    let Ok((health, arsenal, carried)) = player.single() else {
        return;
    };
    let a = &arsenal.0;
    for (cell, mut text, mut color) in &mut cells {
        match *cell {
            StatusCell::Health => set(&mut text, &format!("HEALTH {}", health.0.health.hp.max(0))),
            StatusCell::Armour => set(&mut text, &format!("ARMOUR {}", health.0.armour.max(0))),
            StatusCell::Item(item) => {
                let (label, active) = item_cell(item, &carried.0);
                set(&mut text, &label);
                set_color(&mut color, if active { ACTIVE } else { INACTIVE });
            }
            StatusCell::Ammo => set(
                &mut text,
                &format!("AMMO {}", ammo_label(a.current, a.readout(&defs.0))),
            ),
            StatusCell::WeaponName => set(&mut text, &defs.0.weapon(a.current).name),
            StatusCell::Slot(id) => {
                let c = if id == a.current {
                    Color::srgb(1.0, 0.85, 0.2)
                } else if a.owned[id.index()] {
                    Color::WHITE
                } else {
                    Color::srgba(1.0, 1.0, 1.0, 0.25)
                };
                set_color(&mut color, c);
            }
        }
    }
}

fn update_keycards(
    player: Query<&Inventory, With<Player>>,
    mut cards: Query<(&KeyCard, &mut BackgroundColor)>,
) {
    let Ok(inv) = player.single() else { return };
    for (card, mut bg) in &mut cards {
        let alpha = if inv.keys.contains(card.0) {
            1.0
        } else {
            MISSING_KEY_ALPHA
        };
        let c = key_color(card.0).with_alpha(alpha);
        if bg.0 != c {
            bg.0 = c;
        }
    }
}

/// An [`FxReaders`] system: it only reads the queue.
fn read_hurt(fx: Res<FxQueue>, mut flash: ResMut<DamageFlash>) {
    for ev in &fx.combat {
        if let CombatEvent::PlayerHurt { amount, .. } = ev {
            flash.0 = flash_add(flash.0, *amount);
        }
    }
}

fn update_flash(
    time: Res<Time>,
    mut flash: ResMut<DamageFlash>,
    mut node: Single<&mut BackgroundColor, With<FlashNode>>,
) {
    let c = Color::srgba(0.85, 0.0, 0.0, flash.0 * FLASH_ALPHA);
    if node.0 != c {
        node.0 = c;
    }
    if flash.0 > 0.0 {
        flash.0 = flash_decay(flash.0, time.delta_secs());
    }
}

fn update_overlay(
    state: Res<PlayState>,
    mut node: Single<(&mut Visibility, &mut BackgroundColor), With<OverlayNode>>,
    mut text: Single<&mut Text, With<OverlayText>>,
) {
    let (vis, bg) = &mut *node;
    match overlay_text(*state) {
        Some(s) => {
            vis.set_if_neq(Visibility::Inherited);
            let tint = overlay_tint(*state);
            if bg.0 != tint {
                bg.0 = tint;
            }
            set(&mut text, s);
        }
        None => {
            vis.set_if_neq(Visibility::Hidden);
        }
    }
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

    #[test]
    fn ammo_label_formats_per_weapon() {
        assert_eq!(
            ammo_label(WeaponId::Pistol, (Some(12), Some(36))),
            "12 / 36"
        );
        assert_eq!(ammo_label(WeaponId::Shotgun, (None, Some(8))), "8");
        assert_eq!(ammo_label(WeaponId::Boot, (None, None)), "—");
    }

    #[test]
    fn flash_accumulates_and_decays() {
        assert!((flash_add(0.0, 10) - 0.4).abs() < 1e-6);
        assert!((flash_add(0.4, 10) - 0.8).abs() < 1e-6);
        assert_eq!(flash_add(0.8, 25), 1.0, "capped at 1");
        assert!((flash_decay(1.0, 0.2) - 0.5).abs() < 1e-6);
        assert_eq!(flash_decay(0.1, 1.0), 0.0, "never negative");
    }

    #[test]
    fn overlay_text_per_state() {
        assert_eq!(overlay_text(PlayState::Playing), None);
        assert_eq!(
            overlay_text(PlayState::Dead),
            Some("You died — press Use or Fire to restart")
        );
        assert_eq!(
            overlay_text(PlayState::Complete),
            Some("Level complete — press Use or Fire to restart")
        );
    }

    #[test]
    fn item_cells_show_percent_and_highlight_active() {
        let mut c = Carried {
            medkit: 45,
            fuel: 80.0,
            battery: 0.5,
            ..Default::default()
        };
        assert_eq!(item_cell(Item::Medkit, &c), ("MED 45%".into(), true));
        assert_eq!(item_cell(Item::Jetpack, &c), ("JET 80%".into(), false));
        assert_eq!(item_cell(Item::NightVision, &c), ("NV 1%".into(), false));
        c.jetpack_on = true;
        assert!(item_cell(Item::Jetpack, &c).1);
    }
}
