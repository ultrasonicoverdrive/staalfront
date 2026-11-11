//! When an AI thinks, and which manager runs in that think.
//!
//! An AI thinks once every `think_interval` ticks (10 in the shipped profile, half a second), and two
//! players never think on the same tick: player `p` thinks when `(tick + stagger * p) % interval == 0`
//! (the profile check guarantees that `interval / gcd(interval, stagger)` is at least the number of players
//! a match can have). Each think runs the observation, the memory update and **one** manager, in rotation,
//! so every manager runs once in four thinks (every 40 ticks). Nothing runs on the other ticks.

use sf_sim::{PlayerId, Tick};

/// The managers of the AI, in rotation. The numbers index `AiState::manager_runs` and are hashed by the
/// digest, so they are permanent.
#[derive(Copy, Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Manager {
    /// Harvesters, silos, power (WP9.1b).
    Economy = 0,
    /// The build order and placement (WP9.1b).
    Construction = 1,
    /// Unit production (WP9.1c).
    Production = 2,
    /// Gathering, attacking, defending (WP9.1c).
    Army = 3,
}

impl Manager {
    /// Every manager, in the order of the rotation.
    pub const ALL: [Manager; 4] = [
        Manager::Economy,
        Manager::Construction,
        Manager::Production,
        Manager::Army,
    ];

    /// The manager that runs in the `n`-th think (counted from zero).
    pub fn of_think(n: u64) -> Manager {
        Manager::ALL[(n % Manager::ALL.len() as u64) as usize]
    }
}

/// Whether `player` thinks at `tick` (the number of ticks the world has run so far, which is the tick the
/// next step will run).
pub fn due(tick: Tick, player: PlayerId, interval: u32, stagger: u32) -> bool {
    (tick.0 + u64::from(stagger) * u64::from(player.0)) % u64::from(interval) == 0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn one_think_in_ten_ticks() {
        let ticks: Vec<u64> = (0..100)
            .filter(|&t| due(Tick(t), PlayerId(0), 10, 3))
            .collect();
        assert_eq!(ticks, vec![0, 10, 20, 30, 40, 50, 60, 70, 80, 90]);
    }

    #[test]
    fn the_players_are_staggered_by_three_ticks() {
        for p in 0..4u8 {
            let first = (0..10).find(|&t| due(Tick(t), PlayerId(p), 10, 3)).unwrap();
            // (tick + 3 p) % 10 == 0: player 0 at 0, player 1 at 7, player 2 at 4, player 3 at 1 (mod 10).
            assert_eq!(first, [0, 7, 4, 1][usize::from(p)], "player {p}");
        }
    }

    #[test]
    fn no_two_of_eight_players_think_on_the_same_tick() {
        for t in 0..200u64 {
            let thinking = (0..8u8)
                .filter(|&p| due(Tick(t), PlayerId(p), 10, 3))
                .count();
            assert!(thinking <= 1, "tick {t}: {thinking} players think");
        }
    }

    #[test]
    fn the_managers_come_round_in_order() {
        let got: Vec<Manager> = (0..9).map(Manager::of_think).collect();
        let m = Manager::ALL;
        assert_eq!(
            got,
            vec![m[0], m[1], m[2], m[3], m[0], m[1], m[2], m[3], m[0]]
        );
        // The numbers are hashed and indexed: pinned as literals.
        assert_eq!(
            [
                Manager::Economy as usize,
                Manager::Construction as usize,
                Manager::Production as usize,
                Manager::Army as usize
            ],
            [0, 1, 2, 3]
        );
    }
}
