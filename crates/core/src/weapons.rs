//! Player weapon state machine. Plain Rust: input in, [`WeaponEvent`]s out. The game drives it
//! each fixed tick and combat turns the events into hits.
//!
//! Timing: a tick covers `dt` seconds. Timers are processed in order inside the tick and the
//! unused remainder carries into the next phase, so a cooldown that ends mid-tick lets the next
//! shot fire in that same tick (the firing rate does not drift with `dt`). A reload or switch
//! pause simply is a phase during which no shot can happen; it costs exactly its configured time.

use crate::defs::{AmmoKind, Attack, Defs, WeaponId};
use crate::rng::Rng;
use glam::Vec3;

#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct WeaponInput {
    /// Fire held (a tap between ticks also counts).
    pub fire: bool,
    /// Fire went down since the last tick (edge). Detonates live pipe bombs.
    pub fire_pressed: bool,
    pub reload: bool,
    pub kick: bool,
    pub select: Option<WeaponId>,
    pub cycle: i32,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum WeaponPhase {
    Ready,
    Cooldown(f32),
    Reloading(f32),
    Switching { to: WeaponId, left: f32 },
}

#[derive(Debug, Clone, PartialEq)]
pub enum WeaponEvent {
    Fire {
        weapon: WeaponId,
        dirs: Vec<Vec3>,
    },
    /// A projectile weapon fired along `dir`; combat spawns the projectile.
    Launch {
        weapon: WeaponId,
        dir: Vec3,
    },
    /// Fire pressed with pipe bombs live: set them all off. Costs no ammo and throws nothing.
    Detonate,
    Kick {
        dir: Vec3,
    },
    DryFire,
    ReloadStart,
    ReloadDone,
    Switched(WeaponId),
}

#[derive(Debug, Clone, PartialEq)]
pub struct Arsenal {
    pub current: WeaponId,
    pub phase: WeaponPhase,
    pub owned: [bool; WeaponId::ALL.len()],
    /// Loaded rounds, by `WeaponId::index()`; only meaningful for weapons with a clip.
    pub clip: [u32; WeaponId::ALL.len()],
    /// Spare ammo, by `AmmoKind::index()`.
    pub reserve: [u32; AmmoKind::ALL.len()],
    pub kick_cooldown: f32,
    /// A manual reload press waiting for the phase to return to `Ready`.
    pub reload_queued: bool,
    /// Pipe bombs currently in the world; the combat layer sets this every tick.
    pub live_bombs: u32,
}

/// `pellets` unit vectors spread uniformly (by solid angle) over a cone of half-angle
/// `spread_deg` around `aim`. With no spread every pellet is exactly `aim`.
pub fn spread_dirs(aim: Vec3, pellets: u32, spread_deg: f32, rng: &mut Rng) -> Vec<Vec3> {
    if spread_deg <= 0.0 {
        return vec![aim; pellets as usize];
    }
    let helper = if aim.z.abs() < 0.9 { Vec3::Z } else { Vec3::X };
    let u = aim.cross(helper).normalize();
    let v = aim.cross(u);
    let cos_max = spread_deg.to_radians().cos();
    (0..pellets)
        .map(|_| {
            let cos_t = 1.0 - rng.unit() * (1.0 - cos_max);
            let sin_t = (1.0 - cos_t * cos_t).max(0.0).sqrt();
            let phi = rng.unit() * std::f32::consts::TAU;
            (aim * cos_t + (u * phi.cos() + v * phi.sin()) * sin_t).normalize()
        })
        .collect()
}

impl Arsenal {
    pub fn new(defs: &Defs) -> Arsenal {
        let mut a = Arsenal {
            current: WeaponId::Boot,
            phase: WeaponPhase::Ready,
            owned: [false; WeaponId::ALL.len()],
            clip: [0; WeaponId::ALL.len()],
            reserve: [0; AmmoKind::ALL.len()],
            kick_cooldown: 0.0,
            reload_queued: false,
            live_bombs: 0,
        };
        for k in AmmoKind::ALL {
            a.reserve[k.index()] = defs.ammo(k).start;
        }
        for &w in &defs.weapons.start_weapons {
            a.owned[w.index()] = true;
            if let (Some(size), Some(kind)) = (defs.weapon(w).clip, defs.weapon(w).ammo) {
                let n = size.min(a.reserve[kind.index()]);
                a.clip[w.index()] = n;
                a.reserve[kind.index()] -= n;
            }
        }
        a.current = a.best_armed(defs);
        a
    }

    /// Whether `w` has anything left to shoot. The boot always does.
    fn has_ammo(&self, defs: &Defs, w: WeaponId) -> bool {
        let d = defs.weapon(w);
        match d.ammo {
            None => true,
            Some(k) => {
                self.reserve[k.index()] > 0 || (d.clip.is_some() && self.clip[w.index()] > 0)
            }
        }
    }

    /// Best owned weapon with ammo by `priority` (ties go to the lower slot); priority 0 is
    /// never auto-picked. The boot is the fallback.
    fn best_armed(&self, defs: &Defs) -> WeaponId {
        WeaponId::ALL
            .into_iter()
            .filter(|&w| {
                defs.weapon(w).priority > 0 && self.owned[w.index()] && self.has_ammo(defs, w)
            })
            .max_by_key(|&w| (defs.weapon(w).priority, std::cmp::Reverse(w.index())))
            .unwrap_or(WeaponId::Boot)
    }

    /// The weapon a switch request is relative to: the one being switched to, else the current.
    fn base(&self) -> WeaponId {
        match self.phase {
            WeaponPhase::Switching { to, .. } => to,
            _ => self.current,
        }
    }

    /// Start switching to `to` (owned, not the current weapon). Cancels a reload or cooldown;
    /// moves no ammo. Re-targeting an in-progress switch keeps its remaining time.
    fn begin_switch(&mut self, defs: &Defs, to: WeaponId) {
        if !self.owned[to.index()] || to == self.current {
            return;
        }
        self.reload_queued = false;
        match &mut self.phase {
            WeaponPhase::Switching { to: t, .. } => *t = to,
            phase => {
                *phase = WeaponPhase::Switching {
                    to,
                    left: defs.weapon(to).switch_time,
                }
            }
        }
    }

    fn cycle_target(&self, defs: &Defs, dir: i32) -> Option<WeaponId> {
        let n = WeaponId::ALL.len() as i32;
        let base = self.base();
        let start = base.index() as i32;
        (1..n)
            .map(|step| WeaponId::ALL[(start + dir.signum() * step).rem_euclid(n) as usize])
            .find(|&w| self.owned[w.index()] && (w == WeaponId::Boot || self.has_ammo(defs, w)))
    }

    pub fn tick(
        &mut self,
        defs: &Defs,
        input: &WeaponInput,
        aim: Vec3,
        rng: &mut Rng,
        dt: f32,
    ) -> Vec<WeaponEvent> {
        let mut ev = vec![];

        // Quick-kick: its own cooldown, any phase.
        self.kick_cooldown = (self.kick_cooldown - dt).max(0.0);
        if input.kick && self.kick_cooldown <= 0.0 {
            self.kick_cooldown = defs.weapons.kick_refire;
            ev.push(WeaponEvent::Kick { dir: aim });
        }

        // Latch the press so it survives Cooldown/Reloading/Switching until `Ready`.
        self.reload_queued |= input.reload;

        // Selection (edge input, applied once, in any phase).
        let wanted = match input.select {
            Some(w) if w != self.base() => Some(w),
            Some(_) => None,
            None if input.cycle != 0 => self.cycle_target(defs, input.cycle),
            None => None,
        };
        if let Some(w) = wanted {
            self.begin_switch(defs, w);
        }

        // Remote detonation: a fresh press with bombs out, unless mid-switch or mid-reload.
        // It eats the press (no throw this tick) and costs one refire.
        let mut masked = *input;
        if self.current == WeaponId::PipeBombs
            && input.fire_pressed
            && self.live_bombs > 0
            && matches!(self.phase, WeaponPhase::Ready | WeaponPhase::Cooldown(_))
        {
            ev.push(WeaponEvent::Detonate);
            self.phase = WeaponPhase::Cooldown(defs.weapon(WeaponId::PipeBombs).refire);
            masked.fire = false;
        }
        let input = &masked;

        let mut left = dt;
        // Bounded: every iteration either consumes time or changes phase towards a timed one.
        for _ in 0..64 {
            match self.phase {
                WeaponPhase::Ready => {
                    if !self.ready_step(defs, input, aim, rng, &mut ev) {
                        break;
                    }
                }
                WeaponPhase::Cooldown(t) => {
                    if t > left {
                        self.phase = WeaponPhase::Cooldown(t - left);
                        break;
                    }
                    left -= t;
                    self.phase = WeaponPhase::Ready;
                }
                WeaponPhase::Reloading(t) => {
                    if t > left {
                        self.phase = WeaponPhase::Reloading(t - left);
                        break;
                    }
                    left -= t;
                    self.finish_reload(defs);
                    ev.push(WeaponEvent::ReloadDone);
                    self.phase = WeaponPhase::Ready;
                }
                WeaponPhase::Switching { to, left: t } => {
                    if t > left {
                        self.phase = WeaponPhase::Switching { to, left: t - left };
                        break;
                    }
                    left -= t;
                    self.current = to;
                    ev.push(WeaponEvent::Switched(to));
                    self.phase = WeaponPhase::Ready;
                }
            }
        }
        // Drop a press that can no longer start a reload (full clip, no reserve, no clip).
        let d = defs.weapon(self.current);
        if let (Some(size), Some(kind)) = (d.clip, d.ammo) {
            if self.clip[self.current.index()] >= size || self.reserve[kind.index()] == 0 {
                self.reload_queued = false;
            }
        } else {
            self.reload_queued = false;
        }
        ev
    }

    /// One decision while `Ready`. Returns true when the phase changed to a timed one and the
    /// loop should keep consuming time.
    fn ready_step(
        &mut self,
        defs: &Defs,
        input: &WeaponInput,
        aim: Vec3,
        rng: &mut Rng,
        ev: &mut Vec<WeaponEvent>,
    ) -> bool {
        let w = self.current;
        let d = defs.weapon(w);
        // Out of bombs with none live: put the launcher away without waiting for a press.
        if w == WeaponId::PipeBombs && self.live_bombs == 0 && !self.has_ammo(defs, w) {
            let best = self.best_armed(defs);
            self.begin_switch(defs, best);
            return matches!(self.phase, WeaponPhase::Switching { .. });
        }
        let queued = std::mem::take(&mut self.reload_queued);
        let (Some(size), Some(kind)) = (d.clip, d.ammo) else {
            return self.try_fire(defs, input, aim, rng, ev);
        };
        let (c, r) = (self.clip[w.index()], self.reserve[kind.index()]);
        let manual = queued && c < size && r > 0;
        if (c == 0 && r > 0) || manual {
            self.phase = WeaponPhase::Reloading(d.reload);
            ev.push(WeaponEvent::ReloadStart);
            return true;
        }
        self.try_fire(defs, input, aim, rng, ev)
    }

    fn try_fire(
        &mut self,
        defs: &Defs,
        input: &WeaponInput,
        aim: Vec3,
        rng: &mut Rng,
        ev: &mut Vec<WeaponEvent>,
    ) -> bool {
        if !input.fire {
            return false;
        }
        let w = self.current;
        let d = defs.weapon(w);
        if !self.has_ammo(defs, w) {
            // Bombs still out: stay armed so the next press can detonate them.
            if w == WeaponId::PipeBombs && self.live_bombs > 0 {
                return false;
            }
            ev.push(WeaponEvent::DryFire);
            let best = self.best_armed(defs);
            self.begin_switch(defs, best);
            return matches!(self.phase, WeaponPhase::Switching { .. });
        }
        if let Some(kind) = d.ammo {
            if d.clip.is_some() {
                self.clip[w.index()] -= 1;
            } else {
                self.reserve[kind.index()] -= 1;
            }
        }
        if let Attack::Projectile { .. } = d.attack {
            ev.push(WeaponEvent::Launch {
                weapon: w,
                dir: aim,
            });
            self.phase = WeaponPhase::Cooldown(d.refire);
            return true;
        }
        let dirs = match d.attack {
            Attack::Projectile { .. } => unreachable!("handled above"),
            Attack::Melee { .. } => vec![aim],
            Attack::Hitscan {
                pellets,
                spread_deg,
                ..
            } => spread_dirs(aim, pellets, spread_deg, rng),
        };
        ev.push(WeaponEvent::Fire { weapon: w, dirs });
        self.phase = WeaponPhase::Cooldown(d.refire);
        true
    }

    fn finish_reload(&mut self, defs: &Defs) {
        let d = defs.weapon(self.current);
        if let (Some(size), Some(kind)) = (d.clip, d.ammo) {
            let c = &mut self.clip[self.current.index()];
            let r = &mut self.reserve[kind.index()];
            let n = (size - *c).min(*r);
            *c += n;
            *r -= n;
        }
    }

    pub fn give_weapon(&mut self, defs: &Defs, w: WeaponId) -> bool {
        let d = defs.weapon(w);
        let newly = !self.owned[w.index()];
        let mut changed = newly;
        self.owned[w.index()] = true;
        if let Some(kind) = d.ammo {
            let mut n = d.pickup_ammo;
            if newly && let Some(size) = d.clip {
                let fill = n.min(size);
                self.clip[w.index()] += fill;
                n -= fill;
            }
            changed |= self.give_ammo(defs, kind, n);
        }
        if newly {
            self.begin_switch(defs, w);
        }
        changed
    }

    pub fn give_ammo(&mut self, defs: &Defs, k: AmmoKind, n: u32) -> bool {
        let max = defs.ammo(k).max;
        let r = &mut self.reserve[k.index()];
        let new = (*r + n).min(max);
        let changed = new > *r;
        *r = new;
        changed
    }

    pub fn readout(&self, defs: &Defs) -> (Option<u32>, Option<u32>) {
        let d = defs.weapon(self.current);
        let reserve = d.ammo.map(|k| self.reserve[k.index()]);
        let clip = d.clip.map(|_| self.clip[self.current.index()]);
        (clip, reserve)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fixtures;

    const DT: f32 = 1.0 / 60.0;

    fn run(
        a: &mut Arsenal,
        d: &Defs,
        input: &WeaponInput,
        rng: &mut Rng,
        secs: f32,
    ) -> Vec<WeaponEvent> {
        let mut ev = vec![];
        let n = (secs / DT).round() as usize;
        for _ in 0..n {
            ev.extend(a.tick(d, input, Vec3::X, rng, DT));
        }
        ev
    }

    fn fires(ev: &[WeaponEvent]) -> usize {
        ev.iter()
            .filter(|e| matches!(e, WeaponEvent::Fire { .. }))
            .count()
    }

    fn setup() -> (Defs, Arsenal, Rng) {
        let d = fixtures::defs();
        let mut a = Arsenal::new(&d);
        a.current = WeaponId::Pistol;
        (d, a, Rng::new(1))
    }

    fn held() -> WeaponInput {
        WeaponInput {
            fire: true,
            ..Default::default()
        }
    }

    #[test]
    fn new_arsenal_matches_shipped_defs() {
        let d = fixtures::defs();
        let a = Arsenal::new(&d);
        assert_eq!(a.owned, [true, true, false, false, false, false]);
        assert_eq!(a.clip[WeaponId::Pistol.index()], 12);
        assert_eq!(a.reserve, [36, 0, 0, 0]);
        assert_eq!(a.current, WeaponId::Pistol);
        assert_eq!(a.phase, WeaponPhase::Ready);
        assert_eq!(a.readout(&d), (Some(12), Some(36)));
    }

    #[test]
    fn pistol_fires_at_refire_rate() {
        let (d, mut a, mut rng) = setup();
        let refire = d.weapon(WeaponId::Pistol).refire;
        // Shots land at t = 0, refire, 2*refire, ... inside [0, 1] s.
        let expected = (1.0 / refire).ceil() as usize;
        let ev = run(&mut a, &d, &held(), &mut rng, 1.0);
        assert_eq!(fires(&ev), expected);
        assert_eq!(expected, 6, "shipped pistol: 0.18 s refire");
        assert_eq!(a.clip[WeaponId::Pistol.index()], 12 - expected as u32);
        // Rate is independent of dt: one big tick fires once, the carry is kept.
        let (d, mut a, mut rng) = setup();
        let mut n = 0;
        for _ in 0..10 {
            n += fires(&a.tick(&d, &held(), Vec3::X, &mut rng, 0.1));
        }
        assert_eq!(n, expected);
    }

    #[test]
    fn reload_pressed_during_cooldown_is_honoured() {
        let (d, mut a, mut rng) = setup();
        let pistol = WeaponId::Pistol.index();
        let ev = a.tick(&d, &held(), Vec3::X, &mut rng, DT);
        assert_eq!(fires(&ev), 1);
        let press = WeaponInput {
            reload: true,
            ..Default::default()
        };
        assert!(a.tick(&d, &press, Vec3::X, &mut rng, DT).is_empty());
        assert!(matches!(a.phase, WeaponPhase::Cooldown(_)));
        let ev = run(&mut a, &d, &WeaponInput::default(), &mut rng, 0.3);
        assert_eq!(
            ev.iter()
                .filter(|e| **e == WeaponEvent::ReloadStart)
                .count(),
            1
        );
        run(&mut a, &d, &WeaponInput::default(), &mut rng, 1.5);
        assert_eq!(a.clip[pistol], 12);
        // A stale press (full clip) never fires later; a switch also clears it.
        a.tick(&d, &press, Vec3::X, &mut rng, DT);
        assert!(!a.reload_queued);
        a.clip[pistol] = 5;
        a.phase = WeaponPhase::Cooldown(0.1);
        a.tick(&d, &press, Vec3::X, &mut rng, DT);
        let sel = WeaponInput {
            select: Some(WeaponId::Boot),
            ..Default::default()
        };
        a.tick(&d, &sel, Vec3::X, &mut rng, DT);
        assert!(!a.reload_queued);
    }

    #[test]
    fn clip_empties_then_auto_reloads() {
        let (d, mut a, mut rng) = setup();
        a.clip[WeaponId::Pistol.index()] = 1;
        let ev = run(&mut a, &d, &held(), &mut rng, 0.3);
        assert_eq!(fires(&ev), 1);
        assert!(ev.contains(&WeaponEvent::ReloadStart));
        assert_eq!(a.clip[WeaponId::Pistol.index()], 0);
        assert!(matches!(a.phase, WeaponPhase::Reloading(_)));
        // No dry fire and no shot while reloading.
        let ev = run(&mut a, &d, &held(), &mut rng, 0.5);
        assert!(ev.is_empty());
    }

    #[test]
    fn reload_moves_rounds_only_on_completion() {
        let (d, mut a, mut rng) = setup();
        a.clip[WeaponId::Pistol.index()] = 5;
        let input = WeaponInput {
            reload: true,
            ..Default::default()
        };
        let ev = a.tick(&d, &input, Vec3::X, &mut rng, DT);
        assert_eq!(ev, vec![WeaponEvent::ReloadStart]);
        let ev = run(&mut a, &d, &WeaponInput::default(), &mut rng, 1.0);
        assert!(ev.is_empty());
        assert_eq!(
            (
                a.clip[WeaponId::Pistol.index()],
                a.reserve[AmmoKind::Bullets.index()]
            ),
            (5, 36)
        );
        let ev = run(&mut a, &d, &WeaponInput::default(), &mut rng, 0.3);
        assert_eq!(ev, vec![WeaponEvent::ReloadDone]);
        assert_eq!(
            (
                a.clip[WeaponId::Pistol.index()],
                a.reserve[AmmoKind::Bullets.index()]
            ),
            (12, 29)
        );
        assert_eq!(a.phase, WeaponPhase::Ready);
        // A reload with a full clip, or with no reserve, does nothing.
        assert!(a.tick(&d, &input, Vec3::X, &mut rng, DT).is_empty());
        a.clip[WeaponId::Pistol.index()] = 3;
        a.reserve[AmmoKind::Bullets.index()] = 0;
        assert!(a.tick(&d, &input, Vec3::X, &mut rng, DT).is_empty());
        // Partial reload moves only what the reserve has.
        a.reserve[AmmoKind::Bullets.index()] = 4;
        a.tick(&d, &input, Vec3::X, &mut rng, DT);
        run(&mut a, &d, &WeaponInput::default(), &mut rng, 1.5);
        assert_eq!(
            (
                a.clip[WeaponId::Pistol.index()],
                a.reserve[AmmoKind::Bullets.index()]
            ),
            (7, 0)
        );
    }

    #[test]
    fn switch_cancels_reload_without_losing_ammo() {
        let (d, mut a, mut rng) = setup();
        a.give_weapon(&d, WeaponId::Shotgun);
        run(&mut a, &d, &WeaponInput::default(), &mut rng, 0.4);
        a.current = WeaponId::Pistol;
        a.phase = WeaponPhase::Ready;
        a.clip[WeaponId::Pistol.index()] = 5;
        let reload = WeaponInput {
            reload: true,
            ..Default::default()
        };
        a.tick(&d, &reload, Vec3::X, &mut rng, DT);
        assert!(matches!(a.phase, WeaponPhase::Reloading(_)));
        let sel = WeaponInput {
            select: Some(WeaponId::Shotgun),
            ..Default::default()
        };
        a.tick(&d, &sel, Vec3::X, &mut rng, DT);
        assert!(matches!(
            a.phase,
            WeaponPhase::Switching {
                to: WeaponId::Shotgun,
                ..
            }
        ));
        let ev = run(&mut a, &d, &WeaponInput::default(), &mut rng, 2.0);
        assert_eq!(ev, vec![WeaponEvent::Switched(WeaponId::Shotgun)]);
        assert_eq!(a.current, WeaponId::Shotgun);
        assert_eq!(
            (
                a.clip[WeaponId::Pistol.index()],
                a.reserve[AmmoKind::Bullets.index()]
            ),
            (5, 36)
        );
    }

    #[test]
    fn cycle_skips_unowned_and_empty() {
        let (d, mut a, mut rng) = setup();
        let next = WeaponInput {
            cycle: 1,
            ..Default::default()
        };
        let prev = WeaponInput {
            cycle: -1,
            ..Default::default()
        };
        let settle = |a: &mut Arsenal, rng: &mut Rng| {
            run(a, &d, &WeaponInput::default(), rng, 0.4);
        };
        // Shotgun unowned: pistol -> next wraps to boot.
        a.tick(&d, &next, Vec3::X, &mut rng, DT);
        settle(&mut a, &mut rng);
        assert_eq!(a.current, WeaponId::Boot);
        // Boot -> next is the pistol; previous from boot skips the unowned shotgun to the pistol.
        a.tick(&d, &prev, Vec3::X, &mut rng, DT);
        settle(&mut a, &mut rng);
        assert_eq!(a.current, WeaponId::Pistol);
        // Own the shotgun but with no shells: still skipped.
        a.owned[2] = true;
        a.tick(&d, &next, Vec3::X, &mut rng, DT);
        settle(&mut a, &mut rng);
        assert_eq!(a.current, WeaponId::Boot);
        // With shells it is reachable.
        a.reserve[AmmoKind::Shells.index()] = 5;
        a.tick(&d, &prev, Vec3::X, &mut rng, DT);
        settle(&mut a, &mut rng);
        assert_eq!(a.current, WeaponId::Shotgun);
        // Empty pistol is skipped, the boot never is.
        a.clip[WeaponId::Pistol.index()] = 0;
        a.reserve[AmmoKind::Bullets.index()] = 0;
        a.tick(&d, &prev, Vec3::X, &mut rng, DT);
        settle(&mut a, &mut rng);
        assert_eq!(a.current, WeaponId::Boot);
    }

    #[test]
    fn dry_fire_switches_to_boot() {
        let (d, mut a, mut rng) = setup();
        a.clip[WeaponId::Pistol.index()] = 0;
        a.reserve[AmmoKind::Bullets.index()] = 0;
        let ev = a.tick(&d, &held(), Vec3::X, &mut rng, DT);
        assert_eq!(ev, vec![WeaponEvent::DryFire]);
        assert!(matches!(
            a.phase,
            WeaponPhase::Switching {
                to: WeaponId::Boot,
                ..
            }
        ));
        let ev = run(&mut a, &d, &WeaponInput::default(), &mut rng, 0.4);
        assert_eq!(ev, vec![WeaponEvent::Switched(WeaponId::Boot)]);
        // With a loaded shotgun owned, that is preferred.
        let (d, mut a, mut rng) = setup();
        a.clip[WeaponId::Pistol.index()] = 0;
        a.reserve[AmmoKind::Bullets.index()] = 0;
        a.owned[2] = true;
        a.reserve[AmmoKind::Shells.index()] = 3;
        a.tick(&d, &held(), Vec3::X, &mut rng, DT);
        assert!(matches!(
            a.phase,
            WeaponPhase::Switching {
                to: WeaponId::Shotgun,
                ..
            }
        ));
    }

    #[test]
    fn shotgun_uses_reserve_and_seven_pellets() {
        let (d, mut a, mut rng) = setup();
        assert!(a.give_weapon(&d, WeaponId::Shotgun));
        run(&mut a, &d, &WeaponInput::default(), &mut rng, 0.4);
        assert_eq!(a.current, WeaponId::Shotgun);
        assert_eq!(a.readout(&d), (None, Some(10)));
        let ev = a.tick(&d, &held(), Vec3::X, &mut rng, DT);
        match &ev[..] {
            [WeaponEvent::Fire { weapon, dirs }] => {
                assert_eq!(*weapon, WeaponId::Shotgun);
                assert_eq!(dirs.len(), 7);
            }
            other => panic!("unexpected {other:?}"),
        }
        assert_eq!(a.reserve[AmmoKind::Shells.index()], 9);
        assert_eq!(a.clip[WeaponId::Shotgun.index()], 0);
        assert!(matches!(a.phase, WeaponPhase::Cooldown(_)));
    }

    #[test]
    fn pellets_stay_inside_cone_and_are_seeded() {
        let aim = Vec3::new(0.3, -0.5, 0.8).normalize();
        let mut r1 = Rng::new(42);
        let dirs = spread_dirs(aim, 500, 6.0, &mut r1);
        assert_eq!(dirs.len(), 500);
        let cos_max = 6.0f32.to_radians().cos();
        for v in &dirs {
            assert!((v.length() - 1.0).abs() < 1e-4);
            assert!(v.dot(aim) >= cos_max - 1e-4);
        }
        let mut r2 = Rng::new(42);
        assert_eq!(dirs, spread_dirs(aim, 500, 6.0, &mut r2));
        let mut r3 = Rng::new(43);
        assert_ne!(dirs, spread_dirs(aim, 500, 6.0, &mut r3));
        // Aim straight up exercises the basis fallback.
        for v in spread_dirs(Vec3::Z, 50, 6.0, &mut r1) {
            assert!(v.dot(Vec3::Z) >= cos_max - 1e-4);
        }
        // No spread, one pellet: exactly the aim.
        assert_eq!(spread_dirs(aim, 1, 0.0, &mut r1), vec![aim]);
    }

    #[test]
    fn kick_works_while_reloading() {
        let (d, mut a, mut rng) = setup();
        a.clip[WeaponId::Pistol.index()] = 5;
        let reload = WeaponInput {
            reload: true,
            ..Default::default()
        };
        a.tick(&d, &reload, Vec3::X, &mut rng, DT);
        let kick = WeaponInput {
            kick: true,
            ..Default::default()
        };
        let ev = a.tick(&d, &kick, Vec3::X, &mut rng, DT);
        assert_eq!(ev, vec![WeaponEvent::Kick { dir: Vec3::X }]);
        assert!(matches!(a.phase, WeaponPhase::Reloading(_)));
        // On cooldown now.
        assert!(a.tick(&d, &kick, Vec3::X, &mut rng, DT).is_empty());
        let ev = run(&mut a, &d, &kick, &mut rng, 0.7);
        assert_eq!(
            ev.iter()
                .filter(|e| matches!(e, WeaponEvent::Kick { .. }))
                .count(),
            1
        );
        // Kick also works while switching.
        a.phase = WeaponPhase::Switching {
            to: WeaponId::Boot,
            left: 0.3,
        };
        a.kick_cooldown = 0.0;
        assert_eq!(a.tick(&d, &kick, Vec3::X, &mut rng, DT).len(), 1);
    }

    #[test]
    fn boot_fire_emits_fire_event() {
        let (d, mut a, mut rng) = setup();
        a.current = WeaponId::Boot;
        let ev = a.tick(&d, &held(), Vec3::Y, &mut rng, DT);
        assert_eq!(
            ev,
            vec![WeaponEvent::Fire {
                weapon: WeaponId::Boot,
                dirs: vec![Vec3::Y]
            }]
        );
    }

    #[test]
    fn give_weapon_switches_once_and_caps_ammo() {
        let (d, mut a, mut rng) = setup();
        assert!(a.give_weapon(&d, WeaponId::Shotgun));
        assert!(a.owned[2]);
        assert_eq!(a.reserve[AmmoKind::Shells.index()], 10);
        let ph = a.phase;
        assert!(matches!(
            ph,
            WeaponPhase::Switching {
                to: WeaponId::Shotgun,
                ..
            }
        ));
        // Picking up another shotgun mid-switch adds ammo but does not restart the switch.
        assert!(a.give_weapon(&d, WeaponId::Shotgun));
        assert_eq!(a.reserve[AmmoKind::Shells.index()], 20);
        assert_eq!(a.phase, ph);
        run(&mut a, &d, &WeaponInput::default(), &mut rng, 0.4);
        assert_eq!(a.current, WeaponId::Shotgun);
        // Already owned: ammo only, no switch.
        a.current = WeaponId::Pistol;
        assert!(a.give_weapon(&d, WeaponId::Shotgun));
        assert_eq!(a.phase, WeaponPhase::Ready);
        // Cap at max, then nothing changes.
        for _ in 0..10 {
            a.give_weapon(&d, WeaponId::Shotgun);
        }
        assert_eq!(a.reserve[AmmoKind::Shells.index()], 50);
        assert!(!a.give_weapon(&d, WeaponId::Shotgun));
        assert!(!a.give_ammo(&d, AmmoKind::Shells, 8));
        assert!(a.give_ammo(&d, AmmoKind::Bullets, 12));
        assert_eq!(a.reserve[AmmoKind::Bullets.index()], 48);
        a.reserve[AmmoKind::Bullets.index()] = 195;
        assert!(a.give_ammo(&d, AmmoKind::Bullets, 12));
        assert_eq!(a.reserve[AmmoKind::Bullets.index()], 200);
    }

    #[test]
    fn select_unowned_is_ignored() {
        let (d, mut a, mut rng) = setup();
        let before = a.clone();
        for w in [WeaponId::Shotgun, WeaponId::Pistol] {
            let sel = WeaponInput {
                select: Some(w),
                ..Default::default()
            };
            assert!(a.tick(&d, &sel, Vec3::X, &mut rng, DT).is_empty());
            assert_eq!(a, before);
        }
    }

    #[test]
    fn best_armed_uses_priority() {
        let (mut d, mut a, _) = setup();
        a.give_weapon(&d, WeaponId::Shotgun);
        a.give_weapon(&d, WeaponId::Chaingun);
        a.give_weapon(&d, WeaponId::Rockets);
        a.give_weapon(&d, WeaponId::PipeBombs);
        a.reserve[AmmoKind::Rockets.index()] = 5;
        a.reserve[AmmoKind::Bombs.index()] = 3;
        // Chaingun (4) outranks shotgun (3); the priority-0 splash weapons are never picked.
        assert_eq!(a.best_armed(&d), WeaponId::Chaingun);
        // Priorities come from data.
        for def in &mut d.weapons.weapons {
            if def.id == WeaponId::Pistol {
                def.priority = 9;
            }
        }
        assert_eq!(a.best_armed(&d), WeaponId::Pistol);
        // Ties go to the lower slot.
        for def in &mut d.weapons.weapons {
            if def.id == WeaponId::Shotgun {
                def.priority = 9;
            }
        }
        assert_eq!(a.best_armed(&d), WeaponId::Pistol);
        // Only splash weapons armed: fall back to the boot.
        let mut only = Arsenal::new(&d);
        only.owned = [false; WeaponId::ALL.len()];
        only.owned[WeaponId::Rockets.index()] = true;
        only.reserve[AmmoKind::Rockets.index()] = 5;
        assert_eq!(only.best_armed(&d), WeaponId::Boot);
    }

    #[test]
    fn cycle_covers_six_slots() {
        let (d, mut a, _) = setup();
        for w in WeaponId::ALL {
            a.owned[w.index()] = true;
        }
        for k in AmmoKind::ALL {
            a.reserve[k.index()] = 5;
        }
        assert_eq!(WeaponId::ALL.len(), 6);
        let mut seen = vec![a.current];
        for _ in 0..5 {
            let next = a.cycle_target(&d, 1).expect("next weapon");
            a.current = next;
            seen.push(next);
        }
        seen.sort_by_key(|w| w.index());
        assert_eq!(seen, WeaponId::ALL.to_vec());
        assert_eq!(
            a.cycle_target(&d, 1),
            Some(WeaponId::ALL[(a.current.index() + 1) % 6])
        );
        assert_eq!(
            a.cycle_target(&d, -1),
            Some(WeaponId::ALL[(a.current.index() + 5) % 6])
        );
    }
    fn armed(w: WeaponId, bullets: u32, rockets: u32, bombs: u32) -> (Defs, Arsenal, Rng) {
        let d = fixtures::defs();
        let mut a = Arsenal::new(&d);
        a.owned[w.index()] = true;
        a.current = w;
        a.reserve[AmmoKind::Bullets.index()] = bullets;
        a.reserve[AmmoKind::Rockets.index()] = rockets;
        a.reserve[AmmoKind::Bombs.index()] = bombs;
        (d, a, Rng::new(1))
    }

    fn launches(ev: &[WeaponEvent]) -> usize {
        ev.iter()
            .filter(|e| matches!(e, WeaponEvent::Launch { .. }))
            .count()
    }

    #[test]
    fn chaingun_fires_at_refire_rate() {
        let (d, mut a, mut rng) = armed(WeaponId::Chaingun, 100, 0, 0);
        let ev = run(&mut a, &d, &held(), &mut rng, 1.0);
        let expected = (1.0 / d.weapon(WeaponId::Chaingun).refire).ceil() as usize;
        assert_eq!(expected, 15, "shipped chaingun: 0.07 s refire");
        assert_eq!(fires(&ev), expected);
        assert_eq!(a.reserve[AmmoKind::Bullets.index()], 100 - expected as u32);
    }

    #[test]
    fn rocket_launch_consumes_one() {
        let (d, mut a, mut rng) = armed(WeaponId::Rockets, 0, 5, 0);
        let ev = a.tick(&d, &held(), Vec3::X, &mut rng, DT);
        assert_eq!(
            ev,
            vec![WeaponEvent::Launch {
                weapon: WeaponId::Rockets,
                dir: Vec3::X
            }]
        );
        assert_eq!(a.reserve[AmmoKind::Rockets.index()], 4);
        assert!(matches!(a.phase, WeaponPhase::Cooldown(_)));
        // Refire 0.8 s: held for 1 s launches twice in total.
        let ev = run(&mut a, &d, &held(), &mut rng, 1.0);
        assert_eq!(launches(&ev), 1);
    }

    #[test]
    fn fire_detonates_live_bombs() {
        let (d, mut a, mut rng) = armed(WeaponId::PipeBombs, 0, 0, 3);
        a.live_bombs = 2;
        let press = WeaponInput {
            fire: true,
            fire_pressed: true,
            ..Default::default()
        };
        let ev = a.tick(&d, &press, Vec3::X, &mut rng, DT);
        assert_eq!(ev, vec![WeaponEvent::Detonate]);
        assert_eq!(a.reserve[AmmoKind::Bombs.index()], 3, "no ammo spent");
        // A held button without a fresh press does not detonate again.
        a.live_bombs = 2;
        let ev = run(&mut a, &d, &held(), &mut rng, 1.0);
        assert!(!ev.contains(&WeaponEvent::Detonate));
    }

    #[test]
    fn fire_throws_when_none_live() {
        let (d, mut a, mut rng) = armed(WeaponId::PipeBombs, 0, 0, 3);
        let press = WeaponInput {
            fire: true,
            fire_pressed: true,
            ..Default::default()
        };
        let ev = a.tick(&d, &press, Vec3::X, &mut rng, DT);
        assert_eq!(
            ev,
            vec![WeaponEvent::Launch {
                weapon: WeaponId::PipeBombs,
                dir: Vec3::X
            }]
        );
        assert_eq!(a.reserve[AmmoKind::Bombs.index()], 2);
        // Live bombs but a mere hold: throws (no edge), consuming ammo.
        let (d, mut a, mut rng) = armed(WeaponId::PipeBombs, 0, 0, 3);
        a.live_bombs = 1;
        let ev = a.tick(&d, &held(), Vec3::X, &mut rng, DT);
        assert_eq!(launches(&ev), 1);
    }

    #[test]
    fn out_of_bombs_switches_away() {
        let (d, mut a, mut rng) = armed(WeaponId::PipeBombs, 0, 0, 0);
        a.owned[WeaponId::Pistol.index()] = true;
        a.reserve[AmmoKind::Bullets.index()] = 10;
        a.tick(&d, &WeaponInput::default(), Vec3::X, &mut rng, DT);
        assert!(matches!(
            a.phase,
            WeaponPhase::Switching {
                to: WeaponId::Pistol,
                ..
            }
        ));
        // With bombs still live and no ammo the player stays to detonate.
        let (d, mut a, mut rng) = armed(WeaponId::PipeBombs, 10, 0, 0);
        a.live_bombs = 1;
        let ev = run(&mut a, &d, &held(), &mut rng, 0.5);
        assert!(ev.is_empty());
        assert_eq!(a.phase, WeaponPhase::Ready);
        let press = WeaponInput {
            fire: true,
            fire_pressed: true,
            ..Default::default()
        };
        assert_eq!(
            a.tick(&d, &press, Vec3::X, &mut rng, DT),
            vec![WeaponEvent::Detonate]
        );
    }
}
