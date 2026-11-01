//! The player's lifetime baseline (`data/baseline.json`), typed and embedded in the binary.
//!
//! # No double counting
//!
//! The baseline is a snapshot taken at [`Baseline::captured_at`]. Every match played **before**
//! that moment is already counted inside it. Therefore:
//!
//! * personal averages = baseline + only the matches played **after** `captured_at`
//!   (see [`Baseline::merged`]);
//! * matches from before `captured_at` are used only for the "last match" view and the history
//!   list, never added to the baseline again.
//!
//! Match timestamps are compared as `YYYY-MM-DDTHH:MM:SS` strings. Note that replay timestamps are
//! the recording PC's local time with a misleading `Z` (see [`Match::timestamp`]), while
//! `capturedAt` is real UTC, so around the boundary the comparison can be off by the UTC offset.

use crate::model::{Match, Round, Side};
use crate::names;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

const BASELINE_JSON: &str = include_str!("../../../data/baseline.json");

/// Raw lifetime counters. Missing fields in the JSON read as 0. Rates are never stored; use the
/// helper methods.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct CounterSet {
    pub aces: u64,
    pub assists: u64,
    pub clutches: u64,
    pub deaths: u64,
    pub first_bloods: u64,
    pub first_deaths: u64,
    pub headshots: u64,
    pub kills: u64,
    pub matches_played: u64,
    pub matches_won: u64,
    pub rounds_played: u64,
    pub rounds_won: u64,
    pub time_played: u64,
    /// "attacker" or "defender"; only present on operator entries.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub side: Option<String>,
}

fn ratio(n: u64, d: u64) -> f64 {
    if d == 0 {
        0.0
    } else {
        n as f64 / d as f64
    }
}

impl CounterSet {
    /// Kills per death. With zero deaths this is the kill count (same rule as the generator).
    pub fn kd(&self) -> f64 {
        if self.deaths == 0 {
            self.kills as f64
        } else {
            self.kills as f64 / self.deaths as f64
        }
    }
    /// Headshots as a percentage of kills (0 when there are no kills).
    pub fn headshot_pct(&self) -> f64 {
        100.0 * ratio(self.headshots, self.kills)
    }
    /// Match win percentage (0 when no matches).
    pub fn win_pct(&self) -> f64 {
        100.0 * ratio(self.matches_won, self.matches_played)
    }
    /// Round win percentage (0 when no rounds).
    pub fn round_win_pct(&self) -> f64 {
        100.0 * ratio(self.rounds_won, self.rounds_played)
    }
    pub fn kills_per_round(&self) -> f64 {
        ratio(self.kills, self.rounds_played)
    }
}

/// Counters for one map: totals plus an attacker/defender split.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct MapStats {
    #[serde(flatten)]
    pub total: CounterSet,
    pub sides: BTreeMap<String, CounterSet>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Baseline {
    pub player: String,
    /// UTC instant the Tracker data was captured (RFC 3339, e.g. "2026-11-01T12:28:49Z").
    pub captured_at: String,
    /// Keyed by Tracker slug = `key` in `data/operators.json`.
    pub operators: BTreeMap<String, CounterSet>,
    /// Keyed by Tracker map slug (e.g. "kafe-dostoyevsky").
    pub maps: BTreeMap<String, MapStats>,
    /// Keyed by playlist (e.g. "pvp_ranked").
    pub gamemodes: BTreeMap<String, CounterSet>,
    pub overall: CounterSet,
}

/// `YYYY-MM-DDTHH:MM:SS` prefix used for ordering.
fn ts_prefix(s: &str) -> &str {
    s.get(..19).unwrap_or(s)
}

fn norm_slug(s: &str) -> String {
    s.chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .map(|c| c.to_ascii_lowercase())
        .collect()
}

fn gamemode_key(match_type: &str) -> Option<&'static str> {
    let t = match_type.to_lowercase();
    if t.contains("ranked") && !t.contains("unranked") {
        Some("pvp_ranked")
    } else if t.contains("quick") || t.contains("casual") || t.contains("unranked") {
        Some("pvp_quickplay")
    } else if t.contains("event") {
        Some("pvp_event")
    } else if t.contains("warm") {
        Some("pvp_warmup")
    } else {
        None
    }
}

fn side_key(side: Side) -> Option<&'static str> {
    match side {
        Side::Attack => Some("attacker"),
        Side::Defense => Some("defender"),
        Side::Unknown => None,
    }
}

impl Baseline {
    pub fn from_json(json: &str) -> Result<Baseline, serde_json::Error> {
        serde_json::from_str(json)
    }

    /// The baseline compiled into the binary.
    pub fn embedded() -> Result<Baseline, serde_json::Error> {
        Self::from_json(BASELINE_JSON)
    }

    /// Operator counters by operator ID, via the slug in `data/operators.json`.
    pub fn operator(&self, operator_id: u64) -> Option<&CounterSet> {
        self.operators.get(&names::operator_key(operator_id)?)
    }

    /// Top `n` operators by rounds played (ties broken by slug).
    pub fn top_operators(&self, n: usize) -> Vec<(&str, &CounterSet)> {
        let mut v: Vec<_> = self.operators.iter().map(|(k, c)| (k.as_str(), c)).collect();
        v.sort_by(|a, b| b.1.rounds_played.cmp(&a.1.rounds_played).then(a.0.cmp(b.0)));
        v.truncate(n);
        v
    }

    /// True when the match was played after the baseline snapshot.
    pub fn is_after_capture(&self, m: &Match) -> bool {
        ts_prefix(&m.timestamp) > ts_prefix(&self.captured_at)
    }

    /// Baseline plus the matches played after `captured_at` (no double counting, see the module
    /// docs). Matches at or before `captured_at` are ignored here.
    ///
    /// Tracked per match: kills, deaths, assists, headshots, first bloods/deaths, rounds,
    /// matches. Aces, clutches and time played are not derivable from a match, so they stay at
    /// the baseline value.
    pub fn merged(&self, matches: &[Match]) -> Baseline {
        let mut out = self.clone();
        for m in matches.iter().filter(|m| self.is_after_capture(m)) {
            out.add_match(m);
        }
        out
    }

    fn add_match(&mut self, m: &Match) {
        add_totals(&mut self.overall, m);
        if let Some(gm) = gamemode_key(&m.match_type) {
            add_totals(self.gamemodes.entry(gm.to_string()).or_default(), m);
        } else {
            log::debug!("match type {:?} has no baseline playlist", m.match_type);
        }

        // Operators: rounds per slug; matches counted once per operator used.
        let mut used: BTreeSet<String> = BTreeSet::new();
        for r in &m.rounds {
            let Some(slug) = names::operator_key(r.operator_id) else {
                log::warn!("operator id {} has no slug; skipped in merge", r.operator_id);
                continue;
            };
            let first_use = used.insert(slug.clone());
            let c = self.operators.entry(slug).or_default();
            if c.side.is_none() {
                c.side = side_key(r.side).map(str::to_string);
            }
            add_round(c, r);
            if first_use {
                c.matches_played += 1;
                if m.won {
                    c.matches_won += 1;
                }
            }
        }

        // Map: match to an existing baseline slug ignoring punctuation ("kafe-dostoyevsky").
        let Some(map_key) = names::map_key(m.map_id) else {
            log::warn!("map id {} has no key; skipped in merge", m.map_id);
            return;
        };
        let norm = norm_slug(&map_key);
        let slug = self
            .maps
            .keys()
            .find(|k| norm_slug(k) == norm)
            .cloned()
            .unwrap_or(map_key);
        let ms = self.maps.entry(slug).or_default();
        add_totals(&mut ms.total, m);
        for side in [Side::Attack, Side::Defense] {
            let Some(key) = side_key(side) else { continue };
            let rounds: Vec<_> = m.rounds.iter().filter(|r| r.side == side).collect();
            if rounds.is_empty() {
                continue;
            }
            let c = ms.sides.entry(key.to_string()).or_default();
            c.matches_played += 1;
            if m.won {
                c.matches_won += 1;
            }
            for r in rounds {
                add_round(c, r);
            }
        }
    }
}

fn add_round(c: &mut CounterSet, r: &Round) {
    c.rounds_played += 1;
    c.rounds_won += r.won as u64;
    c.kills += r.kills as u64;
    c.deaths += r.deaths as u64;
    c.assists += r.assists as u64;
    c.headshots += r.headshots as u64;
    c.first_bloods += r.first_blood as u64;
    c.first_deaths += r.first_death as u64;
}

/// Adds a whole match (all rounds and the match itself) to `c`.
fn add_totals(c: &mut CounterSet, m: &Match) {
    c.matches_played += 1;
    if m.won {
        c.matches_won += 1;
    }
    for r in &m.rounds {
        add_round(c, r);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn op(slug: &str) -> u64 {
        let json = include_str!("../../../data/operators.json");
        let v: serde_json::Value = serde_json::from_str(json).unwrap();
        v["operators"]
            .as_array()
            .unwrap()
            .iter()
            .find(|o| o["key"] == slug)
            .unwrap()["id"]
            .as_u64()
            .unwrap()
    }

    fn round(operator_id: u64, kills: u32, deaths: u32, won: bool) -> Round {
        Round {
            number: 1,
            side: Side::Attack,
            operator_id,
            kills,
            deaths,
            assists: 1,
            headshots: 1,
            first_blood: true,
            first_death: false,
            planted: false,
            defused: false,
            won,
        }
    }

    fn mat(ts: &str, operator_id: u64, won: bool) -> Match {
        Match {
            match_id: ts.into(),
            timestamp: ts.into(),
            match_type: "Ranked".into(),
            gamemode: "Bomb".into(),
            map_id: 407558616688, // Chalet
            won,
            rounds: vec![round(operator_id, 2, 1, won), round(operator_id, 1, 1, false)],
        }
    }

    #[test]
    fn embedded_loads() {
        let b = Baseline::embedded().unwrap();
        assert_eq!(b.player, "H.A.N-01");
        assert_eq!(b.operators.len(), 77);
        assert_eq!(b.maps.len(), 20);
        assert_eq!(b.overall.kills, 8004);
        assert!(b.maps["bank"].sides.contains_key("attacker"));
    }

    #[test]
    fn rates() {
        let b = Baseline::embedded().unwrap();
        let rook = &b.operators["rook"];
        assert_eq!(rook.rounds_played, 388);
        assert!((rook.kd() - 1.52).abs() < 0.005);
        assert!((rook.headshot_pct() - 73.2).abs() < 0.05);
        let z = CounterSet::default();
        assert_eq!(z.kd(), 0.0);
        assert_eq!(z.win_pct(), 0.0);
        assert_eq!(z.kills_per_round(), 0.0);
    }

    #[test]
    fn every_baseline_operator_slug_is_in_the_table() {
        let b = Baseline::embedded().unwrap();
        let json = include_str!("../../../data/operators.json");
        for slug in b.operators.keys() {
            assert!(json.contains(&format!("\"key\": \"{slug}\"")), "{slug}");
        }
        assert!(b.operator(op("rook")).is_some());
    }

    #[test]
    fn merged_adds_only_matches_after_capture() {
        let b = Baseline::embedded().unwrap();
        let rook = op("rook");
        let before = mat("2026-10-30T20:00:00Z", rook, true);
        let after = mat("2026-11-01T13:00:00Z", rook, true);
        let merged = b.merged(&[before, after]);

        assert_eq!(merged.overall.kills, b.overall.kills + 3);
        assert_eq!(merged.overall.matches_played, b.overall.matches_played + 1);
        assert_eq!(merged.overall.matches_won, b.overall.matches_won + 1);
        let (r0, r1) = (&b.operators["rook"], &merged.operators["rook"]);
        assert_eq!(r1.rounds_played, r0.rounds_played + 2);
        assert_eq!(r1.rounds_won, r0.rounds_won + 1);
        assert_eq!(r1.matches_played, r0.matches_played + 1);
        assert_eq!(r1.first_bloods, r0.first_bloods + 2);
        assert_eq!(r1.clutches, r0.clutches);
        assert_eq!(
            merged.gamemodes["pvp_ranked"].matches_played,
            b.gamemodes["pvp_ranked"].matches_played + 1
        );
        assert_eq!(
            merged.maps["chalet"].sides["attacker"].rounds_played,
            b.maps["chalet"].sides["attacker"].rounds_played + 2
        );
        assert_eq!(merged.maps["chalet"].total.kills, b.maps["chalet"].total.kills + 3);
    }

    #[test]
    fn merged_with_only_old_matches_is_identity() {
        let b = Baseline::embedded().unwrap();
        let old = mat("2026-10-31T10:00:00Z", op("rook"), true);
        assert_eq!(b.merged(&[old]), b);
        assert_eq!(b.merged(&[]), b);
    }
}
