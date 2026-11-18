//! World snapshots for network co-op. The host captures its simulation into a [`Snapshot`];
//! a client applies it to its own copy of the same level, which it never ticks: no AI runs and
//! no random number is drawn there. Plain data with serde derives; the wire format lives in
//! `rr-net`.
//!
//! Players are carried as [`PlayerSnap`]s for the game to apply to its own components;
//! [`Snapshot::apply`] leaves them alone.

use crate::actors::{Actor, AiState};
use crate::collide::Body;
use crate::combat::Combat;
use crate::defs::Defs;
use crate::destruct::Destruct;
use crate::difficulty::Difficulty;
use crate::inventory::Inventory;
use crate::map::{ActorKind, ActorSpawn, KeySet, Map, SectorId, WallId};
use crate::mechanics::{Mechanics, Motion, PropState, moved_sector, place_mover};
use crate::players::{MAX_PLAYERS, PlayerId};
use crate::projectile::{Projectile, Shooter};
use crate::vitals::Vitals;
use crate::weapons::Arsenal;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Most actors a snapshot may carry (authored ones plus spawned minions); a longer one is
/// refused, so a hostile host cannot make a client grow without bound.
pub const MAX_ACTORS: usize = 1024;

/// How far a snapped mover pose may lie outside its travel (float slack).
const MOVER_SLACK: f32 = 1e-3;

/// The whole shared world on one host tick.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Snapshot {
    pub tick: u32,
    pub actors: Vec<ActorSnap>,
    pub projectiles: Vec<Projectile>,
    pub world: WorldSnap,
    pub players: Vec<PlayerSnap>,
}

/// What a client shows of one actor (index = `Combat::actors` index). Only these fields travel:
/// the AI-only ones (target, summoner, refire/repath/pain/burrow/pound timers, route) stay at
/// their defaults on the client, which never runs AI.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ActorSnap {
    pub kind: ActorKind,
    pub body: Body,
    pub angle: f32,
    pub state: AiState,
    pub hp: i32,
    pub phase: u8,
}

/// One mover's pose (index = `Mechanics::movers` index).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MoverSnap {
    pub z: f32,
    pub motion: Motion,
}

/// The level's runtime state outside the actors.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WorldSnap {
    pub movers: Vec<MoverSnap>,
    pub switch_on: Vec<bool>,
    pub taken: Vec<bool>,
    pub props: Vec<PropState>,
    pub destruct: Destruct,
    /// Walls that were glass in the source map and are broken now (both twins).
    pub glass_broken: Vec<WallId>,
    /// Per authored quake (map order): seconds since it started while it runs, as `Mechanics`
    /// holds it.
    pub quakes: Vec<Option<f32>>,
}

/// One player as the game holds it (built from its components in PR 5).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PlayerSnap {
    pub id: PlayerId,
    pub body: Body,
    pub angle: f32,
    pub pitch: f32,
    pub vitals: Vitals,
    pub arsenal: Arsenal,
    pub inventory: Inventory,
    pub keys: KeySet,
    /// Last input sequence the host applied for this player.
    pub ack: u32,
}

/// Why a snapshot does not fit the client's level; the client is left unchanged.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ApplyError {
    Shape(&'static str),
}

/// What an apply changed that the game must rebuild or react to. Every list is sorted and
/// free of duplicates; a second apply of the same snapshot reports nothing.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Changes {
    /// Sectors whose geometry changed because a mover moved: the sectors `Mechanics::tick`
    /// reports for it (rebuild them with their neighbours, as for a tick).
    pub sectors: Vec<SectorId>,
    /// Walls whose glass was broken by this apply (both twins).
    pub glass: Vec<WallId>,
    /// Lights (index in `Map::lights`) broken by this apply.
    pub lights: Vec<usize>,
    /// Crack walls (sectors) set off by this apply.
    pub cracks: Vec<SectorId>,
    /// Indices of the actors this apply appended to `Combat::actors`.
    pub actors_added: std::ops::Range<usize>,
}

impl Changes {
    /// Whether the apply changed nothing listed here.
    pub fn is_empty(&self) -> bool {
        self.sectors.is_empty()
            && self.glass.is_empty()
            && self.lights.is_empty()
            && self.cracks.is_empty()
            && self.actors_added.is_empty()
    }
}

fn sorted<T: Ord>(mut v: Vec<T>) -> Vec<T> {
    v.sort_unstable();
    v.dedup();
    v
}

impl Snapshot {
    /// Whether the snapshot's sizes and ids are within bounds: at most `MAX_PLAYERS` players
    /// with unique ids below `MAX_PLAYERS`, and at most `MAX_ACTORS` actors. Checked on every
    /// snapshot decoded from the network.
    pub fn is_sane(&self) -> bool {
        let mut seen = [false; MAX_PLAYERS];
        let players = self.players.len() <= MAX_PLAYERS
            && self.players.iter().all(|p| {
                let i = usize::from(p.id.0);
                i < MAX_PLAYERS && !std::mem::replace(&mut seen[i], true)
            });
        players && self.actors.len() <= MAX_ACTORS
    }

    /// Whether every float in the snapshot is finite. A snapshot decoded from the network is
    /// untrusted: one NaN would poison positions and collision on the client.
    pub fn is_finite(&self) -> bool {
        let v3 = |v: glam::Vec3| v.is_finite();
        let body =
            |b: &Body| v3(b.pos) && v3(b.vel) && b.radius.is_finite() && b.height.is_finite();
        let actors = self
            .actors
            .iter()
            .all(|a| body(&a.body) && a.angle.is_finite() && a.state.is_finite());
        let shots = self.projectiles.iter().all(|p| {
            v3(p.pos)
                && v3(p.prev)
                && v3(p.vel)
                && p.radius.is_finite()
                && p.life.is_finite()
                && p.gravity.is_finite()
                && p.bounce.is_none_or(f32::is_finite)
                && p.splash
                    .is_none_or(|s| s.radius.is_finite() && s.self_scale.is_finite())
        });
        let movers = self.world.movers.iter().all(|m| {
            m.z.is_finite()
                && match m.motion {
                    Motion::AtEnd { wait } => wait.is_finite(),
                    _ => true,
                }
        });
        let props = self.world.props.iter().all(|p| p.cooldown.is_finite());
        let quakes = self
            .world
            .quakes
            .iter()
            .all(|q| q.is_none_or(f32::is_finite));
        let players = self.players.iter().all(|p| {
            use crate::weapons::WeaponPhase;
            body(&p.body)
                && p.angle.is_finite()
                && p.pitch.is_finite()
                && p.inventory.fuel.is_finite()
                && p.inventory.battery.is_finite()
                && p.arsenal.kick_cooldown.is_finite()
                && match p.arsenal.phase {
                    WeaponPhase::Ready => true,
                    WeaponPhase::Cooldown(t) | WeaponPhase::Reloading(t) => t.is_finite(),
                    WeaponPhase::Switching { left, .. } => left.is_finite(),
                }
        });
        actors && shots && movers && props && quakes && players
    }

    /// `source` is the authored map the live `map` started from (for broken glass).
    pub fn capture(
        tick: u32,
        combat: &Combat,
        mech: &Mechanics,
        map: &Map,
        source: &Map,
        players: Vec<PlayerSnap>,
    ) -> Snapshot {
        debug_assert_eq!(map.walls.len(), source.walls.len(), "source is map's level");
        let actors = combat
            .actors
            .iter()
            .map(|a| ActorSnap {
                kind: a.kind,
                body: a.body,
                angle: a.angle,
                state: a.state,
                hp: a.health.hp,
                phase: a.phase,
            })
            .collect();
        let movers = mech
            .movers
            .iter()
            .map(|m| MoverSnap {
                z: m.z,
                motion: m.motion,
            })
            .collect();
        let glass_broken = map
            .walls
            .iter()
            .zip(&source.walls)
            .enumerate()
            .filter(|(_, (now, was))| was.glass && !now.glass)
            .map(|(w, _)| w)
            .collect();
        Snapshot {
            tick,
            actors,
            projectiles: combat.projectiles.clone(),
            world: WorldSnap {
                movers,
                switch_on: mech.switch_on.clone(),
                taken: mech.taken.clone(),
                props: mech.props.clone(),
                destruct: combat.destruct.clone(),
                glass_broken,
                quakes: mech.quake_times(),
            },
            players,
        }
    }

    /// Writes actors (pos/body, prev_pos = the old pos, angle, state, hp, phase; appends missing
    /// actors from `defs` and the snap), replaces projectiles, and applies `world` to mech + map.
    /// Never runs AI, never touches rng. Validates every length/index first; on Err nothing
    /// changed. Players are the game's job. Returns what changed for the game to rebuild.
    ///
    /// Callers must drop a snapshot whose `tick` is not newer than the last one applied (UDP
    /// reorders): `prev_pos` is set to the pose being replaced, so an older snapshot would make
    /// interpolation run backwards.
    pub fn apply(
        &self,
        combat: &mut Combat,
        mech: &mut Mechanics,
        map: &mut Map,
        defs: &Defs,
    ) -> Result<Changes, ApplyError> {
        let fresh = self.check(combat, mech, map, defs)?;
        let w = &self.world;
        // Work out what changes before writing anything.
        let sectors = mech
            .movers
            .iter()
            .zip(&w.movers)
            .filter(|(mv, s)| mv.z != s.z)
            .map(|(mv, _)| moved_sector(mv))
            .collect();
        let glass = w
            .glass_broken
            .iter()
            .flat_map(|&g| std::iter::once(g).chain(map.walls[g].next_wall))
            .filter(|&g| map.walls[g].glass)
            .collect();
        let lights = (0..map.lights.len())
            .filter(|&i| !combat.destruct.light_broken(i) && w.destruct.light_broken(i))
            .collect();
        let cracks = (0..map.sectors.len())
            .filter(|&s| !combat.destruct.crack_open(s) && w.destruct.crack_open(s))
            .collect();
        let added = combat.actors.len()..self.actors.len();
        // Everything below only writes: the snapshot fits.
        for (a, s) in combat.actors.iter_mut().zip(&self.actors) {
            a.prev_pos = a.body.pos;
            write_actor(a, s);
        }
        combat.actors.extend(fresh);
        combat.projectiles.clone_from(&self.projectiles);
        for (mv, s) in mech.movers.iter_mut().zip(&w.movers) {
            mv.z = s.z;
            mv.motion = s.motion;
            place_mover(map, mv, s.z);
        }
        mech.switch_on.clone_from(&w.switch_on);
        mech.taken.clone_from(&w.taken);
        mech.props.clone_from(&w.props);
        mech.set_quake_times(&w.quakes);
        combat.destruct.clone_from(&w.destruct);
        for &g in &w.glass_broken {
            map.walls[g].glass = false;
            if let Some(twin) = map.walls[g].next_wall {
                map.walls[twin].glass = false;
            }
        }
        Ok(Changes {
            sectors: sorted(sectors),
            glass: sorted(glass),
            lights: sorted(lights),
            cracks: sorted(cracks),
            actors_added: added,
        })
    }

    /// Blends this snapshot with `next` at `f` (clamped to [0, 1]; 0 is this snapshot, 1 is
    /// `next`), for the client to draw between two host ticks. Each actor of `combat` that both
    /// snapshots carry (matched by index) gets `prev_pos` = its current position and `body.pos`
    /// = the blend. Projectiles are matched by `id` the same way. Returns each player both carry,
    /// by id, with its body's `pos` blended (its other fields are this snapshot's). Writes
    /// nothing else, never touches rng, and tolerates mismatched lengths.
    pub fn blend(&self, next: &Snapshot, f: f32, combat: &mut Combat) -> Vec<(PlayerId, Body)> {
        let f = f.clamp(0.0, 1.0);
        for ((a, from), to) in combat.actors.iter_mut().zip(&self.actors).zip(&next.actors) {
            a.prev_pos = a.body.pos;
            a.body.pos = from.body.pos.lerp(to.body.pos, f);
        }
        let shots: HashMap<u32, &Projectile> = self.projectiles.iter().map(|p| (p.id, p)).collect();
        let next_shots: HashMap<u32, &Projectile> =
            next.projectiles.iter().map(|p| (p.id, p)).collect();
        for p in &mut combat.projectiles {
            if let (Some(from), Some(to)) = (shots.get(&p.id), next_shots.get(&p.id)) {
                p.prev = p.pos;
                p.pos = from.pos.lerp(to.pos, f);
            }
        }
        self.players
            .iter()
            .filter_map(|p| {
                let to = next.players.iter().find(|q| q.id == p.id)?;
                let mut body = p.body;
                body.pos = p.body.pos.lerp(to.body.pos, f);
                Some((p.id, body))
            })
            .collect()
    }

    /// Checks that the snapshot fits the client's level and builds the actors it appends,
    /// touching nothing.
    fn check(
        &self,
        combat: &Combat,
        mech: &Mechanics,
        map: &Map,
        defs: &Defs,
    ) -> Result<Vec<Actor>, ApplyError> {
        let shape = |ok: bool, what| {
            if ok {
                Ok(())
            } else {
                Err(ApplyError::Shape(what))
            }
        };
        let sectors = map.sectors.len();
        shape(self.actors.len() <= MAX_ACTORS, "too many actors")?;
        shape(
            self.actors.len() >= combat.actors.len(),
            "fewer actors than the client has",
        )?;
        shape(
            combat
                .actors
                .iter()
                .zip(&self.actors)
                .all(|(a, s)| a.kind == s.kind),
            "actor kind differs",
        )?;
        shape(
            self.actors.iter().all(|s| s.body.sector < sectors),
            "actor sector out of range",
        )?;
        shape(
            self.projectiles.iter().all(|p| {
                p.sector < sectors
                    && match p.owner {
                        Shooter::Actor(i) => i < self.actors.len(),
                        Shooter::Player(_) | Shooter::World => true,
                    }
            }),
            "projectile sector or owner out of range",
        )?;
        // Players are the game's to apply, but their sectors and ids index the client's level
        // and per-player slices. Everything else a player carries is fixed-size arrays and
        // enums, bounded by decoding.
        shape(
            self.players
                .iter()
                .all(|p| p.body.sector < sectors && usize::from(p.id.0) < MAX_PLAYERS),
            "player sector or id out of range",
        )?;
        let w = &self.world;
        shape(w.movers.len() == mech.movers.len(), "mover count differs")?;
        shape(
            w.movers.iter().zip(&mech.movers).all(|(s, mv)| {
                let (lo, hi) = (mv.start.min(mv.end), mv.start.max(mv.end));
                s.z >= lo - MOVER_SLACK && s.z <= hi + MOVER_SLACK
            }),
            "mover pose outside its travel",
        )?;
        shape(
            w.switch_on.len() == mech.switch_on.len(),
            "switch count differs",
        )?;
        shape(w.taken.len() == mech.taken.len(), "item count differs")?;
        shape(
            w.props.len() == mech.props.len()
                && w.props
                    .iter()
                    .zip(&mech.props)
                    .all(|(s, p)| s.kind == p.kind),
            "props differ",
        )?;
        shape(w.quakes.len() == mech.quake_count(), "quake count differs")?;
        shape(
            combat.destruct.same_shape(&w.destruct),
            "destruction state differs",
        )?;
        shape(
            w.glass_broken.iter().all(|&g| g < map.walls.len()),
            "glass wall out of range",
        )?;
        // Appended actors take the snapped body as it is (its sector was checked above): no
        // fresh sector lookup that could fail where the host's actor stands.
        Ok(self.actors[combat.actors.len()..]
            .iter()
            .map(|s| {
                let spawn = ActorSpawn {
                    kind: s.kind,
                    pos: s.body.pos.truncate(),
                    angle: s.angle,
                    asleep: false,
                    // Placeholder: `Actor::with_body` ignores the spawn's skill (skill only
                    // filters which authored spawns exist).
                    skill: Difficulty::Easy,
                    on_death: None,
                };
                let mut a = Actor::with_body(defs.enemy(s.kind), &spawn, s.body);
                write_actor(&mut a, s);
                a.prev_pos = s.body.pos;
                a
            })
            .collect())
    }
}

/// Overwrites what a snapshot carries of an actor.
fn write_actor(a: &mut Actor, s: &ActorSnap) {
    a.body = s.body;
    a.angle = s.angle;
    a.state = s.state;
    a.health.hp = s.hp;
    a.phase = s.phase;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::combat::{PlayerTarget, level_seed};
    use crate::defs::Defs;
    use crate::map::{MoverKind, SectorId};
    use crate::players::MAX_PLAYERS;
    use crate::projectile::Targets;
    use crate::rng::Rng;
    use glam::{Vec2, Vec3};

    const DT: f32 = 1.0 / 60.0;
    const ARENA: &str = include_str!("../../../assets/levels/combat_arena.ron");
    const ENGINE_LAB: &str = include_str!("../../../assets/levels/engine_lab.ron");
    const DEPOT: &str = include_str!("../../../assets/levels/arsenal_depot.ron");

    /// One side's copy of a level: the authored map, the live map and its simulation.
    struct World {
        source: Map,
        map: Map,
        mech: Mechanics,
        combat: Combat,
    }

    impl World {
        /// The level spawned fresh, as host and client both start it.
        fn load(src: &str, defs: &Defs) -> World {
            let source = Map::from_ron(src).expect("level parses");
            let mut map = source.clone();
            let mech = Mechanics::new(&mut map);
            let combat = Combat::spawn(&map, defs, level_seed(&map.name));
            World {
                source,
                map,
                mech,
                combat,
            }
        }

        fn capture(&self, tick: u32) -> Snapshot {
            Snapshot::capture(
                tick,
                &self.combat,
                &self.mech,
                &self.map,
                &self.source,
                Vec::new(),
            )
        }

        fn apply(&mut self, snap: &Snapshot, defs: &Defs) -> Result<Changes, ApplyError> {
            snap.apply(&mut self.combat, &mut self.mech, &mut self.map, defs)
        }

        /// Runs `ticks` host ticks with one player standing at `at`, as the game does.
        fn run(&mut self, defs: &Defs, at: Vec2, ticks: usize) {
            let mut body = Body::spawn(&self.map, at, 0.35, 1.8).expect("player spot");
            let mut vitals = Vitals::new();
            for _ in 0..ticks {
                let eye = body.pos + Vec3::Z * 0.9 * body.height;
                let mut p = PlayerTarget {
                    body: &mut body,
                    vitals: &mut vitals,
                    eye,
                };
                self.combat.tick(
                    &mut self.map,
                    &mut self.mech,
                    defs,
                    std::slice::from_mut(&mut p),
                    DT,
                );
                let (idx, mut bodies) = self.combat.living_bodies();
                self.mech.tick(&mut self.map, &mut bodies, DT);
                self.combat.write_back(&idx, &bodies);
            }
        }

        /// Everything apply may write, for "nothing changed" checks (`Combat` is not
        /// `PartialEq`).
        fn state(&self) -> (Vec<Actor>, Vec<Projectile>, Rng, Destruct, Mechanics, Map) {
            (
                self.combat.actors.clone(),
                self.combat.projectiles.clone(),
                self.combat.rng.clone(),
                self.combat.destruct.clone(),
                self.mech.clone(),
                self.map.clone(),
            )
        }
    }

    fn bolt(id: u32, at: Vec3) -> Projectile {
        Projectile {
            id,
            pos: at,
            prev: at,
            vel: Vec3::X * 20.0,
            sector: 0,
            radius: 0.1,
            damage: 10,
            owner: Shooter::Actor(0),
            targets: Targets::Players,
            life: 5.0,
            gravity: 0.0,
            bounce: None,
            remote: false,
            splash: None,
            resting: false,
        }
    }

    /// The arena after 120 ticks with the player inside the arena door, in view of the Grunts.
    fn fought_arena(defs: &Defs) -> World {
        let mut host = World::load(ARENA, defs);
        host.run(defs, Vec2::new(5.0, 10.0), 120);
        host
    }

    #[test]
    fn apply_matches_host_actors() {
        let defs = Defs::builtin();
        let host = fought_arena(&defs);
        let fresh = World::load(ARENA, &defs);
        assert!(
            host.combat
                .actors
                .iter()
                .zip(&fresh.combat.actors)
                .any(|(h, f)| h.body.pos != f.body.pos && h.state != f.state),
            "the host's actors woke and moved"
        );
        let mut client = World::load(ARENA, &defs);
        let rng = client.combat.rng.clone();
        let olds: Vec<Vec3> = client.combat.actors.iter().map(|a| a.body.pos).collect();
        client.apply(&host.capture(120), &defs).unwrap();
        assert_eq!(client.combat.actors.len(), host.combat.actors.len());
        for ((c, h), old) in client
            .combat
            .actors
            .iter()
            .zip(&host.combat.actors)
            .zip(olds)
        {
            assert_eq!(c.body, h.body);
            assert_eq!(c.angle, h.angle);
            assert_eq!(c.state, h.state);
            assert_eq!(c.health.hp, h.health.hp);
            assert_eq!(c.phase, h.phase);
            assert_eq!(c.prev_pos, old, "prev_pos is where the client had it");
        }
        assert_eq!(client.combat.rng, rng, "apply draws no random number");
    }

    #[test]
    fn apply_replaces_projectiles() {
        let defs = Defs::builtin();
        let mut host = fought_arena(&defs);
        host.combat
            .projectiles
            .push(bolt(77, Vec3::new(5.0, 9.0, 1.0)));
        host.combat
            .projectiles
            .push(bolt(78, Vec3::new(5.0, 9.5, 1.2)));
        let mut client = World::load(ARENA, &defs);
        client.apply(&host.capture(1), &defs).unwrap();
        assert_eq!(client.combat.projectiles, host.combat.projectiles);
        assert!(client.combat.projectiles.iter().any(|p| p.id == 77));

        host.combat.projectiles.clear();
        client.apply(&host.capture(2), &defs).unwrap();
        assert!(client.combat.projectiles.is_empty());
    }

    #[test]
    fn apply_grows_actors_for_spawned_minions() {
        let defs = Defs::builtin();
        let mut host = fought_arena(&defs);
        let spawn = ActorSpawn {
            kind: ActorKind::Slasher,
            pos: Vec2::new(5.0, 11.0),
            angle: 1.0,
            asleep: false,
            skill: crate::difficulty::Difficulty::Easy,
            on_death: None,
        };
        let mut minion =
            Actor::new(&host.map, defs.enemy(ActorKind::Slasher), &spawn).expect("on the floor");
        minion.state = AiState::Chase;
        minion.body.pos.x += 0.25;
        minion.summoner = Some(0);
        host.combat.actors.push(minion.clone());
        let mut client = World::load(ARENA, &defs);
        let before = client.combat.actors.len();
        let changes = client.apply(&host.capture(1), &defs).unwrap();
        assert_eq!(changes.actors_added, before..before + 1);
        assert_eq!(client.combat.actors.len(), before + 1);
        let got = client.combat.actors.last().unwrap();
        assert_eq!(got.kind, ActorKind::Slasher);
        assert_eq!(got.body, minion.body);
        assert_eq!(got.state, AiState::Chase);
        assert_eq!(got.angle, minion.angle);
        assert_eq!(got.health, minion.health);
    }

    #[test]
    fn appended_actor_needs_no_sector_lookup() {
        let defs = Defs::builtin();
        let mut host = fought_arena(&defs);
        let spawn = ActorSpawn {
            kind: ActorKind::Slasher,
            pos: Vec2::new(5.0, 11.0),
            angle: 1.0,
            asleep: false,
            skill: crate::difficulty::Difficulty::Easy,
            on_death: None,
        };
        let mut minion =
            Actor::new(&host.map, defs.enemy(ActorKind::Slasher), &spawn).expect("on the floor");
        // Its snapped position lies outside every sector (a fresh `Body::spawn` there fails), but
        // its sector is valid: the client takes the snapped body as it is.
        minion.body.pos = Vec3::new(1.0e4, -1.0e4, 0.0);
        assert!(Body::spawn(&host.map, minion.body.pos.truncate(), 0.35, 1.8).is_none());
        host.combat.actors.push(minion.clone());
        let mut client = World::load(ARENA, &defs);
        client.apply(&host.capture(1), &defs).unwrap();
        let got = client.combat.actors.last().unwrap();
        assert_eq!(got.body, minion.body);
        assert_eq!(got.prev_pos, minion.body.pos);
    }

    #[test]
    fn apply_reports_changes() {
        let defs = Defs::builtin();
        let host = stirred_lab(&defs);
        let mut client = World::load(ENGINE_LAB, &defs);
        let snap = host.capture(20);
        let changes = client.apply(&snap, &defs).unwrap();
        let mut moved: Vec<SectorId> = host
            .mech
            .movers
            .iter()
            .filter(|m| m.z != m.start)
            .map(|m| m.sector)
            .collect();
        moved.sort_unstable();
        assert!(!moved.is_empty());
        assert_eq!(changes.sectors, moved);
        let of = |k: fn(&MoverKind) -> bool| {
            host.mech
                .movers
                .iter()
                .find(|m| k(&m.def.kind))
                .map(|m| m.sector)
                .unwrap()
        };
        assert!(
            changes
                .sectors
                .contains(&of(|k| matches!(k, MoverKind::Door)))
        );
        assert!(
            changes
                .sectors
                .contains(&of(|k| matches!(k, MoverKind::Slide { .. })))
        );
        let cracks: Vec<SectorId> = host
            .mech
            .movers
            .iter()
            .filter(|m| m.def.kind == MoverKind::Crack)
            .map(|m| m.sector)
            .collect();
        assert!(!cracks.is_empty());
        assert_eq!(changes.cracks, cracks);
        assert!(changes.glass.is_empty() && changes.lights.is_empty());
        assert!(changes.actors_added.is_empty());
        assert!(
            client.apply(&snap, &defs).unwrap().is_empty(),
            "nothing new"
        );

        let mut host = World::load(DEPOT, &defs);
        let w = (0..host.map.walls.len())
            .find(|&w| host.map.walls[w].glass)
            .expect("the depot has glass");
        let twin = host.map.walls[w].next_wall.unwrap();
        host.combat.destruct.break_glass(&mut host.map, w);
        let light = host.map.lights.iter().position(|l| l.breakable).unwrap();
        assert!(host.combat.destruct.break_light(&host.map, light));
        let snap = host.capture(1);
        let mut client = World::load(DEPOT, &defs);
        let changes = client.apply(&snap, &defs).unwrap();
        let mut glass = vec![w, twin];
        glass.sort_unstable();
        assert_eq!(changes.glass, glass);
        assert_eq!(changes.lights, vec![light]);
        assert!(changes.sectors.is_empty() && changes.cracks.is_empty());
        let again = client.apply(&snap, &defs).unwrap();
        assert!(again.is_empty(), "{again:?}");
    }

    #[test]
    fn apply_copies_quakes() {
        let defs = Defs::builtin();
        let mut host = World::load(ENGINE_LAB, &defs);
        host.mech.fire(7);
        for _ in 0..150 {
            host.mech.tick(&mut host.map, &mut [], DT);
        }
        let mid = host.mech.quake_strength();
        assert!(mid > 0.0 && mid < 0.8, "mid-fade: {mid}");
        let mut client = World::load(ENGINE_LAB, &defs);
        client.apply(&host.capture(150), &defs).unwrap();
        assert_eq!(client.mech.quake_strength(), mid);
        for _ in 0..120 {
            host.mech.tick(&mut host.map, &mut [], DT);
        }
        assert_eq!(host.mech.quake_strength(), 0.0);
        client.apply(&host.capture(270), &defs).unwrap();
        assert_eq!(client.mech.quake_strength(), 0.0);
    }

    #[test]
    fn is_sane_bounds_players_and_actors() {
        let defs = Defs::builtin();
        let host = fought_arena(&defs);
        let body = Body::spawn(&host.map, Vec2::new(5.0, 10.0), 0.35, 1.8).unwrap();
        let player = |id: u8| PlayerSnap {
            id: PlayerId(id),
            body,
            angle: 0.0,
            pitch: 0.0,
            vitals: Vitals::new(),
            arsenal: Arsenal::new(&defs),
            inventory: Inventory::new(),
            keys: KeySet::default(),
            ack: 0,
        };
        let mut snap = host.capture(1);
        snap.players = (0..MAX_PLAYERS as u8).map(player).collect();
        assert!(snap.is_sane());
        let mut bad = snap.clone();
        bad.players.push(player(0));
        assert!(!bad.is_sane(), "too many players");
        let mut bad = snap.clone();
        bad.players[1].id = PlayerId(0);
        assert!(!bad.is_sane(), "duplicate id");
        let mut bad = snap.clone();
        bad.players.truncate(1);
        bad.players[0].id = PlayerId(MAX_PLAYERS as u8);
        assert!(!bad.is_sane(), "id out of range");
        let mut bad = snap.clone();
        let last = bad.actors.last().unwrap().clone();
        bad.actors.resize(MAX_ACTORS + 1, last);
        assert!(!bad.is_sane(), "too many actors");
        bad.actors.truncate(MAX_ACTORS);
        assert!(bad.is_sane());
    }

    /// The engine lab with every mover set going (cracks opened) and 20 ticks run.
    fn stirred_lab(defs: &Defs) -> World {
        let mut host = World::load(ENGINE_LAB, defs);
        for m in 0..host.mech.movers.len() {
            let s = host.mech.movers[m].sector;
            match host.mech.movers[m].def.kind {
                MoverKind::Crack => assert!(host.combat.destruct.open_crack(&mut host.mech, s)),
                _ => host.mech.toggle(m),
            }
        }
        for _ in 0..20 {
            host.mech.tick(&mut host.map, &mut [], DT);
        }
        host
    }

    #[test]
    fn apply_moves_map_geometry() {
        let defs = Defs::builtin();
        let host = stirred_lab(&defs);
        let mid = |k: fn(&MoverKind) -> bool| {
            host.mech.movers.iter().any(|mv| {
                k(&mv.def.kind)
                    && matches!(mv.motion, Motion::ToEnd)
                    && mv.z != mv.start
                    && mv.z != mv.end
            })
        };
        assert!(mid(|k| matches!(k, MoverKind::Door)), "a door mid-travel");
        assert!(
            mid(|k| matches!(k, MoverKind::Lift { .. })),
            "a lift mid-travel"
        );
        assert!(
            mid(|k| matches!(k, MoverKind::Slide { .. })),
            "a slider mid-travel"
        );
        assert!(mid(|k| matches!(k, MoverKind::Crack)), "a crack mid-travel");

        let mut client = World::load(ENGINE_LAB, &defs);
        client.apply(&host.capture(20), &defs).unwrap();
        assert_eq!(client.map.sectors, host.map.sectors, "floor_z / ceil_z");
        assert_eq!(client.map.walls, host.map.walls, "slider walls a / b");
        for (c, h) in client.mech.movers.iter().zip(&host.mech.movers) {
            assert_eq!((c.z, c.motion), (h.z, h.motion));
        }
        assert_eq!(client.combat.destruct, host.combat.destruct, "cracks open");
    }

    #[test]
    fn apply_breaks_glass_and_lights() {
        let defs = Defs::builtin();
        let mut host = World::load(DEPOT, &defs);
        let w = (0..host.map.walls.len())
            .find(|&w| host.map.walls[w].glass)
            .expect("the depot has glass");
        let twin = host.map.walls[w].next_wall.unwrap();
        assert!(
            !host
                .combat
                .destruct
                .break_glass(&mut host.map, w)
                .is_empty()
        );
        let light = host.map.lights.iter().position(|l| l.breakable).unwrap();
        assert!(host.combat.destruct.break_light(&host.map, light));

        let mut client = World::load(DEPOT, &defs);
        let snap = host.capture(1);
        assert!(snap.world.glass_broken.contains(&w));
        assert!(snap.world.glass_broken.contains(&twin));
        client.apply(&snap, &defs).unwrap();
        assert!(!client.map.walls[w].glass);
        assert!(!client.map.walls[twin].glass);
        assert_eq!(client.map.walls, host.map.walls);
        assert_eq!(client.combat.destruct, host.combat.destruct);
        assert!(client.combat.destruct.light_broken(light));
    }

    #[test]
    fn apply_sets_switches_items_props() {
        let defs = Defs::builtin();
        let mut host = World::load(ENGINE_LAB, &defs);
        assert!(!host.mech.switch_on.is_empty() && !host.mech.taken.is_empty());
        host.mech.switch_on[0] = true;
        let last = host.mech.taken.len() - 1;
        host.mech.taken[last] = true;
        let mut vitals = Vitals::new();
        let n = host.mech.props.len();
        for i in 0..n {
            host.mech.use_prop(i, &mut vitals, 0);
        }
        let mut client = World::load(ENGINE_LAB, &defs);
        client.apply(&host.capture(1), &defs).unwrap();
        assert_eq!(client.mech.switch_on, host.mech.switch_on);
        assert_eq!(client.mech.taken, host.mech.taken);
        assert_eq!(client.mech.props, host.mech.props);
        assert!(client.mech.props.iter().any(|p| p.cooldown > 0.0));
    }

    #[test]
    fn apply_rejects_wrong_level() {
        let defs = Defs::builtin();
        let snap = fought_arena(&defs).capture(120);
        let mut client = World::load(ENGINE_LAB, &defs);
        let before = client.state();
        assert!(matches!(
            client.apply(&snap, &defs),
            Err(ApplyError::Shape(_))
        ));
        assert_eq!(client.state(), before);
    }

    #[test]
    fn apply_rejects_bad_shapes_unchanged() {
        let defs = Defs::builtin();
        let mut host = World::load(ENGINE_LAB, &defs);
        host.combat
            .projectiles
            .push(bolt(5, Vec3::new(1.0, 1.0, 1.0)));
        let mut good = host.capture(1);
        let start = host.map.player_start.pos;
        good.players.push(PlayerSnap {
            id: PlayerId(0),
            body: Body::spawn(&host.map, Vec2::new(start.0, start.1), 0.35, 1.8).unwrap(),
            angle: 0.0,
            pitch: 0.0,
            vitals: Vitals::new(),
            arsenal: crate::weapons::Arsenal::new(&defs),
            inventory: Inventory::new(),
            keys: KeySet::default(),
            ack: 0,
        });
        type Tamper = fn(&mut Snapshot);
        let tampers: [(&str, Tamper); 15] = [
            ("player sector", |s| {
                let mut p = s.players[0].clone();
                p.body.sector = 9999;
                s.players.push(PlayerSnap {
                    id: PlayerId(1),
                    ..p
                });
            }),
            ("player id", |s| {
                let p = s.players[0].clone();
                s.players.push(PlayerSnap {
                    id: PlayerId(MAX_PLAYERS as u8),
                    ..p
                });
            }),
            ("mover count", |s| {
                s.world.movers.pop();
            }),
            ("switch count", |s| s.world.switch_on.push(true)),
            ("item count", |s| {
                s.world.taken.pop();
            }),
            ("prop count", |s| {
                s.world.props.pop();
            }),
            ("glass wall", |s| s.world.glass_broken.push(usize::MAX)),
            ("destruct", |s| s.world.destruct = Destruct::default()),
            ("actor sector", |s| {
                s.actors.last_mut().unwrap().body.sector = 9999
            }),
            ("actor kind", |s| {
                let a = &mut s.actors[0];
                a.kind = if a.kind == ActorKind::Grunt {
                    ActorKind::Slasher
                } else {
                    ActorKind::Grunt
                };
            }),
            ("fewer actors", |s| {
                s.actors.pop();
            }),
            ("projectile sector", |s| s.projectiles[0].sector = 9999),
            ("quake count", |s| {
                s.world.quakes.pop();
            }),
            ("mover z out of range", |s| s.world.movers[0].z = 1.0e6),
            ("too many actors", |s| {
                let last = s.actors.last().unwrap().clone();
                s.actors.resize(MAX_ACTORS + 1, last);
            }),
        ];
        for (what, tamper) in tampers {
            let mut snap = good.clone();
            // Change every mover too (to mid-travel), so a partial apply would show.
            for (m, mv) in snap.world.movers.iter_mut().zip(&host.mech.movers) {
                m.z = (mv.start + mv.end) / 2.0;
            }
            tamper(&mut snap);
            let mut client = World::load(ENGINE_LAB, &defs);
            let before = client.state();
            assert!(
                matches!(client.apply(&snap, &defs), Err(ApplyError::Shape(_))),
                "{what} accepted"
            );
            assert_eq!(client.state(), before, "{what} changed the client");
        }
        let mut client = World::load(ENGINE_LAB, &defs);
        client.apply(&good, &defs).unwrap();
    }

    #[test]
    fn apply_is_idempotent() {
        let defs = Defs::builtin();
        let mut host = fought_arena(&defs);
        host.combat
            .projectiles
            .push(bolt(3, Vec3::new(5.0, 9.0, 1.0)));
        let snap = host.capture(120);
        let mut client = World::load(ARENA, &defs);
        client.apply(&snap, &defs).unwrap();
        let once = client.state();
        client.apply(&snap, &defs).unwrap();
        let mut twice = client.state();
        // `prev_pos` is "where the client had it": after the second apply that is the snapped
        // position itself. Everything else is the same.
        for (a, b) in twice.0.iter().zip(&snap.actors) {
            assert_eq!(a.prev_pos, b.body.pos);
        }
        for (a, b) in twice.0.iter_mut().zip(&once.0) {
            a.prev_pos = b.prev_pos;
        }
        assert_eq!(twice, once);
    }

    #[test]
    fn snapshot_round_trips_through_serde() {
        let defs = Defs::builtin();
        let mut host = fought_arena(&defs);
        host.combat
            .projectiles
            .push(bolt(9, Vec3::new(5.0, 9.0, 1.0)));
        let body = Body::spawn(&host.map, Vec2::new(5.0, 10.0), 0.35, 1.8).unwrap();
        let player = PlayerSnap {
            id: PlayerId(1),
            body,
            angle: 0.5,
            pitch: -0.25,
            vitals: Vitals::new(),
            arsenal: Arsenal::new(&defs),
            inventory: Inventory::new(),
            keys: KeySet::default(),
            ack: 42,
        };
        let snap = Snapshot::capture(
            7,
            &host.combat,
            &host.mech,
            &host.map,
            &host.source,
            vec![player],
        );
        let text = ron::to_string(&snap).expect("serialises");
        let back: Snapshot = ron::from_str(&text).expect("deserialises");
        assert_eq!(back, snap);
    }

    #[test]
    fn is_finite_flags_nan_and_infinity() {
        let defs = Defs::builtin();
        let mut host = fought_arena(&defs);
        host.combat
            .projectiles
            .push(bolt(9, Vec3::new(5.0, 9.0, 1.0)));
        let snap = host.capture(1);
        assert!(snap.is_finite());
        let mut bad = snap.clone();
        bad.actors[0].angle = f32::NAN;
        assert!(!bad.is_finite());
        let mut bad = snap.clone();
        bad.actors[0].body.pos.x = f32::INFINITY;
        assert!(!bad.is_finite());
        let mut bad = snap.clone();
        bad.projectiles[0].vel.y = f32::NAN;
        assert!(!bad.is_finite());
        let mut bad = snap.clone();
        bad.actors[0].state = AiState::Pain { t: f32::NAN };
        assert!(!bad.is_finite());
        let plan = crate::defs::BurrowDef {
            every: (1.0, 2.0),
            first: 1.0,
            dig: 1.0,
            under: 1.0,
            rise: 1.0,
            spawn: 0,
        };
        let mut bad = snap.clone();
        bad.actors[0].state = AiState::Under { t: 1.0, plan };
        assert!(bad.is_finite());
        let mut nan_plan = plan;
        nan_plan.rise = f32::NAN;
        bad.actors[0].state = AiState::Burrow {
            t: 1.0,
            plan: nan_plan,
        };
        assert!(!bad.is_finite());
        let mut bad = snap.clone();
        bad.world.movers.push(MoverSnap {
            z: f32::NAN,
            motion: Motion::AtStart,
        });
        assert!(!bad.is_finite());
        let mut bad = snap.clone();
        bad.world.movers.push(MoverSnap {
            z: 0.0,
            motion: Motion::AtEnd { wait: f32::NAN },
        });
        assert!(!bad.is_finite());
        let mut bad = snap.clone();
        bad.world.quakes.push(Some(f32::INFINITY));
        assert!(!bad.is_finite());
        let body = Body::spawn(&host.map, Vec2::new(5.0, 10.0), 0.35, 1.8).unwrap();
        let player = PlayerSnap {
            id: PlayerId(0),
            body,
            angle: 0.0,
            pitch: 0.0,
            vitals: Vitals::new(),
            arsenal: Arsenal::new(&defs),
            inventory: Inventory::new(),
            keys: KeySet::default(),
            ack: 0,
        };
        let mut ok = snap.clone();
        ok.players.push(player.clone());
        assert!(ok.is_finite());
        let mut bad = ok.clone();
        bad.players[0].pitch = f32::NAN;
        assert!(!bad.is_finite());
        let mut bad = ok.clone();
        bad.players[0].body.vel.z = f32::NEG_INFINITY;
        assert!(!bad.is_finite());
        let mut bad = ok;
        bad.players[0].arsenal.phase = crate::weapons::WeaponPhase::Reloading(f32::NAN);
        assert!(!bad.is_finite());
    }

    /// The arena's snapshot with actor 0 moved to `at`.
    fn arena_snap(defs: &Defs, at: Vec3) -> Snapshot {
        let mut snap = World::load(ARENA, defs).capture(0);
        snap.actors[0].body.pos = at;
        snap
    }

    /// A player standing at `at` on the arena floor.
    fn player_at(defs: &Defs, id: u8, at: Vec2) -> PlayerSnap {
        let world = World::load(ARENA, defs);
        PlayerSnap {
            id: PlayerId(id),
            body: Body::spawn(&world.map, at, 0.35, 1.8).expect("player spot"),
            angle: 0.0,
            pitch: 0.0,
            vitals: Vitals::new(),
            arsenal: Arsenal::new(defs),
            inventory: Inventory::new(),
            keys: KeySet::default(),
            ack: 0,
        }
    }

    #[test]
    fn blend_halfway() {
        let defs = Defs::builtin();
        let a = arena_snap(&defs, Vec3::new(0.0, 5.0, 0.0));
        let b = arena_snap(&defs, Vec3::new(2.0, 5.0, 0.0));
        let mut combat = World::load(ARENA, &defs).combat;
        let before = Vec3::new(0.5, 5.0, 0.0);
        combat.actors[0].body.pos = before;
        a.blend(&b, 0.5, &mut combat);
        let actor = &combat.actors[0];
        assert!((actor.body.pos - Vec3::new(1.0, 5.0, 0.0)).length() < 1e-6);
        assert_eq!(
            actor.prev_pos, before,
            "prev_pos is where the client had it"
        );
    }

    #[test]
    fn blend_matches_projectiles_by_id() {
        let defs = Defs::builtin();
        let mut a = arena_snap(&defs, Vec3::new(0.0, 5.0, 0.0));
        let mut b = a.clone();
        a.projectiles = vec![
            bolt(1, Vec3::new(0.0, 0.0, 1.0)),
            bolt(2, Vec3::new(0.0, 1.0, 1.0)),
        ];
        // Same ids, reversed order.
        b.projectiles = vec![
            bolt(2, Vec3::new(2.0, 1.0, 1.0)),
            bolt(1, Vec3::new(4.0, 0.0, 1.0)),
        ];
        let mut combat = World::load(ARENA, &defs).combat;
        combat.projectiles = vec![bolt(2, Vec3::splat(9.0)), bolt(1, Vec3::splat(9.0))];
        a.blend(&b, 0.5, &mut combat);
        let shot = |id| combat.projectiles.iter().find(|p| p.id == id).unwrap();
        assert_eq!(shot(1).pos, Vec3::new(2.0, 0.0, 1.0));
        assert_eq!(shot(2).pos, Vec3::new(1.0, 1.0, 1.0));
        assert_eq!(shot(1).prev, Vec3::splat(9.0));
        assert_eq!(shot(2).prev, Vec3::splat(9.0));
    }

    #[test]
    fn blend_skips_newcomers() {
        let defs = Defs::builtin();
        let mut a = arena_snap(&defs, Vec3::new(0.0, 5.0, 0.0));
        let mut b = arena_snap(&defs, Vec3::new(2.0, 5.0, 0.0));
        let mut combat = World::load(ARENA, &defs).combat;
        // Actor only `next` knows about: the client's copy stays as it is.
        let newcomer = combat.actors.len() - 1;
        a.actors.truncate(newcomer);
        let before = combat.actors[newcomer].clone();
        // Projectile only in `next`: the client's copy of its id stays as it is.
        b.projectiles = vec![bolt(7, Vec3::X)];
        a.projectiles = Vec::new();
        combat.projectiles = vec![bolt(7, Vec3::Y)];
        a.players = vec![player_at(&defs, 0, Vec2::new(5.0, 10.0))];
        b.players = vec![
            player_at(&defs, 0, Vec2::new(5.5, 10.0)),
            player_at(&defs, 1, Vec2::new(5.0, 10.5)),
        ];
        let players = a.blend(&b, 0.5, &mut combat);
        assert_eq!(combat.actors[newcomer], before);
        assert_eq!(combat.projectiles[0].pos, Vec3::Y);
        assert_eq!(combat.projectiles[0].prev, Vec3::Y);
        assert_eq!(players.len(), 1);
        assert_eq!(players[0].0, PlayerId(0));
    }

    #[test]
    fn blend_clamps_f() {
        let defs = Defs::builtin();
        let a = arena_snap(&defs, Vec3::new(0.0, 5.0, 0.0));
        let b = arena_snap(&defs, Vec3::new(2.0, 5.0, 0.0));
        let mut over = World::load(ARENA, &defs).combat;
        let mut one = World::load(ARENA, &defs).combat;
        let players_over = a.blend(&b, 2.0, &mut over);
        let players_one = a.blend(&b, 1.0, &mut one);
        assert_eq!(over.actors[0].body.pos, Vec3::new(2.0, 5.0, 0.0));
        assert_eq!(over.actors[0].body.pos, one.actors[0].body.pos);
        assert_eq!(over.actors[0].prev_pos, one.actors[0].prev_pos);
        assert_eq!(players_over, players_one);
    }
}
