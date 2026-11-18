//! Range checks for events that arrive over the wire. A co-op client replays the host's
//! `CombatEvent`s, `WeaponEvent`s and `MechEvent`s through its own presentation code, which
//! indexes the client's level by actor, light, wall, sector, item, prop, switch and trigger.
//! Decoding only guarantees the shape, so a hostile or mismatched host could name an index that
//! does not exist; these checks run first and the caller drops an event that does not fit.

use crate::combat::{Combat, CombatEvent};
use crate::map::{Map, SwitchAction};
use crate::mechanics::{MechEvent, Mechanics};
use crate::players::{MAX_PLAYERS, PlayerId};
use crate::projectile::Shooter;
use crate::snapshot::MAX_ACTORS;
use crate::weapons::WeaponEvent;
use glam::Vec3;

/// The most pellets one `WeaponEvent::Fire` may carry (the shotgun fires well under this).
pub const MAX_FIRE_DIRS: usize = 32;

fn finite(v: Vec3) -> bool {
    v.is_finite()
}

fn player_ok(p: PlayerId) -> bool {
    usize::from(p.0) < MAX_PLAYERS
}

/// Whether every index in `e` is in range for this level and every float is finite.
/// `ActorSpawned` may name an actor the client has not appended yet (the host's snapshot
/// carries it), so it is bounded by `MAX_ACTORS` instead of the client's actor count.
pub fn combat_event_fits(e: &CombatEvent, combat: &Combat, map: &Map) -> bool {
    let actor = |i: usize| i < combat.actors.len();
    let sector = |s: usize| s < map.sectors.len();
    let wall = |w: usize| w < map.walls.len();
    let shooter = |s: Shooter| match s {
        Shooter::Player(p) => player_ok(p),
        Shooter::Actor(i) => actor(i),
        Shooter::World => true,
    };
    match e {
        CombatEvent::ActorHurt { actor: a, .. }
        | CombatEvent::ActorWoke(a)
        | CombatEvent::ActorFired { actor: a }
        | CombatEvent::PhaseChanged { actor: a, .. }
        | CombatEvent::Burrowed { actor: a }
        | CombatEvent::Surfaced { actor: a } => actor(*a),
        CombatEvent::ActorKilled { actor: a, by } => actor(*a) && shooter(*by),
        CombatEvent::ActorSpawned(a) => *a < MAX_ACTORS,
        CombatEvent::PlayerHurt { player, from, .. } => player_ok(*player) && finite(*from),
        CombatEvent::PlayerKilled(p) | CombatEvent::HazardBurn { player: p, .. } => player_ok(*p),
        CombatEvent::CrackOpened(s) => sector(*s),
        CombatEvent::LightBroken(i) => *i < map.lights.len(),
        CombatEvent::Impact {
            point,
            normal,
            sector: s,
            wall: w,
        } => finite(*point) && finite(*normal) && sector(*s) && w.is_none_or(wall),
        CombatEvent::Explosion { point, radius } => finite(*point) && radius.is_finite(),
        CombatEvent::GlassBroken { wall: w, dirty } => wall(*w) && dirty.iter().all(|&s| sector(s)),
        CombatEvent::GroundPound {
            actor: a,
            point,
            radius,
        } => actor(*a) && finite(*point) && radius.is_finite(),
        CombatEvent::DeathAction(a) => matches!(a, SwitchAction::Channel(_) | SwitchAction::Exit),
        CombatEvent::SecretFound | CombatEvent::BombsDetonated | CombatEvent::ProjectileGone(_) => {
            true
        }
    }
}

/// Whether every float in `e` is finite. Needs no world, so the wire layer can use it.
pub fn combat_event_finite(e: &CombatEvent) -> bool {
    match e {
        CombatEvent::PlayerHurt { from, .. } => finite(*from),
        CombatEvent::Impact { point, normal, .. } => finite(*point) && finite(*normal),
        CombatEvent::Explosion { point, radius }
        | CombatEvent::GroundPound { point, radius, .. } => finite(*point) && radius.is_finite(),
        _ => true,
    }
}

/// Whether every player id `e` names is a seat below `MAX_PLAYERS`. Needs no world.
pub fn combat_event_players_ok(e: &CombatEvent) -> bool {
    match e {
        CombatEvent::PlayerHurt { player, .. }
        | CombatEvent::PlayerKilled(player)
        | CombatEvent::HazardBurn { player, .. } => player_ok(*player),
        CombatEvent::ActorKilled {
            by: Shooter::Player(p),
            ..
        } => player_ok(*p),
        _ => true,
    }
}

/// Whether every float in `e` is finite. Needs no world.
pub fn mech_event_finite(e: &MechEvent) -> bool {
    match e {
        MechEvent::QuakeStarted { strength, duration } => {
            strength.is_finite() && duration.is_finite()
        }
        _ => true,
    }
}

/// Whether every index in `e` is in range for this level and every float is finite.
pub fn mech_event_fits(e: &MechEvent, mech: &Mechanics, map: &Map) -> bool {
    match e {
        MechEvent::MoverStarted { sector, .. } | MechEvent::MoverStopped { sector, .. } => {
            *sector < map.sectors.len()
        }
        MechEvent::SwitchUsed(i) => *i < map.switches.len(),
        MechEvent::NeedKey { player, .. } => player_ok(*player),
        MechEvent::Exit => true,
        MechEvent::ItemTaken { item, .. } => *item < map.items.len(),
        MechEvent::TriggerFired(i) => *i < map.triggers.len(),
        MechEvent::QuakeStarted { strength, duration } => {
            strength.is_finite() && duration.is_finite()
        }
        MechEvent::PropUsed { prop, .. } => *prop < map.props.len() && *prop < mech.props.len(),
    }
}

/// Whether `e` is well formed: finite directions and at most `MAX_FIRE_DIRS` pellets.
pub fn weapon_event_fits(e: &WeaponEvent) -> bool {
    match e {
        WeaponEvent::Fire { dirs, .. } => {
            dirs.len() <= MAX_FIRE_DIRS && dirs.iter().all(|&d| finite(d))
        }
        WeaponEvent::Launch { dir, .. } | WeaponEvent::Kick { dir } => finite(*dir),
        WeaponEvent::Detonate
        | WeaponEvent::DryFire
        | WeaponEvent::ReloadStart
        | WeaponEvent::ReloadDone
        | WeaponEvent::Switched(_) => true,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::collide::Body;
    use crate::combat::PlayerTarget;
    use crate::defs::WeaponId;
    use crate::fixtures::{combat_room, defs};
    use crate::hazard::HazardKind;
    use crate::map::{ItemKind, Key, MoverKind, SectorId};
    use crate::props::{PropKind, PropOutcome};
    use crate::vitals::Vitals;
    use glam::Vec2;

    fn level() -> (Map, Combat, Mechanics) {
        let mut map = combat_room();
        for (x, y) in [(7.5, 1.5), (7.5, 2.6)] {
            map.actors.push(crate::map::ActorSpawn {
                kind: crate::map::ActorKind::Grunt,
                pos: Vec2::new(x, y),
                angle: 0.0,
                asleep: true,
                skill: crate::difficulty::Difficulty::Easy,
                on_death: None,
            });
        }
        let d = defs();
        let c = Combat::spawn(&map, &d, 1);
        let mech = Mechanics::new(&mut map.clone());
        (map, c, mech)
    }

    #[test]
    fn fits_accepts_real_events() {
        let (mut map, mut c, mut mech) = level();
        let d = defs();
        let mut body = Body::spawn(&map, Vec2::new(1.5, 1.5), 0.35, 1.8).unwrap();
        let mut vitals = Vitals::new();
        let eye = body.pos + Vec3::Z * 1.6;
        let aim = {
            let a = &c.actors[0];
            (a.body.pos + Vec3::Z * 0.6 * a.body.height - eye).normalize()
        };
        let launch = WeaponEvent::Launch {
            weapon: WeaponId::Rockets,
            dir: aim,
        };
        let mut events = c.player_attack(&mut map, &d, PlayerId(0), eye, body.sector, &launch);
        assert!(weapon_event_fits(&launch));
        for _ in 0..300 {
            let mut t = PlayerTarget {
                body: &mut body,
                vitals: &mut vitals,
                eye,
            };
            events.extend(c.tick(
                &mut map,
                &mut mech,
                &d,
                std::slice::from_mut(&mut t),
                1.0 / 60.0,
            ));
        }
        assert!(
            events
                .iter()
                .any(|e| matches!(e, CombatEvent::Explosion { .. })),
            "{events:?}"
        );
        for e in &events {
            assert!(combat_event_fits(e, &c, &map), "{e:?}");
        }
    }

    #[test]
    fn fits_rejects_out_of_range() {
        let (map, c, mech) = level();
        let bad_combat = [
            CombatEvent::ActorHurt {
                actor: 9999,
                amount: 1,
            },
            CombatEvent::LightBroken(9999),
            CombatEvent::GlassBroken {
                wall: 99999,
                dirty: vec![],
            },
            CombatEvent::GlassBroken {
                wall: 0,
                dirty: vec![9999],
            },
            CombatEvent::Impact {
                point: Vec3::ZERO,
                normal: Vec3::Z,
                sector: 9999,
                wall: None,
            },
            CombatEvent::PlayerHurt {
                player: PlayerId(4),
                amount: 1,
                from: Vec3::ZERO,
            },
        ];
        for e in &bad_combat {
            assert!(!combat_event_fits(e, &c, &map), "{e:?}");
        }
        let bad_mech = [
            MechEvent::ItemTaken {
                item: 9999,
                kind: ItemKind::Key(Key::Red),
            },
            MechEvent::PropUsed {
                prop: 9999,
                kind: PropKind::Toilet,
                outcome: PropOutcome::Flushed,
            },
            MechEvent::SwitchUsed(9999),
            MechEvent::TriggerFired(9999),
            MechEvent::NeedKey {
                player: PlayerId(4),
                key: Key::Red,
            },
        ];
        for e in &bad_mech {
            assert!(!mech_event_fits(e, &mech, &map), "{e:?}");
        }
        assert!(!weapon_event_fits(&WeaponEvent::Fire {
            weapon: WeaponId::Shotgun,
            dirs: vec![Vec3::X; MAX_FIRE_DIRS + 1],
        }));
    }

    #[test]
    fn fits_rejects_non_finite() {
        let (map, c, _) = level();
        let boom = CombatEvent::Explosion {
            point: Vec3::new(f32::NAN, 0.0, 0.0),
            radius: 1.0,
        };
        assert!(!combat_event_fits(&boom, &c, &map));
        let fire = WeaponEvent::Fire {
            weapon: WeaponId::Shotgun,
            dirs: vec![Vec3::new(f32::INFINITY, 0.0, 0.0)],
        };
        assert!(!weapon_event_fits(&fire));
        let ok = CombatEvent::Explosion {
            point: Vec3::ONE,
            radius: 1.0,
        };
        assert!(combat_event_fits(&ok, &c, &map));
    }

    #[test]
    fn world_free_checks() {
        let nan = Vec3::new(f32::NAN, 0.0, 0.0);
        assert!(!combat_event_finite(&CombatEvent::Explosion {
            point: nan,
            radius: 1.0
        }));
        assert!(!combat_event_finite(&CombatEvent::PlayerHurt {
            player: PlayerId(0),
            from: nan,
            amount: 1,
        }));
        assert!(combat_event_finite(&CombatEvent::SecretFound));
        assert!(!mech_event_finite(&MechEvent::QuakeStarted {
            strength: f32::INFINITY,
            duration: 1.0
        }));
        assert!(mech_event_finite(&MechEvent::Exit));
        assert!(combat_event_players_ok(&CombatEvent::PlayerKilled(
            PlayerId(3)
        )));
        assert!(!combat_event_players_ok(&CombatEvent::PlayerKilled(
            PlayerId(4)
        )));
        assert!(!combat_event_players_ok(&CombatEvent::ActorKilled {
            actor: 0,
            by: Shooter::Player(PlayerId(9)),
        }));
    }

    fn round_trip<T>(v: &T)
    where
        T: serde::Serialize + serde::de::DeserializeOwned + PartialEq + std::fmt::Debug,
    {
        let bytes = postcard::to_stdvec(v).unwrap();
        let back: T = postcard::from_bytes(&bytes).unwrap();
        assert_eq!(&back, v);
    }

    #[test]
    fn events_round_trip_postcard() {
        let p = PlayerId(1);
        let combat = vec![
            CombatEvent::ActorHurt {
                actor: 3,
                amount: 7,
            },
            CombatEvent::ActorKilled {
                actor: 1,
                by: Shooter::Player(p),
            },
            CombatEvent::ActorWoke(2),
            CombatEvent::ActorFired { actor: 2 },
            CombatEvent::PlayerHurt {
                player: p,
                amount: 5,
                from: Vec3::new(1.0, 2.0, 3.0),
            },
            CombatEvent::PlayerKilled(p),
            CombatEvent::CrackOpened(1),
            CombatEvent::LightBroken(0),
            CombatEvent::SecretFound,
            CombatEvent::HazardBurn {
                player: p,
                kind: HazardKind::Slime,
            },
            CombatEvent::Impact {
                point: Vec3::ONE,
                normal: Vec3::Z,
                sector: 0,
                wall: Some(2),
            },
            CombatEvent::ProjectileGone(9),
            CombatEvent::Explosion {
                point: Vec3::ONE,
                radius: 2.0,
            },
            CombatEvent::BombsDetonated,
            CombatEvent::GlassBroken {
                wall: 1,
                dirty: vec![0, 1],
            },
            CombatEvent::PhaseChanged { actor: 0, phase: 2 },
            CombatEvent::DeathAction(SwitchAction::Exit),
            CombatEvent::DeathAction(SwitchAction::Channel(3)),
            CombatEvent::Burrowed { actor: 0 },
            CombatEvent::Surfaced { actor: 0 },
            CombatEvent::ActorSpawned(5),
            CombatEvent::GroundPound {
                actor: 0,
                point: Vec3::ONE,
                radius: 3.0,
            },
        ];
        for e in &combat {
            round_trip(e);
        }
        let weapon = vec![
            WeaponEvent::Fire {
                weapon: WeaponId::Shotgun,
                dirs: vec![Vec3::X, Vec3::Y],
            },
            WeaponEvent::Launch {
                weapon: WeaponId::Rockets,
                dir: Vec3::Z,
            },
            WeaponEvent::Detonate,
            WeaponEvent::Kick { dir: Vec3::X },
            WeaponEvent::DryFire,
            WeaponEvent::ReloadStart,
            WeaponEvent::ReloadDone,
            WeaponEvent::Switched(WeaponId::Pistol),
        ];
        for e in &weapon {
            round_trip(e);
        }
        let sector: SectorId = 1;
        let mech = vec![
            MechEvent::MoverStarted {
                sector,
                kind: MoverKind::Door,
            },
            MechEvent::MoverStopped {
                sector,
                kind: MoverKind::Crack,
            },
            MechEvent::SwitchUsed(0),
            MechEvent::NeedKey {
                player: p,
                key: Key::Blue,
            },
            MechEvent::Exit,
            MechEvent::ItemTaken {
                item: 1,
                kind: ItemKind::Key(Key::Red),
            },
            MechEvent::TriggerFired(0),
            MechEvent::QuakeStarted {
                strength: 0.5,
                duration: 1.0,
            },
            MechEvent::PropUsed {
                prop: 0,
                kind: PropKind::Toilet,
                outcome: PropOutcome::Healed(10),
            },
        ];
        for e in &mech {
            round_trip(e);
        }
    }
}
