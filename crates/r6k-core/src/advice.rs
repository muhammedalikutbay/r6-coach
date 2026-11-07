//! Advice engine v1: short, friendly English advice for the last match, compared with the
//! player's own history. Rules and thresholds follow the vault note `02-oneri-mantigi.md`.
//!
//! Limits: this engine works on the tracked-player `Match` model, which carries no kill weapons,
//! so it gives no weapon advice (the coach in `coach.rs` reads weapons from the full model). Assists
//! are not claimed until the parser delivers them. Only numbers that were actually computed go
//! into the text.

use crate::baseline::{Baseline, MapStats};
use crate::model::{Match, Side};
use crate::names;
use crate::stats::{
    adjusted, first_bloods, first_deaths, kd, operator_lines, operator_map_records, rounds_won,
    Confidence, SideKey, FIRM_ROUNDS, MIN_ROUNDS, REL_THRESHOLD,
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

/// Headshot-rate gap (percentage points) that triggers praise or an aim note.
const HS_GAP_POINTS: f64 = 8.0;
/// Minimum kills before the headshot rate says anything.
const HS_MIN_KILLS: u32 = 5;
/// First deaths minus first kills above this triggers the patience note.
const FIRST_DUEL_GAP: i64 = 2;
/// Rounds an alternative operator needs on the map to be suggested.
const ALT_MIN_ROUNDS: u32 = 10;
/// Max number of "improve" items per report.
const MAX_IMPROVE: usize = 2;
/// Max number of "praise" items per report.
const MAX_PRAISE: usize = 3;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AdviceKind {
    Praise,
    Improve,
    Info,
}

/// One line of advice. `evidence` holds the numbers behind `text` so the UI can show them.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Advice {
    pub kind: AdviceKind,
    pub text: String,
    pub evidence: String,
    /// Optional subject tag (e.g. [`TOPIC_HEADSHOT`]) used to avoid repeating a point across the
    /// match and session reports. Absent in the JSON when unset.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub topic: Option<String>,
}

/// `Advice::topic` of headshot-rate items.
pub const TOPIC_HEADSHOT: &str = "headshot";

fn advice(kind: AdviceKind, text: String, evidence: String) -> Advice {
    Advice { kind, text, evidence, topic: None }
}

fn norm_slug(s: &str) -> String {
    s.chars().filter(|c| c.is_ascii_alphanumeric()).map(|c| c.to_ascii_lowercase()).collect()
}

fn find_map<'a>(b: &'a Baseline, key: &str) -> Option<&'a MapStats> {
    let n = norm_slug(key);
    b.maps.iter().find(|(k, _)| norm_slug(k) == n).map(|(_, v)| v)
}

/// Every (operator, side) pair played in a match.
fn ops_and_sides(m: &Match) -> BTreeSet<(u64, SideKey)> {
    m.rounds
        .iter()
        .filter_map(|r| SideKey::of(r.side).map(|s| (r.operator_id, s)))
        .collect()
}

/// Builds the report for `last`.
///
/// * `history`: every known replay match (may include `last`; only matches strictly before
///   `last` count as "earlier").
/// * `baseline`: the personal baseline; matches played after its `capturedAt` are merged in.
///
/// The result is ordered: summary, praise, improve, other info. It always has the summary, at
/// least one praise (when the match has enough rounds to judge) and at most two improve items.
pub fn advise(last: &Match, history: &[Match], baseline: &Baseline) -> Vec<Advice> {
    let mut upto: Vec<Match> = history
        .iter()
        .filter(|m| m.match_id != last.match_id && m.timestamp < last.timestamp)
        .cloned()
        .collect();
    let earlier_count = upto.len();
    upto.push(last.clone());
    let full = baseline.merged(&upto);
    let earlier = &upto[..earlier_count];

    let mut out = vec![summary(last)];
    let rounds = last.rounds.len() as u32;
    let conf = Confidence::from_rounds(rounds);
    if conf == Confidence::Describe {
        log::debug!("match {}: only {rounds} rounds, describing only", last.match_id);
        return out;
    }

    let mut praise: Vec<Advice> = Vec::new();
    let mut improve: Vec<(u8, Advice)> = Vec::new();
    let mut info: Vec<Advice> = Vec::new();

    match headshot_rule(last, &full, conf) {
        Some((AdviceKind::Praise, a)) => praise.push(a),
        Some((_, a)) => improve.push((3, a)),
        None => {}
    }
    if let Some(a) = first_duel_rule(last, conf) {
        improve.push((2, a));
    }
    if let Some(a) = map_operator_rule(last, &upto, &full) {
        improve.push((1, a));
    }
    info.extend(early_operator_rule(last, &full));
    praise.extend(record_rule(last, earlier));

    improve.sort_by_key(|(p, _)| *p);
    let improve: Vec<Advice> = improve.into_iter().map(|(_, a)| a).take(MAX_IMPROVE).collect();
    praise.truncate(MAX_PRAISE);
    if praise.is_empty() {
        praise.push(fallback_praise(last, &full));
    }

    out.extend(praise);
    out.extend(improve);
    out.extend(info);
    out
}

/// Rule 1: map, result, operators, K/D.
fn summary(m: &Match) -> Advice {
    let mut lines: Vec<_> = operator_lines(m).into_iter().collect();
    lines.sort_by_key(|(id, l)| (std::cmp::Reverse(l.rounds), *id));
    let ops = lines
        .iter()
        .map(|(id, l)| {
            format!("{} ({} {})", names::operator_name(*id), l.rounds, plural(l.rounds, "round"))
        })
        .collect::<Vec<_>>()
        .join(", ");
    let result = if m.won { "a win" } else { "a loss" };
    let (k, d) = (m.kills(), m.deaths());
    let text = format!(
        "Last match: {}, {result}. You played {ops}: {k} {} and {d} {}.",
        names::map_name(m.map_id),
        plural(k, "kill"),
        plural(d, "death"),
    );
    let evidence = format!(
        "{} rounds, {} won; K {k} / D {d}; K/D {:.2}",
        m.rounds.len(),
        rounds_won(m),
        kd(k, d)
    );
    advice(AdviceKind::Info, text, evidence)
}

fn plural(n: u32, word: &str) -> String {
    if n == 1 {
        word.to_string()
    } else {
        format!("{word}s")
    }
}

/// Headshot rate the player usually has with the operators used, weighted by kills. Falls back
/// to the overall rate for operators with fewer than 20 baseline kills.
fn expected_headshot_pct(m: &Match, b: &Baseline) -> Option<f64> {
    if b.overall.kills == 0 {
        return None;
    }
    let (mut num, mut den) = (0.0, 0.0);
    for r in m.rounds.iter().filter(|r| r.kills > 0) {
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

/// Rules 2 and 3: headshot rate against the player's own average.
fn headshot_rule(m: &Match, b: &Baseline, conf: Confidence) -> Option<(AdviceKind, Advice)> {
    let kills = m.kills();
    if kills < HS_MIN_KILLS {
        return None;
    }
    let expected = expected_headshot_pct(m, b)?;
    let hs = m.headshot_pct();
    let diff = hs - expected;
    if diff.abs() < HS_GAP_POINTS || expected <= 0.0 || diff.abs() / expected < REL_THRESHOLD {
        return None;
    }
    let evidence = format!("HS {hs:.0}% on {kills} kills; your usual {expected:.0}%");
    let firm = conf == Confidence::Firm;
    if diff > 0.0 {
        let text = if firm {
            format!("Your headshot rate was {hs:.0}%, normally it is {expected:.0}%. Your aim was razor sharp.")
        } else {
            format!("Your headshot rate was {hs:.0}%, normally it is {expected:.0}%. Your aim looks sharp today.")
        };
        Some((AdviceKind::Praise, topic_headshot(advice(AdviceKind::Praise, text, evidence))))
    } else {
        let text = if firm {
            format!("Lots of body shots this match (headshot rate {hs:.0}%, normally {expected:.0}%). Hold your crosshair at head height while you clear angles.")
        } else {
            format!("There may have been a few too many body shots (headshot rate {hs:.0}%, normally {expected:.0}%). Try keeping your crosshair at head height while you clear angles.")
        };
        Some((AdviceKind::Improve, topic_headshot(advice(AdviceKind::Improve, text, evidence))))
    }
}

fn topic_headshot(mut a: Advice) -> Advice {
    a.topic = Some(TOPIC_HEADSHOT.to_string());
    a
}

/// Rule 4: dying first much more often than getting the first kill.
fn first_duel_rule(m: &Match, conf: Confidence) -> Option<Advice> {
    let (fb, fd) = (first_bloods(m), first_deaths(m));
    if (fd as i64) - (fb as i64) <= FIRST_DUEL_GAP {
        return None;
    }
    let on_attack = m.rounds.iter().filter(|r| r.first_death && r.side == Side::Attack).count();
    let on_defense = m.rounds.iter().filter(|r| r.first_death && r.side == Side::Defense).count();
    let tip = if on_attack > on_defense {
        "Wait a few seconds or send the drone before you peek."
    } else if on_defense > on_attack {
        "Let them come to you and wait a few seconds before you push out."
    } else {
        "Wait a few seconds before the first fight, or send the drone first when attacking."
    };
    let text = if conf == Confidence::Firm {
        format!("You were the first to die in {fd} rounds, and got the first kill in {fb}. {tip}")
    } else {
        format!("You were the first to die in {fd} rounds and got the first kill in {fb}. It could be a small sample, but: {tip}")
    };
    let evidence = format!("first deaths {fd}, first kills {fb}, {} rounds", m.rounds.len());
    Some(advice(AdviceKind::Improve, text, evidence))
}

/// Rule 5: operator on this map wins less than the map average; suggest a better operator on the
/// same side. Operator-on-map numbers come from replay history only.
fn map_operator_rule(last: &Match, upto: &[Match], full: &Baseline) -> Option<Advice> {
    let map_key = names::map_key(last.map_id)?;
    let map_stats = find_map(full, &map_key)?;
    let map_name = names::map_name(last.map_id);
    let recs = operator_map_records(upto, last.map_id);

    let mut best: Option<(f64, Advice)> = None;
    for (op, side) in ops_and_sides(last) {
        let Some(rec) = recs.get(&(op, side)) else { continue };
        if rec.rounds < FIRM_ROUNDS {
            continue;
        }
        let avg_set = map_stats
            .sides
            .get(side.baseline_key())
            .filter(|c| c.rounds_played >= FIRM_ROUNDS as u64)
            .unwrap_or(&map_stats.total);
        if avg_set.rounds_played < FIRM_ROUNDS as u64 {
            continue;
        }
        let avg = avg_set.round_win_pct();
        // Smoothed win rate as a fraction, compared with the map average as a fraction.
        let avg_frac = avg / 100.0;
        let adj = adjusted(rec.won as f64, rec.rounds as f64, avg_frac);
        if adj >= avg_frac * (1.0 - REL_THRESHOLD) {
            continue;
        }
        let alt = recs
            .iter()
            .filter(|((o, s), r)| *s == side && *o != op && r.rounds >= ALT_MIN_ROUNDS)
            .filter(|(_, r)| r.win_pct() > rec.win_pct())
            .max_by(|a, b| {
                let fa = adjusted(a.1.won as f64, a.1.rounds as f64, avg_frac);
                let fb = adjusted(b.1.won as f64, b.1.rounds as f64, avg_frac);
                fa.total_cmp(&fb)
            });
        let side_word = if side == SideKey::Attack { "attack" } else { "defense" };
        let mut text = format!(
            "On {map_name} you win {:.0}% of {side_word} rounds with {}, while your usual there is {avg:.0}%.",
            rec.win_pct(),
            names::operator_name(op),
        );
        let mut evidence = format!(
            "{} on {map_name}: {}/{} rounds won ({:.0}%); map average {avg:.0}%",
            names::operator_name(op),
            rec.won,
            rec.rounds,
            rec.win_pct()
        );
        match alt {
            Some(((alt_op, _), alt_rec)) => {
                text.push_str(&format!(
                    " {} is worth a try there ({:.0}% over {} rounds).",
                    names::operator_name(*alt_op),
                    alt_rec.win_pct(),
                    alt_rec.rounds
                ));
                evidence.push_str(&format!(
                    "; {}: {}/{} ({:.0}%)",
                    names::operator_name(*alt_op),
                    alt_rec.won,
                    alt_rec.rounds,
                    alt_rec.win_pct()
                ));
            }
            None => text.push_str(" Maybe spend some extra time practicing this map."),
        }
        let gap = avg_frac - adj;
        if best.as_ref().is_none_or(|(g, _)| gap > *g) {
            best = Some((gap, advice(AdviceKind::Improve, text, evidence)));
        }
    }
    best.map(|(_, a)| a)
}

/// Rounds played with an operator before this match: baseline (with earlier replays merged in)
/// minus this match, never negative.
fn prior_rounds(full: &Baseline, op: u64, in_last: u32) -> u32 {
    let total = full.operator(op).map_or(0, |c| c.rounds_played);
    total.saturating_sub(in_last as u64) as u32
}

/// Rule 6: operators with fewer than 5 rounds of history.
fn early_operator_rule(last: &Match, full: &Baseline) -> Vec<Advice> {
    let mut out = Vec::new();
    for (op, line) in operator_lines(last) {
        let prior = prior_rounds(full, op, line.rounds);
        let total = prior + line.rounds;
        if total >= MIN_ROUNDS {
            continue;
        }
        let name = names::operator_name(op);
        out.push(advice(
            AdviceKind::Info,
            format!(
                "You have played only {total} {} with {name}, too early to say much. A nice addition to your pool!",
                plural(total, "round")
            ),
            format!("{name}: {total} rounds in total, {} this match", line.rounds),
        ));
    }
    out
}

/// Rule 7: best K/D in a single match with an operator (from replay history).
fn record_rule(last: &Match, earlier: &[Match]) -> Vec<Advice> {
    const MIN_ROUNDS_PER_MATCH: u32 = 3;
    const MIN_PRIOR_MATCHES: usize = 2;
    let mut out = Vec::new();
    for (op, line) in operator_lines(last) {
        if line.rounds < MIN_ROUNDS_PER_MATCH || line.kills < 3 {
            continue;
        }
        let prior: Vec<f64> = earlier
            .iter()
            .filter_map(|m| operator_lines(m).remove(&op))
            .filter(|l| l.rounds >= MIN_ROUNDS_PER_MATCH)
            .map(|l| l.kd())
            .collect();
        if prior.len() < MIN_PRIOR_MATCHES {
            continue;
        }
        let best = prior.iter().cloned().fold(0.0, f64::max);
        if line.kd() <= best {
            continue;
        }
        let name = names::operator_name(op);
        out.push(advice(
            AdviceKind::Praise,
            format!("New personal best with {name}: {:.2} K/D in a match. Congratulations!", line.kd()),
            format!(
                "{name}: K/D {:.2} ({}/{} in {} rounds); previous best {best:.2} over {} matches",
                line.kd(),
                line.kills,
                line.deaths,
                line.rounds,
                prior.len()
            ),
        ));
    }
    out
}

/// A truthful praise when no rule produced one, so every report starts on a good note.
fn fallback_praise(m: &Match, full: &Baseline) -> Advice {
    let (k, d) = (m.kills(), m.deaths());
    let usual_kd = full.overall.kd();
    if m.won {
        advice(
            AdviceKind::Praise,
            "You took the win in this match. Well played!".into(),
            format!("won; {} of {} rounds won", rounds_won(m), m.rounds.len()),
        )
    } else if full.overall.deaths > 0 && kd(k, d) > usual_kd {
        advice(
            AdviceKind::Praise,
            "Your K/D was above your usual in this match. Nice work!".into(),
            format!("K/D {:.2}; your usual {usual_kd:.2}", kd(k, d)),
        )
    } else if let Some(best) = m.rounds.iter().filter(|r| r.kills >= 2).max_by_key(|r| r.kills) {
        advice(
            AdviceKind::Praise,
            format!("Round {} was a good one: {} kills.", best.number, best.kills),
            format!("round {}: {} kills, {} deaths", best.number, best.kills, best.deaths),
        )
    } else if first_bloods(m) > 0 {
        advice(
            AdviceKind::Praise,
            "You won the opening duel in some rounds. Keep taking those fights.".into(),
            format!("first kills {}", first_bloods(m)),
        )
    } else {
        advice(
            AdviceKind::Praise,
            "Every match you play adds to your picture. Keep going!".into(),
            format!("{} rounds played", m.rounds.len()),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Round;

    const JAGER: u64 = 92270642604; // defender
    const ARUNI: u64 = 104189664704; // defender
    const ASH: u64 = 92270642656; // attacker
    const CHALET_A: u64 = 407558616688;
    const CHALET_B: u64 = 259816839773;
    const BANK: u64 = 355496559878;

    #[allow(clippy::too_many_arguments)]
    fn round(side: Side, op: u64, kills: u32, deaths: u32, hs: u32, fb: bool, fd: bool, won: bool) -> Round {
        Round {
            number: 1,
            side,
            operator_id: op,
            kills,
            deaths,
            assists: 0,
            headshots: hs,
            first_blood: fb,
            first_death: fd,
            planted: false,
            defused: false,
            won,
        }
    }

    fn mk(id: &str, day: u32, map: u64, won: bool, rounds: Vec<Round>) -> Match {
        Match {
            match_id: id.into(),
            timestamp: format!("2026-11-{day:02}T12:00:00Z"),
            match_type: "Ranked".into(),
            gamemode: "Bomb".into(),
            map_id: map,
            won,
            rounds,
        }
    }

    /// Baseline with a flat 40% headshot rate (every operator 400 of 1000 kills) and no map data.
    fn flat_baseline() -> Baseline {
        let mut b = Baseline { captured_at: "2026-11-01T00:00:00Z".into(), ..Default::default() };
        b.overall.kills = 1000;
        b.overall.deaths = 1000;
        for id in [JAGER, ASH, ARUNI] {
            let c = b.operators.entry(names::operator_key(id).unwrap()).or_default();
            c.kills = 1000;
            c.headshots = 400;
            c.deaths = 1000;
        }
        b
    }

    /// `n` plain rounds: 1 kill, 1 death, `hs` headshots each round.
    fn plain(n: usize, side: Side, op: u64, hs: u32) -> Vec<Round> {
        (0..n).map(|_| round(side, op, 1, 1, hs, false, false, false)).collect()
    }

    fn of(kind: AdviceKind, v: &[Advice]) -> Vec<&Advice> {
        v.iter().filter(|a| a.kind == kind).collect()
    }

    #[test]
    fn rule1_summary_names_map_result_operators_and_kd() {
        let m = mk("a", 2, CHALET_A, true, plain(6, Side::Defense, JAGER, 0));
        let v = advise(&m, &[], &flat_baseline());
        let s = &v[0];
        assert_eq!(s.kind, AdviceKind::Info);
        assert!(s.text.contains("Chalet") && s.text.contains("a win"), "{}", s.text);
        assert!(s.text.contains("6 kills") && s.text.contains("6 deaths"), "{}", s.text);
        assert!(s.text.contains("Jäger (6 rounds)"), "{}", s.text);
        assert!(s.evidence.contains("K/D 1.00"), "{}", s.evidence);
    }

    #[test]
    fn short_match_is_only_described() {
        let m = mk("a", 2, CHALET_A, false, plain(4, Side::Defense, JAGER, 0));
        let v = advise(&m, &[], &flat_baseline());
        assert_eq!(v.len(), 1);
    }

    #[test]
    fn rule2_high_headshot_rate_is_praised() {
        // 10 kills, 7 headshots = 70% vs ~40%.
        let mut rs = plain(10, Side::Defense, JAGER, 0);
        for r in rs.iter_mut().take(7) {
            r.headshots = 1;
        }
        let m = mk("a", 2, CHALET_A, true, rs);
        let v = advise(&m, &[], &flat_baseline());
        let p = of(AdviceKind::Praise, &v);
        assert!(p.iter().any(|a| a.text.contains("headshot rate was 70%")), "{v:?}");
        assert!(p[0].evidence.contains("70%"));
    }

    #[test]
    fn rule3_low_headshot_rate_needs_five_kills() {
        let m = mk("a", 2, CHALET_A, false, plain(10, Side::Defense, JAGER, 0));
        let v = advise(&m, &[], &flat_baseline());
        assert!(of(AdviceKind::Improve, &v).iter().any(|a| a.text.contains("body shots")), "{v:?}");

        // Only 4 kills: no aim note.
        let mut rs = plain(10, Side::Defense, JAGER, 0);
        for r in rs.iter_mut().skip(4) {
            r.kills = 0;
        }
        let m = mk("b", 3, CHALET_A, false, rs);
        let v = advise(&m, &[], &flat_baseline());
        assert!(!v.iter().any(|a| a.text.contains("body shots")), "{v:?}");
    }

    #[test]
    fn rule3_small_gap_is_ignored() {
        // 10 kills, 4 headshots = 40%: right on the usual rate.
        let mut rs = plain(10, Side::Defense, JAGER, 0);
        for r in rs.iter_mut().take(4) {
            r.headshots = 1;
        }
        let v = advise(&mk("a", 2, CHALET_A, true, rs), &[], &flat_baseline());
        assert!(!v.iter().any(|a| a.text.contains("body shots") || a.text.contains("headshot rate was")));
    }

    #[test]
    fn rule4_first_deaths_far_above_first_kills() {
        let mut rs = plain(8, Side::Attack, ASH, 1);
        for r in rs.iter_mut().take(4) {
            r.first_death = true;
        }
        rs[7].first_blood = true;
        let v = advise(&mk("a", 2, BANK, false, rs), &[], &flat_baseline());
        let imp = of(AdviceKind::Improve, &v);
        let a = imp.iter().find(|a| a.text.contains("first to die")).expect("rule 4");
        assert!(a.text.contains("4 rounds") && a.text.contains("drone"), "{}", a.text);
        assert!(a.evidence.contains("first deaths 4"));
    }

    #[test]
    fn rule4_gap_of_two_does_not_fire() {
        let mut rs = plain(8, Side::Attack, ASH, 1);
        rs[0].first_death = true;
        rs[1].first_death = true;
        let v = advise(&mk("a", 2, BANK, false, rs), &[], &flat_baseline());
        assert!(!v.iter().any(|a| a.text.contains("first to die")));
    }

    fn chalet_history(jager_won: u32) -> Vec<Match> {
        // Jäger: 24 defense rounds on Chalet over three matches (two different Chalet IDs).
        let mut h = Vec::new();
        for i in 0..3u32 {
            let rs = (0..8u32)
                .map(|j| round(Side::Defense, JAGER, 1, 1, 0, false, false, i * 8 + j < jager_won))
                .collect();
            h.push(mk(&format!("j{i}"), 2 + i, if i == 1 { CHALET_B } else { CHALET_A }, false, rs));
        }
        // Aruni: 12 defense rounds, 10 won.
        let rs = (0..12u32)
            .map(|j| round(Side::Defense, ARUNI, 1, 1, 0, false, false, j < 10))
            .collect();
        h.push(mk("ar", 6, CHALET_A, true, rs));
        h
    }

    #[test]
    fn rule5_suggests_better_operator_on_same_map_and_side() {
        let base = flat_baseline();
        let hist = chalet_history(6);
        let last = mk("last", 10, CHALET_A, false, plain(6, Side::Defense, JAGER, 0));
        let v = advise(&last, &hist, &base);
        let a = of(AdviceKind::Improve, &v)
            .into_iter()
            .find(|a| a.text.contains("worth a try"))
            .unwrap_or_else(|| panic!("rule 5 missing: {v:?}"));
        assert!(a.text.contains("Aruni") && a.text.contains("Chalet"), "{}", a.text);
        assert!(a.evidence.contains("Jäger") && a.evidence.contains("Aruni"));
    }

    #[test]
    fn rule5_needs_twenty_rounds_of_history() {
        let base = flat_baseline();
        let mut hist = chalet_history(6);
        hist.remove(2); // Jäger drops to 16 rounds + 2 in last = 18
        let mut last = mk("last", 10, CHALET_A, false, plain(2, Side::Defense, JAGER, 0));
        last.rounds.extend(plain(4, Side::Defense, ARUNI, 0));
        let v = advise(&last, &hist, &base);
        assert!(!v.iter().any(|a| a.text.contains("worth a try")), "{v:?}");
    }

    #[test]
    fn rule5_stays_quiet_when_operator_beats_the_map_average() {
        let base = flat_baseline();
        let hist = chalet_history(24); // Jäger wins everything
        let last = mk("last", 10, CHALET_A, true, plain(6, Side::Defense, JAGER, 0));
        let v = advise(&last, &hist, &base);
        assert!(!v.iter().any(|a| a.text.contains("worth a try")));
    }

    #[test]
    fn rule6_few_rounds_with_an_operator_says_too_early() {
        let mut rs = plain(6, Side::Defense, JAGER, 0);
        rs.extend(plain(2, Side::Defense, ARUNI, 0));
        let last = mk("a", 2, BANK, true, rs);
        let v = advise(&last, &[], &flat_baseline());
        let infos = of(AdviceKind::Info, &v);
        assert!(
            infos.iter().any(|a| a.text.contains("only 2 rounds with Aruni") && a.text.contains("too early")),
            "{v:?}"
        );
        assert!(!infos.iter().any(|a| a.text.contains("with Jäger, too early")));
    }

    #[test]
    fn rule6_counts_baseline_history() {
        let mut b = flat_baseline();
        b.operators.insert(
            "aruni".into(),
            crate::baseline::CounterSet { rounds_played: 50, ..Default::default() },
        );
        let last = mk("a", 2, BANK, true, plain(6, Side::Defense, ARUNI, 0));
        let v = advise(&last, &[], &b);
        assert!(!v.iter().any(|a| a.text.contains("too early")), "{v:?}");
    }

    fn op_match(id: &str, day: u32, kills: u32, deaths: u32) -> Match {
        let rs = (0..6)
            .map(|i| {
                round(
                    Side::Defense,
                    JAGER,
                    if i == 0 { kills } else { 0 },
                    if i < deaths { 1 } else { 0 },
                    0,
                    false,
                    false,
                    false,
                )
            })
            .collect();
        mk(id, day, BANK, false, rs)
    }

    #[test]
    fn rule7_new_personal_best_kd_is_celebrated() {
        let hist = vec![op_match("p1", 2, 4, 3), op_match("p2", 3, 3, 3)];
        let last = op_match("l", 5, 8, 2);
        let v = advise(&last, &hist, &flat_baseline());
        let p = of(AdviceKind::Praise, &v);
        assert!(p.iter().any(|a| a.text.contains("New personal best with Jäger: 4.00 K/D")), "{v:?}");
    }

    #[test]
    fn rule7_not_a_record_or_no_history() {
        let hist = vec![op_match("p1", 2, 8, 2), op_match("p2", 3, 3, 3)];
        let last = op_match("l", 5, 6, 2);
        let v = advise(&last, &hist, &flat_baseline());
        assert!(!v.iter().any(|a| a.text.contains("personal best")));
        let v = advise(&last, &hist[..1], &flat_baseline());
        assert!(!v.iter().any(|a| a.text.contains("personal best")), "one earlier match is not enough");
    }

    #[test]
    fn report_has_one_praise_and_at_most_two_improvements() {
        // Triggers rule 3 (no headshots) and rule 4 at once, plus rule 5 history.
        let base = flat_baseline();
        let hist = chalet_history(6);
        let mut rs = plain(10, Side::Defense, JAGER, 0);
        for r in rs.iter_mut().take(5) {
            r.first_death = true;
        }
        let last = mk("last", 10, CHALET_A, false, rs);
        let v = advise(&last, &hist, &base);
        assert_eq!(v[0].kind, AdviceKind::Info);
        assert!(!of(AdviceKind::Praise, &v).is_empty(), "{v:?}");
        assert_eq!(of(AdviceKind::Improve, &v).len(), 2, "{v:?}");
        // Priority: the map/operator note and the patience note beat the aim note.
        assert!(v.iter().any(|a| a.text.contains("worth a try")));
        assert!(!v.iter().any(|a| a.text.contains("body shots")));
    }

    #[test]
    fn serializes_to_the_frontend_shape() {
        let a = advice(AdviceKind::Praise, "t".into(), "e".into());
        assert_eq!(
            serde_json::to_string(&a).unwrap(),
            r#"{"kind":"praise","text":"t","evidence":"e"}"#
        );
    }

    #[test]
    fn never_mentions_weapons_or_assists() {
        let mut rs = plain(10, Side::Attack, ASH, 0);
        rs[0].assists = 3;
        let v = advise(&mk("a", 2, BANK, false, rs), &[], &flat_baseline());
        for a in &v {
            let t = a.text.to_lowercase();
            assert!(!t.contains("assist") && !t.contains("weapon") && !t.contains("gun"), "{t}");
        }
    }
}
