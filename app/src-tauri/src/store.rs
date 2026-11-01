//! SQLite store: matches, rounds, settings and a tiny migration scheme.

use r6k_core::{Match, Round, Side};
use rusqlite::{params, Connection, OptionalExtension};
use serde::Serialize;
use std::path::Path;
use std::sync::Mutex;

pub const SCHEMA_VERSION: i64 = 1;
pub const KEY_REPLAY_DIR: &str = "replay_dir";

/// Shared connection; locked only for short database steps, never while parsing.
pub struct Db(pub Mutex<Connection>);

#[derive(Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct RefreshResult {
    pub new_matches: u32,
    pub failed: Vec<FailedFolder>,
}

#[derive(Debug, Serialize, PartialEq, Eq)]
pub struct FailedFolder {
    pub folder: String,
    pub error: String,
}

pub fn open(path: &Path) -> rusqlite::Result<Connection> {
    let conn = Connection::open(path)?;
    migrate(&conn)?;
    Ok(conn)
}

#[cfg(test)]
pub fn open_in_memory() -> rusqlite::Result<Connection> {
    let conn = Connection::open_in_memory()?;
    migrate(&conn)?;
    Ok(conn)
}

/// Brings the database to `SCHEMA_VERSION`. Each step runs once, in order.
pub fn migrate(conn: &Connection) -> rusqlite::Result<()> {
    conn.execute_batch("CREATE TABLE IF NOT EXISTS meta (schema_version INTEGER NOT NULL);")?;
    let current: i64 = conn
        .query_row("SELECT schema_version FROM meta LIMIT 1", [], |r| r.get(0))
        .optional()?
        .unwrap_or(0);
    if current == 0 {
        conn.execute("INSERT INTO meta (schema_version) VALUES (0)", [])?;
    }
    if current < 1 {
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS matches (
                match_id    TEXT PRIMARY KEY,
                timestamp   TEXT NOT NULL,
                map_id      INTEGER NOT NULL,
                match_type  TEXT NOT NULL,
                gamemode    TEXT NOT NULL,
                won         INTEGER NOT NULL,
                folder_name TEXT NOT NULL
            );
            CREATE INDEX IF NOT EXISTS idx_matches_folder ON matches(folder_name);
            CREATE TABLE IF NOT EXISTS rounds (
                match_id    TEXT NOT NULL REFERENCES matches(match_id) ON DELETE CASCADE,
                number      INTEGER NOT NULL,
                side        TEXT NOT NULL,
                operator_id INTEGER NOT NULL,
                kills       INTEGER NOT NULL,
                deaths      INTEGER NOT NULL,
                assists     INTEGER NOT NULL,
                headshots   INTEGER NOT NULL,
                first_blood INTEGER NOT NULL,
                first_death INTEGER NOT NULL,
                won         INTEGER NOT NULL,
                PRIMARY KEY (match_id, number)
            );
            CREATE TABLE IF NOT EXISTS settings (key TEXT PRIMARY KEY, value TEXT NOT NULL);",
        )?;
    }
    // Future steps: `if current < 2 { ... }`
    conn.execute("UPDATE meta SET schema_version = ?1", [SCHEMA_VERSION])?;
    Ok(())
}

pub fn get_setting(conn: &Connection, key: &str) -> rusqlite::Result<Option<String>> {
    conn.query_row("SELECT value FROM settings WHERE key = ?1", [key], |r| r.get(0)).optional()
}

pub fn set_setting(conn: &Connection, key: &str, value: &str) -> rusqlite::Result<()> {
    conn.execute(
        "INSERT INTO settings (key, value) VALUES (?1, ?2)
         ON CONFLICT(key) DO UPDATE SET value = excluded.value",
        params![key, value],
    )?;
    Ok(())
}

/// Wipes matches and rounds; settings stay.
pub fn reset(conn: &Connection) -> rusqlite::Result<()> {
    conn.execute_batch("DELETE FROM rounds; DELETE FROM matches;")
}

pub fn folder_known(conn: &Connection, folder: &str) -> rusqlite::Result<bool> {
    conn.query_row("SELECT 1 FROM matches WHERE folder_name = ?1", [folder], |_| Ok(()))
        .optional()
        .map(|o| o.is_some())
}

/// Inserts a match with its rounds in one transaction. Returns false when the match ID is
/// already stored (nothing is written then).
pub fn insert_match(conn: &mut Connection, m: &Match, folder: &str) -> rusqlite::Result<bool> {
    let tx = conn.transaction()?;
    let added = tx.execute(
        "INSERT OR IGNORE INTO matches (match_id, timestamp, map_id, match_type, gamemode, won, folder_name)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        params![m.match_id, m.timestamp, m.map_id as i64, m.match_type, m.gamemode, m.won, folder],
    )?;
    if added == 0 {
        return Ok(false);
    }
    {
        let mut st = tx.prepare(
            "INSERT OR REPLACE INTO rounds
             (match_id, number, side, operator_id, kills, deaths, assists, headshots, first_blood, first_death, won)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
        )?;
        for r in &m.rounds {
            st.execute(params![
                m.match_id,
                r.number,
                side_str(r.side),
                r.operator_id as i64,
                r.kills,
                r.deaths,
                r.assists,
                r.headshots,
                r.first_blood,
                r.first_death,
                r.won
            ])?;
        }
    }
    tx.commit()?;
    Ok(true)
}

fn side_str(s: Side) -> &'static str {
    match s {
        Side::Attack => "attack",
        Side::Defense => "defense",
        Side::Unknown => "unknown",
    }
}

fn side_from(s: &str) -> Side {
    match s {
        "attack" => Side::Attack,
        "defense" => Side::Defense,
        _ => Side::Unknown,
    }
}

/// All stored matches, oldest first, rounds ordered by number.
pub fn load_matches(conn: &Connection) -> rusqlite::Result<Vec<Match>> {
    let mut st = conn.prepare(
        "SELECT match_id, timestamp, map_id, match_type, gamemode, won FROM matches
         ORDER BY timestamp, match_id",
    )?;
    let mut matches: Vec<Match> = st
        .query_map([], |r| {
            Ok(Match {
                match_id: r.get(0)?,
                timestamp: r.get(1)?,
                map_id: r.get::<_, i64>(2)? as u64,
                match_type: r.get(3)?,
                gamemode: r.get(4)?,
                won: r.get(5)?,
                rounds: Vec::new(),
            })
        })?
        .collect::<rusqlite::Result<_>>()?;

    let mut st = conn.prepare(
        "SELECT number, side, operator_id, kills, deaths, assists, headshots, first_blood, first_death, won
         FROM rounds WHERE match_id = ?1 ORDER BY number",
    )?;
    for m in &mut matches {
        m.rounds = st
            .query_map([&m.match_id], |r| {
                Ok(Round {
                    number: r.get(0)?,
                    side: side_from(&r.get::<_, String>(1)?),
                    operator_id: r.get::<_, i64>(2)? as u64,
                    kills: r.get(3)?,
                    deaths: r.get(4)?,
                    assists: r.get(5)?,
                    headshots: r.get(6)?,
                    first_blood: r.get(7)?,
                    first_death: r.get(8)?,
                    // Not stored; nothing downstream of the store needs them.
                    planted: false,
                    defused: false,
                    won: r.get(9)?,
                })
            })?
            .collect::<rusqlite::Result<_>>()?;
    }
    Ok(matches)
}

/// Scans `dir` for `Match-*` folders and imports those not yet stored. A broken folder is
/// reported in `failed` and never stops the others. `parse` turns a folder into a `Match`.
pub fn refresh<F>(db: &Db, dir: &Path, parse: F) -> Result<RefreshResult, String>
where
    F: Fn(&Path) -> Result<Match, String>,
{
    let rd = std::fs::read_dir(dir)
        .map_err(|e| format!("cannot read replay folder {}: {e}", dir.display()))?;
    let mut folders: Vec<_> = rd
        .flatten()
        .filter(|e| e.path().is_dir() && e.file_name().to_string_lossy().starts_with("Match-"))
        .collect();
    folders.sort_by_key(|e| e.file_name());

    let mut result = RefreshResult { new_matches: 0, failed: Vec::new() };
    for entry in folders {
        let name = entry.file_name().to_string_lossy().into_owned();
        let known = {
            let conn = db.0.lock().map_err(|_| "database lock poisoned".to_string())?;
            folder_known(&conn, &name)
        };
        match known {
            Ok(true) => continue,
            Ok(false) => {}
            Err(e) => {
                result.failed.push(FailedFolder { folder: name, error: format!("database error: {e}") });
                continue;
            }
        }
        let outcome = parse(&entry.path()).and_then(|m| {
            let mut conn = db.0.lock().map_err(|_| "database lock poisoned".to_string())?;
            insert_match(&mut conn, &m, &name).map_err(|e| format!("database error: {e}"))
        });
        match outcome {
            Ok(true) => result.new_matches += 1,
            Ok(false) => log::debug!("{name}: match already stored under another folder"),
            Err(error) => {
                log::warn!("refresh: {name} failed: {error}");
                result.failed.push(FailedFolder { folder: name, error });
            }
        }
    }
    log::info!("refresh: {} new, {} failed", result.new_matches, result.failed.len());
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn round(n: u32) -> Round {
        Round {
            number: n,
            side: Side::Defense,
            operator_id: 5,
            kills: 2,
            deaths: 1,
            assists: 0,
            headshots: 1,
            first_blood: true,
            first_death: false,
            planted: false,
            defused: false,
            won: n % 2 == 1,
        }
    }

    fn mat(id: &str) -> Match {
        Match {
            match_id: id.into(),
            timestamp: "2026-11-02T10:00:00Z".into(),
            match_type: "Ranked".into(),
            gamemode: "Bomb".into(),
            map_id: 1,
            won: true,
            rounds: vec![round(1), round(2)],
        }
    }

    #[test]
    fn migrate_is_idempotent_and_roundtrips() {
        let mut conn = open_in_memory().unwrap();
        migrate(&conn).unwrap();
        let v: i64 = conn.query_row("SELECT schema_version FROM meta", [], |r| r.get(0)).unwrap();
        assert_eq!(v, SCHEMA_VERSION);
        assert!(insert_match(&mut conn, &mat("a"), "Match-a").unwrap());
        assert!(!insert_match(&mut conn, &mat("a"), "Match-b").unwrap());
        let ms = load_matches(&conn).unwrap();
        assert_eq!(ms.len(), 1);
        assert_eq!(ms[0].rounds.len(), 2);
        assert_eq!(ms[0].rounds[0].side, Side::Defense);
        assert!(folder_known(&conn, "Match-a").unwrap());
        assert!(!folder_known(&conn, "Match-b").unwrap());
    }

    #[test]
    fn settings_survive_reset() {
        let mut conn = open_in_memory().unwrap();
        set_setting(&conn, KEY_REPLAY_DIR, "C:\\x").unwrap();
        set_setting(&conn, KEY_REPLAY_DIR, "C:\\y").unwrap();
        insert_match(&mut conn, &mat("a"), "Match-a").unwrap();
        reset(&conn).unwrap();
        assert!(load_matches(&conn).unwrap().is_empty());
        assert_eq!(get_setting(&conn, KEY_REPLAY_DIR).unwrap().as_deref(), Some("C:\\y"));
    }

    #[test]
    fn refresh_skips_known_and_survives_bad_folders() {
        let t = tempfile::tempdir().unwrap();
        for n in ["Match-1", "Match-2", "Match-bad", "Match-empty", "Other"] {
            std::fs::create_dir(t.path().join(n)).unwrap();
        }
        let db = Db(Mutex::new(open_in_memory().unwrap()));
        let parse = |p: &Path| -> Result<Match, String> {
            let n = p.file_name().unwrap().to_string_lossy().into_owned();
            match n.as_str() {
                "Match-bad" => Err("replay file is corrupted".into()),
                "Match-empty" => Err("no .rec files".into()),
                _ => Ok(mat(&n)),
            }
        };
        let first = refresh(&db, t.path(), parse).unwrap();
        assert_eq!(first.new_matches, 2);
        assert_eq!(first.failed.len(), 2);
        let second = refresh(&db, t.path(), parse).unwrap();
        assert_eq!(second.new_matches, 0);
        assert!(refresh(&db, &t.path().join("nope"), parse).is_err());
    }
}
