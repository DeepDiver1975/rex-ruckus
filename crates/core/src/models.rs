//! Visual model definitions as authored in `assets/defs/models.ron`: which glTF scene shows
//! each enemy, weapon and pickup, how it is placed, and which animation clips it plays.
//! Pure data, so the validator can check it without Bevy.

use crate::defs::WeaponId;
use crate::map::{ActorKind, ItemKind};
use serde::Deserialize;

fn finite3((x, y, z): (f32, f32, f32)) -> bool {
    x.is_finite() && y.is_finite() && z.is_finite()
}

fn one() -> f32 {
    1.0
}

/// Transform applied to a glTF scene root, in Bevy model space (y-up).
#[derive(Debug, Clone, Copy, PartialEq, Deserialize)]
#[serde(default)]
pub struct Placement {
    pub scale: f32,
    pub yaw_deg: f32,
    pub pitch_deg: f32,
    pub roll_deg: f32,
    pub offset: (f32, f32, f32),
}

impl Default for Placement {
    fn default() -> Self {
        Placement {
            scale: one(),
            yaw_deg: 0.0,
            pitch_deg: 0.0,
            roll_deg: 0.0,
            offset: (0.0, 0.0, 0.0),
        }
    }
}

impl Placement {
    fn numbers(&self) -> [f32; 7] {
        let (x, y, z) = self.offset;
        [
            self.scale,
            self.yaw_deg,
            self.pitch_deg,
            self.roll_deg,
            x,
            y,
            z,
        ]
    }

    fn check(&self, what: &str) -> Result<(), String> {
        if !self.numbers().iter().all(|n| n.is_finite()) {
            return Err(format!("{what}: placement has a non-finite number"));
        }
        if self.scale <= 0.0 {
            return Err(format!("{what}: scale must be > 0, got {}", self.scale));
        }
        Ok(())
    }
}

/// Animation clip names inside the enemy's scene; `None` means the enemy has no such clip.
#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
#[serde(default)]
pub struct Clips {
    pub idle: Option<String>,
    pub walk: Option<String>,
    pub run: Option<String>,
    pub attack: Option<String>,
    pub pain: Option<String>,
    pub death: Option<String>,
}

/// Where the shot glow sits.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub enum Tip {
    /// A named node in the scene.
    Node(String),
    /// A fixed offset from the model root, in Bevy model space.
    Offset((f32, f32, f32)),
}

/// A second scene parented to a bone of the enemy's skeleton (e.g. the grunt's pistol).
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct Attach {
    pub bone: String,
    pub scene: String,
    #[serde(default)]
    pub place: Placement,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct EnemyModel {
    pub kind: ActorKind,
    pub scene: String,
    #[serde(default)]
    pub place: Placement,
    #[serde(default)]
    pub tint: Option<(f32, f32, f32)>,
    #[serde(default)]
    pub clips: Clips,
    pub tip: Tip,
    #[serde(default)]
    pub attach: Option<Attach>,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct WeaponModel {
    pub id: WeaponId,
    pub scene: String,
    #[serde(default)]
    pub place: Placement,
    /// Muzzle flash position in Bevy model space of the viewmodel.
    #[serde(default)]
    pub muzzle: Option<(f32, f32, f32)>,
}

/// Like [`ItemKind`] but with a single `Key` variant: every keycard colour shares one scene.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Deserialize)]
pub enum ItemKindModel {
    Key,
    PistolAmmo,
    ShotgunShells,
    Shotgun,
    HealthSmall,
    Chaingun,
    RocketLauncher,
    Rockets,
    PipeBombs,
    Armour,
    Medkit,
    Atom,
    Jetpack,
    NightVision,
}

impl ItemKindModel {
    pub const ALL: [ItemKindModel; 14] = [
        ItemKindModel::Key,
        ItemKindModel::PistolAmmo,
        ItemKindModel::ShotgunShells,
        ItemKindModel::Shotgun,
        ItemKindModel::HealthSmall,
        ItemKindModel::Chaingun,
        ItemKindModel::RocketLauncher,
        ItemKindModel::Rockets,
        ItemKindModel::PipeBombs,
        ItemKindModel::Armour,
        ItemKindModel::Medkit,
        ItemKindModel::Atom,
        ItemKindModel::Jetpack,
        ItemKindModel::NightVision,
    ];

    pub fn of(kind: ItemKind) -> ItemKindModel {
        match kind {
            ItemKind::Key(_) => ItemKindModel::Key,
            ItemKind::PistolAmmo => ItemKindModel::PistolAmmo,
            ItemKind::ShotgunShells => ItemKindModel::ShotgunShells,
            ItemKind::Shotgun => ItemKindModel::Shotgun,
            ItemKind::HealthSmall => ItemKindModel::HealthSmall,
            ItemKind::Chaingun => ItemKindModel::Chaingun,
            ItemKind::RocketLauncher => ItemKindModel::RocketLauncher,
            ItemKind::Rockets => ItemKindModel::Rockets,
            ItemKind::PipeBombs => ItemKindModel::PipeBombs,
            ItemKind::Armour => ItemKindModel::Armour,
            ItemKind::Medkit => ItemKindModel::Medkit,
            ItemKind::Atom => ItemKindModel::Atom,
            ItemKind::Jetpack => ItemKindModel::Jetpack,
            ItemKind::NightVision => ItemKindModel::NightVision,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct ItemModel {
    pub kind: ItemKindModel,
    pub scene: String,
    #[serde(default)]
    pub place: Placement,
}

/// A scene plus its placement (used where a bare tuple would be awkward in RON).
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct SceneRef {
    pub scene: String,
    #[serde(default)]
    pub place: Placement,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct ExtraModels {
    /// The pipe bomb in the player's hand.
    pub held_bomb: SceneRef,
    /// The remote detonator.
    pub detonator: SceneRef,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct ModelDefs {
    pub enemies: Vec<EnemyModel>,
    pub weapons: Vec<WeaponModel>,
    pub items: Vec<ItemModel>,
    pub extras: ExtraModels,
}

impl ModelDefs {
    pub fn from_ron(s: &str) -> Result<Self, ron::error::SpannedError> {
        ron::from_str(s)
    }

    pub fn builtin() -> ModelDefs {
        ModelDefs::from_ron(include_str!("../../../assets/defs/models.ron"))
            .expect("shipped assets/defs/models.ron is invalid")
    }

    pub fn enemy(&self, kind: ActorKind) -> &EnemyModel {
        self.enemies
            .iter()
            .find(|e| e.kind == kind)
            .unwrap_or_else(|| panic!("no model for enemy {kind:?}; run validate()"))
    }

    /// `None` for weapons that stay code-built (the boot and the pipe-bomb fist).
    pub fn weapon(&self, id: WeaponId) -> Option<&WeaponModel> {
        self.weapons.iter().find(|w| w.id == id)
    }

    pub fn item(&self, kind: ItemKind) -> &ItemModel {
        let k = ItemKindModel::of(kind);
        self.items
            .iter()
            .find(|i| i.kind == k)
            .unwrap_or_else(|| panic!("no model for item {k:?}; run validate()"))
    }

    /// Checks coverage (each kind exactly once, no weapon model for the boot or the pipe bombs) and sane numbers.
    pub fn validate(&self) -> Result<(), String> {
        for k in ActorKind::ALL {
            match self.enemies.iter().filter(|e| e.kind == k).count() {
                1 => {}
                0 => return Err(format!("enemy {k:?}: no model")),
                n => return Err(format!("enemy {k:?}: {n} models, expected exactly one")),
            }
        }
        for w in WeaponId::ALL {
            let n = self.weapons.iter().filter(|m| m.id == w).count();
            match (w, n) {
                (WeaponId::Boot | WeaponId::PipeBombs, 0) => {}
                (WeaponId::Boot | WeaponId::PipeBombs, _) => {
                    return Err(format!(
                        "weapon {w:?}: must not have a model (it is code-built; the held bomb is an extra)"
                    ));
                }
                (_, 1) => {}
                (_, 0) => return Err(format!("weapon {w:?}: no model")),
                (_, n) => return Err(format!("weapon {w:?}: {n} models, expected exactly one")),
            }
        }
        for k in ItemKindModel::ALL {
            match self.items.iter().filter(|i| i.kind == k).count() {
                1 => {}
                0 => return Err(format!("item {k:?}: no model")),
                n => return Err(format!("item {k:?}: {n} models, expected exactly one")),
            }
        }
        for e in &self.enemies {
            let what = format!("enemy {:?}", e.kind);
            e.place.check(&what)?;
            if let Some(a) = &e.attach {
                a.place.check(&format!("{what} attach"))?;
            }
            let finite3 =
                |(x, y, z): (f32, f32, f32)| x.is_finite() && y.is_finite() && z.is_finite();
            if let Some(t) = e.tint
                && !finite3(t)
            {
                return Err(format!("{what}: tint has a non-finite number"));
            }
            if let Tip::Offset(o) = e.tip
                && !finite3(o)
            {
                return Err(format!("{what}: tip offset has a non-finite number"));
            }
        }
        for w in &self.weapons {
            let what = format!("weapon {:?}", w.id);
            w.place.check(&what)?;
            if let Some(m) = w.muzzle
                && !finite3(m)
            {
                return Err(format!("{what}: muzzle has a non-finite number"));
            }
        }
        for i in &self.items {
            i.place.check(&format!("item {:?}", i.kind))?;
        }
        self.extras.held_bomb.place.check("extras held_bomb")?;
        self.extras.detonator.place.check("extras detonator")?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::map::Key;

    #[test]
    fn builtin_parses_and_validates() {
        let d = ModelDefs::builtin();
        assert_eq!(d.validate(), Ok(()));
        for k in ActorKind::ALL {
            assert_eq!(d.enemy(k).kind, k);
        }
        assert!(d.weapon(WeaponId::Boot).is_none());
        assert!(d.weapon(WeaponId::PipeBombs).is_none());
        assert!(d.weapon(WeaponId::Pistol).is_some());
        assert_eq!(
            d.item(ItemKind::Key(Key::Red)).kind,
            d.item(ItemKind::Key(Key::Blue)).kind
        );
    }

    #[test]
    fn duplicate_enemy_fails() {
        let mut d = ModelDefs::builtin();
        d.enemies.push(d.enemies[0].clone());
        let err = d.validate().unwrap_err();
        assert!(err.contains("Grunt") && err.contains("2 models"), "{err}");
    }

    #[test]
    fn missing_enemy_weapon_item_fail() {
        let mut d = ModelDefs::builtin();
        d.enemies.retain(|e| e.kind != ActorKind::Drone);
        assert_eq!(d.validate(), Err("enemy Drone: no model".into()));

        let mut d = ModelDefs::builtin();
        d.weapons.retain(|w| w.id != WeaponId::Shotgun);
        assert_eq!(d.validate(), Err("weapon Shotgun: no model".into()));

        let mut d = ModelDefs::builtin();
        d.items.retain(|i| i.kind != ItemKindModel::Atom);
        assert_eq!(d.validate(), Err("item Atom: no model".into()));
    }

    #[test]
    fn boot_model_is_rejected() {
        let mut d = ModelDefs::builtin();
        let mut w = d.weapons[0].clone();
        w.id = WeaponId::Boot;
        d.weapons.push(w);
        assert!(d.validate().unwrap_err().contains("Boot"));
    }

    #[test]
    fn pipe_bombs_weapon_model_is_rejected() {
        let mut d = ModelDefs::builtin();
        let mut w = d.weapons[0].clone();
        w.id = WeaponId::PipeBombs;
        d.weapons.push(w);
        assert!(d.validate().unwrap_err().contains("PipeBombs"));
        // The pickup model is a separate list and stays required.
        let mut d = ModelDefs::builtin();
        d.items.retain(|i| i.kind != ItemKindModel::PipeBombs);
        assert_eq!(d.validate(), Err("item PipeBombs: no model".into()));
    }

    #[test]
    fn bad_numbers_are_rejected() {
        let mut d = ModelDefs::builtin();
        d.enemies[0].place.scale = 0.0;
        assert!(d.validate().unwrap_err().contains("scale"));
        let mut d = ModelDefs::builtin();
        d.items[0].place.offset.1 = f32::NAN;
        assert!(d.validate().unwrap_err().contains("non-finite"));
        let mut d = ModelDefs::builtin();
        d.extras.detonator.place.yaw_deg = f32::INFINITY;
        assert!(d.validate().unwrap_err().contains("detonator"));
    }

    #[test]
    fn placement_defaults() {
        let d: Placement = ron::from_str("()").unwrap();
        assert_eq!(d, Placement::default());
        assert_eq!(d.scale, 1.0);
    }
}
