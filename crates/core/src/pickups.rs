//! What each world item does when the player walks over it. Plain data in, HUD message out;
//! the game decides when to call it (see `Mechanics::pickup`'s `accept` hook).

use crate::defs::{AmmoKind, Defs, WeaponId};
use crate::health::Health;
use crate::map::{ItemKind, KeySet};
use crate::weapons::Arsenal;

/// Hit points a small health pack restores.
pub const HEALTH_SMALL: i32 = 10;

/// The player state a pickup can change.
pub struct Loadout<'a> {
    pub health: &'a mut Health,
    pub arsenal: &'a mut Arsenal,
    pub keys: &'a mut KeySet,
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
            .health
            .heal(HEALTH_SMALL)
            .then(|| format!("+{HEALTH_SMALL} health")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::defs::Defs;
    use crate::map::Key;

    struct Rig {
        defs: Defs,
        health: Health,
        arsenal: Arsenal,
        keys: KeySet,
    }

    impl Rig {
        fn new() -> Rig {
            let defs = Defs::builtin();
            Rig {
                health: Health::new(100),
                arsenal: Arsenal::new(&defs),
                keys: KeySet::default(),
                defs,
            }
        }
        fn pick(&mut self, kind: ItemKind) -> Option<String> {
            let mut l = Loadout {
                health: &mut self.health,
                arsenal: &mut self.arsenal,
                keys: &mut self.keys,
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
        r.health.hp = 95;
        assert_eq!(r.pick(ItemKind::HealthSmall).as_deref(), Some("+10 health"));
        assert_eq!(r.health.hp, 100, "capped at max");
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
}
