//! HUD: bottom status bar (health, ammo, weapon slots, keycards), crosshair, damage flash,
//! death and level-complete overlays, plus the M2 use prompt, the message line and the
//! subtitle line for the hero's quips.
//! Spawned once at startup; it is not a `LevelEntity`, so it persists across restarts.

use crate::bindings::{Action, Bindings};
use crate::combat::{
    FxQueue, FxReaders, GameDefs, LevelCombat, PlayerArsenal, PlayerInventory, PlayerVitals,
};
use crate::flow::PlayState;
use crate::level::CurrentMap;
use crate::mechanics::{HudMessage, HudSubtitle, LevelMechanics, UsePrompt};
use crate::player::{Inventory, Player};
use crate::props::key_color;
use bevy::prelude::*;
use rr_core::combat::CombatEvent;
use rr_core::defs::WeaponId;
use rr_core::hazard::HazardKind;
use rr_core::inventory::{BATTERY_MAX, FUEL_MAX, Inventory as Carried, MEDKIT_MAX};
use rr_core::map::{Key, Map, MoverKind, SwitchAction};
use rr_core::mechanics::{Mechanics, UseTarget};
use rr_core::props::PropKind;

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

/// A top-level HUD node (the overlay manages its own visibility); hidden while a menu screen
/// is shown, so the menu is never drawn over.
#[derive(Component)]
pub struct HudRoot;
#[derive(Component)]
struct PromptText;
#[derive(Component)]
struct MessageText;
#[derive(Component)]
struct SubtitleText;
/// The boss health bar's frame; hidden unless a boss lives.
#[derive(Component)]
struct BossBar;
/// The red fill inside the [`BossBar`].
#[derive(Component)]
struct BossBarFill;
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

const HURT_RED: Color = Color::srgb(0.85, 0.0, 0.0);

/// The flash colour; red unless a hazard burned.
#[derive(Resource, Clone, Copy, Debug)]
pub struct FlashColor(pub Color);

impl Default for FlashColor {
    fn default() -> Self {
        FlashColor(HURT_RED)
    }
}

pub fn hazard_color(kind: HazardKind) -> Color {
    match kind {
        HazardKind::Slime => Color::srgb(0.25, 0.85, 0.1),
        HazardKind::Electric => Color::srgb(0.3, 0.6, 1.0),
    }
}

/// The flash colour for a frame's events: `None` without a `PlayerHurt`; a hazard's colour when
/// a `HazardBurn` came with it; red otherwise.
pub fn hurt_tint(events: &[CombatEvent]) -> Option<Color> {
    if !events
        .iter()
        .any(|e| matches!(e, CombatEvent::PlayerHurt { .. }))
    {
        return None;
    }
    let hazard = events.iter().rev().find_map(|e| match e {
        CombatEvent::HazardBurn { kind } => Some(hazard_color(*kind)),
        _ => None,
    });
    Some(hazard.unwrap_or(HURT_RED))
}

/// Red screen flash strength in 0..=1.
#[derive(Resource, Default, Debug)]
pub struct DamageFlash(pub f32);

/// The use prompt for `t`, prefixed with the key bound to Use (`key`, e.g. `E`); empty when the
/// target has nothing to show.
pub fn prompt_label(map: &Map, mech: &Mechanics, t: UseTarget, key: &str) -> String {
    let what = match t {
        UseTarget::Mover(m) => match mech.movers[m].def.kind {
            MoverKind::Door => "Door",
            MoverKind::Lift { .. } => "Lift",
            MoverKind::Crack => return String::new(),
        },
        UseTarget::Switch(i) => match map.switches[i].action {
            SwitchAction::Exit => "Exit",
            SwitchAction::Channel(_) => "Switch",
        },
        UseTarget::Prop(i) => match (map.props[i].kind, mech.props[i].stock) {
            (PropKind::Toilet, _) => "Use toilet",
            (PropKind::Vending, 0) => "Sold out",
            (PropKind::Vending, _) => "Buy soda",
            (PropKind::PoolTable, _) => "Rack 'em",
        },
    };
    format!("[{key}] {what}")
}

/// The label of the first input bound to Use, or a dash when it is unbound.
fn use_key_label(bindings: Option<&Bindings>) -> String {
    bindings
        .and_then(|b| b.of(Action::Use).first().map(|b| b.label()))
        .unwrap_or_else(|| "\u{2014}".to_string())
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

/// Overlay message for a play state; `None` while playing. With an episode (`has_episode`) the
/// stats screen replaces the level-complete message.
pub fn overlay_text(state: PlayState, has_episode: bool) -> Option<&'static str> {
    match state {
        // Menus draw their own screens.
        PlayState::Playing | PlayState::Menu | PlayState::Paused | PlayState::EpisodeEnd => None,
        PlayState::Dead => Some("You died \u{2014} press Use or Fire to restart"),
        PlayState::Complete if has_episode => None,
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
        // Loaded here, not at Startup, so `spawn_hud` and the menus can rely on it. Without an
        // asset server (headless tests) it stays the default handle.
        if !app.world().contains_resource::<UiFont>() {
            let ui = match (
                app.world().get_resource::<AssetServer>(),
                app.world().contains_resource::<Assets<Font>>(),
            ) {
                (Some(server), true) => UiFont(server.load(UI_FONT_PATH)),
                _ => UiFont::default(),
            };
            app.insert_resource(ui);
        }
        app.init_resource::<DamageFlash>()
            .init_resource::<FlashColor>()
            .init_resource::<HudSubtitle>()
            .add_systems(Startup, spawn_hud)
            .add_systems(
                Update,
                (
                    update_prompt,
                    update_message,
                    update_subtitle,
                    update_boss_bar,
                    update_status,
                    update_keycards,
                    read_hurt.in_set(FxReaders),
                    update_flash.after(read_hurt),
                    update_overlay,
                    hide_under_menu,
                ),
            );
    }
}

/// The UI font (Kenney Future). `Handle::default()` is Bevy's built-in font, used when the asset
/// server is absent (headless tests) or the file is missing.
#[derive(Resource, Clone, Default)]
pub struct UiFont(pub Handle<Font>);

/// Path of the UI font under `assets/`.
pub const UI_FONT_PATH: &str = "fonts/kenney_future.ttf";

/// A [`TextFont`] in the UI font at `px` pixels.
pub fn font(ui: &UiFont, px: f32) -> TextFont {
    TextFont {
        font: FontSource::Handle(ui.0.clone()),
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

/// The boss bar's fill percent for the boss's health fraction; `None` when no boss lives.
pub fn boss_bar_percent(frac: Option<f32>) -> Option<f32> {
    frac.map(|f| f.clamp(0.0, 1.0) * 100.0)
}

fn update_boss_bar(
    combat: Option<Res<LevelCombat>>,
    mut bar: Single<&mut Visibility, With<BossBar>>,
    mut fill: Single<&mut Node, With<BossBarFill>>,
) {
    match boss_bar_percent(combat.and_then(|c| c.0.boss_health())) {
        Some(p) => {
            bar.set_if_neq(Visibility::Inherited);
            if fill.width != Val::Percent(p) {
                fill.width = Val::Percent(p);
            }
        }
        None => {
            bar.set_if_neq(Visibility::Hidden);
        }
    }
}

fn spawn_hud(mut commands: Commands, ui: Res<UiFont>) {
    // The boss bar sits under the message row.
    commands
        .spawn((row(Val::Px(56.0)), HudRoot))
        .with_children(|p| {
            p.spawn((
                Node {
                    width: Val::Percent(40.0),
                    height: Val::Px(12.0),
                    ..default()
                },
                BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.7)),
                Visibility::Hidden,
                BossBar,
            ))
            .with_children(|b| {
                b.spawn((
                    Node {
                        width: Val::Percent(100.0),
                        height: Val::Percent(100.0),
                        ..default()
                    },
                    BackgroundColor(Color::srgb(0.85, 0.1, 0.05)),
                    BossBarFill,
                ));
            });
        });
    commands
        .spawn((row(Val::Percent(60.0)), HudRoot))
        .with_children(|p| {
            p.spawn((
                Text::new(""),
                font(&ui, 22.0),
                TextColor(Color::WHITE),
                PromptText,
            ));
        });
    commands
        .spawn((row(Val::Px(24.0)), HudRoot))
        .with_children(|p| {
            p.spawn((
                Text::new(""),
                font(&ui, 28.0),
                TextColor(Color::srgb(1.0, 0.85, 0.2)),
                MessageText,
            ));
        });
    // Subtitles sit low, above the status bar, out of the way of the prompt and messages.
    commands
        .spawn((row(Val::Percent(74.0)), HudRoot))
        .with_children(|p| {
            p.spawn((
                Text::new(""),
                font(&ui, 20.0),
                TextColor(Color::srgb(0.85, 0.9, 1.0)),
                SubtitleText,
            ));
        });

    // Crosshair: two thin bars, centred.
    commands
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                width: Val::Percent(100.0),
                height: Val::Percent(100.0),
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                ..default()
            },
            HudRoot,
        ))
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
    commands.spawn((
        fill_screen(5, Color::srgba(0.85, 0.0, 0.0, 0.0)),
        FlashNode,
        HudRoot,
    ));

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
            HudRoot,
        ))
        .with_children(|bar| {
            let cell = |p: &mut ChildSpawnerCommands, kind: StatusCell, text: &str| {
                p.spawn((
                    Text::new(text),
                    font(&ui, 26.0),
                    TextColor(Color::WHITE),
                    kind,
                ));
            };
            cell(bar, StatusCell::Health, "HEALTH 100");
            cell(bar, StatusCell::Armour, "ARMOUR 0");
            cell(bar, StatusCell::Ammo, "AMMO \u{2014}");
            bar.spawn(Node {
                column_gap: Val::Px(10.0),
                align_items: AlignItems::Center,
                min_width: Val::Px(150.0),
                flex_shrink: 0.0,
                ..default()
            })
            .with_children(|w| {
                w.spawn((
                    Text::new(""),
                    font(&ui, 26.0),
                    TextColor(Color::WHITE),
                    TextLayout::no_wrap(),
                    StatusCell::WeaponName,
                ));
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
                        font(&ui, 18.0),
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
                font(&ui, 36.0),
                TextColor(Color::WHITE),
                OverlayText,
            ));
        });
}

// The helpers take `Mut<T>` and compare through `Deref`: coercing a `Mut<T>` to `&mut T`
// goes through `DerefMut`, which flags the component changed before any comparison.

/// Hides the HUD while a menu screen is shown. (Without menus, as in demos, it stays.)
fn hide_under_menu(
    screen: Option<Res<crate::menu::MenuScreen>>,
    mut roots: Query<&mut Visibility, With<HudRoot>>,
) {
    let wanted = if screen.is_some_and(|s| s.0.is_some()) {
        Visibility::Hidden
    } else {
        Visibility::Inherited
    };
    for mut vis in &mut roots {
        vis.set_if_neq(wanted);
    }
}

fn set(text: &mut Mut<Text>, s: &str) {
    if text.0 != s {
        text.0 = s.to_string();
    }
}

fn set_color(color: &mut Mut<TextColor>, c: Color) {
    color.set_if_neq(TextColor(c));
}

fn set_bg(bg: &mut Mut<BackgroundColor>, c: Color) {
    bg.set_if_neq(BackgroundColor(c));
}

fn update_prompt(
    prompt: Res<UsePrompt>,
    map: Res<CurrentMap>,
    mech: Res<LevelMechanics>,
    state: Res<PlayState>,
    bindings: Option<Res<Bindings>>,
    mut text: Single<&mut Text, With<PromptText>>,
) {
    let label = match prompt.0 {
        Some(t) if *state == PlayState::Playing => {
            prompt_label(&map.0, &mech.0, t, &use_key_label(bindings.as_deref()))
        }
        _ => String::new(),
    };
    set(&mut text, &label);
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

fn update_subtitle(
    time: Res<Time>,
    mut sub: ResMut<HudSubtitle>,
    mut text: Single<&mut Text, With<SubtitleText>>,
) {
    sub.remaining = (sub.remaining - time.delta_secs()).max(0.0);
    let s = if sub.remaining > 0.0 {
        sub.text.as_str()
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
            StatusCell::WeaponName => set(&mut text, defs.0.weapon(a.current).label()),
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
        set_bg(&mut bg, key_color(card.0).with_alpha(alpha));
    }
}

/// An [`FxReaders`] system: it only reads the queue.
fn read_hurt(fx: Res<FxQueue>, mut flash: ResMut<DamageFlash>, mut color: ResMut<FlashColor>) {
    if let Some(c) = hurt_tint(&fx.combat) {
        color.0 = c;
    }
    for ev in &fx.combat {
        if let CombatEvent::PlayerHurt { amount, .. } = ev {
            flash.0 = flash_add(flash.0, *amount);
        }
    }
}

fn update_flash(
    time: Res<Time>,
    mut flash: ResMut<DamageFlash>,
    color: Res<FlashColor>,
    mut node: Single<&mut BackgroundColor, With<FlashNode>>,
) {
    set_bg(&mut node, color.0.with_alpha(flash.0 * FLASH_ALPHA));
    if flash.0 > 0.0 {
        flash.0 = flash_decay(flash.0, time.delta_secs());
    }
}

fn update_overlay(
    state: Res<PlayState>,
    episode: Option<Res<crate::episode::Episode>>,
    mut node: Single<(&mut Visibility, &mut BackgroundColor), With<OverlayNode>>,
    mut text: Single<&mut Text, With<OverlayText>>,
) {
    let (vis, bg) = &mut *node;
    match overlay_text(*state, episode.is_some()) {
        Some(s) => {
            vis.set_if_neq(Visibility::Inherited);
            set_bg(bg, overlay_tint(*state));
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
    use rr_core::hazard::HazardKind;

    #[test]
    fn hazard_burns_tint_the_flash() {
        let hurt = CombatEvent::PlayerHurt {
            amount: 4,
            from: Vec3::ZERO,
        };
        assert_eq!(hurt_tint(&[]), None);
        assert_eq!(hurt_tint(std::slice::from_ref(&hurt)), Some(HURT_RED));
        let slime = CombatEvent::HazardBurn {
            kind: HazardKind::Slime,
        };
        assert_eq!(
            hurt_tint(&[slime, hurt]),
            Some(hazard_color(HazardKind::Slime))
        );
    }

    use rr_core::fixtures::{door_rooms, lift_shaft};

    #[test]
    fn boss_bar_follows_the_boss() {
        assert_eq!(boss_bar_percent(None), None);
        assert_eq!(boss_bar_percent(Some(0.5)), Some(50.0));
        assert_eq!(boss_bar_percent(Some(-0.2)), Some(0.0));
    }

    #[test]
    fn prompt_names_the_target() {
        let mut map = door_rooms(
            "(kind: Door)",
            "switches: [(wall: (7, 0), action: Exit), (wall: (3, 4), action: Channel(2))],",
        );
        let mech = Mechanics::new(&mut map);
        assert_eq!(
            prompt_label(&map, &mech, UseTarget::Mover(0), "E"),
            "[E] Door"
        );
        assert_eq!(
            prompt_label(&map, &mech, UseTarget::Switch(0), "E"),
            "[E] Exit"
        );
        assert_eq!(
            prompt_label(&map, &mech, UseTarget::Switch(1), "X"),
            "[X] Switch"
        );
        let mut map = rr_core::fixtures::engine_room("");
        let mut mech = Mechanics::new(&mut map);
        assert_eq!(
            prompt_label(&map, &mech, UseTarget::Prop(0), "E"),
            "[E] Use toilet"
        );
        assert_eq!(
            prompt_label(&map, &mech, UseTarget::Prop(1), "E"),
            "[E] Buy soda"
        );
        assert_eq!(
            prompt_label(&map, &mech, UseTarget::Prop(2), "E"),
            "[E] Rack 'em"
        );
        mech.props[1].stock = 0;
        assert_eq!(
            prompt_label(&map, &mech, UseTarget::Prop(1), "E"),
            "[E] Sold out"
        );
        let mut b = Bindings::default();
        assert_eq!(use_key_label(Some(&b)), "E");
        b.bind(Action::Use, crate::bindings::Binding::Key(KeyCode::KeyG));
        assert_eq!(use_key_label(Some(&b)), "G");
        assert_eq!(use_key_label(None), "\u{2014}");
        let mut map = lift_shaft("(kind: Lift(to: 2.0))", "");
        let mech = Mechanics::new(&mut map);
        assert_eq!(
            prompt_label(&map, &mech, UseTarget::Mover(0), "E"),
            "[E] Lift"
        );
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
        assert_eq!(overlay_text(PlayState::Playing, false), None);
        for s in [PlayState::Menu, PlayState::Paused, PlayState::EpisodeEnd] {
            assert_eq!(overlay_text(s, false), None, "{s:?}");
        }
        assert_eq!(
            overlay_text(PlayState::Dead, true),
            Some("You died — press Use or Fire to restart")
        );
        assert_eq!(
            overlay_text(PlayState::Complete, false),
            Some("Level complete — press Use or Fire to restart")
        );
        // The stats screen replaces it in an episode.
        assert_eq!(overlay_text(PlayState::Complete, true), None);
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
