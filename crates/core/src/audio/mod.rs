//! Sound cues: every audible gameplay event as a typed `Cue` with an optional world position.
//! The pure event-to-cue mapping lives in `cues`; the game looks each cue up in its sound bank.

mod bank;
mod cues;
mod footsteps;
mod powers;
mod quips;

pub use bank::{SoundBankDef, SoundDef, gain};
pub use cues::{combat_cues, mech_cues, weapon_cues};
pub use footsteps::Footsteps;
pub use powers::PowerWatch;
pub use quips::{
    COOLDOWN, KILL_CHANCE, LOW_HEALTH, LowHealthWatch, MULTI_KILL_WINDOW, Quip, QuipDirector,
    QuipError, QuipOn, QuipTable, QuipTrigger, REARM_HEALTH,
};

use crate::defs::WeaponId;
use crate::map::{ActorKind, ItemKind};
use glam::Vec3;
use serde::{Deserialize, Serialize};

/// Where a cue sounds. `None` means at the listener (the player), without spatialisation.
pub type CuePos = Option<Vec3>;

/// What kind of thing a pickup was, for choosing its sound.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum PickupClass {
    Weapon,
    Ammo,
    Health,
    Armour,
    Key,
    Power,
}

impl PickupClass {
    pub const ALL: [PickupClass; 6] = [
        PickupClass::Weapon,
        PickupClass::Ammo,
        PickupClass::Health,
        PickupClass::Armour,
        PickupClass::Key,
        PickupClass::Power,
    ];
}

impl From<ItemKind> for PickupClass {
    fn from(k: ItemKind) -> Self {
        // Exhaustive on purpose: a new item kind must pick its sound class.
        match k {
            ItemKind::Shotgun | ItemKind::Chaingun | ItemKind::RocketLauncher => Self::Weapon,
            ItemKind::PistolAmmo
            | ItemKind::ShotgunShells
            | ItemKind::Rockets
            | ItemKind::PipeBombs => Self::Ammo,
            ItemKind::HealthSmall | ItemKind::Medkit | ItemKind::Atom => Self::Health,
            ItemKind::Armour => Self::Armour,
            ItemKind::Key(_) => Self::Key,
            ItemKind::Jetpack | ItemKind::NightVision => Self::Power,
        }
    }
}

/// One sound the game can play. `all()` lists exactly the cues the mapping in `cues` can emit,
/// plus the ones the game raises itself (noted on the variants).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Cue {
    Fire(WeaponId),
    DryFire,
    Reload,
    /// Weapon switch (not the wall switch, see `SwitchUse`).
    Switch,
    Kick,
    Impact,
    Explosion,
    GlassBreak,
    LightBreak,
    CrackOpen,
    ActorWake(ActorKind),
    ActorFire(ActorKind),
    ActorPain(ActorKind),
    /// Never for a barrel: its blast sounds as `Explosion`.
    ActorDeath(ActorKind),
    PlayerHurt,
    PlayerDeath,
    DoorStart,
    DoorStop,
    LiftStart,
    LiftStop,
    /// A wall switch was used.
    SwitchUse,
    /// A locked door or switch refused the player (no key).
    Denied,
    Pickup(PickupClass),
    Secret,
    LevelComplete,
    /// Raised by the game when the jetpack state turns on (no core event).
    JetpackStart,
    /// Raised by the game while the jetpack is burning (no core event).
    JetpackLoop,
    /// Raised by the game when the jetpack state turns off (no core event).
    JetpackStop,
    /// Raised by the game when night vision turns on (no core event).
    NightVisionOn,
    /// Raised by the game when night vision turns off (no core event).
    NightVisionOff,
    /// Raised by the game from player movement (no core event).
    Footstep,
    /// Raised by the game from player movement (no core event).
    Land,
}

impl Cue {
    /// Every cue, with `WeaponId`, `ActorKind` and `PickupClass` expanded. Exactly the cues the
    /// mapping can emit or the game raises itself, with no duplicates. Left out because they
    /// never occur: every `Actor*` cue for `Barrel` (a static body; `combat_cues` drops those
    /// events for it, death included). `Fire(Boot)` stays: the boot swing emits `Fire`.
    pub fn all() -> Vec<Cue> {
        use Cue::*;
        let mut v: Vec<Cue> = WeaponId::ALL.into_iter().map(Fire).collect();
        v.extend([
            DryFire, Reload, Switch, Kick, Impact, Explosion, GlassBreak, LightBreak, CrackOpen,
        ]);
        for k in ActorKind::ALL {
            if k != ActorKind::Barrel {
                v.extend([ActorWake(k), ActorFire(k), ActorPain(k), ActorDeath(k)]);
            }
        }
        v.extend([
            PlayerHurt,
            PlayerDeath,
            DoorStart,
            DoorStop,
            LiftStart,
            LiftStop,
            SwitchUse,
            Denied,
        ]);
        v.extend(PickupClass::ALL.into_iter().map(Pickup));
        v.extend([
            Secret,
            LevelComplete,
            JetpackStart,
            JetpackLoop,
            JetpackStop,
            NightVisionOn,
            NightVisionOff,
            Footstep,
            Land,
        ]);
        v
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::map::Key as KeyColour;
    use std::collections::HashSet;

    #[test]
    fn pickup_class_for_every_item_kind() {
        use ItemKind::*;
        use PickupClass as P;
        let table = [
            (Key(KeyColour::Red), P::Key),
            (PistolAmmo, P::Ammo),
            (ShotgunShells, P::Ammo),
            (Shotgun, P::Weapon),
            (HealthSmall, P::Health),
            (Chaingun, P::Weapon),
            (RocketLauncher, P::Weapon),
            (Rockets, P::Ammo),
            (PipeBombs, P::Ammo),
            (Armour, P::Armour),
            (Medkit, P::Health),
            (Atom, P::Health),
            (Jetpack, P::Power),
            (NightVision, P::Power),
        ];
        for (k, c) in table {
            assert_eq!(PickupClass::from(k), c, "{k:?}");
        }
        // Compile-time guard that the table above covers every kind.
        fn _exhaustive(k: ItemKind) {
            match k {
                Key(_) | PistolAmmo | ShotgunShells | Shotgun | HealthSmall | Chaingun
                | RocketLauncher | Rockets | PipeBombs | Armour | Medkit | Atom | Jetpack
                | NightVision => {}
            }
        }
    }

    #[test]
    fn all_has_no_duplicates() {
        let all = Cue::all();
        let set: HashSet<_> = all.iter().collect();
        assert_eq!(set.len(), all.len());
    }

    #[test]
    fn cue_round_trips_through_ron() {
        for c in [
            Cue::Fire(WeaponId::Pistol),
            Cue::ActorDeath(ActorKind::Grunt),
            Cue::Pickup(PickupClass::Key),
            Cue::DryFire,
        ] {
            let s = ron::to_string(&c).unwrap();
            assert_eq!(ron::from_str::<Cue>(&s).unwrap(), c, "{s}");
        }
        for c in Cue::all() {
            let s = ron::to_string(&c).unwrap();
            assert_eq!(ron::from_str::<Cue>(&s).unwrap(), c, "{s}");
        }
    }
}
