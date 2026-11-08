//! v5 operator and weapon screens: the commands around `r6k_core::details`, plus the matching of
//! the kill and death events to recorded clips (the core cannot see the `clips` table).

use std::collections::{BTreeSet, HashMap};
use std::path::Path;

use r6k_core::details::{self, ClipEvent, OperatorDetail, OperatorRow, WeaponDetail, WeaponRow};
use r6k_core::query::Filter;
use rusqlite::Connection;
use serde::Serialize;

use crate::clips::db::{self, ClipRow};
use crate::{lock, store, Cmd};

/// Clips shown on one detail screen.
const MAX_CLIPS: usize = 30;

/// A recorded clip together with what the screen knows about the event.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DetailClip {
    pub clip: ClipRow,
    pub headshot: bool,
    pub weapon: Option<String>,
    pub map_key: String,
    /// Operator the tracked player used.
    pub operator_key: String,
    /// Operator of the other player; empty when unknown.
    pub enemy_operator_key: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OperatorScreen {
    pub detail: OperatorDetail,
    pub clips: Vec<DetailClip>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WeaponScreen {
    pub detail: WeaponDetail,
    pub clips: Vec<DetailClip>,
}

/// Clips of the given events. A clip belongs to an event when match, round, kind, clock value and
/// victim agree (the same key the `clips` table is unique on). Rows whose file is gone are skipped.
/// Newest clips first.
fn match_clips(conn: &Connection, events: &[ClipEvent]) -> rusqlite::Result<Vec<DetailClip>> {
    let match_ids: BTreeSet<&str> = events.iter().map(|e| e.match_id.as_str()).collect();
    let mut by_key: HashMap<(String, u32, String, i64, String), ClipRow> = HashMap::new();
    for id in match_ids {
        for c in db::for_match(conn, id)? {
            if Path::new(&c.path).is_file() {
                by_key.insert((c.match_id.clone(), c.round, c.kind.clone(), c.time_s, c.victim.clone()), c);
            }
        }
    }
    let mut out: Vec<DetailClip> = events
        .iter()
        .filter_map(|e| {
            let clip = by_key.remove(&(e.match_id.clone(), e.round, e.kind.clone(), e.time_s, e.victim.clone()))?;
            Some(DetailClip {
                clip,
                headshot: e.headshot,
                weapon: e.weapon.clone(),
                map_key: e.map_key.clone(),
                operator_key: e.operator_key.clone(),
                enemy_operator_key: e.enemy_operator_key.clone(),
            })
        })
        .collect();
    out.sort_by(|a, b| b.clip.created_ms.cmp(&a.clip.created_ms).then_with(|| b.clip.id.cmp(&a.clip.id)));
    out.truncate(MAX_CLIPS);
    Ok(out)
}

fn run_operator_screen(conn: &Connection, filter: &Filter, key: &str) -> Cmd<OperatorScreen> {
    let matches = store::load_full_matches(conn).map_err(|e| e.to_string())?;
    let detail = details::operator_detail(filter, &matches, key);
    let clips = match_clips(conn, &detail.events).map_err(|e| e.to_string())?;
    Ok(OperatorScreen { detail, clips })
}

fn run_weapon_screen(conn: &Connection, filter: &Filter, key: &str) -> Cmd<WeaponScreen> {
    let matches = store::load_full_matches(conn).map_err(|e| e.to_string())?;
    let detail = details::weapon_detail(filter, &matches, key);
    let clips = match_clips(conn, &detail.events).map_err(|e| e.to_string())?;
    Ok(WeaponScreen { detail, clips })
}

#[tauri::command]
pub fn operator_list(db: tauri::State<store::Db>, filter: Filter) -> Cmd<Vec<OperatorRow>> {
    let matches = store::load_full_matches(&*lock(&db)?).map_err(|e| e.to_string())?;
    Ok(details::operator_list(&filter, &matches))
}

#[tauri::command]
pub fn weapon_list(db: tauri::State<store::Db>, filter: Filter) -> Cmd<Vec<WeaponRow>> {
    let matches = store::load_full_matches(&*lock(&db)?).map_err(|e| e.to_string())?;
    Ok(details::weapon_list(&filter, &matches))
}

#[tauri::command]
pub fn operator_detail(db: tauri::State<store::Db>, filter: Filter, key: String) -> Cmd<OperatorScreen> {
    run_operator_screen(&*lock(&db)?, &filter, &key)
}

#[tauri::command]
pub fn weapon_detail(db: tauri::State<store::Db>, filter: Filter, key: String) -> Cmd<WeaponScreen> {
    run_weapon_screen(&*lock(&db)?, &filter, &key)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ev(kind: &str, round: u32, time_s: i64, victim: &str) -> ClipEvent {
        ClipEvent {
            match_id: "m1".into(),
            round,
            time_s,
            kind: kind.into(),
            killer: "Me".into(),
            victim: victim.into(),
            map_key: "bank".into(),
            ..Default::default()
        }
    }

    fn new_clip<'a>(kind: &'a str, round: u32, time_s: i64, victim: &'a str, path: &'a str) -> db::NewClip<'a> {
        db::NewClip { match_id: "m1", round, kind, time_s, killer: "Me", victim, path, size: 1, start_ms: 0, duration_ms: 1, created_ms: 5 }
    }

    #[test]
    fn events_pick_up_their_clips() {
        let conn = store::open_in_memory().unwrap();
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("a.mp4");
        std::fs::write(&file, b"x").unwrap();
        let path = file.to_string_lossy().into_owned();
        db::insert(&conn, &new_clip("kill", 1, 170, "Foe1", &path)).unwrap();
        db::insert(&conn, &new_clip("kill", 2, 90, "Foe2", "missing-clip-file.mp4")).unwrap();
        db::insert(&conn, &new_clip("kill", 3, 50, "Other", &path)).unwrap();

        let events = [ev("kill", 1, 170, "Foe1"), ev("kill", 2, 90, "Foe2"), ev("death", 1, 170, "Foe1")];
        let got = match_clips(&conn, &events).unwrap();
        // Round 2's file is missing and the death event has no clip.
        assert_eq!(got.len(), 1);
        assert_eq!((got[0].clip.round, got[0].clip.victim.as_str(), got[0].map_key.as_str()), (1, "Foe1", "bank"));
        assert!(match_clips(&conn, &[]).unwrap().is_empty());
    }

    #[test]
    fn screens_on_an_empty_database_are_empty() {
        let conn = store::open_in_memory().unwrap();
        let o = run_operator_screen(&conn, &Filter::default(), "jager").unwrap();
        assert_eq!(o.detail.stats.rounds, 0);
        assert!(o.clips.is_empty());
        let w = run_weapon_screen(&conn, &Filter::default(), "r4-c").unwrap();
        assert_eq!(w.detail.kills, 0);
    }
}
