//! What each world item does when the player walks over it. Plain data in, HUD message out;
//! the game decides when to call it (see `Mechanics::pickup`'s `accept` hook).

use crate::defs::{AmmoKind, Defs, WeaponId};
use crate::health::PLAYER_MAX_HEALTH;
use crate::inventory::Inventory;
use crate::map::{ItemKind, KeySet};
use crate::vitals::{OVERCHARGE_MAX, Vitals};
use crate::weapons::Arsenal;

/// Hit points a small health pack restores.
pub const HEALTH_SMALL: i32 = 10;
pub const ARMOUR_PICKUP: i32 = 50;
pub const MEDKIT_PICKUP: i32 = 50;
pub const ATOM_HEAL: i32 = 50;
pub const FUEL_PICKUP: f32 = 100.0;
pub const BATTERY_PICKUP: f32 = 100.0;

/// The player state a pickup can change.
pub struct Loadout<'a> {
    pub vitals: &'a mut Vitals,
    pub arsenal: &'a mut Arsenal,
    pub keys: &'a mut KeySet,
    pub inventory: &'a mut Inventory,
}

/// Applies `kind` to `l`. Returns the HUD message, or `None` when nothing changed (the item
/// should then stay in the world). Keys always apply.
pub fn apply_pickup(defs: &Defs, kind: ItemKind, l: &mut Loadout) -> Option<String> {
    match kind {
        ItemKind::Key(k) => {
            l.keys.insert(k);
            Some(format!("Picked up the {} keycard", k.name()))
        }
        ItemKind::PistolAmmo => {
            let n = defs.ammo(AmmoKind::Bullets).pickup;
            l.arsenal
                .give_ammo(defs, AmmoKind::Bullets, n)
                .then(|| "Pistol ammo".to_string())
        }
        ItemKind::ShotgunShells => {
            let n = defs.ammo(AmmoKind::Shells).pickup;
            l.arsenal
                .give_ammo(defs, AmmoKind::Shells, n)
                .then(|| "Shotgun shells".to_string())
        }
        ItemKind::Shotgun => {
            let newly = !l.arsenal.owned[WeaponId::Shotgun.index()];
            l.arsenal.give_weapon(defs, WeaponId::Shotgun).then(|| {
                if newly {
                    "Picked up a shotgun!"
                } else {
                    "Shotgun shells"
                }
                .to_string()
            })
        }
        ItemKind::HealthSmall => l
            .vitals
            .heal_to(HEALTH_SMALL, PLAYER_MAX_HEALTH)
            .then(|| format!("+{HEALTH_SMALL} health")),
        ItemKind::Chaingun => weapon(defs, l, WeaponId::Chaingun, "Picked up the chaingun"),
        ItemKind::RocketLauncher => {
            weapon(defs, l, WeaponId::Rockets, "Picked up the rocket launcher")
        }
        ItemKind::PipeBombs => weapon(defs, l, WeaponId::PipeBombs, "Picked up pipe bombs"),
        ItemKind::Rockets => {
            let n = defs.ammo(AmmoKind::Rockets).pickup;
            l.arsenal
                .give_ammo(defs, AmmoKind::Rockets, n)
                .then(|| "Rockets".to_string())
        }
        ItemKind::Armour => l
            .vitals
            .add_armour(ARMOUR_PICKUP)
            .then(|| "Armour".to_string()),
        ItemKind::Medkit => l
            .inventory
            .add_medkit(MEDKIT_PICKUP)
            .then(|| "Medkit".to_string()),
        ItemKind::Atom => l
            .vitals
            .heal_to(ATOM_HEAL, OVERCHARGE_MAX)
            .then(|| "Atomic health!".to_string()),
        ItemKind::Jetpack => l
            .inventory
            .add_fuel(FUEL_PICKUP)
            .then(|| "Jetpack".to_string()),
        ItemKind::NightVision => l
            .inventory
            .add_battery(BATTERY_PICKUP)
            .then(|| "Night vision goggles".to_string()),
    }
}

/// Gives a weapon (and its pickup ammo); a repeat pickup only counts if it added ammo.
fn weapon(defs: &Defs, l: &mut Loadout, w: WeaponId, msg: &str) -> Option<String> {
    let newly = !l.arsenal.owned[w.index()];
    l.arsenal
        .give_weapon(defs, w)
        .then(|| if newly { msg } else { "Ammo" }.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::defs::Defs;
    use crate::map::Key;

    struct Rig {
        defs: Defs,
        vitals: Vitals,
        inv: Inventory,
        arsenal: Arsenal,
        keys: KeySet,
    }

    impl Rig {
        fn new() -> Rig {
            let defs = Defs::builtin();
            Rig {
                vitals: Vitals::new(),
                inv: Inventory::new(),
                arsenal: Arsenal::new(&defs),
                keys: KeySet::default(),
                defs,
            }
        }
        fn pick(&mut self, kind: ItemKind) -> Option<String> {
            let mut l = Loadout {
                vitals: &mut self.vitals,
                arsenal: &mut self.arsenal,
                keys: &mut self.keys,
                inventory: &mut self.inv,
            };
            apply_pickup(&self.defs, kind, &mut l)
        }
    }

    #[test]
    fn key_always_applies() {
        let mut r = Rig::new();
        assert_eq!(
            r.pick(ItemKind::Key(Key::Red)).as_deref(),
            Some("Picked up the red keycard")
        );
        assert!(r.keys.contains(Key::Red));
        assert!(
            r.pick(ItemKind::Key(Key::Red)).is_some(),
            "even a duplicate"
        );
    }

    #[test]
    fn full_health_leaves_health_pack() {
        let mut r = Rig::new();
        assert_eq!(r.pick(ItemKind::HealthSmall), None);
        r.vitals.health.hp = 95;
        assert_eq!(r.pick(ItemKind::HealthSmall).as_deref(), Some("+10 health"));
        assert_eq!(r.vitals.health.hp, 100, "capped at max");
        assert_eq!(r.pick(ItemKind::HealthSmall), None);
    }

    #[test]
    fn ammo_at_max_stays() {
        let mut r = Rig::new();
        let max = r.defs.ammo(AmmoKind::Bullets).max;
        r.arsenal.reserve[AmmoKind::Bullets.index()] = max - 1;
        assert_eq!(r.pick(ItemKind::PistolAmmo).as_deref(), Some("Pistol ammo"));
        assert_eq!(r.arsenal.reserve[AmmoKind::Bullets.index()], max);
        assert_eq!(r.pick(ItemKind::PistolAmmo), None);
        let max = r.defs.ammo(AmmoKind::Shells).max;
        r.arsenal.reserve[AmmoKind::Shells.index()] = max;
        assert_eq!(r.pick(ItemKind::ShotgunShells), None);
        r.arsenal.reserve[AmmoKind::Shells.index()] = 0;
        assert_eq!(
            r.pick(ItemKind::ShotgunShells).as_deref(),
            Some("Shotgun shells")
        );
        assert_eq!(
            r.arsenal.reserve[AmmoKind::Shells.index()],
            r.defs.ammo(AmmoKind::Shells).pickup.min(max)
        );
    }

    #[test]
    fn shotgun_pickup_gives_weapon_then_only_ammo() {
        let mut r = Rig::new();
        assert!(!r.arsenal.owned[WeaponId::Shotgun.index()]);
        assert_eq!(
            r.pick(ItemKind::Shotgun).as_deref(),
            Some("Picked up a shotgun!")
        );
        assert!(r.arsenal.owned[WeaponId::Shotgun.index()]);
        assert_eq!(r.pick(ItemKind::Shotgun).as_deref(), Some("Shotgun shells"));
        // Full on everything: the item stays.
        let max = r.defs.ammo(AmmoKind::Shells).max;
        r.arsenal.reserve[AmmoKind::Shells.index()] = max;
        assert_eq!(r.pick(ItemKind::Shotgun), None);
    }

    #[test]
    fn health_small_never_overcharges() {
        let mut r = Rig::new();
        r.vitals.health.hp = 150;
        assert_eq!(r.pick(ItemKind::HealthSmall), None);
        assert_eq!(r.vitals.health.hp, 150);
    }

    #[test]
    fn chaingun_gives_weapon_and_bullets() {
        let mut r = Rig::new();
        let before = r.arsenal.reserve[AmmoKind::Bullets.index()];
        assert_eq!(
            r.pick(ItemKind::Chaingun).as_deref(),
            Some("Picked up the chaingun")
        );
        assert!(r.arsenal.owned[WeaponId::Chaingun.index()]);
        assert_eq!(r.arsenal.reserve[AmmoKind::Bullets.index()], before + 50);
    }

    #[test]
    fn rocket_launcher_gives_weapon_and_five_rockets() {
        let mut r = Rig::new();
        assert!(r.pick(ItemKind::RocketLauncher).is_some());
        assert!(r.arsenal.owned[WeaponId::Rockets.index()]);
        assert_eq!(r.arsenal.reserve[AmmoKind::Rockets.index()], 5);
    }

    #[test]
    fn rockets_pickup_adds_five() {
        let mut r = Rig::new();
        assert_eq!(r.pick(ItemKind::Rockets).as_deref(), Some("Rockets"));
        assert_eq!(r.arsenal.reserve[AmmoKind::Rockets.index()], 5);
        assert!(!r.arsenal.owned[WeaponId::Rockets.index()]);
        let max = r.defs.ammo(AmmoKind::Rockets).max;
        r.arsenal.reserve[AmmoKind::Rockets.index()] = max;
        assert_eq!(r.pick(ItemKind::Rockets), None);
    }

    #[test]
    fn pipe_bombs_give_weapon_and_three_bombs() {
        let mut r = Rig::new();
        assert!(r.pick(ItemKind::PipeBombs).is_some());
        assert!(r.arsenal.owned[WeaponId::PipeBombs.index()]);
        assert_eq!(r.arsenal.reserve[AmmoKind::Bombs.index()], 3);
    }

    #[test]
    fn armour_pickup_adds_fifty_capped() {
        let mut r = Rig::new();
        assert_eq!(r.pick(ItemKind::Armour).as_deref(), Some("Armour"));
        assert_eq!(r.vitals.armour, 50);
        r.vitals.armour = 80;
        assert!(r.pick(ItemKind::Armour).is_some());
        assert_eq!(r.vitals.armour, 100);
    }

    #[test]
    fn full_armour_pickup_stays() {
        let mut r = Rig::new();
        r.vitals.armour = 100;
        assert_eq!(r.pick(ItemKind::Armour), None);
        assert_eq!(r.vitals.armour, 100);
    }

    #[test]
    fn medkit_pickup_adds_fifty_charge_capped() {
        let mut r = Rig::new();
        assert_eq!(r.pick(ItemKind::Medkit).as_deref(), Some("Medkit"));
        assert_eq!(r.inv.medkit, 50);
        r.pick(ItemKind::Medkit);
        assert_eq!(r.inv.medkit, 100);
        assert_eq!(r.pick(ItemKind::Medkit), None);
    }

    #[test]
    fn atom_heals_fifty_up_to_two_hundred() {
        let mut r = Rig::new();
        assert_eq!(r.pick(ItemKind::Atom).as_deref(), Some("Atomic health!"));
        assert_eq!(r.vitals.health.hp, 150);
        r.pick(ItemKind::Atom);
        assert_eq!(r.vitals.health.hp, 200);
        assert_eq!(r.pick(ItemKind::Atom), None);
    }

    #[test]
    fn jetpack_pickup_adds_fuel() {
        let mut r = Rig::new();
        assert_eq!(r.pick(ItemKind::Jetpack).as_deref(), Some("Jetpack"));
        assert_eq!(r.inv.fuel, 100.0);
        assert_eq!(r.pick(ItemKind::Jetpack), None);
    }

    #[test]
    fn night_vision_pickup_adds_battery() {
        let mut r = Rig::new();
        assert_eq!(
            r.pick(ItemKind::NightVision).as_deref(),
            Some("Night vision goggles")
        );
        assert_eq!(r.inv.battery, 100.0);
        assert_eq!(r.pick(ItemKind::NightVision), None);
    }
}
