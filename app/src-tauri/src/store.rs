//! SQLite store: full matches, settings and a tiny migration scheme.

use r6k_core::{
    FullMatch, FullRound, KillEvent, Match, PlayerInfo, Round, RoundPlayer, Side, WinCondition,
};
use rusqlite::{params, Connection, OptionalExtension};
use serde::Serialize;
use std::path::Path;
use std::sync::Mutex;

pub const SCHEMA_VERSION: i64 = 3;
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
    // Foreign keys are off by default in SQLite; `reset` and the cascades rely on them.
    conn.execute_batch("PRAGMA foreign_keys = ON;")?;
    migrate(&conn)?;
    Ok(conn)
}

#[cfg(test)]
pub fn open_in_memory() -> rusqlite::Result<Connection> {
    let conn = Connection::open_in_memory()?;
    conn.execute_batch("PRAGMA foreign_keys = ON;")?;
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
    if current == 1 {
        // v2: stored timestamps were the recording PC's local time mislabelled as UTC. Drop the
        // derived data; the next Update re-parses the replays with correct UTC timestamps.
        conn.execute_batch("DELETE FROM rounds; DELETE FROM matches;")?;
    }
    if current < 3 {
        // v3: the full match model (v2 spec section 4). The v1 per-player `rounds` table is
        // replaced by a table of the same name with a different shape; the tracked player's
        // rounds are now derived from `round_players`. Match data is dropped (settings stay)
        // and the next Update re-parses the replays.
        conn.execute_batch(
            "DROP TABLE IF EXISTS rounds;
            DELETE FROM matches;
            ALTER TABLE matches ADD COLUMN score_us INTEGER NOT NULL DEFAULT 0;
            ALTER TABLE matches ADD COLUMN score_them INTEGER NOT NULL DEFAULT 0;
            ALTER TABLE matches ADD COLUMN me_profile_id TEXT NOT NULL DEFAULT '';
            CREATE TABLE match_players (
                match_id   TEXT NOT NULL REFERENCES matches(match_id) ON DELETE CASCADE,
                profile_id TEXT NOT NULL,
                username   TEXT NOT NULL,
                team_index INTEGER NOT NULL,
                is_me      INTEGER NOT NULL,
                PRIMARY KEY (match_id, profile_id)
            );
            CREATE TABLE rounds (
                match_id      TEXT NOT NULL REFERENCES matches(match_id) ON DELETE CASCADE,
                round         INTEGER NOT NULL,
                winner_team   INTEGER,
                win_condition TEXT NOT NULL,
                site          TEXT NOT NULL,
                my_side       TEXT NOT NULL,
                PRIMARY KEY (match_id, round)
            );
            CREATE TABLE round_players (
                match_id    TEXT NOT NULL REFERENCES matches(match_id) ON DELETE CASCADE,
                round       INTEGER NOT NULL,
                profile_id  TEXT NOT NULL,
                team_index  INTEGER NOT NULL,
                operator_id INTEGER NOT NULL,
                side        TEXT NOT NULL,
                kills       INTEGER NOT NULL,
                deaths      INTEGER NOT NULL,
                headshots   INTEGER NOT NULL,
                first_kill  INTEGER NOT NULL,
                first_death INTEGER NOT NULL,
                survived    INTEGER NOT NULL,
                PRIMARY KEY (match_id, round, profile_id)
            );
            CREATE TABLE kill_events (
                match_id TEXT NOT NULL REFERENCES matches(match_id) ON DELETE CASCADE,
                round    INTEGER NOT NULL,
                seq      INTEGER NOT NULL,
                time_s   INTEGER NOT NULL,
                killer   TEXT NOT NULL,
                victim   TEXT NOT NULL,
                headshot INTEGER NOT NULL,
                PRIMARY KEY (match_id, round, seq)
            );",
        )?;
    }
    // Future steps: `if current < 4 { ... }`
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

/// Wipes all match data; settings stay.
pub fn reset(conn: &Connection) -> rusqlite::Result<()> {
    conn.execute_batch(
        "DELETE FROM kill_events; DELETE FROM round_players; DELETE FROM rounds;
         DELETE FROM match_players; DELETE FROM matches;",
    )
}

pub fn folder_known(conn: &Connection, folder: &str) -> rusqlite::Result<bool> {
    conn.query_row("SELECT 1 FROM matches WHERE folder_name = ?1", [folder], |_| Ok(()))
        .optional()
        .map(|o| o.is_some())
}

/// Inserts a full match (players, rounds, per-round player stats, kill feed) in one
/// transaction. Returns false when the match ID is already stored (nothing is written then).
pub fn insert_full_match(conn: &mut Connection, m: &FullMatch, folder: &str) -> rusqlite::Result<bool> {
    let tx = conn.transaction()?;
    let added = tx.execute(
        "INSERT OR IGNORE INTO matches
         (match_id, timestamp, map_id, match_type, gamemode, won, folder_name, score_us, score_them, me_profile_id)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
        params![
            m.match_id,
            m.timestamp,
            m.map_id as i64,
            m.match_type,
            m.gamemode,
            m.won,
            folder,
            m.score_us,
            m.score_them,
            m.me_profile_id
        ],
    )?;
    if added == 0 {
        return Ok(false);
    }
    {
        let mut pl = tx.prepare(
            "INSERT OR REPLACE INTO match_players (match_id, profile_id, username, team_index, is_me)
             VALUES (?1, ?2, ?3, ?4, ?5)",
        )?;
        for p in &m.players {
            pl.execute(params![
                m.match_id,
                p.profile_id,
                p.username,
                p.team_index as i64,
                p.profile_id == m.me_profile_id
            ])?;
        }
        let mut rd = tx.prepare(
            "INSERT OR REPLACE INTO rounds (match_id, round, winner_team, win_condition, site, my_side)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        )?;
        let mut rp = tx.prepare(
            "INSERT OR REPLACE INTO round_players
             (match_id, round, profile_id, team_index, operator_id, side, kills, deaths, headshots,
              first_kill, first_death, survived)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
        )?;
        let mut ke = tx.prepare(
            "INSERT OR REPLACE INTO kill_events (match_id, round, seq, time_s, killer, victim, headshot)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        )?;
        for r in &m.rounds {
            rd.execute(params![
                m.match_id,
                r.number,
                r.winner_team.map(|t| t as i64),
                win_str(r.win_condition),
                r.site,
                side_str(r.my_side)
            ])?;
            for p in &r.players {
                rp.execute(params![
                    m.match_id,
                    r.number,
                    p.profile_id,
                    p.team_index as i64,
                    p.operator_id as i64,
                    side_str(p.side),
                    p.kills,
                    p.deaths,
                    p.headshots,
                    p.first_kill,
                    p.first_death,
                    p.survived
                ])?;
            }
            for (i, k) in r.kills.iter().enumerate() {
                ke.execute(params![m.match_id, r.number, i as i64, k.time_s, k.killer, k.victim, k.headshot])?;
            }
        }
    }
    tx.commit()?;
    Ok(true)
}

fn win_str(w: WinCondition) -> &'static str {
    match w {
        WinCondition::Elimination => "elimination",
        WinCondition::DefuserDisabled => "defuser_disabled",
        WinCondition::BombDefused => "bomb_defused",
        WinCondition::Planted => "planted",
        WinCondition::Time => "time",
        WinCondition::Unknown => "unknown",
    }
}

fn win_from(s: &str) -> WinCondition {
    match s {
        "elimination" => WinCondition::Elimination,
        "defuser_disabled" => WinCondition::DefuserDisabled,
        "bomb_defused" => WinCondition::BombDefused,
        "planted" => WinCondition::Planted,
        "time" => WinCondition::Time,
        _ => WinCondition::Unknown,
    }
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

/// All stored matches in full, oldest first.
pub fn load_full_matches(conn: &Connection) -> rusqlite::Result<Vec<FullMatch>> {
    let ids: Vec<String> = conn
        .prepare("SELECT match_id FROM matches ORDER BY timestamp, match_id")?
        .query_map([], |r| r.get(0))?
        .collect::<rusqlite::Result<_>>()?;
    ids.iter().filter_map(|id| load_full_match(conn, id).transpose()).collect()
}

/// One stored match in full, or `None` for an unknown ID.
pub fn load_full_match(conn: &Connection, id: &str) -> rusqlite::Result<Option<FullMatch>> {
    let head = conn
        .query_row(
            "SELECT timestamp, map_id, match_type, gamemode, won, score_us, score_them, me_profile_id
             FROM matches WHERE match_id = ?1",
            [id],
            |r| {
                Ok(FullMatch {
                    match_id: id.to_string(),
                    timestamp: r.get(0)?,
                    map_id: r.get::<_, i64>(1)? as u64,
                    match_type: r.get(2)?,
                    gamemode: r.get(3)?,
                    won: r.get(4)?,
                    score_us: r.get(5)?,
                    score_them: r.get(6)?,
                    me_profile_id: r.get(7)?,
                    players: Vec::new(),
                    season: String::new(),
                    rounds: Vec::new(),
                })
            },
        )
        .optional()?;
    let Some(mut m) = head else { return Ok(None) };

    m.players = conn
        .prepare(
            "SELECT profile_id, username, team_index FROM match_players
             WHERE match_id = ?1 ORDER BY rowid",
        )?
        .query_map([id], |r| {
            Ok(PlayerInfo {
                profile_id: r.get(0)?,
                username: r.get(1)?,
                team_index: r.get::<_, i64>(2)? as usize,
            })
        })?
        .collect::<rusqlite::Result<_>>()?;

    m.rounds = conn
        .prepare(
            "SELECT round, winner_team, win_condition, site, my_side FROM rounds
             WHERE match_id = ?1 ORDER BY round",
        )?
        .query_map([id], |r| {
            Ok(FullRound {
                number: r.get(0)?,
                winner_team: r.get::<_, Option<i64>>(1)?.map(|t| t as usize),
                win_condition: win_from(&r.get::<_, String>(2)?),
                site: r.get(3)?,
                my_side: side_from(&r.get::<_, String>(4)?),
                players: Vec::new(),
                kills: Vec::new(),
            })
        })?
        .collect::<rusqlite::Result<_>>()?;

    let mut players = conn.prepare(
        "SELECT profile_id, team_index, operator_id, side, kills, deaths, headshots,
                first_kill, first_death, survived
         FROM round_players WHERE match_id = ?1 AND round = ?2 ORDER BY rowid",
    )?;
    let mut kills = conn.prepare(
        "SELECT time_s, killer, victim, headshot FROM kill_events
         WHERE match_id = ?1 AND round = ?2 ORDER BY seq",
    )?;
    for r in &mut m.rounds {
        r.players = players
            .query_map(params![id, r.number], |x| {
                Ok(RoundPlayer {
                    profile_id: x.get(0)?,
                    team_index: x.get::<_, i64>(1)? as usize,
                    operator_id: x.get::<_, i64>(2)? as u64,
                    side: side_from(&x.get::<_, String>(3)?),
                    kills: x.get(4)?,
                    deaths: x.get(5)?,
                    headshots: x.get(6)?,
                    first_kill: x.get(7)?,
                    first_death: x.get(8)?,
                    survived: x.get(9)?,
                })
            })?
            .collect::<rusqlite::Result<_>>()?;
        r.kills = kills
            .query_map(params![id, r.number], |x| {
                Ok(KillEvent { time_s: x.get(0)?, killer: x.get(1)?, victim: x.get(2)?, headshot: x.get(3)? })
            })?
            .collect::<rusqlite::Result<_>>()?;
    }
    Ok(Some(m))
}

/// The tracked player's view of a full match (what the v1 commands work on). Assists, plants
/// and defuses are not part of the full model and come out as 0 / false.
pub fn to_match(m: &FullMatch) -> Match {
    let rounds = m
        .rounds
        .iter()
        .filter_map(|r| {
            let me = r.players.iter().find(|p| p.profile_id == m.me_profile_id)?;
            Some(Round {
                number: r.number,
                side: me.side,
                operator_id: me.operator_id,
                kills: me.kills,
                deaths: me.deaths,
                assists: 0,
                headshots: me.headshots,
                first_blood: me.first_kill,
                first_death: me.first_death,
                planted: false,
                defused: false,
                won: r.winner_team == Some(me.team_index),
            })
        })
        .collect();
    Match {
        match_id: m.match_id.clone(),
        timestamp: m.timestamp.clone(),
        match_type: m.match_type.clone(),
        gamemode: m.gamemode.clone(),
        map_id: m.map_id,
        won: m.won,
        rounds,
    }
}

/// All stored matches as the tracked player's `Match`, oldest first (v1 view).
pub fn load_matches(conn: &Connection) -> rusqlite::Result<Vec<Match>> {
    Ok(load_full_matches(conn)?.iter().map(to_match).collect())
}

/// Scans `dir` for `Match-*` folders and imports those not yet stored. A broken folder is
/// reported in `failed` and never stops the others. `parse` turns a folder into a `FullMatch`.
pub fn refresh<F>(db: &Db, dir: &Path, parse: F) -> Result<RefreshResult, String>
where
    F: Fn(&Path) -> Result<FullMatch, String>,
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
            insert_full_match(&mut conn, &m, &name).map_err(|e| format!("database error: {e}"))
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

    fn rp(id: &str, team: usize, side: Side, kills: u32, died: bool) -> RoundPlayer {
        RoundPlayer {
            profile_id: id.into(),
            team_index: team,
            operator_id: 5,
            side,
            kills,
            deaths: died as u32,
            headshots: kills.min(1),
            first_kill: id == "me" && kills > 0,
            first_death: id == "foe" && died,
            survived: !died,
        }
    }

    fn mat(id: &str) -> FullMatch {
        let round = |n: u32| FullRound {
            number: n,
            winner_team: if n % 2 == 1 { Some(0) } else { Some(1) },
            win_condition: WinCondition::Elimination,
            site: "Lobby".into(),
            my_side: Side::Defense,
            players: vec![rp("me", 0, Side::Defense, 2, false), rp("foe", 1, Side::Attack, 0, true)],
            kills: vec![
                KillEvent { time_s: 150, killer: "me".into(), victim: "foe".into(), headshot: true },
                KillEvent { time_s: 100, killer: "me".into(), victim: "foe2".into(), headshot: false },
            ],
        };
        FullMatch {
            match_id: id.into(),
            timestamp: "2026-11-02T10:00:00Z".into(),
            match_type: "Ranked".into(),
            gamemode: "Bomb".into(),
            map_id: 1,
            score_us: 1,
            score_them: 1,
            won: true,
            me_profile_id: "me".into(),
            players: vec![
                PlayerInfo { profile_id: "me".into(), username: "Me".into(), team_index: 0 },
                PlayerInfo { profile_id: "foe".into(), username: "Foe".into(), team_index: 1 },
            ],
            rounds: vec![round(1), round(2)],
            season: String::new(),
        }
    }

    #[test]
    fn v2_to_v3_clears_matches_keeps_settings() {
        let mut conn = open_in_memory().unwrap();
        set_setting(&conn, KEY_REPLAY_DIR, "C:\\replays").unwrap();
        // Rebuild a v2 database: old tables, one stored match.
        conn.execute_batch(
            "DROP TABLE kill_events; DROP TABLE round_players; DROP TABLE rounds; DROP TABLE match_players;
             DROP TABLE matches;
             CREATE TABLE matches (match_id TEXT PRIMARY KEY, timestamp TEXT NOT NULL, map_id INTEGER NOT NULL,
               match_type TEXT NOT NULL, gamemode TEXT NOT NULL, won INTEGER NOT NULL, folder_name TEXT NOT NULL);
             CREATE TABLE rounds (match_id TEXT NOT NULL REFERENCES matches(match_id) ON DELETE CASCADE,
               number INTEGER NOT NULL, side TEXT NOT NULL, operator_id INTEGER NOT NULL, kills INTEGER NOT NULL,
               deaths INTEGER NOT NULL, assists INTEGER NOT NULL, headshots INTEGER NOT NULL,
               first_blood INTEGER NOT NULL, first_death INTEGER NOT NULL, won INTEGER NOT NULL,
               PRIMARY KEY (match_id, number));
             INSERT INTO matches VALUES ('old', 't', 1, 'Ranked', 'Bomb', 1, 'Match-old');
             INSERT INTO rounds VALUES ('old', 1, 'attack', 5, 1, 0, 0, 0, 0, 0, 1);
             UPDATE meta SET schema_version = 2;",
        )
        .unwrap();
        migrate(&conn).unwrap();
        assert!(load_full_matches(&conn).unwrap().is_empty());
        assert_eq!(get_setting(&conn, KEY_REPLAY_DIR).unwrap().as_deref(), Some("C:\\replays"));
        let v: i64 = conn.query_row("SELECT schema_version FROM meta", [], |r| r.get(0)).unwrap();
        assert_eq!(v, 3);
        // New shape works, and a second run must not wipe freshly stored data.
        assert!(insert_full_match(&mut conn, &mat("b"), "Match-b").unwrap());
        migrate(&conn).unwrap();
        assert_eq!(load_full_matches(&conn).unwrap().len(), 1);
    }

    #[test]
    fn roundtrips_full_match() {
        let mut conn = open_in_memory().unwrap();
        let m = mat("a");
        assert!(insert_full_match(&mut conn, &m, "Match-a").unwrap());
        assert!(!insert_full_match(&mut conn, &m, "Match-b").unwrap());
        assert!(folder_known(&conn, "Match-a").unwrap());
        assert!(!folder_known(&conn, "Match-b").unwrap());

        let back = load_full_match(&conn, "a").unwrap().unwrap();
        assert_eq!(
            serde_json::to_value(&back).unwrap(),
            serde_json::to_value(&m).unwrap()
        );
        assert!(load_full_match(&conn, "zzz").unwrap().is_none());
        assert_eq!(load_full_matches(&conn).unwrap().len(), 1);
    }

    #[test]
    fn v1_view_derives_from_full() {
        let mut conn = open_in_memory().unwrap();
        insert_full_match(&mut conn, &mat("a"), "Match-a").unwrap();
        let ms = load_matches(&conn).unwrap();
        assert_eq!(ms.len(), 1);
        assert_eq!(ms[0].rounds.len(), 2);
        assert_eq!(ms[0].kills(), 4);
        assert_eq!(ms[0].rounds[0].side, Side::Defense);
        assert!(ms[0].rounds[0].won && !ms[0].rounds[1].won);
        assert!(ms[0].rounds[0].first_blood);
    }

    #[test]
    fn reset_wipes_matches_keeps_settings() {
        let mut conn = open_in_memory().unwrap();
        set_setting(&conn, KEY_REPLAY_DIR, "C:\\x").unwrap();
        set_setting(&conn, KEY_REPLAY_DIR, "C:\\y").unwrap();
        insert_full_match(&mut conn, &mat("a"), "Match-a").unwrap();
        reset(&conn).unwrap();
        assert!(load_full_matches(&conn).unwrap().is_empty());
        let n: i64 = conn.query_row("SELECT COUNT(*) FROM kill_events", [], |r| r.get(0)).unwrap();
        assert_eq!(n, 0);
        assert_eq!(get_setting(&conn, KEY_REPLAY_DIR).unwrap().as_deref(), Some("C:\\y"));
    }

    #[test]
    fn refresh_skips_known_and_survives_bad_folders() {
        let t = tempfile::tempdir().unwrap();
        for n in ["Match-1", "Match-2", "Match-bad", "Match-empty", "Other"] {
            std::fs::create_dir(t.path().join(n)).unwrap();
        }
        let db = Db(Mutex::new(open_in_memory().unwrap()));
        let parse = |p: &Path| -> Result<FullMatch, String> {
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
