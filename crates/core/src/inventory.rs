//! Carried utility items: medkit charge, jetpack fuel, night-vision battery.

use crate::vitals::Vitals;

pub const MEDKIT_MAX: i32 = 100;
pub const FUEL_MAX: f32 = 100.0;
pub const BATTERY_MAX: f32 = 100.0;
/// Fuel units burned per second while the jetpack is on.
pub const JETPACK_DRAIN: f32 = 8.0;
/// Battery units burned per second while night vision is on.
pub const NV_DRAIN: f32 = 4.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InvItem {
    Medkit,
    Jetpack,
    NightVision,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InvEvent {
    JetpackOff,
    NightVisionOff,
}

#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Inventory {
    pub medkit: i32,
    pub fuel: f32,
    pub battery: f32,
    pub jetpack_on: bool,
    pub nv_on: bool,
}

impl Inventory {
    pub fn new() -> Inventory {
        Inventory::default()
    }

    /// Heals `min(charge, 100 - hp)` and spends exactly that much charge.
    /// Returns false when nothing was healed.
    pub fn use_medkit(&mut self, v: &mut Vitals) -> bool {
        let amount = self.medkit.min(MEDKIT_MAX - v.health.hp);
        if amount <= 0 || !v.heal_to(amount, MEDKIT_MAX) {
            return false;
        }
        self.medkit -= amount;
        true
    }

    /// Switches an item on or off; returns true if the state changed.
    /// Turning on an empty item does nothing. The medkit is not a toggle.
    pub fn toggle(&mut self, it: InvItem) -> bool {
        match it {
            InvItem::Medkit => false,
            InvItem::Jetpack => Self::flip(&mut self.jetpack_on, self.fuel),
            InvItem::NightVision => Self::flip(&mut self.nv_on, self.battery),
        }
    }

    fn flip(on: &mut bool, charge: f32) -> bool {
        if !*on && charge <= 0.0 {
            return false;
        }
        *on = !*on;
        true
    }

    pub fn tick(&mut self, dt: f32) -> Vec<InvEvent> {
        let mut events = Vec::new();
        if self.jetpack_on {
            self.fuel = (self.fuel - JETPACK_DRAIN * dt).max(0.0);
            if self.fuel <= 0.0 {
                self.jetpack_on = false;
                events.push(InvEvent::JetpackOff);
            }
        }
        if self.nv_on {
            self.battery = (self.battery - NV_DRAIN * dt).max(0.0);
            if self.battery <= 0.0 {
                self.nv_on = false;
                events.push(InvEvent::NightVisionOff);
            }
        }
        events
    }

    /// Each returns false when already full.
    pub fn add_medkit(&mut self, n: i32) -> bool {
        if self.medkit >= MEDKIT_MAX {
            return false;
        }
        self.medkit = (self.medkit + n.max(0)).min(MEDKIT_MAX);
        true
    }

    pub fn add_fuel(&mut self, n: f32) -> bool {
        if self.fuel >= FUEL_MAX {
            return false;
        }
        self.fuel = (self.fuel + n.max(0.0)).min(FUEL_MAX);
        true
    }

    pub fn add_battery(&mut self, n: f32) -> bool {
        if self.battery >= BATTERY_MAX {
            return false;
        }
        self.battery = (self.battery + n.max(0.0)).min(BATTERY_MAX);
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn jetpack_drains_only_when_on() {
        let mut inv = Inventory::new();
        inv.add_fuel(100.0);
        assert!(inv.tick(1.0).is_empty());
        assert_eq!(inv.fuel, 100.0);
        assert!(inv.toggle(InvItem::Jetpack));
        assert!(inv.tick(2.0).is_empty());
        assert!((inv.fuel - 84.0).abs() < 1e-4);
        inv.add_battery(100.0);
        inv.toggle(InvItem::NightVision);
        inv.tick(1.0);
        assert!((inv.battery - 96.0).abs() < 1e-4);
        assert!(inv.toggle(InvItem::Jetpack));
        assert!(!inv.jetpack_on);
    }

    #[test]
    fn empty_jetpack_turns_off() {
        let mut inv = Inventory::new();
        inv.add_fuel(10.0);
        inv.toggle(InvItem::Jetpack);
        assert!(inv.tick(1.0).is_empty());
        assert_eq!(inv.tick(1.0), vec![InvEvent::JetpackOff]);
        assert_eq!(inv.fuel, 0.0);
        assert!(!inv.jetpack_on);
        assert!(inv.tick(1.0).is_empty());

        inv.add_battery(2.0);
        inv.toggle(InvItem::NightVision);
        assert_eq!(inv.tick(1.0), vec![InvEvent::NightVisionOff]);
        assert_eq!(inv.battery, 0.0);
        assert!(!inv.nv_on);
    }

    #[test]
    fn medkit_partial_use_keeps_rest() {
        let mut inv = Inventory::new();
        assert!(inv.add_medkit(100));
        assert!(!inv.add_medkit(1));
        let mut v = Vitals::new();
        assert!(!inv.use_medkit(&mut v)); // full health
        assert_eq!(inv.medkit, 100);
        v.health.hp = 70;
        assert!(inv.use_medkit(&mut v));
        assert_eq!(v.health.hp, 100);
        assert_eq!(inv.medkit, 70);
        v.health.hp = 10;
        inv.medkit = 20;
        assert!(inv.use_medkit(&mut v));
        assert_eq!((v.health.hp, inv.medkit), (30, 0));
        assert!(!inv.use_medkit(&mut v));
        // overcharged: never heals, never spends
        v.health.hp = 150;
        inv.medkit = 50;
        assert!(!inv.use_medkit(&mut v));
        assert_eq!((v.health.hp, inv.medkit), (150, 50));
    }

    #[test]
    fn toggle_empty_does_nothing() {
        let mut inv = Inventory::new();
        assert!(!inv.toggle(InvItem::Jetpack));
        assert!(!inv.toggle(InvItem::NightVision));
        assert!(!inv.toggle(InvItem::Medkit));
        assert!(!inv.jetpack_on && !inv.nv_on);
    }

    #[test]
    fn add_caps_at_100() {
        let mut inv = Inventory::new();
        assert!(inv.add_fuel(60.0));
        assert!(inv.add_fuel(60.0));
        assert_eq!(inv.fuel, FUEL_MAX);
        assert!(!inv.add_fuel(1.0));
        assert!(inv.add_battery(150.0));
        assert_eq!(inv.battery, BATTERY_MAX);
        assert!(!inv.add_battery(1.0));
    }
}
