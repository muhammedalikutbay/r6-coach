//! Small statistics helpers for the advice engine: smoothing, sample-size confidence and
//! per-match / replay-history aggregates.

use crate::model::{Match, Side};
use crate::names;
use std::collections::BTreeMap;

/// Smoothing strength in rounds (see `docs`: `adj = (x + k*base) / (n + k)`).
pub const K: f64 = 10.0;
/// Relative deviation from the baseline that turns a number into advice.
pub const REL_THRESHOLD: f64 = 0.15;
/// Fewer rounds than this: describe only.
pub const MIN_ROUNDS: u32 = 5;
/// This many rounds or more: firm language.
pub const FIRM_ROUNDS: u32 = 20;

/// Smoothed rate: `(x + k*base_rate) / (n + k)`. `x` is the observed count of successes and `n`
/// the sample size; `base_rate` is a fraction (0..1) and so is the result.
pub fn adjusted(x: f64, n: f64, base_rate: f64) -> f64 {
    (x + K * base_rate) / (n + K)
}

/// How sure the wording may be, by sample size in rounds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Confidence {
    /// Fewer than 5 rounds: say what happened, do not judge.
    Describe,
    /// 5 to 19 rounds: careful wording.
    Cautious,
    /// 20 rounds or more: plain wording.
    Firm,
}

impl Confidence {
    pub fn from_rounds(rounds: u32) -> Self {
        if rounds < MIN_ROUNDS {
            Confidence::Describe
        } else if rounds < FIRM_ROUNDS {
            Confidence::Cautious
        } else {
            Confidence::Firm
        }
    }
}

/// Kills per death; with zero deaths this is the kill count.
pub fn kd(kills: u32, deaths: u32) -> f64 {
    if deaths == 0 {
        kills as f64
    } else {
        kills as f64 / deaths as f64
    }
}

/// Totals for one operator within one match.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct OpLine {
    pub rounds: u32,
    pub kills: u32,
    pub deaths: u32,
}

impl OpLine {
    pub fn kd(&self) -> f64 {
        kd(self.kills, self.deaths)
    }
}

/// Per-operator totals of one match, keyed by operator ID.
pub fn operator_lines(m: &Match) -> BTreeMap<u64, OpLine> {
    let mut out: BTreeMap<u64, OpLine> = BTreeMap::new();
    for r in &m.rounds {
        let l = out.entry(r.operator_id).or_default();
        l.rounds += 1;
        l.kills += r.kills;
        l.deaths += r.deaths;
    }
    out
}

pub fn first_bloods(m: &Match) -> u32 {
    m.rounds.iter().filter(|r| r.first_blood).count() as u32
}

pub fn first_deaths(m: &Match) -> u32 {
    m.rounds.iter().filter(|r| r.first_death).count() as u32
}

pub fn rounds_won(m: &Match) -> u32 {
    m.rounds.iter().filter(|r| r.won).count() as u32
}

/// Round record of one operator on one map on one side, from replay history.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct OpMapRecord {
    pub rounds: u32,
    pub won: u32,
}

impl OpMapRecord {
    /// Round win percentage (0 with no rounds).
    pub fn win_pct(&self) -> f64 {
        if self.rounds == 0 {
            0.0
        } else {
            100.0 * self.won as f64 / self.rounds as f64
        }
    }
}

/// True when both IDs are the same map. Several IDs share one map (see `data/maps.json`), so
/// compare the map key and fall back to the raw ID for unknown maps.
pub fn same_map(a: u64, b: u64) -> bool {
    if a == b {
        return true;
    }
    match (names::map_key(a), names::map_key(b)) {
        (Some(x), Some(y)) => x == y,
        _ => false,
    }
}

/// Round records keyed by `(operator_id, side)` for one map, over all given matches.
pub fn operator_map_records(
    history: &[Match],
    map_id: u64,
) -> BTreeMap<(u64, SideKey), OpMapRecord> {
    let mut out: BTreeMap<(u64, SideKey), OpMapRecord> = BTreeMap::new();
    for m in history.iter().filter(|m| same_map(m.map_id, map_id)) {
        for r in &m.rounds {
            let Some(side) = SideKey::of(r.side) else { continue };
            let rec = out.entry((r.operator_id, side)).or_default();
            rec.rounds += 1;
            rec.won += r.won as u32;
        }
    }
    out
}

/// A known side (attack or defense), usable as a map key.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum SideKey {
    Attack,
    Defense,
}

impl SideKey {
    pub fn of(side: Side) -> Option<Self> {
        match side {
            Side::Attack => Some(SideKey::Attack),
            Side::Defense => Some(SideKey::Defense),
            Side::Unknown => None,
        }
    }
    /// Key used by `Baseline::maps[..].sides`.
    pub fn baseline_key(self) -> &'static str {
        match self {
            SideKey::Attack => "attacker",
            SideKey::Defense => "defender",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn smoothing_pulls_toward_base() {
        // 10 successes in 10 rounds with base rate 0.5 over 10 rounds -> halfway.
        assert!((adjusted(10.0, 10.0, 0.5) - 0.75).abs() < 1e-9);
        // No data -> exactly the base.
        assert!((adjusted(0.0, 0.0, 0.42) - 0.42).abs() < 1e-9);
    }

    #[test]
    fn confidence_bands() {
        assert_eq!(Confidence::from_rounds(4), Confidence::Describe);
        assert_eq!(Confidence::from_rounds(5), Confidence::Cautious);
        assert_eq!(Confidence::from_rounds(19), Confidence::Cautious);
        assert_eq!(Confidence::from_rounds(20), Confidence::Firm);
    }

    #[test]
    fn kd_with_no_deaths() {
        assert_eq!(kd(5, 0), 5.0);
        assert_eq!(kd(6, 3), 2.0);
    }
}
