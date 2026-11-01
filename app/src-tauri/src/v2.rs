//! v2 commands: the global-filter query engine, the paged match list and the match detail.
//! The DTOs are the serde types of `r6k_core::query` (camelCase, same shapes as `app/src/lib/v2/types.ts`).

use r6k_core::coach::CoachReport;
use r6k_core::query::{self, Filter, MatchDetail, MatchRow, QueryResult};

use crate::{baseline, lock, store, Cmd};

fn run_query(conn: &rusqlite::Connection, filter: &Filter) -> Cmd<QueryResult> {
    let matches = store::load_full_matches(conn).map_err(|e| e.to_string())?;
    let result = query::query(filter, &matches, &baseline()?);
    log::info!("query {:?}: {} match(es), source {:?}", filter, result.match_count, result.source);
    Ok(result)
}

fn run_list(conn: &rusqlite::Connection, filter: &Filter, page: u32, page_size: u32) -> Cmd<Vec<MatchRow>> {
    let matches = store::load_full_matches(conn).map_err(|e| e.to_string())?;
    Ok(query::list_matches(filter, &matches, page, page_size))
}

fn run_detail(conn: &rusqlite::Connection, match_id: &str) -> Cmd<MatchDetail> {
    let m = store::load_full_match(conn, match_id)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| format!("match not found: {match_id}"))?;
    Ok(query::match_detail(&m))
}

fn run_coach(conn: &rusqlite::Connection, filter: &Filter) -> Cmd<CoachReport> {
    let matches = store::load_full_matches(conn).map_err(|e| e.to_string())?;
    let report = r6k_core::coach::coach(filter, &matches, &baseline()?);
    log::info!("coach {:?}: {} match(es) loaded", filter, matches.len());
    Ok(report)
}

#[tauri::command]
pub fn coach(db: tauri::State<store::Db>, filter: Filter) -> Cmd<CoachReport> {
    run_coach(&*lock(&db)?, &filter)
}

#[tauri::command]
pub fn query(db: tauri::State<store::Db>, filter: Filter) -> Cmd<QueryResult> {
    run_query(&*lock(&db)?, &filter)
}

#[tauri::command]
pub fn list_matches(db: tauri::State<store::Db>, filter: Filter, page: u32, page_size: u32) -> Cmd<Vec<MatchRow>> {
    run_list(&*lock(&db)?, &filter, page, page_size)
}

#[tauri::command]
pub fn match_detail(db: tauri::State<store::Db>, match_id: String) -> Cmd<MatchDetail> {
    run_detail(&*lock(&db)?, &match_id)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn empty_database_gives_empty_results() {
        let conn = store::open(Path::new(":memory:")).unwrap();
        // Default filter reads the career baseline, so only the replay-derived lists are empty.
        let r = run_query(&conn, &Filter::default()).unwrap();
        assert!(r.trend.is_empty() && r.recent_matches.is_empty());
        let r = run_query(&conn, &Filter { range: query::Range::Last5, ..Default::default() }).unwrap();
        assert_eq!(r.match_count, 0);
        assert!(run_list(&conn, &Filter::default(), 0, 10).unwrap().is_empty());
        assert!(run_detail(&conn, "nope").is_err());
        run_coach(&conn, &Filter::default()).unwrap();
    }

    #[test]
    fn filter_deserializes_from_camel_case_json() {
        let f: Filter = serde_json::from_str(r#"{"range":"last5","mode":"ranked","side":"attack","operators":["zofia"],"maps":[]}"#).unwrap();
        assert_eq!(f.operators, vec!["zofia".to_string()]);
    }

    /// Needs the real replays and the sidecar: `cargo test -p r6koc-app -- --ignored --nocapture`.
    #[test]
    #[ignore]
    fn commands_on_real_replays() {
        let sidecar = Path::new(env!("CARGO_MANIFEST_DIR")).join("binaries/r6-dissect-x86_64-pc-windows-msvc.exe");
        let dir = Path::new(r"C:\Users\Kutbay\Desktop\R6Koc-veri\MatchReplay");
        let t = tempfile::tempdir().unwrap();
        let db = store::Db(std::sync::Mutex::new(store::open(&t.path().join("r6koc.db")).unwrap()));
        store::refresh(&db, dir, |p| crate::dissect_full_match(&sidecar, p)).unwrap();
        let conn = db.0.lock().unwrap();

        let last5 = run_query(&conn, &Filter { range: query::Range::Last5, ..Default::default() }).unwrap();
        println!("last5: {} matches, rounds {}", last5.match_count, last5.kpis.rounds);
        assert_eq!(last5.match_count, 5);

        let all = run_query(&conn, &Filter::default()).unwrap();
        let zofia = run_query(&conn, &Filter { operators: vec!["zofia".into()], ..Default::default() }).unwrap();
        println!("all rounds {}, zofia rounds {} kd {}", all.kpis.rounds, zofia.kpis.rounds, zofia.kpis.kd);
        assert!(zofia.kpis.rounds > 0 && zofia.kpis.rounds < all.kpis.rounds);
        assert!(zofia.operators.iter().all(|o| o.key == "zofia"));

        let report = run_coach(&conn, &Filter { range: query::Range::Last5, ..Default::default() }).unwrap();
        println!("coach last5: {}", serde_json::to_string(&report).unwrap().len());

        let list = run_list(&conn, &Filter::default(), 0, 10).unwrap();
        assert_eq!(list.len(), 10);
        let id = all.recent_matches.iter().find(|m| m.match_id.starts_with("f7a14123")).map(|m| m.match_id.clone())
            .or_else(|| {
                let all_rows = run_list(&conn, &Filter::default(), 0, 100).unwrap();
                all_rows.into_iter().find(|m| m.match_id.starts_with("f7a14123")).map(|m| m.match_id)
            })
            .expect("match f7a14123 present");
        let d = run_detail(&conn, &id).unwrap();
        println!("detail {id}: {}+{} players, {} rounds", d.teams[0].len(), d.teams[1].len(), d.rounds.len());
        assert_eq!(d.teams[0].len() + d.teams[1].len(), 10);
    }
}
