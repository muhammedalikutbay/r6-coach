//! Coach v2 (spec section 5): compares a focus set of matches with a reference set and turns
//! the differences into a `CoachReport`.
//!
//! * Focus = `apply(filter)`. The Coach view sends `range = last1 | last5`; other ranges work too.
//! * Reference = the same filter without the range, minus the focus: every other match for
//!   `last1`, the 20 matches before the focus for `last5`. When that is too thin the career
//!   counters of the Tracker baseline stand in for the headline metrics.
//! * Everything is judged on smoothed rates (`stats::adjusted`) so a short streak cannot
//!   produce firm advice, and every metric has a minimum sample size.
//! * Honest by construction: there are no weapon or assist claims (the parser has neither).

use crate::baseline::{Baseline, CounterSet};
use crate::derive::round_clutches;
use crate::full::{FullMatch, FullRound, RoundPlayer};
use crate::model::Side;
use crate::names;
use crate::query::{apply, Filter, Mode, Range, SideFilter};
use crate::stats::{adjusted, Confidence, K, REL_THRESHOLD};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashSet};

/// Matches before the focus that form the reference for `last5`.
pub const PREVIOUS_MATCHES: usize = 20;
/// Smoothing strength for match-level rates (a match is worth far more than a round).
const K_MATCH: f64 = 3.0;
/// Smoothing strength for clutch rates (clutch situations are rare).
const K_CLUTCH: f64 = 3.0;
/// Rounds an operator or map needs inside the focus before it gets its own card.
const MIN_FOCUS_ROUNDS_SPLIT: u32 = 6;
/// Rounds an operator or map needs in the reference before it can be compared.
const MIN_REF_ROUNDS_SPLIT: u32 = 12;
/// Rounds in the baseline before an operator or map counts as history.
const MIN_BASELINE_ROUNDS: u64 = 30;
/// Significant insight cards (praise or improve) that are shown at most.
const MAX_SIGNAL: usize = 4;
/// Insight cards in total.
const MAX_INSIGHTS: usize = 6;

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
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Comparison {
    pub metric: String,
    pub focus: f64,
    pub reference: f64,
    pub delta: f64,
    pub better: bool,
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
        Ok(id) => format!("Map #{id}"),
        Err(_) => key.to_string(),
    })
}
fn norm_slug(s: &str) -> String {
    s.chars().filter(|c| c.is_ascii_alphanumeric()).map(|c| c.to_ascii_lowercase()).collect()
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

/// The tracked player's entry in a round if it passes the side and operator filters
/// (same rule as the query engine).
fn selected<'a>(f: &Filter, round: &'a FullRound, me: &str) -> Option<&'a RoundPlayer> {
    let p = round.players.iter().find(|p| p.profile_id == me)?;
    match f.side {
        SideFilter::Both => {}
        SideFilter::Attack if p.side != Side::Attack => return None,
        SideFilter::Defense if p.side != Side::Defense => return None,
        _ => {}
    }
    if !f.operators.is_empty() {
        let key = op_key(p.operator_id);
        if !f.operators.iter().any(|k| k.eq_ignore_ascii_case(&key)) {
            return None;
        }
    }
    Some(p)
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
}

const METRICS: [Metric; 10] = [
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
        }
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
        let (x, n) = match self {
            Metric::Kpr => (t.kills, t.rounds),
            Metric::Hs => (t.headshots, t.kills),
            Metric::Win => (a.wins, a.matches),
            Metric::Clutch => (t.clutch_won, t.clutch_won + t.clutch_lost),
            Metric::Survival => (t.survived, t.rounds),
            Metric::Multi => (t.multi, t.rounds),
            Metric::AtkWin => (t.atk_won, t.atk_rounds),
            Metric::DefWin => (t.def_won, t.def_rounds),
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
            Metric::Clutch => t.clutch_won + t.clutch_lost,
            Metric::AtkWin => t.atk_rounds,
            Metric::DefWin => t.def_rounds,
        }
    }

    /// Minimum sample for the focus; the reference needs twice as much.
    fn need(self) -> u32 {
        match self {
            Metric::Kd | Metric::Kpr | Metric::Survival | Metric::Multi | Metric::Hs => 5,
            Metric::Win => 3,
            Metric::FkFd => 4,
            Metric::Clutch => 2,
            Metric::AtkWin | Metric::DefWin => 6,
        }
    }

    fn k(self) -> f64 {
        match self {
            Metric::Win => K_MATCH,
            Metric::Clutch => K_CLUTCH,
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

/// Counters of the Tracker baseline that stand for "career" under a filter, with a label.
fn career_counters<'a>(f: &Filter, b: &'a Baseline) -> Option<(&'a CounterSet, String)> {
    if f.side != SideFilter::Both || !f.maps.is_empty() || f.operators.len() > 1 {
        return None;
    }
    if let Some(op) = f.operators.first() {
        if f.mode != Mode::All {
            return None;
        }
        let key = op.to_ascii_lowercase();
        return b.operators.get(&key).map(|c| (c, format!("{} career", op_name(&key))));
    }
    match f.mode {
        Mode::All => Some((&b.overall, "career".to_string())),
        Mode::Ranked => b.gamemodes.get("pvp_ranked").map(|c| (c, "ranked career".to_string())),
        Mode::Unranked | Mode::Quick => {
            b.gamemodes.get("pvp_quickplay").map(|c| (c, "casual career".to_string()))
        }
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
        Range::Last1 => format!("Your other {}", matches_txt(n as u64)),
        _ => format!("Your other {}", matches_txt(n as u64)),
    }
}

// ---------------------------------------------------------------- candidates

/// A significant difference, before selection.
struct Candidate {
    /// Dedupe key: one card per subject.
    key: String,
    kind: InsightKind,
    score: f64,
    insight: Insight,
    /// One-line form for the strengths / work-on lists.
    short: String,
}

fn confidence_prefix(rounds: u32) -> &'static str {
    match Confidence::from_rounds(rounds) {
        Confidence::Firm => "",
        _ => "Early read: ",
    }
}

fn signed(metric: Metric, d: f64) -> String {
    let sign = if d >= 0.0 { "+" } else { "-" };
    if metric.is_pct() {
        format!("{sign}{:.1} pts", d.abs())
    } else {
        format!("{sign}{:.2}", d.abs())
    }
}

/// Candidate from one headline metric.
fn metric_candidate(m: Metric, f: &Agg, r: &Agg, ref_name: &str) -> Option<Candidate> {
    if !m.enough(f, true) || !m.enough(r, false) {
        return None;
    }
    let rv = m.value(r);
    if rv <= 0.0 {
        return None;
    }
    let sm = m.smoothed(f, r)?;
    let rel = (sm - rv) / rv;
    if rel.abs() < REL_THRESHOLD {
        return None;
    }
    let conf = m.sample(f) as f64 / (m.sample(f) as f64 + m.k());
    let (pt, pa, it, ia) = m.wording();
    let praise = rel > 0.0;
    let fv = m.value(f);
    let text = format!("{}{}", confidence_prefix(f.t.rounds), if praise { pa } else { ia });
    let evidence = format!(
        "{} vs {} {} ({}) over {}",
        m.fmt(fv),
        m.fmt(rv),
        ref_name,
        signed(m, fv - rv),
        rounds_txt(f.t.rounds as u64)
    );
    Some(Candidate {
        key: format!("metric:{}", m.label()),
        kind: if praise { InsightKind::Praise } else { InsightKind::Improve },
        score: rel.abs() * conf,
        insight: Insight {
            kind: if praise { InsightKind::Praise } else { InsightKind::Improve },
            title: (if praise { pt } else { it }).to_string(),
            text,
            evidence,
            operator_key: None,
            map_key: None,
        },
        short: format!("{}: {} vs {} usual", m.label(), m.fmt(fv), m.fmt(rv)),
    })
}

/// Smoothed K/D of a tally against reference kill and death rates per round.
fn smoothed_kd(t: &Tally, ref_kpr: f64, ref_dpr: f64) -> Option<f64> {
    let n = t.rounds as f64;
    let k = adjusted(t.kills as f64, n, ref_kpr);
    let d = adjusted(t.deaths as f64, n, ref_dpr);
    (d > 0.0).then(|| k / d)
}

fn operator_candidates(f: &Agg, r: &Agg, filter: &Filter, baseline: &Baseline) -> Vec<Candidate> {
    let mut out = Vec::new();
    if filter.operators.len() == 1 {
        return out; // The headline cards already describe this one operator.
    }
    for (key, t) in &f.by_op {
        if t.rounds < MIN_FOCUS_ROUNDS_SPLIT {
            continue;
        }
        // Reference rates: the operator's own replay history, else its Tracker career.
        let (kpr, dpr, ref_kd, src) = match r.by_op.get(key).filter(|x| x.rounds >= MIN_REF_ROUNDS_SPLIT) {
            Some(x) => (
                x.kills as f64 / x.rounds as f64,
                x.deaths as f64 / x.rounds as f64,
                ratio(x.kills, x.deaths),
                "in your history",
            ),
            None => match baseline.operators.get(key).filter(|c| c.rounds_played >= MIN_BASELINE_ROUNDS) {
                Some(c) => (
                    c.kills as f64 / c.rounds_played as f64,
                    c.deaths as f64 / c.rounds_played as f64,
                    c.kd(),
                    "in your career",
                ),
                None => continue,
            },
        };
        if ref_kd <= 0.0 {
            continue;
        }
        let Some(sm) = smoothed_kd(t, kpr, dpr) else { continue };
        let rel = (sm - ref_kd) / ref_kd;
        if rel.abs() < REL_THRESHOLD {
            continue;
        }
        let praise = rel > 0.0;
        let name = op_name(key);
        let fkd = ratio(t.kills, t.deaths);
        let conf = t.rounds as f64 / (t.rounds as f64 + K);
        out.push(Candidate {
            key: format!("op:{key}"),
            kind: if praise { InsightKind::Praise } else { InsightKind::Improve },
            score: rel.abs() * conf,
            insight: Insight {
                kind: if praise { InsightKind::Praise } else { InsightKind::Improve },
                title: if praise { format!("{name} is working") } else { format!("{name} is struggling") },
                text: format!(
                    "{}{}",
                    confidence_prefix(t.rounds),
                    if praise {
                        format!("You are doing better on {name} than usual. Keep it in your rotation.")
                    } else {
                        format!("You are doing worse on {name} than usual. Review how you set up and consider a different pick for a while.")
                    }
                ),
                evidence: format!(
                    "K/D {:.2} vs {:.2} {src} over {}",
                    fkd,
                    ref_kd,
                    rounds_txt(t.rounds as u64)
                ),
                operator_key: Some(key.clone()),
                map_key: None,
            },
            short: format!("{name}: K/D {:.2} vs {:.2} usual", fkd, ref_kd),
        });
    }
    out
}

fn map_candidates(f: &Agg, r: &Agg, filter: &Filter, baseline: &Baseline) -> Vec<Candidate> {
    let mut out = Vec::new();
    if filter.maps.len() == 1 {
        return out;
    }
    for (key, t) in &f.by_map {
        if t.rounds < MIN_FOCUS_ROUNDS_SPLIT {
            continue;
        }
        let (ref_rate, src) = match r.by_map.get(key).filter(|x| x.rounds >= MIN_REF_ROUNDS_SPLIT) {
            Some(x) => (x.rounds_won as f64 / x.rounds as f64, "in your history"),
            None => {
                let slug = norm_slug(key);
                match baseline
                    .maps
                    .iter()
                    .find(|(k, _)| norm_slug(k) == slug)
                    .map(|(_, s)| &s.total)
                    .filter(|c| c.rounds_played >= MIN_BASELINE_ROUNDS)
                {
                    Some(c) => (c.rounds_won as f64 / c.rounds_played as f64, "in your career"),
                    None => continue,
                }
            }
        };
        if ref_rate <= 0.0 {
            continue;
        }
        let sm = adjusted(t.rounds_won as f64, t.rounds as f64, ref_rate);
        let rel = (sm - ref_rate) / ref_rate;
        if rel.abs() < REL_THRESHOLD {
            continue;
        }
        let praise = rel > 0.0;
        let name = map_name(key);
        let fr = 100.0 * t.rounds_won as f64 / t.rounds as f64;
        let conf = t.rounds as f64 / (t.rounds as f64 + K);
        out.push(Candidate {
            key: format!("map:{key}"),
            kind: if praise { InsightKind::Praise } else { InsightKind::Improve },
            score: rel.abs() * conf,
            insight: Insight {
                kind: if praise { InsightKind::Praise } else { InsightKind::Improve },
                title: if praise { format!("Strong on {name}") } else { format!("{name} needs work") },
                text: format!(
                    "{}{}",
                    confidence_prefix(t.rounds),
                    if praise {
                        format!("Your team wins more rounds on {name} than usual with you in the game.")
                    } else {
                        format!("Rounds on {name} are going worse than usual. Rewatch the losses and check the angles you took.")
                    }
                ),
                evidence: format!(
                    "{fr:.1}% round wins vs {:.1}% {src} over {}",
                    100.0 * ref_rate,
                    rounds_txt(t.rounds as u64)
                ),
                operator_key: None,
                map_key: Some(key.clone()),
            },
            short: format!("{name}: {fr:.1}% round wins vs {:.1}% usual", 100.0 * ref_rate),
        });
    }
    out
}

/// Focus K/D, hs and win against the Tracker career, when the reference is the replay history.
fn career_candidates(m: Metric, f: &Agg, c: &CounterSet, label: &str) -> Option<Candidate> {
    if !matches!(m, Metric::Kd | Metric::Hs | Metric::Win) || !m.enough(f, true) {
        return None;
    }
    let (cv, rate_n) = match m {
        Metric::Kd => (c.kd(), c.deaths + c.kills),
        Metric::Hs => (c.headshot_pct(), c.kills),
        _ => (c.win_pct(), c.matches_played),
    };
    if cv <= 0.0 || rate_n < 30 {
        return None;
    }
    // Pull the focus toward the career value and judge the same way as the history cards.
    let sm = match m {
        Metric::Kd => {
            let rp = c.rounds_played.max(1) as f64;
            smoothed_kd(&f.t, c.kills as f64 / rp, c.deaths as f64 / rp)?
        }
        _ => {
            let (x, n) = m.parts(f);
            100.0 * (x + m.k() * cv / 100.0) / (n + m.k())
        }
    };
    let rel = (sm - cv) / cv;
    if rel.abs() < REL_THRESHOLD {
        return None;
    }
    let praise = rel > 0.0;
    let fv = m.value(f);
    let what = match m {
        Metric::Kd => "K/D",
        Metric::Hs => "headshot rate",
        _ => "win rate",
    };
    let conf = m.sample(f) as f64 / (m.sample(f) as f64 + m.k());
    Some(Candidate {
        key: format!("career:{}", m.label()),
        kind: if praise { InsightKind::Praise } else { InsightKind::Improve },
        // Slightly below the history cards so the same subject is not shown twice.
        score: rel.abs() * conf * 0.9,
        insight: Insight {
            kind: if praise { InsightKind::Praise } else { InsightKind::Improve },
            title: if praise { format!("Above your {label} {what}") } else { format!("Below your {label} {what}") },
            text: format!(
                "{}Your {what} in this stretch is {} your long-term level.",
                confidence_prefix(f.t.rounds),
                if praise { "above" } else { "below" }
            ),
            evidence: format!("{} vs {} {label} ({})", m.fmt(fv), m.fmt(cv), signed(m, fv - cv)),
            operator_key: None,
            map_key: None,
        },
        short: format!("{} vs {label}: {} vs {}", m.label(), m.fmt(fv), m.fmt(cv)),
    })
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
            "{} kills, {} deaths over {} in {}",
            t.kills,
            t.deaths,
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
                    "{} kills, {} deaths (K/D {:.2}) over {}",
                    best.total.kills,
                    best.total.deaths,
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
pub fn coach(filter: &Filter, all: &[FullMatch], baseline: &Baseline) -> CoachReport {
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

    let mut reference_label_text = reference_label(filter, r.matches);
    let mut comparisons: Vec<Comparison> = Vec::new();
    let mut cands: Vec<Candidate> = Vec::new();
    let career = career_counters(filter, baseline);

    if r.t.rounds > 0 {
        for m in METRICS {
            if !m.enough(&f, true) || !m.enough(&r, false) {
                continue;
            }
            // Per-side win rates only make sense when the filter does not already pick a side.
            if matches!(m, Metric::AtkWin | Metric::DefWin) && filter.side != SideFilter::Both {
                continue;
            }
            let (fv, rv) = (m.value(&f), m.value(&r));
            let places = if m.is_pct() { 1 } else { 2 };
            comparisons.push(Comparison {
                metric: m.label().to_string(),
                focus: round_to(fv, places),
                reference: round_to(rv, places),
                delta: round_to(fv - rv, places),
                better: fv > rv,
            });
            if let Some(c) = metric_candidate(m, &f, &r, ref_name) {
                cands.push(c);
            }
        }
        cands.extend(operator_candidates(&f, &r, filter, baseline));
        cands.extend(map_candidates(&f, &r, filter, baseline));
    } else if let Some((c, label)) = &career {
        // No replay history to compare with: use the career headline numbers instead.
        reference_label_text = format!("Your {label}");
        let kpr = c.kills_per_round();
        let rows: [(Metric, f64); 4] = [
            (Metric::Kd, c.kd()),
            (Metric::Kpr, kpr),
            (Metric::Hs, c.headshot_pct()),
            (Metric::Win, c.win_pct()),
        ];
        for (m, rv) in rows {
            if rv <= 0.0 || !m.enough(&f, true) {
                continue;
            }
            let fv = m.value(&f);
            let places = if m.is_pct() { 1 } else { 2 };
            comparisons.push(Comparison {
                metric: m.label().to_string(),
                focus: round_to(fv, places),
                reference: round_to(rv, places),
                delta: round_to(fv - rv, places),
                better: fv > rv,
            });
        }
        cands.extend(operator_candidates(&f, &r, filter, baseline));
        cands.extend(map_candidates(&f, &r, filter, baseline));
    }

    // Career cards for the focus sets that are shorter than the reference.
    if let (Some((c, label)), true) = (&career, matches!(filter.range, Range::Last1 | Range::Last5 | Range::Last20)) {
        if r.t.rounds > 0 {
            // Not Hs: the parser counts headshots differently from Tracker (about 2x on the same games).
            for m in [Metric::Kd, Metric::Win] {
                if let Some(cand) = career_candidates(m, &f, c, label) {
                    cands.push(cand);
                }
            }
        }
    }

    // One card per subject; a career card is dropped when the history card for the same
    // metric exists (the history card is the more specific statement).
    cands.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap_or(std::cmp::Ordering::Equal));
    let mut seen: HashSet<String> = HashSet::new();
    let mut unique: Vec<Candidate> = Vec::new();
    for c in cands {
        let subject = c.key.strip_prefix("career:").map(|m| format!("metric:{m}")).unwrap_or_else(|| c.key.clone());
        if seen.insert(subject) {
            unique.push(c);
        }
    }

    let strengths: Vec<String> =
        unique.iter().filter(|c| c.kind == InsightKind::Praise).take(3).map(|c| c.short.clone()).collect();
    let work_on: Vec<String> =
        unique.iter().filter(|c| c.kind == InsightKind::Improve).take(3).map(|c| c.short.clone()).collect();

    let mut insights: Vec<Insight> = unique.into_iter().take(MAX_SIGNAL).map(|c| c.insight).collect();
    for card in info_cards(&f, &focus_label) {
        if insights.len() >= MAX_INSIGHTS {
            break;
        }
        insights.push(card);
    }

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
        out.push_str(&format!("[{tag}] {}\n    {}\n    ({})\n", i.title, i.text, i.evidence));
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
        out.push_str(&format!("\n{:<24}{:>10}{:>12}{:>10}\n", "Metric", "Focus", "Reference", "Delta"));
        for c in &r.comparisons {
            out.push_str(&format!(
                "{:<24}{:>10}{:>12}{:>+10}  {}\n",
                c.metric,
                c.focus,
                c.reference,
                c.delta,
                if c.better { "better" } else { "worse" }
            ));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::full::{PlayerInfo, WinCondition};

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
        let b = Baseline::default();
        let rep = coach(&filter(Range::Last1), &all, &b);
        assert_eq!(rep.focus_label, "Last match");
        assert_eq!(rep.reference_label, "Your other 8 matches");
        let kd = rep.comparisons.iter().find(|c| c.metric == "K/D").unwrap();
        assert_eq!(kd.focus, 20.0); // 20 kills, 1 death
        assert_eq!(kd.reference, 1.25);
        assert!(kd.better);
        assert!(rep.insights.iter().any(|i| i.kind == InsightKind::Praise));
        assert!(!rep.strengths.is_empty());
        assert!(rep.insights.len() >= 3 && rep.insights.len() <= 6);
    }

    #[test]
    fn last5_uses_previous_matches_and_respects_cap() {
        let all = history();
        let rep = coach(&filter(Range::Last5), &all, &Baseline::default());
        assert_eq!(rep.focus_label, "Last 5 matches");
        assert_eq!(rep.reference_label, "Previous 4 matches");
    }

    #[test]
    fn singular_plural_and_no_duplicates() {
        assert_eq!(matches_txt(1), "1 match");
        assert_eq!(rounds_txt(2), "2 rounds");
        let all = history();
        let rep = coach(&filter(Range::Last1), &all, &Baseline::default());
        let mut titles: Vec<&str> = rep.insights.iter().map(|i| i.title.as_str()).collect();
        titles.sort();
        let n = titles.len();
        titles.dedup();
        assert_eq!(n, titles.len());
        assert!(rep.strengths.len() <= 3 && rep.work_on.len() <= 3);
    }

    #[test]
    fn thin_history_makes_no_comparisons() {
        let all = vec![mk("a", "2026-10-01T20:00:00", 10, 1, 5, 5)];
        let rep = coach(&filter(Range::Last1), &all, &Baseline::default());
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
        let rep = coach(&f, &all, &Baseline::default());
        assert!(rep.insights.is_empty() && rep.comparisons.is_empty());
        assert!(rep.summary.contains("No matches"));
    }

    #[test]
    fn serializes_to_the_ts_contract() {
        let all = history();
        let rep = coach(&filter(Range::Last1), &all, &Baseline::default());
        let v = serde_json::to_value(&rep).unwrap();
        for k in ["summary", "insights", "strengths", "workOn", "comparisons", "focusLabel", "referenceLabel"] {
            assert!(v.get(k).is_some(), "missing {k}");
        }
        let i = &v["insights"][0];
        for k in ["kind", "title", "text", "evidence"] {
            assert!(i.get(k).is_some(), "missing insight.{k}");
        }
        let c = &v["comparisons"][0];
        for k in ["metric", "focus", "reference", "delta", "better"] {
            assert!(c.get(k).is_some(), "missing comparison.{k}");
        }
    }
}
