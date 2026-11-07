//! Online stats from the Arenyze API (v4 spec, user decision 1): rank and the other official
//! values come from the **user's own** free API key. This replaces the Ubisoft sign-in.
//!
//! # Network rule
//! * The app talks only to `public-api.arenyze.com`, only when the user entered a key, and only
//!   for the tracked player's own profile (username of the newest stored match, platform `uplay`).
//! * Calls happen on a manual refresh, or when the 15-minute cache is stale and the last attempt
//!   is at least 10 minutes old. There is no background timer.
//!
//! # Secrets
//! The key lives in the Windows Credential Manager (`keyring`). It is never logged, never written
//! to the database and never part of an error message. The "save last response" diagnostics file
//! has every occurrence of the key replaced before it is written.
//!
//! # Response shapes
//! `docs/arenyze-api.md` shows only empty example segments, so the parser is tolerant: it looks for
//! Tracker-Network-style segments (`type`, `attributes`, `stats.<key>.{value, metadata}`) anywhere in
//! the Profile V2 / Full Stats V2 response and picks the newest ranked season. The rank name comes
//! from the tier name when present (`Emerald II` style, case-insensitive) and from the numeric rank
//! id (0..36) otherwise. A missing field stays `null`; nothing is invented.

use std::sync::Mutex as StdMutex;
use std::time::{Duration, Instant};

use chrono::{DateTime, SecondsFormat, Utc};
use r6k_core::query::{Filter, Mode, Range, SideFilter};
use r6k_core::FullMatch;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use tauri::async_runtime::Mutex;

use crate::{lock, store, Cmd};

const BASE_URL: &str = "https://public-api.arenyze.com/r6/api";
const KEY_PAGE_URL: &str = "https://arenyze.com/profile#developer";
const USER_AGENT: &str = "R6Coach/0.1 (desktop; own-profile only)";
const PLATFORM: &str = "uplay";
/// The docs print `Uplay`, the SDK uses `uplay`; tried once when the first form is refused (HTTP 400).
const PLATFORM_ALT: &str = "Uplay";

const KEYRING_SERVICE: &str = "R6 Coach (Arenyze)";
const KEYRING_USER: &str = "api-key";
/// The Ubisoft sign-in of v3 stored its remember-me ticket here; it is deleted on first start.
const LEGACY_KEYRING_SERVICE: &str = "R6 Coach (Ubisoft)";
const LEGACY_KEYRING_USER: &str = "remember-me";
const LEGACY_SETTINGS: [&str; 2] = ["ubi_account", "ubi_ranked_profile"];

const KEY_CACHE: &str = "arenyze_profile_cache";
const KEY_QUOTA: &str = "arenyze_quota";
const KEY_LEGACY_DONE: &str = "arenyze_legacy_cleaned";

/// How long a fetched profile is served without a new request.
const CACHE_TTL_SECS: i64 = 15 * 60;
/// Minimum gap between two automatic network attempts (also after a failed one).
const AUTO_GAP: Duration = Duration::from_secs(10 * 60);
/// Minimum gap between two manual refreshes.
const MANUAL_GAP: Duration = Duration::from_secs(20);
const HTTP_TIMEOUT: Duration = Duration::from_secs(20);
const MAX_HISTORY_POINTS: usize = 60;

// ---------------------------------------------------------------- DTOs

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Scope {
    Season,
    All,
}

/// Official Arenyze totals (Ranked). `winPct` is 0-100 like the replay KPIs.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OfficialTotals {
    pub scope: Scope,
    pub kd: f64,
    pub win_pct: f64,
    pub kills: u64,
    pub deaths: u64,
    pub wins: u64,
    pub losses: u64,
    pub matches: u64,
    pub fetched_at: String,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct OfficialPair {
    pub season: Option<OfficialTotals>,
    pub all: Option<OfficialTotals>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RankPoint {
    pub label: Option<String>,
    pub rank_points: i64,
}

/// Official per-operator numbers of the current season (Operators V2, ranked).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OfficialOperator {
    pub name: String,
    /// `attack`, `defense` or empty when Arenyze did not say.
    pub side: String,
    pub rounds: u64,
    pub kd: f64,
    pub kills: u64,
    pub deaths: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OnlineProfile {
    /// Always `arenyze`.
    pub source: String,
    /// The player name the request was made for.
    pub username: String,
    pub season: Option<String>,
    pub rank_name: Option<String>,
    pub rank_points: Option<i64>,
    pub max_rank_name: Option<String>,
    pub max_rank_points: Option<i64>,
    /// Champion position; `null` below Champion.
    pub top_rank_position: Option<u32>,
    /// Local asset key under `/assets/ranks/<key>.png`.
    pub rank_icon: Option<String>,
    pub season_kd: Option<f64>,
    pub kills: Option<u64>,
    pub deaths: Option<u64>,
    pub wins: Option<u64>,
    pub losses: Option<u64>,
    pub abandons: Option<u64>,
    pub matches: Option<u64>,
    pub level: Option<u32>,
    pub profile_picture_url: Option<String>,
    pub rank_history: Vec<RankPoint>,
    pub operators: Vec<OfficialOperator>,
    pub fetched_at: String,
    pub official: OfficialPair,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct QuotaInfo {
    pub plan: Option<String>,
    pub used: Option<u64>,
    pub limit: Option<u64>,
    pub remaining: Option<u64>,
    pub checked_at: String,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ArenyzeStatus {
    pub has_key: bool,
    /// Tracked player (newest stored match); `null` until a replay is imported.
    pub username: Option<String>,
    pub quota: Option<QuotaInfo>,
    pub last_fetched_at: Option<String>,
    /// A raw response is in memory and can be saved for diagnostics.
    pub has_raw_response: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CachedProfile {
    username: String,
    profile: OnlineProfile,
}

// ---------------------------------------------------------------- stored, non-secret data

fn load_json<T: for<'de> Deserialize<'de>>(conn: &rusqlite::Connection, key: &str) -> Option<T> {
    let raw = store::get_setting(conn, key).ok().flatten()?;
    if raw.trim().is_empty() {
        return None;
    }
    serde_json::from_str(&raw).map_err(|e| log::warn!("arenyze: setting {key} unreadable: {e}")).ok()
}

fn save_json<T: Serialize>(conn: &rusqlite::Connection, key: &str, value: Option<&T>) {
    let raw = value.and_then(|v| serde_json::to_string(v).ok()).unwrap_or_default();
    if let Err(e) = store::set_setting(conn, key, &raw) {
        log::warn!("arenyze: cannot save setting {key}: {e}");
    }
}

// ---------------------------------------------------------------- API key (keyring)

fn keyring_entry(service: &str, user: &str) -> Option<keyring::Entry> {
    keyring::Entry::new(service, user)
        .map_err(|e| log::warn!("arenyze: credential store unavailable: {e}"))
        .ok()
}

fn key_load() -> Option<String> {
    match keyring_entry(KEYRING_SERVICE, KEYRING_USER)?.get_password() {
        Ok(k) if !k.trim().is_empty() => Some(k),
        Ok(_) | Err(keyring::Error::NoEntry) => None,
        Err(e) => {
            log::warn!("arenyze: cannot read the key: {e}");
            None
        }
    }
}

fn key_save(key: &str) -> Result<(), OnlineError> {
    let entry = keyring_entry(KEYRING_SERVICE, KEYRING_USER).ok_or(OnlineError::CredentialStore)?;
    entry.set_password(key).map_err(|e| {
        log::warn!("arenyze: cannot save the key: {e}");
        OnlineError::CredentialStore
    })
}

fn key_delete() {
    if let Some(entry) = keyring_entry(KEYRING_SERVICE, KEYRING_USER) {
        match entry.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => {}
            Err(e) => log::warn!("arenyze: cannot delete the key: {e}"),
        }
    }
}

/// Removes what the removed Ubisoft sign-in left behind (Credential Manager entry, stored
/// account and rank cache). Runs once; later starts only read one setting.
pub fn cleanup_legacy(conn: &rusqlite::Connection) {
    if store::get_setting(conn, KEY_LEGACY_DONE).ok().flatten().is_some_and(|v| v == "1") {
        return;
    }
    if let Some(entry) = keyring_entry(LEGACY_KEYRING_SERVICE, LEGACY_KEYRING_USER) {
        match entry.delete_credential() {
            Ok(()) => log::info!("arenyze: removed the old Ubisoft credential entry"),
            Err(keyring::Error::NoEntry) => {}
            Err(e) => log::warn!("arenyze: cannot remove the old Ubisoft credential entry: {e}"),
        }
    }
    for k in LEGACY_SETTINGS {
        if let Err(e) = store::set_setting(conn, k, "") {
            log::warn!("arenyze: cannot clear old setting {k}: {e}");
        }
    }
    if let Err(e) = store::set_setting(conn, KEY_LEGACY_DONE, "1") {
        log::warn!("arenyze: cannot mark legacy cleanup: {e}");
    }
}

// ---------------------------------------------------------------- errors

#[derive(Debug, Clone, PartialEq, Eq)]
enum OnlineError {
    InvalidKey,
    KeyExpired,
    BadKeyFormat,
    RateLimited,
    QuotaExceeded,
    PlayerNotFound,
    BadRequest,
    Network,
    Server(u16),
    Refused(u16),
    Unexpected(String),
    CredentialStore,
    TooSoon,
}

impl OnlineError {
    /// English text the UI can show as is. Never contains the key or response bodies.
    fn message(&self) -> String {
        match self {
            OnlineError::InvalidKey => "Arenyze did not accept this key. Check that you copied the whole key, or create a new one.".into(),
            OnlineError::KeyExpired => "Your Arenyze key has expired. Create a new key on arenyze.com and enter it here.".into(),
            OnlineError::BadKeyFormat => "That does not look like an Arenyze key. Paste the key without spaces.".into(),
            OnlineError::RateLimited => "Arenyze is limiting requests right now. Wait a minute and try again.".into(),
            OnlineError::QuotaExceeded => "Your monthly Arenyze call limit is used up. It resets on the 1st of the month (UTC).".into(),
            OnlineError::PlayerNotFound => "Arenyze could not find this player. Check that your Ubisoft name in the replays is correct.".into(),
            OnlineError::BadRequest => "Arenyze refused the request (HTTP 400).".into(),
            OnlineError::Network => "Cannot reach Arenyze. Check your internet connection.".into(),
            OnlineError::Server(s) => format!("Arenyze is not responding (HTTP {s}). Try again later."),
            OnlineError::Refused(s) => format!("Arenyze refused the request (HTTP {s}). Your key may not have access."),
            OnlineError::Unexpected(what) => format!("Unexpected answer from Arenyze ({what})."),
            OnlineError::CredentialStore => "Windows Credential Manager is not available, so the key cannot be stored.".into(),
            OnlineError::TooSoon => "Please wait a few seconds before refreshing again.".into(),
        }
    }

    /// Errors that mean "fix the key"; a cached profile must not hide them.
    fn is_key_problem(&self) -> bool {
        matches!(self, OnlineError::InvalidKey | OnlineError::KeyExpired)
    }
}

fn body_error_text(body: &str) -> String {
    serde_json::from_str::<Value>(body)
        .ok()
        .and_then(|v| {
            let code = v.get("error").and_then(Value::as_str).unwrap_or_default().to_string();
            let msg = v.get("message").and_then(Value::as_str).unwrap_or_default();
            Some(format!("{code} {msg}").to_ascii_uppercase())
        })
        .unwrap_or_default()
}

/// Maps a failed Arenyze answer to a user-facing error (codes from `docs/arenyze-api.md` 2.4).
fn classify_error(status: u16, body: &str) -> OnlineError {
    let text = body_error_text(body);
    match status {
        401 if text.contains("EXPIRED") => OnlineError::KeyExpired,
        401 => OnlineError::InvalidKey,
        429 if text.contains("QUOTA") || text.contains("MONTHLY") => OnlineError::QuotaExceeded,
        429 => OnlineError::RateLimited,
        404 if text.contains("ENDPOINT_NOT_FOUND") => OnlineError::Unexpected("HTTP 404".into()),
        404 => OnlineError::PlayerNotFound,
        400 => OnlineError::BadRequest,
        403 => OnlineError::Refused(status),
        500..=599 => OnlineError::Server(status),
        _ => OnlineError::Unexpected(format!("HTTP {status}")),
    }
}

fn net_error(e: reqwest::Error) -> OnlineError {
    // `reqwest::Error`'s text carries the URL only (no headers), so it is safe to log.
    log::warn!("arenyze: request failed: {e}");
    if e.is_decode() || e.is_body() {
        OnlineError::Unexpected("unreadable response".into())
    } else {
        OnlineError::Network
    }
}

// ---------------------------------------------------------------- rank table

/// Ranked 2.0 / Siege X rank table for seasons 28+ (danielwerg/r6data `src/ranks.ts`, also
/// siegeapi `ranks_v6`): rank id -> (name, icon key under `/assets/ranks/<key>.png`).
static RANKS: [(&str, &str); 37] = [
    ("Unranked", "unranked"),
    ("Copper V", "copper-5"),
    ("Copper IV", "copper-4"),
    ("Copper III", "copper-3"),
    ("Copper II", "copper-2"),
    ("Copper I", "copper-1"),
    ("Bronze V", "bronze-5"),
    ("Bronze IV", "bronze-4"),
    ("Bronze III", "bronze-3"),
    ("Bronze II", "bronze-2"),
    ("Bronze I", "bronze-1"),
    ("Silver V", "silver-5"),
    ("Silver IV", "silver-4"),
    ("Silver III", "silver-3"),
    ("Silver II", "silver-2"),
    ("Silver I", "silver-1"),
    ("Gold V", "gold-5"),
    ("Gold IV", "gold-4"),
    ("Gold III", "gold-3"),
    ("Gold II", "gold-2"),
    ("Gold I", "gold-1"),
    ("Platinum V", "platinum-5"),
    ("Platinum IV", "platinum-4"),
    ("Platinum III", "platinum-3"),
    ("Platinum II", "platinum-2"),
    ("Platinum I", "platinum-1"),
    ("Emerald V", "emerald-5"),
    ("Emerald IV", "emerald-4"),
    ("Emerald III", "emerald-3"),
    ("Emerald II", "emerald-2"),
    ("Emerald I", "emerald-1"),
    ("Diamond V", "diamond-5"),
    ("Diamond IV", "diamond-4"),
    ("Diamond III", "diamond-3"),
    ("Diamond II", "diamond-2"),
    ("Diamond I", "diamond-1"),
    ("Champion", "champion"),
];

/// Rank name and icon key; an unknown id keeps the raw number and has no icon.
fn rank_info(id: i64) -> (String, Option<String>) {
    match usize::try_from(id).ok().and_then(|i| RANKS.get(i)) {
        Some((name, key)) => (name.to_string(), Some(key.to_string())),
        None => (format!("Rank {id}"), None),
    }
}

/// `Emerald II`, `EMERALD II`, `emerald-2`, `Champion`, `Unranked` -> table entry.
fn rank_from_name(name: &str) -> Option<(&'static str, &'static str)> {
    let low = name.trim().to_ascii_lowercase().replace(['-', '_'], " ");
    let mut words = low.split_whitespace();
    let tier = words.next()?;
    if tier == "unranked" {
        return Some(RANKS[0]);
    }
    if tier.starts_with("champion") {
        return Some(RANKS[36]);
    }
    if !["copper", "bronze", "silver", "gold", "platinum", "emerald", "diamond"].contains(&tier) {
        return None;
    }
    let division = match words.next()? {
        "i" | "1" => 1,
        "ii" | "2" => 2,
        "iii" | "3" => 3,
        "iv" | "4" => 4,
        "v" | "5" => 5,
        _ => return None,
    };
    let key = format!("{tier}-{division}");
    RANKS.iter().find(|(_, k)| *k == key).copied()
}

/// Ubisoft season number -> code (siegeapi `season_id_to_code`): 43 -> `Y11S3`.
fn season_code(season_id: i64) -> String {
    if season_id <= 0 {
        return String::new();
    }
    format!("Y{}S{}", (season_id - 1) / 4 + 1, (season_id - 1) % 4 + 1)
}

fn r2(x: f64) -> f64 {
    (x * 100.0).round() / 100.0
}
fn r1(x: f64) -> f64 {
    (x * 10.0).round() / 10.0
}

fn totals(scope: Scope, kills: u64, deaths: u64, wins: u64, losses: u64, matches: u64, fetched_at: &str) -> OfficialTotals {
    // Same rules as the replay KPIs: zero deaths gives the kill count; win % over matches.
    let kd = if deaths == 0 { kills as f64 } else { kills as f64 / deaths as f64 };
    let win_pct = if matches == 0 { 0.0 } else { 100.0 * wins as f64 / matches as f64 };
    OfficialTotals { scope, kd: r2(kd), win_pct: r1(win_pct), kills, deaths, wins, losses, matches, fetched_at: fetched_at.to_string() }
}

// ---------------------------------------------------------------- tolerant response parsing

/// A JSON number, or a numeric string such as `"3,100"` or `"50.7%"`.
fn num(v: &Value) -> Option<f64> {
    match v {
        Value::Number(n) => n.as_f64(),
        Value::String(s) => s.trim().trim_end_matches('%').replace(',', "").parse().ok(),
        _ => None,
    }
}

fn nonempty_str(v: Option<&Value>) -> Option<String> {
    v.and_then(Value::as_str).map(str::trim).filter(|s| !s.is_empty()).map(str::to_string)
}

fn first_pointer<'a>(root: &'a Value, paths: &[&str]) -> Option<&'a Value> {
    paths.iter().find_map(|p| root.pointer(p).filter(|v| !v.is_null()))
}

/// A segment looks like `{ type?, attributes?, metadata?, stats: { <key>: { value, metadata } } }`.
fn is_segment(v: &Value) -> bool {
    v.get("stats").is_some_and(Value::is_object) && (v.get("type").is_some() || v.get("attributes").is_some())
}

/// Collects every segment found anywhere in the response (the exact place is not documented).
fn collect_segments<'a>(v: &'a Value, depth: u8, out: &mut Vec<&'a Value>) {
    if depth > 12 {
        return;
    }
    if is_segment(v) {
        out.push(v);
        return;
    }
    match v {
        Value::Array(a) => a.iter().for_each(|e| collect_segments(e, depth + 1, out)),
        Value::Object(m) => m.values().for_each(|e| collect_segments(e, depth + 1, out)),
        _ => {}
    }
}

fn segments_of(root: &Value) -> Vec<&Value> {
    let mut out = Vec::new();
    collect_segments(root, 0, &mut out);
    out
}

fn attr<'a>(seg: &'a Value, key: &str) -> Option<&'a Value> {
    seg.get("attributes")?.get(key)
}

fn attr_lower(seg: &Value, key: &str) -> Option<String> {
    attr(seg, key).and_then(Value::as_str).map(|s| s.trim().to_ascii_lowercase())
}

fn seg_type(seg: &Value) -> String {
    seg.get("type").and_then(Value::as_str).unwrap_or_default().to_ascii_lowercase()
}

fn season_id(seg: &Value) -> Option<i64> {
    attr(seg, "season").and_then(num).map(|f| f as i64).filter(|s| *s > 0)
}

fn is_ranked(seg: &Value) -> bool {
    let session = attr_lower(seg, "sessionType");
    let mode = attr_lower(seg, "gamemode");
    if session.as_deref() == Some("ranked") || matches!(mode.as_deref(), Some("pvp_ranked" | "ranked")) {
        return true;
    }
    session.is_none()
        && mode.is_none()
        && seg
            .pointer("/metadata/sessionTypeName")
            .and_then(Value::as_str)
            .is_some_and(|s| s.eq_ignore_ascii_case("ranked"))
}

fn stat<'a>(seg: &'a Value, key: &str) -> Option<&'a Value> {
    seg.get("stats")?.get(key)
}

fn stat_f64(seg: &Value, key: &str) -> Option<f64> {
    let s = stat(seg, key)?;
    match s {
        Value::Object(o) => o.get("value").and_then(num).or_else(|| o.get("displayValue").and_then(num)),
        other => num(other),
    }
}

fn stat_u64(seg: &Value, key: &str) -> Option<u64> {
    stat_f64(seg, key).filter(|f| *f >= 0.0).map(|f| f.round() as u64)
}

fn stat_i64(seg: &Value, key: &str) -> Option<i64> {
    stat_f64(seg, key).map(|f| f.round() as i64)
}

/// Tier name text stored in a stat's metadata (`tierName`, else `name`).
fn stat_tier_label(seg: &Value, key: &str) -> Option<String> {
    let meta = stat(seg, key)?.get("metadata")?;
    ["tierName", "name"].iter().find_map(|f| nonempty_str(meta.get(*f)))
}

/// Rank name + icon key. Tier name first (on the rank stat, then on the points stat), then a text
/// display value, then the numeric id.
fn resolve_rank(seg: &Value, rank_key: &str, points_key: &str) -> Option<(String, Option<String>)> {
    for label in [stat_tier_label(seg, rank_key), stat_tier_label(seg, points_key)].into_iter().flatten() {
        if let Some((name, icon)) = rank_from_name(&label) {
            return Some((name.to_string(), Some(icon.to_string())));
        }
    }
    if let Some(text) = nonempty_str(stat(seg, rank_key).and_then(|s| s.get("displayValue"))) {
        if let Some((name, icon)) = rank_from_name(&text) {
            return Some((name.to_string(), Some(icon.to_string())));
        }
    }
    stat_i64(seg, rank_key).map(rank_info)
}

/// The newest ranked season segment: highest `season` id among ranked segments.
fn current_ranked_season<'a>(segs: &[&'a Value]) -> Option<&'a Value> {
    segs.iter()
        .copied()
        .filter(|s| is_ranked(s))
        .filter_map(|s| season_id(s).map(|id| (id, stat(s, "rank").is_some() || stat(s, "rankPoints").is_some(), s)))
        .max_by_key(|(id, has_rank, _)| (*id, *has_rank))
        .map(|(_, _, s)| s)
}

/// Season totals from one segment; `None` without kills and deaths.
fn totals_of(seg: &Value, scope: Scope, fetched_at: &str) -> Option<OfficialTotals> {
    let kills = stat_u64(seg, "kills")?;
    let deaths = stat_u64(seg, "deaths")?;
    let wins = stat_u64(seg, "matchesWon").unwrap_or(0);
    let losses = stat_u64(seg, "matchesLost").unwrap_or(0);
    let matches = stat_u64(seg, "matchesPlayed").unwrap_or(wins + losses);
    (matches > 0).then(|| totals(scope, kills, deaths, wins, losses, matches, fetched_at))
}

/// Ranked all-time totals: the ranked `gamemode` segment, else the sum over ranked season segments.
fn all_time_totals(segs: &[&Value], fetched_at: &str) -> Option<OfficialTotals> {
    if let Some(seg) = segs.iter().find(|s| seg_type(s) == "gamemode" && is_ranked(s)) {
        if let Some(t) = totals_of(seg, Scope::All, fetched_at) {
            return Some(t);
        }
    }
    let mut seen = std::collections::BTreeSet::new();
    let (mut kills, mut deaths, mut wins, mut losses, mut matches) = (0u64, 0u64, 0u64, 0u64, 0u64);
    for seg in segs.iter().filter(|s| is_ranked(s)) {
        let Some(id) = season_id(seg) else { continue };
        let Some(t) = totals_of(seg, Scope::Season, fetched_at) else { continue };
        if !seen.insert(id) {
            continue;
        }
        kills += t.kills;
        deaths += t.deaths;
        wins += t.wins;
        losses += t.losses;
        matches += t.matches;
    }
    (matches > 0).then(|| totals(Scope::All, kills, deaths, wins, losses, matches, fetched_at))
}

fn https_url(v: Option<&Value>) -> Option<String> {
    let s = match v? {
        Value::String(s) => Some(s.as_str()),
        Value::Object(o) => o.get("url").and_then(Value::as_str),
        _ => None,
    }?;
    s.trim().starts_with("https://").then(|| s.trim().to_string())
}

/// Rank-points history: a list of numbers, or of objects with a points field and an optional date.
fn parse_history(root: &Value) -> Vec<RankPoint> {
    let Some(arr) = ["/history/data/history", "/history/data", "/history", "/data/history"]
        .iter()
        .find_map(|p| root.pointer(p).and_then(Value::as_array))
    else {
        return Vec::new();
    };
    const POINT_KEYS: [&str; 6] = ["rankPoints", "rank_points", "rp", "points", "mmr", "value"];
    const LABEL_KEYS: [&str; 7] = ["date", "timestamp", "time", "createdAt", "startDate", "at", "day"];
    let mut out: Vec<RankPoint> = arr
        .iter()
        .filter_map(|e| match e {
            Value::Number(n) => Some(RankPoint { label: None, rank_points: n.as_f64()?.round() as i64 }),
            Value::Object(o) => {
                let points = POINT_KEYS
                    .iter()
                    .find_map(|k| o.get(*k).and_then(num))
                    .or_else(|| e.pointer("/stats/rankPoints/value").and_then(num))?;
                let label = LABEL_KEYS.iter().find_map(|k| nonempty_str(o.get(*k)));
                Some(RankPoint { label, rank_points: points.round() as i64 })
            }
            _ => None,
        })
        .collect();
    if out.len() > MAX_HISTORY_POINTS {
        out.drain(..out.len() - MAX_HISTORY_POINTS);
    }
    out
}

/// True when the profile answer holds no ranked season segment, so Full Stats V2 is worth a call.
fn needs_fullstats(profile: &Value) -> bool {
    current_ranked_season(&segments_of(profile)).is_none()
}

fn parse_operators(body: &Value) -> Vec<OfficialOperator> {
    let Some(arr) = ["/operators/operators", "/operators", "/data/operators"]
        .iter()
        .find_map(|p| body.pointer(p).and_then(Value::as_array))
    else {
        return Vec::new();
    };
    arr.iter()
        .filter_map(|o| {
            let name = nonempty_str(o.get("operator").or_else(|| o.get("name")))?;
            let rounds = o.get("roundsPlayed").and_then(num).map(|f| f.round() as u64).unwrap_or(0);
            if rounds == 0 {
                return None;
            }
            let kills = o.get("kills").and_then(num).map(|f| f.round() as u64).unwrap_or(0);
            let deaths = o.get("deaths").and_then(num).map(|f| f.round() as u64).unwrap_or(0);
            let kd = o
                .get("kd")
                .and_then(num)
                .unwrap_or(if deaths == 0 { kills as f64 } else { kills as f64 / deaths as f64 });
            let side = match nonempty_str(o.get("side")).map(|s| s.to_ascii_lowercase()).as_deref() {
                Some(s) if s.starts_with("att") => "attack",
                Some(s) if s.starts_with("def") => "defense",
                _ => "",
            };
            Some(OfficialOperator { name, side: side.to_string(), rounds, kd: r2(kd), kills, deaths })
        })
        .collect()
}

/// Builds the profile from the Profile V2 answer, optional extra answers that may carry segments
/// (Full Stats V2) and the optional Operators V2 answer.
fn build_profile(username: &str, profile: &Value, extra: &[&Value], operators: Option<&Value>, fetched_at: &str) -> OnlineProfile {
    let mut segs = segments_of(profile);
    for e in extra {
        segs.extend(segments_of(e));
    }
    let current = current_ranked_season(&segs);

    let (mut rank_name, mut rank_icon, mut max_rank_name) = (None, None, None);
    let (mut rank_points, mut max_rank_points, mut top, mut season) = (None, None, None, None);
    let (mut season_kd, mut kills, mut deaths, mut wins, mut losses, mut abandons, mut matches) =
        (None, None, None, None, None, None, None);
    let mut official_season = None;
    if let Some(seg) = current {
        if let Some((name, icon)) = resolve_rank(seg, "rank", "rankPoints") {
            rank_name = Some(name);
            rank_icon = icon;
        }
        max_rank_name = resolve_rank(seg, "maxRank", "maxRankPoints").map(|(n, _)| n);
        rank_points = stat_i64(seg, "rankPoints");
        max_rank_points = stat_i64(seg, "maxRankPoints");
        top = stat_u64(seg, "topRankPosition")
            .or_else(|| {
                stat(seg, "rankPoints")
                    .and_then(|s| s.pointer("/metadata/topRankPosition"))
                    .and_then(num)
                    .map(|f| f.round() as u64)
            })
            .filter(|p| *p > 0)
            .and_then(|p| u32::try_from(p).ok());
        season = nonempty_str(seg.pointer("/metadata/shortName")).or_else(|| season_id(seg).map(season_code).filter(|s| !s.is_empty()));
        kills = stat_u64(seg, "kills");
        deaths = stat_u64(seg, "deaths");
        wins = stat_u64(seg, "matchesWon");
        losses = stat_u64(seg, "matchesLost");
        abandons = stat_u64(seg, "matchesAbandoned");
        matches = stat_u64(seg, "matchesPlayed").or_else(|| Some(wins? + losses?));
        season_kd = stat_f64(seg, "kdRatio")
            .or_else(|| Some(kills? as f64 / deaths? as f64).filter(|v| v.is_finite()))
            .map(r2);
        official_season = totals_of(seg, Scope::Season, fetched_at);
    }

    let level = first_pointer(profile, &["/account/level", "/data/account/level", "/data/metadata/clearanceLevel"])
        .and_then(num)
        .filter(|l| *l >= 0.0)
        .map(|l| l.round() as u32);
    let profile_picture_url = https_url(first_pointer(
        profile,
        &["/account/profilePicture", "/data/account/profilePicture", "/data/userInfo/customAvatarUrl"],
    ));

    OnlineProfile {
        source: "arenyze".into(),
        username: username.to_string(),
        season,
        rank_name,
        rank_points,
        max_rank_name,
        max_rank_points,
        top_rank_position: top,
        rank_icon,
        season_kd,
        kills,
        deaths,
        wins,
        losses,
        abandons,
        matches,
        level,
        profile_picture_url,
        rank_history: parse_history(profile),
        operators: operators.map(parse_operators).unwrap_or_default(),
        fetched_at: fetched_at.to_string(),
        official: OfficialPair { season: official_season, all: all_time_totals(&segs, fetched_at) },
    }
}

fn parse_usage(body: &Value, headers: Option<(u64, u64)>, now: &str) -> QuotaInfo {
    let plan = nonempty_str(body.get("plan"));
    let limit = body.get("limit").and_then(num).map(|f| f as u64);
    let used = body.pointer("/usage/total_calls").and_then(num).map(|f| f as u64);
    let (used, limit) = match headers {
        Some((u, l)) => (Some(u), Some(l)),
        None => (used, limit),
    };
    QuotaInfo {
        plan,
        used,
        limit,
        remaining: used.zip(limit).map(|(u, l)| l.saturating_sub(u)),
        checked_at: now.to_string(),
    }
}

// ---------------------------------------------------------------- official totals for v2::query

/// True when the filter is plain `season` / `all`: no operator, map, side, date or hour filter,
/// and mode all or ranked (the official numbers are Ranked). Date/hour fields are read through
/// serde so this keeps working before and after they are added to `Filter`.
fn plain_filter(f: &Filter) -> bool {
    let no_date_hour = serde_json::to_value(f).ok().is_some_and(|v| {
        ["dateFrom", "dateTo", "hourFrom", "hourTo"].iter().all(|k| v.get(k).is_none_or(Value::is_null))
    });
    matches!(f.range, Range::Season | Range::All)
        && matches!(f.mode, Mode::All | Mode::Ranked)
        && f.side == SideFilter::Both
        && f.operators.is_empty()
        && f.maps.is_empty()
        && no_date_hour
}

fn official_from_cache(me: Option<&str>, cache: Option<&CachedProfile>, f: &Filter) -> Option<OfficialTotals> {
    let (me, cache) = (me?, cache?);
    if !cache.username.eq_ignore_ascii_case(me) || !plain_filter(f) {
        return None;
    }
    match f.range {
        Range::Season => cache.profile.official.season.clone(),
        Range::All => cache.profile.official.all.clone(),
        _ => None,
    }
}

/// The tracked player: recording player of the newest stored match.
pub fn tracked_username(matches: &[FullMatch]) -> Option<String> {
    let m = matches.last()?;
    m.player(&m.me_profile_id).map(|p| p.username.trim().to_string()).filter(|s| !s.is_empty())
}

/// Official totals for a query, from the stored cache only (no network). `None` without a cached
/// profile of the tracked player or when the filter is not plain `season` / `all`.
pub fn cached_official(conn: &rusqlite::Connection, f: &Filter, matches: &[FullMatch]) -> Option<OfficialTotals> {
    if !plain_filter(f) {
        return None;
    }
    let cache: Option<CachedProfile> = load_json(conn, KEY_CACHE);
    official_from_cache(tracked_username(matches).as_deref(), cache.as_ref(), f)
}

/// Name of the tracked player from the database (newest stored match), loading only that match.
fn newest_username(conn: &rusqlite::Connection) -> Option<String> {
    let id: String = conn
        .query_row(
            "SELECT match_id FROM matches WHERE status != 'in_progress' ORDER BY timestamp DESC, match_id DESC LIMIT 1",
            [],
            |r| r.get(0),
        )
        .ok()?;
    let m = store::load_full_match(conn, &id).map_err(|e| log::warn!("arenyze: cannot load match: {e}")).ok()??;
    tracked_username(std::slice::from_ref(&m))
}

// ---------------------------------------------------------------- state and HTTP

#[derive(Default)]
struct Inner {
    cache: Option<CachedProfile>,
    last_attempt: Option<Instant>,
    last_error: Option<String>,
}

/// Tauri state. The async mutex serializes fetches (no duplicate requests).
pub struct ArenyzeState {
    inner: Mutex<Inner>,
    /// Last raw answers (endpoint name -> JSON), for the diagnostics file. Memory only.
    raw: StdMutex<Option<Map<String, Value>>>,
    client: Option<reqwest::Client>,
}

impl ArenyzeState {
    /// Builds the HTTP client. No network traffic happens here.
    pub fn new() -> Self {
        // reqwest uses rustls without a built-in provider; install ring once (an error only
        // means a provider is already installed).
        let _ = rustls::crypto::ring::default_provider().install_default();
        let client = reqwest::Client::builder()
            .timeout(HTTP_TIMEOUT)
            .user_agent(USER_AGENT)
            .build()
            .map_err(|e| log::error!("arenyze: cannot build HTTP client: {e}"))
            .ok();
        Self { inner: Mutex::new(Inner::default()), raw: StdMutex::new(None), client }
    }

    fn client(&self) -> Result<&reqwest::Client, OnlineError> {
        self.client.as_ref().ok_or_else(|| OnlineError::Unexpected("HTTP client unavailable".into()))
    }

    fn set_raw(&self, raw: Option<Map<String, Value>>) {
        if let Ok(mut g) = self.raw.lock() {
            *g = raw;
        }
    }

    fn has_raw(&self) -> bool {
        self.raw.lock().map(|g| g.is_some()).unwrap_or(false)
    }
}

fn now_iso() -> String {
    Utc::now().to_rfc3339_opts(SecondsFormat::Secs, true)
}

fn age_secs(iso: &str, now: DateTime<Utc>) -> Option<i64> {
    DateTime::parse_from_rfc3339(iso).ok().map(|t| (now - t.with_timezone(&Utc)).num_seconds())
}

fn pct_encode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b'~') {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

struct Answer {
    status: u16,
    body: String,
    quota_headers: Option<(u64, u64)>,
}

async fn http_get(client: &reqwest::Client, key: &str, path: &str, params: &[(&str, &str)]) -> Result<Answer, OnlineError> {
    let mut url = format!("{BASE_URL}{path}");
    for (i, (k, v)) in params.iter().enumerate() {
        url.push(if i == 0 { '?' } else { '&' });
        url.push_str(&format!("{k}={}", pct_encode(v)));
    }
    log::info!("arenyze: GET {path}");
    let resp = client.get(&url).header("api-key", key).header("Accept", "application/json").send().await.map_err(net_error)?;
    let status = resp.status().as_u16();
    let header_u64 = |name: &str| resp.headers().get(name).and_then(|v| v.to_str().ok()).and_then(|v| v.trim().parse::<u64>().ok());
    let quota_headers = header_u64("X-Quota-Used").zip(header_u64("X-Quota-Limit"));
    let body = resp.text().await.map_err(net_error)?;
    Ok(Answer { status, body, quota_headers })
}

/// One fetch session for one player: remembers the raw answers and the newest quota headers.
struct Fetcher<'a> {
    client: &'a reqwest::Client,
    key: &'a str,
    username: &'a str,
    raw: Map<String, Value>,
    quota: Option<(u64, u64)>,
}

impl Fetcher<'_> {
    fn remember(&mut self, name: &str, status: u16, body: &str) {
        let parsed = serde_json::from_str::<Value>(body).unwrap_or_else(|_| Value::String(body.chars().take(500).collect()));
        let entry = if (200..300).contains(&status) { parsed } else { serde_json::json!({ "httpStatus": status, "body": parsed }) };
        self.raw.insert(name.to_string(), entry);
    }

    /// GET with the player parameters. Retries once with the alternative platform spelling when
    /// the first one is refused as a bad request.
    async fn player(&mut self, name: &str, path: &str, extra: &[(&str, &str)]) -> Result<Value, OnlineError> {
        let mut last = OnlineError::BadRequest;
        for platform in [PLATFORM, PLATFORM_ALT] {
            let mut params: Vec<(&str, &str)> = vec![("nameOnPlatform", self.username), ("platformType", platform)];
            params.extend_from_slice(extra);
            let a = http_get(self.client, self.key, path, &params).await?;
            self.remember(name, a.status, &a.body);
            if a.quota_headers.is_some() {
                self.quota = a.quota_headers;
            }
            if (200..300).contains(&a.status) {
                return serde_json::from_str(&a.body).map_err(|_| OnlineError::Unexpected("answer is not JSON".into()));
            }
            last = classify_error(a.status, &a.body);
            if last != OnlineError::BadRequest {
                return Err(last);
            }
        }
        Err(last)
    }
}

/// Profile V2, then Full Stats V2 only when Profile V2 holds no ranked season, then Operators V2
/// (best effort) for the current season.
async fn fetch_all(f: &mut Fetcher<'_>) -> Result<OnlineProfile, OnlineError> {
    let profile = f.player("profile", "/v2/profile", &[]).await?;
    let mut full = None;
    if needs_fullstats(&profile) {
        match f.player("fullstats", "/v2/fullstats", &[("seasonYear", "all"), ("modes", "Ranked")]).await {
            Ok(v) => full = Some(v),
            Err(e) if e.is_key_problem() || e == OnlineError::QuotaExceeded => return Err(e),
            Err(e) => log::warn!("arenyze: full stats unavailable: {e:?}"),
        }
    }
    let extra: Vec<&Value> = full.iter().collect();
    let first = build_profile(f.username, &profile, &extra, None, &now_iso());
    let mut ops = None;
    if let Some(season) = first.season.clone() {
        match f.player("operators", "/v2/operators", &[("seasonYear", season.as_str()), ("modes", "ranked")]).await {
            Ok(v) => ops = Some(v),
            Err(e) if e.is_key_problem() || e == OnlineError::QuotaExceeded => return Err(e),
            Err(e) => log::warn!("arenyze: operators unavailable: {e:?}"),
        }
    }
    Ok(build_profile(f.username, &profile, &extra, ops.as_ref(), &now_iso()))
}

fn store_quota(conn: &rusqlite::Connection, quota: Option<(u64, u64)>) {
    let Some((used, limit)) = quota else { return };
    let mut q: QuotaInfo = load_json(conn, KEY_QUOTA).unwrap_or_default();
    q.used = Some(used);
    q.limit = Some(limit);
    q.remaining = Some(limit.saturating_sub(used));
    q.checked_at = now_iso();
    save_json(conn, KEY_QUOTA, Some(&q));
}

fn status_of(conn: &rusqlite::Connection, state: &ArenyzeState) -> ArenyzeStatus {
    let has_key = key_load().is_some();
    let cache: Option<CachedProfile> = load_json(conn, KEY_CACHE);
    ArenyzeStatus {
        has_key,
        username: if has_key { newest_username(conn) } else { None },
        quota: if has_key { load_json(conn, KEY_QUOTA) } else { None },
        last_fetched_at: if has_key { cache.map(|c| c.profile.fetched_at) } else { None },
        has_raw_response: state.has_raw(),
    }
}

fn valid_key_shape(key: &str) -> bool {
    (8..=256).contains(&key.len()) && key.chars().all(|c| c.is_ascii_graphic())
}

// ---------------------------------------------------------------- commands

/// Local only (settings + Credential Manager), no network.
#[tauri::command]
pub fn arenyze_status(state: tauri::State<'_, ArenyzeState>, db: tauri::State<'_, store::Db>) -> Cmd<ArenyzeStatus> {
    Ok(status_of(&*lock(&db)?, &state))
}

/// Checks the key with one `/me/usage` call, then stores it in the Credential Manager.
#[tauri::command]
pub async fn arenyze_set_key(
    state: tauri::State<'_, ArenyzeState>,
    db: tauri::State<'_, store::Db>,
    key: String,
) -> Cmd<ArenyzeStatus> {
    let key = key.trim().to_string();
    if !valid_key_shape(&key) {
        return Err(OnlineError::BadKeyFormat.message());
    }
    let client = state.client().map_err(|e| e.message())?;
    let a = http_get(client, &key, "/me/usage", &[]).await.map_err(|e| e.message())?;
    if !(200..300).contains(&a.status) {
        let e = classify_error(a.status, &a.body);
        log::info!("arenyze: key check failed: {e:?}");
        return Err(e.message());
    }
    key_save(&key).map_err(|e| e.message())?;
    log::info!("arenyze: key saved");
    let body: Value = serde_json::from_str(&a.body).unwrap_or(Value::Null);
    let quota = parse_usage(&body, a.quota_headers, &now_iso());
    {
        let conn = lock(&db)?;
        save_json(&conn, KEY_QUOTA, Some(&quota));
        // A new key may belong to another account: forget the old profile.
        save_json::<CachedProfile>(&conn, KEY_CACHE, None);
    }
    *state.inner.lock().await = Inner::default();
    state.set_raw(None);
    Ok(status_of(&*lock(&db)?, &state))
}

/// Deletes the key, the cached profile and the quota note.
#[tauri::command]
pub async fn arenyze_clear_key(state: tauri::State<'_, ArenyzeState>, db: tauri::State<'_, store::Db>) -> Cmd<ArenyzeStatus> {
    key_delete();
    {
        let conn = lock(&db)?;
        save_json::<CachedProfile>(&conn, KEY_CACHE, None);
        save_json::<QuotaInfo>(&conn, KEY_QUOTA, None);
    }
    *state.inner.lock().await = Inner::default();
    state.set_raw(None);
    log::info!("arenyze: key removed");
    Ok(status_of(&*lock(&db)?, &state))
}

/// The tracked player's online profile; `null` without a key or before the first replay import.
/// Served from the 15-minute cache unless `force`. A failed refresh without `force` serves the
/// older cached profile (its `fetchedAt` shows its age), except for key problems.
#[tauri::command]
pub async fn online_profile(
    state: tauri::State<'_, ArenyzeState>,
    db: tauri::State<'_, store::Db>,
    force: bool,
) -> Cmd<Option<OnlineProfile>> {
    let Some(key) = key_load() else {
        return Ok(None);
    };
    let mut inner = state.inner.lock().await;
    let (stored, username) = {
        let conn = lock(&db)?;
        (load_json::<CachedProfile>(&conn, KEY_CACHE), newest_username(&conn))
    };
    let Some(username) = username else {
        return Ok(None);
    };
    if inner.cache.is_none() {
        inner.cache = stored;
    }
    let cached = inner.cache.clone().filter(|c| c.username.eq_ignore_ascii_case(&username));
    let now = Utc::now();
    if !force {
        if let Some(c) = cached.as_ref().filter(|c| age_secs(&c.profile.fetched_at, now).is_some_and(|a| a < CACHE_TTL_SECS)) {
            return Ok(Some(c.profile.clone()));
        }
    }
    if let Some(t) = inner.last_attempt {
        let gap = if force { MANUAL_GAP } else { AUTO_GAP };
        if t.elapsed() < gap {
            return match (cached, force) {
                (Some(c), _) => Ok(Some(c.profile)),
                (None, true) => Err(OnlineError::TooSoon.message()),
                (None, false) => Err(inner.last_error.clone().unwrap_or_else(|| OnlineError::TooSoon.message())),
            };
        }
    }

    let client = state.client().map_err(|e| e.message())?;
    inner.last_attempt = Some(Instant::now());
    let mut fetcher = Fetcher { client, key: &key, username: &username, raw: Map::new(), quota: None };
    let result = fetch_all(&mut fetcher).await;
    let (raw, quota) = (std::mem::take(&mut fetcher.raw), fetcher.quota);
    drop(fetcher);
    if !raw.is_empty() {
        state.set_raw(Some(raw));
    }
    if let Ok(conn) = lock(&db) {
        store_quota(&conn, quota);
    }
    match result {
        Ok(profile) => {
            let fresh = CachedProfile { username: username.clone(), profile: profile.clone() };
            if let Ok(conn) = lock(&db) {
                save_json(&conn, KEY_CACHE, Some(&fresh));
            }
            inner.cache = Some(fresh);
            inner.last_error = None;
            log::info!("arenyze: profile refreshed (rank {:?})", profile.rank_name);
            Ok(Some(profile))
        }
        Err(e) => {
            log::info!("arenyze: refresh failed: {e:?}");
            inner.last_error = Some(e.message());
            match cached {
                Some(c) if !force && !e.is_key_problem() => Ok(Some(c.profile)),
                _ => Err(e.message()),
            }
        }
    }
}

/// Replaces every occurrence of the key in a JSON text (defensive: answers should not contain it).
fn scrub(text: &str, key: Option<&str>) -> String {
    match key.filter(|k| k.len() >= 8) {
        Some(k) => text.replace(k, "[removed]"),
        None => text.to_string(),
    }
}

/// Diagnostics: writes the last raw Arenyze answers (key removed) to a file the user picks.
/// Returns the saved path, or `null` when the dialog was cancelled.
#[tauri::command]
pub async fn arenyze_save_last_response(
    app: tauri::AppHandle,
    state: tauri::State<'_, ArenyzeState>,
) -> Cmd<Option<String>> {
    use tauri_plugin_dialog::DialogExt;
    let raw = state
        .raw
        .lock()
        .map_err(|_| "response store is busy".to_string())?
        .clone()
        .ok_or_else(|| "No Arenyze response yet. Refresh your rank first.".to_string())?;
    let mut doc = Map::new();
    doc.insert("savedAt".into(), Value::String(now_iso()));
    doc.insert("note".into(), Value::String("Raw Arenyze answers for the tracked player. The API key is not included.".into()));
    doc.insert("responses".into(), Value::Object(raw));
    let text = serde_json::to_string_pretty(&Value::Object(doc)).map_err(|e| e.to_string())?;
    let text = scrub(&text, key_load().as_deref());
    tauri::async_runtime::spawn_blocking(move || {
        let mut dialog = app.dialog().file().set_file_name("arenyze-last-response.json").add_filter("JSON", &["json"]);
        if let Some(dir) = downloads_dir() {
            dialog = dialog.set_directory(dir);
        }
        let Some(picked) = dialog.blocking_save_file() else {
            return Ok(None);
        };
        let path = picked.into_path().map_err(|e| format!("cannot use that location: {e}"))?;
        std::fs::write(&path, text.as_bytes()).map_err(|e| format!("cannot write the file: {e}"))?;
        log::info!("arenyze: last response saved");
        Ok(Some(path.to_string_lossy().into_owned()))
    })
    .await
    .map_err(|e| format!("save task failed: {e}"))?
}

fn downloads_dir() -> Option<std::path::PathBuf> {
    let home = std::env::var_os("USERPROFILE").or_else(|| std::env::var_os("HOME"))?;
    let p = std::path::PathBuf::from(home).join("Downloads");
    p.is_dir().then_some(p)
}

/// Opens the Arenyze key page in the default browser (a fixed URL, nothing user-supplied).
#[tauri::command]
pub fn arenyze_open_key_page() -> Cmd<()> {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        std::process::Command::new("rundll32")
            .args(["url.dll,FileProtocolHandler", KEY_PAGE_URL])
            .creation_flags(0x0800_0000)
            .spawn()
            .map(|_| ())
            .map_err(|e| format!("cannot open the browser: {e}"))
    }
    #[cfg(not(windows))]
    {
        std::process::Command::new("xdg-open")
            .arg(KEY_PAGE_URL)
            .spawn()
            .map(|_| ())
            .map_err(|e| format!("cannot open the browser: {e}"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const PROFILE: &str = include_str!("../tests/fixtures/arenyze_profile_v2.json");
    const FULLSTATS: &str = include_str!("../tests/fixtures/arenyze_fullstats_v2.json");
    const OPERATORS: &str = include_str!("../tests/fixtures/arenyze_operators_v2.json");
    const NOW: &str = "2026-11-07T12:00:00Z";

    fn json(s: &str) -> Value {
        serde_json::from_str(s).unwrap()
    }

    #[test]
    fn rank_names_map_to_icon_keys() {
        for (text, key) in [
            ("Emerald II", "emerald-2"),
            ("EMERALD II", "emerald-2"),
            ("platinum iv", "platinum-4"),
            ("Copper-5", "copper-5"),
            ("Diamond 1", "diamond-1"),
            ("Champion", "champion"),
            ("CHAMPIONS", "champion"),
            ("Unranked", "unranked"),
        ] {
            assert_eq!(rank_from_name(text).map(|r| r.1), Some(key), "{text}");
        }
        assert_eq!(rank_from_name("Emerald VI"), None);
        assert_eq!(rank_from_name("Grandmaster II"), None);
        assert_eq!(rank_from_name(""), None);
        // Every table entry round-trips through its own name.
        for (name, key) in RANKS.iter() {
            assert_eq!(rank_from_name(name).map(|r| r.1), Some(*key));
        }
    }

    #[test]
    fn numeric_rank_ids_use_the_table() {
        assert_eq!(rank_info(0), ("Unranked".into(), Some("unranked".into())));
        assert_eq!(rank_info(22), ("Platinum IV".into(), Some("platinum-4".into())));
        assert_eq!(rank_info(36), ("Champion".into(), Some("champion".into())));
        assert_eq!(rank_info(99), ("Rank 99".into(), None));
        assert_eq!(season_code(43), "Y11S3");
    }

    #[test]
    fn tracker_style_profile_gives_the_current_ranked_season() {
        let p = build_profile("TestPlayer-01", &json(PROFILE), &[], Some(&json(OPERATORS)), NOW);
        // Season 43 ranked beats 39 and 26; the standard season-43 segment is ignored.
        assert_eq!(p.source, "arenyze");
        assert_eq!(p.season.as_deref(), Some("Y11S3"));
        assert_eq!(p.rank_name.as_deref(), Some("Platinum IV"));
        assert_eq!(p.rank_icon.as_deref(), Some("platinum-4"));
        assert_eq!(p.rank_points, Some(3100));
        assert_eq!(p.max_rank_name.as_deref(), Some("Platinum III"));
        assert_eq!(p.max_rank_points, Some(3261));
        assert_eq!(p.top_rank_position, None); // value 0 = below Champion
        assert_eq!(p.season_kd, Some(1.18));
        assert_eq!((p.kills, p.deaths), (Some(1327), Some(1121)));
        assert_eq!((p.wins, p.losses, p.matches), (Some(144), Some(140), Some(284)));
        assert_eq!(p.level, Some(187));
        assert_eq!(p.profile_picture_url.as_deref(), Some("https://example.invalid/avatar.png"));
        assert_eq!(p.rank_history.len(), 4);
        assert_eq!(p.rank_history[3].rank_points, 3100);
        assert_eq!(p.rank_history[0].label.as_deref(), Some("2026-10-20T12:00:00Z"));
    }

    #[test]
    fn official_totals_come_from_ranked_segments() {
        let p = build_profile("TestPlayer-01", &json(PROFILE), &[], None, NOW);
        let season = p.official.season.expect("season totals");
        assert_eq!(season.scope, Scope::Season);
        assert_eq!((season.kills, season.deaths, season.matches), (1327, 1121, 284));
        assert_eq!(season.kd, 1.18);
        assert_eq!(season.win_pct, 50.7);
        let all = p.official.all.expect("all-time totals");
        assert_eq!(all.scope, Scope::All);
        assert!(all.matches > 0 && all.kills > 0);
        assert_eq!(season.fetched_at, NOW);
        assert!(p.operators.is_empty());
    }

    #[test]
    fn tier_name_in_rank_metadata_and_segments_in_fullstats() {
        // Profile without segments -> Full Stats V2 is needed and supplies them.
        let empty = json(r#"{"player":{},"account":{"level":12},"seasons":{"data":{"segments":[]}},"history":{"data":{"history":[]}}}"#);
        assert!(needs_fullstats(&empty));
        let full = json(FULLSTATS);
        assert!(!needs_fullstats(&full));
        let p = build_profile("TestPlayer-01", &empty, &[&full], None, NOW);
        assert_eq!(p.season.as_deref(), Some("Y11S4"));
        assert_eq!(p.rank_name.as_deref(), Some("Emerald III"));
        assert_eq!(p.rank_icon.as_deref(), Some("emerald-3"));
        assert_eq!(p.max_rank_name.as_deref(), Some("Emerald I"));
        assert_eq!(p.rank_points, Some(3650));
        assert_eq!(p.season_kd, Some(1.5));
        assert_eq!(p.level, Some(12));
        assert!(p.rank_history.is_empty());
    }

    #[test]
    fn numeric_rank_id_is_the_fallback_and_missing_fields_stay_null() {
        let v = json(
            r#"{"x":{"deeper":[{"type":"season","attributes":{"season":43,"sessionType":"ranked"},
                "stats":{"rank":{"value":29},"rankPoints":{"value":"3,420"}}}]}}"#,
        );
        let p = build_profile("TestPlayer-01", &v, &[], None, NOW);
        assert_eq!(p.rank_name.as_deref(), Some("Emerald II"));
        assert_eq!(p.rank_icon.as_deref(), Some("emerald-2"));
        assert_eq!(p.rank_points, Some(3420));
        assert_eq!(p.season.as_deref(), Some("Y11S3")); // from the season id
        assert_eq!(p.max_rank_name, None);
        assert_eq!(p.season_kd, None);
        assert_eq!((p.kills, p.matches), (None, None));
        assert!(p.official.season.is_none() && p.official.all.is_none());
        assert_eq!(p.level, None);
    }

    #[test]
    fn unknown_shapes_give_an_empty_profile() {
        for text in ["{}", "[]", "null", r#"{"seasons":{"data":{"segments":"nope"}}}"#] {
            let p = build_profile("TestPlayer-01", &json(text), &[], None, NOW);
            assert_eq!(p.rank_name, None);
            assert!(p.official.season.is_none());
            assert!(p.rank_history.is_empty());
        }
    }

    #[test]
    fn champion_position_is_read_from_the_stat_or_its_metadata() {
        let v = json(
            r#"[{"type":"season","attributes":{"season":43,"gamemode":"pvp_ranked"},
                "stats":{"rank":{"value":36},"rankPoints":{"value":5400,"metadata":{"topRankPosition":1234}}}}]"#,
        );
        let p = build_profile("TestPlayer-01", &v, &[], None, NOW);
        assert_eq!(p.rank_name.as_deref(), Some("Champion"));
        assert_eq!(p.top_rank_position, Some(1234));
    }

    #[test]
    fn operators_are_parsed_and_empty_ones_dropped() {
        let ops = parse_operators(&json(OPERATORS));
        assert_eq!(ops.len(), 2);
        assert_eq!(ops[0], OfficialOperator { name: "Twitch".into(), side: "attack".into(), rounds: 47, kd: 1.08, kills: 38, deaths: 35 });
        assert_eq!(ops[1].side, "defense");
    }

    #[test]
    fn history_accepts_numbers_and_nested_stats() {
        let nums = json(r#"{"history":{"data":{"history":[1000,1100,1200]}}}"#);
        assert_eq!(parse_history(&nums).iter().map(|p| p.rank_points).collect::<Vec<_>>(), vec![1000, 1100, 1200]);
        let nested = json(r#"{"history":{"data":[{"date":"d1","stats":{"rankPoints":{"value":2000}}},{"date":"d2","rp":2100}]}}"#);
        let h = parse_history(&nested);
        assert_eq!(h.len(), 2);
        assert_eq!(h[1], RankPoint { label: Some("d2".into()), rank_points: 2100 });
    }

    #[test]
    fn errors_are_classified() {
        assert_eq!(classify_error(401, r#"{"error":"INVALID_API_KEY"}"#), OnlineError::InvalidKey);
        assert_eq!(classify_error(401, r#"{"error":"API_KEY_EXPIRED"}"#), OnlineError::KeyExpired);
        assert_eq!(classify_error(401, "Missing api-key header"), OnlineError::InvalidKey);
        assert_eq!(classify_error(429, r#"{"error":"MONTHLY_QUOTA_EXCEEDED"}"#), OnlineError::QuotaExceeded);
        assert_eq!(classify_error(429, r#"{"error":"FREE_GAME_QUOTA_EXCEEDED"}"#), OnlineError::QuotaExceeded);
        assert_eq!(classify_error(429, r#"{"error":"Rate limit exceeded"}"#), OnlineError::RateLimited);
        assert_eq!(classify_error(404, r#"{"error":"PLAYER_NOT_FOUND"}"#), OnlineError::PlayerNotFound);
        assert_eq!(classify_error(404, ""), OnlineError::PlayerNotFound);
        assert_eq!(classify_error(503, ""), OnlineError::Server(503));
        assert_eq!(classify_error(400, ""), OnlineError::BadRequest);
        // Messages are English and never carry response bodies.
        assert!(OnlineError::QuotaExceeded.message().contains("monthly"));
        assert!(OnlineError::InvalidKey.message().contains("key"));
        assert!(OnlineError::Network.message().contains("internet"));
    }

    #[test]
    fn usage_and_quota_headers() {
        let body = json(r#"{"plan":"free","limit":2000,"usage":{"total_calls":123}}"#);
        let q = parse_usage(&body, None, NOW);
        assert_eq!((q.plan.as_deref(), q.used, q.limit, q.remaining), (Some("free"), Some(123), Some(2000), Some(1877)));
        let q = parse_usage(&body, Some((500, 2000)), NOW);
        assert_eq!((q.used, q.remaining), (Some(500), Some(1500)));
        let q = parse_usage(&json("{}"), None, NOW);
        assert_eq!((q.used, q.limit, q.remaining), (None, None, None));
    }

    #[test]
    fn key_shape_scrub_and_url_encoding() {
        assert!(valid_key_shape("abcd1234efgh"));
        assert!(!valid_key_shape("short"));
        assert!(!valid_key_shape("has space in it"));
        assert!(!valid_key_shape(""));
        assert_eq!(scrub("a SECRETKEY123 b", Some("SECRETKEY123")), "a [removed] b");
        assert_eq!(scrub("keep", Some("x")), "keep");
        assert_eq!(pct_encode("Test.Player-01_x"), "Test.Player-01_x");
        assert_eq!(pct_encode("a b&c"), "a%20b%26c");
    }

    #[test]
    fn cache_serves_official_totals_only_for_the_tracked_player_and_plain_filters() {
        let p = build_profile("TestPlayer-01", &json(PROFILE), &[], None, NOW);
        let cache = CachedProfile { username: "TestPlayer-01".into(), profile: p };
        let season = Filter { range: Range::Season, ..Default::default() };
        let all = Filter { range: Range::All, ..Default::default() };
        assert_eq!(official_from_cache(Some("testplayer-01"), Some(&cache), &season).map(|t| t.scope), Some(Scope::Season));
        assert_eq!(official_from_cache(Some("TestPlayer-01"), Some(&cache), &all).map(|t| t.scope), Some(Scope::All));
        assert!(official_from_cache(Some("Somebody"), Some(&cache), &season).is_none());
        assert!(official_from_cache(None, Some(&cache), &season).is_none());
        assert!(official_from_cache(Some("TestPlayer-01"), None, &season).is_none());
        let with_op = Filter { range: Range::Season, operators: vec!["zofia".into()], ..Default::default() };
        assert!(official_from_cache(Some("TestPlayer-01"), Some(&cache), &with_op).is_none());
        let last5 = Filter { range: Range::Last5, ..Default::default() };
        assert!(official_from_cache(Some("TestPlayer-01"), Some(&cache), &last5).is_none());
    }

    #[test]
    fn empty_database_has_no_tracked_player() {
        let conn = store::open(std::path::Path::new(":memory:")).unwrap();
        assert_eq!(newest_username(&conn), None);
        assert!(cached_official(&conn, &Filter { range: Range::Season, ..Default::default() }, &[]).is_none());
        // Legacy cleanup is idempotent and marks itself done.
        cleanup_legacy(&conn);
        assert_eq!(store::get_setting(&conn, KEY_LEGACY_DONE).unwrap().as_deref(), Some("1"));
        cleanup_legacy(&conn);
    }
}
