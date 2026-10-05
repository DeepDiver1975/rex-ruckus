//! Hit points shared by the player and actors.

pub const PLAYER_MAX_HEALTH: i32 = 100;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Health {
    pub hp: i32,
    pub max: i32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DamageOutcome {
    Hurt,
    /// Only returned on the alive-to-dead transition.
    Killed,
    /// Already dead, or no damage dealt.
    Ignored,
}

impl Health {
    pub fn new(max: i32) -> Health {
        Health { hp: max, max }
    }

    pub fn damage(&mut self, n: i32) -> DamageOutcome {
        if !self.alive() || n <= 0 {
            return DamageOutcome::Ignored;
        }
        self.hp = (self.hp - n).max(0);
        if self.hp == 0 {
            DamageOutcome::Killed
        } else {
            DamageOutcome::Hurt
        }
    }

    /// Returns false (and changes nothing) when already full or dead.
    pub fn heal(&mut self, n: i32) -> bool {
        if !self.alive() || self.hp >= self.max {
            return false;
        }
        self.hp = (self.hp + n.max(0)).min(self.max);
        true
    }

    pub fn alive(&self) -> bool {
        self.hp > 0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn killed_once_even_with_two_hits_same_tick() {
        let mut h = Health::new(20);
        assert_eq!(h.damage(5), DamageOutcome::Hurt);
        assert_eq!(h.damage(30), DamageOutcome::Killed);
        assert_eq!(h.damage(30), DamageOutcome::Ignored);
        assert_eq!(h.hp, 0);
        assert!(!h.alive());
    }

    #[test]
    fn heal_caps_and_refuses_when_full() {
        let mut h = Health::new(100);
        assert!(!h.heal(10));
        h.damage(30);
        assert!(h.heal(10));
        assert_eq!(h.hp, 80);
        assert!(h.heal(50));
        assert_eq!(h.hp, 100);
        assert!(!h.heal(1));
    }
}
