//! The player's weapon events: hitscan and melee rays, launched projectiles and remote
//! detonation.

use super::blasts::PendingBlast;
use super::{Combat, CombatEvent};
use crate::collide::Body;
use crate::defs::{Attack, Defs, WeaponId};
use crate::destruct::hits_pane;
use crate::explosion::Blast;
use crate::map::{Map, SectorId};
use crate::projectile::{Projectile, Shooter, Targets};
use crate::trace::{HitKind, Ray, trace};
use crate::weapons::WeaponEvent;
use glam::Vec3;

/// Upward bias added to the aim of a thrown (gravity-affected) projectile, so a bomb is lobbed.
const THROW_LIFT: f32 = 0.15;

impl Combat {
    /// Resolves one player weapon event from the eye point `eye` (inside `sector`).
    /// - `Fire` and `Kick` trace their rays against living actors. Damage is summed per actor
    ///   and applied once each, in ascending actor index. Each pellet that hits the world
    ///   reports an `Impact`, except one that hits intact glass: it shatters the pane
    ///   (`GlassBroken`) and stops there. Later pellets of the same shot already find it gone.
    /// - `Launch` spawns the weapon's projectile at the eye (`Targets::All`; the player, its
    ///   owner, is never hit by it). Thrown projectiles (with gravity) get a slight upward lob.
    /// - `Detonate` turns every live player remote bomb into a blast (see [`Combat::detonate`]).
    ///
    /// Every other event does nothing. After a shot or launch its noise wakes sleepers in
    /// earshot.
    pub fn player_attack(
        &mut self,
        map: &mut Map,
        defs: &Defs,
        eye: Vec3,
        sector: SectorId,
        ev: &WeaponEvent,
    ) -> Vec<CombatEvent> {
        let (attack, dirs, noise) = match ev {
            WeaponEvent::Fire { weapon, dirs } => {
                let def = defs.weapon(*weapon);
                (def.attack, dirs.as_slice(), def.noise)
            }
            WeaponEvent::Kick { dir } => (
                defs.weapons.kick,
                std::slice::from_ref(dir),
                defs.weapon(WeaponId::Boot).noise,
            ),
            WeaponEvent::Launch { weapon, dir } => {
                let mut out = Vec::new();
                self.launch(defs, *weapon, eye, sector, *dir);
                let woken = self.make_noise(map, eye, sector, defs.weapon(*weapon).noise);
                out.extend(woken.into_iter().map(CombatEvent::ActorWoke));
                return out;
            }
            WeaponEvent::Detonate => return self.detonate(),
            _ => return Vec::new(),
        };
        let (damage, range, dirs) = match attack {
            // Melee is a single ray along the aim.
            Attack::Melee { range, damage } => (damage, range, &dirs[..dirs.len().min(1)]),
            Attack::Hitscan { damage, range, .. } => (damage, range, dirs),
            // Projectile weapons arrive as `Launch` events.
            Attack::Projectile { .. } => return Vec::new(),
        };

        // The actors alone form the bodies slice here, so body `i` is actor `i`; the player
        // (the shooter) is simply not in it.
        let bodies: Vec<Body> = self.actors.iter().map(|a| a.body).collect();
        let mut dealt = vec![0; self.actors.len()];
        let mut out = Vec::new();
        for &dir in dirs {
            let ray = Ray {
                origin: eye,
                dir: dir.normalize_or(Vec3::X),
                sector,
                max: range,
            };
            let Some(h) = trace(map, &ray, &bodies, |i| !self.actors[i].alive(), 0.0) else {
                continue;
            };
            match h.kind {
                HitKind::Body(i) => dealt[i] += damage,
                HitKind::Wall(w) if hits_pane(map, &h) => self.break_glass(map, w, &mut out),
                _ => out.push(CombatEvent::Impact {
                    point: h.point,
                    normal: h.normal,
                    sector: h.sector,
                }),
            }
        }
        for (i, &n) in dealt.iter().enumerate() {
            if n > 0 {
                out.extend(self.damage_actor(defs, i, n));
            }
        }
        let woken = self.make_noise(map, eye, sector, noise);
        out.extend(woken.into_iter().map(CombatEvent::ActorWoke));
        out
    }

    /// Spawns `weapon`'s projectile at `eye`, flying along `dir`. A weapon without a
    /// projectile attack spawns nothing.
    fn launch(&mut self, defs: &Defs, weapon: WeaponId, eye: Vec3, sector: SectorId, dir: Vec3) {
        let Attack::Projectile { proj: pd } = defs.weapon(weapon).attack else {
            return;
        };
        let aim = dir.normalize_or(Vec3::X);
        let dir = if pd.gravity > 0.0 {
            (aim + Vec3::Z * THROW_LIFT).normalize()
        } else {
            aim
        };
        let id = self.take_id();
        self.projectiles.push(Projectile {
            id,
            pos: eye,
            prev: eye,
            vel: dir * pd.speed,
            sector,
            radius: pd.radius,
            damage: pd.damage,
            owner: Shooter::Player,
            targets: Targets::All,
            life: pd.life,
            gravity: pd.gravity,
            bounce: pd.bounce,
            remote: pd.remote,
            splash: pd.splash,
            resting: false,
        });
    }

    /// Every live player remote bomb leaves play and queues its blast (fuse 0, so it goes off
    /// in this tick's `tick`). Reports `BombsDetonated` once (if any bomb was live) and a
    /// `ProjectileGone` per bomb; the `Explosion`s follow from `tick`.
    fn detonate(&mut self) -> Vec<CombatEvent> {
        let mut out = Vec::new();
        let mut blasts = Vec::new();
        self.projectiles.retain(|p| {
            if !is_player_bomb(p) {
                return true;
            }
            // A bomb without splash data just fizzles.
            blasts.extend(p.splash.map(|splash| PendingBlast {
                fuse: 0.0,
                blast: Blast {
                    center: p.pos,
                    sector: p.sector,
                    splash,
                    owner: p.owner,
                },
            }));
            out.push(CombatEvent::ProjectileGone(p.id));
            false
        });
        if !out.is_empty() {
            out.insert(0, CombatEvent::BombsDetonated);
        }
        self.pending_blasts.extend(blasts);
        out
    }

    /// Pipe bombs the player has in the world. The game copies this into
    /// `Arsenal::live_bombs` right before ticking the arsenal.
    pub fn live_bombs(&self) -> u32 {
        self.projectiles
            .iter()
            .filter(|p| is_player_bomb(p))
            .count() as u32
    }
}

/// A remote-fused projectile thrown by the player.
fn is_player_bomb(p: &Projectile) -> bool {
    p.remote && p.owner == Shooter::Player
}
