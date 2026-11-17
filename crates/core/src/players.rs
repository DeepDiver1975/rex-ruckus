//! Player identity and the combat body layout.
//!
//! Combat keeps every body in one slice, players first: `[player 0, .., player n-1, actor 0,
//! actor 1, ..]`. The helpers here are the only place that knows the layout, so the rest of the
//! core never does index arithmetic on body indices itself.

use crate::projectile::Shooter;

/// One player in the game, 0-based (`PlayerId(0)` is the local or host player).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Default)]
pub struct PlayerId(pub u8);

/// The most players one game holds.
pub const MAX_PLAYERS: usize = 4;

/// Body index of actor `i` in `[players.., actors..]`.
pub fn actor_body(players: usize, i: usize) -> usize {
    players + i
}

/// What a body index is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BodyRef {
    Player(PlayerId),
    /// Index into the actor list.
    Actor(usize),
}

/// What body index `b` is in `[players.., actors..]`.
pub fn body_ref(players: usize, b: usize) -> BodyRef {
    if b < players {
        BodyRef::Player(PlayerId(b as u8))
    } else {
        BodyRef::Actor(b - players)
    }
}

/// The body index of a shooter, if it has one (`World` has none).
pub fn shooter_body(players: usize, s: Shooter) -> Option<usize> {
    match s {
        Shooter::Player(p) => {
            debug_assert!((p.0 as usize) < players, "{p:?} with {players} players");
            Some(p.0 as usize)
        }
        Shooter::Actor(a) => Some(actor_body(players, a)),
        Shooter::World => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn actor_bodies_follow_the_players() {
        assert_eq!(actor_body(2, 0), 2);
        assert_eq!(actor_body(1, 3), 4);
    }

    #[test]
    fn body_ref_splits_players_from_actors() {
        assert_eq!(body_ref(2, 1), BodyRef::Player(PlayerId(1)));
        assert_eq!(body_ref(2, 3), BodyRef::Actor(1));
        assert_eq!(body_ref(1, 0), BodyRef::Player(PlayerId(0)));
        assert_eq!(body_ref(1, 1), BodyRef::Actor(0));
    }

    #[test]
    fn shooter_body_maps_through_the_layout() {
        assert_eq!(shooter_body(2, Shooter::Actor(0)), Some(2));
        assert_eq!(shooter_body(2, Shooter::Player(PlayerId(1))), Some(1));
        assert_eq!(shooter_body(1, Shooter::World), None);
    }
}
