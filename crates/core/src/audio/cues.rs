//! Pure mapping from simulation events to sound cues.

use super::{Cue, CuePos};
use crate::combat::{Combat, CombatEvent};
use crate::hazard::HazardKind;
use crate::map::{ActorKind, Map, MoverKind, SectorId, WallId};
use crate::mechanics::MechEvent;
use crate::props::{PropKind, PropOutcome};
use crate::weapons::WeaponEvent;
use glam::Vec3;

type Out = Vec<(Cue, CuePos)>;

/// Midpoint of wall `w`, at the mid height of its sector.
fn wall_mid(map: &Map, w: WallId) -> Vec3 {
    let wall = &map.walls[w];
    let sec = &map.sectors[wall.sector];
    ((wall.a + wall.b) * 0.5).extend((sec.floor_z + sec.ceil_z) * 0.5)
}

/// Cues for one combat event. `combat` supplies the actor behind an index.
///
/// `ProjectileGone` and `BombsDetonated` give nothing: the `Impact` or `Explosion` event that
/// accompanies them already carries the sound. Barrels are static bodies, so all their events
/// are silent, death included: the blast after the death fuse sounds as an `Explosion`. The boss
/// has its own voice (roar on waking and on a phase change, rocket or minigun by phase, death);
/// only its pain uses the generic arm. A non-boss `PhaseChanged` and every `DeathAction` are
/// silent. An actor index out of range gives nothing.
pub fn combat_cues(ev: &CombatEvent, combat: &Combat, map: &Map, out: &mut Out) {
    let boss = |i: usize| combat.actors.get(i).filter(|a| a.kind == ActorKind::Boss);
    match *ev {
        CombatEvent::ActorWoke(i) | CombatEvent::PhaseChanged { actor: i, .. }
            if boss(i).is_some() =>
        {
            out.push((Cue::BossRoar, boss(i).map(|a| a.body.pos)));
            return;
        }
        CombatEvent::ActorFired { actor } if boss(actor).is_some() => {
            if let Some(a) = boss(actor) {
                let cue = if a.phase > 0 {
                    Cue::BossMinigun
                } else {
                    Cue::BossRocket
                };
                out.push((cue, Some(a.body.pos)));
            }
            return;
        }
        CombatEvent::ActorKilled { actor, .. } if boss(actor).is_some() => {
            out.push((Cue::BossDeath, boss(actor).map(|a| a.body.pos)));
            return;
        }
        _ => {}
    }
    let actor = |i: usize| combat.actors.get(i).map(|a| (a.kind, a.body.pos));
    let mut living = |i: usize, cue: fn(ActorKind) -> Cue| {
        if let Some((k, p)) = actor(i)
            && k != ActorKind::Barrel
        {
            out.push((cue(k), Some(p)));
        }
    };
    match *ev {
        CombatEvent::ActorHurt { actor, .. } => living(actor, Cue::ActorPain),
        CombatEvent::ActorWoke(i) => living(i, Cue::ActorWake),
        CombatEvent::ActorFired { actor } => living(actor, Cue::ActorFire),
        CombatEvent::ActorKilled { actor, .. } => living(actor, Cue::ActorDeath),
        CombatEvent::PlayerHurt { .. } => out.push((Cue::PlayerHurt, None)),
        CombatEvent::PlayerKilled => out.push((Cue::PlayerDeath, None)),
        CombatEvent::CrackOpened(s) => {
            if s < map.sectors.len() {
                out.push((Cue::CrackOpen, Some(map.sector_centre(s))));
            }
        }
        CombatEvent::LightBroken(i) => {
            if let Some(l) = map.lights.get(i) {
                out.push((Cue::LightBreak, Some(Vec3::new(l.pos.0, l.pos.1, l.pos.2))));
            }
        }
        CombatEvent::SecretFound => out.push((Cue::Secret, None)),
        CombatEvent::HazardBurn { kind } => out.push((
            match kind {
                HazardKind::Slime => Cue::HazardSizzle,
                HazardKind::Electric => Cue::HazardZap,
            },
            None,
        )),
        CombatEvent::Impact { point, .. } => out.push((Cue::Impact, Some(point))),
        CombatEvent::Explosion { point, .. } => out.push((Cue::Explosion, Some(point))),
        CombatEvent::GlassBroken { wall, .. } => {
            if wall < map.walls.len() {
                out.push((Cue::GlassBreak, Some(wall_mid(map, wall))));
            }
        }
        CombatEvent::ProjectileGone(_) | CombatEvent::BombsDetonated => {}
        // The boss arms above took its phase changes; the rest are silent.
        CombatEvent::PhaseChanged { .. } | CombatEvent::DeathAction(_) => {}
    }
}

/// Cues for one weapon event, all at the listener. `ReloadDone` and `Detonate` give nothing
/// (the blast that follows a detonation sounds as an `Explosion`).
pub fn weapon_cues(ev: &WeaponEvent, out: &mut Out) {
    match *ev {
        WeaponEvent::Fire { weapon, .. } | WeaponEvent::Launch { weapon, .. } => {
            out.push((Cue::Fire(weapon), None))
        }
        WeaponEvent::Kick { .. } => out.push((Cue::Kick, None)),
        WeaponEvent::DryFire => out.push((Cue::DryFire, None)),
        WeaponEvent::ReloadStart => out.push((Cue::Reload, None)),
        WeaponEvent::Switched(_) => out.push((Cue::Switch, None)),
        WeaponEvent::ReloadDone | WeaponEvent::Detonate => {}
    }
}

/// Cues for one mechanics event. A crack mover gives nothing: `CrackOpened` covers it.
pub fn mech_cues(ev: &MechEvent, map: &Map, out: &mut Out) {
    let mover = |sector: SectorId, kind: MoverKind, start: bool, out: &mut Out| {
        let cue = match (kind, start) {
            (MoverKind::Door, true) => Cue::DoorStart,
            (MoverKind::Door, false) => Cue::DoorStop,
            (MoverKind::Lift { .. }, true) => Cue::LiftStart,
            (MoverKind::Lift { .. }, false) => Cue::LiftStop,
            (MoverKind::Crack, _) => return,
        };
        if sector < map.sectors.len() {
            out.push((cue, Some(map.sector_centre(sector))));
        }
    };
    match *ev {
        MechEvent::MoverStarted { sector, kind } => mover(sector, kind, true, out),
        MechEvent::MoverStopped { sector, kind } => mover(sector, kind, false, out),
        MechEvent::SwitchUsed(i) => {
            if let Some(s) = map.switches.get(i) {
                out.push((Cue::SwitchUse, Some(wall_mid(map, s.wall))));
            }
        }
        MechEvent::NeedKey(_) => out.push((Cue::Denied, None)),
        MechEvent::Exit => out.push((Cue::LevelComplete, None)),
        // The trigger itself is silent: what it starts makes the noise.
        MechEvent::TriggerFired(_) => {}
        MechEvent::QuakeStarted { .. } => out.push((Cue::QuakeRumble, None)),
        MechEvent::PropUsed {
            prop,
            kind,
            outcome,
        } => {
            let cue = match (kind, outcome) {
                (_, PropOutcome::Busy) => return,
                (PropKind::Toilet, _) => Cue::ToiletFlush,
                (PropKind::Vending, PropOutcome::SoldOut) => Cue::VendingEmpty,
                (PropKind::Vending, _) => Cue::VendingDispense,
                (PropKind::PoolTable, _) => Cue::PoolBreak,
            };
            if let Some(p) = map.props.get(prop) {
                out.push((cue, Some(p.pos.extend(map.sectors[p.sector].floor_z + 0.5))));
            }
        }
        MechEvent::ItemTaken { item, kind } => {
            if let Some(it) = map.items.get(item) {
                let z = map
                    .find_sector(it.pos, None)
                    .map_or(0.0, |s| map.sectors[s].floor_z);
                out.push((Cue::Pickup(kind.into()), Some(it.pos.extend(z + 0.5))));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::audio::PickupClass;
    use crate::defs::WeaponId;
    use crate::difficulty::Difficulty;
    use crate::fixtures::{defs, door_rooms};
    use crate::map::{ActorSpawn, ItemKind, Key};
    use crate::projectile::Shooter;
    use crate::props::{PropKind, PropOutcome};
    use glam::Vec2;
    use std::collections::HashSet;

    const EXTRA: &str = r#"
        items: [(kind: Medkit, pos: (2.0, 2.0)), (kind: Key(Red), pos: (6.0, 2.0))],
        switches: [(wall: (7, 0), action: Exit)],
        lights: [(pos: (1.0, 2.0, 2.5), color: (1.0, 1.0, 1.0), intensity: 100.0, range: 5.0, breakable: true)],
        props: [(kind: Toilet, pos: (1.0, 1.0)), (kind: Vending, pos: (2.5, 3.0), angle_deg: 270.0), (kind: PoolTable, pos: (6.5, 2.0), angle_deg: 90.0)],
    "#;

    fn setup() -> (Map, Combat) {
        let mut map = door_rooms("(kind: Door)", EXTRA);
        let spawn = |kind, x| ActorSpawn {
            kind,
            pos: Vec2::new(x, 2.0),
            angle: 0.0,
            asleep: true,
            skill: Difficulty::Easy,
            on_death: None,
        };
        map.actors.push(spawn(ActorKind::Grunt, 2.0));
        map.actors.push(spawn(ActorKind::Barrel, 6.0));
        map.actors.push(spawn(ActorKind::Boss, 7.5));
        let combat = Combat::spawn(&map, &defs(), 1);
        (map, combat)
    }

    /// Exhaustive over `CombatEvent`: a new variant fails to compile here.
    fn all_combat_events() -> Vec<CombatEvent> {
        let p = Vec3::new(1.0, 2.0, 3.0);
        let evs = vec![
            CombatEvent::ActorHurt {
                actor: 0,
                amount: 5,
            },
            CombatEvent::ActorKilled {
                actor: 0,
                by: Shooter::Player,
            },
            CombatEvent::ActorWoke(0),
            CombatEvent::ActorFired { actor: 0 },
            CombatEvent::PlayerHurt { amount: 5, from: p },
            CombatEvent::PlayerKilled,
            CombatEvent::CrackOpened(1),
            CombatEvent::LightBroken(0),
            CombatEvent::SecretFound,
            CombatEvent::HazardBurn {
                kind: crate::hazard::HazardKind::Slime,
            },
            CombatEvent::Impact {
                point: p,
                normal: Vec3::Z,
                sector: 0,
                wall: None,
            },
            CombatEvent::ProjectileGone(1),
            CombatEvent::Explosion {
                point: p,
                radius: 3.0,
            },
            CombatEvent::BombsDetonated,
            CombatEvent::GlassBroken {
                wall: 0,
                dirty: vec![0],
            },
            CombatEvent::PhaseChanged { actor: 0, phase: 1 },
            CombatEvent::DeathAction(crate::map::SwitchAction::Exit),
        ];
        for e in &evs {
            match e {
                CombatEvent::ActorHurt { .. }
                | CombatEvent::ActorKilled { .. }
                | CombatEvent::ActorWoke(_)
                | CombatEvent::ActorFired { .. }
                | CombatEvent::PlayerHurt { .. }
                | CombatEvent::PlayerKilled
                | CombatEvent::CrackOpened(_)
                | CombatEvent::LightBroken(_)
                | CombatEvent::SecretFound
                | CombatEvent::HazardBurn { .. }
                | CombatEvent::Impact { .. }
                | CombatEvent::ProjectileGone(_)
                | CombatEvent::Explosion { .. }
                | CombatEvent::BombsDetonated
                | CombatEvent::GlassBroken { .. }
                | CombatEvent::PhaseChanged { .. }
                | CombatEvent::DeathAction(_) => {}
            }
        }
        evs
    }

    fn run_combat(ev: &CombatEvent, c: &Combat, m: &Map) -> Out {
        let mut out = Vec::new();
        combat_cues(ev, c, m, &mut out);
        out
    }

    #[test]
    fn combat_table() {
        let (map, c) = setup();
        let g = ActorKind::Grunt;
        let gp = c.actors[0].body.pos;
        let p = Vec3::new(1.0, 2.0, 3.0);
        let expected: Vec<Out> = vec![
            vec![(Cue::ActorPain(g), Some(gp))],
            vec![(Cue::ActorDeath(g), Some(gp))],
            vec![(Cue::ActorWake(g), Some(gp))],
            vec![(Cue::ActorFire(g), Some(gp))],
            vec![(Cue::PlayerHurt, None)],
            vec![(Cue::PlayerDeath, None)],
            vec![(Cue::CrackOpen, Some(map.sector_centre(1)))],
            vec![(Cue::LightBreak, Some(p.with_z(2.5)))],
            vec![(Cue::Secret, None)],
            vec![(Cue::HazardSizzle, None)],
            vec![(Cue::Impact, Some(p))],
            vec![],
            vec![(Cue::Explosion, Some(p))],
            vec![],
            vec![(Cue::GlassBreak, Some(wall_mid(&map, 0)))],
            vec![],
            vec![],
        ];
        let evs = all_combat_events();
        assert_eq!(evs.len(), expected.len());
        for (e, want) in evs.iter().zip(expected) {
            assert_eq!(run_combat(e, &c, &map), want, "{e:?}");
        }
        // A barrel's death is silent: its blast sounds as `Explosion` when the fuse ends.
        assert_eq!(c.actors[1].kind, ActorKind::Barrel);
        assert_eq!(
            run_combat(
                &CombatEvent::ActorKilled {
                    actor: 1,
                    by: Shooter::Player
                },
                &c,
                &map
            ),
            vec![]
        );
    }

    #[test]
    fn boss_voice_follows_its_phase() {
        let (map, mut c) = setup();
        let cue = |c: &Combat, e: CombatEvent| {
            run_combat(&e, c, &map)
                .into_iter()
                .map(|(q, _)| q)
                .collect::<Vec<_>>()
        };
        assert_eq!(cue(&c, CombatEvent::ActorWoke(2)), vec![Cue::BossRoar]);
        assert_eq!(
            cue(&c, CombatEvent::ActorFired { actor: 2 }),
            vec![Cue::BossRocket]
        );
        c.actors[2].phase = 1;
        assert_eq!(
            cue(&c, CombatEvent::ActorFired { actor: 2 }),
            vec![Cue::BossMinigun]
        );
        assert_eq!(
            cue(&c, CombatEvent::PhaseChanged { actor: 2, phase: 1 }),
            vec![Cue::BossRoar]
        );
        assert_eq!(
            cue(
                &c,
                CombatEvent::ActorKilled {
                    actor: 2,
                    by: Shooter::Player
                }
            ),
            vec![Cue::BossDeath]
        );
        assert_eq!(
            cue(
                &c,
                CombatEvent::ActorHurt {
                    actor: 2,
                    amount: 5
                }
            ),
            vec![Cue::ActorPain(ActorKind::Boss)]
        );
    }

    #[test]
    fn hazards_quakes_and_props_have_cues() {
        use crate::hazard::HazardKind;
        let (map, c) = setup();
        let one = |e: &CombatEvent| run_combat(e, &c, &map);
        assert_eq!(
            one(&CombatEvent::HazardBurn {
                kind: HazardKind::Slime
            }),
            vec![(Cue::HazardSizzle, None)]
        );
        assert_eq!(
            one(&CombatEvent::HazardBurn {
                kind: HazardKind::Electric
            }),
            vec![(Cue::HazardZap, None)]
        );
        let mech = |e: MechEvent| {
            let mut out = Vec::new();
            mech_cues(&e, &map, &mut out);
            out.into_iter().map(|(q, _)| q).collect::<Vec<_>>()
        };
        assert_eq!(
            mech(MechEvent::QuakeStarted {
                strength: 0.8,
                duration: 2.0
            }),
            vec![Cue::QuakeRumble]
        );
        let used = |prop, kind, outcome| MechEvent::PropUsed {
            prop,
            kind,
            outcome,
        };
        assert_eq!(
            mech(used(0, PropKind::Toilet, PropOutcome::Healed(10))),
            vec![Cue::ToiletFlush]
        );
        assert_eq!(
            mech(used(0, PropKind::Toilet, PropOutcome::Flushed)),
            vec![Cue::ToiletFlush]
        );
        assert_eq!(
            mech(used(1, PropKind::Vending, PropOutcome::Dispensed(5))),
            vec![Cue::VendingDispense]
        );
        assert_eq!(
            mech(used(1, PropKind::Vending, PropOutcome::SoldOut)),
            vec![Cue::VendingEmpty]
        );
        assert_eq!(
            mech(used(2, PropKind::PoolTable, PropOutcome::Racked)),
            vec![Cue::PoolBreak]
        );
        assert_eq!(
            mech(used(9, PropKind::PoolTable, PropOutcome::Racked)),
            vec![],
            "out of range"
        );
    }

    #[test]
    fn glass_cue_sits_at_wall_midpoint() {
        let (map, c) = setup();
        let w = 3;
        let wall = &map.walls[w];
        let sec = &map.sectors[wall.sector];
        let out = run_combat(
            &CombatEvent::GlassBroken {
                wall: w,
                dirty: vec![],
            },
            &c,
            &map,
        );
        let want = ((wall.a + wall.b) * 0.5).extend((sec.floor_z + sec.ceil_z) * 0.5);
        assert_eq!(out, vec![(Cue::GlassBreak, Some(want))]);
    }

    #[test]
    fn barrel_is_silent() {
        let (map, c) = setup();
        assert_eq!(c.actors[1].kind, ActorKind::Barrel);
        for ev in [
            CombatEvent::ActorHurt {
                actor: 1,
                amount: 1,
            },
            CombatEvent::ActorWoke(1),
            CombatEvent::ActorFired { actor: 1 },
            CombatEvent::ActorKilled {
                actor: 1,
                by: Shooter::Player,
            },
        ] {
            assert!(run_combat(&ev, &c, &map).is_empty(), "{ev:?}");
        }
    }

    #[test]
    fn out_of_range_indices_give_nothing() {
        let (map, c) = setup();
        for ev in [
            CombatEvent::ActorHurt {
                actor: 99,
                amount: 1,
            },
            CombatEvent::ActorKilled {
                actor: 99,
                by: Shooter::Player,
            },
            CombatEvent::ActorWoke(99),
            CombatEvent::ActorFired { actor: 99 },
            CombatEvent::LightBroken(99),
            CombatEvent::CrackOpened(99),
            CombatEvent::GlassBroken {
                wall: 999,
                dirty: vec![],
            },
        ] {
            assert!(run_combat(&ev, &c, &map).is_empty(), "{ev:?}");
        }
        let mut out = Vec::new();
        mech_cues(&MechEvent::SwitchUsed(9), &map, &mut out);
        mech_cues(
            &MechEvent::ItemTaken {
                item: 9,
                kind: ItemKind::Medkit,
            },
            &map,
            &mut out,
        );
        assert!(out.is_empty());
    }

    fn all_weapon_events() -> Vec<WeaponEvent> {
        let evs = vec![
            WeaponEvent::Fire {
                weapon: WeaponId::Shotgun,
                dirs: vec![],
            },
            WeaponEvent::Launch {
                weapon: WeaponId::Rockets,
                dir: Vec3::X,
            },
            WeaponEvent::Detonate,
            WeaponEvent::Kick { dir: Vec3::X },
            WeaponEvent::DryFire,
            WeaponEvent::ReloadStart,
            WeaponEvent::ReloadDone,
            WeaponEvent::Switched(WeaponId::Chaingun),
        ];
        for e in &evs {
            match e {
                WeaponEvent::Fire { .. }
                | WeaponEvent::Launch { .. }
                | WeaponEvent::Detonate
                | WeaponEvent::Kick { .. }
                | WeaponEvent::DryFire
                | WeaponEvent::ReloadStart
                | WeaponEvent::ReloadDone
                | WeaponEvent::Switched(_) => {}
            }
        }
        evs
    }

    #[test]
    fn weapon_table() {
        let expected: Vec<Out> = vec![
            vec![(Cue::Fire(WeaponId::Shotgun), None)],
            vec![(Cue::Fire(WeaponId::Rockets), None)],
            vec![],
            vec![(Cue::Kick, None)],
            vec![(Cue::DryFire, None)],
            vec![(Cue::Reload, None)],
            vec![],
            vec![(Cue::Switch, None)],
        ];
        for (e, want) in all_weapon_events().iter().zip(expected) {
            let mut out = Vec::new();
            weapon_cues(e, &mut out);
            assert_eq!(out, want, "{e:?}");
        }
    }

    fn all_mech_events() -> Vec<MechEvent> {
        let door = MoverKind::Door;
        let lift = MoverKind::Lift { to: 2.0 };
        let evs = vec![
            MechEvent::MoverStarted {
                sector: 1,
                kind: door,
            },
            MechEvent::MoverStopped {
                sector: 1,
                kind: door,
            },
            MechEvent::MoverStarted {
                sector: 1,
                kind: lift,
            },
            MechEvent::MoverStopped {
                sector: 1,
                kind: lift,
            },
            MechEvent::MoverStarted {
                sector: 1,
                kind: MoverKind::Crack,
            },
            MechEvent::MoverStopped {
                sector: 1,
                kind: MoverKind::Crack,
            },
            MechEvent::SwitchUsed(0),
            MechEvent::NeedKey(Key::Red),
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
        for e in &evs {
            match e {
                MechEvent::MoverStarted { .. }
                | MechEvent::MoverStopped { .. }
                | MechEvent::SwitchUsed(_)
                | MechEvent::NeedKey(_)
                | MechEvent::Exit
                | MechEvent::ItemTaken { .. }
                | MechEvent::TriggerFired(_)
                | MechEvent::QuakeStarted { .. }
                | MechEvent::PropUsed { .. } => {}
            }
        }
        evs
    }

    #[test]
    fn mech_table() {
        let (map, _) = setup();
        let centre = Some(map.sector_centre(1));
        let sw = &map.switches[0];
        let sec = &map.sectors[map.walls[sw.wall].sector];
        let sw_pos = Some(
            ((map.walls[sw.wall].a + map.walls[sw.wall].b) * 0.5)
                .extend((sec.floor_z + sec.ceil_z) * 0.5),
        );
        let key_pos = Some(Vec3::new(6.0, 2.0, map.sectors[2].floor_z + 0.5));
        let toilet = &map.props[0];
        let toilet_pos = Some(toilet.pos.extend(map.sectors[toilet.sector].floor_z + 0.5));
        let expected: Vec<Out> = vec![
            vec![(Cue::DoorStart, centre)],
            vec![(Cue::DoorStop, centre)],
            vec![(Cue::LiftStart, centre)],
            vec![(Cue::LiftStop, centre)],
            vec![],
            vec![],
            vec![(Cue::SwitchUse, sw_pos)],
            vec![(Cue::Denied, None)],
            vec![(Cue::LevelComplete, None)],
            vec![(Cue::Pickup(PickupClass::Key), key_pos)],
            vec![],
            vec![(Cue::QuakeRumble, None)],
            vec![(Cue::ToiletFlush, toilet_pos)],
        ];
        for (e, want) in all_mech_events().iter().zip(expected) {
            let mut out = Vec::new();
            mech_cues(e, &map, &mut out);
            assert_eq!(out, want, "{e:?}");
        }
    }

    #[test]
    fn pickup_is_at_the_item() {
        let (map, _) = setup();
        let mut out = Vec::new();
        mech_cues(
            &MechEvent::ItemTaken {
                item: 0,
                kind: ItemKind::Medkit,
            },
            &map,
            &mut out,
        );
        let (cue, pos) = out[0];
        assert_eq!(cue, Cue::Pickup(PickupClass::Health));
        let p = pos.unwrap();
        assert_eq!((p.x, p.y), (2.0, 2.0));
    }

    #[test]
    fn every_emitted_cue_is_in_all() {
        let (map, c) = setup();
        let all: HashSet<Cue> = Cue::all().into_iter().collect();
        let mut out = Vec::new();
        for e in all_combat_events() {
            combat_cues(&e, &c, &map, &mut out);
        }
        // Every actor kind, through each actor event.
        for k in ActorKind::ALL {
            let mut g = c.clone();
            g.actors[0].kind = k;
            for e in [
                CombatEvent::ActorHurt {
                    actor: 0,
                    amount: 1,
                },
                CombatEvent::ActorKilled {
                    actor: 0,
                    by: Shooter::Player,
                },
                CombatEvent::ActorWoke(0),
                CombatEvent::ActorFired { actor: 0 },
            ] {
                combat_cues(&e, &g, &map, &mut out);
            }
            g.actors[0].phase = 1;
            combat_cues(&CombatEvent::ActorFired { actor: 0 }, &g, &map, &mut out);
        }
        for e in [
            CombatEvent::PhaseChanged { actor: 2, phase: 1 },
            CombatEvent::HazardBurn {
                kind: crate::hazard::HazardKind::Slime,
            },
            CombatEvent::HazardBurn {
                kind: crate::hazard::HazardKind::Electric,
            },
        ] {
            combat_cues(&e, &c, &map, &mut out);
        }
        mech_cues(
            &MechEvent::QuakeStarted {
                strength: 0.5,
                duration: 1.0,
            },
            &map,
            &mut out,
        );
        for (prop, kind, outcome) in [
            (0, PropKind::Toilet, PropOutcome::Healed(10)),
            (0, PropKind::Toilet, PropOutcome::Flushed),
            (1, PropKind::Vending, PropOutcome::Dispensed(5)),
            (1, PropKind::Vending, PropOutcome::SoldOut),
            (2, PropKind::PoolTable, PropOutcome::Racked),
        ] {
            mech_cues(
                &MechEvent::PropUsed {
                    prop,
                    kind,
                    outcome,
                },
                &map,
                &mut out,
            );
        }
        for e in all_weapon_events() {
            weapon_cues(&e, &mut out);
        }
        for w in WeaponId::ALL {
            weapon_cues(
                &WeaponEvent::Fire {
                    weapon: w,
                    dirs: vec![],
                },
                &mut out,
            );
            weapon_cues(
                &WeaponEvent::Launch {
                    weapon: w,
                    dir: Vec3::X,
                },
                &mut out,
            );
        }
        for e in all_mech_events() {
            mech_cues(&e, &map, &mut out);
        }
        for kind in [
            ItemKind::Shotgun,
            ItemKind::Rockets,
            ItemKind::Atom,
            ItemKind::Armour,
            ItemKind::Key(Key::Red),
            ItemKind::Jetpack,
        ] {
            mech_cues(&MechEvent::ItemTaken { item: 0, kind }, &map, &mut out);
        }
        assert!(!out.is_empty());
        for (cue, _) in &out {
            assert!(all.contains(cue), "{cue:?} missing from Cue::all()");
        }
        // Reverse: everything in all() is emitted, or raised by the game itself.
        let emitted: HashSet<Cue> = out.iter().map(|(c, _)| *c).collect();
        let game_side = [
            Cue::JetpackStart,
            Cue::JetpackLoop,
            Cue::JetpackStop,
            Cue::NightVisionOn,
            Cue::NightVisionOff,
            Cue::Footstep,
            Cue::Land,
        ];
        for cue in &all {
            assert!(
                emitted.contains(cue) || game_side.contains(cue),
                "{cue:?} in Cue::all() is never emitted"
            );
        }
    }
}
