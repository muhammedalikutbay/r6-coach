//! Operator and map tables for the stats screens: the baseline merged with post-capture matches.

use r6k_core::{names, Baseline, CounterSet, Match};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::OnceLock;

const OPERATORS_JSON: &str = include_str!("../../../data/operators.json");
const MAPS_JSON: &str = include_str!("../../../data/maps.json");

/// Recent-form window and thresholds for the trend arrow.
const TREND_ROUNDS: usize = 10;
const TREND_MIN_ROUNDS: usize = 5;
const TREND_DELTA: f64 = 0.15;

#[derive(Debug, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct OperatorRow {
    pub name: String,
    pub side: String, // "attack" | "defense"
    pub rounds: u64,
    pub win_pct: f64,
    pub kd: f64,
    pub hs_pct: f64,
    pub trend: String, // "up" | "down" | "flat"
}

#[derive(Debug, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct MapRow {
    pub name: String,
    pub rounds: u64,
    pub win_pct: f64,
    pub kd: f64,
    pub attack_win_pct: f64,
    pub defense_win_pct: f64,
}

#[derive(Deserialize)]
struct OpFile {
    operators: Vec<OpEntry>,
}
#[derive(Deserialize)]
struct OpEntry {
    key: String,
    name: String,
    #[serde(default)]
    side: String,
}
#[derive(Deserialize)]
struct MapFile {
    maps: Vec<MapEntry>,
}
#[derive(Deserialize)]
struct MapEntry {
    key: String,
    name: String,
}

/// slug -> (display name, "attack"/"defense")
fn operator_info() -> &'static HashMap<String, (String, String)> {
    static T: OnceLock<HashMap<String, (String, String)>> = OnceLock::new();
    T.get_or_init(|| {
        serde_json::from_str::<OpFile>(OPERATORS_JSON)
            .map(|f| {
                f.operators
                    .into_iter()
                    .map(|o| (o.key, (o.name, side_label(&o.side))))
                    .collect()
            })
            .unwrap_or_else(|e| {
                log::error!("operators.json invalid: {e}");
                HashMap::new()
            })
    })
}

/// normalised map key -> display name (several map IDs share one key)
fn map_names() -> &'static HashMap<String, String> {
    static T: OnceLock<HashMap<String, String>> = OnceLock::new();
    T.get_or_init(|| {
        serde_json::from_str::<MapFile>(MAPS_JSON)
            .map(|f| {
                let mut m = HashMap::new();
                for e in f.maps {
                    m.entry(norm(&e.key)).or_insert(e.name);
                }
                m
            })
            .unwrap_or_else(|e| {
                log::error!("maps.json invalid: {e}");
                HashMap::new()
            })
    })
}

fn norm(s: &str) -> String {
    s.chars().filter(|c| c.is_ascii_alphanumeric()).map(|c| c.to_ascii_lowercase()).collect()
}

fn side_label(s: &str) -> String {
    if s.starts_with("def") { "defense" } else { "attack" }.to_string()
}

fn title_case(slug: &str) -> String {
    slug.split(['-', '_'])
        .filter(|w| !w.is_empty())
        .map(|w| {
            let mut c = w.chars();
            c.next().map(|f| f.to_uppercase().chain(c).collect::<String>()).unwrap_or_default()
        })
        .collect::<Vec<_>>()
        .join(" ")
}

fn kd(kills: u64, deaths: u64) -> f64 {
    if deaths == 0 { kills as f64 } else { kills as f64 / deaths as f64 }
}

/// Trend of the last rounds on one operator against the baseline K/D.
fn trend(recent_kills: u64, recent_deaths: u64, recent_rounds: usize, base: &CounterSet) -> &'static str {
    let base_kd = base.kd();
    if recent_rounds < TREND_MIN_ROUNDS || base_kd <= 0.0 {
        return "flat";
    }
    let change = kd(recent_kills, recent_deaths) / base_kd - 1.0;
    if change >= TREND_DELTA {
        "up"
    } else if change <= -TREND_DELTA {
        "down"
    } else {
        "flat"
    }
}

/// Operators with at least one round, most played first. `matches` must be oldest first
/// (as `store::load_matches` returns them); all of them feed the trend, only those after the
/// baseline capture feed the totals.
pub fn operator_table(baseline: &Baseline, matches: &[Match]) -> Vec<OperatorRow> {
    let merged = baseline.merged(matches);

    // slug -> (kills, deaths, rounds) of the last TREND_ROUNDS rounds, newest first.
    let mut recent: HashMap<String, (u64, u64, usize)> = HashMap::new();
    for m in matches.iter().rev() {
        for r in m.rounds.iter().rev() {
            let Some(slug) = names::operator_key(r.operator_id) else { continue };
            let e = recent.entry(slug).or_default();
            if e.2 < TREND_ROUNDS {
                e.0 += r.kills as u64;
                e.1 += r.deaths as u64;
                e.2 += 1;
            }
        }
    }

    let mut rows: Vec<OperatorRow> = merged
        .operators
        .iter()
        .filter(|(_, c)| c.rounds_played > 0)
        .map(|(slug, c)| {
            let info = operator_info().get(slug);
            let side = match &c.side {
                Some(s) => side_label(s),
                None => info.map(|i| i.1.clone()).unwrap_or_else(|| "attack".into()),
            };
            let (rk, rd, rn) = recent.get(slug).copied().unwrap_or_default();
            // Trend compares against the baseline as captured, not the merged value.
            let base = baseline.operators.get(slug).unwrap_or(c);
            OperatorRow {
                name: info.map(|i| i.0.clone()).unwrap_or_else(|| title_case(slug)),
                side,
                rounds: c.rounds_played,
                win_pct: c.round_win_pct(),
                kd: c.kd(),
                hs_pct: c.headshot_pct(),
                trend: trend(rk, rd, rn, base).to_string(),
            }
        })
        .collect();
    rows.sort_by(|a, b| b.rounds.cmp(&a.rounds).then(a.name.cmp(&b.name)));
    rows
}

/// Maps with at least one round, most played first.
pub fn map_table(baseline: &Baseline, matches: &[Match]) -> Vec<MapRow> {
    let merged = baseline.merged(matches);
    // Merging already folds IDs that share a key into one slug; fold any remaining duplicates.
    let mut by_key: HashMap<String, (String, Vec<&r6k_core::MapStats>)> = HashMap::new();
    for (slug, ms) in &merged.maps {
        by_key.entry(norm(slug)).or_insert_with(|| (slug.clone(), Vec::new())).1.push(ms);
    }
    let mut rows: Vec<MapRow> = by_key
        .into_iter()
        .filter_map(|(key, (slug, list))| {
            let mut total = CounterSet::default();
            let mut atk = CounterSet::default();
            let mut def = CounterSet::default();
            for ms in list {
                acc(&mut total, &ms.total);
                if let Some(c) = ms.sides.get("attacker") {
                    acc(&mut atk, c);
                }
                if let Some(c) = ms.sides.get("defender") {
                    acc(&mut def, c);
                }
            }
            if total.rounds_played == 0 {
                return None;
            }
            Some(MapRow {
                name: map_names().get(&key).cloned().unwrap_or_else(|| title_case(&slug)),
                rounds: total.rounds_played,
                win_pct: total.round_win_pct(),
                kd: total.kd(),
                attack_win_pct: atk.round_win_pct(),
                defense_win_pct: def.round_win_pct(),
            })
        })
        .collect();
    rows.sort_by(|a, b| b.rounds.cmp(&a.rounds).then(a.name.cmp(&b.name)));
    rows
}

fn acc(into: &mut CounterSet, from: &CounterSet) {
    into.kills += from.kills;
    into.deaths += from.deaths;
    into.headshots += from.headshots;
    into.rounds_played += from.rounds_played;
    into.rounds_won += from.rounds_won;
}

#[cfg(test)]
mod tests {
    use super::*;
    use r6k_core::{Round, Side};

    fn rook_id() -> u64 {
        let v: serde_json::Value = serde_json::from_str(OPERATORS_JSON).unwrap();
        v["operators"].as_array().unwrap().iter().find(|o| o["key"] == "rook").unwrap()["id"]
            .as_u64()
            .unwrap()
    }

    fn round(n: u32, kills: u32, deaths: u32) -> Round {
        Round {
            number: n,
            side: Side::Defense,
            operator_id: rook_id(),
            kills,
            deaths,
            assists: 0,
            headshots: 0,
            first_blood: false,
            first_death: false,
            planted: false,
            defused: false,
            won: true,
        }
    }

    fn mat(ts: &str, kills: u32, deaths: u32) -> Match {
        Match {
            match_id: ts.into(),
            timestamp: ts.into(),
            match_type: "Ranked".into(),
            gamemode: "Bomb".into(),
            map_id: 407558616688,
            won: true,
            rounds: (1..=6).map(|n| round(n, kills, deaths)).collect(),
        }
    }

    #[test]
    fn baseline_rounds_shown_and_old_matches_not_double_counted() {
        let b = Baseline::embedded().unwrap();
        let rows = operator_table(&b, &[mat("2026-10-30T10:00:00Z", 1, 1)]);
        let rook = rows.iter().find(|r| r.name == "Rook").unwrap();
        assert_eq!(rook.rounds, b.operators["rook"].rounds_played);
        assert_eq!(rows[0].rounds, b.top_operators(1)[0].1.rounds_played);
        assert!(rows.windows(2).all(|w| w[0].rounds >= w[1].rounds));
    }

    #[test]
    fn trend_follows_recent_kd() {
        let b = Baseline::embedded().unwrap(); // Rook baseline K/D ~1.52
        let hot = operator_table(&b, &[mat("2026-10-30T10:00:00Z", 4, 1)]);
        assert_eq!(hot.iter().find(|r| r.name == "Rook").unwrap().trend, "up");
        let cold = operator_table(&b, &[mat("2026-10-30T10:00:00Z", 0, 1)]);
        assert_eq!(cold.iter().find(|r| r.name == "Rook").unwrap().trend, "down");
        let few = operator_table(&b, &[Match { rounds: vec![round(1, 9, 0)], ..mat("2026-10-30T10:00:00Z", 0, 0) }]);
        assert_eq!(few.iter().find(|r| r.name == "Rook").unwrap().trend, "flat");
    }

    #[test]
    fn map_table_adds_post_capture_matches() {
        let b = Baseline::embedded().unwrap();
        let before = map_table(&b, &[]);
        let after = map_table(&b, &[mat("2026-11-02T10:00:00Z", 1, 1)]);
        let chalet = |rows: &[MapRow]| rows.iter().find(|r| r.name == "Chalet").unwrap().rounds;
        assert_eq!(chalet(&after), chalet(&before) + 6);
        assert!(before.windows(2).all(|w| w[0].rounds >= w[1].rounds));
    }
}
