//! Match report commands: a stored match plus the advice engine's output for it.

use crate::summary::MatchSummary;
use crate::{baseline, lock, store, Cmd};
use r6k_core::{advise, Advice, Baseline, Match};
use serde::Serialize;

/// Same shape as `LastMatchReport` in `app/src/lib/types.ts`.
#[derive(Debug, Serialize, PartialEq)]
pub struct LastMatchReport {
    #[serde(rename = "match")]
    pub match_summary: MatchSummary,
    pub advice: Vec<Advice>,
}

/// Builds the report for `target`; the history is every stored match (the engine itself
/// ignores `target` and anything newer).
fn build(target: &Match, history: &[Match], baseline: &Baseline) -> LastMatchReport {
    LastMatchReport {
        match_summary: MatchSummary::from(target),
        advice: advise(target, history, baseline),
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
        store::insert_match(&mut conn, &mat("new", 20), "Match-new").unwrap();
        store::insert_match(&mut conn, &mat("old", 10), "Match-old").unwrap();
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
        let first = store::refresh(&db, dir, |p| crate::dissect_match(&sidecar, p)).unwrap();
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
    fn serializes_like_the_typescript_dto() {
        let b = Baseline::embedded().unwrap();
        let v = serde_json::to_value(build(&mat("a", 2), &[], &b)).unwrap();
        assert!(v["match"]["matchId"].is_string());
        assert!(v["advice"][0]["evidence"].is_string());
        assert_eq!(v["advice"][0]["kind"], "info");
    }
}
