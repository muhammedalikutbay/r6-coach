//! DTOs sent to the frontend. Field names match `app/src/lib/types.ts` (camelCase).

use r6k_core::{names, Match};
use serde::Serialize;
use std::collections::HashMap;

#[derive(Debug, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct OperatorUse {
    pub name: String,
    pub rounds: u32,
}

#[derive(Debug, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct MatchSummary {
    pub match_id: String,
    pub timestamp: String,
    pub map: String,
    pub match_type: String,
    pub won: bool,
    pub score_us: u32,
    pub score_them: u32,
    pub operators: Vec<OperatorUse>,
    pub kills: u32,
    pub deaths: u32,
    pub headshots: u32,
    pub first_bloods: u32,
    pub first_deaths: u32,
}

impl From<&Match> for MatchSummary {
    fn from(m: &Match) -> Self {
        let score_us = m.rounds.iter().filter(|r| r.won).count() as u32;
        let score_them = m.rounds.len() as u32 - score_us;

        let mut counts: HashMap<u64, u32> = HashMap::new();
        for r in &m.rounds {
            *counts.entry(r.operator_id).or_default() += 1;
        }
        let mut ops: Vec<(u64, u32)> = counts.into_iter().collect();
        // Most played first; ties by id so the order is stable.
        ops.sort_by_key(|&(id, n)| (std::cmp::Reverse(n), id));

        MatchSummary {
            match_id: m.match_id.clone(),
            timestamp: m.timestamp.clone(),
            map: names::map_name(m.map_id),
            match_type: m.match_type.clone(),
            won: m.won,
            score_us,
            score_them,
            operators: ops
                .into_iter()
                .map(|(id, rounds)| OperatorUse { name: names::operator_name(id), rounds })
                .collect(),
            kills: m.kills(),
            deaths: m.deaths(),
            headshots: m.headshots(),
            first_bloods: m.rounds.iter().filter(|r| r.first_blood).count() as u32,
            first_deaths: m.rounds.iter().filter(|r| r.first_death).count() as u32,
        }
    }
}

#[derive(Debug, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct MapUse {
    pub name: String,
    pub matches: u32,
    pub wins: u32,
}

/// Totals of one sitting with readable names (the core `SessionSummary` keeps raw IDs).
#[derive(Debug, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SessionView {
    pub matches: u32,
    pub wins: u32,
    pub losses: u32,
    pub rounds: u32,
    pub kills: u32,
    pub deaths: u32,
    pub headshots: u32,
    pub first_bloods: u32,
    pub first_deaths: u32,
    pub operators: Vec<OperatorUse>,
    pub maps: Vec<MapUse>,
}

impl From<&[Match]> for SessionView {
    fn from(session: &[Match]) -> Self {
        let sum = r6k_core::session::summarize(session);
        // Group by display name, so unknown maps with different IDs stay separate but known
        // duplicate IDs of one map merge.
        let mut maps: Vec<MapUse> = Vec::new();
        for m in session {
            let name = names::map_name(m.map_id);
            match maps.iter_mut().find(|u| u.name == name) {
                Some(u) => {
                    u.matches += 1;
                    u.wins += m.won as u32;
                }
                None => maps.push(MapUse { name, matches: 1, wins: m.won as u32 }),
            }
        }
        maps.sort_by(|a, b| b.matches.cmp(&a.matches).then(a.name.cmp(&b.name)));
        SessionView {
            matches: sum.matches,
            wins: sum.wins,
            losses: sum.losses,
            rounds: sum.rounds,
            kills: sum.kills,
            deaths: sum.deaths,
            headshots: sum.headshots,
            first_bloods: sum.first_bloods,
            first_deaths: sum.first_deaths,
            operators: sum
                .top_operators
                .iter()
                .map(|&(id, rounds, _, _)| OperatorUse { name: names::operator_name(id), rounds })
                .collect(),
            maps,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use r6k_core::{Round, Side};

    fn round(n: u32, op: u64, won: bool, kills: u32) -> Round {
        Round {
            number: n,
            side: Side::Attack,
            operator_id: op,
            kills,
            deaths: 0,
            assists: 0,
            headshots: 0,
            first_blood: n == 1,
            first_death: false,
            planted: false,
            defused: false,
            won,
        }
    }

    #[test]
    fn summary_matches_frontend_shape() {
        let m = Match {
            match_id: "m1".into(),
            timestamp: "2026-10-31T00:54:47Z".into(),
            match_type: "Ranked".into(),
            gamemode: "Bomb".into(),
            map_id: 999,
            won: true,
            rounds: vec![round(1, 378305069945, true, 2), round(2, 378305069945, false, 1), round(3, 5, true, 0)],
        };
        let s = MatchSummary::from(&m);
        assert_eq!((s.score_us, s.score_them), (2, 1));
        assert_eq!(s.operators[0], OperatorUse { name: "Azami".into(), rounds: 2 });
        assert_eq!(s.operators[1].name, "Operator #5");
        assert_eq!(s.map, "Map #999");
        assert_eq!((s.kills, s.first_bloods), (3, 1));
        let json = serde_json::to_value(&s).unwrap();
        for key in ["matchId", "scoreUs", "scoreThem", "matchType", "firstBloods", "firstDeaths"] {
            assert!(json.get(key).is_some(), "missing {key}");
        }
    }
}
