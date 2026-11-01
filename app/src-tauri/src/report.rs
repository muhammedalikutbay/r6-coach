//! Match report commands: a stored match plus the advice engine's output for it.

use crate::summary::{MatchSummary, SessionView};
use crate::{baseline, lock, store, Cmd};
use r6k_core::session::{advise_session, session_ending_at};
use r6k_core::{advise, Advice, AdviceKind, Baseline, Match, TOPIC_HEADSHOT};
use serde::Serialize;

/// Same shape as `LastMatchReport` in `app/src/lib/types.ts`.
#[derive(Debug, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct LastMatchReport {
    #[serde(rename = "match")]
    pub match_summary: MatchSummary,
    pub advice: Vec<Advice>,
    /// The sitting that ends with this match (up to 10 matches, gaps under 3 h); `None` when
    /// the match is not part of the stored history.
    pub session: Option<SessionView>,
    pub session_advice: Vec<Advice>,
}

/// Builds the report for `target`; the history is every stored match (the engine itself
/// ignores `target` and anything newer).
fn build(target: &Match, history: &[Match], baseline: &Baseline) -> LastMatchReport {
    let advice = advise(target, history, baseline);
    let sitting = session_ending_at(history, target);
    let (session, mut session_advice) = if sitting.is_empty() {
        (None, Vec::new())
    } else {
        (Some(SessionView::from(sitting.as_slice())), advise_session(&sitting, history, baseline))
    };
    // Do not praise the headshot rate twice: the match card already did.
    let has_hs_praise = |a: &Advice| a.kind == AdviceKind::Praise && a.topic.as_deref() == Some(TOPIC_HEADSHOT);
    if advice.iter().any(has_hs_praise) {
        session_advice.retain(|a| !has_hs_praise(a));
    }
    LastMatchReport {
        match_summary: MatchSummary::from(target),
        advice,
        session,
        session_advice,
    }
}

/// Newest match (history is sorted oldest first) or the one with `match_id`.
fn report_from(matches: &[Match], match_id: Option<&str>, baseline: &Baseline) -> Option<LastMatchReport> {
    let target = match match_id {
        Some(id) => matches.iter().find(|m| m.match_id == id)?,
        None => matches.last()?,
    };
    Some(build(target, matches, baseline))
}

#[tauri::command]
pub fn last_match_report(db: tauri::State<store::Db>) -> Cmd<Option<LastMatchReport>> {
    let matches = store::load_matches(&*lock(&db)?).map_err(|e| e.to_string())?;
    Ok(report_from(&matches, None, &baseline()?))
}

#[tauri::command]
pub fn match_report(db: tauri::State<store::Db>, match_id: String) -> Cmd<Option<LastMatchReport>> {
    let matches = store::load_matches(&*lock(&db)?).map_err(|e| e.to_string())?;
    let report = report_from(&matches, Some(&match_id), &baseline()?);
    if report.is_none() {
        log::warn!("match_report: unknown match id {match_id}");
    }
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;
    use r6k_core::{AdviceKind, Round, Side};

    fn mat(id: &str, day: u32) -> Match {
        Match {
            match_id: id.into(),
            timestamp: format!("2026-10-{day:02}T10:00:00Z"),
            match_type: "Ranked".into(),
            gamemode: "Bomb".into(),
            map_id: 1,
            won: true,
            rounds: (1..=8)
                .map(|n| Round {
                    number: n,
                    side: Side::Defense,
                    operator_id: 5,
                    kills: 1,
                    deaths: 1,
                    assists: 0,
                    headshots: 0,
                    first_blood: false,
                    first_death: false,
                    planted: false,
                    defused: false,
                    won: n <= 5,
                })
                .collect(),
        }
    }

    /// Wraps a v1 match into the stored full model (only the tracked player is present).
    fn full(m: &Match) -> r6k_core::FullMatch {
        use r6k_core::{FullRound, PlayerInfo, RoundPlayer, WinCondition};
        r6k_core::FullMatch {
            match_id: m.match_id.clone(),
            timestamp: m.timestamp.clone(),
            match_type: m.match_type.clone(),
            gamemode: m.gamemode.clone(),
            map_id: m.map_id,
            score_us: 0,
            score_them: 0,
            won: m.won,
            me_profile_id: "me".into(),
            players: vec![PlayerInfo { profile_id: "me".into(), username: "Me".into(), team_index: 0 }],
            rounds: m
                .rounds
                .iter()
                .map(|r| FullRound {
                    number: r.number,
                    winner_team: Some(if r.won { 0 } else { 1 }),
                    win_condition: WinCondition::Elimination,
                    site: String::new(),
                    my_side: r.side,
                    players: vec![RoundPlayer {
                        profile_id: "me".into(),
                        team_index: 0,
                        operator_id: r.operator_id,
                        side: r.side,
                        kills: r.kills,
                        deaths: r.deaths,
                        headshots: r.headshots,
                        first_kill: r.first_blood,
                        first_death: r.first_death,
                        survived: r.deaths == 0,
                    }],
                    kills: vec![],
                })
                .collect(),
        }
    }

    #[test]
    fn empty_store_gives_none() {
        let conn = store::open_in_memory().unwrap();
        let ms = store::load_matches(&conn).unwrap();
        let b = Baseline::embedded().unwrap();
        assert!(report_from(&ms, None, &b).is_none());
        assert!(report_from(&ms, Some("x"), &b).is_none());
    }

    #[test]
    fn last_and_by_id() {
        let mut conn = store::open_in_memory().unwrap();
        store::insert_full_match(&mut conn, &full(&mat("new", 20)), "Match-new").unwrap();
        store::insert_full_match(&mut conn, &full(&mat("old", 10)), "Match-old").unwrap();
        let ms = store::load_matches(&conn).unwrap();
        let b = Baseline::embedded().unwrap();
        let last = report_from(&ms, None, &b).unwrap();
        assert_eq!(last.match_summary.match_id, "new");
        assert_eq!(last.advice[0].kind, AdviceKind::Info);
        let old = report_from(&ms, Some("old"), &b).unwrap();
        assert_eq!(old.match_summary.match_id, "old");
        assert!(report_from(&ms, Some("nope"), &b).is_none());
    }

    /// Needs the real replays and the built sidecar: `cargo test -- --ignored --nocapture`.
    /// Prints the advice in the `r6k advise --last` format so the two can be diffed.
    #[test]
    #[ignore]
    fn real_last_report_matches_cli() {
        use std::path::Path;
        let sidecar =
            Path::new(env!("CARGO_MANIFEST_DIR")).join("binaries/r6-dissect-x86_64-pc-windows-msvc.exe");
        let dir = Path::new(r"C:\Users\Kutbay\Desktop\R6Koc-veri\MatchReplay");
        let t = tempfile::tempdir().unwrap();
        let db = store::Db(std::sync::Mutex::new(store::open(&t.path().join("r6koc.db")).unwrap()));
        let first = store::refresh(&db, dir, |p| crate::dissect_full_match(&sidecar, p)).unwrap();
        assert_eq!(first.new_matches, 30, "failed: {:?}", first.failed);

        let matches = store::load_matches(&db.0.lock().unwrap()).unwrap();
        let b = Baseline::embedded().unwrap();
        let report = report_from(&matches, None, &b).expect("report");
        // Same call the CLI makes, on the matches as parsed (not via the store).
        let direct = advise(matches.last().unwrap(), &matches, &b);
        assert_eq!(report.advice, direct);

        let m = &report.match_summary;
        println!("{} | {} | {} | {}", m.match_id, m.timestamp, m.match_type, m.map);
        for a in &report.advice {
            let tag = match a.kind {
                AdviceKind::Praise => "[praise]",
                AdviceKind::Improve => "[improve]",
                AdviceKind::Info => "[info]",
            };
            println!("\n{tag} {}\n    ({})", a.text, a.evidence);
        }
    }

    #[test]
    fn unknown_match_has_no_session() {
        let b = Baseline::embedded().unwrap();
        let r = build(&mat("a", 2), &[], &b);
        assert!(r.session.is_none() && r.session_advice.is_empty());
    }

    #[test]
    fn session_covers_the_sitting_only() {
        let b = Baseline::embedded().unwrap();
        // The stamps are a day or more apart, so the sitting for "c" is just "c".
        let ms = vec![mat("a", 1), mat("b", 20), mat("c", 21)];
        let r = build(&ms[2], &ms, &b);
        assert_eq!(r.session.unwrap().matches, 1);
    }

    #[test]
    fn serializes_like_the_typescript_dto() {
        let b = Baseline::embedded().unwrap();
        let m = mat("a", 2);
        let v = serde_json::to_value(build(&m, std::slice::from_ref(&m), &b)).unwrap();
        assert!(v["match"]["matchId"].is_string());
        assert!(v["advice"][0]["evidence"].is_string());
        assert_eq!(v["advice"][0]["kind"], "info");
        assert_eq!(v["session"]["matches"], 1);
        assert!(v["session"]["operators"][0]["name"].is_string());
        assert!(v["session"]["maps"][0]["name"].is_string());
        assert!(v["sessionAdvice"].is_array());
    }
}
