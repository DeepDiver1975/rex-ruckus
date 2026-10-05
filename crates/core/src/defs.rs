//! Data-driven weapon and enemy definitions.

use crate::map::ActorKind;
use crate::movement::Tuning;
use serde::Deserialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Deserialize)]
pub enum WeaponId {
    Boot,
    Pistol,
    Shotgun,
    Chaingun,
    Rockets,
    PipeBombs,
}

impl WeaponId {
    /// Every weapon, in slot order. `index()` is the position in this list.
    pub const ALL: [WeaponId; 6] = [
        WeaponId::Boot,
        WeaponId::Pistol,
        WeaponId::Shotgun,
        WeaponId::Chaingun,
        WeaponId::Rockets,
        WeaponId::PipeBombs,
    ];

    /// Number-key slot, 1..=ALL.len().
    pub fn slot(self) -> u8 {
        self.index() as u8 + 1
    }

    pub fn index(self) -> usize {
        WeaponId::ALL
            .iter()
            .position(|&w| w == self)
            .expect("WeaponId::ALL lists every weapon")
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Deserialize)]
pub enum AmmoKind {
    Bullets,
    Shells,
    Rockets,
    Bombs,
}

impl AmmoKind {
    pub const ALL: [AmmoKind; 4] = [
        AmmoKind::Bullets,
        AmmoKind::Shells,
        AmmoKind::Rockets,
        AmmoKind::Bombs,
    ];

    pub fn index(self) -> usize {
        AmmoKind::ALL
            .iter()
            .position(|&k| k == self)
            .expect("AmmoKind::ALL lists every ammo kind")
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Deserialize)]
pub enum Attack {
    Melee {
        range: f32,
        damage: i32,
    },
    Hitscan {
        damage: i32,
        pellets: u32,
        spread_deg: f32,
        range: f32,
    },
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct WeaponDef {
    pub id: WeaponId,
    pub name: String,
    pub attack: Attack,
    pub refire: f32,
    pub ammo: Option<AmmoKind>,
    /// Magazine size; `None` means the weapon draws straight from reserve.
    pub clip: Option<u32>,
    pub reload: f32,
    pub switch_time: f32,
    pub noise: f32,
    /// Ammo granted when the weapon itself is picked up.
    pub pickup_ammo: u32,
    /// Auto-switch rank for `best_armed`: higher wins, ties go to the lower slot. Priority 0
    /// is never auto-selected (splash weapons, and the boot, which is the fallback).
    pub priority: u8,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
pub struct AmmoDef {
    pub kind: AmmoKind,
    pub max: u32,
    pub start: u32,
    pub pickup: u32,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct WeaponsFile {
    pub ammo: Vec<AmmoDef>,
    pub weapons: Vec<WeaponDef>,
    pub start_weapons: Vec<WeaponId>,
    pub kick: Attack,
    pub kick_refire: f32,
}

#[derive(Debug, Clone, Copy, PartialEq, Deserialize)]
pub struct ProjectileDef {
    pub speed: f32,
    pub damage: i32,
    pub radius: f32,
    pub life: f32,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct EnemyDef {
    pub kind: ActorKind,
    pub health: i32,
    pub radius: f32,
    pub height: f32,
    pub speed: f32,
    pub sight_range: f32,
    pub fov_deg: f32,
    pub reaction: f32,
    pub attack_range: f32,
    pub attack_refire: f32,
    pub burst: u32,
    pub burst_gap: f32,
    pub aim_error_deg: f32,
    pub projectile: ProjectileDef,
    pub pain_chance: f32,
    pub pain_time: f32,
    pub death_time: f32,
    pub strafe: bool,
}

impl EnemyDef {
    /// Movement tuning for this enemy: walks at `speed`, never jumps or crouches.
    pub fn tuning(&self) -> Tuning {
        Tuning {
            max_speed: self.speed,
            stand_height: self.height,
            crouch_height: self.height,
            jump_speed: 0.0,
            ..Tuning::default()
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Defs {
    pub weapons: WeaponsFile,
    pub enemies: Vec<EnemyDef>,
}

#[derive(Debug, thiserror::Error, PartialEq)]
pub enum DefsError {
    #[error("defs parse error: {0}")]
    Parse(String),
    #[error("missing definition: {0}")]
    Missing(String),
    #[error("invalid definition {what}: {reason}")]
    Invalid { what: String, reason: &'static str },
    #[error("duplicate weapon definition: {0:?}")]
    DuplicateWeapon(WeaponId),
    #[error("duplicate ammo definition: {0:?}")]
    DuplicateAmmo(AmmoKind),
    #[error("duplicate enemy definition: {0:?}")]
    DuplicateEnemy(ActorKind),
    #[error("{what} grants {pickup} but the ammo max is {max}")]
    PickupOverMax { what: String, pickup: u32, max: u32 },
}

fn invalid(what: impl Into<String>, reason: &'static str) -> DefsError {
    DefsError::Invalid {
        what: what.into(),
        reason,
    }
}

fn pos(v: f32) -> bool {
    v.is_finite() && v > 0.0
}

fn nonneg(v: f32) -> bool {
    v.is_finite() && v >= 0.0
}

fn check_attack(what: &str, a: &Attack) -> Result<(), DefsError> {
    match *a {
        Attack::Melee { range, damage } => {
            if !pos(range) {
                return Err(invalid(what, "melee range must be positive"));
            }
            if damage <= 0 {
                return Err(invalid(what, "damage must be positive"));
            }
        }
        Attack::Hitscan {
            damage,
            pellets,
            spread_deg,
            range,
        } => {
            if damage <= 0 {
                return Err(invalid(what, "damage must be positive"));
            }
            if pellets == 0 {
                return Err(invalid(what, "pellets must be at least 1"));
            }
            if !nonneg(spread_deg) {
                return Err(invalid(what, "spread must not be negative"));
            }
            if !pos(range) {
                return Err(invalid(what, "range must be positive"));
            }
        }
    }
    Ok(())
}

impl Defs {
    pub fn from_ron(weapons: &str, enemies: &str) -> Result<Defs, DefsError> {
        let weapons: WeaponsFile =
            ron::from_str(weapons).map_err(|e| DefsError::Parse(format!("weapons: {e}")))?;
        let enemies: Vec<EnemyDef> =
            ron::from_str(enemies).map_err(|e| DefsError::Parse(format!("enemies: {e}")))?;
        let defs = Defs { weapons, enemies };
        defs.validate()?;
        Ok(defs)
    }

    /// The shipped defs. Panics if they are invalid; the tests guarantee they are not.
    pub fn builtin() -> Defs {
        Defs::from_ron(
            include_str!("../../../assets/defs/weapons.ron"),
            include_str!("../../../assets/defs/enemies.ron"),
        )
        .expect("shipped defs in assets/defs/*.ron are invalid")
    }

    fn validate(&self) -> Result<(), DefsError> {
        let w = &self.weapons;
        for (i, a) in w.ammo.iter().enumerate() {
            if w.ammo[..i].iter().any(|o| o.kind == a.kind) {
                return Err(DefsError::DuplicateAmmo(a.kind));
            }
        }
        for (i, d) in w.weapons.iter().enumerate() {
            if w.weapons[..i].iter().any(|o| o.id == d.id) {
                return Err(DefsError::DuplicateWeapon(d.id));
            }
        }
        for (i, e) in self.enemies.iter().enumerate() {
            if self.enemies[..i].iter().any(|o| o.kind == e.kind) {
                return Err(DefsError::DuplicateEnemy(e.kind));
            }
        }
        for a in &w.ammo {
            let what = format!("ammo {:?}", a.kind);
            if a.max == 0 {
                return Err(invalid(what, "max must be positive"));
            }
            if a.start > a.max {
                return Err(invalid(what, "start exceeds max"));
            }
            if a.pickup == 0 {
                return Err(invalid(what, "pickup must be positive"));
            }
            if a.pickup > a.max {
                return Err(DefsError::PickupOverMax {
                    what,
                    pickup: a.pickup,
                    max: a.max,
                });
            }
        }
        for id in WeaponId::ALL {
            if !w.weapons.iter().any(|d| d.id == id) {
                return Err(DefsError::Missing(format!("weapon {id:?}")));
            }
        }
        for d in &w.weapons {
            let what = format!("weapon {:?}", d.id);
            check_attack(&what, &d.attack)?;
            if !pos(d.refire) {
                return Err(invalid(what, "refire must be positive"));
            }
            if !nonneg(d.reload) || !nonneg(d.switch_time) || !nonneg(d.noise) {
                return Err(invalid(
                    what,
                    "reload, switch time and noise must not be negative",
                ));
            }
            if let Some(kind) = d.ammo {
                let ammo = w
                    .ammo
                    .iter()
                    .find(|a| a.kind == kind)
                    .ok_or_else(|| DefsError::Missing(format!("ammo {kind:?} for {what}")))?;
                if d.pickup_ammo > ammo.max {
                    return Err(DefsError::PickupOverMax {
                        what,
                        pickup: d.pickup_ammo,
                        max: ammo.max,
                    });
                }
                if let Some(clip) = d.clip {
                    if clip == 0 {
                        return Err(invalid(what, "clip must be positive"));
                    }
                    if clip > ammo.max {
                        return Err(invalid(what, "clip exceeds ammo max"));
                    }
                }
            } else if d.clip.is_some() {
                return Err(invalid(what, "clip without ammo"));
            }
        }
        for k in AmmoKind::ALL {
            if !w.ammo.iter().any(|a| a.kind == k) {
                return Err(DefsError::Missing(format!("ammo {k:?}")));
            }
        }
        check_attack("kick", &w.kick)?;
        if !pos(w.kick_refire) {
            return Err(invalid("kick", "refire must be positive"));
        }
        if w.start_weapons.is_empty() {
            return Err(invalid("start_weapons", "must not be empty"));
        }
        for e in &self.enemies {
            let what = format!("enemy {:?}", e.kind);
            if e.health <= 0 {
                return Err(invalid(what, "health must be positive"));
            }
            if !pos(e.radius) || !pos(e.height) || !pos(e.speed) {
                return Err(invalid(what, "radius, height and speed must be positive"));
            }
            if !pos(e.sight_range) || !pos(e.attack_range) || !pos(e.attack_refire) {
                return Err(invalid(what, "ranges and refire must be positive"));
            }
            if !(e.fov_deg.is_finite() && e.fov_deg > 0.0 && e.fov_deg <= 360.0) {
                return Err(invalid(what, "fov must be in (0, 360]"));
            }
            if !nonneg(e.reaction)
                || !nonneg(e.burst_gap)
                || !nonneg(e.aim_error_deg)
                || !nonneg(e.pain_time)
                || !nonneg(e.death_time)
            {
                return Err(invalid(what, "times and aim error must not be negative"));
            }
            if e.burst == 0 {
                return Err(invalid(what, "burst must be at least 1"));
            }
            if !(0.0..=1.0).contains(&e.pain_chance) {
                return Err(invalid(what, "pain chance must be in [0, 1]"));
            }
            let p = &e.projectile;
            if !pos(p.speed) || !pos(p.radius) || !pos(p.life) || p.damage <= 0 {
                return Err(invalid(what, "projectile values must be positive"));
            }
        }
        if !self.enemies.iter().any(|e| e.kind == ActorKind::Grunt) {
            return Err(DefsError::Missing("enemy Grunt".into()));
        }
        Ok(())
    }

    pub fn weapon(&self, w: WeaponId) -> &WeaponDef {
        self.weapons
            .weapons
            .iter()
            .find(|d| d.id == w)
            .expect("validated: every weapon has a def")
    }

    pub fn ammo(&self, k: AmmoKind) -> &AmmoDef {
        self.weapons
            .ammo
            .iter()
            .find(|a| a.kind == k)
            .expect("validated: every ammo kind has a def")
    }

    pub fn enemy(&self, k: ActorKind) -> &EnemyDef {
        self.enemies
            .iter()
            .find(|e| e.kind == k)
            .expect("validated: every actor kind has a def")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::map::ActorKind;

    const W: &str = include_str!("../../../assets/defs/weapons.ron");
    const E: &str = include_str!("../../../assets/defs/enemies.ron");

    #[test]
    fn builtin_defs_load_and_are_complete() {
        let d = Defs::builtin();
        for w in WeaponId::ALL {
            assert_eq!(d.weapon(w).id, w);
            assert_eq!(WeaponId::ALL[w.index()], w);
            assert_eq!(w.slot(), w.index() as u8 + 1);
        }
        for k in AmmoKind::ALL {
            assert_eq!(d.ammo(k).kind, k);
            assert_eq!(AmmoKind::ALL[k.index()], k);
        }
        assert_eq!(d.enemy(ActorKind::Grunt).health, 30);
        assert_eq!(d.weapon(WeaponId::Shotgun).clip, None);
        assert_eq!(d.weapon(WeaponId::Pistol).clip, Some(12));
        let t = d.enemy(ActorKind::Grunt).tuning();
        assert_eq!(t.max_speed, 3.5);
        assert_eq!(t.jump_speed, 0.0);
    }

    fn rejects(w: &str, e: &str) {
        assert!(
            matches!(
                Defs::from_ron(w, e),
                Err(DefsError::Invalid { .. } | DefsError::Missing(_))
            ),
            "expected rejection"
        );
    }

    #[test]
    fn defs_reject_bad_values() {
        assert!(Defs::from_ron(W, E).is_ok());
        rejects(&W.replacen("refire: 0.18", "refire: 0.0", 1), E);
        rejects(&W.replacen("pellets: 7", "pellets: 0", 1), E);
        rejects(&W.replacen("clip: Some(12)", "clip: Some(0)", 1), E);
        rejects(&W.replacen("clip: Some(12)", "clip: Some(999)", 1), E);
        rejects(W, &E.replacen("pain_chance: 0.5", "pain_chance: -0.1", 1));
        // Missing weapon: drop the shotgun entry.
        let cut = W[..W.find("id: Shotgun").expect("shotgun entry")]
            .rfind('(')
            .expect("entry start");
        let end = W[cut..].find("\n        ),").expect("end") + cut;
        let mut no_shotgun = W.to_string();
        no_shotgun.replace_range(cut..end + "\n        ),".len(), "");
        rejects(&no_shotgun, E);
        assert!(matches!(
            Defs::from_ron("garbage", E),
            Err(DefsError::Parse(_))
        ));
    }

    #[test]
    fn duplicate_kind_rejected() {
        let ammo = "(kind: Bullets, max: 200, start: 48, pickup: 12),";
        let dup_ammo = W.replacen(ammo, &format!("{ammo}\n        {ammo}"), 1);
        assert_eq!(
            Defs::from_ron(&dup_ammo, E),
            Err(DefsError::DuplicateAmmo(AmmoKind::Bullets))
        );
        let boot = "id: Boot, name: \"Boot\",";
        let at = W.find(boot).expect("boot entry") - "(\n            ".len();
        let end = at + W[at..].find("\n        ),").expect("end") + "\n        ),".len();
        let entry = &W[at..end];
        let mut dup_weapon = W.to_string();
        dup_weapon.insert_str(end, &format!("\n        {entry}"));
        assert_eq!(
            Defs::from_ron(&dup_weapon, E),
            Err(DefsError::DuplicateWeapon(WeaponId::Boot))
        );
        let mut d = Defs::builtin();
        d.enemies.push(d.enemies[0].clone());
        assert_eq!(
            d.validate(),
            Err(DefsError::DuplicateEnemy(d.enemies[0].kind))
        );
    }

    #[test]
    fn pickup_ammo_over_max_rejected() {
        // Shotgun grants 10 shells; the shell max is 50.
        let low = W.replacen("kind: Shells, max: 50", "kind: Shells, max: 9", 1);
        assert!(matches!(
            Defs::from_ron(&low, E),
            Err(DefsError::PickupOverMax {
                pickup: 10,
                max: 9,
                ..
            })
        ));
        let big = W.replacen("pickup_ammo: 10", "pickup_ammo: 51", 1);
        assert!(matches!(
            Defs::from_ron(&big, E),
            Err(DefsError::PickupOverMax {
                pickup: 51,
                max: 50,
                ..
            })
        ));
    }

    #[test]
    fn grunt_fits_humanoid_envelope() {
        let d = Defs::builtin();
        let g = d.enemy(ActorKind::Grunt);
        assert!(g.radius <= 0.4);
        assert!(g.height <= crate::movement::Tuning::default().stand_height);
    }
}
