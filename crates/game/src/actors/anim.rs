//! Enemy animation: which glTF clip an actor plays for its AI state ([`anim_for`], pure and
//! unit-tested) and the thin system that drives each model's `AnimationPlayer` from it.

use super::{ActorModel, ActorVisual};
use crate::combat::{FxQueue, GameDefs, LevelCombat};
use crate::models::{ClipRole, ModelLibrary, ModelReady};
use bevy::animation::RepeatAnimation;
use bevy::prelude::*;
use rr_core::actors::AiState;
use rr_core::combat::CombatEvent;
use std::time::Duration;

/// Cross-fade between two clips.
const FADE: Duration = Duration::from_millis(150);
/// Idle playback speed while asleep (a slow, drowsy sway).
const SLEEP_SPEED: f32 = 0.3;
/// Fraction of the enemy's top speed from which it runs instead of walking.
const RUN_FRAC: f32 = 0.6;
/// Death clip playback speed bounds.
const DEATH_SPEED: (f32, f32) = (0.5, 3.0);

/// What an enemy model should play.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AnimCmd {
    pub role: ClipRole,
    pub speed: f32,
    pub repeat: bool,
    /// Show the clip's last frame, paused (a corpse).
    pub hold_end: bool,
}

/// The clip for an AI state. `speed_frac` is the actor's ground speed over its top speed,
/// `death_time` the def's dying time and `clip_secs` the Death clip's length: dying plays the
/// clip once, stretched to roughly fill `death_time`.
pub fn anim_for(state: &AiState, speed_frac: f32, death_time: f32, clip_secs: f32) -> AnimCmd {
    let looped = |role, speed| AnimCmd {
        role,
        speed,
        repeat: true,
        hold_end: false,
    };
    let once = |role, speed| AnimCmd {
        role,
        speed,
        repeat: false,
        hold_end: false,
    };
    match state {
        AiState::Sleep => looped(ClipRole::Idle, SLEEP_SPEED),
        AiState::Alert { .. } | AiState::Chase if speed_frac < RUN_FRAC => {
            looped(ClipRole::Walk, 1.0)
        }
        AiState::Alert { .. } | AiState::Chase => looped(ClipRole::Run, 1.0),
        AiState::Attack { .. } => looped(ClipRole::Attack, 1.0),
        AiState::Pain { .. } => once(ClipRole::Pain, 1.0),
        AiState::Dying { .. } => {
            let speed = if death_time > 0.0 {
                clip_secs / death_time
            } else {
                DEATH_SPEED.1
            };
            once(ClipRole::Death, speed.clamp(DEATH_SPEED.0, DEATH_SPEED.1))
        }
        AiState::Dead => AnimCmd {
            hold_end: true,
            ..once(ClipRole::Death, 1.0)
        },
    }
}

/// The role a model actually plays for `want`, given which roles it `has` and what it plays
/// now. A missing Pain keeps the current clip; Walk and Run stand in for each other; anything
/// else falls back to Idle. `None` if the model has nothing suitable (e.g. no clips at all).
pub fn pick_role(
    want: ClipRole,
    has: impl Fn(ClipRole) -> bool,
    current: Option<ClipRole>,
) -> Option<ClipRole> {
    if has(want) {
        return Some(want);
    }
    let stand_in = match want {
        ClipRole::Pain => current,
        ClipRole::Walk => Some(ClipRole::Run),
        ClipRole::Run => Some(ClipRole::Walk),
        _ => None,
    };
    stand_in
        .filter(|r| has(*r))
        .or_else(|| has(ClipRole::Idle).then_some(ClipRole::Idle))
}

/// The clip an actor's model is playing (on its [`ActorVisual`] root).
#[derive(Component, Default)]
pub(super) struct EnemyAnim {
    role: Option<ClipRole>,
    hold: bool,
}

/// Plays each ready enemy model's clip for its AI state: cross-fades when the role changes and
/// restarts the attack clip on every shot. An [`FxReaders`](crate::combat::FxReaders) system
/// (reads `ActorFired`).
#[allow(clippy::too_many_arguments)]
pub(super) fn animate_enemies(
    fx: Res<FxQueue>,
    combat: Res<LevelCombat>,
    defs: Res<GameDefs>,
    lib: Res<ModelLibrary>,
    clips: Option<Res<Assets<AnimationClip>>>,
    mut roots: Query<(&ActorVisual, &ActorModel, &mut EnemyAnim)>,
    ready: Query<&ModelReady>,
    mut players: Query<(&mut AnimationPlayer, &mut AnimationTransitions)>,
) {
    for (g, model, mut anim) in &mut roots {
        let (Some(actor), Ok(ready)) = (combat.0.actors.get(g.0), ready.get(model.root)) else {
            continue;
        };
        let Some(Ok((mut player, mut transitions))) = ready.player.map(|p| players.get_mut(p))
        else {
            continue;
        };
        let Some(graph) = &lib.enemy(model.kind).graph else {
            continue;
        };
        let secs = |role| {
            graph
                .clips
                .get(&role)
                .and_then(|h| clips.as_ref()?.get(h))
                .map(|c| c.duration())
        };
        let def = defs.0.enemy(actor.kind);
        let speed_frac = if def.speed > 0.0 {
            actor.body.vel.truncate().length() / def.speed
        } else {
            0.0
        };
        let death_secs = secs(ClipRole::Death).unwrap_or(def.death_time);
        let cmd = anim_for(&actor.state, speed_frac, def.death_time, death_secs);
        let Some(role) = pick_role(cmd.role, |r| graph.nodes.contains_key(&r), anim.role) else {
            continue;
        };
        let node = graph.nodes[&role];
        let started = anim.role != Some(role);
        if started || anim.hold != cmd.hold_end {
            // A stand-in for a missing clip keeps that clip's own playback settings.
            let own = role == cmd.role;
            let active = if started {
                Some(transitions.play(&mut player, node, FADE))
            } else {
                player.animation_mut(node)
            };
            if let Some(active) = active {
                if own || started {
                    let repeat = if own && !cmd.repeat {
                        RepeatAnimation::Never
                    } else {
                        RepeatAnimation::Forever
                    };
                    active.set_repeat(repeat);
                    // Only on (re)start: a hold flip must not reset a clamped clip's speed.
                    if started {
                        active.set_speed(if own { cmd.speed } else { 1.0 });
                    }
                }
                // A corpse seen for the first time (e.g. dead when the model became ready)
                // jumps to the end; one that just finished dying already holds there.
                if own && cmd.hold_end && started {
                    active.seek_to(secs(role).unwrap_or(0.0)).pause();
                }
            }
            anim.role = Some(role);
            anim.hold = cmd.hold_end;
        }
        if role == ClipRole::Attack
            && fx
                .combat
                .iter()
                .any(|ev| matches!(ev, CombatEvent::ActorFired { actor } if *actor == g.0))
            && let Some(active) = player.animation_mut(node)
        {
            active.replay();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ClipRole::*;

    const LIVING: [AiState; 6] = [
        AiState::Sleep,
        AiState::Alert { t: 0.1 },
        AiState::Chase,
        AiState::Attack { t: 0.2, left: 2 },
        AiState::Pain { t: 0.1 },
        AiState::Dying { t: 0.4 },
    ];

    #[test]
    fn sleep_idles_slowly() {
        let c = anim_for(&AiState::Sleep, 0.0, 0.8, 1.2);
        assert_eq!(
            (c.role, c.speed, c.repeat, c.hold_end),
            (Idle, 0.3, true, false)
        );
    }

    #[test]
    fn moving_walks_then_runs() {
        for s in [AiState::Alert { t: 0.0 }, AiState::Chase] {
            assert_eq!(anim_for(&s, 0.0, 0.8, 1.0).role, Walk);
            assert_eq!(anim_for(&s, 0.59, 0.8, 1.0).role, Walk);
            assert_eq!(anim_for(&s, 0.6, 0.8, 1.0).role, Run);
            assert_eq!(anim_for(&s, 1.5, 0.8, 1.0).role, Run);
            assert!(anim_for(&s, 1.0, 0.8, 1.0).repeat);
        }
    }

    #[test]
    fn attack_loops_and_pain_plays_once() {
        let a = anim_for(&AiState::Attack { t: 0.1, left: 1 }, 0.0, 0.8, 1.0);
        assert_eq!((a.role, a.speed, a.repeat), (Attack, 1.0, true));
        let p = anim_for(&AiState::Pain { t: 0.1 }, 0.0, 0.8, 1.0);
        assert_eq!((p.role, p.repeat, p.hold_end), (Pain, false, false));
    }

    #[test]
    fn dying_stretches_the_death_clip_within_bounds() {
        let d = |clip, death| anim_for(&AiState::Dying { t: 0.3 }, 0.0, death, clip);
        let c = d(1.6, 0.8);
        assert_eq!((c.role, c.repeat, c.hold_end), (Death, false, false));
        assert!((c.speed - 2.0).abs() < 1e-6);
        assert_eq!(d(10.0, 0.8).speed, 3.0, "clamped fast");
        assert_eq!(d(0.1, 0.8).speed, 0.5, "clamped slow");
        assert_eq!(d(1.0, 0.0).speed, 3.0, "no death time: as fast as allowed");
    }

    #[test]
    fn dead_holds_the_last_death_frame() {
        let c = anim_for(&AiState::Dead, 0.0, 0.8, 1.0);
        assert_eq!((c.role, c.repeat, c.hold_end), (Death, false, true));
    }

    #[test]
    fn only_dead_holds_and_only_sleep_is_slow() {
        for s in LIVING {
            let c = anim_for(&s, 0.3, 0.8, 0.8);
            assert!(!c.hold_end, "{s:?}");
            assert_eq!(c.speed < 1.0, s == AiState::Sleep, "{s:?}");
        }
    }

    #[test]
    fn pick_role_falls_back_sensibly() {
        let all = |_| true;
        let no_pain = |r| r != Pain;
        let idle_only = |r| r == Idle;
        let walk_only = |r| r == Walk || r == Idle;
        let run_only = |r| r == Run || r == Idle;
        assert_eq!(pick_role(Pain, all, Some(Walk)), Some(Pain));
        assert_eq!(
            pick_role(Pain, no_pain, Some(Attack)),
            Some(Attack),
            "keeps the clip"
        );
        assert_eq!(pick_role(Pain, no_pain, None), Some(Idle));
        assert_eq!(pick_role(Run, walk_only, None), Some(Walk));
        assert_eq!(pick_role(Walk, run_only, None), Some(Run));
        assert_eq!(pick_role(Attack, idle_only, Some(Idle)), Some(Idle));
        assert_eq!(pick_role(Death, idle_only, None), Some(Idle));
        assert_eq!(pick_role(Idle, |_| false, None), None, "no clips at all");
    }
}
