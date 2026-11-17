//! The text fields of the co-op screens: the player name and the join address.
//!
//! A field is edited like a key capture: [`MenuAction::Edit`] starts it, typed text (Bevy
//! `KeyboardInput` text) appends, Backspace deletes, Enter or Esc (or any other button) ends it.
//! Menu navigation ignores keys meanwhile.

use bevy::ecs::system::SystemParam;
use bevy::input::ButtonState;
use bevy::input::keyboard::KeyboardInput;
use bevy::prelude::*;
use rr_net::MAX_NAME;

use super::{MenuAction, MenuScreen, Screen};
use crate::settings::{SaveSettings, Settings, default_player_name};

/// A text field of the co-op screens.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Field {
    /// `Settings::player_name`, on the Multiplayer screen.
    Name,
    /// [`JoinAddress`], on the Join screen.
    Address,
}

impl Field {
    fn screen(self) -> Screen {
        match self {
            Field::Name => Screen::Multiplayer,
            Field::Address => Screen::Join,
        }
    }
}

/// The field being typed into, if any.
#[derive(Resource, Default, Debug, PartialEq, Eq)]
pub struct Editing(pub Option<Field>);

/// The longest address the field takes.
pub const MAX_ADDRESS: usize = 64;

/// The Join screen's address field.
#[derive(Resource, Debug)]
pub struct JoinAddress(pub String);

impl Default for JoinAddress {
    fn default() -> Self {
        Self("127.0.0.1".into())
    }
}

/// `--name`: the name for this run only, never saved. Editing the name field replaces it.
#[derive(Resource, Debug)]
pub struct NameOverride(pub String);

/// The text of a field button.
#[derive(Component)]
pub struct FieldText(pub Field);

/// Appends `text` to a name, keeping it within [`MAX_NAME`] bytes; control characters are dropped.
pub fn push_name(name: &mut String, text: &str) {
    for c in text.chars().filter(|c| !c.is_control()) {
        if name.len() + c.len_utf8() > MAX_NAME {
            break;
        }
        name.push(c);
    }
}

/// Appends `text` to an address, keeping hex digits, `.`, `:` and brackets, at most
/// [`MAX_ADDRESS`] characters.
pub fn push_address(addr: &mut String, text: &str) {
    for c in text.chars() {
        let ok = c.is_ascii_hexdigit() || matches!(c, '.' | ':' | '[' | ']');
        if ok && addr.chars().count() < MAX_ADDRESS {
            addr.push(c);
        }
    }
}

/// What the field texts show.
#[derive(SystemParam)]
pub struct FieldValues<'w> {
    settings: Option<Res<'w, Settings>>,
    name_override: Option<Res<'w, NameOverride>>,
    address: Res<'w, JoinAddress>,
    editing: Res<'w, Editing>,
}

/// Keeps the field texts current ("Name: Rex", with a caret while editing).
pub fn refresh_fields(src: FieldValues, mut fields: Query<(&mut Text, &FieldText)>) {
    for (mut text, FieldText(field)) in &mut fields {
        let (name, value) = match field {
            Field::Name => (
                "Name",
                src.name_override.as_ref().map_or_else(
                    || src.settings.as_ref().map_or("", |s| s.player_name.as_str()),
                    |n| n.0.as_str(),
                ),
            ),
            Field::Address => ("Address", src.address.0.as_str()),
        };
        let caret = if src.editing.0 == Some(*field) {
            "_"
        } else {
            ""
        };
        let wanted = format!("{name}: {value}{caret}");
        if text.0 != wanted {
            text.0 = wanted;
        }
    }
}

/// Ends editing `field`: an emptied name falls back to the default, and a name is saved.
fn end_edit(field: Field, settings: Option<&mut Settings>, save: Option<&mut SaveSettings>) {
    if field != Field::Name {
        return;
    }
    if let Some(settings) = settings
        && settings.player_name.trim().is_empty()
    {
        settings.player_name = default_player_name();
    }
    if let Some(save) = save {
        save.0 = true;
    }
}

/// Types into the field being edited. The frame editing starts is skipped, so the Enter that
/// chose the field does not end it.
#[allow(clippy::too_many_arguments)]
pub fn text_input(
    mut typed: MessageReader<KeyboardInput>,
    mut editing: ResMut<Editing>,
    mut settings: Option<ResMut<Settings>>,
    mut save: Option<ResMut<SaveSettings>>,
    mut address: ResMut<JoinAddress>,
    screen: Res<MenuScreen>,
    mut armed: Local<bool>,
) {
    let Some(field) = editing.0 else {
        typed.clear();
        *armed = false;
        return;
    };
    let mut done = screen.0 != Some(field.screen());
    if !*armed && !done {
        typed.clear();
        *armed = true;
        return;
    }
    for ev in typed.read() {
        if done || ev.state != ButtonState::Pressed {
            continue;
        }
        match ev.key_code {
            KeyCode::Enter | KeyCode::NumpadEnter | KeyCode::Escape => done = true,
            KeyCode::Backspace => match field {
                Field::Name => {
                    if let Some(s) = settings.as_mut() {
                        s.player_name.pop();
                    }
                }
                Field::Address => {
                    address.0.pop();
                }
            },
            _ => {
                let Some(text) = &ev.text else { continue };
                match field {
                    Field::Name => {
                        if let Some(s) = settings.as_mut() {
                            push_name(&mut s.player_name, text);
                        }
                    }
                    Field::Address => push_address(&mut address.0, text),
                }
            }
        }
    }
    if done {
        editing.0 = None;
        *armed = false;
        end_edit(field, settings.as_deref_mut(), save.as_deref_mut());
    }
}

/// Ends any edit before another button acts; `Edit` toggles or switches the field.
pub fn edit_action(world: &mut World, action: MenuAction) {
    let current = world.resource::<Editing>().0;
    if let Some(field) = current {
        world.resource_mut::<Editing>().0 = None;
        let mut settings = world.remove_resource::<Settings>();
        let mut save = world.remove_resource::<SaveSettings>();
        end_edit(field, settings.as_mut(), save.as_mut());
        // Back in place (a remove and insert, so the settings count as changed and are applied).
        if let Some(settings) = settings {
            world.insert_resource(settings);
        }
        if let Some(save) = save {
            world.insert_resource(save);
        }
    }
    if let MenuAction::Edit(field) = action
        && current != Some(field)
    {
        if field == Field::Name
            && let Some(name) = world.remove_resource::<NameOverride>()
            && let Some(mut settings) = world.get_resource_mut::<Settings>()
        {
            settings.player_name = name.0;
        }
        world.resource_mut::<Editing>().0 = Some(field);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_stay_within_the_byte_cap_on_a_char_boundary() {
        let mut name = "a".repeat(22);
        push_name(&mut name, "b");
        assert_eq!(name.len(), 23);
        // 'é' is two bytes: it would end at byte 25.
        push_name(&mut name, "é");
        assert_eq!(name.len(), 23);
        push_name(&mut name, "c");
        assert_eq!(name.len(), MAX_NAME);
        push_name(&mut name, "d");
        assert_eq!(name.len(), MAX_NAME);
        let mut name = String::new();
        push_name(&mut name, "Zoë\t\n");
        assert_eq!(name, "Zoë");
    }

    #[test]
    fn addresses_take_only_address_characters() {
        let mut addr = String::new();
        push_address(&mut addr, "[::1]:27960");
        assert_eq!(addr, "[::1]:27960");
        let mut addr = String::new();
        push_address(&mut addr, "10.0.0.2 xyz/ ;");
        assert_eq!(addr, "10.0.0.2");
        let mut addr = "1".repeat(MAX_ADDRESS - 1);
        push_address(&mut addr, "234");
        assert_eq!(addr.chars().count(), MAX_ADDRESS);
        assert!(addr.ends_with('2'));
    }
}
