//! Session-level advice: the last few matches played in one sitting, compared with the
//! player's own history.
//!
//! Ranked matches last about five rounds, so a single match rarely gives an operator or map
//! enough rounds to judge. A session pools them. Same hard limits as `advice`: no weapon
//! advice, no assists, and only numbers that were actually computed go into the text.

use crate::advice::{Advice, AdviceKind};
use crate::baseline::{Baseline, CounterSet};
use crate::model::Match;
use crate::names;
use crate::stats::{adjusted, first_bloods, first_deaths, kd, operator_lines, REL_THRESHOLD};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Two consecutive matches whose start times are this far apart (or more) belong to different
/// sessions.
pub const SESSION_GAP_SECS: i64 = 3 * 3600;
/// A session never covers more than this many matches.
pub const MAX_SESSION_MATCHES: usize = 10;
/// Rounds the whole session needs before its overall K/D or first-death rate is judged.
const MIN_SESSION_ROUNDS: u32 = 15;
/// Rounds the most played operator needs before it is judged.
const MIN_TOP_OP_ROUNDS: u32 = 8;
/// Rounds both the best and the worst operator need.
const MIN_BEST_WORST_ROUNDS: u32 = 5;
/// Baseline rounds an operator needs before its own rates are used instead of the overall ones.
const MIN_BASE_OP_ROUNDS: u64 = 20;
/// Smoothed first-death rate must exceed the baseline by this relative amount.
const FIRST_DEATH_REL: f64 = 0.25;
/// Matches before the session needed for a "best match of all time" claim.
const MIN_RECORD_HISTORY: usize = 3;
/// Rounds a match needs to count for the record rule.
const MIN_RECORD_ROUNDS: usize = 4;
/// First kills needed for the opening-duel praise.
const MIN_OPENING_WINS: u32 = 2;
/// Session kills needed before the headshot rate says anything.
const MIN_HS_KILLS: u32 = 8;
/// Headshot-rate gap (percentage points) that triggers praise or an aim note.
const HS_GAP_POINTS: f64 = 8.0;
/// Max items per report.
const MAX_ITEMS: usize = 4;

/// Totals of one session.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionSummary {
    pub matches: u32,
    pub wins: u32,
    pub losses: u32,
    pub rounds: u32,
    pub kills: u32,
    pub deaths: u32,
    pub headshots: u32,
    pub first_bloods: u32,
    pub first_deaths: u32,
    /// `(operator_id, rounds, kills, deaths)`, most rounds first.
    pub top_operators: Vec<(u64, u32, u32, u32)>,
    /// `(map_key, matches, wins)`, most matches first. Unknown maps use `map-<id>`.
    pub maps: Vec<(String, u32, u32)>,
}

/// `YYYY-MM-DDTHH:MM:SS...` to seconds since 1970-01-01 (the zone is ignored: the stamps are
/// local time, which is fine for differences).
fn ts_secs(ts: &str) -> Option<i64> {
    let b = ts.as_bytes();
    if b.len() < 19 {
        return None;
    }
    let n = |r: std::ops::Range<usize>| ts.get(r)?.parse::<i64>().ok();
    let (y, mo, d) = (n(0..4)?, n(5..7)?, n(8..10)?);
    let (h, mi, s) = (n(11..13)?, n(14..16)?, n(17..19)?);
    // Days from civil (Howard Hinnant).
    let y = if mo <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let doy = (153 * (if mo > 2 { mo - 3 } else { mo + 9 }) + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146097 + doe - 719468;
    Some(days * 86400 + h * 3600 + mi * 60 + s)
}

/// The session that ends with `target`: walking back from it, matches stay in while the gap to
/// the next one is under 3 hours; at most 10 matches. `matches` need not be sorted. The result
/// is in chronological order. Empty when `target` is not in `matches`.
pub fn session_ending_at(matches: &[Match], target: &Match) -> Vec<Match> {
    let mut sorted: Vec<&Match> = matches.iter().collect();
    sorted.sort_by(|a, b| a.timestamp.cmp(&b.timestamp));
    let Some(end) = sorted.iter().position(|m| m.match_id == target.match_id) else {
        return Vec::new();
    };
    let mut out: Vec<Match> = vec![sorted[end].clone()];
    let mut i = end;
    while i > 0 && out.len() < MAX_SESSION_MATCHES {
        let (prev, next) = (sorted[i - 1], sorted[i]);
        let gap = match (ts_secs(&prev.timestamp), ts_secs(&next.timestamp)) {
            (Some(a), Some(b)) => b - a,
            _ => {
                log::warn!(
                    "unparseable timestamp near match {}; closing the session",
                    next.match_id
                );
                break;
            }
        };
        if gap >= SESSION_GAP_SECS {
            break;
        }
        out.push(prev.clone());
        i -= 1;
    }
    out.reverse();
    out
}

/// The session that ends with the newest match.
pub fn last_session(matches: &[Match]) -> Vec<Match> {
    match matches.iter().max_by(|a, b| a.timestamp.cmp(&b.timestamp)) {
        Some(last) => session_ending_at(matches, last),
        None => Vec::new(),
    }
}

fn map_label(id: u64) -> String {
    names::map_key(id).unwrap_or_else(|| format!("map-{id}"))
}

pub fn summarize(session: &[Match]) -> SessionSummary {
    let mut ops: BTreeMap<u64, (u32, u32, u32)> = BTreeMap::new();
    let mut maps: BTreeMap<String, (u32, u32)> = BTreeMap::new();
    for m in session {
        for (id, l) in operator_lines(m) {
            let e = ops.entry(id).or_default();
            e.0 += l.rounds;
            e.1 += l.kills;
            e.2 += l.deaths;
        }
        let e = maps.entry(map_label(m.map_id)).or_default();
        e.0 += 1;
        e.1 += m.won as u32;
    }
    let mut top_operators: Vec<_> = ops
        .into_iter()
        .map(|(id, (r, k, d))| (id, r, k, d))
        .collect();
    top_operators.sort_by_key(|t| (std::cmp::Reverse(t.1), t.0));
    let mut maps: Vec<_> = maps.into_iter().map(|(k, (n, w))| (k, n, w)).collect();
    maps.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
    let wins = session.iter().filter(|m| m.won).count() as u32;
    SessionSummary {
        matches: session.len() as u32,
        wins,
        losses: session.len() as u32 - wins,
        rounds: session.iter().map(|m| m.rounds.len() as u32).sum(),
        kills: session.iter().map(Match::kills).sum(),
        deaths: session.iter().map(Match::deaths).sum(),
        headshots: session.iter().map(Match::headshots).sum(),
        first_bloods: session.iter().map(first_bloods).sum(),
        first_deaths: session.iter().map(first_deaths).sum(),
        top_operators,
        maps,
    }
}

/// "1 round", "2 rounds".
fn count(n: u32, word: &str) -> String {
    format!("{n} {}", plural(n, word))
}

fn plural(n: u32, word: &str) -> String {
    if n == 1 {
        word.to_string()
    } else if word.ends_with("ch") {
        format!("{word}es")
    } else {
        format!("{word}s")
    }
}

/// Headshot rate (percent) the player usually has with the operators used, weighted by kills;
/// operators with fewer than 20 baseline kills use the overall rate. Same method as `advice`.
fn expected_headshot_pct(session: &[Match], b: &Baseline) -> Option<f64> {
    if b.overall.kills == 0 {
        return None;
    }
    let (mut num, mut den) = (0.0, 0.0);
    for r in session.iter().flat_map(|m| &m.rounds).filter(|r| r.kills > 0) {
        let pct = b
            .operator(r.operator_id)
            .filter(|c| c.kills >= 20)
            .map(|c| c.headshot_pct())
            .unwrap_or_else(|| b.career_headshot_pct());
        num += pct * r.kills as f64;
        den += r.kills as f64;
    }
    if den == 0.0 {
        None
    } else {
        Some(num / den)
    }
}

fn rate(x: u64, n: u64) -> f64 {
    if n == 0 {
        0.0
    } else {
        x as f64 / n as f64
    }
}

/// Smoothed K/D of `kills`/`deaths` over `rounds`, pulled toward the per-round rates of `base`.
/// Returns `(smoothed_kd, base_kd)`; `None` when the baseline has no usable rounds.
fn smoothed_kd(kills: u32, deaths: u32, rounds: u32, base: &CounterSet) -> Option<(f64, f64)> {
    if base.rounds_played == 0 || base.deaths == 0 {
        return None;
    }
    let n = rounds as f64;
    let k = adjusted(kills as f64, n, rate(base.kills, base.rounds_played));
    let d = adjusted(deaths as f64, n, rate(base.deaths, base.rounds_played));
    if d <= 0.0 {
        return None;
    }
    Some((k / d, base.kd()))
}

/// The operator's own baseline counters when they are large enough, else the overall ones.
fn op_base<'a>(b: &'a Baseline, op: u64) -> &'a CounterSet {
    b.operator(op)
        .filter(|c| c.rounds_played >= MIN_BASE_OP_ROUNDS)
        .unwrap_or(&b.overall)
}

/// Marks an item as being about headshots so the UI can avoid saying it twice.
fn headshot_topic(mut it: Item) -> Item {
    it.advice.topic = Some(crate::advice::TOPIC_HEADSHOT.to_string());
    it
}

struct Item {
    priority: u8,
    advice: Advice,
}

fn item(priority: u8, kind: AdviceKind, text: String, evidence: String) -> Item {
    Item {
        priority,
        advice: Advice {
            kind,
            text,
            evidence,
            topic: None,
        },
    }
}

/// Builds the session report for `session` (chronological, as from `session_ending_at`).
///
/// * `history`: every known replay match; those that are part of the session or came after its
///   first match are ignored.
/// * `baseline`: the personal baseline; earlier replay matches are merged into it.
///
/// At most 4 items, at least one praise, ordered praise, improve, info. Empty for an empty
/// session.
pub fn advise_session(session: &[Match], history: &[Match], baseline: &Baseline) -> Vec<Advice> {
    let Some(first) = session.first() else {
        return Vec::new();
    };
    let prior: Vec<Match> = history
        .iter()
        .filter(|m| {
            m.timestamp < first.timestamp && !session.iter().any(|s| s.match_id == m.match_id)
        })
        .cloned()
        .collect();
    let base = baseline.merged(&prior);
    let sum = summarize(session);
    let n_matches = sum.matches;
    log::debug!("session of {n_matches} matches, {} rounds", sum.rounds);

    let mut items: Vec<Item> = Vec::new();
    let mut used_ops: Vec<u64> = Vec::new();

    // Overall K/D and wins.
    if sum.rounds >= MIN_SESSION_ROUNDS {
        if let Some((adj, base_kd)) = smoothed_kd(sum.kills, sum.deaths, sum.rounds, &base.overall)
        {
            let raw = kd(sum.kills, sum.deaths);
            let base_wins = if base.overall.matches_played > 0 {
                format!("; usual win rate {:.0}%", base.overall.win_pct())
            } else {
                String::new()
            };
            let evidence = format!(
                "{}, K {} / D {}, K/D {raw:.2} (adjusted {adj:.2}); usual K/D {base_kd:.2}; {}-{} in matches{base_wins}",
                count(sum.rounds, "round"), sum.kills, sum.deaths, sum.wins, sum.losses
            );
            let rec = format!("{} of {n_matches} won", sum.wins);
            if adj > base_kd * (1.0 + REL_THRESHOLD) {
                items.push(item(
                    1,
                    AdviceKind::Praise,
                    format!("Strong session: a {raw:.2} K/D over {n_matches} {} ({rec}), above your usual {base_kd:.2}. Keep this rhythm!", plural(n_matches, "match")),
                    evidence,
                ));
            } else if adj < base_kd * (1.0 - REL_THRESHOLD) {
                items.push(item(
                    1,
                    AdviceKind::Improve,
                    format!("A rougher session: {raw:.2} K/D over {n_matches} {} ({rec}), below your usual {base_kd:.2}. If it keeps going like this, a short break can help.", plural(n_matches, "match")),
                    evidence,
                ));
            } else {
                items.push(item(
                    1,
                    AdviceKind::Info,
                    format!("Steady session: a {raw:.2} K/D over {n_matches} {} ({rec}), right around your usual {base_kd:.2}.", plural(n_matches, "match")),
                    evidence,
                ));
            }
        }
    }

    // Too few rounds to judge: just describe the session.
    if sum.rounds < MIN_SESSION_ROUNDS && sum.rounds > 0 {
        items.push(item(
            1,
            AdviceKind::Info,
            format!(
                "Your last {} in one sitting: {} and {} over {} ({} of {n_matches} won). Too few rounds to judge, but it is a start.",
                count(n_matches, "match"),
                count(sum.kills, "kill"),
                count(sum.deaths, "death"),
                count(sum.rounds, "round"),
                sum.wins
            ),
            format!("{}, K/D {:.2}", count(sum.rounds, "round"), kd(sum.kills, sum.deaths)),
        ));
    }

    // Opening duels: more first kills than first deaths.
    if sum.first_bloods >= MIN_OPENING_WINS && sum.first_bloods > sum.first_deaths {
        items.push(item(
            2,
            AdviceKind::Praise,
            format!(
                "You won the opening duel in {} and lost it in {}. Taking the first fight is a real strength.",
                count(sum.first_bloods, "round"),
                count(sum.first_deaths, "round")
            ),
            format!("first kills {}, first deaths {}, {}", sum.first_bloods, sum.first_deaths, count(sum.rounds, "round")),
        ));
    }

    // Headshot rate against the usual one.
    if let Some(usual) = expected_headshot_pct(session, &base).filter(|_| sum.kills >= MIN_HS_KILLS) {
        let hs = 100.0 * sum.headshots as f64 / sum.kills as f64;
        let diff = hs - usual;
        if usual > 0.0 && diff.abs() >= HS_GAP_POINTS && diff.abs() / usual >= REL_THRESHOLD {
            let evidence = format!("HS {hs:.0}% on {} kills; your usual {usual:.0}%", sum.kills);
            if diff > 0.0 {
                items.push(headshot_topic(item(
                    4,
                    AdviceKind::Praise,
                    format!("Your headshot rate this session was {hs:.0}%, above your usual {usual:.0}%. Your aim looked sharp."),
                    evidence,
                )));
            } else {
                items.push(headshot_topic(item(
                    4,
                    AdviceKind::Improve,
                    format!("Your headshot rate this session was {hs:.0}%, below your usual {usual:.0}%. Keep your crosshair at head height while you clear angles."),
                    evidence,
                )));
            }
        }
    }

    // Most played operator against its own baseline.
    if let Some(&(op, rounds, kills, deaths)) = sum.top_operators.first() {
        if rounds >= MIN_TOP_OP_ROUNDS {
            if let Some((adj, base_kd)) = smoothed_kd(kills, deaths, rounds, op_base(&base, op)) {
                let name = names::operator_name(op);
                let raw = kd(kills, deaths);
                let evidence = format!(
                    "{name}: {rounds} rounds, K {kills} / D {deaths}, K/D {raw:.2} (adjusted {adj:.2}); usual {base_kd:.2}"
                );
                if adj > base_kd * (1.0 + REL_THRESHOLD) {
                    items.push(item(
                        3,
                        AdviceKind::Praise,
                        format!("{name} was your most played operator ({rounds} rounds) and it went well: {raw:.2} K/D against your usual {base_kd:.2}."),
                        evidence,
                    ));
                    used_ops.push(op);
                } else if adj < base_kd * (1.0 - REL_THRESHOLD) {
                    items.push(item(
                        3,
                        AdviceKind::Improve,
                        format!("You leaned on {name} for {rounds} rounds, but it gave a {raw:.2} K/D against your usual {base_kd:.2}. Maybe switch it up when it is not clicking."),
                        evidence,
                    ));
                    used_ops.push(op);
                }
            }
        }
    }

    // Best and worst operator of the session.
    let mut cand: Vec<(u64, u32, u32, u32)> = sum
        .top_operators
        .iter()
        .copied()
        .filter(|t| t.1 >= MIN_BEST_WORST_ROUNDS && !used_ops.contains(&t.0))
        .collect();
    cand.sort_by(|a, b| kd(b.2, b.3).total_cmp(&kd(a.2, a.3)).then(a.0.cmp(&b.0)));
    if cand.len() >= 2 {
        let best = cand[0];
        let worst = cand[cand.len() - 1];
        let (bk, wk) = (kd(best.2, best.3), kd(worst.2, worst.3));
        if bk > wk && bk >= 1.0 {
            items.push(item(
                4,
                AdviceKind::Praise,
                format!(
                    "{} was your best operator this session: {bk:.2} K/D over {}.",
                    names::operator_name(best.0),
                    count(best.1, "round")
                ),
                format!(
                    "{}: {}, K {} / D {}",
                    names::operator_name(best.0),
                    count(best.1, "round"),
                    best.2,
                    best.3
                ),
            ));
        }
        if wk < bk && wk < 1.0 {
            items.push(item(
                4,
                AdviceKind::Improve,
                format!("{} was the hardest one this session ({wk:.2} K/D over {}). Worth a rethink of how you play it.", names::operator_name(worst.0), count(worst.1, "round")),
                format!("{}: {}, K {} / D {}", names::operator_name(worst.0), count(worst.1, "round"), worst.2, worst.3),
            ));
        }
    }

    // Same map more than once.
    if let Some((key, played, won)) = sum.maps.first().filter(|m| m.1 >= 2) {
        let id = session
            .iter()
            .find(|m| &map_label(m.map_id) == key)
            .map(|m| m.map_id)
            .unwrap_or(0);
        let name = names::map_name(id);
        let evidence = format!("{name}: {won} of {played} matches won");
        let (kind, text) = if won == played {
            (
                AdviceKind::Praise,
                format!("You played {name} {played} times and won every one. This map suits you!"),
            )
        } else if *won == 0 {
            (AdviceKind::Improve, format!("You met {name} {played} times and lost every one. Try a different operator or plan there next time."))
        } else {
            (
                AdviceKind::Info,
                format!("You played {name} {played} times and won {won}."),
            )
        };
        items.push(item(5, kind, text, evidence));
    }

    // Dying first more often than usual.
    if sum.rounds >= MIN_SESSION_ROUNDS && base.overall.rounds_played > 0 {
        let base_rate = rate(base.overall.first_deaths, base.overall.rounds_played);
        let adj = adjusted(sum.first_deaths as f64, sum.rounds as f64, base_rate);
        if base_rate > 0.0 && adj > base_rate * (1.0 + FIRST_DEATH_REL) && adj - base_rate >= 0.03 {
            items.push(item(
                2,
                AdviceKind::Improve,
                format!(
                    "You died first in {} of {} this session (usually about {:.0}%). Send the drone or wait a few seconds before the first fight.",
                    sum.first_deaths,
                    count(sum.rounds, "round"),
                    base_rate * 100.0
                ),
                format!(
                    "first deaths {}/{} = {:.0}% (adjusted {:.0}%); first kills {}; usual {:.0}%",
                    sum.first_deaths,
                    sum.rounds,
                    100.0 * rate(sum.first_deaths as u64, sum.rounds as u64),
                    adj * 100.0,
                    sum.first_bloods,
                    base_rate * 100.0
                ),
            ));
        }
    }

    // New best match K/D.
    let best_here = session
        .iter()
        .filter(|m| m.rounds.len() >= MIN_RECORD_ROUNDS)
        .map(|m| (kd(m.kills(), m.deaths()), m))
        .max_by(|a, b| a.0.total_cmp(&b.0));
    let earlier: Vec<f64> = prior
        .iter()
        .filter(|m| m.rounds.len() >= MIN_RECORD_ROUNDS)
        .map(|m| kd(m.kills(), m.deaths()))
        .collect();
    if let Some((k, m)) = best_here {
        let prev_best = earlier.iter().cloned().fold(0.0, f64::max);
        if earlier.len() >= MIN_RECORD_HISTORY
            && k > prev_best
            && k > base.overall.kd()
            && m.kills() >= 3
        {
            items.push(item(
                3,
                AdviceKind::Praise,
                format!(
                    "New best match: {k:.2} K/D on {}. Congratulations!",
                    names::map_name(m.map_id)
                ),
                format!(
                    "{}/{} in {}; previous best {prev_best:.2} over {}",
                    m.kills(),
                    m.deaths(),
                    count(m.rounds.len() as u32, "round"),
                    count(earlier.len() as u32, "match")
                ),
            ));
        }
    }

    select(items, &sum)
}

/// Picks at most 4 items, guarantees one praise, and orders them praise, improve, info.
fn select(mut items: Vec<Item>, sum: &SessionSummary) -> Vec<Advice> {
    items.sort_by_key(|i| i.priority);
    let mut picked: Vec<Item> = Vec::new();
    let mut rest: Vec<Item> = Vec::new();
    for it in items {
        if picked.len() < MAX_ITEMS {
            picked.push(it);
        } else {
            rest.push(it);
        }
    }
    if !picked.iter().any(|i| i.advice.kind == AdviceKind::Praise) {
        if let Some(pos) = rest
            .iter()
            .position(|i| i.advice.kind == AdviceKind::Praise)
        {
            let p = rest.remove(pos);
            if picked.len() >= MAX_ITEMS {
                picked.pop();
            }
            picked.push(p);
        } else {
            if picked.len() >= MAX_ITEMS {
                picked.pop();
            }
            picked.push(fallback_praise(sum));
        }
    }
    let rank = |k: AdviceKind| match k {
        AdviceKind::Praise => 0,
        AdviceKind::Improve => 1,
        AdviceKind::Info => 2,
    };
    picked.sort_by_key(|i| (rank(i.advice.kind), i.priority));
    picked.into_iter().map(|i| i.advice).collect()
}

/// A truthful praise when no rule produced one.
fn fallback_praise(sum: &SessionSummary) -> Item {
    let n = sum.matches;
    if sum.wins * 2 > n {
        item(
            9,
            AdviceKind::Praise,
            format!(
                "You won {} of {n} {} this session. Well played!",
                sum.wins,
                plural(n, "match")
            ),
            format!("{}-{} in matches", sum.wins, sum.losses),
        )
    } else if sum.first_bloods > 0 {
        item(
            9,
            AdviceKind::Praise,
            format!(
                "You won the opening duel in {} {} this session. Keep taking those fights.",
                sum.first_bloods,
                plural(sum.first_bloods, "round")
            ),
            format!("first kills {} in {}", sum.first_bloods, count(sum.rounds, "round")),
        )
    } else {
        item(
            9,
            AdviceKind::Praise,
            format!(
                "You stayed at it for {n} {} in one sitting. That is how you get better!",
                plural(n, "match")
            ),
            format!("{} played", count(sum.rounds, "round")),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Round, Side};

    #[test]
    fn counts_use_the_right_plural() {
        assert_eq!(count(1, "round"), "1 round");
        assert_eq!(count(0, "round"), "0 rounds");
        assert_eq!(count(2, "match"), "2 matches");
    }

    const JAGER: u64 = 92270642604;
    const ASH: u64 = 92270642656;
    const ARUNI: u64 = 104189664704;
    const CHALET: u64 = 407558616688;
    const BANK: u64 = 355496559878;

    fn round(op: u64, kills: u32, deaths: u32, fd: bool, fb: bool) -> Round {
        Round {
            number: 1,
            side: Side::Defense,
            operator_id: op,
            kills,
            deaths,
            assists: 0,
            headshots: 0,
            first_blood: fb,
            first_death: fd,
            planted: false,
            defused: false,
            won: false,
        }
    }

    fn at(id: &str, ts: &str, map: u64, won: bool, rounds: Vec<Round>) -> Match {
        Match {
            match_id: id.into(),
            timestamp: format!("{ts}Z"),
            match_type: "Ranked".into(),
            gamemode: "Bomb".into(),
            map_id: map,
            won,
            rounds,
        }
    }

    /// `n` rounds with the given per-round kills/deaths.
    fn rs(op: u64, n: usize, k: u32, d: u32) -> Vec<Round> {
        (0..n).map(|_| round(op, k, d, false, false)).collect()
    }

    /// Baseline: 1000 rounds, K/D 1.0 (0.8 kills and deaths per round), 10% first deaths,
    /// 50% match wins.
    fn base() -> Baseline {
        let mut b = Baseline {
            captured_at: "2026-10-01T00:00:00Z".into(),
            ..Default::default()
        };
        b.overall.rounds_played = 1000;
        b.overall.kills = 800;
        b.overall.deaths = 800;
        b.overall.first_deaths = 100;
        b.overall.matches_played = 100;
        b.overall.matches_won = 50;
        b
    }

    fn of(kind: AdviceKind, v: &[Advice]) -> Vec<&Advice> {
        v.iter().filter(|a| a.kind == kind).collect()
    }

    // ---- session cutting ----

    fn stamps(list: &[&str]) -> Vec<Match> {
        list.iter()
            .enumerate()
            .map(|(i, t)| at(&format!("m{i}"), t, CHALET, true, rs(JAGER, 5, 1, 1)))
            .collect()
    }

    #[test]
    fn timestamps_parse_across_midnight_and_months() {
        let a = ts_secs("2026-10-31T23:30:00Z").unwrap();
        let b = ts_secs("2026-11-01T00:30:00Z").unwrap();
        assert_eq!(b - a, 3600);
        let c = ts_secs("2026-02-28T12:00:00Z").unwrap();
        let d = ts_secs("2026-03-01T12:00:00Z").unwrap();
        assert_eq!(d - c, 86400);
        assert!(ts_secs("garbage").is_none());
    }

    #[test]
    fn gap_of_three_hours_cuts_the_session() {
        let ms = stamps(&[
            "2026-10-30T20:00:00",
            "2026-10-30T20:30:00",
            "2026-10-30T23:30:00",
            "2026-10-31T00:10:00",
        ]);
        // 20:30 -> 23:30 is exactly 3 hours: cut.
        let s = last_session(&ms);
        assert_eq!(
            s.iter().map(|m| m.match_id.as_str()).collect::<Vec<_>>(),
            ["m2", "m3"]
        );
        // 2h59 stays in.
        let ms = stamps(&[
            "2026-10-30T20:00:00",
            "2026-10-30T22:59:00",
            "2026-10-30T23:10:00",
        ]);
        assert_eq!(last_session(&ms).len(), 3);
    }

    #[test]
    fn session_is_capped_at_ten_and_input_order_does_not_matter() {
        let times: Vec<String> = (0..12)
            .map(|i| format!("2026-10-30T{:02}:{:02}:00", 10 + i / 4, (i % 4) * 10))
            .collect();
        let refs: Vec<&str> = times.iter().map(String::as_str).collect();
        let mut ms = stamps(&refs);
        ms.reverse();
        let s = last_session(&ms);
        assert_eq!(s.len(), 10);
        assert_eq!(s.last().unwrap().match_id, "m11");
        assert!(s.windows(2).all(|w| w[0].timestamp < w[1].timestamp));
        assert!(last_session(&[]).is_empty());
    }

    #[test]
    fn session_ending_at_an_older_match() {
        let ms = stamps(&[
            "2026-10-29T10:00:00",
            "2026-10-29T10:20:00",
            "2026-10-30T10:00:00",
        ]);
        let s = session_ending_at(&ms, &ms[1]);
        assert_eq!(s.len(), 2);
    }

    #[test]
    fn summary_adds_up() {
        let m1 = at("a", "2026-10-30T10:00:00", CHALET, true, {
            let mut r = rs(JAGER, 3, 2, 1);
            r.extend(rs(ASH, 2, 0, 1));
            r[0].first_death = true;
            r[4].first_blood = true;
            r
        });
        let m2 = at("b", "2026-10-30T10:30:00", BANK, false, rs(JAGER, 4, 1, 1));
        let s = summarize(&[m1, m2]);
        assert_eq!((s.matches, s.wins, s.losses, s.rounds), (2, 1, 1, 9));
        assert_eq!((s.kills, s.deaths), (10, 9));
        assert_eq!((s.first_deaths, s.first_bloods), (1, 1));
        assert_eq!(s.top_operators[0], (JAGER, 7, 10, 7));
        assert_eq!(s.top_operators[1], (ASH, 2, 0, 2));
        assert_eq!(s.maps.len(), 2);
    }

    // ---- rules ----

    #[test]
    fn rule_overall_praise_when_kd_is_well_above_usual() {
        let ms = vec![
            at("a", "2026-10-30T10:00:00", CHALET, true, rs(JAGER, 8, 2, 1)),
            at("b", "2026-10-30T10:30:00", BANK, true, rs(JAGER, 8, 2, 1)),
        ];
        let v = advise_session(&ms, &[], &base());
        let p = of(AdviceKind::Praise, &v);
        assert!(
            p.iter()
                .any(|a| a.text.contains("Strong session") && a.text.contains("2.00")),
            "{v:?}"
        );
    }

    #[test]
    fn rule_overall_improve_when_kd_is_well_below_usual() {
        let ms = vec![
            at(
                "a",
                "2026-10-30T10:00:00",
                CHALET,
                false,
                rs(JAGER, 8, 1, 2),
            ),
            at("b", "2026-10-30T10:30:00", BANK, false, rs(JAGER, 8, 0, 1)),
        ];
        let v = advise_session(&ms, &[], &base());
        assert!(
            of(AdviceKind::Improve, &v)
                .iter()
                .any(|a| a.text.contains("rougher session")),
            "{v:?}"
        );
        assert!(
            !of(AdviceKind::Praise, &v).is_empty(),
            "always one praise: {v:?}"
        );
    }

    #[test]
    fn rule_overall_needs_fifteen_rounds() {
        let ms = vec![at(
            "a",
            "2026-10-30T10:00:00",
            CHALET,
            true,
            rs(JAGER, 14, 3, 1),
        )];
        let v = advise_session(&ms, &[], &base());
        assert!(!v.iter().any(|a| a.text.contains("session:")), "{v:?}");
        assert!(!of(AdviceKind::Praise, &v).is_empty());
    }

    #[test]
    fn rule_top_operator_compares_with_its_own_baseline() {
        let mut b = base();
        let mut c = CounterSet::default();
        c.rounds_played = 100;
        c.kills = 120;
        c.deaths = 100; // K/D 1.2
        b.operators.insert("jager".into(), c);
        // 8 rounds: 6 kills / 8 deaths = 0.75 -> clearly below 1.2 even smoothed.
        let ms = vec![at("a", "2026-10-30T10:00:00", CHALET, false, {
            let mut r = rs(JAGER, 8, 0, 1);
            r[0].kills = 6;
            r
        })];
        let v = advise_session(&ms, &[], &b);
        let a = of(AdviceKind::Improve, &v)
            .into_iter()
            .find(|a| a.text.contains("leaned on"))
            .expect("top op rule");
        assert!(
            a.text.contains("Jäger") && a.text.contains("1.20"),
            "{}",
            a.text
        );

        // Seven rounds only: silent.
        let ms = vec![at(
            "a",
            "2026-10-30T10:00:00",
            CHALET,
            false,
            rs(JAGER, 7, 0, 1),
        )];
        let v = advise_session(&ms, &[], &b);
        assert!(
            !v.iter()
                .any(|a| a.text.contains("leaned on") || a.text.contains("most played")),
            "{v:?}"
        );
    }

    #[test]
    fn rule_top_operator_praise() {
        let ms = vec![at(
            "a",
            "2026-10-30T10:00:00",
            CHALET,
            true,
            rs(JAGER, 10, 2, 1),
        )];
        let v = advise_session(&ms, &[], &base());
        assert!(
            of(AdviceKind::Praise, &v)
                .iter()
                .any(|a| a.text.contains("most played operator")),
            "{v:?}"
        );
    }

    #[test]
    fn rule_best_and_worst_operator() {
        // Jäger 6 rounds K/D 3.0, Ash 6 rounds K/D 0.33; neither reaches 8 rounds.
        let mut r = rs(JAGER, 6, 3, 1);
        r.extend(rs(ASH, 6, 1, 3));
        let ms = vec![at("a", "2026-10-30T10:00:00", CHALET, true, r)];
        let v = advise_session(&ms, &[], &base());
        assert!(
            of(AdviceKind::Praise, &v)
                .iter()
                .any(|a| a.text.contains("Jäger was your best operator")),
            "{v:?}"
        );
        assert!(
            of(AdviceKind::Improve, &v)
                .iter()
                .any(|a| a.text.contains("Ash was the hardest")),
            "{v:?}"
        );
    }

    #[test]
    fn rule_best_and_worst_needs_five_rounds_each() {
        let mut r = rs(JAGER, 6, 3, 1);
        r.extend(rs(ASH, 4, 1, 3));
        let ms = vec![at("a", "2026-10-30T10:00:00", CHALET, true, r)];
        let v = advise_session(&ms, &[], &base());
        assert!(
            !v.iter()
                .any(|a| a.text.contains("best operator") || a.text.contains("hardest")),
            "{v:?}"
        );
    }

    #[test]
    fn rule_same_map_twice() {
        let ms = vec![
            at(
                "a",
                "2026-10-30T10:00:00",
                CHALET,
                false,
                rs(JAGER, 4, 1, 1),
            ),
            at("b", "2026-10-30T10:30:00", CHALET, false, rs(ASH, 4, 1, 1)),
        ];
        let v = advise_session(&ms, &[], &base());
        assert!(
            of(AdviceKind::Improve, &v)
                .iter()
                .any(|a| a.text.contains("Chalet 2 times and lost every one")),
            "{v:?}"
        );

        let ms = vec![
            at("a", "2026-10-30T10:00:00", CHALET, true, rs(JAGER, 4, 1, 1)),
            at("b", "2026-10-30T10:30:00", CHALET, false, rs(ASH, 4, 1, 1)),
        ];
        let v = advise_session(&ms, &[], &base());
        assert!(
            of(AdviceKind::Info, &v)
                .iter()
                .any(|a| a.text.contains("Chalet 2 times and won 1")),
            "{v:?}"
        );

        // Different maps: no map item.
        let ms = vec![
            at("a", "2026-10-30T10:00:00", CHALET, true, rs(JAGER, 4, 1, 1)),
            at("b", "2026-10-30T10:30:00", BANK, true, rs(ASH, 4, 1, 1)),
        ];
        let v = advise_session(&ms, &[], &base());
        assert!(!v.iter().any(|a| a.text.contains("times")), "{v:?}");
    }

    #[test]
    fn rule_first_death_trend() {
        let mut r = rs(JAGER, 8, 1, 1);
        for x in r.iter_mut().take(5) {
            x.first_death = true;
        }
        let ms = vec![
            at("a", "2026-10-30T10:00:00", CHALET, false, r.clone()),
            at("b", "2026-10-30T10:30:00", BANK, false, rs(ASH, 8, 1, 1)),
        ];
        let v = advise_session(&ms, &[], &base());
        let a = of(AdviceKind::Improve, &v)
            .into_iter()
            .find(|a| a.text.contains("died first"))
            .expect("rule");
        assert!(
            a.text.contains("5 of 16") && a.text.contains("drone"),
            "{}",
            a.text
        );

        // Usual rate (about 1 in 10): quiet.
        let mut r = rs(JAGER, 16, 1, 1);
        r[0].first_death = true;
        r[1].first_death = true;
        let v = advise_session(
            &[at("a", "2026-10-30T10:00:00", CHALET, false, r)],
            &[],
            &base(),
        );
        assert!(!v.iter().any(|a| a.text.contains("died first")), "{v:?}");
    }

    #[test]
    fn rule_short_session_is_described_not_judged() {
        let ms = vec![at("a", "2026-10-30T10:00:00", CHALET, false, rs(JAGER, 6, 1, 1))];
        let v = advise_session(&ms, &[], &base());
        let a = of(AdviceKind::Info, &v).into_iter().find(|a| a.text.contains("Too few rounds")).expect("rule");
        assert!(a.text.contains("6 kills") && a.text.contains("6 rounds"), "{}", a.text);
        // 15+ rounds: judged instead.
        let ms = vec![at("a", "2026-10-30T10:00:00", CHALET, false, rs(JAGER, 16, 1, 1))];
        assert!(!advise_session(&ms, &[], &base()).iter().any(|a| a.text.contains("Too few rounds")));
    }

    #[test]
    fn rule_opening_duels() {
        let mut r = rs(JAGER, 6, 1, 1);
        r[0].first_blood = true;
        r[1].first_blood = true;
        r[2].first_death = true;
        let v = advise_session(&[at("a", "2026-10-30T10:00:00", CHALET, false, r)], &[], &base());
        assert!(of(AdviceKind::Praise, &v).iter().any(|a| a.text.contains("won the opening duel in 2 rounds and lost it in 1")), "{v:?}");

        // Only one first kill: quiet.
        let mut r = rs(JAGER, 6, 1, 1);
        r[0].first_blood = true;
        let v = advise_session(&[at("a", "2026-10-30T10:00:00", CHALET, false, r)], &[], &base());
        assert!(!v.iter().any(|a| a.text.contains("lost it in")), "{v:?}");
    }

    #[test]
    fn rule_headshot_rate() {
        // Baseline: no headshots -> usual 0% (rule needs usual > 0), so give JAGER 40%
        // (320 of 800 kills, read from the operator segments).
        let mut b = base();
        let jager = b.operators.entry(names::operator_key(JAGER).unwrap()).or_default();
        jager.kills = 800;
        jager.headshots = 320;
        let mut r = rs(JAGER, 10, 1, 1);
        for x in r.iter_mut().take(8) {
            x.headshots = 1;
        }
        let v = advise_session(&[at("a", "2026-10-30T10:00:00", CHALET, true, r)], &[], &b);
        assert!(of(AdviceKind::Praise, &v).iter().any(|a| a.text.contains("headshot rate this session was 80%")), "{v:?}");
        let r = rs(JAGER, 10, 1, 1);
        let v = advise_session(&[at("a", "2026-10-30T10:00:00", CHALET, true, r)], &[], &b);
        assert!(of(AdviceKind::Improve, &v).iter().any(|a| a.text.contains("below your usual 40%")), "{v:?}");
        // Under 8 kills: quiet.
        let r = rs(JAGER, 7, 1, 1);
        let v = advise_session(&[at("a", "2026-10-30T10:00:00", CHALET, true, r)], &[], &b);
        assert!(!v.iter().any(|a| a.text.contains("headshot")), "{v:?}");
    }

    #[test]
    fn rule_new_best_match_kd() {
        let hist: Vec<Match> = (0..4)
            .map(|i| {
                at(
                    &format!("h{i}"),
                    &format!("2026-10-2{i}T10:00:00"),
                    BANK,
                    false,
                    rs(JAGER, 6, 1, 1),
                )
            })
            .collect();
        let ms = vec![at(
            "a",
            "2026-10-30T10:00:00",
            CHALET,
            true,
            rs(JAGER, 6, 2, 1),
        )];
        let v = advise_session(&ms, &hist, &base());
        assert!(
            of(AdviceKind::Praise, &v)
                .iter()
                .any(|a| a.text.contains("New best match: 2.00")),
            "{v:?}"
        );

        // Not a record when history already had better.
        let mut hist = hist;
        hist[0] = at("h0", "2026-10-20T10:00:00", BANK, true, rs(JAGER, 6, 3, 1));
        let v = advise_session(&ms, &hist, &base());
        assert!(
            !v.iter().any(|a| a.text.contains("New best match")),
            "{v:?}"
        );

        // Not enough history.
        let v = advise_session(&ms, &hist[..2], &base());
        assert!(
            !v.iter().any(|a| a.text.contains("New best match")),
            "{v:?}"
        );
    }

    #[test]
    fn history_inside_or_after_the_session_is_ignored() {
        let ms = vec![at(
            "a",
            "2026-10-30T10:00:00",
            CHALET,
            true,
            rs(JAGER, 6, 2, 1),
        )];
        // History contains the session match itself and a later one with a huge K/D.
        let mut hist: Vec<Match> = (0..4)
            .map(|i| {
                at(
                    &format!("h{i}"),
                    &format!("2026-10-2{i}T10:00:00"),
                    BANK,
                    false,
                    rs(JAGER, 6, 1, 1),
                )
            })
            .collect();
        hist.push(ms[0].clone());
        hist.push(at(
            "z",
            "2026-10-31T10:00:00",
            BANK,
            true,
            rs(JAGER, 6, 9, 1),
        ));
        let v = advise_session(&ms, &hist, &base());
        assert!(
            v.iter().any(|a| a.text.contains("New best match: 2.00")),
            "{v:?}"
        );
    }

    #[test]
    fn report_is_capped_ordered_and_has_praise() {
        // Good K/D, strong best/worst split, repeated map, first deaths: more than 4 candidates.
        let mut r = rs(JAGER, 9, 3, 1);
        r.extend(rs(ARUNI, 6, 0, 3));
        for x in r.iter_mut().take(8) {
            x.first_death = true;
        }
        let ms = vec![
            at("a", "2026-10-30T10:00:00", CHALET, true, r),
            at("b", "2026-10-30T10:30:00", CHALET, true, rs(JAGER, 6, 3, 1)),
        ];
        let hist: Vec<Match> = (0..4)
            .map(|i| {
                at(
                    &format!("h{i}"),
                    &format!("2026-10-2{i}T10:00:00"),
                    BANK,
                    false,
                    rs(JAGER, 6, 1, 1),
                )
            })
            .collect();
        let v = advise_session(&ms, &hist, &base());
        assert!(v.len() <= 4 && !v.is_empty(), "{v:?}");
        assert!(!of(AdviceKind::Praise, &v).is_empty());
        assert_eq!(v[0].kind, AdviceKind::Praise, "praise comes first: {v:?}");
        let rank = |k: AdviceKind| match k {
            AdviceKind::Praise => 0,
            AdviceKind::Improve => 1,
            AdviceKind::Info => 2,
        };
        assert!(
            v.windows(2).all(|w| rank(w[0].kind) <= rank(w[1].kind)),
            "{v:?}"
        );
    }

    #[test]
    fn praise_survives_when_only_improvements_were_found() {
        // Everything bad; the fallback praise must still be there within 4 items.
        let mut r = rs(JAGER, 9, 0, 1);
        r.extend(rs(ARUNI, 6, 1, 1));
        for x in r.iter_mut().take(8) {
            x.first_death = true;
        }
        let ms = vec![
            at("a", "2026-10-30T10:00:00", CHALET, false, r),
            at(
                "b",
                "2026-10-30T10:30:00",
                CHALET,
                false,
                rs(JAGER, 6, 0, 1),
            ),
        ];
        let v = advise_session(&ms, &[], &base());
        assert!(v.len() <= 4);
        assert!(!of(AdviceKind::Praise, &v).is_empty(), "{v:?}");
    }

    #[test]
    fn empty_session_gives_nothing_and_text_has_no_weapons_or_assists() {
        assert!(advise_session(&[], &[], &base()).is_empty());
        let ms = vec![
            at(
                "a",
                "2026-10-30T10:00:00",
                CHALET,
                true,
                rs(JAGER, 10, 2, 1),
            ),
            at("b", "2026-10-30T10:30:00", CHALET, false, rs(ASH, 10, 0, 1)),
        ];
        for a in advise_session(&ms, &[], &base()) {
            let t = format!("{} {}", a.text, a.evidence).to_lowercase();
            assert!(
                !t.contains("weapon") && !t.contains("assist") && !t.contains("gun"),
                "{t}"
            );
        }
    }
}
