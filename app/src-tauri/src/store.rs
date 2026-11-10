//! SQLite store (schema v4): one row per match with the compressed r6-dissect output and the
//! serialized `FullMatch`, the settings, and the replay folder import (`refresh`).

use flate2::{read::ZlibDecoder, write::ZlibEncoder, Compression};
use r6k_core::{FullMatch, Match, Round};
use rusqlite::{params, Connection, OptionalExtension};
use serde::Serialize;
use serde_json::Value;
use std::io::{Read, Write};
use std::path::Path;
use std::sync::Mutex;
use std::time::{Duration, SystemTime};

pub const SCHEMA_VERSION: i64 = 4;
/// Version of the raw -> `FullMatch` conversion. Bump it whenever `r6k_core::full_match_from_json`
/// changes what it produces: on the next start every stored match is rebuilt from its raw JSON
/// (no `.rec` files needed).
pub const PARSER_VERSION: i64 = 4; // 4: Quick Match map ids, Snake pistol (r6k-znk.1); 3: unknown ids (r6k-2a6.3); 2: weapon per kill (r6k-v45.1)
pub const KEY_REPLAY_DIR: &str = "replay_dir";
/// A folder is imported only when none of its files changed for this long.
pub const SETTLE: Duration = Duration::from_secs(60);
/// An unfinished match whose folder stayed quiet this long was left: it becomes `abandoned`.
pub const ABANDON_AFTER: Duration = Duration::from_secs(20 * 60);

/// Shared connection; locked only for short database steps, never while parsing.
pub struct Db(pub Mutex<Connection>);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Status {
    Complete,
    /// Not ended yet; stored but excluded from every query.
    InProgress,
    /// Never ended and quiet for `ABANDON_AFTER`; counted (the player left).
    Abandoned,
}

impl Status {
    pub fn as_str(self) -> &'static str {
        match self {
            Status::Complete => "complete",
            Status::InProgress => "in_progress",
            Status::Abandoned => "abandoned",
        }
    }
    fn from_str(s: &str) -> Status {
        match s {
            "in_progress" => Status::InProgress,
            "abandoned" => Status::Abandoned,
            _ => Status::Complete,
        }
    }
    fn visible(self) -> bool {
        self != Status::InProgress
    }
}

#[derive(Debug, Default, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct RefreshResult {
    /// Matches that became visible (new folders, or in-progress matches that ended / were left).
    pub new_matches: u32,
    /// Visible matches whose folder changed and that were re-parsed and replaced.
    pub updated_matches: u32,
    /// Folders still being written (changed in the last minute, or not parseable yet).
    pub pending: u32,
    pub failed: Vec<FailedFolder>,
    /// Stored matches that have not ended yet. Internal: drives the watcher's retry.
    #[serde(skip)]
    pub in_progress: u32,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct FailedFolder {
    pub folder: String,
    pub error: String,
}

pub fn open(path: &Path) -> rusqlite::Result<Connection> {
    let conn = Connection::open(path)?;
    conn.execute_batch("PRAGMA foreign_keys = ON;")?;
    migrate(&conn)?;
    rebuild_if_parser_changed(&conn)?;
    Ok(conn)
}

#[cfg(test)]
pub fn open_in_memory() -> rusqlite::Result<Connection> {
    let conn = Connection::open_in_memory()?;
    conn.execute_batch("PRAGMA foreign_keys = ON;")?;
    migrate(&conn)?;
    rebuild_if_parser_changed(&conn)?;
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
    conn.execute_batch("CREATE TABLE IF NOT EXISTS settings (key TEXT PRIMARY KEY, value TEXT NOT NULL);")?;
    if current < 4 {
        // v4: one row per match with the raw dissect JSON and the serialized `FullMatch`.
        // Older match tables (v1-v3) are dropped, settings stay; v3 rows have no raw JSON, so
        // the next refresh re-imports the replay folder once.
        conn.execute_batch(
            "DROP TABLE IF EXISTS kill_events;
            DROP TABLE IF EXISTS round_players;
            DROP TABLE IF EXISTS rounds;
            DROP TABLE IF EXISTS match_players;
            DROP TABLE IF EXISTS matches;
            CREATE TABLE matches (
                match_id    TEXT PRIMARY KEY,
                folder      TEXT NOT NULL UNIQUE,
                fingerprint TEXT NOT NULL,
                status      TEXT NOT NULL,
                season      TEXT NOT NULL,
                timestamp   TEXT NOT NULL,
                raw_json    BLOB NOT NULL,
                full_json   TEXT NOT NULL
            );
            CREATE INDEX idx_matches_status_time ON matches(status, timestamp);
            ALTER TABLE meta ADD COLUMN parser_version INTEGER NOT NULL DEFAULT 0;",
        )?;
    }
    // Kill clips (v4): created whenever it is missing, independent of the schema number, so it
    // does not collide with other tables added in parallel. Rows point at match IDs without a
    // foreign key (a clip is cut when a round ends, before the match row exists).
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS clips (
            id          INTEGER PRIMARY KEY AUTOINCREMENT,
            match_id    TEXT NOT NULL,
            round       INTEGER NOT NULL,
            kind        TEXT NOT NULL,
            time_s      INTEGER NOT NULL,
            killer      TEXT NOT NULL DEFAULT '',
            victim      TEXT NOT NULL DEFAULT '',
            path        TEXT NOT NULL,
            starred     INTEGER NOT NULL DEFAULT 0,
            size        INTEGER NOT NULL,
            start_ms    INTEGER NOT NULL DEFAULT 0,
            duration_ms INTEGER NOT NULL DEFAULT 0,
            created_ms  INTEGER NOT NULL,
            UNIQUE (match_id, round, kind, time_s, victim)
        );
        CREATE INDEX IF NOT EXISTS idx_clips_match ON clips(match_id);",
    )?;
    // Future steps: `if current < 5 { ... }`
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
    conn.execute_batch("DELETE FROM matches;")
}

// ---------------------------------------------------------------------------------------------
// Raw JSON: compression, season, match end

fn compress(s: &str) -> Vec<u8> {
    let mut enc = ZlibEncoder::new(Vec::new(), Compression::default());
    // Writing into a Vec cannot fail.
    enc.write_all(s.as_bytes()).expect("in-memory write");
    enc.finish().expect("in-memory write")
}

fn decompress(b: &[u8]) -> Result<String, String> {
    let mut s = String::new();
    ZlibDecoder::new(b).read_to_string(&mut s).map_err(|e| format!("cannot decompress raw JSON: {e}"))?;
    Ok(s)
}

/// `Y11S3_Alpha04` -> `Y11S3`; empty when the replay has no recognisable game version.
pub fn season_of(raw: &Value) -> String {
    let version = raw["rounds"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|r| r["gameVersion"].as_str())
        .find(|v| !v.is_empty())
        .unwrap_or("");
    let prefix = version.split('_').next().unwrap_or("");
    let digits = |s: &str| !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit());
    let ok = prefix
        .strip_prefix('Y')
        .and_then(|rest| rest.split_once('S'))
        .is_some_and(|(y, s)| digits(y) && digits(s));
    if ok { prefix.to_string() } else { String::new() }
}

/// Rounds a team needs to win, and whether a tie one round short of that goes to overtime
/// (then one more round is needed, e.g. Ranked 3-3 -> first to 5).
fn win_rule(match_type: &str) -> (u32, bool) {
    let t = match_type.to_ascii_lowercase();
    if t.contains("quick") { (3, false) } else { (4, true) }
}

/// Did the match in this r6-dissect output end? Uses the final team scores of the last round
/// (falling back to counting the per-round `won` flags) and the mode's win rule.
pub fn match_finished(raw: &Value) -> bool {
    let Some(rounds) = raw["rounds"].as_array().filter(|r| !r.is_empty()) else {
        return false;
    };
    let last = rounds.iter().max_by_key(|r| r["roundNumber"].as_u64().unwrap_or(0)).unwrap_or(&rounds[0]);
    let score = |r: &Value, i: usize| r["teams"][i]["score"].as_u64().unwrap_or(0) as u32;
    let (mut a, mut b) = (score(last, 0), score(last, 1));
    if a == 0 && b == 0 {
        // Scores missing: count clean round wins instead.
        for r in rounds {
            let won = |i: usize| r["teams"][i]["won"].as_bool().unwrap_or(false);
            match (won(0), won(1)) {
                (true, false) => a += 1,
                (false, true) => b += 1,
                _ => {}
            }
        }
    }
    let (hi, lo) = (a.max(b), a.min(b));
    let (target, overtime) = win_rule(rounds[0]["matchType"]["name"].as_str().unwrap_or(""));
    if overtime {
        (hi >= target && lo + 2 <= target) || hi > target
    } else {
        hi >= target
    }
}

/// One dissect output, converted and ready to store.
struct Imported {
    full: FullMatch,
    full_json: String,
    season: String,
    finished: bool,
}

fn import_raw(raw: &str) -> Result<Imported, String> {
    let value: Value = serde_json::from_str(raw).map_err(|e| format!("invalid r6-dissect JSON: {e}"))?;
    let full = r6k_core::full_match_from_json(raw).map_err(|e| format!("cannot convert match: {e}"))?;
    if full.match_id.is_empty() {
        return Err("replay has no match ID".into());
    }
    let full_json = serde_json::to_string(&full).map_err(|e| format!("cannot serialize match: {e}"))?;
    Ok(Imported { season: season_of(&value), finished: match_finished(&value), full, full_json })
}

// ---------------------------------------------------------------------------------------------
// Rows

struct StoredRow {
    fingerprint: String,
    status: Status,
}

fn stored_row(conn: &Connection, folder: &str) -> rusqlite::Result<Option<StoredRow>> {
    conn.query_row("SELECT fingerprint, status FROM matches WHERE folder = ?1", [folder], |r| {
        Ok(StoredRow { fingerprint: r.get(0)?, status: Status::from_str(&r.get::<_, String>(1)?) })
    })
    .optional()
}

/// Writes (or replaces) the row of `folder`. Returns false when the match ID is already stored
/// under another folder (nothing is written then).
fn put_match(
    conn: &mut Connection,
    folder: &str,
    fingerprint: &str,
    status: Status,
    raw: &str,
    m: &Imported,
) -> rusqlite::Result<bool> {
    let tx = conn.transaction()?;
    let owner: Option<String> = tx
        .query_row("SELECT folder FROM matches WHERE match_id = ?1", [&m.full.match_id], |r| r.get(0))
        .optional()?;
    if owner.is_some_and(|f| f != folder) {
        return Ok(false);
    }
    tx.execute("DELETE FROM matches WHERE folder = ?1", [folder])?;
    tx.execute(
        "INSERT INTO matches (match_id, folder, fingerprint, status, season, timestamp, raw_json, full_json)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
        params![
            m.full.match_id,
            folder,
            fingerprint,
            status.as_str(),
            m.season,
            m.full.timestamp,
            compress(raw),
            m.full_json
        ],
    )?;
    tx.commit()?;
    Ok(true)
}

fn set_status(conn: &Connection, folder: &str, status: Status) -> rusqlite::Result<()> {
    conn.execute("UPDATE matches SET status = ?1 WHERE folder = ?2", params![status.as_str(), folder])?;
    Ok(())
}

/// Test helper: stores a ready `FullMatch` as a complete match (its raw JSON is a stub, so a
/// parser rebuild skips it). Returns false when the match ID is already stored.
#[cfg(test)]
pub fn insert_full_match(conn: &mut Connection, m: &FullMatch, folder: &str) -> rusqlite::Result<bool> {
    let exists = conn
        .query_row("SELECT 1 FROM matches WHERE match_id = ?1 OR folder = ?2", params![m.match_id, folder], |_| Ok(()))
        .optional()?
        .is_some();
    if exists {
        return Ok(false);
    }
    conn.execute(
        "INSERT INTO matches (match_id, folder, fingerprint, status, season, timestamp, raw_json, full_json)
         VALUES (?1, ?2, '', 'complete', '', ?3, ?4, ?5)",
        params![m.match_id, folder, m.timestamp, compress("{}"), serde_json::to_string(m).unwrap()],
    )?;
    Ok(true)
}

/// Rebuilds `full_json` (and season / timestamp / end state) of every row from its raw JSON.
/// Rows that fail keep their old data. Returns the number of rebuilt rows.
pub fn rebuild_all(conn: &Connection) -> rusqlite::Result<u32> {
    let rows: Vec<(String, Vec<u8>, String)> = conn
        .prepare("SELECT match_id, raw_json, status FROM matches")?
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?
        .collect::<rusqlite::Result<_>>()?;
    let mut done = 0;
    for (id, raw, status) in rows {
        match decompress(&raw).and_then(|s| import_raw(&s)) {
            Ok(m) if m.full.match_id == id => {
                let status = if m.finished { Status::Complete } else { Status::from_str(&status) };
                conn.execute(
                    "UPDATE matches SET full_json = ?1, season = ?2, timestamp = ?3, status = ?4 WHERE match_id = ?5",
                    params![m.full_json, m.season, m.full.timestamp, status.as_str(), id],
                )?;
                done += 1;
            }
            Ok(_) => log::warn!("rebuild: {id}: raw JSON holds another match ID; kept as is"),
            Err(e) => log::warn!("rebuild: {id}: {e}; kept as is"),
        }
    }
    Ok(done)
}

fn rebuild_if_parser_changed(conn: &Connection) -> rusqlite::Result<()> {
    let stored: i64 = conn.query_row("SELECT parser_version FROM meta LIMIT 1", [], |r| r.get(0))?;
    if stored != PARSER_VERSION {
        let n = rebuild_all(conn)?;
        log::info!("parser version {stored} -> {PARSER_VERSION}: rebuilt {n} match(es) from raw JSON");
        // Raw JSON from an older parser lacks what the new parser reads. Marking the fingerprints
        // stale makes the next refresh re-parse every match whose replay folder still exists;
        // matches whose folder is gone keep the rebuild from raw JSON above.
        conn.execute("UPDATE matches SET fingerprint = 'reparse:' || fingerprint WHERE fingerprint NOT LIKE 'reparse:%'", [])?;
        conn.execute("UPDATE meta SET parser_version = ?1", [PARSER_VERSION])?;
    }
    Ok(())
}

/// Deserializes a stored `full_json`. If the model changed and it no longer fits, the row is
/// rebuilt from its raw JSON (and written back); `None` when that fails too.
fn decode(conn: &Connection, id: &str, json: &str) -> Option<FullMatch> {
    match serde_json::from_str(json) {
        Ok(m) => Some(m),
        Err(e) => {
            log::warn!("{id}: stored match does not fit the model ({e}); rebuilding from raw JSON");
            let raw: Vec<u8> = conn
                .query_row("SELECT raw_json FROM matches WHERE match_id = ?1", [id], |r| r.get(0))
                .ok()?;
            match decompress(&raw).and_then(|s| import_raw(&s)) {
                Ok(m) => {
                    if let Err(e) = conn.execute(
                        "UPDATE matches SET full_json = ?1 WHERE match_id = ?2",
                        params![m.full_json, id],
                    ) {
                        log::warn!("{id}: cannot write rebuilt match: {e}");
                    }
                    Some(m.full)
                }
                Err(e) => {
                    log::warn!("{id}: rebuild failed: {e}; match skipped");
                    None
                }
            }
        }
    }
}

/// All countable stored matches (complete and abandoned) in full, oldest first.
pub fn load_full_matches(conn: &Connection) -> rusqlite::Result<Vec<FullMatch>> {
    let rows: Vec<(String, String)> = conn
        .prepare(
            "SELECT match_id, full_json FROM matches WHERE status != 'in_progress'
             ORDER BY timestamp, match_id",
        )?
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
        .collect::<rusqlite::Result<_>>()?;
    Ok(rows.iter().filter_map(|(id, json)| decode(conn, id, json)).collect())
}

/// One countable stored match in full, or `None` for an unknown (or in-progress) ID.
pub fn load_full_match(conn: &Connection, id: &str) -> rusqlite::Result<Option<FullMatch>> {
    let json: Option<String> = conn
        .query_row(
            "SELECT full_json FROM matches WHERE match_id = ?1 AND status != 'in_progress'",
            [id],
            |r| r.get(0),
        )
        .optional()?;
    Ok(json.and_then(|j| decode(conn, id, &j)))
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

/// All countable stored matches as the tracked player's `Match`, oldest first (v1 view).
pub fn load_matches(conn: &Connection) -> rusqlite::Result<Vec<Match>> {
    Ok(load_full_matches(conn)?.iter().map(to_match).collect())
}

// ---------------------------------------------------------------------------------------------
// Folder import

struct FolderState {
    /// File names and sizes, sorted: `name:size|name:size`.
    fingerprint: String,
    /// Newest file modification (the folder's own time when it has no files).
    last_change: SystemTime,
}

fn folder_state(path: &Path) -> std::io::Result<FolderState> {
    let mut files = Vec::new();
    let mut last: Option<SystemTime> = None;
    for e in std::fs::read_dir(path)? {
        let e = e?;
        let md = e.metadata()?;
        if !md.is_file() {
            continue;
        }
        files.push(format!("{}:{}", e.file_name().to_string_lossy(), md.len()));
        if let Ok(t) = md.modified() {
            last = Some(last.map_or(t, |l| l.max(t)));
        }
    }
    files.sort();
    let last_change = match last {
        Some(t) => t,
        None => std::fs::metadata(path)?.modified()?,
    };
    Ok(FolderState { fingerprint: files.join("|"), last_change })
}

/// Scans `dir` for `Match-*` folders and imports new or changed ones. `parse` runs r6-dissect on
/// a folder and returns its JSON output. A broken folder never stops the others.
pub fn refresh<F>(db: &Db, dir: &Path, parse: F) -> Result<RefreshResult, String>
where
    F: Fn(&Path) -> Result<String, String>,
{
    refresh_at(db, dir, parse, SystemTime::now())
}

fn refresh_at<F>(db: &Db, dir: &Path, parse: F, now: SystemTime) -> Result<RefreshResult, String>
where
    F: Fn(&Path) -> Result<String, String>,
{
    let lock = || db.0.lock().map_err(|_| "database lock poisoned".to_string());
    let rd = std::fs::read_dir(dir)
        .map_err(|e| format!("cannot read replay folder {}: {e}", dir.display()))?;
    let mut folders: Vec<_> = rd
        .flatten()
        .filter(|e| e.path().is_dir() && e.file_name().to_string_lossy().starts_with("Match-"))
        .collect();
    folders.sort_by_key(|e| e.file_name());

    let mut result = RefreshResult::default();
    for entry in folders {
        let name = entry.file_name().to_string_lossy().into_owned();
        let path = entry.path();
        let state = match folder_state(&path) {
            Ok(s) => s,
            Err(e) => {
                // Usually a folder the game is still creating or deleting.
                log::info!("refresh: {name}: cannot read folder ({e}); pending");
                result.pending += 1;
                continue;
            }
        };
        let quiet = now.duration_since(state.last_change).unwrap_or(Duration::ZERO);
        let known = stored_row(&*lock()?, &name).map_err(|e| format!("database error: {e}"))?;

        if let Some(k) = known.as_ref().filter(|k| k.fingerprint == state.fingerprint) {
            if k.status == Status::InProgress {
                if quiet >= ABANDON_AFTER {
                    set_status(&*lock()?, &name, Status::Abandoned).map_err(|e| format!("database error: {e}"))?;
                    log::info!("refresh: {name}: unfinished and quiet for 20 min; abandoned");
                    result.new_matches += 1;
                } else {
                    result.in_progress += 1;
                }
            }
            continue;
        }
        if quiet < SETTLE {
            log::debug!("refresh: {name}: changed {}s ago; pending", quiet.as_secs());
            result.pending += 1;
            continue;
        }

        let parsed = parse(&path).and_then(|raw| import_raw(&raw).map(|m| (raw, m)));
        let (raw, m) = match parsed {
            Ok(x) => x,
            Err(error) if quiet < ABANDON_AFTER => {
                // Most likely a match still being recorded; try again later.
                log::info!("refresh: {name}: not readable yet ({error}); pending");
                result.pending += 1;
                continue;
            }
            Err(error) => {
                log::warn!("refresh: {name} failed: {error}");
                result.failed.push(FailedFolder { folder: name, error });
                continue;
            }
        };
        let status = if m.finished {
            Status::Complete
        } else if quiet >= ABANDON_AFTER {
            Status::Abandoned
        } else {
            Status::InProgress
        };
        let written = put_match(&mut *lock()?, &name, &state.fingerprint, status, &raw, &m)
            .map_err(|e| format!("database error: {e}"));
        match written {
            Ok(true) => {}
            Ok(false) => {
                log::debug!("refresh: {name}: match already stored under another folder");
                continue;
            }
            Err(error) => {
                log::warn!("refresh: {name} failed: {error}");
                result.failed.push(FailedFolder { folder: name, error });
                continue;
            }
        }
        let was_visible = known.as_ref().is_some_and(|k| k.status.visible());
        match (was_visible, status.visible()) {
            (false, true) => result.new_matches += 1,
            (true, true) => result.updated_matches += 1,
            (true, false) => {
                result.updated_matches += 1;
                result.in_progress += 1;
            }
            (false, false) => result.in_progress += 1,
        }
        log::info!("refresh: {name}: stored as {}", status.as_str());
    }
    log::info!(
        "refresh: {} new, {} updated, {} pending, {} in progress, {} failed",
        result.new_matches,
        result.updated_matches,
        result.pending,
        result.in_progress,
        result.failed.len()
    );
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use r6k_core::Side;
    use serde_json::json;
    use std::path::PathBuf;

    /// r6-dissect style output: one entry per round with the running team scores.
    fn raw(id: &str, scores: &[(u32, u32)], match_type: &str) -> String {
        let mut prev = (0, 0);
        let rounds: Vec<Value> = scores
            .iter()
            .enumerate()
            .map(|(i, &(a, b))| {
                let won0 = a > prev.0;
                prev = (a, b);
                json!({
                    "gameVersion": "Y11S3_Alpha04",
                    "timestamp": "2026-11-02T10:00:00Z",
                    "matchType": { "name": match_type, "id": 1 },
                    "map": { "name": "Lair", "id": 7 },
                    "gamemode": { "name": "Bomb", "id": 2 },
                    "site": "Lobby",
                    "recordingProfileID": "me",
                    "matchID": id,
                    "roundNumber": i,
                    "teams": [
                        { "score": a, "won": won0, "role": if i % 2 == 0 { "Defense" } else { "Attack" } },
                        { "score": b, "won": !won0, "role": if i % 2 == 0 { "Attack" } else { "Defense" } }
                    ],
                    "players": [
                        { "profileID": "me", "username": "Me", "teamIndex": 0, "operator": { "name": "Zofia", "id": 5 } },
                        { "profileID": "foe", "username": "Foe", "teamIndex": 1, "operator": { "name": "Ash", "id": 6 } }
                    ],
                    "matchFeedback": [
                        { "type": { "name": "Kill" }, "username": "Me", "target": "Foe", "headshot": true, "timeInSeconds": 120 }
                    ],
                    "stats": [
                        { "username": "Me", "kills": 1, "died": false, "headshots": 1 },
                        { "username": "Foe", "kills": 0, "died": true, "headshots": 0 }
                    ]
                })
            })
            .collect();
        json!({ "rounds": rounds }).to_string()
    }

    const DONE: &[(u32, u32)] = &[(1, 0), (2, 0), (3, 0), (4, 0)];
    const HALF: &[(u32, u32)] = &[(1, 0), (2, 0)];

    fn ago(secs: u64) -> SystemTime {
        SystemTime::now() - Duration::from_secs(secs)
    }

    /// Creates `dir/name` with one round file of `size` bytes, last modified `mtime`.
    fn folder(dir: &Path, name: &str, files: &[(&str, usize)], mtime: SystemTime) -> PathBuf {
        let p = dir.join(name);
        std::fs::create_dir_all(&p).unwrap();
        for (f, size) in files {
            let path = p.join(f);
            std::fs::write(&path, vec![0u8; *size]).unwrap();
            std::fs::File::options().write(true).open(&path).unwrap().set_modified(mtime).unwrap();
        }
        p
    }

    fn db() -> Db {
        Db(Mutex::new(open_in_memory().unwrap()))
    }

    fn stored(db: &Db, folder: &str) -> Option<(String, String)> {
        db.0.lock()
            .unwrap()
            .query_row("SELECT status, season FROM matches WHERE folder = ?1", [folder], |r| Ok((r.get(0)?, r.get(1)?)))
            .optional()
            .unwrap()
    }

    #[test]
    fn season_from_game_version() {
        let v: Value = serde_json::from_str(&raw("a", DONE, "Ranked")).unwrap();
        assert_eq!(season_of(&v), "Y11S3");
        assert_eq!(season_of(&json!({ "rounds": [{ "gameVersion": "Y9S10" }] })), "Y9S10");
        assert_eq!(season_of(&json!({ "rounds": [{ "gameVersion": "weird_1" }] })), "");
        assert_eq!(season_of(&json!({ "rounds": [] })), "");
    }

    #[test]
    fn match_end_rules() {
        let fin = |s: &[(u32, u32)], t: &str| match_finished(&serde_json::from_str(&raw("a", s, t)).unwrap());
        assert!(fin(DONE, "Ranked"));
        assert!(!fin(HALF, "Ranked"));
        assert!(fin(&[(1, 0), (2, 0), (3, 0), (3, 1), (3, 2), (4, 2)], "Ranked"));
        // 3-3 goes to overtime: 4-3 is not the end, 5-3 and 5-4 are.
        assert!(!fin(&[(1, 0), (2, 0), (3, 0), (3, 1), (3, 2), (3, 3), (4, 3)], "Ranked"));
        assert!(fin(&[(1, 0), (2, 0), (3, 0), (3, 1), (3, 2), (3, 3), (4, 3), (5, 3)], "Ranked"));
        assert!(fin(&[(0, 1), (0, 2), (0, 3), (1, 3), (2, 3), (3, 3), (3, 4), (4, 4), (4, 5)], "Unranked"));
        assert!(fin(&[(1, 0), (2, 0), (3, 0)], "Quick Match"));
        assert!(!match_finished(&json!({ "rounds": [] })));
        // Scores missing: the per-round winners count.
        let flags = json!({ "rounds": (0..4).map(|i| json!({ "roundNumber": i, "matchType": { "name": "Ranked" },
            "teams": [{ "won": true }, { "won": false }] })).collect::<Vec<_>>() });
        assert!(match_finished(&flags));
    }

    #[test]
    fn v3_to_v4_drops_matches_keeps_settings() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE meta (schema_version INTEGER NOT NULL); INSERT INTO meta VALUES (3);
             CREATE TABLE settings (key TEXT PRIMARY KEY, value TEXT NOT NULL);
             INSERT INTO settings VALUES ('replay_dir', 'C:\\replays');
             CREATE TABLE matches (match_id TEXT PRIMARY KEY, timestamp TEXT NOT NULL, map_id INTEGER NOT NULL,
               match_type TEXT NOT NULL, gamemode TEXT NOT NULL, won INTEGER NOT NULL, folder_name TEXT NOT NULL,
               score_us INTEGER NOT NULL DEFAULT 0, score_them INTEGER NOT NULL DEFAULT 0, me_profile_id TEXT NOT NULL DEFAULT '');
             CREATE TABLE match_players (match_id TEXT, profile_id TEXT, username TEXT, team_index INTEGER, is_me INTEGER);
             CREATE TABLE rounds (match_id TEXT, round INTEGER);
             CREATE TABLE round_players (match_id TEXT, round INTEGER, profile_id TEXT);
             CREATE TABLE kill_events (match_id TEXT, round INTEGER, seq INTEGER);
             INSERT INTO matches (match_id, timestamp, map_id, match_type, gamemode, won, folder_name)
               VALUES ('old', 't', 1, 'Ranked', 'Bomb', 1, 'Match-old');",
        )
        .unwrap();
        migrate(&conn).unwrap();
        rebuild_if_parser_changed(&conn).unwrap();
        assert!(load_full_matches(&conn).unwrap().is_empty());
        assert_eq!(get_setting(&conn, KEY_REPLAY_DIR).unwrap().as_deref(), Some("C:\\replays"));
        let (v, p): (i64, i64) =
            conn.query_row("SELECT schema_version, parser_version FROM meta", [], |r| Ok((r.get(0)?, r.get(1)?))).unwrap();
        assert_eq!((v, p), (4, PARSER_VERSION));
        let old_tables: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE name IN ('rounds','round_players','kill_events','match_players')",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(old_tables, 0);
        // A second run must not wipe freshly stored data.
        let mut conn = conn;
        let m = r6k_core::full_match_from_json(&raw("b", DONE, "Ranked")).unwrap();
        assert!(insert_full_match(&mut conn, &m, "Match-b").unwrap());
        migrate(&conn).unwrap();
        assert_eq!(load_full_matches(&conn).unwrap().len(), 1);
    }

    #[test]
    fn refresh_imports_and_roundtrips() {
        let t = tempfile::tempdir().unwrap();
        folder(t.path(), "Match-a", &[("R01.rec", 10)], ago(600));
        std::fs::create_dir(t.path().join("Other")).unwrap();
        let db = db();
        let parse = |_: &Path| Ok(raw("a", DONE, "Ranked"));
        let r = refresh(&db, t.path(), parse).unwrap();
        assert_eq!((r.new_matches, r.updated_matches, r.pending, r.failed.len()), (1, 0, 0, 0));
        assert_eq!(stored(&db, "Match-a"), Some(("complete".into(), "Y11S3".into())));

        let conn = db.0.lock().unwrap();
        let back = load_full_match(&conn, "a").unwrap().unwrap();
        let direct = r6k_core::full_match_from_json(&raw("a", DONE, "Ranked")).unwrap();
        assert_eq!(serde_json::to_value(&back).unwrap(), serde_json::to_value(&direct).unwrap());
        let ms = load_matches(&conn).unwrap();
        assert_eq!(ms.len(), 1);
        assert_eq!(ms[0].rounds.len(), 4);
        assert_eq!(ms[0].kills(), 4);
        assert_eq!(ms[0].rounds[0].side, Side::Defense);
        assert!(ms[0].rounds[0].won && ms[0].rounds[0].first_blood);
        drop(conn);

        // Unchanged folder: nothing to do, and the parser is not called again.
        let r = refresh(&db, t.path(), |_| -> Result<String, String> { panic!("must not re-parse") }).unwrap();
        assert_eq!(r, RefreshResult::default());
        assert!(refresh(&db, &t.path().join("nope"), parse).is_err());
    }

    #[test]
    fn fingerprint_change_replaces_the_row() {
        let t = tempfile::tempdir().unwrap();
        folder(t.path(), "Match-a", &[("R01.rec", 10)], ago(600));
        let db = db();
        refresh(&db, t.path(), |_| Ok(raw("a", DONE, "Ranked"))).unwrap();
        // The folder grows (same names, other size): re-parsed and replaced.
        folder(t.path(), "Match-a", &[("R01.rec", 20)], ago(600));
        let longer = &[(1, 0), (2, 0), (3, 0), (3, 1), (4, 1)];
        let r = refresh(&db, t.path(), |_| Ok(raw("a", longer, "Ranked"))).unwrap();
        assert_eq!((r.new_matches, r.updated_matches), (0, 1));
        let conn = db.0.lock().unwrap();
        assert_eq!(load_full_match(&conn, "a").unwrap().unwrap().rounds.len(), 5);
        let n: i64 = conn.query_row("SELECT COUNT(*) FROM matches", [], |r| r.get(0)).unwrap();
        assert_eq!(n, 1);
    }

    #[test]
    fn recent_or_unreadable_folders_are_pending() {
        let t = tempfile::tempdir().unwrap();
        folder(t.path(), "Match-fresh", &[("R01.rec", 10)], ago(5));
        folder(t.path(), "Match-writing", &[("R01.rec", 10)], ago(120));
        folder(t.path(), "Match-broken", &[("R01.rec", 10)], ago(3 * 3600));
        let db = db();
        let parse = |p: &Path| -> Result<String, String> {
            match p.file_name().unwrap().to_str().unwrap() {
                "Match-fresh" => panic!("a folder that changed within 60 s must not be parsed"),
                _ => Err("replay file is corrupted".into()),
            }
        };
        let r = refresh(&db, t.path(), parse).unwrap();
        assert_eq!(r.pending, 2);
        assert_eq!(r.failed.len(), 1);
        assert_eq!(r.failed[0].folder, "Match-broken");
        assert_eq!(r.new_matches, 0);
    }

    #[test]
    fn in_progress_is_hidden_then_completes_or_is_abandoned() {
        let t = tempfile::tempdir().unwrap();
        folder(t.path(), "Match-a", &[("R01.rec", 10)], ago(120));
        folder(t.path(), "Match-b", &[("R01.rec", 10)], ago(120));
        let db = db();
        let parse = |p: &Path| -> Result<String, String> {
            let id = if p.ends_with("Match-a") { "a" } else { "b" };
            Ok(raw(id, HALF, "Ranked"))
        };
        let r = refresh(&db, t.path(), parse).unwrap();
        assert_eq!((r.new_matches, r.in_progress), (0, 2));
        assert_eq!(stored(&db, "Match-a").unwrap().0, "in_progress");
        {
            let conn = db.0.lock().unwrap();
            assert!(load_full_matches(&conn).unwrap().is_empty());
            assert!(load_matches(&conn).unwrap().is_empty());
            assert!(load_full_match(&conn, "a").unwrap().is_none());
        }

        // "a" gets more rounds and ends; "b" stays silent for 20 minutes and is left.
        folder(t.path(), "Match-a", &[("R01.rec", 10), ("R02.rec", 10)], ago(120));
        let parse = |p: &Path| -> Result<String, String> {
            assert!(p.ends_with("Match-a"), "b is unchanged and must not be re-parsed");
            Ok(raw("a", DONE, "Ranked"))
        };
        let later = SystemTime::now() + Duration::from_secs(20 * 60);
        let r = refresh_at(&db, t.path(), parse, later).unwrap();
        assert_eq!((r.new_matches, r.updated_matches, r.in_progress), (2, 0, 0));
        assert_eq!(stored(&db, "Match-a").unwrap().0, "complete");
        assert_eq!(stored(&db, "Match-b").unwrap().0, "abandoned");
        assert_eq!(load_full_matches(&db.0.lock().unwrap()).unwrap().len(), 2);
    }

    #[test]
    fn parser_change_rebuilds_full_from_raw() {
        let t = tempfile::tempdir().unwrap();
        let file = t.path().join("r6koc.db");
        let rp = tempfile::tempdir().unwrap();
        folder(rp.path(), "Match-a", &[("R01.rec", 10)], ago(600));
        {
            let db = Db(Mutex::new(open(&file).unwrap()));
            refresh(&db, rp.path(), |_| Ok(raw("a", DONE, "Ranked"))).unwrap();
            // Simulate an older parser: stale full_json and version.
            let conn = db.0.lock().unwrap();
            conn.execute("UPDATE matches SET full_json = '{\"stale\":true}', season = ''", []).unwrap();
            conn.execute("UPDATE meta SET parser_version = 0", []).unwrap();
        }
        let conn = open(&file).unwrap();
        let m = load_full_match(&conn, "a").unwrap().expect("rebuilt from raw JSON");
        assert_eq!(m.rounds.len(), 4);
        let (season, json): (String, String) =
            conn.query_row("SELECT season, full_json FROM matches", [], |r| Ok((r.get(0)?, r.get(1)?))).unwrap();
        assert_eq!(season, "Y11S3");
        assert!(!json.contains("stale"));
        let p: i64 = conn.query_row("SELECT parser_version FROM meta", [], |r| r.get(0)).unwrap();
        assert_eq!(p, PARSER_VERSION);
        let fp: String = conn.query_row("SELECT fingerprint FROM matches", [], |r| r.get(0)).unwrap();
        assert!(fp.starts_with("reparse:"), "a parser change marks the folder for re-parsing");
    }

    #[test]
    fn stale_full_json_is_repaired_on_load() {
        let t = tempfile::tempdir().unwrap();
        folder(t.path(), "Match-a", &[("R01.rec", 10)], ago(600));
        let db = db();
        refresh(&db, t.path(), |_| Ok(raw("a", DONE, "Ranked"))).unwrap();
        let conn = db.0.lock().unwrap();
        conn.execute("UPDATE matches SET full_json = '{}'", []).unwrap();
        assert_eq!(load_full_matches(&conn).unwrap().len(), 1);
        let json: String = conn.query_row("SELECT full_json FROM matches", [], |r| r.get(0)).unwrap();
        assert!(json.contains("\"match_id\":\"a\""));
    }

    #[test]
    fn reset_wipes_matches_keeps_settings() {
        let mut conn = open_in_memory().unwrap();
        set_setting(&conn, KEY_REPLAY_DIR, "C:\\x").unwrap();
        set_setting(&conn, KEY_REPLAY_DIR, "C:\\y").unwrap();
        let m = r6k_core::full_match_from_json(&raw("a", DONE, "Ranked")).unwrap();
        assert!(insert_full_match(&mut conn, &m, "Match-a").unwrap());
        assert!(!insert_full_match(&mut conn, &m, "Match-b").unwrap());
        reset(&conn).unwrap();
        assert!(load_full_matches(&conn).unwrap().is_empty());
        assert_eq!(get_setting(&conn, KEY_REPLAY_DIR).unwrap().as_deref(), Some("C:\\y"));
    }

    #[test]
    fn same_match_in_two_folders_is_stored_once() {
        let t = tempfile::tempdir().unwrap();
        folder(t.path(), "Match-1", &[("R01.rec", 10)], ago(600));
        folder(t.path(), "Match-2", &[("R01.rec", 10)], ago(600));
        let db = db();
        let r = refresh(&db, t.path(), |_| Ok(raw("same", DONE, "Ranked"))).unwrap();
        assert_eq!((r.new_matches, r.failed.len()), (1, 0));
    }
}
