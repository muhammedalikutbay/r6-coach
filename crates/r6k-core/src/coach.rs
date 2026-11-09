//! Coach v2/v4 (spec section 5; v4 decision 2): compares a focus set of matches with a reference
//! set across every metric the replays give us and ranks the differences by importance.
//!
//! * Focus = `apply(filter)`. The Coach view sends `range = last1 | last5`; other ranges work too.
//! * Reference = the same filter without the range, minus the focus: every other match for
//!   `last1`, the 20 matches before the focus for `last5`. The reference is always the player's own
//!   stored matches; when it is too thin there is no comparison (no external baseline exists).
//! * Everything is judged on smoothed rates (`stats::adjusted`) so a short streak cannot
//!   produce firm advice, and every metric has a minimum sample size (the reference needs twice
//!   as much).
//! * Importance (`docs/coach-audit.md`, section 3): `score = E x C x W`.
//!   `E = min(1, |change| / S)` (S = the change that counts as a full effect), `C = n / (n + N_min)`
//!   and `W` is the weight for how controllable the metric is and how close it sits to the round
//!   outcome. A card needs `n >= N_min` and a change of at least 5 points (rates) or 15 % (ratios).
//!   Cards of one family (duel, trade, impact, side, ...) are deduplicated: the best one wins.
//! * Weapons, assists and score: only kill-side weapon facts are used (the parser has no weapon
//!   for deaths). Assists and score are not claimed until the parser delivers them.

use crate::full::{FullMatch, FullRound, RoundPlayer};
use crate::metrics::{Deep, Rate};
use crate::model::Side;
use crate::names;
use crate::query::{apply, selected, Filter, Mode, Range, SideFilter};
use crate::derive::round_clutches;
use crate::stats::{adjusted, Confidence, K, REL_THRESHOLD};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashSet};

/// Matches before the focus that form the reference for `last5`.
pub const PREVIOUS_MATCHES: usize = 20;
/// Smoothing strength for match-level rates (a match is worth far more than a round).
const K_MATCH: f64 = 3.0;
/// Smoothing strength for clutch rates (clutch situations are rare).
const K_CLUTCH: f64 = 3.0;
/// Smoothing strength for event-level rates (duels, trades, deaths, kills).
const K_EVENT: f64 = 5.0;
/// Rounds an operator or map needs inside the focus before it gets its own card.
const MIN_FOCUS_ROUNDS_SPLIT: u32 = 6;
/// Rounds an operator or map needs in the reference before it can be compared.
const MIN_REF_ROUNDS_SPLIT: u32 = 12;
/// Rounds a map-and-side or site split needs in the focus (the reference needs twice as many).
const MIN_SIDE_ROUNDS: u32 = 10;
/// Kills with one weapon the focus needs before its headshot rate is compared.
const MIN_WEAPON_KILLS: u32 = 20;
/// Deaths to one enemy operator the focus needs before it is called a nemesis.
const MIN_NEMESIS_DEATHS: u32 = 5;
/// Rounds against one enemy operator the reference needs before the nemesis comparison.
const MIN_NEMESIS_FACED: u32 = 15;
/// A rate must move at least this many points to count.
const PTS_THRESHOLD: f64 = 5.0;
/// Significant insight cards (praise or improve) that are kept at most.
const MAX_SIGNAL: usize = 16;
/// Strengths and work-on lines.
const MAX_WORK_ON: usize = 3;
const MAX_STRENGTHS: usize = 2;

// ---------------------------------------------------------------- output (matches types.ts)

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum InsightKind {
    Praise,
    Improve,
    Info,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Insight {
    pub kind: InsightKind,
    pub title: String,
    pub text: String,
    pub evidence: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub operator_key: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub map_key: Option<String>,
    /// Importance (`E x C x W`, 0 to 1.5); 0 for info cards.
    #[serde(default)]
    pub score: f64,
    /// Card family; at most one card per family is shown.
    #[serde(default)]
    pub family: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Comparison {
    pub metric: String,
    pub focus: f64,
    pub reference: f64,
    pub delta: f64,
    pub better: bool,
    /// Importance of the difference (0 when it is below the noise gate).
    #[serde(default)]
    pub score: f64,
    /// Sample size behind the focus value (rounds, kills, duels, ...).
    #[serde(default)]
    pub n: u32,
    /// `pct` (0..100) or `ratio`.
    #[serde(default)]
    pub unit: String,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CoachReport {
    pub summary: String,
    pub insights: Vec<Insight>,
    pub strengths: Vec<String>,
    pub work_on: Vec<String>,
    pub comparisons: Vec<Comparison>,
    pub focus_label: String,
    pub reference_label: String,
}

// ---------------------------------------------------------------- small helpers

fn round_to(x: f64, places: i32) -> f64 {
    let f = 10f64.powi(places);
    (x * f).round() / f
}

/// `1 round`, `2 rounds`.
fn plural(n: u64, one: &str, many: &str) -> String {
    format!("{n} {}", if n == 1 { one } else { many })
}
fn matches_txt(n: u64) -> String {
    plural(n, "match", "matches")
}
fn rounds_txt(n: u64) -> String {
    plural(n, "round", "rounds")
}

fn op_key(id: u64) -> String {
    names::operator_key(id).unwrap_or_else(|| id.to_string())
}
fn op_name(key: &str) -> String {
    names::operator_name_by_key(key).unwrap_or_else(|| match key.parse::<u64>() {
        Ok(id) => format!("Operator #{id}"),
        Err(_) => key.to_string(),
    })
}
fn map_key_of(m: &FullMatch) -> String {
    names::map_key(m.map_id).unwrap_or_else(|| m.map_id.to_string())
}
fn map_name(key: &str) -> String {
    names::map_name_by_key(key).unwrap_or_else(|| match key.parse::<u64>() {
        Ok(id) => crate::names::unknown_map_name(id),
        Err(_) => key.to_string(),
    })
}

/// Importance: `E x C x W` (see the module docs).
fn importance(change: f64, scale: f64, n: u32, n_min: u32, weight: f64) -> f64 {
    let e = (change.abs() / scale).min(1.0);
    let c = n as f64 / (n as f64 + n_min as f64);
    e * c * weight
}

// ---------------------------------------------------------------- tallies

/// Counters of the tracked player over some rounds.
#[derive(Debug, Clone, Copy, Default)]
struct Tally {
    rounds: u32,
    rounds_won: u32,
    kills: u32,
    deaths: u32,
    headshots: u32,
    fk: u32,
    fd: u32,
    survived: u32,
    /// Rounds with two or more kills.
    multi: u32,
    clutch_won: u32,
    clutch_lost: u32,
    atk_rounds: u32,
    atk_won: u32,
    def_rounds: u32,
    def_won: u32,
}

impl Tally {
    fn add_round(&mut self, r: &FullRound, p: &RoundPlayer) {
        let won = r.winner_team == Some(p.team_index);
        self.rounds += 1;
        self.rounds_won += won as u32;
        self.kills += p.kills;
        self.deaths += p.deaths;
        self.headshots += p.headshots;
        self.fk += p.first_kill as u32;
        self.fd += p.first_death as u32;
        self.survived += p.survived as u32;
        self.multi += (p.kills >= 2) as u32;
        match p.side {
            Side::Attack => {
                self.atk_rounds += 1;
                self.atk_won += won as u32;
            }
            Side::Defense => {
                self.def_rounds += 1;
                self.def_won += won as u32;
            }
            Side::Unknown => {}
        }
        for c in round_clutches(r).iter().filter(|c| c.profile_id == p.profile_id) {
            if c.won {
                self.clutch_won += 1;
            } else {
                self.clutch_lost += 1;
            }
        }
    }
    fn merge(&mut self, o: &Tally) {
        self.rounds += o.rounds;
        self.rounds_won += o.rounds_won;
        self.kills += o.kills;
        self.deaths += o.deaths;
        self.headshots += o.headshots;
        self.fk += o.fk;
        self.fd += o.fd;
        self.survived += o.survived;
        self.multi += o.multi;
        self.clutch_won += o.clutch_won;
        self.clutch_lost += o.clutch_lost;
        self.atk_rounds += o.atk_rounds;
        self.atk_won += o.atk_won;
        self.def_rounds += o.def_rounds;
        self.def_won += o.def_won;
    }
}

/// One match's contribution to an aggregate.
struct MatchTally {
    id: String,
    map_key: String,
    won: bool,
    total: Tally,
}

/// Totals over a set of matches plus operator and map splits.
#[derive(Default)]
struct Agg {
    t: Tally,
    deep: Deep,
    matches: u32,
    wins: u32,
    by_op: BTreeMap<String, Tally>,
    by_map: BTreeMap<String, Tally>,
    per_match: Vec<MatchTally>,
}

impl Agg {
    fn build(f: &Filter, set: &[&FullMatch]) -> Agg {
        let mut a = Agg::default();
        for m in set {
            let map_key = map_key_of(m);
            let mut mt = Tally::default();
            for r in &m.rounds {
                let Some(p) = selected(f, r, &m.me_profile_id) else { continue };
                mt.add_round(r, p);
                a.deep.add_round(&map_key, r, p);
                a.by_op.entry(op_key(p.operator_id)).or_default().add_round(r, p);
                a.by_map.entry(map_key.clone()).or_default().add_round(r, p);
            }
            if mt.rounds == 0 {
                continue;
            }
            a.t.merge(&mt);
            a.matches += 1;
            a.wins += m.won as u32;
            a.per_match.push(MatchTally { id: m.match_id.clone(), map_key, won: m.won, total: mt });
        }
        a
    }
}

// ---------------------------------------------------------------- metrics

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum Metric {
    Kd,
    Kpr,
    Hs,
    Win,
    FkFd,
    Clutch,
    Survival,
    Multi,
    AtkWin,
    DefWin,
    // v4: gaps G1-G11 of the coach audit
    Entry,
    EntryAtk,
    EntryDef,
    Traded,
    TradesGiven,
    Kost,
    AfterDeath,
    AtkLossTime,
    PrimaryHs,
    EarlyDeath,
    Clutch1v1,
    ClutchMulti,
}

const METRICS: [Metric; 22] = [
    Metric::Kd,
    Metric::Kpr,
    Metric::Hs,
    Metric::Win,
    Metric::FkFd,
    Metric::Clutch,
    Metric::Survival,
    Metric::Multi,
    Metric::AtkWin,
    Metric::DefWin,
    Metric::Entry,
    Metric::EntryAtk,
    Metric::EntryDef,
    Metric::Traded,
    Metric::TradesGiven,
    Metric::Kost,
    Metric::AfterDeath,
    Metric::AtkLossTime,
    Metric::PrimaryHs,
    Metric::EarlyDeath,
    Metric::Clutch1v1,
    Metric::ClutchMulti,
];

fn ratio(x: u32, y: u32) -> f64 {
    if y == 0 {
        x as f64
    } else {
        x as f64 / y as f64
    }
}

impl Metric {
    fn label(self) -> &'static str {
        match self {
            Metric::Kd => "K/D",
            Metric::Kpr => "Kills / round",
            Metric::Hs => "HS %",
            Metric::Win => "Win %",
            Metric::FkFd => "FK / FD",
            Metric::Clutch => "Clutch %",
            Metric::Survival => "Survival %",
            Metric::Multi => "Multi-kill rounds %",
            Metric::AtkWin => "Attack round win %",
            Metric::DefWin => "Defense round win %",
            Metric::Entry => "Opening duel win %",
            Metric::EntryAtk => "Opening duels on attack %",
            Metric::EntryDef => "Opening duels on defense %",
            Metric::Traded => "Deaths traded %",
            Metric::TradesGiven => "Teammate deaths traded %",
            Metric::Kost => "KOST %",
            Metric::AfterDeath => "Team wins after your death %",
            Metric::AtkLossTime => "Lost attacks ending on time %",
            Metric::PrimaryHs => "Primary weapon HS %",
            Metric::EarlyDeath => "Deaths in the first 30 s %",
            Metric::Clutch1v1 => "1v1 clutch %",
            Metric::ClutchMulti => "1vX (2+) clutch %",
        }
    }

    /// Card family: at most one card per family is kept.
    fn family(self) -> &'static str {
        match self {
            Metric::Kd | Metric::Kpr | Metric::Win | Metric::Survival | Metric::Multi => "outcome",
            Metric::FkFd | Metric::Entry | Metric::EntryAtk | Metric::EntryDef => "duel",
            Metric::Traded | Metric::TradesGiven => "trade",
            Metric::Kost => "impact",
            Metric::AfterDeath => "deathcost",
            Metric::AtkWin | Metric::DefWin => "side",
            Metric::AtkLossTime => "roundend",
            Metric::EarlyDeath => "time",
            Metric::Hs | Metric::PrimaryHs => "aim",
            Metric::Clutch | Metric::Clutch1v1 | Metric::ClutchMulti => "clutch",
        }
    }

    /// Weight: controllability and closeness to the round outcome (audit section 3).
    fn weight(self) -> f64 {
        match self {
            Metric::FkFd | Metric::Entry | Metric::EntryAtk | Metric::EntryDef => 1.5,
            Metric::Traded | Metric::TradesGiven => 1.5,
            Metric::Kost | Metric::AtkWin | Metric::DefWin | Metric::AtkLossTime => 1.2,
            Metric::AfterDeath | Metric::EarlyDeath | Metric::Hs | Metric::PrimaryHs => 1.0,
            Metric::Clutch | Metric::Clutch1v1 | Metric::ClutchMulti => 0.7,
            Metric::Kd | Metric::Kpr | Metric::Win | Metric::Survival | Metric::Multi => 0.8,
        }
    }

    /// The change that counts as a full effect (`S`).
    fn scale(self) -> f64 {
        match self {
            Metric::Kd | Metric::FkFd => 0.25,
            Metric::Kpr => 0.15,
            _ => 10.0,
        }
    }

    /// Is a bigger value better?
    fn higher_better(self) -> bool {
        !matches!(self, Metric::AtkLossTime | Metric::EarlyDeath)
    }

    /// Is the metric a percentage (value 0..100)?
    fn is_pct(self) -> bool {
        !matches!(self, Metric::Kd | Metric::Kpr | Metric::FkFd)
    }

    fn fmt(self, v: f64) -> String {
        if self.is_pct() {
            format!("{v:.1}%")
        } else {
            format!("{v:.2}")
        }
    }

    /// Successes and trials for rate-type metrics.
    fn parts(self, a: &Agg) -> (f64, f64) {
        let t = &a.t;
        let d = &a.deep;
        let rate = |r: Rate| (r.x, r.n);
        let (x, n) = match self {
            Metric::Kpr => (t.kills, t.rounds),
            Metric::Hs => (t.headshots, t.kills),
            Metric::Win => (a.wins, a.matches),
            Metric::Clutch => (t.clutch_won, t.clutch_won + t.clutch_lost),
            Metric::Survival => (t.survived, t.rounds),
            Metric::Multi => (t.multi, t.rounds),
            Metric::AtkWin => (t.atk_won, t.atk_rounds),
            Metric::DefWin => (t.def_won, t.def_rounds),
            Metric::Entry => rate(d.entry_all()),
            Metric::EntryAtk => rate(d.entry[0]),
            Metric::EntryDef => rate(d.entry[1]),
            Metric::Traded => rate(d.traded),
            Metric::TradesGiven => rate(d.trades_given),
            Metric::Kost => rate(d.kost),
            Metric::AfterDeath => rate(d.after_death),
            Metric::AtkLossTime => (d.lost[0].time, d.lost[0].total()),
            Metric::PrimaryHs => rate(d.primary_hs),
            Metric::EarlyDeath => rate(d.early_death),
            Metric::Clutch1v1 => rate(d.clutch[0]),
            Metric::ClutchMulti => rate(d.clutch_multi()),
            Metric::Kd | Metric::FkFd => (0, 0),
        };
        (x as f64, n as f64)
    }

    /// Sample size behind the metric.
    fn sample(self, a: &Agg) -> u32 {
        let t = &a.t;
        match self {
            Metric::Kd | Metric::Kpr | Metric::Survival | Metric::Multi => t.rounds,
            Metric::Hs => t.kills,
            Metric::Win => a.matches,
            Metric::FkFd => t.fk + t.fd,
            _ => self.parts(a).1 as u32,
        }
    }

    /// Minimum sample (`N_min`) for the focus; the reference needs twice as much.
    fn need(self) -> u32 {
        match self {
            Metric::Kd | Metric::Kpr | Metric::Survival | Metric::Multi | Metric::Hs => 5,
            Metric::Win => 3,
            Metric::Clutch => 2,
            Metric::Clutch1v1 | Metric::ClutchMulti => 3,
            Metric::PrimaryHs => 20,
            _ => 10,
        }
    }

    fn k(self) -> f64 {
        match self {
            Metric::Win => K_MATCH,
            Metric::Clutch | Metric::Clutch1v1 | Metric::ClutchMulti => K_CLUTCH,
            Metric::Traded
            | Metric::TradesGiven
            | Metric::AfterDeath
            | Metric::AtkLossTime
            | Metric::EarlyDeath
            | Metric::PrimaryHs
            | Metric::Entry
            | Metric::EntryAtk
            | Metric::EntryDef => K_EVENT,
            _ => K,
        }
    }

    fn enough(self, a: &Agg, focus: bool) -> bool {
        let need = if focus { self.need() } else { self.need() * 2 };
        self.sample(a) >= need
    }

    fn value(self, a: &Agg) -> f64 {
        match self {
            Metric::Kd => ratio(a.t.kills, a.t.deaths),
            Metric::FkFd => ratio(a.t.fk, a.t.fd),
            Metric::Kpr => {
                let (x, n) = self.parts(a);
                if n == 0.0 { 0.0 } else { x / n }
            }
            _ => {
                let (x, n) = self.parts(a);
                if n == 0.0 { 0.0 } else { 100.0 * x / n }
            }
        }
    }

    /// Focus value pulled toward the reference by the smoothing; `None` when it cannot be formed.
    fn smoothed(self, f: &Agg, r: &Agg) -> Option<f64> {
        match self {
            Metric::Kd => {
                if r.t.rounds == 0 {
                    return None;
                }
                let rn = r.t.rounds as f64;
                let n = f.t.rounds as f64;
                let k = adjusted(f.t.kills as f64, n, r.t.kills as f64 / rn);
                let d = adjusted(f.t.deaths as f64, n, r.t.deaths as f64 / rn);
                (d > 0.0).then(|| k / d)
            }
            Metric::FkFd => {
                let (rfk, rfd) = (r.t.fk as f64, r.t.fd as f64);
                if rfk + rfd == 0.0 {
                    return None;
                }
                let n = (f.t.fk + f.t.fd) as f64;
                let share = adjusted(f.t.fk as f64, n, rfk / (rfk + rfd));
                (share < 1.0).then(|| share / (1.0 - share))
            }
            _ => {
                let (x, n) = self.parts(f);
                let (rx, rn) = self.parts(r);
                if rn == 0.0 {
                    return None;
                }
                let s = (x + self.k() * rx / rn) / (n + self.k());
                Some(if self.is_pct() { 100.0 * s } else { s })
            }
        }
    }

    /// What the numbers behind a percentage are, e.g. `won 6 of 16 opening duels`.
    fn phrase(self, f: &Agg) -> String {
        let (x, n) = self.parts(f);
        let (x, n) = (x as u64, n as u64);
        match self {
            Metric::Hs => format!("{} in {}", plural(x, "headshot", "headshots"), plural(n, "kill", "kills")),
            Metric::Win => format!("won {x} of {}", matches_txt(n)),
            Metric::Clutch => format!("won {x} of {}", plural(n, "clutch", "clutches")),
            Metric::Survival => format!("survived {x} of {}", rounds_txt(n)),
            Metric::Multi => format!("{x} of {} with 2+ kills", rounds_txt(n)),
            Metric::AtkWin => format!("won {x} of {}", plural(n, "attack round", "attack rounds")),
            Metric::DefWin => format!("won {x} of {}", plural(n, "defense round", "defense rounds")),
            Metric::Entry | Metric::EntryAtk | Metric::EntryDef => {
                let on = match self {
                    Metric::EntryAtk => " on attack",
                    Metric::EntryDef => " on defense",
                    _ => "",
                };
                format!("won {x} of {}{on}", plural(n, "opening duel", "opening duels"))
            }
            Metric::Traded => format!("{x} of {} traded", plural(n, "death", "deaths")),
            Metric::TradesGiven => {
                format!("traded {x} of {}", plural(n, "teammate death", "teammate deaths"))
            }
            Metric::Kost => format!("kill, survival or trade in {x} of {}", rounds_txt(n)),
            Metric::AfterDeath => {
                format!("team won {x} of the {} you died in", rounds_txt(n))
            }
            Metric::AtkLossTime => {
                format!("{x} of {} ended on time", plural(n, "lost attack round", "lost attack rounds"))
            }
            Metric::PrimaryHs => format!(
                "{} in {}",
                plural(x, "headshot", "headshots"),
                plural(n, "primary-weapon kill", "primary-weapon kills")
            ),
            Metric::EarlyDeath => {
                format!("{x} of {} in the first 30 s", plural(n, "death", "deaths"))
            }
            Metric::Clutch1v1 => format!("won {x} of {}", plural(n, "1v1 clutch", "1v1 clutches")),
            Metric::ClutchMulti => {
                format!("won {x} of {} against 2+", plural(n, "clutch", "clutches"))
            }
            Metric::Kd | Metric::Kpr | Metric::FkFd => String::new(),
        }
    }

    /// One short phrase per direction: (praise title, praise advice, improve title, improve advice).
    fn wording(self) -> (&'static str, &'static str, &'static str, &'static str) {
        match self {
            Metric::Kd => (
                "Winning more duels",
                "You are trading kills for deaths better than usual. Keep taking the same fights.",
                "Losing more duels",
                "Deaths are outpacing kills. Pick fights with a teammate close for a trade and avoid solo peeks.",
            ),
            Metric::Kpr => (
                "More kills every round",
                "You are getting more kills per round than usual.",
                "Fewer kills per round",
                "Your kill output per round is down. Look for earlier information and safer angles.",
            ),
            Metric::Hs => (
                "Sharper aim",
                "A bigger share of your kills are headshots than usual.",
                "Headshot rate dropped",
                "Keep your crosshair at head height and pre-aim common angles before you peek.",
            ),
            Metric::Win => (
                "Winning more matches",
                "Your results are better than your usual win rate.",
                "Fewer wins than usual",
                "Results are below your usual win rate. Check the round-by-round story in the Matches tab.",
            ),
            Metric::FkFd => (
                "Winning the opening duel",
                "You get the first kill more often than the first death. That sets your team up for every round.",
                "Losing the opening duel",
                "You are dying first more often than usual. Let the enemy come to you, or take the first fight with a teammate.",
            ),
            Metric::Clutch => (
                "Clutching more",
                "You convert 1vX situations better than usual.",
                "Clutches are slipping",
                "You are losing more 1vX situations than usual. Take time, use sound and isolate one enemy at a time.",
            ),
            Metric::Survival => (
                "Staying alive longer",
                "You survive more rounds than usual, which keeps your team in the fight.",
                "Dying more often",
                "You survive fewer rounds than usual. Play for the round, not only for the kill.",
            ),
            Metric::Multi => (
                "More multi-kill rounds",
                "You are stacking kills inside a round more often than usual.",
                "Fewer multi-kill rounds",
                "Fewer rounds with two or more kills than usual. Stay on your angle after the first kill.",
            ),
            Metric::AtkWin => (
                "Strong on attack",
                "Your team wins more attacking rounds with you than usual.",
                "Attack rounds are struggling",
                "Attacking rounds are going worse than usual. Check how you enter the site and who is with you.",
            ),
            Metric::DefWin => (
                "Strong on defense",
                "Your team wins more defending rounds with you than usual.",
                "Defense rounds are struggling",
                "Defending rounds are going worse than usual. Check your roam timing and where you set up.",
            ),
            Metric::Entry => (
                "Winning the first fight",
                "You win more opening duels than usual. That sets your team up for every round.",
                "Losing the first fight",
                "You lose more opening duels than usual. Take the first contact with a teammate behind you, or let the enemy come to you.",
            ),
            Metric::EntryAtk => (
                "Strong entries on attack",
                "You win more first fights on attack than usual. Keep taking them with a teammate close.",
                "Entries on attack are costing rounds",
                "Your first contact on attack is going badly. Use drones and utility before the first peek, and take it with a teammate.",
            ),
            Metric::EntryDef => (
                "Winning the first fight on defense",
                "You win more first fights on defense than usual.",
                "Losing the first fight on defense",
                "Attackers win the first fight against you more often than usual. Hold a tighter angle or let a teammate cover you.",
            ),
            Metric::Traded => (
                "Your deaths get traded",
                "More of your deaths are traded than usual. Your positioning keeps the team together.",
                "Deaths are not being traded",
                "Fewer of your deaths are traded than usual. Stay within trading distance of a teammate when you take a fight.",
            ),
            Metric::TradesGiven => (
                "You trade for your team",
                "You punish the killer of a teammate more often than usual.",
                "Teammate deaths go unanswered",
                "You trade fewer teammate deaths than usual. Stay close enough to punish whoever kills a teammate.",
            ),
            Metric::Kost => (
                "More impact every round",
                "In more rounds than usual you get a kill, survive or are traded.",
                "Too many rounds without impact",
                "More rounds end with you dead, with no kill and no trade. Take the early fight only with a plan, or play for survival.",
            ),
            Metric::AfterDeath => (
                "Your team holds on after you fall",
                "Your team wins more of the rounds you die in than usual.",
                "Rounds are lost after you die",
                "Your team wins fewer of the rounds you die in than usual. Check how early those deaths come and whether anyone is close to trade.",
            ),
            Metric::AtkLossTime => (
                "Fewer attacks run out of time",
                "Fewer of your lost attack rounds end on the clock than usual.",
                "Attacks are running out of time",
                "More of your lost attack rounds end on the clock. Plan a faster first contact and plant earlier.",
            ),
            Metric::PrimaryHs => (
                "Primary weapon aim is sharp",
                "More of your primary-weapon kills are headshots than usual.",
                "Primary weapon headshots dropped",
                "Fewer of your primary-weapon kills are headshots. Keep your crosshair at head height before you peek.",
            ),
            Metric::EarlyDeath => (
                "Fewer early deaths",
                "Fewer of your deaths come in the first 30 seconds than usual.",
                "Dying early in the round",
                "More of your deaths come in the first 30 seconds than usual. Consider a slower first peek.",
            ),
            Metric::Clutch1v1 => (
                "Winning 1v1 clutches",
                "You win 1v1 situations more often than usual.",
                "1v1 clutches are slipping",
                "In a 1v1, take your time: use sound and make the enemy come to you.",
            ),
            Metric::ClutchMulti => (
                "Beating several opponents",
                "You turn clutches against two or more enemies into wins more often than usual.",
                "Outnumbered clutches are slipping",
                "Against two or more enemies, isolate one at a time and avoid open fights.",
            ),
        }
    }
}

// ---------------------------------------------------------------- reference selection

fn range_label(r: Range) -> &'static str {
    match r {
        Range::Last1 => "Last match",
        Range::Last5 => "Last 5 matches",
        Range::Last20 => "Last 20 matches",
        Range::Season => "This season",
        Range::All => "All time",
    }
}

/// Reference matches: everything the filter allows (ignoring the range) that is not in the focus.
/// `last5` keeps only the [`PREVIOUS_MATCHES`] newest ones before the focus.
fn reference_set<'a>(
    f: &Filter,
    all: &'a [FullMatch],
    focus: &[&FullMatch],
) -> Vec<&'a FullMatch> {
    let ids: HashSet<&str> = focus.iter().map(|m| m.match_id.as_str()).collect();
    let mut wide = f.clone();
    wide.range = Range::All;
    let mut v: Vec<&FullMatch> =
        apply(&wide, all).into_iter().filter(|m| !ids.contains(m.match_id.as_str())).collect();
    if f.range == Range::Last5 {
        let drop = v.len().saturating_sub(PREVIOUS_MATCHES);
        v.drain(..drop);
    }
    v
}

fn reference_label(f: &Filter, n: u32) -> String {
    match f.range {
        Range::Last5 => format!("Previous {}", matches_txt(n as u64)),
        _ => format!("All your other stored matches ({})", n),
    }
}

// ---------------------------------------------------------------- candidates

/// A significant difference, before selection.
struct Candidate {
    /// Dedupe key: one card per subject.
    key: String,
    family: &'static str,
    kind: InsightKind,
    score: f64,
    /// Sample size behind it (tie-break: larger wins).
    n: u32,
    insight: Insight,
    /// One-line form for the strengths / work-on lists.
    short: String,
    /// Table row for candidates that are not one of the headline metrics.
    row: Option<Comparison>,
}

fn confidence_prefix(n: u32) -> &'static str {
    match Confidence::from_rounds(n) {
        Confidence::Firm => "",
        _ => "Early read: ",
    }
}

fn signed(pct: bool, d: f64) -> String {
    let sign = if d >= 0.0 { "+" } else { "-" };
    if pct {
        format!("{sign}{:.1} pts", d.abs())
    } else {
        format!("{sign}{:.2}", d.abs())
    }
}

fn make_insight(
    kind: InsightKind,
    title: String,
    text: String,
    evidence: String,
    score: f64,
    family: &str,
) -> Insight {
    Insight {
        kind,
        title,
        text,
        evidence,
        operator_key: None,
        map_key: None,
        score: round_to(score, 2),
        family: family.to_string(),
    }
}

/// A headline metric compared between focus and reference.
struct MetricEval {
    row: Comparison,
    cand: Option<Candidate>,
}

fn eval_metric(m: Metric, f: &Agg, r: &Agg, ref_name: &str) -> Option<MetricEval> {
    if !m.enough(f, true) || !m.enough(r, false) {
        return None;
    }
    let (fv, rv) = (m.value(f), m.value(r));
    let n = m.sample(f);
    let places = if m.is_pct() { 1 } else { 2 };
    let mut row = Comparison {
        metric: m.label().to_string(),
        focus: round_to(fv, places),
        reference: round_to(rv, places),
        delta: round_to(fv - rv, places),
        better: if fv == rv { false } else { (fv > rv) == m.higher_better() },
        score: 0.0,
        n,
        unit: if m.is_pct() { "pct" } else { "ratio" }.to_string(),
    };

    let Some(sm) = m.smoothed(f, r) else {
        return Some(MetricEval { row, cand: None });
    };
    let change = sm - rv;
    let passes = if m.is_pct() {
        change.abs() >= PTS_THRESHOLD
    } else {
        rv > 0.0 && change.abs() / rv >= REL_THRESHOLD
    };
    if !passes {
        return Some(MetricEval { row, cand: None });
    }
    let score = importance(change, m.scale(), n, m.need(), m.weight());
    row.score = round_to(score, 2);

    let praise = (change > 0.0) == m.higher_better();
    let (pt, pa, it, ia) = m.wording();
    let prefix_n = if m == Metric::Win { f.t.rounds } else { n };
    let text = format!("{}{}", confidence_prefix(prefix_n), if praise { pa } else { ia });
    let diff = signed(m.is_pct(), fv - rv);
    let evidence = if m.is_pct() {
        format!("{}: {} vs {} {ref_name} ({diff})", m.phrase(f), m.fmt(fv), m.fmt(rv))
    } else {
        format!(
            "{} vs {} {ref_name} ({diff}) over {}",
            m.fmt(fv),
            m.fmt(rv),
            rounds_txt(f.t.rounds as u64)
        )
    };
    let kind = if praise { InsightKind::Praise } else { InsightKind::Improve };
    Some(MetricEval {
        row,
        cand: Some(Candidate {
            key: format!("metric:{}", m.label()),
            family: m.family(),
            kind,
            score,
            n,
            insight: make_insight(
                kind,
                (if praise { pt } else { it }).to_string(),
                text,
                evidence,
                score,
                m.family(),
            ),
            short: format!("{}: {} vs {} usual", m.label(), m.fmt(fv), m.fmt(rv)),
            row: None,
        }),
    })
}

/// A split of one rate (a map and side, a site, a weapon, an enemy operator).
struct Split {
    key: String,
    family: &'static str,
    /// Row label in the table and the short line, e.g. `Defense win % · Bank`.
    label: String,
    praise_title: String,
    praise_text: String,
    improve_title: String,
    improve_text: String,
    /// What the counts are, e.g. `won 4 of 10 defense rounds`.
    phrase: String,
    focus: Rate,
    reference: Rate,
    n_min: u32,
    weight: f64,
    higher_better: bool,
    /// Only keep the card when the change is in the bad direction.
    only_bad: bool,
    k: f64,
    operator_key: Option<String>,
    map_key: Option<String>,
}

fn split_candidate(s: Split, ref_name: &str) -> Option<Candidate> {
    if s.focus.n < s.n_min || s.reference.n < s.n_min * 2 {
        return None;
    }
    let (fv, rv) = (s.focus.pct(), s.reference.pct());
    let sm = 100.0 * (s.focus.x as f64 + s.k * s.reference.x as f64 / s.reference.n as f64)
        / (s.focus.n as f64 + s.k);
    let change = sm - rv;
    if change.abs() < PTS_THRESHOLD {
        return None;
    }
    let praise = (change > 0.0) == s.higher_better;
    if s.only_bad && praise {
        return None;
    }
    let score = importance(change, 10.0, s.focus.n, s.n_min, s.weight);
    let kind = if praise { InsightKind::Praise } else { InsightKind::Improve };
    let text = format!(
        "{}{}",
        confidence_prefix(s.focus.n),
        if praise { &s.praise_text } else { &s.improve_text }
    );
    let evidence = format!(
        "{}: {fv:.1}% vs {rv:.1}% {ref_name} ({})",
        s.phrase,
        signed(true, fv - rv)
    );
    let mut insight = make_insight(
        kind,
        if praise { s.praise_title } else { s.improve_title },
        text,
        evidence,
        score,
        s.family,
    );
    insight.operator_key = s.operator_key;
    insight.map_key = s.map_key;
    Some(Candidate {
        key: s.key,
        family: s.family,
        kind,
        score,
        n: s.focus.n,
        insight,
        short: format!("{}: {fv:.1}% vs {rv:.1}% usual", s.label),
        row: Some(Comparison {
            metric: s.label,
            focus: round_to(fv, 1),
            reference: round_to(rv, 1),
            delta: round_to(fv - rv, 1),
            better: praise,
            score: round_to(score, 2),
            n: s.focus.n,
            unit: "pct".to_string(),
        }),
    })
}

/// Smoothed K/D of a tally against reference kill and death rates per round.
fn smoothed_kd(t: &Tally, ref_kpr: f64, ref_dpr: f64) -> Option<f64> {
    let n = t.rounds as f64;
    let k = adjusted(t.kills as f64, n, ref_kpr);
    let d = adjusted(t.deaths as f64, n, ref_dpr);
    (d > 0.0).then(|| k / d)
}

fn operator_candidates(f: &Agg, r: &Agg, filter: &Filter) -> Vec<Candidate> {
    let mut out = Vec::new();
    if filter.operators.len() == 1 {
        return out; // The headline cards already describe this one operator.
    }
    for (key, t) in &f.by_op {
        if t.rounds < MIN_FOCUS_ROUNDS_SPLIT {
            continue;
        }
        // Reference rates: the operator's own stored history; too few rounds means no card.
        let Some(x) = r.by_op.get(key).filter(|x| x.rounds >= MIN_REF_ROUNDS_SPLIT) else { continue };
        let (kpr, dpr, ref_kd) = (
            x.kills as f64 / x.rounds as f64,
            x.deaths as f64 / x.rounds as f64,
            ratio(x.kills, x.deaths),
        );
        if ref_kd <= 0.0 {
            continue;
        }
        let Some(sm) = smoothed_kd(t, kpr, dpr) else { continue };
        let change = sm - ref_kd;
        if change.abs() / ref_kd < REL_THRESHOLD {
            continue;
        }
        let praise = change > 0.0;
        let name = op_name(key);
        let fkd = ratio(t.kills, t.deaths);
        let score = importance(change, 0.25, t.rounds, MIN_FOCUS_ROUNDS_SPLIT, 0.8);
        let kind = if praise { InsightKind::Praise } else { InsightKind::Improve };
        let mut insight = make_insight(
            kind,
            if praise { format!("{name} is working") } else { format!("{name} is struggling") },
            format!(
                "{}{}",
                confidence_prefix(t.rounds),
                if praise {
                    format!("You are doing better on {name} than usual. Keep it in your rotation.")
                } else {
                    format!("You are doing worse on {name} than usual. Review how you set up and consider a different pick for a while.")
                }
            ),
            format!(
                "K/D {:.2} vs {:.2} in your history ({}) over {}",
                fkd,
                ref_kd,
                signed(false, fkd - ref_kd),
                rounds_txt(t.rounds as u64)
            ),
            score,
            "operator",
        );
        insight.operator_key = Some(key.clone());
        out.push(Candidate {
            key: format!("op:{key}"),
            family: "operator",
            kind,
            score,
            n: t.rounds,
            insight,
            short: format!("{name}: K/D {:.2} vs {:.2} usual", fkd, ref_kd),
            row: Some(Comparison {
                metric: format!("{name} K/D"),
                focus: round_to(fkd, 2),
                reference: round_to(ref_kd, 2),
                delta: round_to(fkd - ref_kd, 2),
                better: praise,
                score: round_to(score, 2),
                n: t.rounds,
                unit: "ratio".to_string(),
            }),
        });
    }
    out
}

fn map_candidates(f: &Agg, r: &Agg, filter: &Filter) -> Vec<Candidate> {
    let mut out = Vec::new();
    if filter.maps.len() == 1 {
        return out;
    }
    for (key, t) in &f.by_map {
        if t.rounds < MIN_FOCUS_ROUNDS_SPLIT {
            continue;
        }
        let Some(x) = r.by_map.get(key).filter(|x| x.rounds >= MIN_REF_ROUNDS_SPLIT) else { continue };
        let ref_rate = x.rounds_won as f64 / x.rounds as f64;
        let sm = adjusted(t.rounds_won as f64, t.rounds as f64, ref_rate);
        let change = 100.0 * (sm - ref_rate);
        if change.abs() < PTS_THRESHOLD {
            continue;
        }
        let praise = change > 0.0;
        let name = map_name(key);
        let fr = 100.0 * t.rounds_won as f64 / t.rounds as f64;
        let rr = 100.0 * ref_rate;
        let score = importance(change, 10.0, t.rounds, MIN_FOCUS_ROUNDS_SPLIT, 0.8);
        let kind = if praise { InsightKind::Praise } else { InsightKind::Improve };
        let mut insight = make_insight(
            kind,
            if praise { format!("Strong on {name}") } else { format!("{name} needs work") },
            format!(
                "{}{}",
                confidence_prefix(t.rounds),
                if praise {
                    format!("Your team wins more rounds on {name} than usual with you in the game.")
                } else {
                    format!("Rounds on {name} are going worse than usual. Rewatch the losses and check the angles you took.")
                }
            ),
            format!(
                "won {} of {}: {fr:.1}% vs {rr:.1}% in your history ({})",
                t.rounds_won,
                rounds_txt(t.rounds as u64),
                signed(true, fr - rr)
            ),
            score,
            "map",
        );
        insight.map_key = Some(key.clone());
        out.push(Candidate {
            key: format!("map:{key}"),
            family: "map",
            kind,
            score,
            n: t.rounds,
            insight,
            short: format!("{name}: {fr:.1}% round wins vs {rr:.1}% usual"),
            row: Some(Comparison {
                metric: format!("{name} round win %"),
                focus: round_to(fr, 1),
                reference: round_to(rr, 1),
                delta: round_to(fr - rr, 1),
                better: praise,
                score: round_to(score, 2),
                n: t.rounds,
                unit: "pct".to_string(),
            }),
        });
    }
    out
}

/// G4 and G8: results per map and side, and per defense site.
fn side_candidates(f: &Agg, r: &Agg, filter: &Filter, ref_name: &str) -> Vec<Candidate> {
    let mut out = Vec::new();
    if filter.side != SideFilter::Both {
        return out; // The filter already picks the side.
    }
    for ((map, side), fr) in &f.deep.map_side {
        let rr = r.deep.map_side.get(&(map.clone(), *side)).copied().unwrap_or_default();
        let (side_word, side_name) = if *side == 0 { ("attack", "Attack") } else { ("defense", "Defense") };
        let name = map_name(map);
        let s = Split {
            key: format!("mapside:{map}:{side}"),
            family: "side",
            label: format!("{side_name} win % · {name}"),
            praise_title: format!("Strong on {name} {side_word}"),
            praise_text: format!("Your team wins more {side_word} rounds on {name} than usual with you in the game."),
            improve_title: format!("{name} {side_word} needs work"),
            improve_text: format!("{side_name} rounds on {name} are going worse than usual. Check your setups and routes on this map."),
            phrase: format!("won {} of {}", fr.x, plural(fr.n as u64, &format!("{side_word} round on {name}"), &format!("{side_word} rounds on {name}"))),
            focus: *fr,
            reference: rr,
            n_min: MIN_SIDE_ROUNDS,
            weight: 1.2,
            higher_better: true,
            only_bad: false,
            k: K,
            operator_key: None,
            map_key: Some(map.clone()),
        };
        out.extend(split_candidate(s, ref_name));
    }
    for ((map, site), fr) in &f.deep.site {
        let rr = r.deep.site.get(&(map.clone(), site.clone())).copied().unwrap_or_default();
        let name = map_name(map);
        let s = Split {
            key: format!("site:{map}:{site}"),
            family: "site",
            label: format!("Defense win % · {name} · {site}"),
            praise_title: format!("Strong defending {site}"),
            praise_text: format!("Your team holds this site on {name} more often than usual."),
            improve_title: format!("Defending {site} needs work"),
            improve_text: format!("Your team loses this site on {name} more often than usual. Review the setup you play there."),
            phrase: format!("won {} of {}", fr.x, plural(fr.n as u64, &format!("defense round at {site}"), &format!("defense rounds at {site}"))),
            focus: *fr,
            reference: rr,
            n_min: MIN_SIDE_ROUNDS,
            weight: 1.0,
            higher_better: true,
            only_bad: false,
            k: K,
            operator_key: None,
            map_key: Some(map.clone()),
        };
        out.extend(split_candidate(s, ref_name));
    }
    out
}

/// G7: headshot rate per weapon.
fn weapon_candidates(f: &Agg, r: &Agg, ref_name: &str) -> Vec<Candidate> {
    let mut out = Vec::new();
    for (name, w) in &f.deep.weapons {
        let Some(rw) = r.deep.weapons.get(name) else { continue };
        let s = Split {
            key: format!("weapon:{name}"),
            family: "aim",
            label: format!("{name} HS %"),
            praise_title: format!("Sharp with the {name}"),
            praise_text: format!("More of your {name} kills are headshots than usual."),
            improve_title: format!("Fewer headshots with the {name}"),
            improve_text: format!("Fewer of your {name} kills are headshots than usual. Check your sight height and pre-aim."),
            phrase: format!("{} in {} with the {name}", plural(w.headshots as u64, "headshot", "headshots"), plural(w.kills as u64, "kill", "kills")),
            focus: Rate { x: w.headshots, n: w.kills },
            reference: Rate { x: rw.headshots, n: rw.kills },
            n_min: MIN_WEAPON_KILLS,
            weight: 1.0,
            higher_better: true,
            only_bad: false,
            k: K_EVENT,
            operator_key: None,
            map_key: None,
        };
        out.extend(split_candidate(s, ref_name));
    }
    out
}

/// G10: enemy operators that kill the player more often than usual (counts per operator, never
/// per player).
fn nemesis_candidates(f: &Agg, r: &Agg, ref_name: &str) -> Vec<Candidate> {
    let mut out = Vec::new();
    for (key, deaths) in &f.deep.deaths_by_op {
        if *deaths < MIN_NEMESIS_DEATHS {
            continue;
        }
        let faced = f.deep.faced_by_op.get(key).copied().unwrap_or(0);
        let rfaced = r.deep.faced_by_op.get(key).copied().unwrap_or(0);
        if faced < *deaths || rfaced < MIN_NEMESIS_FACED {
            continue;
        }
        let rdeaths = r.deep.deaths_by_op.get(key).copied().unwrap_or(0);
        let name = op_name(key);
        let s = Split {
            key: format!("nemesis:{key}"),
            family: "nemesis",
            label: format!("Deaths against {name} %"),
            praise_title: format!("{name} is not a problem"),
            praise_text: String::new(),
            improve_title: format!("{name} keeps killing you"),
            improve_text: format!("Enemy {name} kills you more often than usual. Check where it gets you and how you hold that angle."),
            phrase: format!("killed by {name} in {} of {}", deaths, plural(faced as u64, "round", "rounds")),
            focus: Rate { x: *deaths, n: faced },
            reference: Rate { x: rdeaths, n: rfaced },
            // `n_min` gates on trials, so use the deaths gate above and a low trial gate here.
            n_min: MIN_NEMESIS_DEATHS,
            weight: 0.6,
            higher_better: false,
            only_bad: true,
            k: K_EVENT,
            operator_key: Some(key.clone()),
            map_key: None,
        };
        out.extend(split_candidate(s, ref_name));
    }
    out
}

// ---------------------------------------------------------------- info cards

fn info(title: &str, text: String, evidence: String) -> Insight {
    Insight {
        kind: InsightKind::Info,
        title: title.to_string(),
        text,
        evidence,
        operator_key: None,
        map_key: None,
        score: 0.0,
        family: "info".to_string(),
    }
}

fn info_cards(f: &Agg, label: &str) -> Vec<Insight> {
    let mut out = Vec::new();
    let t = &f.t;
    let losses = f.matches - f.wins;
    out.push(info(
        "Result",
        format!(
            "{label}: {} won, {} lost.",
            f.wins, losses
        ),
        format!(
            "{}, {} over {} in {}",
            plural(t.kills as u64, "kill", "kills"),
            plural(t.deaths as u64, "death", "deaths"),
            rounds_txt(t.rounds as u64),
            matches_txt(f.matches as u64)
        ),
    ));

    // Most played operator.
    if let Some((key, ot)) = f.by_op.iter().max_by_key(|(k, o)| (o.rounds, std::cmp::Reverse((*k).clone()))) {
        if ot.rounds >= 3 && f.by_op.len() > 1 {
            let name = op_name(key);
            let mut card = info(
                "Most played operator",
                format!("{name} was your most played operator in this stretch."),
                format!(
                    "{} on {name}, K/D {:.2}",
                    rounds_txt(ot.rounds as u64),
                    ratio(ot.kills, ot.deaths)
                ),
            );
            card.operator_key = Some(key.clone());
            out.push(card);
        }
    }

    // Best match by K/D (only meaningful with a few matches to choose from).
    if f.per_match.len() >= 2 {
        if let Some(best) = f
            .per_match
            .iter()
            .filter(|m| m.total.rounds >= 4)
            .max_by(|a, b| {
                ratio(a.total.kills, a.total.deaths)
                    .partial_cmp(&ratio(b.total.kills, b.total.deaths))
                    .unwrap_or(std::cmp::Ordering::Equal)
                    .then_with(|| b.id.cmp(&a.id))
            })
        {
            let name = map_name(&best.map_key);
            let mut card = info(
                "Best match",
                format!(
                    "Your best match was on {name}, which you {}.",
                    if best.won { "won" } else { "lost" }
                ),
                format!(
                    "{}, {} (K/D {:.2}) over {}",
                    plural(best.total.kills as u64, "kill", "kills"),
                    plural(best.total.deaths as u64, "death", "deaths"),
                    ratio(best.total.kills, best.total.deaths),
                    rounds_txt(best.total.rounds as u64)
                ),
            );
            card.map_key = Some(best.map_key.clone());
            out.push(card);
        }
    }

    // Small-sample reminder.
    if t.rounds < 20 {
        out.push(info(
            "Small sample",
            "This is a short stretch, so treat the comparisons as hints, not verdicts.".to_string(),
            format!("Based on {}", rounds_txt(t.rounds as u64)),
        ));
    }
    out
}

// ---------------------------------------------------------------- report

/// Builds the coach report for `filter`. `all` is every known match (any order).
pub fn coach(filter: &Filter, all: &[FullMatch]) -> CoachReport {
    let focus_set = apply(filter, all);
    let focus_label = {
        let mut s = range_label(filter.range).to_string();
        match filter.mode {
            Mode::All => {}
            Mode::Ranked => s.push_str(" · Ranked"),
            Mode::Unranked => s.push_str(" · Unranked"),
            Mode::Quick => s.push_str(" · Quick Match"),
        }
        s
    };
    let f = Agg::build(filter, &focus_set);
    if f.matches == 0 {
        return CoachReport {
            summary: "No matches fit this filter yet, so there is nothing to coach on.".to_string(),
            focus_label,
            ..CoachReport::default()
        };
    }

    let ref_set = reference_set(filter, all, &focus_set);
    let r = Agg::build(filter, &ref_set);
    let ref_name = "usual";

    let reference_label_text = reference_label(filter, r.matches);
    let mut comparisons: Vec<Comparison> = Vec::new();
    let mut cands: Vec<Candidate> = Vec::new();

    if r.t.rounds > 0 {
        for m in METRICS {
            // Per-side numbers only make sense when the filter does not already pick a side.
            if filter.side != SideFilter::Both
                && matches!(m, Metric::AtkWin | Metric::DefWin | Metric::EntryAtk | Metric::EntryDef)
            {
                continue;
            }
            // Attack-only metrics under a defense filter (and the reverse) have no sample anyway.
            let Some(ev) = eval_metric(m, &f, &r, ref_name) else { continue };
            comparisons.push(ev.row);
            cands.extend(ev.cand);
        }
        cands.extend(operator_candidates(&f, &r, filter));
        cands.extend(map_candidates(&f, &r, filter));
        cands.extend(side_candidates(&f, &r, filter, ref_name));
        cands.extend(weapon_candidates(&f, &r, ref_name));
        cands.extend(nemesis_candidates(&f, &r, ref_name));
        comparisons.extend(cands.iter().filter_map(|c| c.row.clone()));
    }

    // Most important first; ties go to the larger sample, then to the key.
    comparisons.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| b.n.cmp(&a.n))
            .then_with(|| a.metric.cmp(&b.metric))
    });
    cands.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| b.n.cmp(&a.n))
            .then_with(|| a.key.cmp(&b.key))
    });

    // One card per subject and one per family.
    let mut seen: HashSet<String> = HashSet::new();
    let mut families: HashSet<&'static str> = HashSet::new();
    let mut unique: Vec<Candidate> = Vec::new();
    for c in cands {
        if seen.insert(c.key.clone()) && families.insert(c.family) {
            unique.push(c);
        }
    }

    let strengths: Vec<String> = unique
        .iter()
        .filter(|c| c.kind == InsightKind::Praise)
        .take(MAX_STRENGTHS)
        .map(|c| c.short.clone())
        .collect();
    let work_on: Vec<String> = unique
        .iter()
        .filter(|c| c.kind == InsightKind::Improve)
        .take(MAX_WORK_ON)
        .map(|c| c.short.clone())
        .collect();

    let mut insights: Vec<Insight> = unique.into_iter().take(MAX_SIGNAL).map(|c| c.insight).collect();
    insights.extend(info_cards(&f, &focus_label));

    let summary = summarize_report(&f, &r, &comparisons, &strengths, &work_on, &focus_label);

    CoachReport {
        summary,
        insights,
        strengths,
        work_on,
        comparisons,
        focus_label,
        reference_label: reference_label_text,
    }
}

fn summarize_report(
    f: &Agg,
    r: &Agg,
    comparisons: &[Comparison],
    strengths: &[String],
    work_on: &[String],
    label: &str,
) -> String {
    let mut s = format!(
        "{label}: {} won, {} lost, K/D {:.2} over {}.",
        f.wins,
        f.matches - f.wins,
        ratio(f.t.kills, f.t.deaths),
        rounds_txt(f.t.rounds as u64)
    );
    if let Some(c) = comparisons.iter().find(|c| c.metric == "K/D") {
        let word = if c.delta > 0.0 { "up" } else if c.delta < 0.0 { "down" } else { "level" };
        if word == "level" {
            s.push_str(" K/D is level with your usual.");
        } else {
            s.push_str(&format!(" K/D is {word} {:.2} on your usual.", c.delta.abs()));
        }
    } else if r.t.rounds == 0 {
        s.push_str(" There is no earlier history under this filter to compare with yet.");
    }
    if !strengths.is_empty() && !work_on.is_empty() {
        s.push_str(" There are things to build on and things to fix; see below.");
    } else if !strengths.is_empty() {
        s.push_str(" Nothing stands out as a problem.");
    } else if !work_on.is_empty() {
        s.push_str(" A few things are worth working on.");
    } else {
        s.push_str(" Nothing stands out either way.");
    }
    s
}

// ---------------------------------------------------------------- plain-text view (CLI)

/// Text form of the report for the dev CLI.
pub fn format_text(r: &CoachReport) -> String {
    let mut out = String::new();
    out.push_str(&format!("{}\n  vs {}\n\n{}\n\n", r.focus_label, r.reference_label, r.summary));
    for i in &r.insights {
        let tag = match i.kind {
            InsightKind::Praise => "+",
            InsightKind::Improve => "!",
            InsightKind::Info => "i",
        };
        out.push_str(&format!("[{tag}] {}  (score {:.2}, {})\n    {}\n    ({})\n", i.title, i.score, i.family, i.text, i.evidence));
    }
    if !r.strengths.is_empty() {
        out.push_str("\nStrengths:\n");
        for s in &r.strengths {
            out.push_str(&format!("  - {s}\n"));
        }
    }
    if !r.work_on.is_empty() {
        out.push_str("\nWork on:\n");
        for s in &r.work_on {
            out.push_str(&format!("  - {s}\n"));
        }
    }
    if !r.comparisons.is_empty() {
        out.push_str(&format!("\n{:<36}{:>9}{:>11}{:>9}{:>7}{:>6}\n", "Metric (ranked)", "Focus", "Reference", "Delta", "Score", "n"));
        for c in &r.comparisons {
            out.push_str(&format!(
                "{:<36}{:>9}{:>11}{:>+9}{:>7.2}{:>6}  {}\n",
                c.metric,
                c.focus,
                c.reference,
                c.delta,
                c.score,
                c.n,
                if c.better { "better" } else { "worse" }
            ));
        }
    }
    out
}


#[cfg(test)]
mod tests {
    use super::*;
    use crate::full::{KillEvent, PlayerInfo, WinCondition};

    fn rp(id: &str, team: usize, op: u64, side: Side, kills: u32, died: bool) -> RoundPlayer {
        RoundPlayer {
            profile_id: id.into(),
            team_index: team,
            operator_id: op,
            side,
            kills,
            deaths: died as u32,
            headshots: kills / 2,
            first_kill: false,
            first_death: false,
            survived: !died,
        }
    }

    /// A match of `n` rounds where "me" gets `kills` kills in each and dies in `deaths_rounds` of
    /// them; me always wins when `win_rounds` covers the round.
    fn mk(id: &str, ts: &str, n: u32, kills: u32, deaths_rounds: u32, win_rounds: u32) -> FullMatch {
        let rounds = (1..=n)
            .map(|i| FullRound {
                number: i,
                start_time_ms: 0,
                winner_team: Some(if i <= win_rounds { 0 } else { 1 }),
                win_condition: WinCondition::Elimination,
                site: String::new(),
                my_side: if i % 2 == 0 { Side::Attack } else { Side::Defense },
                players: vec![
                    rp("me", 0, 1, if i % 2 == 0 { Side::Attack } else { Side::Defense }, kills, i <= deaths_rounds),
                    rp("mate", 0, 2, Side::Attack, 0, true),
                    rp("foe1", 1, 3, Side::Defense, 0, false),
                    rp("foe2", 1, 4, Side::Defense, 0, false),
                ],
                kills: vec![],
            })
            .collect();
        FullMatch {
            match_id: id.into(),
            timestamp: ts.into(),
            match_type: "Ranked".into(),
            gamemode: "Bomb".into(),
            map_id: 1,
            score_us: win_rounds,
            score_them: n - win_rounds,
            won: win_rounds * 2 > n,
            me_profile_id: "me".into(),
            players: vec![
                PlayerInfo { profile_id: "me".into(), username: "Me".into(), team_index: 0 },
                PlayerInfo { profile_id: "mate".into(), username: "Mate".into(), team_index: 0 },
                PlayerInfo { profile_id: "foe1".into(), username: "Foe1".into(), team_index: 1 },
                PlayerInfo { profile_id: "foe2".into(), username: "Foe2".into(), team_index: 1 },
            ],
            rounds,
            season: String::new(),
        }
    }

    fn history() -> Vec<FullMatch> {
        // 8 older matches: 1 kill/round, dies in 8 of 10 rounds, K/D 1.25.
        let mut v: Vec<FullMatch> =
            (1..=8).map(|i| mk(&format!("o{i}"), &format!("2026-10-0{i}T20:00:00"), 10, 1, 8, 5)).collect();
        // Newest match: 2 kills/round, one death: much better.
        v.push(mk("new", "2026-10-20T20:00:00", 10, 2, 1, 8));
        v
    }

    fn filter(range: Range) -> Filter {
        Filter { range, ..Filter::default() }
    }

    #[test]
    fn last_match_is_compared_with_the_others() {
        let all = history();
        let rep = coach(&filter(Range::Last1), &all);
        assert_eq!(rep.focus_label, "Last match");
        assert_eq!(rep.reference_label, "All your other stored matches (8)");
        let kd = rep.comparisons.iter().find(|c| c.metric == "K/D").unwrap();
        assert_eq!(kd.focus, 20.0); // 20 kills, 1 death
        assert_eq!(kd.reference, 1.25);
        assert!(kd.better);
        assert!(rep.insights.iter().any(|i| i.kind == InsightKind::Praise));
        assert!(!rep.strengths.is_empty());
        assert!(rep.insights.len() >= 3);
    }

    #[test]
    fn last5_uses_previous_matches_and_respects_cap() {
        let all = history();
        let rep = coach(&filter(Range::Last5), &all);
        assert_eq!(rep.focus_label, "Last 5 matches");
        assert_eq!(rep.reference_label, "Previous 4 matches");
    }

    #[test]
    fn singular_plural_and_no_duplicates() {
        assert_eq!(matches_txt(1), "1 match");
        assert_eq!(rounds_txt(2), "2 rounds");
        let all = history();
        let rep = coach(&filter(Range::Last1), &all);
        let mut titles: Vec<&str> = rep.insights.iter().map(|i| i.title.as_str()).collect();
        titles.sort();
        let n = titles.len();
        titles.dedup();
        assert_eq!(n, titles.len());
        assert!(rep.strengths.len() <= MAX_STRENGTHS && rep.work_on.len() <= MAX_WORK_ON);
    }

    #[test]
    fn thin_history_makes_no_comparisons() {
        let all = vec![mk("a", "2026-10-01T20:00:00", 10, 1, 5, 5)];
        let rep = coach(&filter(Range::Last1), &all);
        assert!(rep.comparisons.is_empty());
        assert!(rep.strengths.is_empty() && rep.work_on.is_empty());
        assert!(rep.summary.contains("no earlier history") || rep.summary.contains("Nothing"));
        // Still reports the result honestly.
        assert!(rep.insights.iter().any(|i| i.title == "Result"));
    }

    #[test]
    fn empty_filter_result_is_handled() {
        let all = history();
        let f = Filter { maps: vec!["no-such-map".into()], ..Filter::default() };
        let rep = coach(&f, &all);
        assert!(rep.insights.is_empty() && rep.comparisons.is_empty());
        assert!(rep.summary.contains("No matches"));
    }

    #[test]
    fn serializes_to_the_ts_contract() {
        let all = history();
        let rep = coach(&filter(Range::Last1), &all);
        let v = serde_json::to_value(&rep).unwrap();
        for k in ["summary", "insights", "strengths", "workOn", "comparisons", "focusLabel", "referenceLabel"] {
            assert!(v.get(k).is_some(), "missing {k}");
        }
        let i = &v["insights"][0];
        for k in ["kind", "title", "text", "evidence", "score", "family"] {
            assert!(i.get(k).is_some(), "missing insight.{k}");
        }
        let c = &v["comparisons"][0];
        for k in ["metric", "focus", "reference", "delta", "better", "score", "n", "unit"] {
            assert!(c.get(k).is_some(), "missing comparison.{k}");
        }
    }

    #[test]
    fn importance_follows_the_audit_formula() {
        // Audit worked example: opening duels, -14 pts over 16 duels, N_min 10, W 1.5.
        let s = importance(-14.0, 10.0, 16, 10, 1.5);
        assert!((s - 0.923).abs() < 0.01, "{s}");
        // K/D -0.40 over 25 rounds, N_min 10, W 0.8.
        let s = importance(-0.40, 0.25, 25, 10, 0.8);
        assert!((s - 0.571).abs() < 0.01, "{s}");
    }

    #[test]
    fn cards_are_ranked_by_score_with_one_card_per_family() {
        let all = history();
        let rep = coach(&filter(Range::Last1), &all);
        let signals: Vec<&Insight> = rep.insights.iter().filter(|i| i.kind != InsightKind::Info).collect();
        assert!(!signals.is_empty());
        for w in signals.windows(2) {
            assert!(w[0].score >= w[1].score, "not sorted: {} < {}", w[0].score, w[1].score);
        }
        let mut fams: Vec<&str> = signals.iter().map(|i| i.family.as_str()).collect();
        let n = fams.len();
        fams.sort();
        fams.dedup();
        assert_eq!(n, fams.len(), "two cards of one family");
        // Comparisons are ranked as well.
        for w in rep.comparisons.windows(2) {
            assert!(w[0].score >= w[1].score);
        }
    }

    // ---- new metrics

    /// One round where I take the first fight (`entry`), lose or win by `won`.
    fn duel_round(i: u32, side: Side, first_kill: bool, first_death: bool, won: bool) -> FullRound {
        let mut me = rp("me", 0, 1, side, first_kill as u32, first_death);
        me.first_kill = first_kill;
        me.first_death = first_death;
        let mut foe = rp("foe1", 1, 3, if side == Side::Attack { Side::Defense } else { Side::Attack }, 0, first_kill);
        foe.first_death = first_kill;
        foe.first_kill = first_death;
        let (killer, victim) = if first_kill { ("me", "foe1") } else { ("foe1", "me") };
        FullRound {
            number: i,
            start_time_ms: 0,
            winner_team: Some(if won { 0 } else { 1 }),
            win_condition: WinCondition::Elimination,
            site: String::new(),
            my_side: side,
            players: vec![me, rp("mate", 0, 2, side, 0, false), foe, rp("foe2", 1, 4, Side::Defense, 0, false)],
            kills: vec![KillEvent {
                time_s: 160,
                killer: killer.into(),
                victim: victim.into(),
                headshot: false,
                weapon_id: None,
                weapon: None,
                weapon_slot: None,
            }],
        }
    }

    fn duel_match(id: &str, ts: &str, wins_of_20: u32) -> FullMatch {
        let mut m = mk(id, ts, 1, 0, 0, 1);
        m.rounds = (1..=20)
            .map(|i| duel_round(i, Side::Attack, i <= wins_of_20, i > wins_of_20, i % 2 == 0))
            .collect();
        m
    }

    #[test]
    fn opening_duels_are_compared_and_respect_the_sample_gate() {
        // History: 10 of 20 opening duels won per match (50%). Focus: 4 of 20 (20%).
        let mut all: Vec<FullMatch> =
            (1..=3).map(|i| duel_match(&format!("o{i}"), &format!("2026-10-0{i}T20:00:00"), 10)).collect();
        all.push(duel_match("new", "2026-10-20T20:00:00", 4));
        let rep = coach(&filter(Range::Last1), &all);
        let row = rep.comparisons.iter().find(|c| c.metric == "Opening duel win %").unwrap();
        assert_eq!((row.focus, row.reference, row.n), (20.0, 50.0, 20));
        assert!(!row.better && row.score > 0.0);
        let card = rep.insights.iter().find(|i| i.family == "duel").unwrap();
        assert_eq!(card.kind, InsightKind::Improve);
        assert!(rep.work_on.iter().any(|w| w.contains("FK") || w.contains("duel")));

        // Too few duels in the focus: no row at all.
        let mut thin = all.clone();
        thin.last_mut().unwrap().rounds.truncate(4);
        let rep = coach(&filter(Range::Last1), &thin);
        assert!(rep.comparisons.iter().all(|c| c.metric != "Opening duel win %"));
    }

    #[test]
    fn side_filter_hides_per_side_metrics() {
        let mut all: Vec<FullMatch> =
            (1..=3).map(|i| duel_match(&format!("o{i}"), &format!("2026-10-0{i}T20:00:00"), 10)).collect();
        all.push(duel_match("new", "2026-10-20T20:00:00", 4));
        let f = Filter { range: Range::Last1, side: SideFilter::Attack, ..Filter::default() };
        let rep = coach(&f, &all);
        assert!(rep.comparisons.iter().all(|c| !c.metric.contains("on attack") && c.metric != "Attack round win %"));
        assert!(rep.comparisons.iter().any(|c| c.metric == "Opening duel win %"));
    }

    #[test]
    fn lower_is_better_metrics_flip_the_direction() {
        assert!(!Metric::EarlyDeath.higher_better() && !Metric::AtkLossTime.higher_better());
        assert!(Metric::Traded.higher_better());
        let _ = Metric::Traded.wording();
    }
}
