//! The `clips` table (created by `store::migrate`).

use rusqlite::{params, Connection};
use serde::Serialize;

use super::retention::ClipMeta;

/// A clip as the front end sees it.
#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ClipRow {
    pub id: i64,
    pub match_id: String,
    /// 1-based, like the kill feed's round number.
    pub round: u32,
    /// `kill` | `death`
    pub kind: String,
    /// Round clock value of the event, the same number the kill feed shows.
    pub time_s: i64,
    pub killer: String,
    pub victim: String,
    pub path: String,
    pub starred: bool,
    pub size: u64,
    /// Where playback should start inside the file (the file starts at a keyframe before it).
    pub start_ms: i64,
    pub duration_ms: i64,
    pub created_ms: i64,
}

pub struct NewClip<'a> {
    pub match_id: &'a str,
    pub round: u32,
    pub kind: &'a str,
    pub time_s: i64,
    pub killer: &'a str,
    pub victim: &'a str,
    pub path: &'a str,
    pub size: u64,
    pub start_ms: i64,
    pub duration_ms: i64,
    pub created_ms: i64,
}

/// Inserts a clip, or replaces the file info of the same event (the star is kept).
pub fn insert(conn: &Connection, c: &NewClip) -> rusqlite::Result<i64> {
    conn.execute(
        "INSERT INTO clips (match_id, round, kind, time_s, killer, victim, path, size, start_ms, duration_ms, created_ms)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)
         ON CONFLICT (match_id, round, kind, time_s, victim) DO UPDATE SET
            killer = excluded.killer, path = excluded.path, size = excluded.size,
            start_ms = excluded.start_ms, duration_ms = excluded.duration_ms, created_ms = excluded.created_ms",
        params![c.match_id, c.round, c.kind, c.time_s, c.killer, c.victim, c.path, c.size as i64, c.start_ms, c.duration_ms, c.created_ms],
    )?;
    conn.query_row(
        "SELECT id FROM clips WHERE match_id = ?1 AND round = ?2 AND kind = ?3 AND time_s = ?4 AND victim = ?5",
        params![c.match_id, c.round, c.kind, c.time_s, c.victim],
        |r| r.get(0),
    )
}

fn row(r: &rusqlite::Row) -> rusqlite::Result<ClipRow> {
    Ok(ClipRow {
        id: r.get(0)?,
        match_id: r.get(1)?,
        round: r.get(2)?,
        kind: r.get(3)?,
        time_s: r.get(4)?,
        killer: r.get(5)?,
        victim: r.get(6)?,
        path: r.get(7)?,
        starred: r.get::<_, i64>(8)? != 0,
        size: r.get::<_, i64>(9)?.max(0) as u64,
        start_ms: r.get(10)?,
        duration_ms: r.get(11)?,
        created_ms: r.get(12)?,
    })
}

const COLS: &str = "id, match_id, round, kind, time_s, killer, victim, path, starred, size, start_ms, duration_ms, created_ms";

pub fn for_match(conn: &Connection, match_id: &str) -> rusqlite::Result<Vec<ClipRow>> {
    conn.prepare(&format!("SELECT {COLS} FROM clips WHERE match_id = ?1 ORDER BY round, time_s DESC"))?
        .query_map([match_id], row)?
        .collect()
}

pub fn get(conn: &Connection, id: i64) -> rusqlite::Result<Option<ClipRow>> {
    use rusqlite::OptionalExtension;
    conn.query_row(&format!("SELECT {COLS} FROM clips WHERE id = ?1"), [id], row).optional()
}

pub fn all(conn: &Connection) -> rusqlite::Result<Vec<ClipRow>> {
    conn.prepare(&format!("SELECT {COLS} FROM clips ORDER BY created_ms, id"))?.query_map([], row)?.collect()
}

pub fn metas(rows: &[ClipRow]) -> Vec<ClipMeta> {
    rows.iter()
        .map(|c| ClipMeta { id: c.id, match_id: c.match_id.clone(), created_ms: c.created_ms, starred: c.starred, size: c.size })
        .collect()
}

pub fn set_starred(conn: &Connection, id: i64, starred: bool) -> rusqlite::Result<bool> {
    Ok(conn.execute("UPDATE clips SET starred = ?1 WHERE id = ?2", params![starred as i64, id])? > 0)
}

pub fn delete(conn: &Connection, id: i64) -> rusqlite::Result<()> {
    conn.execute("DELETE FROM clips WHERE id = ?1", [id])?;
    Ok(())
}

/// `(count, starred count, total size)` of all clips.
pub fn totals(conn: &Connection) -> rusqlite::Result<(u32, u32, u64)> {
    conn.query_row("SELECT COUNT(*), COALESCE(SUM(starred), 0), COALESCE(SUM(size), 0) FROM clips", [], |r| {
        Ok((r.get::<_, i64>(0)? as u32, r.get::<_, i64>(1)? as u32, r.get::<_, i64>(2)?.max(0) as u64))
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store;

    fn new<'a>(m: &'a str, round: u32, time_s: i64) -> NewClip<'a> {
        NewClip { match_id: m, round, kind: "kill", time_s, killer: "me", victim: "foe", path: "x.mp4", size: 10, start_ms: 500, duration_ms: 8000, created_ms: 1 }
    }

    #[test]
    fn insert_list_star_delete() {
        let conn = store::open_in_memory().unwrap();
        let id = insert(&conn, &new("m1", 1, 120)).unwrap();
        insert(&conn, &new("m1", 2, 90)).unwrap();
        insert(&conn, &new("m2", 1, 60)).unwrap();
        let rows = for_match(&conn, "m1").unwrap();
        assert_eq!(rows.len(), 2);
        assert_eq!((rows[0].round, rows[0].start_ms, rows[0].kind.as_str()), (1, 500, "kill"));
        assert!(set_starred(&conn, id, true).unwrap());
        // Cutting the same event again keeps the star and the row.
        let mut again = new("m1", 1, 120);
        again.size = 99;
        assert_eq!(insert(&conn, &again).unwrap(), id);
        let got = get(&conn, id).unwrap().unwrap();
        assert!(got.starred);
        assert_eq!(got.size, 99);
        assert_eq!(totals(&conn).unwrap(), (3, 1, 99 + 10 + 10));
        delete(&conn, id).unwrap();
        assert_eq!(all(&conn).unwrap().len(), 2);
        assert!(get(&conn, id).unwrap().is_none());
    }
}
