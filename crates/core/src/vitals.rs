//! Player vitals: health plus armour, with overcharge above 100.

use crate::health::{DamageOutcome, Health, PLAYER_MAX_HEALTH};

pub const ARMOUR_MAX: i32 = 100;
/// Fraction of each hit armour soaks while it lasts.
pub const ARMOUR_ABSORB: f32 = 0.5;
pub const OVERCHARGE_MAX: i32 = 200;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Vitals {
    pub health: Health,
    pub armour: i32,
}

impl Vitals {
    pub fn new() -> Vitals {
        Vitals {
            health: Health::new(PLAYER_MAX_HEALTH),
            armour: 0,
        }
    }

    pub fn damage(&mut self, n: i32) -> DamageOutcome {
        if !self.health.alive() || n <= 0 {
            return DamageOutcome::Ignored;
        }
        let absorbed = self.armour.min((n as f32 * ARMOUR_ABSORB).floor() as i32);
        self.armour -= absorbed;
        self.health.damage(n - absorbed)
    }

    /// Heals up to `cap`; health above 100 needs a `cap` above 100.
    pub fn heal_to(&mut self, n: i32, cap: i32) -> bool {
        self.health.heal_to(n, cap)
    }

    /// Returns false (and changes nothing) when armour is already full.
    pub fn add_armour(&mut self, n: i32) -> bool {
        if self.armour >= ARMOUR_MAX {
            return false;
        }
        self.armour = (self.armour + n.max(0)).min(ARMOUR_MAX);
        true
    }
}

impl Default for Vitals {
    fn default() -> Vitals {
        Vitals::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn armoured(armour: i32) -> Vitals {
        let mut v = Vitals::new();
        v.armour = armour;
        v
    }

    #[test]
    fn armour_absorbs_fraction() {
        let mut v = armoured(50);
        assert_eq!(v.damage(20), DamageOutcome::Hurt);
        assert_eq!(v.armour, 40);
        assert_eq!(v.health.hp, 90);
        // odd hit: floor(15 * 0.5) = 7 absorbed
        v.damage(15);
        assert_eq!(v.armour, 33);
        assert_eq!(v.health.hp, 82);
    }

    #[test]
    fn armour_runs_out_mid_hit() {
        let mut v = armoured(5);
        v.damage(40);
        assert_eq!(v.armour, 0);
        assert_eq!(v.health.hp, 65);
        v.damage(10);
        assert_eq!(v.health.hp, 55);
    }

    #[test]
    fn medkit_caps_at_100() {
        let mut v = Vitals::new();
        assert!(!v.heal_to(10, PLAYER_MAX_HEALTH));
        v.health.hp = 90;
        assert!(v.heal_to(50, PLAYER_MAX_HEALTH));
        assert_eq!(v.health.hp, 100);
        v.health.hp = 150;
        assert!(!v.heal_to(10, PLAYER_MAX_HEALTH));
        assert_eq!(v.health.hp, 150);
    }

    #[test]
    fn atom_overcharges_to_200() {
        let mut v = Vitals::new();
        assert!(v.heal_to(50, OVERCHARGE_MAX));
        assert_eq!(v.health.hp, 150);
        assert!(v.heal_to(80, OVERCHARGE_MAX));
        assert_eq!(v.health.hp, 200);
        assert!(!v.heal_to(1, OVERCHARGE_MAX));
    }

    #[test]
    fn killed_once_through_armour() {
        let mut v = armoured(100);
        v.health.hp = 10;
        assert_eq!(v.damage(200), DamageOutcome::Killed);
        assert_eq!(v.health.hp, 0);
        assert_eq!(v.damage(200), DamageOutcome::Ignored);
        assert_eq!(v.armour, 0);
        assert_eq!(v.damage(0), DamageOutcome::Ignored);
    }

    #[test]
    fn add_armour_caps_and_refuses_when_full() {
        let mut v = Vitals::new();
        assert!(v.add_armour(60));
        assert!(v.add_armour(60));
        assert_eq!(v.armour, ARMOUR_MAX);
        assert!(!v.add_armour(1));
    }
}
