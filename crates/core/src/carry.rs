//! What the player takes from one level into the next: health (overcharge drops back to 100),
//! armour, weapons and ammo, carried items. Keycards stay behind (they live elsewhere).

use crate::health::PLAYER_MAX_HEALTH;
use crate::inventory::Inventory;
use crate::vitals::Vitals;
use crate::weapons::{Arsenal, WeaponPhase};

/// The player's loadout at the end of a level.
#[derive(Debug, Clone, PartialEq)]
pub struct CarryOver {
    pub vitals: Vitals,
    pub arsenal: Arsenal,
    pub inventory: Inventory,
}

impl CarryOver {
    /// Snapshots the loadout, dropping overcharge, in-flight weapon state and active items.
    pub fn take(vitals: &Vitals, arsenal: &Arsenal, inventory: &Inventory) -> CarryOver {
        let mut vitals = *vitals;
        vitals.health.hp = vitals.health.hp.min(PLAYER_MAX_HEALTH);
        let mut arsenal = arsenal.clone();
        arsenal.phase = WeaponPhase::Ready;
        arsenal.reload_queued = false;
        arsenal.live_bombs = 0;
        arsenal.kick_cooldown = 0.0;
        let mut inventory = *inventory;
        inventory.jetpack_on = false;
        inventory.nv_on = false;
        CarryOver {
            vitals,
            arsenal,
            inventory,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::defs::{Defs, WeaponId};
    use crate::inventory::Inventory;
    use crate::vitals::Vitals;
    use crate::weapons::{Arsenal, WeaponPhase};

    #[test]
    fn overcharge_and_active_items_do_not_carry() {
        let defs = Defs::builtin();
        let mut v = Vitals::new();
        v.heal_to(80, 200); // 180 hp
        v.armour = 40;
        let mut arsenal = Arsenal::new(&defs);
        arsenal.give_weapon(&defs, WeaponId::Shotgun);
        arsenal.phase = WeaponPhase::Cooldown(0.3);
        arsenal.live_bombs = 2;
        let inv = Inventory {
            medkit: 30,
            fuel: 50.0,
            battery: 10.0,
            jetpack_on: true,
            nv_on: true,
        };
        let c = CarryOver::take(&v, &arsenal, &inv);
        assert_eq!((c.vitals.health.hp, c.vitals.armour), (100, 40));
        assert!(c.arsenal.owned[WeaponId::Shotgun.index()]);
        assert_eq!(
            (c.arsenal.phase, c.arsenal.live_bombs),
            (WeaponPhase::Ready, 0)
        );
        assert!(!c.inventory.jetpack_on && !c.inventory.nv_on);
        assert_eq!(c.inventory.medkit, 30);
    }

    #[test]
    fn hurt_player_keeps_hurt_health() {
        let mut v = Vitals::new();
        v.damage(70);
        let c = CarryOver::take(&v, &Arsenal::new(&Defs::builtin()), &Inventory::default());
        assert_eq!(c.vitals.health.hp, 30);
    }
}
