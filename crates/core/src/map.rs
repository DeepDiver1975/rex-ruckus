//! Level format (RON) and the built, linked map.
//!
//! Authoring rule: each sector lists vertex-index loops. The first loop is the outer boundary,
//! counter-clockwise; further loops are holes, clockwise. A wall a→b becomes a portal into the
//! sector that owns the edge b→a.

use glam::Vec2;
use serde::Deserialize;
use std::collections::HashMap;
use thiserror::Error;

pub type SectorId = usize;
pub type WallId = usize;
pub type MaterialId = usize;

#[derive(Debug, Clone, Deserialize)]
pub struct RawLevel {
    pub name: String,
    pub materials: Vec<String>,
    pub vertices: Vec<(f32, f32)>,
    pub sectors: Vec<RawSector>,
    pub player_start: PlayerStart,
    #[serde(default)]
    pub lights: Vec<RawLight>,
    #[serde(default)]
    pub switches: Vec<RawSwitch>,
    #[serde(default)]
    pub items: Vec<RawItem>,
    #[serde(default)]
    pub actors: Vec<RawActor>,
    /// Breakable glass panes: vertex indices (from, to) of one side of a portal, like a switch's
    /// `wall`. Both sides of the portal get `Wall::glass`.
    #[serde(default)]
    pub glass: Vec<(usize, usize)>,
}

/// Material name the glass pane of a `Wall::glass` portal renders with. `Map::from_raw` appends
/// it to `Map::materials` when the level has glass and does not list it already.
pub const GLASS_MATERIAL: &str = "glass";

#[derive(Debug, Clone, Deserialize)]
pub struct RawSector {
    pub loops: Vec<Vec<usize>>,
    pub floor_z: f32,
    pub ceil_z: f32,
    pub floor_mat: MaterialId,
    pub ceil_mat: MaterialId,
    pub wall_mat: MaterialId,
    /// Material neighbours use for step faces on portals into this sector (a door's face).
    #[serde(default)]
    pub face_mat: Option<MaterialId>,
    #[serde(default)]
    pub mover: Option<MoverDef>,
    /// A secret area: the level counts it once the player first stands in it.
    #[serde(default)]
    pub secret: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Deserialize)]
pub struct PlayerStart {
    pub pos: (f32, f32),
    pub angle_deg: f32,
}

#[derive(Debug, Clone, Copy, PartialEq, Deserialize)]
pub struct RawLight {
    pub pos: (f32, f32, f32),
    pub color: (f32, f32, f32),
    /// Luminous power in lumens.
    pub intensity: f32,
    pub range: f32,
    /// A shot or blast can break the fixture, putting the light out for good.
    #[serde(default)]
    pub breakable: bool,
}

/// Keycard colours. Locked doors and keyed switches name one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Deserialize)]
pub enum Key {
    Red,
    Blue,
    Yellow,
}

impl Key {
    pub const ALL: [Key; 3] = [Key::Red, Key::Blue, Key::Yellow];

    pub fn name(self) -> &'static str {
        match self {
            Key::Red => "red",
            Key::Blue => "blue",
            Key::Yellow => "yellow",
        }
    }
}

/// The keycards someone holds.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct KeySet(u8);

impl KeySet {
    pub fn insert(&mut self, k: Key) {
        self.0 |= 1 << k as u8;
    }

    pub fn contains(self, k: Key) -> bool {
        self.0 & (1 << k as u8) != 0
    }
}

/// Trigger channel (Build's lotag/hitag pairing): a switch fires it and every mover listening toggles.
pub type Channel = u16;

#[derive(Debug, Clone, Copy, PartialEq, Deserialize)]
pub enum MoverKind {
    /// The ceiling travels from the floor (closed, the start pose) up to the authored `ceil_z`.
    Door,
    /// The floor travels between the authored `floor_z` (start) and `to`.
    Lift { to: f32 },
    /// A sealed wall that only an explosion opens, for good. Authored open like a door (the
    /// ceiling at its open height) and closed by `Mechanics::new`; the use key and channels
    /// leave it alone.
    Crack,
}

#[derive(Debug, Clone, Copy, PartialEq, Deserialize)]
pub struct MoverDef {
    pub kind: MoverKind,
    /// Metres per second.
    #[serde(default = "default_mover_speed")]
    pub speed: f32,
    /// Keycard needed to operate it by hand.
    #[serde(default)]
    pub lock: Option<Key>,
    /// When set, only switches on this channel operate it; the use key does nothing.
    #[serde(default)]
    pub channel: Option<Channel>,
    /// Seconds to wait at the far end before returning on its own.
    #[serde(default)]
    pub auto_return: Option<f32>,
}

fn default_mover_speed() -> f32 {
    2.5
}

#[derive(Debug, Clone, Copy, PartialEq, Deserialize)]
pub enum SwitchAction {
    Channel(Channel),
    Exit,
}

#[derive(Debug, Clone, Copy, PartialEq, Deserialize)]
pub struct RawSwitch {
    /// Vertex indices (from, to) of the wall it is mounted on; it faces the sector owning that edge.
    pub wall: (usize, usize),
    pub action: SwitchAction,
    #[serde(default)]
    pub key: Option<Key>,
}

#[derive(Debug, Clone, Copy, PartialEq, Deserialize)]
pub enum ItemKind {
    Key(Key),
    PistolAmmo,
    ShotgunShells,
    Shotgun,
    HealthSmall,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Deserialize)]
pub enum ActorKind {
    Grunt,
    Enforcer,
    Slasher,
    /// Flying enemy: hovers, swoops at the player to shoot.
    Drone,
    /// Exploding barrel: a static, killable body.
    Barrel,
}

fn yes() -> bool {
    true
}

#[derive(Debug, Clone, Copy, PartialEq, Deserialize)]
pub struct RawActor {
    pub kind: ActorKind,
    pub pos: (f32, f32),
    #[serde(default)]
    pub angle_deg: f32,
    #[serde(default = "yes")]
    pub asleep: bool,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ActorSpawn {
    pub kind: ActorKind,
    pub pos: Vec2,
    /// Heading in radians (0 = +x, counter-clockwise).
    pub angle: f32,
    pub asleep: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Deserialize)]
pub struct RawItem {
    pub kind: ItemKind,
    pub pos: (f32, f32),
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Switch {
    pub wall: WallId,
    pub action: SwitchAction,
    pub key: Option<Key>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Item {
    pub kind: ItemKind,
    pub pos: Vec2,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Wall {
    pub a: Vec2,
    pub b: Vec2,
    pub sector: SectorId,
    pub next_sector: Option<SectorId>,
    pub next_wall: Option<WallId>,
    pub material: MaterialId,
    /// Runtime: an intact glass pane fills this portal. It behaves as a solid wall (movement,
    /// shots, sight, sound, pathing) until `Destruct::break_glass` clears it on both sides.
    pub glass: bool,
}

impl Wall {
    /// The sector beyond this wall when something can pass through it: `next_sector`, unless
    /// the wall is solid or holds intact glass.
    pub fn passage(&self) -> Option<SectorId> {
        self.next_sector.filter(|_| !self.glass)
    }

    /// Unit normal pointing into this wall's own sector (left of a→b).
    pub fn inward_normal(&self) -> Vec2 {
        let d = (self.b - self.a).normalize_or_zero();
        Vec2::new(-d.y, d.x)
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Sector {
    pub loops: Vec<Vec<WallId>>,
    pub floor_z: f32,
    pub ceil_z: f32,
    pub floor_mat: MaterialId,
    pub ceil_mat: MaterialId,
    pub wall_mat: MaterialId,
    pub face_mat: Option<MaterialId>,
    pub mover: Option<MoverDef>,
    pub secret: bool,
}

impl Sector {
    pub fn walls(&self) -> impl Iterator<Item = WallId> + '_ {
        self.loops.iter().flatten().copied()
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Map {
    pub name: String,
    pub materials: Vec<String>,
    pub sectors: Vec<Sector>,
    pub walls: Vec<Wall>,
    pub player_start: PlayerStart,
    pub lights: Vec<RawLight>,
    pub switches: Vec<Switch>,
    pub items: Vec<Item>,
    pub actors: Vec<ActorSpawn>,
}

#[derive(Debug, Error, PartialEq)]
pub enum MapError {
    #[error("RON parse error: {0}")]
    Parse(String),
    #[error("sector {sector}: vertex index {index} out of range")]
    BadVertex { sector: SectorId, index: usize },
    #[error("sector {sector}: loop {loop_index} has fewer than 3 vertices")]
    ShortLoop { sector: SectorId, loop_index: usize },
    #[error("sector {sector}: floor_z must be below ceil_z")]
    InvertedHeights { sector: SectorId },
    #[error("sector {sector}: has no loops")]
    NoLoops { sector: SectorId },
    #[error("sector {sector}: floor_z and ceil_z must be finite")]
    NonFiniteHeight { sector: SectorId },
    #[error("sector {sector}: material {material} out of range")]
    BadMaterial {
        sector: SectorId,
        material: MaterialId,
    },
    #[error("edge {from}->{to} is used by more than one sector in the same direction")]
    DuplicateEdge { from: usize, to: usize },
    #[error("switch {index}: no wall runs from vertex {from} to vertex {to}")]
    BadSwitchWall {
        index: usize,
        from: usize,
        to: usize,
    },
    #[error("sector {sector}: invalid mover: {reason}")]
    BadMover {
        sector: SectorId,
        reason: &'static str,
    },
    #[error("actor {index}: {reason}")]
    BadActor { index: usize, reason: &'static str },
    #[error("glass {index}: no portal runs from vertex {from} to vertex {to}")]
    GlassNotPortal {
        index: usize,
        from: usize,
        to: usize,
    },
}

impl Map {
    pub fn from_ron(src: &str) -> Result<Map, MapError> {
        let raw: RawLevel = ron::from_str(src).map_err(|e| MapError::Parse(e.to_string()))?;
        Map::from_raw(raw)
    }

    pub fn from_raw(raw: RawLevel) -> Result<Map, MapError> {
        let verts: Vec<Vec2> = raw.vertices.iter().map(|&(x, y)| Vec2::new(x, y)).collect();
        let mut walls: Vec<Wall> = Vec::new();
        let mut sectors = Vec::with_capacity(raw.sectors.len());
        let mut edge_owner: HashMap<(usize, usize), WallId> = HashMap::new();

        for (si, rs) in raw.sectors.iter().enumerate() {
            if !(rs.floor_z.is_finite() && rs.ceil_z.is_finite()) {
                return Err(MapError::NonFiniteHeight { sector: si });
            }
            if rs.loops.is_empty() {
                return Err(MapError::NoLoops { sector: si });
            }
            if rs.floor_z >= rs.ceil_z {
                return Err(MapError::InvertedHeights { sector: si });
            }
            for material in [rs.floor_mat, rs.ceil_mat, rs.wall_mat]
                .into_iter()
                .chain(rs.face_mat)
            {
                if material >= raw.materials.len() {
                    return Err(MapError::BadMaterial {
                        sector: si,
                        material,
                    });
                }
            }
            check_mover(si, rs)?;
            let mut loops = Vec::with_capacity(rs.loops.len());
            for (li, lp) in rs.loops.iter().enumerate() {
                if lp.len() < 3 {
                    return Err(MapError::ShortLoop {
                        sector: si,
                        loop_index: li,
                    });
                }
                let mut ids = Vec::with_capacity(lp.len());
                for i in 0..lp.len() {
                    let (ia, ib) = (lp[i], lp[(i + 1) % lp.len()]);
                    for index in [ia, ib] {
                        if index >= verts.len() {
                            return Err(MapError::BadVertex { sector: si, index });
                        }
                    }
                    if edge_owner.insert((ia, ib), walls.len()).is_some() {
                        return Err(MapError::DuplicateEdge { from: ia, to: ib });
                    }
                    ids.push(walls.len());
                    walls.push(Wall {
                        a: verts[ia],
                        b: verts[ib],
                        sector: si,
                        next_sector: None,
                        next_wall: None,
                        material: rs.wall_mat,
                        glass: false,
                    });
                }
                loops.push(ids);
            }
            sectors.push(Sector {
                loops,
                floor_z: rs.floor_z,
                ceil_z: rs.ceil_z,
                floor_mat: rs.floor_mat,
                ceil_mat: rs.ceil_mat,
                wall_mat: rs.wall_mat,
                face_mat: rs.face_mat,
                mover: rs.mover,
                secret: rs.secret,
            });
        }

        for (&(ia, ib), &wid) in &edge_owner {
            if let Some(&other) = edge_owner.get(&(ib, ia)) {
                walls[wid].next_sector = Some(walls[other].sector);
                walls[wid].next_wall = Some(other);
            }
        }

        for (index, &(from, to)) in raw.glass.iter().enumerate() {
            let side = edge_owner.get(&(from, to)).copied();
            let Some((w, back)) = side.and_then(|w| Some((w, walls[w].next_wall?))) else {
                return Err(MapError::GlassNotPortal { index, from, to });
            };
            walls[w].glass = true;
            walls[back].glass = true;
        }
        let mut materials = raw.materials;
        if !raw.glass.is_empty() && !materials.iter().any(|m| m == GLASS_MATERIAL) {
            materials.push(GLASS_MATERIAL.to_string());
        }

        let switches = raw
            .switches
            .iter()
            .enumerate()
            .map(|(index, s)| {
                let (from, to) = s.wall;
                edge_owner
                    .get(&(from, to))
                    .map(|&wall| Switch {
                        wall,
                        action: s.action,
                        key: s.key,
                    })
                    .ok_or(MapError::BadSwitchWall { index, from, to })
            })
            .collect::<Result<Vec<_>, _>>()?;
        let items = raw
            .items
            .iter()
            .map(|i| Item {
                kind: i.kind,
                pos: Vec2::new(i.pos.0, i.pos.1),
            })
            .collect();
        let actors = raw
            .actors
            .iter()
            .enumerate()
            .map(|(index, a)| {
                if !(a.pos.0.is_finite() && a.pos.1.is_finite()) {
                    return Err(MapError::BadActor {
                        index,
                        reason: "position must be finite",
                    });
                }
                if !a.angle_deg.is_finite() {
                    return Err(MapError::BadActor {
                        index,
                        reason: "angle must be finite",
                    });
                }
                Ok(ActorSpawn {
                    kind: a.kind,
                    pos: Vec2::new(a.pos.0, a.pos.1),
                    angle: a.angle_deg.to_radians(),
                    asleep: a.asleep,
                })
            })
            .collect::<Result<Vec<_>, _>>()?;

        Ok(Map {
            name: raw.name,
            materials,
            sectors,
            walls,
            player_start: raw.player_start,
            lights: raw.lights,
            switches,
            items,
            actors,
        })
    }
}

impl Map {
    /// Index of `GLASS_MATERIAL` in `materials`, if the level has one.
    pub fn glass_material(&self) -> Option<MaterialId> {
        self.materials.iter().position(|m| m == GLASS_MATERIAL)
    }
}

fn check_mover(sector: SectorId, rs: &RawSector) -> Result<(), MapError> {
    let Some(m) = rs.mover else { return Ok(()) };
    let bad = |reason| Err(MapError::BadMover { sector, reason });
    if !(m.speed > 0.0 && m.speed.is_finite()) {
        return bad("speed must be positive");
    }
    if m.auto_return.is_some_and(|t| t.is_nan() || t < 0.0) {
        return bad("auto_return must not be negative");
    }
    if let MoverKind::Lift { to } = m.kind {
        if !to.is_finite() {
            return bad("lift end must be finite");
        }
        if to == rs.floor_z {
            return bad("lift end must differ from its start");
        }
        if to >= rs.ceil_z {
            return bad("lift end must be below the ceiling");
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fixtures::{pillar_room, two_rooms};

    #[test]
    fn shared_edge_becomes_a_mirrored_portal() {
        let map = two_rooms(0.0, 3.0);
        let portals: Vec<&Wall> = map
            .walls
            .iter()
            .filter(|w| w.next_sector.is_some())
            .collect();
        assert_eq!(portals.len(), 2);
        let a_side = portals.iter().find(|w| w.sector == 0).unwrap();
        assert_eq!(a_side.next_sector, Some(1));
        assert_eq!(a_side.a, Vec2::new(4.0, 0.0));
        assert_eq!(a_side.b, Vec2::new(4.0, 4.0));
        let back = &map.walls[a_side.next_wall.unwrap()];
        assert_eq!((back.sector, back.next_sector), (1, Some(0)));
        assert_eq!((back.a, back.b), (a_side.b, a_side.a));
    }

    #[test]
    fn hole_loops_are_kept_and_solid() {
        let map = pillar_room();
        assert_eq!(map.sectors[0].loops.len(), 2);
        assert_eq!(map.walls.len(), 8);
        assert!(map.walls.iter().all(|w| w.next_sector.is_none()));
    }

    #[test]
    fn inward_normal_points_into_ccw_sector() {
        let map = two_rooms(0.0, 3.0);
        // First wall of A runs (0,0)->(4,0); the room interior is +y.
        assert_eq!(map.walls[0].inward_normal(), Vec2::new(0.0, 1.0));
    }

    fn level(vertices: &str, sectors: &str) -> String {
        format!(
            r#"(name: "t", materials: ["m"], vertices: [{vertices}], sectors: [{sectors}],
                player_start: (pos: (0.5, 0.5), angle_deg: 0.0))"#
        )
    }

    #[test]
    fn rejects_out_of_range_vertex() {
        let src = level(
            "(0.0,0.0),(1.0,0.0),(1.0,1.0)",
            "(loops: [[0,1,7]], floor_z: 0.0, ceil_z: 1.0, floor_mat: 0, ceil_mat: 0, wall_mat: 0)",
        );
        assert_eq!(
            Map::from_ron(&src).unwrap_err(),
            MapError::BadVertex {
                sector: 0,
                index: 7
            }
        );
    }

    #[test]
    fn rejects_short_loop() {
        let src = level(
            "(0.0,0.0),(1.0,0.0)",
            "(loops: [[0,1]], floor_z: 0.0, ceil_z: 1.0, floor_mat: 0, ceil_mat: 0, wall_mat: 0)",
        );
        assert_eq!(
            Map::from_ron(&src).unwrap_err(),
            MapError::ShortLoop {
                sector: 0,
                loop_index: 0
            }
        );
    }

    #[test]
    fn rejects_inverted_heights() {
        let src = level(
            "(0.0,0.0),(1.0,0.0),(1.0,1.0)",
            "(loops: [[0,1,2]], floor_z: 2.0, ceil_z: 1.0, floor_mat: 0, ceil_mat: 0, wall_mat: 0)",
        );
        assert_eq!(
            Map::from_ron(&src).unwrap_err(),
            MapError::InvertedHeights { sector: 0 }
        );
    }

    #[test]
    fn rejects_sectors_without_loops() {
        let src = level(
            "(0.0,0.0),(1.0,0.0),(1.0,1.0)",
            "(loops: [], floor_z: 0.0, ceil_z: 1.0, floor_mat: 0, ceil_mat: 0, wall_mat: 0)",
        );
        assert_eq!(
            Map::from_ron(&src).unwrap_err(),
            MapError::NoLoops { sector: 0 }
        );
    }

    #[test]
    fn rejects_non_finite_heights() {
        for (floor, ceil) in [
            ("NaN", "1.0"),
            ("0.0", "NaN"),
            ("0.0", "inf"),
            ("-inf", "1.0"),
        ] {
            let src = level(
                "(0.0,0.0),(1.0,0.0),(1.0,1.0)",
                &format!(
                    "(loops: [[0,1,2]], floor_z: {floor}, ceil_z: {ceil}, floor_mat: 0, ceil_mat: 0, wall_mat: 0)"
                ),
            );
            assert_eq!(
                Map::from_ron(&src).unwrap_err(),
                MapError::NonFiniteHeight { sector: 0 },
                "{floor}..{ceil}"
            );
        }
    }

    #[test]
    fn rejects_bad_material() {
        let src = level(
            "(0.0,0.0),(1.0,0.0),(1.0,1.0)",
            "(loops: [[0,1,2]], floor_z: 0.0, ceil_z: 1.0, floor_mat: 3, ceil_mat: 0, wall_mat: 0)",
        );
        assert_eq!(
            Map::from_ron(&src).unwrap_err(),
            MapError::BadMaterial {
                sector: 0,
                material: 3
            }
        );
    }

    #[test]
    fn rejects_duplicate_directed_edge() {
        let src = level(
            "(0.0,0.0),(1.0,0.0),(1.0,1.0)",
            "(loops: [[0,1,2]], floor_z: 0.0, ceil_z: 1.0, floor_mat: 0, ceil_mat: 0, wall_mat: 0),
             (loops: [[0,1,2]], floor_z: 0.0, ceil_z: 1.0, floor_mat: 0, ceil_mat: 0, wall_mat: 0)",
        );
        assert_eq!(
            Map::from_ron(&src).unwrap_err(),
            MapError::DuplicateEdge { from: 0, to: 1 }
        );
    }

    #[test]
    fn reports_parse_errors() {
        assert!(matches!(
            Map::from_ron("(nonsense"),
            Err(MapError::Parse(_))
        ));
    }

    use crate::fixtures::{door_rooms, lift_shaft};

    #[test]
    fn parses_movers_switches_and_items() {
        let map = door_rooms(
            "(kind: Door, lock: Some(Red), auto_return: Some(3.0))",
            "switches: [(wall: (7, 0), action: Channel(4), key: Some(Blue))],
             items: [(kind: Key(Red), pos: (2.0, 3.0))],",
        );
        let def = map.sectors[1].mover.unwrap();
        assert_eq!(def.kind, MoverKind::Door);
        assert_eq!(
            (def.speed, def.lock, def.channel, def.auto_return),
            (2.5, Some(Key::Red), None, Some(3.0))
        );
        assert_eq!(map.sectors[1].face_mat, Some(3));
        let sw = map.switches[0];
        assert_eq!(
            (map.walls[sw.wall].a, map.walls[sw.wall].b),
            (Vec2::new(0.0, 4.0), Vec2::new(0.0, 0.0))
        );
        assert_eq!(
            (sw.action, sw.key),
            (SwitchAction::Channel(4), Some(Key::Blue))
        );
        assert_eq!(
            map.items,
            vec![Item {
                kind: ItemKind::Key(Key::Red),
                pos: Vec2::new(2.0, 3.0)
            }]
        );
        assert_eq!(
            lift_shaft("(kind: Lift(to: 2.0))", "").sectors[1]
                .mover
                .unwrap()
                .kind,
            MoverKind::Lift { to: 2.0 }
        );
    }

    #[test]
    fn parses_new_item_kinds() {
        let map = door_rooms(
            "(kind: Door)",
            "items: [(kind: PistolAmmo, pos: (1.0, 1.0)), (kind: ShotgunShells, pos: (1.0, 2.0)),
                     (kind: Shotgun, pos: (1.0, 3.0)), (kind: HealthSmall, pos: (2.0, 1.0))],",
        );
        let kinds: Vec<_> = map.items.iter().map(|i| i.kind).collect();
        assert_eq!(
            kinds,
            vec![
                ItemKind::PistolAmmo,
                ItemKind::ShotgunShells,
                ItemKind::Shotgun,
                ItemKind::HealthSmall
            ]
        );
    }

    #[test]
    fn parses_actors_with_defaults() {
        let map = door_rooms(
            "(kind: Door)",
            "actors: [(kind: Grunt, pos: (1.0, 2.0)),
                      (kind: Grunt, pos: (3.0, 1.0), angle_deg: 90.0, asleep: false)],",
        );
        assert_eq!(map.actors.len(), 2);
        let a = map.actors[0];
        assert_eq!(
            (a.kind, a.pos, a.angle, a.asleep),
            (ActorKind::Grunt, Vec2::new(1.0, 2.0), 0.0, true)
        );
        let b = map.actors[1];
        assert!((b.angle - std::f32::consts::FRAC_PI_2).abs() < 1e-6 && !b.asleep);
    }

    #[test]
    fn rejects_non_finite_actor() {
        let mut raw: RawLevel = ron::from_str(include_str!("../../../assets/levels/test_yard.ron"))
            .expect("shipped level parses");
        raw.actors.push(RawActor {
            kind: ActorKind::Grunt,
            pos: (f32::NAN, 0.0),
            angle_deg: 0.0,
            asleep: true,
        });
        assert!(matches!(
            Map::from_raw(raw.clone()),
            Err(MapError::BadActor { index: 0, .. })
        ));
        raw.actors[0].pos = (0.0, 0.0);
        raw.actors[0].angle_deg = f32::INFINITY;
        assert!(matches!(
            Map::from_raw(raw),
            Err(MapError::BadActor { index: 0, .. })
        ));
    }

    #[test]
    fn key_set_tracks_each_colour() {
        let mut k = KeySet::default();
        assert!(!k.contains(Key::Red));
        k.insert(Key::Blue);
        assert!(k.contains(Key::Blue) && !k.contains(Key::Red) && !k.contains(Key::Yellow));
    }

    #[test]
    fn rejects_switch_on_missing_edge() {
        let src = level(
            "(0.0,0.0),(1.0,0.0),(1.0,1.0)",
            "(loops: [[0,1,2]], floor_z: 0.0, ceil_z: 1.0, floor_mat: 0, ceil_mat: 0, wall_mat: 0)",
        )
        .replace(
            "player_start",
            "switches: [(wall: (1, 0), action: Exit)], player_start",
        );
        assert_eq!(
            Map::from_ron(&src).unwrap_err(),
            MapError::BadSwitchWall {
                index: 0,
                from: 1,
                to: 0
            }
        );
    }

    #[test]
    fn rejects_bad_movers() {
        let with_mover = |m: &str| {
            Map::from_ron(&level(
                "(0.0,0.0),(1.0,0.0),(1.0,1.0)",
                &format!(
                    "(loops: [[0,1,2]], floor_z: 0.0, ceil_z: 3.0, floor_mat: 0, ceil_mat: 0, wall_mat: 0, mover: Some({m}))"
                ),
            ))
        };
        for (m, reason) in [
            (
                "(kind: Lift(to: 3.0))",
                "lift end must be below the ceiling",
            ),
            (
                "(kind: Lift(to: 0.0))",
                "lift end must differ from its start",
            ),
            ("(kind: Lift(to: inf))", "lift end must be finite"),
            ("(kind: Lift(to: NaN))", "lift end must be finite"),
            ("(kind: Door, speed: 0.0)", "speed must be positive"),
            (
                "(kind: Door, auto_return: Some(-1.0))",
                "auto_return must not be negative",
            ),
        ] {
            assert_eq!(
                with_mover(m).unwrap_err(),
                MapError::BadMover { sector: 0, reason },
                "{m}"
            );
        }
    }

    #[test]
    fn glass_marks_both_sides_and_adds_its_material() {
        let map = crate::fixtures::glass_rooms();
        let panes: Vec<WallId> = (0..map.walls.len())
            .filter(|&w| map.walls[w].glass)
            .collect();
        assert_eq!(panes.len(), 2);
        let (w, back) = (panes[0], panes[1]);
        assert_eq!(map.walls[w].next_wall, Some(back));
        assert_eq!(map.walls[w].passage(), None);
        assert_eq!(map.glass_material(), Some(3));
        assert_eq!(map.materials[3], GLASS_MATERIAL);
        assert_eq!(two_rooms(0.0, 3.0).glass_material(), None);
    }

    #[test]
    fn glass_on_solid_wall_rejected() {
        let src = level(
            "(0.0,0.0),(1.0,0.0),(1.0,1.0)",
            "(loops: [[0,1,2]], floor_z: 0.0, ceil_z: 1.0, floor_mat: 0, ceil_mat: 0, wall_mat: 0)",
        );
        for (pair, from, to) in [("(0, 1)", 0, 1), ("(1, 0)", 1, 0)] {
            let src = src.replace("player_start", &format!("glass: [{pair}], player_start"));
            assert_eq!(
                Map::from_ron(&src).unwrap_err(),
                MapError::GlassNotPortal { index: 0, from, to },
                "{pair}"
            );
        }
    }

    #[test]
    fn rejects_bad_face_material() {
        let src = level(
            "(0.0,0.0),(1.0,0.0),(1.0,1.0)",
            "(loops: [[0,1,2]], floor_z: 0.0, ceil_z: 1.0, floor_mat: 0, ceil_mat: 0, wall_mat: 0, face_mat: Some(9))",
        );
        assert_eq!(
            Map::from_ron(&src).unwrap_err(),
            MapError::BadMaterial {
                sector: 0,
                material: 9
            }
        );
    }
}
