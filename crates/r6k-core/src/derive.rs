//! Pure derived statistics over the full match model: multi-kills, clutches, KOST,
//! first kills/deaths, survival. No IO.
//!
//! Known limit: only `Kill` events are on the kill feed. A player who died without a killer
//! (fall damage, suicide) is not removed from the alive set, so a clutch right after such a
//! death can be missed or its opponent count can be too high. Those deaths are rare (7 in 168
//! real rounds).

use crate::full::{FullMatch, FullRound, KillEvent};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// A trade must happen within this many seconds of the death.
pub const TRADE_WINDOW_S: i64 = 10;

/// Rounds with exactly 2, 3, 4 and 5 or more kills.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct MultiKills {
    pub k2: u32,
    pub k3: u32,
    pub k4: u32,
    pub k5: u32,
}

impl MultiKills {
    pub fn add_round(&mut self, kills: u32) {
        match kills {
            0 | 1 => {}
            2 => self.k2 += 1,
            3 => self.k3 += 1,
            4 => self.k4 += 1,
            _ => self.k5 += 1,
        }
    }
    pub fn total(&self) -> u32 {
        self.k2 + self.k3 + self.k4 + self.k5
    }
}

/// A 1vX situation: the player was the last one alive in their team with `vs` opponents alive.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Clutch {
    pub round: u32,
    pub profile_id: String,
    pub vs: u32,
    /// The player's team won the round.
    pub won: bool,
}

/// Clutch situations in one round (at most one per team). Only teams that started with at
/// least two players can clutch.
pub fn round_clutches(round: &FullRound) -> Vec<Clutch> {
    let team_of: HashMap<&str, usize> = round
        .players
        .iter()
        .map(|p| (p.profile_id.as_str(), p.team_index))
        .collect();
    let mut alive = [0u32; 2];
    let mut size = [0u32; 2];
    for p in &round.players {
        if p.team_index < 2 {
            alive[p.team_index] += 1;
            size[p.team_index] += 1;
        }
    }
    let mut dead: Vec<&str> = Vec::new();
    let mut done = [false; 2];
    let mut out = Vec::new();
    for k in &round.kills {
        let Some(&vt) = team_of.get(k.victim.as_str()) else {
            continue;
        };
        if vt >= 2 || dead.contains(&k.victim.as_str()) {
            continue;
        }
        dead.push(k.victim.as_str());
        alive[vt] = alive[vt].saturating_sub(1);
        for t in 0..2 {
            let enemies = alive[1 - t];
            if !done[t] && size[t] >= 2 && alive[t] == 1 && enemies >= 1 {
                done[t] = true;
                let last = round
                    .players
                    .iter()
                    .find(|p| p.team_index == t && !dead.contains(&p.profile_id.as_str()));
                if let Some(p) = last {
                    out.push(Clutch {
                        round: round.number,
                        profile_id: p.profile_id.clone(),
                        vs: enemies,
                        won: round.winner_team == Some(t),
                    });
                }
            }
        }
    }
    out
}

/// Was `victim`'s death traded: a teammate killed the killer within [`TRADE_WINDOW_S`] after it?
pub fn is_traded(round: &FullRound, victim: &str) -> bool {
    let team_of: HashMap<&str, usize> = round
        .players
        .iter()
        .map(|p| (p.profile_id.as_str(), p.team_index))
        .collect();
    let Some(death) = round.kills.iter().find(|k| k.victim == victim) else {
        return false;
    };
    let Some(&vteam) = team_of.get(victim) else {
        return false;
    };
    round.kills.iter().any(|k: &KillEvent| {
        k.victim == death.killer
            && k.killer != victim
            && team_of.get(k.killer.as_str()) == Some(&vteam)
            // the clock counts down: a later event has a smaller time_s
            && (0..=TRADE_WINDOW_S).contains(&(death.time_s - k.time_s))
    })
}

/// Per-player totals for one match (all derived from the full model).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlayerMatchStats {
    pub profile_id: String,
    pub username: String,
    pub team_index: usize,
    /// Rounds the player took part in.
    pub rounds: u32,
    pub kills: u32,
    pub deaths: u32,
    pub headshots: u32,
    pub first_kills: u32,
    pub first_deaths: u32,
    pub multi_kills: MultiKills,
    pub clutches_won: u32,
    pub clutches_lost: u32,
    pub clutches: Vec<Clutch>,
    pub survived: u32,
    pub traded_deaths: u32,
    /// Rounds with a kill, survival or a traded death.
    pub kost_rounds: u32,
}

impl PlayerMatchStats {
    pub fn headshot_pct(&self) -> f64 {
        if self.kills == 0 {
            0.0
        } else {
            100.0 * self.headshots as f64 / self.kills as f64
        }
    }
    pub fn kd(&self) -> f64 {
        if self.deaths == 0 {
            self.kills as f64
        } else {
            self.kills as f64 / self.deaths as f64
        }
    }
    pub fn kost_pct(&self) -> f64 {
        if self.rounds == 0 {
            0.0
        } else {
            100.0 * self.kost_rounds as f64 / self.rounds as f64
        }
    }
    pub fn survival_pct(&self) -> f64 {
        if self.rounds == 0 {
            0.0
        } else {
            100.0 * self.survived as f64 / self.rounds as f64
        }
    }
}

/// Stats for every player of the match: the tracked player's team first, then by kills
/// (descending), ties by username.
pub fn player_match_stats(m: &FullMatch) -> Vec<PlayerMatchStats> {
    let mut map: HashMap<String, PlayerMatchStats> = m
        .players
        .iter()
        .map(|p| {
            (
                p.profile_id.clone(),
                PlayerMatchStats {
                    profile_id: p.profile_id.clone(),
                    username: p.username.clone(),
                    team_index: p.team_index,
                    rounds: 0,
                    kills: 0,
                    deaths: 0,
                    headshots: 0,
                    first_kills: 0,
                    first_deaths: 0,
                    multi_kills: MultiKills::default(),
                    clutches_won: 0,
                    clutches_lost: 0,
                    clutches: Vec::new(),
                    survived: 0,
                    traded_deaths: 0,
                    kost_rounds: 0,
                },
            )
        })
        .collect();

    for r in &m.rounds {
        for p in &r.players {
            let Some(s) = map.get_mut(&p.profile_id) else {
                continue;
            };
            s.rounds += 1;
            s.kills += p.kills;
            s.deaths += p.deaths;
            s.headshots += p.headshots;
            s.first_kills += p.first_kill as u32;
            s.first_deaths += p.first_death as u32;
            s.multi_kills.add_round(p.kills);
            let traded = p.deaths > 0 && is_traded(r, &p.profile_id);
            s.survived += p.survived as u32;
            s.traded_deaths += traded as u32;
            if p.kills > 0 || p.survived || traded {
                s.kost_rounds += 1;
            }
        }
        for c in round_clutches(r) {
            if let Some(s) = map.get_mut(&c.profile_id) {
                if c.won {
                    s.clutches_won += 1;
                } else {
                    s.clutches_lost += 1;
                }
                s.clutches.push(c);
            }
        }
    }

    let mine = m.my_team_index();
    let mut v: Vec<PlayerMatchStats> = map.into_values().collect();
    v.sort_by(|a, b| {
        ((Some(a.team_index) != mine) as u8)
            .cmp(&((Some(b.team_index) != mine) as u8))
            .then(a.team_index.cmp(&b.team_index))
            .then(b.kills.cmp(&a.kills))
            .then(a.username.cmp(&b.username))
    });
    v
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::full::{PlayerInfo, RoundPlayer, WinCondition};
    use crate::model::Side;

    /// Players "a1".."a5" are team 0, "b1".."b5" team 1. `dead` lists who died.
    fn round(number: u32, winner: usize, kills: &[(&str, &str, i64)]) -> FullRound {
        let mut players = Vec::new();
        for (t, prefix) in ["a", "b"].iter().enumerate() {
            for i in 1..=5 {
                let id = format!("{prefix}{i}");
                let n = kills.iter().filter(|k| k.0 == id).count() as u32;
                let died = kills.iter().any(|k| k.1 == id);
                players.push(RoundPlayer {
                    profile_id: id.clone(),
                    team_index: t,
                    operator_id: 1,
                    side: if t == 0 { Side::Attack } else { Side::Defense },
                    kills: n,
                    deaths: died as u32,
                    headshots: 0,
                    first_kill: false,
                    first_death: false,
                    survived: !died,
                });
            }
        }
        FullRound {
            number,
            winner_team: Some(winner),
            win_condition: WinCondition::Elimination,
            site: String::new(),
            my_side: Side::Attack,
            players,
            kills: kills
                .iter()
                .map(|k| KillEvent {
                    time_s: k.2,
                    killer: k.0.into(),
                    victim: k.1.into(),
                    headshot: false,
                })
                .collect(),
        }
    }

    fn full(rounds: Vec<FullRound>) -> FullMatch {
        let mut players = Vec::new();
        for (t, prefix) in ["a", "b"].iter().enumerate() {
            for i in 1..=5 {
                players.push(PlayerInfo {
                    profile_id: format!("{prefix}{i}"),
                    username: format!("{prefix}{i}"),
                    team_index: t,
                });
            }
        }
        FullMatch {
            match_id: "m".into(),
            timestamp: "2026-01-01T00:00:00Z".into(),
            match_type: "Ranked".into(),
            gamemode: "Bomb".into(),
            map_id: 1,
            score_us: 0,
            score_them: 0,
            won: false,
            me_profile_id: "a1".into(),
            players,
            rounds,
        }
    }

    fn stats_of<'a>(v: &'a [PlayerMatchStats], id: &str) -> &'a PlayerMatchStats {
        v.iter().find(|s| s.profile_id == id).unwrap()
    }

    #[test]
    fn multi_kills_are_counted_per_round() {
        let r1 = round(1, 0, &[("a1", "b1", 150), ("a1", "b2", 140), ("a1", "b3", 130), ("a2", "b4", 120), ("a3", "b5", 110)]);
        let r2 = round(2, 0, &[("a1", "b1", 150), ("a1", "b2", 140)]);
        let r3 = round(3, 1, &[("b1", "a1", 150)]);
        let stats = player_match_stats(&full(vec![r1, r2, r3]));
        let a1 = stats_of(&stats, "a1");
        assert_eq!((a1.multi_kills.k2, a1.multi_kills.k3), (1, 1));
        assert_eq!(a1.multi_kills.total(), 2);
        assert_eq!((a1.kills, a1.deaths), (5, 1));
        let mut mk = MultiKills::default();
        for k in [1, 2, 3, 4, 5, 6] {
            mk.add_round(k);
        }
        assert_eq!((mk.k2, mk.k3, mk.k4, mk.k5), (1, 1, 1, 2));
    }

    #[test]
    fn won_1v2_clutch() {
        // b3..b5 are killed first, then a2..a5 die; a1 faces b1 and b2 and wins.
        let r = round(
            2,
            0,
            &[
                ("a2", "b3", 170), ("a3", "b4", 160), ("a4", "b5", 150),
                ("b1", "a2", 140), ("b1", "a3", 130), ("b2", "a4", 120), ("b2", "a5", 110),
                ("a1", "b1", 90), ("a1", "b2", 80),
            ],
        );
        // b2 is also last alive (1v1) once a1 kills b1; that is a separate clutch.
        let c = round_clutches(&r);
        assert_eq!(c.len(), 2);
        assert_eq!((c[0].profile_id.as_str(), c[0].vs, c[0].won), ("a1", 2, true));
        assert_eq!((c[1].profile_id.as_str(), c[1].vs, c[1].won), ("b2", 1, false));
        let stats = player_match_stats(&full(vec![r]));
        assert_eq!(stats_of(&stats, "a1").clutches_won, 1);
    }

    #[test]
    fn lost_1v2_clutch() {
        let r = round(
            1,
            1,
            &[
                ("a2", "b3", 170), ("a3", "b4", 160), ("a4", "b5", 150),
                ("b1", "a2", 140), ("b1", "a3", 130), ("b2", "a4", 120), ("b2", "a5", 110),
                ("a1", "b1", 90), ("b2", "a1", 80),
            ],
        );
        let c = round_clutches(&r);
        assert_eq!(c.len(), 2);
        assert_eq!((c[0].profile_id.as_str(), c[0].vs, c[0].won), ("a1", 2, false));
        let stats = player_match_stats(&full(vec![r]));
        let a1 = stats_of(&stats, "a1");
        assert_eq!((a1.clutches_won, a1.clutches_lost), (0, 1));
        // b2 became last alive (1v1) after a1 killed b1, and won it.
        assert_eq!(stats_of(&stats, "b2").clutches_won, 1);
    }

    #[test]
    fn kost_counts_kill_survival_and_trade() {
        // a2 dies to b1 at 140 and a3 kills b1 at 135 (traded, 5s). a4 dies to b2 at 120;
        // b2 is killed at 100 (20s later): not traded. a5 survives, a1 gets a kill and dies.
        let r = round(
            1,
            0,
            &[
                ("b1", "a2", 140), ("a3", "b1", 135), ("b2", "a4", 120),
                ("a1", "b2", 100), ("b3", "a1", 90),
            ],
        );
        assert!(is_traded(&r, "a2"));
        assert!(!is_traded(&r, "a4"));
        let stats = player_match_stats(&full(vec![r]));
        assert_eq!(stats_of(&stats, "a2").kost_rounds, 1); // traded death
        assert_eq!(stats_of(&stats, "a4").kost_rounds, 0); // died, not traded, no kill
        assert_eq!(stats_of(&stats, "a1").kost_rounds, 1); // got a kill
        assert_eq!(stats_of(&stats, "a5").kost_rounds, 1); // survived
        assert_eq!(stats_of(&stats, "a2").traded_deaths, 1);
        assert!((stats_of(&stats, "a5").kost_pct() - 100.0).abs() < 1e-9);
    }

    #[test]
    fn own_team_comes_first() {
        let stats = player_match_stats(&full(vec![round(1, 0, &[("b1", "a1", 100)])]));
        assert_eq!(stats.len(), 10);
        assert!(stats[..5].iter().all(|s| s.team_index == 0));
    }
}
