//! Online stats from the Arenyze API (v4 spec, user decision 1): rank and the other official
//! values come from the **user's own** free API key. This replaces the Ubisoft sign-in.
//!
//! # Network rule
//! * The app talks only to `public-api.arenyze.com`, only when the user entered a key, and only
//!   for the tracked player's own profile (username of the newest stored match, platform `uplay`).
//! * Calls happen on a manual refresh, and once per app launch in the background right after start
//!   (v7, only when a key is saved; the free plan has a monthly call limit). After that the stored
//!   data is served for the whole session; there is no TTL and no timer. A (season, mode) operator
//!   table is fetched the first time it is asked for in a session and then reused.
//!
//! # Secrets
//! The key lives in the Windows Credential Manager (`keyring`). It is never logged, never written
//! to the database and never part of an error message. The "save last response" diagnostics file
//! has every occurrence of the key replaced before it is written.
//!
//! # Response shapes
//! The real Profile V2 answer (seen 2026-11-08, `docs/v5-spec.md`) keeps the rank in
//! `stats.platform_families_full_profiles[].board_ids_full_profiles[]` (`ranked` / `standard` /
//! `casual`, rank id 0..36), the RP history in `history.data.history.data` (newest first), and one
//! Tracker-style segment per season and playlist in `seasons.data.segments`. The parser reads these
//! first. The older tolerant path stays as a fallback: it looks for Tracker-Network-style segments
//! (`type`, `attributes`, `stats.<key>.{value, metadata}`) anywhere in the answer and picks the
//! newest ranked season; the rank name then comes from a tier name (`Emerald II`, case-insensitive)
//! or the numeric rank id. A missing field stays `null`; nothing is invented. The only derived value
//! is the rank of past seasons, computed from RP (100 RP per tier from 1000 RP; checked against the
//! real board ranks 21 / 23).

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

/// Minimum gap between two manual refreshes.
const MANUAL_GAP: Duration = Duration::from_secs(20);
const HTTP_TIMEOUT: Duration = Duration::from_secs(20);
const MAX_HISTORY_POINTS: usize = 150;
/// A season needs this many ranked matches to count as the "best K/D season" (when any does).
const PEAK_KD_MIN_MATCHES: u64 = 20;

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
    /// Rank at that point (`Platinum V`), when the history says.
    #[serde(default)]
    pub rank_name: Option<String>,
}

/// Official per-operator numbers (Operators V2) for one season and playlist.
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
    /// Round win percentage (0-100); `null` when Arenyze did not send it.
    #[serde(default)]
    pub win_pct: Option<f64>,
    #[serde(default)]
    pub hs_pct: Option<f64>,
    #[serde(default)]
    pub first_bloods: Option<u64>,
    #[serde(default)]
    pub first_deaths: Option<u64>,
    #[serde(default)]
    pub matches: Option<u64>,
}

/// One playlist inside one season (Ranked, Casual, Unranked, Event, Warmup) or the career line.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlaylistStats {
    /// `ranked`, `casual`, `unranked`, `event`, `warmup` or `career`.
    pub playlist: String,
    pub rank_name: Option<String>,
    /// Local asset key under `/assets/ranks/<key>.png`.
    pub rank_icon: Option<String>,
    pub rank_points: Option<i64>,
    pub max_rank_name: Option<String>,
    pub max_rank_icon: Option<String>,
    pub max_rank_points: Option<i64>,
    /// Matchmaking rating of the pre-RP seasons.
    pub mmr: Option<i64>,
    pub max_mmr: Option<i64>,
    pub kd: Option<f64>,
    pub kills: Option<u64>,
    pub deaths: Option<u64>,
    pub matches: Option<u64>,
    pub wins: Option<u64>,
    pub losses: Option<u64>,
    pub abandons: Option<u64>,
    /// 0-100, over matches played.
    pub win_pct: Option<f64>,
    /// Text such as `597h` or `2h 49m`, exactly as Arenyze sends it.
    pub time_played: Option<String>,
}

/// One season with the playlists Arenyze has numbers for.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SeasonEntry {
    pub season_id: i64,
    /// `Y11S3`.
    pub short_name: String,
    /// `Split Fire`.
    pub name: Option<String>,
    /// `#rrggbb`, from Arenyze.
    pub color: Option<String>,
    pub playlists: Vec<PlaylistStats>,
}

/// A season picked as a personal best (highest rank, best K/D).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PeakEntry {
    pub season_id: i64,
    pub short_name: String,
    pub name: Option<String>,
    pub color: Option<String>,
    /// RP for the rank peak, K/D for the K/D peak.
    pub value: f64,
    /// `RP`, `MMR` or `K/D`.
    pub unit: String,
    pub rank_name: Option<String>,
    pub rank_icon: Option<String>,
    pub matches: Option<u64>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SeasonPeaks {
    pub best_rank: Option<PeakEntry>,
    pub best_kd: Option<PeakEntry>,
}

/// Answer of `online_operators`: one (season, mode) table.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OperatorsView {
    /// `Y11S3` or `all`.
    pub season: String,
    /// `all`, `ranked`, `standard`, `quick-match`, `dual-front` or `siege-cup`.
    pub mode: String,
    pub operators: Vec<OfficialOperator>,
    pub fetched_at: String,
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
    /// `PC`, `PlayStation` or `Xbox`.
    #[serde(default)]
    pub platform: Option<String>,
    #[serde(default)]
    pub current_season_id: Option<i64>,
    /// `Split Fire`.
    #[serde(default)]
    pub season_name: Option<String>,
    #[serde(default)]
    pub season_color: Option<String>,
    /// Newest season first.
    #[serde(default)]
    pub seasons: Vec<SeasonEntry>,
    #[serde(default)]
    pub peaks: SeasonPeaks,
    /// All-time line (Arenyze `overview` segment).
    #[serde(default)]
    pub career: Option<PlaylistStats>,
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

/// Rank-points history. The real answer is `history.data.history.data`: a list of
/// `[isoTime, {value, metadata: {rank: "Platinum 5"}}]` pairs, newest first. Older shapes (numbers, or
/// objects with a points field and an optional date) still work. The result is oldest first.
fn parse_history(root: &Value) -> Vec<RankPoint> {
    let Some(arr) = ["/history/data/history/data", "/history/data/history", "/history/data", "/history", "/data/history"]
        .iter()
        .find_map(|p| root.pointer(p).and_then(Value::as_array))
    else {
        return Vec::new();
    };
    const POINT_KEYS: [&str; 6] = ["rankPoints", "rank_points", "rp", "points", "mmr", "value"];
    const LABEL_KEYS: [&str; 7] = ["date", "timestamp", "time", "createdAt", "startDate", "at", "day"];
    let rank_of = |v: &Value| {
        nonempty_str(v.pointer("/metadata/rank")).map(|r| rank_from_name(&r).map_or(r, |(n, _)| n.to_string()))
    };
    let mut out: Vec<RankPoint> = arr
        .iter()
        .filter_map(|e| match e {
            Value::Number(n) => Some(RankPoint { label: None, rank_points: n.as_f64()?.round() as i64, rank_name: None }),
            Value::Array(pair) if pair.len() == 2 => {
                let inner = &pair[1];
                let points = inner.get("value").and_then(num).or_else(|| num(inner))?;
                Some(RankPoint { label: nonempty_str(pair.first()), rank_points: points.round() as i64, rank_name: rank_of(inner) })
            }
            Value::Object(o) => {
                let points = POINT_KEYS
                    .iter()
                    .find_map(|k| o.get(*k).and_then(num))
                    .or_else(|| e.pointer("/stats/rankPoints/value").and_then(num))?;
                let label = LABEL_KEYS.iter().find_map(|k| nonempty_str(o.get(*k)));
                Some(RankPoint { label, rank_points: points.round() as i64, rank_name: rank_of(e) })
            }
            _ => None,
        })
        .collect();
    // Arenyze lists the newest point first; the chart wants time to run left to right.
    let stamp = |p: Option<&RankPoint>| p.and_then(|p| p.label.as_deref()).and_then(|l| DateTime::parse_from_rfc3339(l).ok());
    if let (Some(first), Some(last)) = (stamp(out.first()), stamp(out.last())) {
        if first > last {
            out.reverse();
        }
    }
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
            let count = |k: &str| o.get(k).and_then(num).filter(|f| *f >= 0.0).map(|f| f.round() as u64);
            Some(OfficialOperator {
                name,
                side: side.to_string(),
                rounds,
                kd: r2(kd),
                kills,
                deaths,
                win_pct: o.get("winPercent").and_then(num).map(r1),
                hs_pct: o.get("headshotPercent").and_then(num).map(r1),
                first_bloods: count("firstBloods"),
                first_deaths: count("firstDeaths"),
                matches: count("matchesPlayed"),
            })
        })
        .collect()
}

// ---------------------------------------------------------------- seasons, playlists, boards

/// Playlist order in the UI.
const PLAYLIST_ORDER: [&str; 5] = ["ranked", "casual", "unranked", "event", "warmup"];

/// Segment `sessionType` (else `gamemode`) -> playlist key.
fn playlist_of_segment(seg: &Value) -> Option<&'static str> {
    let by_session = |s: &str| match s {
        "ranked" => Some("ranked"),
        "quick-match" | "casual" => Some("casual"),
        "standard" | "unranked" => Some("unranked"),
        "event" => Some("event"),
        "arcade" | "warmup" => Some("warmup"),
        _ => None,
    };
    attr_lower(seg, "sessionType").as_deref().and_then(by_session).or_else(|| {
        match attr_lower(seg, "gamemode")?.as_str() {
            "pvp_ranked" => Some("ranked"),
            "pvp_casual" => Some("casual"),
            "pvp_standard" => Some("unranked"),
            "pvp_event" => Some("event"),
            "pvp_warmup" => Some("warmup"),
            _ => None,
        }
    })
}

/// Rank id from RP in the Siege X system: 100 RP per tier from 1000 RP (Copper V = 1, Platinum V =
/// 21 at 3000, Champion = 36). `None` at or below 1000 RP (placement, no rank yet).
fn rank_id_from_rp(rp: i64) -> Option<i64> {
    (rp > 1000).then(|| (1 + (rp - 1000) / 100).min(36))
}

fn rank_of_rp(rp: Option<i64>) -> (Option<String>, Option<String>) {
    match rp.and_then(rank_id_from_rp).map(rank_info) {
        Some((name, icon)) => (Some(name), icon),
        None => (None, None),
    }
}

fn pct_of(part: Option<u64>, whole: Option<u64>) -> Option<f64> {
    let (p, w) = (part?, whole?);
    (w > 0).then(|| r1(100.0 * p as f64 / w as f64))
}

/// One playlist line from a season / overview segment. Ranks of past seasons are derived from RP.
fn playlist_from_segment(seg: &Value, playlist: &str) -> PlaylistStats {
    let rank_points = stat_i64(seg, "rankPoints").filter(|v| *v > 0);
    let max_rank_points = stat_i64(seg, "maxRankPoints").filter(|v| *v > 0);
    let (kills, deaths) = (stat_u64(seg, "kills"), stat_u64(seg, "deaths"));
    let (wins, losses) = (stat_u64(seg, "matchesWon"), stat_u64(seg, "matchesLost"));
    let matches = stat_u64(seg, "matchesPlayed").or_else(|| Some(wins? + losses?));
    let (rank_name, rank_icon, max_rank_name, max_rank_icon) = if playlist == "ranked" {
        let (n, i) = rank_of_rp(rank_points);
        let (mn, mi) = rank_of_rp(max_rank_points);
        (n, i, mn, mi)
    } else {
        (None, None, None, None)
    };
    PlaylistStats {
        playlist: playlist.to_string(),
        rank_name,
        rank_icon,
        rank_points,
        max_rank_name,
        max_rank_icon,
        max_rank_points,
        mmr: stat_i64(seg, "mmr").filter(|v| *v > 0),
        max_mmr: stat_i64(seg, "maxMmr").filter(|v| *v > 0),
        kd: stat_f64(seg, "kdRatio").or_else(|| Some(kills? as f64 / deaths? as f64).filter(|v| v.is_finite())).map(r2),
        kills,
        deaths,
        matches,
        wins,
        losses,
        abandons: None,
        win_pct: pct_of(wins, matches),
        time_played: nonempty_str(stat(seg, "timePlayed").and_then(|s| s.get("displayValue"))),
    }
}

/// One board of the real `stats.platform_families_full_profiles` tree (newest season only).
#[derive(Debug, Clone, PartialEq)]
struct Board {
    playlist: &'static str,
    season_id: i64,
    rank: i64,
    rank_points: i64,
    max_rank: i64,
    max_rank_points: i64,
    kills: u64,
    deaths: u64,
    wins: u64,
    losses: u64,
    abandons: u64,
}

fn parse_boards(root: &Value) -> Vec<Board> {
    let Some(families) = first_pointer(root, &["/stats/platform_families_full_profiles", "/platform_families_full_profiles"])
        .and_then(Value::as_array)
    else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for fam in families {
        for board in fam.get("board_ids_full_profiles").and_then(Value::as_array).into_iter().flatten() {
            let playlist = match board.get("board_id").and_then(Value::as_str).map(str::to_ascii_lowercase).as_deref() {
                Some("ranked") => "ranked",
                Some("casual") => "casual",
                Some("standard") => "unranked",
                Some("event") => "event",
                Some("warmup") | Some("arcade") => "warmup",
                _ => continue,
            };
            // Newest season of this board.
            let Some(full) = board
                .get("full_profiles")
                .and_then(Value::as_array)
                .and_then(|a| a.iter().max_by_key(|p| p.get("season_id").and_then(num).map_or(0, |f| f as i64)))
            else {
                continue;
            };
            let Some(season_id) = full.get("season_id").and_then(num).map(|f| f as i64).filter(|s| *s > 0) else { continue };
            let Some(p) = full.get("profile") else { continue };
            let n = |k: &str| p.get(k).and_then(num).filter(|f| *f >= 0.0).map_or(0, |f| f.round() as u64);
            let i = |k: &str| p.get(k).and_then(num).map_or(0, |f| f.round() as i64);
            out.push(Board {
                playlist,
                season_id,
                rank: i("rank"),
                rank_points: i("rank_points"),
                max_rank: i("max_rank"),
                max_rank_points: i("max_rank_points"),
                kills: n("kills"),
                deaths: n("deaths"),
                wins: n("wins"),
                losses: n("losses"),
                abandons: n("abandon"),
            });
        }
    }
    out
}

/// Board numbers are the official live ones for the newest season: they replace the segment's.
fn overlay_board(p: &mut PlaylistStats, b: &Board) {
    if b.rank > 0 || b.playlist == "ranked" {
        let (name, icon) = rank_info(b.rank);
        p.rank_name = Some(name);
        p.rank_icon = icon;
        if b.max_rank > 0 {
            let (name, icon) = rank_info(b.max_rank);
            p.max_rank_name = Some(name);
            p.max_rank_icon = icon;
        }
    }
    if b.rank_points > 0 {
        p.rank_points = Some(b.rank_points);
    }
    if b.max_rank_points > 0 {
        p.max_rank_points = Some(b.max_rank_points);
    }
    p.kills = Some(b.kills);
    p.deaths = Some(b.deaths);
    p.wins = Some(b.wins);
    p.losses = Some(b.losses);
    p.abandons = Some(b.abandons);
    if p.matches.is_none() || p.matches == Some(0) {
        p.matches = Some(b.wins + b.losses);
    }
    if p.kd.is_none() && b.deaths > 0 {
        p.kd = Some(r2(b.kills as f64 / b.deaths as f64));
    }
    p.win_pct = pct_of(p.wins, p.matches);
}

/// Seasons (newest first, playlists in UI order) from the segments and boards, plus the career line.
fn build_seasons(segs: &[&Value], boards: &[Board]) -> (Vec<SeasonEntry>, Option<PlaylistStats>) {
    let mut by_id: std::collections::BTreeMap<i64, SeasonEntry> = std::collections::BTreeMap::new();
    let mut career = None;
    for seg in segs {
        if seg_type(seg) == "overview" {
            career.get_or_insert_with(|| playlist_from_segment(seg, "career"));
            continue;
        }
        let (Some(id), Some(playlist)) = (season_id(seg), playlist_of_segment(seg)) else { continue };
        let entry = by_id.entry(id).or_insert_with(|| SeasonEntry { season_id: id, ..Default::default() });
        if entry.short_name.is_empty() {
            entry.short_name = nonempty_str(seg.pointer("/metadata/shortName")).unwrap_or_else(|| season_code(id));
        }
        if entry.name.is_none() {
            entry.name = nonempty_str(seg.pointer("/metadata/name")).or_else(|| nonempty_str(seg.pointer("/metadata/seasonName")));
        }
        if entry.color.is_none() {
            entry.color = nonempty_str(seg.pointer("/metadata/color")).filter(|c| c.starts_with('#') && c.len() <= 9);
        }
        if !entry.playlists.iter().any(|p| p.playlist == playlist) {
            entry.playlists.push(playlist_from_segment(seg, playlist));
        }
    }
    for b in boards {
        let entry = by_id.entry(b.season_id).or_insert_with(|| SeasonEntry {
            season_id: b.season_id,
            short_name: season_code(b.season_id),
            ..Default::default()
        });
        if !entry.playlists.iter().any(|p| p.playlist == b.playlist) {
            entry.playlists.push(PlaylistStats { playlist: b.playlist.to_string(), ..Default::default() });
        }
        if let Some(p) = entry.playlists.iter_mut().find(|p| p.playlist == b.playlist) {
            overlay_board(p, b);
        }
    }
    let mut seasons: Vec<SeasonEntry> = by_id.into_values().rev().collect();
    for s in &mut seasons {
        s.playlists.sort_by_key(|p| PLAYLIST_ORDER.iter().position(|k| *k == p.playlist).unwrap_or(usize::MAX));
    }
    (seasons, career)
}

fn ranked_of(season: &SeasonEntry) -> Option<&PlaylistStats> {
    season.playlists.iter().find(|p| p.playlist == "ranked")
}

fn peak_entry(s: &SeasonEntry, value: f64, unit: &str, rank: Option<(&Option<String>, &Option<String>)>, matches: Option<u64>) -> PeakEntry {
    PeakEntry {
        season_id: s.season_id,
        short_name: s.short_name.clone(),
        name: s.name.clone(),
        color: s.color.clone(),
        value,
        unit: unit.to_string(),
        rank_name: rank.and_then(|r| r.0.clone()),
        rank_icon: rank.and_then(|r| r.1.clone()),
        matches,
    }
}

/// Highest-rank season (by best RP; MMR seasons only when no season has RP) and best-K/D season.
/// Ties go to the newer season.
fn compute_peaks(seasons: &[SeasonEntry]) -> SeasonPeaks {
    let best_rank = seasons
        .iter()
        .filter_map(|s| {
            let p = ranked_of(s)?;
            let rp = p.max_rank_points.or(p.rank_points).filter(|v| *v > 1000)?;
            Some((rp, s.season_id, s, p))
        })
        .max_by_key(|(rp, id, _, _)| (*rp, *id))
        .map(|(rp, _, s, p)| {
            let (name, icon) = if p.max_rank_name.is_some() { (&p.max_rank_name, &p.max_rank_icon) } else { (&p.rank_name, &p.rank_icon) };
            peak_entry(s, rp as f64, "RP", Some((name, icon)), p.matches)
        })
        .or_else(|| {
            seasons
                .iter()
                .filter_map(|s| {
                    let p = ranked_of(s)?;
                    Some((p.max_mmr.or(p.mmr)?, s.season_id, s, p))
                })
                .max_by_key(|(v, id, _, _)| (*v, *id))
                .map(|(v, _, s, p)| peak_entry(s, v as f64, "MMR", None, p.matches))
        });
    let kd_of = |min: u64| {
        seasons
            .iter()
            .filter_map(|s| {
                let p = ranked_of(s)?;
                let m = p.matches.filter(|m| *m >= min.max(1))?;
                Some((p.kd?, s.season_id, s, m))
            })
            .max_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)))
    };
    let best_kd = kd_of(PEAK_KD_MIN_MATCHES).or_else(|| kd_of(1)).map(|(kd, _, s, m)| peak_entry(s, kd, "K/D", None, Some(m)));
    SeasonPeaks { best_rank, best_kd }
}

fn platform_label(root: &Value) -> Option<String> {
    let family = nonempty_str(root.pointer("/player/platformFamilies")).map(|s| s.to_ascii_lowercase());
    let kind = nonempty_str(root.pointer("/player/platformType")).map(|s| s.to_ascii_lowercase());
    match (family.as_deref(), kind.as_deref()) {
        (Some("pc"), _) | (_, Some("uplay")) => Some("PC".into()),
        (Some("psn"), _) | (_, Some("psn")) => Some("PlayStation".into()),
        (Some("xbl"), _) | (_, Some("xbl")) => Some("Xbox".into()),
        _ => None,
    }
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

    // Real-shape data: boards + season list. They win over the Tracker-style guess above.
    let boards = parse_boards(profile);
    let (seasons, career) = build_seasons(&segs, &boards);
    let current_season_id = first_pointer(profile, &["/seasons/data/metadata/currentSeason", "/data/metadata/currentSeason"])
        .and_then(num)
        .map(|f| f as i64)
        .filter(|s| *s > 0)
        .or_else(|| boards.iter().find(|b| b.playlist == "ranked").map(|b| b.season_id))
        .or_else(|| seasons.first().map(|s| s.season_id));
    let current_entry = current_season_id.and_then(|id| seasons.iter().find(|s| s.season_id == id));
    if let Some(entry) = current_entry {
        season = Some(entry.short_name.clone());
        if let Some(r) = ranked_of(entry) {
            // A board rank is the real id; a rank derived from RP only fills what is still empty.
            let from_board = boards.iter().any(|b| b.playlist == "ranked" && b.season_id == entry.season_id);
            if r.rank_name.is_some() && (from_board || rank_name.is_none()) {
                rank_name = r.rank_name.clone();
                rank_icon = r.rank_icon.clone();
            }
            if from_board || max_rank_name.is_none() {
                max_rank_name = r.max_rank_name.clone().or(max_rank_name);
            }
            rank_points = r.rank_points.or(rank_points);
            max_rank_points = r.max_rank_points.or(max_rank_points);
            season_kd = r.kd.or(season_kd);
            kills = r.kills.or(kills);
            deaths = r.deaths.or(deaths);
            wins = r.wins.or(wins);
            losses = r.losses.or(losses);
            abandons = r.abandons.or(abandons);
            matches = r.matches.or(matches);
        }
    }
    let peaks = compute_peaks(&seasons);
    let (season_name, season_color) = current_entry.map_or((None, None), |e| (e.name.clone(), e.color.clone()));

    let level = first_pointer(profile, &["/account/level", "/data/account/level", "/data/metadata/clearanceLevel"])
        .and_then(num)
        .filter(|l| *l >= 0.0)
        .map(|l| l.round() as u32);
    let profile_picture_url = https_url(first_pointer(
        profile,
        &["/account/profilePicture", "/data/account/profilePicture", "/data/userInfo/customAvatarUrl", "/seasons/data/platformInfo/avatarUrl"],
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
        platform: platform_label(profile),
        current_season_id,
        season_name,
        season_color,
        seasons,
        peaks,
        career,
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
    /// Operator tables by (season, mode); memory only, reused for the whole session.
    ops_cache: std::collections::HashMap<(String, String), OperatorsView>,
    last_attempt: Option<Instant>,
    /// The one automatic network attempt of this launch has been made (success or not).
    auto_done: bool,
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

/// Why the profile is requested (decides whether the network is used).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ProfileRequest {
    /// The user pressed the refresh button.
    Manual,
    /// The one background refresh right after the app starts.
    Startup,
    /// A page asks for the profile: stored data, network only when there is nothing stored yet and
    /// the automatic attempt of this launch has not happened.
    View,
}

/// Pure network policy (v7): one automatic attempt per launch, manual refresh always.
fn should_fetch(req: ProfileRequest, has_cached: bool, auto_done: bool) -> bool {
    match req {
        ProfileRequest::Manual => true,
        ProfileRequest::Startup => !auto_done,
        ProfileRequest::View => !has_cached && !auto_done,
    }
}

/// Stored profile of the tracked player, straight from the database (no lock on the fetch state).
fn stored_profile(db: &store::Db) -> Cmd<Option<OnlineProfile>> {
    let conn = lock(db)?;
    let Some(username) = newest_username(&conn) else {
        return Ok(None);
    };
    Ok(load_json::<CachedProfile>(&conn, KEY_CACHE).filter(|c| c.username.eq_ignore_ascii_case(&username)).map(|c| c.profile))
}

/// The tracked player's online profile; `null` without a key or before the first replay import.
/// Without `force` it serves the stored profile and never refreshes on its own (the startup
/// refresh is `startup_refresh`); a failed automatic refresh serves the older stored profile (its
/// `fetchedAt` shows its age), except for key problems.
#[tauri::command]
pub async fn online_profile(
    state: tauri::State<'_, ArenyzeState>,
    db: tauri::State<'_, store::Db>,
    force: bool,
) -> Cmd<Option<OnlineProfile>> {
    profile_request(&state, &db, if force { ProfileRequest::Manual } else { ProfileRequest::View }).await
}

/// Tauri event sent after the startup refresh stored a new profile.
pub const EVENT_ARENYZE_UPDATED: &str = "arenyze-updated";

/// The automatic refresh once per app launch, run in the background right after start. Stored data
/// is served meanwhile (`online_profile` does not wait for a running fetch). Emits `arenyze-updated`
/// when a new profile arrived.
pub async fn startup_refresh(app: tauri::AppHandle) {
    use tauri::{Emitter, Manager};
    if key_load().is_none() {
        return;
    }
    let state = app.state::<ArenyzeState>();
    let db = app.state::<store::Db>();
    match profile_request(&state, &db, ProfileRequest::Startup).await {
        Ok(Some(_)) => {
            if let Err(e) = app.emit(EVENT_ARENYZE_UPDATED, ()) {
                log::warn!("cannot emit {EVENT_ARENYZE_UPDATED}: {e}");
            }
        }
        Ok(None) => {}
        Err(e) => log::info!("arenyze: startup refresh: {e}"),
    }
}

async fn profile_request(state: &ArenyzeState, db: &store::Db, req: ProfileRequest) -> Cmd<Option<OnlineProfile>> {
    let Some(key) = key_load() else {
        return Ok(None);
    };
    let manual = req == ProfileRequest::Manual;
    let mut inner = if req == ProfileRequest::View {
        match state.inner.try_lock() {
            Ok(g) => g,
            // A fetch is running: show what is stored instead of waiting for the network.
            Err(_) => return stored_profile(db),
        }
    } else {
        state.inner.lock().await
    };
    let (stored, username) = {
        let conn = lock(db)?;
        (load_json::<CachedProfile>(&conn, KEY_CACHE), newest_username(&conn))
    };
    let Some(username) = username else {
        return Ok(None);
    };
    if inner.cache.is_none() {
        inner.cache = stored;
    }
    let cached = inner.cache.clone().filter(|c| c.username.eq_ignore_ascii_case(&username));
    if !should_fetch(req, cached.is_some(), inner.auto_done) {
        return Ok(cached.map(|c| c.profile));
    }
    if manual {
        if let Some(t) = inner.last_attempt {
            if t.elapsed() < MANUAL_GAP {
                return match cached {
                    Some(c) => Ok(Some(c.profile)),
                    None => Err(OnlineError::TooSoon.message()),
                };
            }
        }
    } else {
        inner.auto_done = true;
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
    if let Ok(conn) = lock(db) {
        store_quota(&conn, quota);
    }
    match result {
        Ok(profile) => {
            let fresh = CachedProfile { username: username.clone(), profile: profile.clone() };
            if let Ok(conn) = lock(db) {
                save_json(&conn, KEY_CACHE, Some(&fresh));
            }
            inner.cache = Some(fresh);
            inner.last_error = None;
            if let Some(season) = profile.season.clone() {
                // The profile already carries the ranked table of its season: serve it from the cache.
                let view = OperatorsView { season: season.clone(), mode: "ranked".into(), operators: profile.operators.clone(), fetched_at: profile.fetched_at.clone() };
                inner.ops_cache.insert((season, "ranked".into()), view);
            }
            log::info!("arenyze: profile refreshed (rank {:?})", profile.rank_name);
            Ok(Some(profile))
        }
        Err(e) => {
            log::info!("arenyze: refresh failed: {e:?}");
            inner.last_error = Some(e.message());
            match cached {
                Some(c) if !manual && !e.is_key_problem() => Ok(Some(c.profile)),
                _ => Err(e.message()),
            }
        }
    }
}

/// Playlist choices of the operator table (Operators V2 `modes` values, lowercase).
const OPERATOR_MODES: [&str; 6] = ["all", "ranked", "standard", "quick-match", "dual-front", "siege-cup"];

/// Normalizes a (season, mode) request: season `all` or `Y<n>S<n>`, mode from the list above.
/// Anything else is refused before it can reach the network.
fn normalize_operator_query(season: &str, mode: &str) -> Option<(String, String)> {
    let mode = mode.trim().to_ascii_lowercase();
    if !OPERATOR_MODES.contains(&mode.as_str()) {
        return None;
    }
    let season = season.trim().to_ascii_uppercase();
    if season == "ALL" {
        return Some(("all".into(), mode));
    }
    let b = season.as_bytes();
    let digits = |s: &[u8]| !s.is_empty() && s.len() <= 2 && s.iter().all(u8::is_ascii_digit);
    let ok = b.first() == Some(&b'Y')
        && b.iter().position(|c| *c == b'S').is_some_and(|i| digits(&b[1..i]) && digits(&b[i + 1..]));
    ok.then_some((season, mode))
}

/// Operator table for one season and playlist (the Home selectors). Called only when the user picks a
/// combination; fetched the first time a combination is asked for in a session and reused after that, unless `force`.
#[tauri::command]
pub async fn online_operators(
    state: tauri::State<'_, ArenyzeState>,
    db: tauri::State<'_, store::Db>,
    season: String,
    mode: String,
    force: bool,
) -> Cmd<OperatorsView> {
    let (season, mode) = normalize_operator_query(&season, &mode).ok_or_else(|| "Unknown season or playlist.".to_string())?;
    let key = key_load().ok_or_else(|| "Add your Arenyze key in Settings to load operator stats.".to_string())?;
    let mut inner = state.inner.lock().await;
    if !force {
        if let Some(v) = inner.ops_cache.get(&(season.clone(), mode.clone())) {
            return Ok(v.clone());
        }
    }
    let username = newest_username(&*lock(&db)?).ok_or_else(|| OnlineError::PlayerNotFound.message())?;
    let client = state.client().map_err(|e| e.message())?;
    let mut fetcher = Fetcher { client, key: &key, username: &username, raw: Map::new(), quota: None };
    let season_param = if season == "all" { "all" } else { season.as_str() };
    let mode_param = if mode == "all" { "All" } else { mode.as_str() };
    let result = fetcher.player("operators", "/v2/operators", &[("seasonYear", season_param), ("modes", mode_param)]).await;
    let (raw, quota) = (std::mem::take(&mut fetcher.raw), fetcher.quota);
    drop(fetcher);
    if let (Ok(mut g), Some(answer)) = (state.raw.lock(), raw.get("operators")) {
        g.get_or_insert_with(Map::new).insert(format!("operators_{season}_{mode}"), answer.clone());
    }
    if let Ok(conn) = lock(&db) {
        store_quota(&conn, quota);
    }
    let body = result.map_err(|e| e.message())?;
    let view = OperatorsView { season: season.clone(), mode: mode.clone(), operators: parse_operators(&body), fetched_at: now_iso() };
    inner.ops_cache.insert((season, mode), view.clone());
    Ok(view)
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
    // Anonymized copies of the real answers (v5): board tree, season segments, pair history.
    const REAL_PROFILE: &str = include_str!("../tests/fixtures/arenyze_profile_v2_real.json");
    const REAL_OPERATORS: &str = include_str!("../tests/fixtures/arenyze_operators_v2_real.json");
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
        assert_eq!(
            ops[0],
            OfficialOperator {
                name: "Twitch".into(),
                side: "attack".into(),
                rounds: 47,
                kd: 1.08,
                kills: 38,
                deaths: 35,
                win_pct: None,
                hs_pct: None,
                first_bloods: None,
                first_deaths: None,
                matches: None,
            }
        );
        assert_eq!(ops[1].side, "defense");
    }

    #[test]
    fn history_accepts_numbers_and_nested_stats() {
        let nums = json(r#"{"history":{"data":{"history":[1000,1100,1200]}}}"#);
        assert_eq!(parse_history(&nums).iter().map(|p| p.rank_points).collect::<Vec<_>>(), vec![1000, 1100, 1200]);
        let nested = json(r#"{"history":{"data":[{"date":"d1","stats":{"rankPoints":{"value":2000}}},{"date":"d2","rp":2100}]}}"#);
        let h = parse_history(&nested);
        assert_eq!(h.len(), 2);
        assert_eq!(h[1], RankPoint { label: Some("d2".into()), rank_points: 2100, rank_name: None });
    }

    #[test]
    fn real_answer_gives_rank_from_the_board_tree() {
        let p = build_profile("TestPlayer-01", &json(REAL_PROFILE), &[], Some(&json(REAL_OPERATORS)), NOW);
        assert_eq!(p.season.as_deref(), Some("Y11S3"));
        assert_eq!(p.current_season_id, Some(43));
        assert_eq!(p.season_name.as_deref(), Some("Split Fire"));
        assert_eq!(p.season_color.as_deref(), Some("#f5d36c"));
        // Board rank 21 = Platinum V, max rank 23 = Platinum III (the segments carry no rank id).
        assert_eq!(p.rank_name.as_deref(), Some("Platinum V"));
        assert_eq!(p.rank_icon.as_deref(), Some("platinum-5"));
        assert_eq!(p.max_rank_name.as_deref(), Some("Platinum III"));
        assert_eq!((p.rank_points, p.max_rank_points), (Some(3007), Some(3261)));
        assert_eq!((p.kills, p.deaths), (Some(1435), Some(1207)));
        assert_eq!((p.wins, p.losses, p.abandons, p.matches), (Some(152), Some(154), Some(0), Some(306)));
        assert_eq!(p.season_kd, Some(1.19));
        assert_eq!(p.level, Some(210));
        assert_eq!(p.platform.as_deref(), Some("PC"));
        assert_eq!(p.profile_picture_url.as_deref(), Some("https://example.invalid/avatar-146.png"));
        assert_eq!(p.operators.len(), 5);
    }

    #[test]
    fn real_history_is_chronological_with_rank_names() {
        let p = build_profile("TestPlayer-01", &json(REAL_PROFILE), &[], None, NOW);
        assert_eq!(p.rank_history.len(), 10);
        // Arenyze sends the newest point first; the chart wants the oldest first.
        let (first, last) = (&p.rank_history[0], &p.rank_history[9]);
        assert!(first.label < last.label);
        assert_eq!(last.rank_points, 3007);
        assert_eq!(last.rank_name.as_deref(), Some("Platinum V"));
        assert_eq!(p.rank_history[8].rank_points, 3037);
    }

    #[test]
    fn real_segments_give_seasons_playlists_and_peaks() {
        let p = build_profile("TestPlayer-01", &json(REAL_PROFILE), &[], None, NOW);
        let ids: Vec<i64> = p.seasons.iter().map(|s| s.season_id).collect();
        assert_eq!(ids, vec![43, 39, 36, 35, 25]); // newest first
        let cur = &p.seasons[0];
        assert_eq!(cur.short_name, "Y11S3");
        assert_eq!(cur.name.as_deref(), Some("Split Fire"));
        let keys: Vec<&str> = cur.playlists.iter().map(|x| x.playlist.as_str()).collect();
        assert_eq!(keys, vec!["ranked", "casual", "unranked", "event"]);
        let ranked = &cur.playlists[0];
        assert_eq!(ranked.rank_name.as_deref(), Some("Platinum V"));
        assert_eq!(ranked.time_played.as_deref(), Some("597h"));
        assert_eq!(ranked.win_pct, Some(49.7));
        // The casual and unranked boards are overlaid with their abandons and results.
        assert_eq!(cur.playlists[1].abandons, Some(3));
        assert_eq!((cur.playlists[2].wins, cur.playlists[2].matches), (Some(1), Some(1)));
        // Past ranked season: the rank is derived from RP.
        let y9s3 = p.seasons.iter().find(|s| s.season_id == 35).unwrap();
        let r = &y9s3.playlists[0];
        assert_eq!(r.rank_points, Some(3553));
        assert_eq!(r.rank_name.as_deref(), Some("Emerald V"));
        assert_eq!(r.max_rank_points, Some(3767));
        assert_eq!(r.max_rank_name.as_deref(), Some("Emerald III"));
        // Pre-RP season keeps its MMR and has no derived rank.
        let y7s1 = p.seasons.iter().find(|s| s.season_id == 25).unwrap();
        assert_eq!(y7s1.playlists[0].mmr, Some(2436));
        assert_eq!(y7s1.playlists[0].rank_name, None);
        // Career line comes from the overview segment.
        let career = p.career.expect("career");
        assert_eq!((career.matches, career.kills), (Some(1572), Some(8112)));
        assert_eq!(career.time_played.as_deref(), Some("789h"));
        // Peaks: best RP is Y9S3 (3767); best K/D with 20+ matches is Y9S4 (1.33).
        let best = p.peaks.best_rank.expect("best rank");
        assert_eq!((best.season_id, best.value as i64, best.unit.as_str()), (35, 3767, "RP"));
        assert_eq!(best.rank_name.as_deref(), Some("Emerald III"));
        let kd = p.peaks.best_kd.expect("best kd");
        assert_eq!((kd.season_id, kd.unit.as_str()), (36, "K/D"));
        assert!((kd.value - 1.33).abs() < 0.01);
    }

    #[test]
    fn rank_from_rp_matches_the_real_board_ranks() {
        assert_eq!(rank_id_from_rp(3007), Some(21)); // Platinum V, real board rank
        assert_eq!(rank_id_from_rp(3261), Some(23)); // Platinum III, real board max rank
        assert_eq!(rank_id_from_rp(1000), None);
        assert_eq!(rank_id_from_rp(1050), Some(1));
        assert_eq!(rank_id_from_rp(9000), Some(36));
    }

    #[test]
    fn peaks_ignore_tiny_samples_and_fall_back_to_mmr() {
        let mk = |id: i64, mmr: Option<i64>, kd: f64, matches: u64| SeasonEntry {
            season_id: id,
            short_name: season_code(id),
            playlists: vec![PlaylistStats {
                playlist: "ranked".into(),
                max_mmr: mmr,
                kd: Some(kd),
                matches: Some(matches),
                ..Default::default()
            }],
            ..Default::default()
        };
        // A 2-match season with K/D 3.0 does not beat a 50-match season.
        let peaks = compute_peaks(&[mk(40, Some(2500), 3.0, 2), mk(39, Some(2900), 1.4, 50)]);
        assert_eq!(peaks.best_kd.map(|p| p.season_id), Some(39));
        let rank = peaks.best_rank.expect("mmr fallback");
        assert_eq!((rank.season_id, rank.unit.as_str(), rank.rank_name), (39, "MMR", None));
        // Only tiny samples: the best of them is still shown.
        let peaks = compute_peaks(&[mk(40, None, 3.0, 2), mk(39, None, 1.0, 3)]);
        assert_eq!(peaks.best_kd.map(|p| p.season_id), Some(40));
        assert!(peaks.best_rank.is_none());
        assert_eq!(compute_peaks(&[]), SeasonPeaks::default());
    }

    #[test]
    fn real_operators_keep_names_and_the_extra_columns() {
        let ops = parse_operators(&json(REAL_OPERATORS));
        assert_eq!(ops.len(), 5);
        let kali = ops.iter().find(|o| o.name == "Kali").expect("Kali");
        assert_eq!((kali.side.as_str(), kali.rounds, kali.kills, kali.deaths), ("attack", 211, 213, 128));
        assert_eq!((kali.win_pct, kali.hs_pct), (Some(50.7), Some(34.7)));
        assert_eq!((kali.first_bloods, kali.first_deaths, kali.matches), (Some(30), Some(15), Some(124)));
        // Arenyze sends "Azami " with a trailing space; names are trimmed.
        assert!(ops.iter().any(|o| o.name == "Azami" && o.side == "defense"));
        assert!(ops.iter().any(|o| o.name.starts_with("J") && o.name.ends_with("ger")));
    }

    #[test]
    fn operator_queries_are_validated_before_any_request() {
        assert_eq!(normalize_operator_query("y11s3", "Ranked"), Some(("Y11S3".into(), "ranked".into())));
        assert_eq!(normalize_operator_query(" ALL ", "all"), Some(("all".into(), "all".into())));
        assert_eq!(normalize_operator_query("Y8S1", "quick-match").map(|q| q.1), Some("quick-match".into()));
        for (season, mode) in [("Y11S3", "casual"), ("11S3", "ranked"), ("Y11S", "ranked"), ("Y111S3", "ranked"), ("Y11S3&x=1", "ranked"), ("", "ranked")] {
            assert_eq!(normalize_operator_query(season, mode), None, "{season}/{mode}");
        }
    }

    #[test]
    fn automatic_refresh_happens_once_per_launch() {
        use ProfileRequest::*;
        // Startup: one network attempt, even when a profile is stored; never twice.
        assert!(should_fetch(Startup, true, false));
        assert!(should_fetch(Startup, false, false));
        assert!(!should_fetch(Startup, true, true));
        assert!(!should_fetch(Startup, false, true));
        // Pages only read stored data; with nothing stored they may trigger the one attempt.
        assert!(!should_fetch(View, true, false));
        assert!(!should_fetch(View, true, true));
        assert!(should_fetch(View, false, false));
        assert!(!should_fetch(View, false, true));
        // The manual refresh always goes to the network.
        assert!(should_fetch(Manual, true, true));
        assert!(should_fetch(Manual, false, false));
    }

    #[test]
    fn operator_tables_are_kept_for_the_session() {
        // No TTL: whatever age the stored table has, a repeated request is served from memory.
        let mut inner = Inner::default();
        let key = ("Y11S3".to_string(), "ranked".to_string());
        let view = OperatorsView { season: key.0.clone(), mode: key.1.clone(), operators: vec![], fetched_at: "2020-01-01T00:00:00Z".into() };
        inner.ops_cache.insert(key.clone(), view);
        assert!(inner.ops_cache.get(&key).is_some());
        assert!(!inner.auto_done);
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
