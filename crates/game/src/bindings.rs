//! Player actions and the keys or mouse buttons bound to them (saved in `settings.ron`).

use bevy::prelude::*;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Everything the player can do with a key or mouse button.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum Action {
    Forward,
    Back,
    StrafeLeft,
    StrafeRight,
    Jump,
    Crouch,
    Use,
    Fire,
    Reload,
    Kick,
    Medkit,
    Jetpack,
    NightVision,
    Weapon1,
    Weapon2,
    Weapon3,
    Weapon4,
    Weapon5,
    Weapon6,
    Pause,
}

impl Action {
    /// Every action, in controls-screen order.
    pub const ALL: [Action; 20] = [
        Action::Forward,
        Action::Back,
        Action::StrafeLeft,
        Action::StrafeRight,
        Action::Jump,
        Action::Crouch,
        Action::Use,
        Action::Fire,
        Action::Reload,
        Action::Kick,
        Action::Medkit,
        Action::Jetpack,
        Action::NightVision,
        Action::Weapon1,
        Action::Weapon2,
        Action::Weapon3,
        Action::Weapon4,
        Action::Weapon5,
        Action::Weapon6,
        Action::Pause,
    ];

    /// Name shown on the controls screen.
    pub fn label(self) -> &'static str {
        match self {
            Action::Forward => "Move forward",
            Action::Back => "Move back",
            Action::StrafeLeft => "Strafe left",
            Action::StrafeRight => "Strafe right",
            Action::Jump => "Jump / jetpack up",
            Action::Crouch => "Crouch / jetpack down",
            Action::Use => "Use",
            Action::Fire => "Fire",
            Action::Reload => "Reload",
            Action::Kick => "Quick kick",
            Action::Medkit => "Medkit",
            Action::Jetpack => "Jetpack",
            Action::NightVision => "Night vision",
            Action::Weapon1 => "Boot",
            Action::Weapon2 => "Pistol",
            Action::Weapon3 => "Shotgun",
            Action::Weapon4 => "Chaingun",
            Action::Weapon5 => "Rockets",
            Action::Weapon6 => "Pipe bombs",
            Action::Pause => "Pause / menu",
        }
    }
}

/// A physical input an action can be bound to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Binding {
    Key(KeyCode),
    Mouse(MouseButton),
}

impl Binding {
    /// Short label for the controls screen: `KeyW` -> `W`, `Digit1` -> `1`, `Left` mouse -> `Mouse 1`.
    pub fn label(self) -> String {
        match self {
            Binding::Key(k) => {
                let s = format!("{k:?}");
                s.strip_prefix("Key")
                    .or(s.strip_prefix("Digit"))
                    .unwrap_or(&s)
                    .to_string()
            }
            Binding::Mouse(MouseButton::Left) => "Mouse 1".into(),
            Binding::Mouse(MouseButton::Right) => "Mouse 2".into(),
            Binding::Mouse(MouseButton::Middle) => "Mouse 3".into(),
            Binding::Mouse(b) => format!("{b:?}"),
        }
    }
}

/// Escape always opens the menu; nothing else may take it.
const PAUSE_KEY: Binding = Binding::Key(KeyCode::Escape);
const MAX_PER_ACTION: usize = 2;

/// The action table: up to two bindings per action, newest first.
#[derive(Resource, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Bindings(BTreeMap<Action, Vec<Binding>>);

impl Default for Bindings {
    fn default() -> Self {
        use Action::*;
        use KeyCode as K;
        let k = Binding::Key;
        let m = Binding::Mouse;
        Bindings(BTreeMap::from([
            (Forward, vec![k(K::KeyW)]),
            (Back, vec![k(K::KeyS)]),
            (StrafeLeft, vec![k(K::KeyA)]),
            (StrafeRight, vec![k(K::KeyD)]),
            (Jump, vec![k(K::Space)]),
            (Crouch, vec![k(K::KeyC), k(K::ControlLeft)]),
            (Use, vec![k(K::KeyE)]),
            (Fire, vec![m(MouseButton::Left)]),
            (Reload, vec![k(K::KeyR)]),
            (Kick, vec![k(K::KeyF)]),
            (Medkit, vec![k(K::KeyQ)]),
            (Jetpack, vec![k(K::KeyJ)]),
            (NightVision, vec![k(K::KeyN)]),
            (Weapon1, vec![k(K::Digit1)]),
            (Weapon2, vec![k(K::Digit2)]),
            (Weapon3, vec![k(K::Digit3)]),
            (Weapon4, vec![k(K::Digit4)]),
            (Weapon5, vec![k(K::Digit5)]),
            (Weapon6, vec![k(K::Digit6)]),
            (Pause, vec![PAUSE_KEY]),
        ]))
    }
}

impl Bindings {
    /// The inputs bound to `a`, newest first.
    pub fn of(&self, a: Action) -> &[Binding] {
        self.0.get(&a).map_or(&[], Vec::as_slice)
    }

    /// Binds `b` to `a` (newest first, at most two). A binding used by another action is moved;
    /// returns that action. Escape stays on Pause.
    pub fn bind(&mut self, a: Action, b: Binding) -> Option<Action> {
        if b == PAUSE_KEY || a == Action::Pause {
            return None;
        }
        let mut taken = None;
        for (other, list) in self.0.iter_mut() {
            if *other != a && list.contains(&b) {
                list.retain(|x| *x != b);
                taken = Some(*other);
            }
        }
        let list = self.0.entry(a).or_default();
        list.retain(|x| *x != b);
        list.insert(0, b);
        list.truncate(MAX_PER_ACTION);
        taken
    }

    /// Repairs a loaded table: Escape back on Pause and nowhere else, at most two per action.
    pub fn sanitise(&mut self) {
        for (a, list) in self.0.iter_mut() {
            if *a != Action::Pause {
                list.retain(|x| *x != PAUSE_KEY);
            }
            list.truncate(MAX_PER_ACTION);
        }
        self.0.insert(Action::Pause, vec![PAUSE_KEY]);
    }

    /// Is any input bound to `a` held down?
    pub fn pressed(
        &self,
        a: Action,
        keys: &ButtonInput<KeyCode>,
        mouse: &ButtonInput<MouseButton>,
    ) -> bool {
        self.of(a).iter().any(|b| match *b {
            Binding::Key(k) => keys.pressed(k),
            Binding::Mouse(m) => mouse.pressed(m),
        })
    }

    /// Did any input bound to `a` go down this frame?
    pub fn just_pressed(
        &self,
        a: Action,
        keys: &ButtonInput<KeyCode>,
        mouse: &ButtonInput<MouseButton>,
    ) -> bool {
        self.of(a).iter().any(|b| match *b {
            Binding::Key(k) => keys.just_pressed(k),
            Binding::Mouse(m) => mouse.just_pressed(m),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_match_the_old_hardcoded_keys() {
        let b = Bindings::default();
        assert_eq!(b.of(Action::Forward), &[Binding::Key(KeyCode::KeyW)]);
        assert_eq!(
            b.of(Action::Crouch),
            &[
                Binding::Key(KeyCode::KeyC),
                Binding::Key(KeyCode::ControlLeft)
            ]
        );
        assert_eq!(b.of(Action::Fire), &[Binding::Mouse(MouseButton::Left)]);
        assert_eq!(b.of(Action::Pause), &[Binding::Key(KeyCode::Escape)]);
    }

    #[test]
    fn binding_a_used_key_moves_it() {
        let mut b = Bindings::default();
        let taken = b.bind(Action::Jump, Binding::Key(KeyCode::KeyE));
        assert_eq!(taken, Some(Action::Use));
        assert_eq!(b.of(Action::Use), &[] as &[Binding]);
        assert_eq!(b.of(Action::Jump)[0], Binding::Key(KeyCode::KeyE));
    }

    #[test]
    fn at_most_two_bindings_newest_first() {
        let mut b = Bindings::default();
        b.bind(Action::Jump, Binding::Key(KeyCode::KeyX));
        b.bind(Action::Jump, Binding::Key(KeyCode::KeyZ));
        assert_eq!(
            b.of(Action::Jump),
            &[Binding::Key(KeyCode::KeyZ), Binding::Key(KeyCode::KeyX)]
        );
    }

    #[test]
    fn pause_cannot_be_unbound_from_escape() {
        let mut b = Bindings::default();
        b.bind(Action::Jump, Binding::Key(KeyCode::Escape));
        assert_eq!(b.of(Action::Pause), &[Binding::Key(KeyCode::Escape)]);
        assert_ne!(b.of(Action::Jump)[0], Binding::Key(KeyCode::Escape));
    }
}
