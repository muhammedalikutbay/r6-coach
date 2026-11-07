//! Ubisoft sign-in (v3 spec, user decision 7): the signed-in user's **own** ranked profile and
//! official totals. The app talks to Ubisoft only from here, only after the user signs in, and
//! never at startup.
//!
//! # Secrets
//! * The password is used for one request and never stored or logged.
//! * Only Ubisoft's remember-me ticket is kept, in the Windows Credential Manager (`keyring`).
//!   Session tickets live in memory. Tickets, the email and the password never reach the log,
//!   the database or an error message (the secret-holding types have redacting `Debug` impls).
//! * The database keeps only non-secret data: the account (profile id, user id, name) and the
//!   last [`RankedProfile`] (10-minute cache, also used offline by [`cached_official`]).
//!
//! # Endpoints (checked 2026-11-07 against open-source clients)
//! * Sign-in / 2FA / remember-me / ticket exchange: `POST /v3/profiles/sessions` on
//!   public-ubiservices. Sources: danielwerg/r6api.js `src/auth.ts`, CNDRD/siegeapi
//!   `siegeapi/auth.py` (v7, 2025-09), wnc-replay/ubi-docs `docs/guide/authentication.md` and
//!   `ticket-exchange.md` (2026-10). Main Auth app id first, then exchange to the R6 PC app id
//!   (what siegeapi v7 does before reading ranks); the main ticket is the fallback.
//! * Rank: `GET /v2/spaces/<R6 space>/title/r6s/skill/full_profiles?profile_ids=<userId>&platform_families=pc`
//!   (r6api.js `getUserSeasonalv2.ts`, siegeapi `player.py`, ubi-docs `endpoints/stats.md`).
//!   Board `ranked`: rank id 0-36, rank points, max rank, top position, season kills / deaths /
//!   wins / losses / abandons. Rank names and icons: danielwerg/r6data `src/ranks.ts` (seasons 28+).
//! * All-time totals: `GET prod.datadev.ubisoft.com/v1/users/<userId>/playerstats` with
//!   `view=seasonal&aggregation=summary` (siegeapi 6.x `url_builder.py` / `player.py`). Several
//!   datadev views answer HTTP 410 since 2024-09 (siegeapi issue #33) and siegeapi v7 dropped it,
//!   so this is best effort: any failure leaves `official.all` empty, it never fails the profile.

use std::time::Duration;

use base64::Engine as _;
use chrono::{DateTime, SecondsFormat, Utc};
use r6k_core::query::{Filter, Mode, Range, SideFilter};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tauri::async_runtime::Mutex;

use crate::{lock, store, Cmd};

const SESSIONS_URL: &str = "https://public-ubiservices.ubi.com/v3/profiles/sessions";
/// "Main Auth" app id: password, 2FA and remember-me sign-in.
const MAIN_APP_ID: &str = "4391c956-8943-48eb-8859-07b0778f47b9";
/// Rainbow Six Siege PC app id: the stats session is exchanged to it.
const R6_APP_ID: &str = "e3d5ea9e-50bd-43b7-88bf-39794f4e3d40";
/// Rainbow Six Siege cross-play space.
const R6_SPACE_ID: &str = "0d2ae42d-4c27-4cb7-af6c-2099062302bb";
const USER_AGENT: &str = "UbiServices_SDK_2020.Release.58_PC64_ansi_static";

const KEYRING_SERVICE: &str = "R6 Coach (Ubisoft)";
const KEYRING_USER: &str = "remember-me";
const KEY_ACCOUNT: &str = "ubi_account";
const KEY_RANKED_CACHE: &str = "ubi_ranked_profile";

/// How long a fetched [`RankedProfile`] is served without a new request.
const CACHE_TTL_SECS: i64 = 10 * 60;
/// Renew a session this long before Ubisoft's expiration.
const RENEW_MARGIN_SECS: i64 = 5 * 60;
const HTTP_TIMEOUT: Duration = Duration::from_secs(15);

// ---------------------------------------------------------------- DTOs (spec "Rank")

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UbiStatus {
    pub signed_in: bool,
    pub username: Option<String>,
    pub profile_id: Option<String>,
    pub matches_replay_player: Option<bool>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LoginStatus {
    Ok,
    Needs2fa,
    Error,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UbiLoginResult {
    pub status: LoginStatus,
    pub message: Option<String>,
    pub user: Option<UbiStatus>,
}

impl UbiLoginResult {
    fn error(e: &UbiError) -> Self {
        Self { status: LoginStatus::Error, message: Some(e.message()), user: None }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Scope {
    Season,
    All,
}

/// Official Ubisoft totals (Ranked). `winPct` is 0-100 like the replay KPIs.
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
pub struct RankedProfile {
    pub season: String,
    pub rank_name: String,
    pub rank_points: i64,
    pub max_rank_name: String,
    pub max_rank_points: i64,
    pub top_rank_position: Option<u32>,
    pub kills: u64,
    pub deaths: u64,
    pub wins: u64,
    pub losses: u64,
    pub abandons: u64,
    pub rank_icon: Option<String>,
    pub fetched_at: String,
    pub official: OfficialPair,
}

// ---------------------------------------------------------------- stored, non-secret data

/// The signed-in account (no secrets). Present while the user is signed in.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Account {
    profile_id: String,
    user_id: String,
    username: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CachedProfile {
    profile_id: String,
    profile: RankedProfile,
}

fn load_json<T: for<'de> Deserialize<'de>>(conn: &rusqlite::Connection, key: &str) -> Option<T> {
    let raw = store::get_setting(conn, key).ok().flatten()?;
    if raw.trim().is_empty() {
        return None;
    }
    serde_json::from_str(&raw).map_err(|e| log::warn!("ubisoft: setting {key} unreadable: {e}")).ok()
}

fn save_json<T: Serialize>(conn: &rusqlite::Connection, key: &str, value: Option<&T>) {
    let raw = value.and_then(|v| serde_json::to_string(v).ok()).unwrap_or_default();
    if let Err(e) = store::set_setting(conn, key, &raw) {
        log::warn!("ubisoft: cannot save setting {key}: {e}");
    }
}

// ---------------------------------------------------------------- remember-me ticket (keyring)

fn keyring_entry() -> Option<keyring::Entry> {
    keyring::Entry::new(KEYRING_SERVICE, KEYRING_USER)
        .map_err(|e| log::warn!("ubisoft: credential store unavailable: {e}"))
        .ok()
}

fn rm_load() -> Option<String> {
    match keyring_entry()?.get_password() {
        Ok(t) if !t.is_empty() => Some(t),
        Ok(_) | Err(keyring::Error::NoEntry) => None,
        Err(e) => {
            log::warn!("ubisoft: cannot read remember-me ticket: {e}");
            None
        }
    }
}

fn rm_save(ticket: &str) {
    if let Some(entry) = keyring_entry() {
        if let Err(e) = entry.set_password(ticket) {
            log::warn!("ubisoft: cannot save remember-me ticket: {e}");
        }
    }
}

fn rm_delete() {
    if let Some(entry) = keyring_entry() {
        match entry.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => {}
            Err(e) => log::warn!("ubisoft: cannot delete remember-me ticket: {e}"),
        }
    }
}

// ---------------------------------------------------------------- errors

#[derive(Debug, Clone, PartialEq, Eq)]
enum UbiError {
    NotSignedIn,
    MissingInput,
    WrongCredentials,
    WrongCode,
    No2faPending,
    Captcha,
    RateLimited,
    Network,
    Expired,
    Server(u16),
    Refused(u16),
    Unexpected(String),
}

impl UbiError {
    /// English text the UI can show as is. Never contains secrets or response bodies.
    fn message(&self) -> String {
        match self {
            UbiError::NotSignedIn => "You are not signed in to Ubisoft.".into(),
            UbiError::MissingInput => "Enter your Ubisoft email and password.".into(),
            UbiError::WrongCredentials => "Wrong email or password.".into(),
            UbiError::WrongCode => "The two-factor code is wrong or has expired. Try again.".into(),
            UbiError::No2faPending => "No sign-in is waiting for a two-factor code. Sign in again.".into(),
            UbiError::Captcha => "Ubisoft wants a security check (captcha). Sign in once at ubisoft.com in your browser, then try again here.".into(),
            UbiError::RateLimited => "Ubisoft is limiting requests right now. Wait a few minutes and try again.".into(),
            UbiError::Network => "Cannot reach Ubisoft. Check your internet connection.".into(),
            UbiError::Expired => "Your Ubisoft sign-in has expired. Please sign in again.".into(),
            UbiError::Server(s) => format!("Ubisoft services are not responding (HTTP {s}). Try again later."),
            UbiError::Refused(s) => format!("Ubisoft refused the request (HTTP {s})."),
            UbiError::Unexpected(what) => format!("Unexpected answer from Ubisoft ({what})."),
        }
    }
}

/// Which request failed; the same HTTP status means different things per step.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Phase {
    Password,
    TwoFactor,
    RememberMe,
    Data,
}

fn error_code(body: &str) -> Option<i64> {
    let v: Value = serde_json::from_str(body).ok()?;
    match v.get("errorCode")? {
        Value::Number(n) => n.as_i64(),
        Value::String(s) => s.trim().parse().ok(),
        _ => None,
    }
}

/// Maps a failed Ubisoft answer to a user-facing error. Error codes from ubi-docs
/// `guide/errors.md`: 1301 reCAPTCHA, 1307 / 1310 / 1022 wrong authenticator / email / SMS code,
/// 1101 too many calls per IP.
fn classify_error(status: u16, body: &str, phase: Phase) -> UbiError {
    let code = error_code(body);
    match (status, code) {
        (429, _) | (_, Some(1101)) => UbiError::RateLimited,
        (_, Some(1301)) => UbiError::Captcha,
        (_, Some(1307 | 1310 | 1022)) => UbiError::WrongCode,
        (500..=599, _) => UbiError::Server(status),
        (400 | 401, _) if phase == Phase::TwoFactor => UbiError::WrongCode,
        (401, _) if phase == Phase::Password => UbiError::WrongCredentials,
        (401, _) => UbiError::Expired,
        (403, _) => UbiError::Refused(status),
        _ => UbiError::Unexpected(format!("HTTP {status}")),
    }
}

fn net_error(e: reqwest::Error) -> UbiError {
    // `reqwest::Error`'s text carries the URL only (no headers), so it is safe to log.
    log::warn!("ubisoft: request failed: {e}");
    if e.is_decode() || e.is_body() {
        UbiError::Unexpected("unreadable response".into())
    } else {
        UbiError::Network
    }
}

// ---------------------------------------------------------------- sessions

/// A full Ubisoft session. Holds secrets: `Debug` is redacted.
#[derive(Clone)]
struct Session {
    app_id: &'static str,
    ticket: String,
    session_id: String,
    /// Raw expiration string; the datadev host wants it back in an `expiration` header.
    expiration_raw: String,
    expiration: Option<DateTime<Utc>>,
    profile_id: String,
    user_id: String,
    username: String,
    remember_me: Option<String>,
}

impl std::fmt::Debug for Session {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Session")
            .field("app_id", &self.app_id)
            .field("profile_id", &self.profile_id)
            .field("expiration", &self.expiration)
            .finish_non_exhaustive()
    }
}

impl Session {
    fn fresh(&self, now: DateTime<Utc>) -> bool {
        self.expiration.is_some_and(|exp| exp - chrono::Duration::seconds(RENEW_MARGIN_SECS) > now)
    }

    fn account(&self) -> Account {
        Account { profile_id: self.profile_id.clone(), user_id: self.user_id.clone(), username: self.username.clone() }
    }
}

/// `POST /v3/profiles/sessions` body (Session in ubi-docs `common-types.md`).
#[derive(Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
struct SessionBody {
    ticket: Option<String>,
    two_factor_authentication_ticket: Option<String>,
    code_communication_preference: Option<String>,
    profile_id: Option<String>,
    user_id: Option<String>,
    name_on_platform: Option<String>,
    expiration: Option<String>,
    session_id: Option<String>,
    remember_me_ticket: Option<String>,
}

enum SessionOutcome {
    Full(Session),
    /// 2FA is on: HTTP 200 with `ticket: null` and a 2FA ticket.
    TwoFactor { ticket: String, channel: Option<String> },
    Failed(UbiError),
}

fn non_empty(s: Option<String>) -> Option<String> {
    s.filter(|s| !s.trim().is_empty())
}

fn parse_expiration(raw: &str) -> Option<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(raw.trim()).ok().map(|d| d.with_timezone(&Utc))
}

/// Reads a sessions answer. Pure, so the 2FA and error branches are unit-tested on fixtures.
fn parse_session(status: u16, body: &str, app_id: &'static str, phase: Phase) -> SessionOutcome {
    if !(200..300).contains(&status) {
        return SessionOutcome::Failed(classify_error(status, body, phase));
    }
    let b: SessionBody = match serde_json::from_str(body) {
        Ok(b) => b,
        Err(_) => return SessionOutcome::Failed(UbiError::Unexpected("unreadable sign-in answer".into())),
    };
    if let Some(ticket) = non_empty(b.ticket) {
        let profile_id = non_empty(b.profile_id).unwrap_or_default();
        let user_id = non_empty(b.user_id).unwrap_or_else(|| profile_id.clone());
        if profile_id.is_empty() && user_id.is_empty() {
            return SessionOutcome::Failed(UbiError::Unexpected("sign-in answer without a profile".into()));
        }
        let expiration_raw = b.expiration.unwrap_or_default();
        return SessionOutcome::Full(Session {
            app_id,
            ticket,
            session_id: b.session_id.unwrap_or_default(),
            expiration: parse_expiration(&expiration_raw),
            expiration_raw,
            profile_id: if profile_id.is_empty() { user_id.clone() } else { profile_id },
            user_id,
            username: b.name_on_platform.unwrap_or_default(),
            remember_me: non_empty(b.remember_me_ticket),
        });
    }
    if let Some(ticket) = non_empty(b.two_factor_authentication_ticket) {
        return SessionOutcome::TwoFactor { ticket, channel: non_empty(b.code_communication_preference) };
    }
    SessionOutcome::Failed(UbiError::Unexpected("sign-in answer without a ticket".into()))
}

fn two_factor_prompt(channel: Option<&str>) -> String {
    match channel.map(|c| c.to_ascii_lowercase()) {
        Some(c) if c.contains("mail") => "Enter the two-factor code Ubisoft sent to your email.".into(),
        Some(c) if c.contains("sms") || c.contains("phone") => "Enter the two-factor code Ubisoft sent to your phone.".into(),
        Some(c) if c.contains("authenticator") || c.contains("app") => "Enter the code from your authenticator app.".into(),
        _ => "Enter your Ubisoft two-factor code.".into(),
    }
}

// ---------------------------------------------------------------- response parsing

/// Ranked 2.0 / Siege X rank table for seasons 28+ (danielwerg/r6data `src/ranks.ts`, also
/// siegeapi `ranks_v6`): rank id -> (name, icon key under `/assets/ranks/<key>.png`).
const RANKS: [(&str, &str); 37] = [
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

#[derive(Deserialize, Default)]
#[serde(default)]
struct SkillProfile {
    rank: i64,
    rank_points: i64,
    max_rank: i64,
    max_rank_points: i64,
    season_id: i64,
    top_rank_position: i64,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct SeasonStatistics {
    kills: u64,
    deaths: u64,
    match_outcomes: MatchOutcomes,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct MatchOutcomes {
    wins: u64,
    losses: u64,
    abandons: u64,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct FullProfileEntry {
    profile: SkillProfile,
    season_statistics: SeasonStatistics,
}

/// Parses `skill/full_profiles` into a profile (without `official.all`). The PC family's
/// `ranked` board is used; without one the player is unranked this season (zeros, no season
/// totals).
fn parse_full_profiles(body: &str, fetched_at: &str) -> Result<RankedProfile, UbiError> {
    let v: Value = serde_json::from_str(body).map_err(|_| UbiError::Unexpected("unreadable rank answer".into()))?;
    let families = v
        .get("platform_families_full_profiles")
        .and_then(Value::as_array)
        .ok_or_else(|| UbiError::Unexpected("rank answer without profiles".into()))?;
    let family = families
        .iter()
        .find(|f| f.get("platform_family").and_then(Value::as_str) == Some("pc"))
        .or_else(|| families.first());
    let boards = family.and_then(|f| f.get("board_ids_full_profiles")).and_then(Value::as_array);
    let board_entry = |id: &str| {
        boards?
            .iter()
            .find(|b| b.get("board_id").and_then(Value::as_str) == Some(id))
            .and_then(|b| b.get("full_profiles")?.as_array()?.first())
            .and_then(|p| serde_json::from_value::<FullProfileEntry>(p.clone()).ok())
    };
    let ranked = board_entry("ranked");
    // Season number from any board when the ranked board is missing.
    let any_season = boards
        .into_iter()
        .flatten()
        .filter_map(|b| b.get("full_profiles")?.as_array()?.first()?.get("profile")?.get("season_id")?.as_i64())
        .max()
        .unwrap_or(0);

    let has_ranked = ranked.is_some();
    let e = ranked.unwrap_or_default();
    let (p, s) = (&e.profile, &e.season_statistics);
    let (rank_name, rank_icon) = rank_info(p.rank);
    let (max_rank_name, _) = rank_info(p.max_rank);
    let o = &s.match_outcomes;
    let season_totals = has_ranked.then(|| {
        totals(Scope::Season, s.kills, s.deaths, o.wins, o.losses, o.wins + o.losses + o.abandons, fetched_at)
    });
    Ok(RankedProfile {
        season: season_code(if has_ranked { p.season_id } else { any_season }),
        rank_name,
        rank_points: p.rank_points,
        max_rank_name,
        max_rank_points: p.max_rank_points,
        top_rank_position: u32::try_from(p.top_rank_position).ok().filter(|&n| n > 0),
        kills: s.kills,
        deaths: s.deaths,
        wins: o.wins,
        losses: o.losses,
        abandons: o.abandons,
        rank_icon,
        fetched_at: fetched_at.to_string(),
        official: OfficialPair { season: season_totals, all: None },
    })
}

fn datadev_url(user_id: &str) -> String {
    format!(
        "https://prod.datadev.ubisoft.com/v1/users/{user_id}/playerstats?spaceId={R6_SPACE_ID}\
         &view=seasonal&aggregation=summary&gameMode=ranked&platformGroup=PC&teamRole=all"
    )
}

/// All-time Ranked totals from the datadev seasonal summary: the sum over every season row of
/// `profileData.<id>.platforms.PC.gameModes.ranked.teamRoles.all[]` (field names from siegeapi 6.x
/// `default_stats.py`: matchesPlayed, matchesWon, matchesLost, kills, death). `None` when the
/// shape is not there or holds no matches.
fn parse_datadev_all(body: &str, fetched_at: &str) -> Option<OfficialTotals> {
    let v: Value = serde_json::from_str(body).ok()?;
    let profiles = v.get("profileData")?.as_object()?;
    let rows = profiles
        .values()
        .next()?
        .get("platforms")?
        .get("PC")?
        .get("gameModes")?
        .get("ranked")?
        .get("teamRoles")?
        .get("all")?
        .as_array()?;
    let n = |r: &Value, k: &str| r.get(k).and_then(Value::as_u64).unwrap_or(0);
    let (mut played, mut won, mut lost, mut kills, mut deaths) = (0, 0, 0, 0, 0);
    for r in rows {
        played += n(r, "matchesPlayed");
        won += n(r, "matchesWon");
        lost += n(r, "matchesLost");
        kills += n(r, "kills");
        deaths += n(r, "death");
    }
    (played > 0).then(|| totals(Scope::All, kills, deaths, won, lost, played, fetched_at))
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

fn official_from_cache(account: Option<&Account>, cache: Option<&CachedProfile>, f: &Filter) -> Option<OfficialTotals> {
    let (account, cache) = (account?, cache?);
    if cache.profile_id != account.profile_id || !plain_filter(f) {
        return None;
    }
    match f.range {
        Range::Season => cache.profile.official.season.clone(),
        Range::All => cache.profile.official.all.clone(),
        _ => None,
    }
}

/// Official totals for a query, from the stored cache only (no network). `None` when signed
/// out, nothing was fetched yet or the filter is not plain `season` / `all`.
pub fn cached_official(conn: &rusqlite::Connection, f: &Filter) -> Option<OfficialTotals> {
    if !plain_filter(f) {
        return None;
    }
    let account: Option<Account> = load_json(conn, KEY_ACCOUNT);
    let cache: Option<CachedProfile> = load_json(conn, KEY_RANKED_CACHE);
    official_from_cache(account.as_ref(), cache.as_ref(), f)
}

// ---------------------------------------------------------------- replay player check

fn same_player(account: &Account, me_profile_id: &str) -> Option<bool> {
    let me = me_profile_id.trim();
    if me.is_empty() {
        return None;
    }
    Some(me.eq_ignore_ascii_case(&account.profile_id) || me.eq_ignore_ascii_case(&account.user_id))
}

/// Compares the account with the newest stored match's recording player.
fn matches_replay_player(conn: &rusqlite::Connection, account: &Account) -> Option<bool> {
    let matches = store::load_full_matches(conn).map_err(|e| log::warn!("ubisoft: cannot load matches: {e}")).ok()?;
    same_player(account, &matches.last()?.me_profile_id)
}

// ---------------------------------------------------------------- state and HTTP

#[derive(Default)]
struct Inner {
    main: Option<Session>,
    r6: Option<Session>,
    /// 2FA ticket of a sign-in waiting for its code (memory only).
    pending_2fa: Option<String>,
    cache: Option<CachedProfile>,
}

/// Tauri state. The async mutex also serializes sign-in and fetches (no duplicate requests).
pub struct UbiState {
    inner: Mutex<Inner>,
    client: Option<reqwest::Client>,
}

impl UbiState {
    /// Builds the HTTP client. No network traffic happens here.
    pub fn new() -> Self {
        // reqwest uses rustls without a built-in provider; install ring once (an error only
        // means a provider is already installed).
        let _ = rustls::crypto::ring::default_provider().install_default();
        let client = reqwest::Client::builder()
            .timeout(HTTP_TIMEOUT)
            .user_agent(USER_AGENT)
            .build()
            .map_err(|e| log::error!("ubisoft: cannot build HTTP client: {e}"))
            .ok();
        Self { inner: Mutex::new(Inner::default()), client }
    }

    fn client(&self) -> Result<&reqwest::Client, UbiError> {
        self.client.as_ref().ok_or_else(|| UbiError::Unexpected("HTTP client unavailable".into()))
    }
}

async fn post_session(
    client: &reqwest::Client,
    authorization: &str,
    app_id: &'static str,
    extra: &[(&str, &str)],
    body: Value,
    phase: Phase,
) -> SessionOutcome {
    let mut req = client
        .post(SESSIONS_URL)
        .header("Authorization", authorization)
        .header("Ubi-AppId", app_id)
        .header("Ubi-RequestedPlatformType", "uplay")
        .header("Content-Type", "application/json; charset=UTF-8")
        .body(body.to_string());
    for (k, v) in extra {
        req = req.header(*k, *v);
    }
    let resp = match req.send().await {
        Ok(r) => r,
        Err(e) => return SessionOutcome::Failed(net_error(e)),
    };
    let status = resp.status().as_u16();
    match resp.text().await {
        Ok(text) => parse_session(status, &text, app_id, phase),
        Err(e) => SessionOutcome::Failed(net_error(e)),
    }
}

async fn get_text(client: &reqwest::Client, url: &str, s: &Session) -> Result<(u16, String), UbiError> {
    let resp = client
        .get(url)
        .header("Authorization", format!("Ubi_v1 t={}", s.ticket))
        .header("Ubi-AppId", s.app_id)
        .header("Ubi-SessionId", &s.session_id)
        .header("Ubi-LocaleCode", "en-US")
        .header("Ubi-RequestedPlatformType", "uplay")
        .header("expiration", &s.expiration_raw)
        .send()
        .await
        .map_err(net_error)?;
    let status = resp.status().as_u16();
    let text = resp.text().await.map_err(net_error)?;
    Ok((status, text))
}

fn now_iso() -> String {
    Utc::now().to_rfc3339_opts(SecondsFormat::Secs, true)
}

fn cache_fresh(c: &CachedProfile, now: DateTime<Utc>) -> bool {
    parse_expiration(&c.profile.fetched_at).is_some_and(|t| (now - t).num_seconds() < CACHE_TTL_SECS)
}

/// Clears everything the app knows about the account (memory, keyring, settings).
fn forget_account(inner: &mut Inner, conn: Option<&rusqlite::Connection>) {
    *inner = Inner::default();
    rm_delete();
    if let Some(conn) = conn {
        save_json::<Account>(conn, KEY_ACCOUNT, None);
        save_json::<CachedProfile>(conn, KEY_RANKED_CACHE, None);
    }
}

/// Stores a fresh main session: remember-me ticket to the keyring, account to the settings.
fn adopt_main(inner: &mut Inner, db: &store::Db, session: Session) {
    match &session.remember_me {
        Some(rm) => rm_save(rm),
        None => log::warn!("ubisoft: no remember-me ticket in session; sign-in lasts until the app closes"),
    }
    let account = session.account();
    if let Ok(conn) = lock(db) {
        let old: Option<Account> = load_json(&conn, KEY_ACCOUNT);
        if old.as_ref().is_some_and(|o| o.profile_id != account.profile_id) {
            save_json::<CachedProfile>(&conn, KEY_RANKED_CACHE, None);
            inner.cache = None;
        }
        save_json(&conn, KEY_ACCOUNT, Some(&account));
    }
    inner.main = Some(session);
    inner.r6 = None;
    inner.pending_2fa = None;
}

/// A valid main session: the one in memory, else renewed with the remember-me ticket.
async fn ensure_main(inner: &mut Inner, client: &reqwest::Client, db: &store::Db) -> Result<Session, UbiError> {
    if let Some(s) = inner.main.as_ref().filter(|s| s.fresh(Utc::now())) {
        return Ok(s.clone());
    }
    let rm = rm_load().or_else(|| inner.main.as_ref().and_then(|s| s.remember_me.clone())).ok_or(UbiError::NotSignedIn)?;
    log::info!("ubisoft: renewing session with remember-me ticket");
    let auth = format!("rm_v1 t={rm}");
    match post_session(client, &auth, MAIN_APP_ID, &[], json!({ "rememberMe": true }), Phase::RememberMe).await {
        SessionOutcome::Full(s) => {
            adopt_main(inner, db, s.clone());
            Ok(s)
        }
        SessionOutcome::TwoFactor { .. } | SessionOutcome::Failed(UbiError::Expired) => {
            log::info!("ubisoft: remember-me ticket rejected; signing out");
            let conn = lock(db).ok();
            forget_account(inner, conn.as_deref());
            Err(UbiError::Expired)
        }
        SessionOutcome::Failed(e) => Err(e),
    }
}

/// The session for stats: the main ticket exchanged to the R6 app id; the main ticket itself
/// when the exchange fails.
async fn stats_session(inner: &mut Inner, client: &reqwest::Client, db: &store::Db) -> Result<Session, UbiError> {
    let main = ensure_main(inner, client, db).await?;
    if let Some(s) = inner.r6.as_ref().filter(|s| s.fresh(Utc::now())) {
        return Ok(s.clone());
    }
    let auth = format!("Ubi_v1 t={}", main.ticket);
    let sid = [("Ubi-SessionId", main.session_id.as_str())];
    match post_session(client, &auth, R6_APP_ID, &sid, json!({}), Phase::Data).await {
        SessionOutcome::Full(s) => {
            inner.r6 = Some(s.clone());
            Ok(s)
        }
        SessionOutcome::Failed(UbiError::RateLimited) => Err(UbiError::RateLimited),
        SessionOutcome::Failed(e) => {
            log::warn!("ubisoft: ticket exchange failed ({e:?}); using the main ticket");
            Ok(main)
        }
        SessionOutcome::TwoFactor { .. } => Ok(main),
    }
}

async fn fetch_profile(inner: &mut Inner, client: &reqwest::Client, db: &store::Db) -> Result<CachedProfile, UbiError> {
    let mut retried = false;
    loop {
        let s = stats_session(inner, client, db).await?;
        let url = format!(
            "https://public-ubiservices.ubi.com/v2/spaces/{R6_SPACE_ID}/title/r6s/skill/full_profiles?profile_ids={}&platform_families=pc",
            s.user_id
        );
        let (status, body) = get_text(client, &url, &s).await?;
        if status == 401 && !retried {
            // Ticket died early: drop both sessions and renew once.
            retried = true;
            inner.main = None;
            inner.r6 = None;
            continue;
        }
        if !(200..300).contains(&status) {
            return Err(classify_error(status, &body, Phase::Data));
        }
        let fetched_at = now_iso();
        let mut profile = parse_full_profiles(&body, &fetched_at)?;
        profile.official.all = match get_text(client, &datadev_url(&s.user_id), &s).await {
            Ok((200, body)) => {
                let all = parse_datadev_all(&body, &fetched_at);
                if all.is_none() {
                    log::info!("ubisoft: datadev summary had no ranked rows");
                }
                all
            }
            Ok((status, _)) => {
                log::info!("ubisoft: datadev summary unavailable (HTTP {status})");
                None
            }
            Err(e) => {
                log::info!("ubisoft: datadev summary failed ({e:?})");
                None
            }
        };
        log::info!(
            "ubisoft: ranked profile {} {} RP, season totals {}, all-time totals {}",
            profile.rank_name,
            profile.rank_points,
            profile.official.season.is_some(),
            profile.official.all.is_some()
        );
        return Ok(CachedProfile { profile_id: s.profile_id.clone(), profile });
    }
}

fn status_of(conn: &rusqlite::Connection, signed_in_memory: bool) -> UbiStatus {
    let account: Option<Account> = load_json(conn, KEY_ACCOUNT);
    match account {
        Some(a) if signed_in_memory || rm_load().is_some() => UbiStatus {
            signed_in: true,
            username: Some(a.username.clone()).filter(|s| !s.is_empty()),
            profile_id: Some(a.profile_id.clone()),
            matches_replay_player: matches_replay_player(conn, &a),
        },
        _ => UbiStatus::default(),
    }
}

fn status_now(db: &store::Db, signed_in_memory: bool) -> Cmd<UbiStatus> {
    Ok(status_of(&*lock(db)?, signed_in_memory))
}

// ---------------------------------------------------------------- commands

/// Local only (settings + Credential Manager), no network.
#[tauri::command]
pub async fn ubi_status(state: tauri::State<'_, UbiState>, db: tauri::State<'_, store::Db>) -> Cmd<UbiStatus> {
    let in_memory = state.inner.lock().await.main.is_some();
    status_now(&db, in_memory)
}

#[tauri::command]
pub async fn ubi_login(
    state: tauri::State<'_, UbiState>,
    db: tauri::State<'_, store::Db>,
    email: String,
    password: String,
) -> Cmd<UbiLoginResult> {
    let email = email.trim();
    if email.is_empty() || password.is_empty() {
        return Ok(UbiLoginResult::error(&UbiError::MissingInput));
    }
    let client = match state.client() {
        Ok(c) => c,
        Err(e) => return Ok(UbiLoginResult::error(&e)),
    };
    let mut inner = state.inner.lock().await;
    inner.pending_2fa = None;
    let basic = base64::engine::general_purpose::STANDARD.encode(format!("{email}:{password}"));
    drop(password);
    log::info!("ubisoft: signing in");
    let outcome =
        post_session(client, &format!("Basic {basic}"), MAIN_APP_ID, &[], json!({ "rememberMe": true }), Phase::Password)
            .await;
    Ok(match outcome {
        SessionOutcome::Full(s) => {
            log::info!("ubisoft: signed in as {}", s.username);
            adopt_main(&mut inner, &db, s);
            UbiLoginResult { status: LoginStatus::Ok, message: None, user: Some(status_now(&db, true)?) }
        }
        SessionOutcome::TwoFactor { ticket, channel } => {
            log::info!("ubisoft: two-factor code needed");
            inner.pending_2fa = Some(ticket);
            UbiLoginResult { status: LoginStatus::Needs2fa, message: Some(two_factor_prompt(channel.as_deref())), user: None }
        }
        SessionOutcome::Failed(e) => {
            log::info!("ubisoft: sign-in failed: {e:?}");
            UbiLoginResult::error(&e)
        }
    })
}

#[tauri::command]
pub async fn ubi_login_2fa(
    state: tauri::State<'_, UbiState>,
    db: tauri::State<'_, store::Db>,
    code: String,
) -> Cmd<UbiLoginResult> {
    let code: String = code.chars().filter(|c| !c.is_whitespace()).collect();
    let client = match state.client() {
        Ok(c) => c,
        Err(e) => return Ok(UbiLoginResult::error(&e)),
    };
    let mut inner = state.inner.lock().await;
    let Some(ticket) = inner.pending_2fa.clone() else {
        return Ok(UbiLoginResult::error(&UbiError::No2faPending));
    };
    if code.is_empty() || !code.chars().all(|c| c.is_ascii_digit()) {
        return Ok(UbiLoginResult::error(&UbiError::WrongCode));
    }
    let auth = format!("Ubi_2fa_v1 t={ticket}");
    let outcome = post_session(
        client,
        &auth,
        MAIN_APP_ID,
        &[("Ubi-2FACode", code.as_str())],
        json!({ "rememberMe": true }),
        Phase::TwoFactor,
    )
    .await;
    Ok(match outcome {
        SessionOutcome::Full(s) => {
            log::info!("ubisoft: signed in with two-factor code as {}", s.username);
            adopt_main(&mut inner, &db, s);
            UbiLoginResult { status: LoginStatus::Ok, message: None, user: Some(status_now(&db, true)?) }
        }
        // Still only a 2FA ticket: the code was not accepted. Keep the pending ticket for a retry.
        SessionOutcome::TwoFactor { .. } => UbiLoginResult::error(&UbiError::WrongCode),
        SessionOutcome::Failed(e) => {
            log::info!("ubisoft: two-factor step failed: {e:?}");
            if e == UbiError::Expired {
                inner.pending_2fa = None;
            }
            UbiLoginResult::error(&e)
        }
    })
}

/// Forgets the session, the remember-me ticket, the account and the cached profile.
#[tauri::command]
pub async fn ubi_logout(state: tauri::State<'_, UbiState>, db: tauri::State<'_, store::Db>) -> Cmd<()> {
    let mut inner = state.inner.lock().await;
    let conn = lock(&db)?;
    forget_account(&mut inner, Some(&conn));
    log::info!("ubisoft: signed out");
    Ok(())
}

/// The signed-in user's ranked profile; `null` when signed out. Served from the 10-minute
/// cache unless `force`. When a refresh fails without `force`, the older cached profile is
/// returned (its `fetchedAt` shows its age).
#[tauri::command]
pub async fn ranked_profile(
    state: tauri::State<'_, UbiState>,
    db: tauri::State<'_, store::Db>,
    force: bool,
) -> Cmd<Option<RankedProfile>> {
    let mut inner = state.inner.lock().await;
    let (account, stored) = {
        let conn = lock(&db)?;
        (load_json::<Account>(&conn, KEY_ACCOUNT), load_json::<CachedProfile>(&conn, KEY_RANKED_CACHE))
    };
    let Some(account) = account else {
        return Ok(None);
    };
    if inner.main.is_none() && rm_load().is_none() {
        return Ok(None);
    }
    if inner.cache.is_none() {
        inner.cache = stored;
    }
    let cached = inner.cache.clone().filter(|c| c.profile_id == account.profile_id);
    if !force {
        if let Some(c) = cached.as_ref().filter(|c| cache_fresh(c, Utc::now())) {
            return Ok(Some(c.profile.clone()));
        }
    }
    let client = state.client().map_err(|e| e.message())?;
    match fetch_profile(&mut inner, client, &db).await {
        Ok(fresh) => {
            if let Ok(conn) = lock(&db) {
                save_json(&conn, KEY_RANKED_CACHE, Some(&fresh));
            }
            let profile = fresh.profile.clone();
            inner.cache = Some(fresh);
            Ok(Some(profile))
        }
        Err(UbiError::NotSignedIn) => Ok(None),
        Err(e @ UbiError::Expired) => Err(e.message()),
        Err(e) => match cached {
            Some(c) if !force => {
                log::warn!("ubisoft: refresh failed ({e:?}); serving cached profile");
                Ok(Some(c.profile))
            }
            _ => Err(e.message()),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const AT: &str = "2026-11-07T12:00:00Z";

    // Fixtures shaped like the documented answers; every id and ticket is made up.
    const SESSION_OK: &str = r#"{"platformType":"uplay","ticket":"FAKE-TICKET","twoFactorAuthenticationTicket":null,
        "profileId":"11111111-2222-3333-4444-555555555555","userId":"11111111-2222-3333-4444-555555555555",
        "nameOnPlatform":"Tester.R6","environment":"Prod","expiration":"2026-11-07T15:00:00.1234567Z",
        "spaceId":"45d58365-547f-4b45-ab5b-53ed14cc79ed","sessionId":"aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee",
        "rememberMeTicket":"FAKE-RM"}"#;
    const SESSION_2FA: &str = r#"{"platformType":"uplay","ticket":null,"twoFactorAuthenticationTicket":"FAKE-2FA",
        "codeCommunicationPreference":"Email","profileId":"11111111-2222-3333-4444-555555555555",
        "userId":"11111111-2222-3333-4444-555555555555","nameOnPlatform":"Tester.R6","sessionId":null,
        "rememberMeTicket":null}"#;
    const FULL_PROFILES: &str = r#"{"platform_families_full_profiles":[
        {"platform_family":"pc","board_ids_full_profiles":[
          {"board_id":"casual","full_profiles":[{"profile":{"board_id":"casual","id":"x","max_rank":0,"max_rank_points":0,
             "platform_family":"pc","rank":0,"rank_points":0,"season_id":43,"top_rank_position":0},
             "season_statistics":{"deaths":50,"kills":60,"match_outcomes":{"abandons":0,"losses":5,"wins":7}}}]},
          {"board_id":"ranked","full_profiles":[{"profile":{"board_id":"ranked","id":"x","max_rank":30,"max_rank_points":3950,
             "platform_family":"pc","rank":29,"rank_points":3850,"season_id":43,"top_rank_position":0},
             "season_statistics":{"deaths":100,"kills":123,"match_outcomes":{"abandons":1,"losses":9,"wins":10}}}]}
        ]}]}"#;

    fn full(outcome: SessionOutcome) -> Session {
        match outcome {
            SessionOutcome::Full(s) => s,
            SessionOutcome::TwoFactor { .. } => panic!("expected full session, got 2FA"),
            SessionOutcome::Failed(e) => panic!("expected full session, got {e:?}"),
        }
    }

    #[test]
    fn full_session_parses() {
        let s = full(parse_session(200, SESSION_OK, MAIN_APP_ID, Phase::Password));
        assert_eq!(s.ticket, "FAKE-TICKET");
        assert_eq!(s.username, "Tester.R6");
        assert_eq!(s.remember_me.as_deref(), Some("FAKE-RM"));
        assert_eq!(s.session_id, "aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee");
        let exp = s.expiration.expect("7-digit fraction parses");
        assert!(s.fresh(exp - chrono::Duration::minutes(10)));
        assert!(!s.fresh(exp - chrono::Duration::minutes(2)));
    }

    #[test]
    fn debug_output_hides_secrets() {
        let s = full(parse_session(200, SESSION_OK, MAIN_APP_ID, Phase::Password));
        let text = format!("{s:?}");
        assert!(!text.contains("FAKE-TICKET") && !text.contains("FAKE-RM"), "{text}");
    }

    #[test]
    fn two_factor_answer_needs_code() {
        match parse_session(200, SESSION_2FA, MAIN_APP_ID, Phase::Password) {
            SessionOutcome::TwoFactor { ticket, channel } => {
                assert_eq!(ticket, "FAKE-2FA");
                assert_eq!(two_factor_prompt(channel.as_deref()), "Enter the two-factor code Ubisoft sent to your email.");
            }
            _ => panic!("expected 2FA"),
        }
        assert_eq!(two_factor_prompt(None), "Enter your Ubisoft two-factor code.");
    }

    fn failed(status: u16, body: &str, phase: Phase) -> UbiError {
        match parse_session(status, body, MAIN_APP_ID, phase) {
            SessionOutcome::Failed(e) => e,
            _ => panic!("expected failure"),
        }
    }

    #[test]
    fn error_branches() {
        let bad = r#"{"errorCode":1,"httpCode":401,"message":"Invalid credentials"}"#;
        assert_eq!(failed(401, bad, Phase::Password), UbiError::WrongCredentials);
        assert_eq!(failed(401, bad, Phase::RememberMe), UbiError::Expired);
        assert_eq!(failed(400, "{}", Phase::TwoFactor), UbiError::WrongCode);
        assert_eq!(failed(400, r#"{"errorCode":1307}"#, Phase::Password), UbiError::WrongCode);
        assert_eq!(failed(429, "", Phase::Password), UbiError::RateLimited);
        assert_eq!(failed(400, r#"{"errorCode":"1101","httpCode":429}"#, Phase::Data), UbiError::RateLimited);
        assert_eq!(failed(409, r#"{"errorCode":1301}"#, Phase::Password), UbiError::Captcha);
        assert_eq!(failed(503, "<html>", Phase::Password), UbiError::Server(503));
        assert_eq!(failed(403, "{}", Phase::Data), UbiError::Refused(403));
        assert_eq!(failed(418, "{}", Phase::Data), UbiError::Unexpected("HTTP 418".into()));
        // 200 without any ticket, or garbage, is not a sign-in.
        assert!(matches!(failed(200, "{}", Phase::Password), UbiError::Unexpected(_)));
        assert!(matches!(failed(200, "not json", Phase::Password), UbiError::Unexpected(_)));
        // Messages are English and never echo the body.
        assert_eq!(UbiError::WrongCredentials.message(), "Wrong email or password.");
        assert!(!UbiError::Unexpected("HTTP 418".into()).message().contains("Invalid credentials"));
    }

    #[test]
    fn login_result_serializes_to_contract() {
        let r = UbiLoginResult { status: LoginStatus::Needs2fa, message: Some("x".into()), user: None };
        assert_eq!(serde_json::to_string(&r).unwrap(), r#"{"status":"needs2fa","message":"x","user":null}"#);
        let s = UbiStatus { signed_in: true, username: Some("A".into()), profile_id: Some("p".into()), matches_replay_player: None };
        assert_eq!(
            serde_json::to_string(&s).unwrap(),
            r#"{"signedIn":true,"username":"A","profileId":"p","matchesReplayPlayer":null}"#
        );
    }

    #[test]
    fn full_profiles_parse_ranked_board() {
        let p = parse_full_profiles(FULL_PROFILES, AT).unwrap();
        assert_eq!(p.season, "Y11S3");
        assert_eq!(p.rank_name, "Emerald II");
        assert_eq!(p.rank_icon.as_deref(), Some("emerald-2"));
        assert_eq!((p.rank_points, p.max_rank_name.as_str(), p.max_rank_points), (3850, "Emerald I", 3950));
        assert_eq!(p.top_rank_position, None);
        assert_eq!((p.kills, p.deaths, p.wins, p.losses, p.abandons), (123, 100, 10, 9, 1));
        let s = p.official.season.clone().unwrap();
        assert_eq!((s.scope, s.kd, s.win_pct, s.matches), (Scope::Season, 1.23, 50.0, 20));
        assert!(p.official.all.is_none());
        let json = serde_json::to_value(&p).unwrap();
        assert_eq!(json["rankIcon"], "emerald-2");
        assert_eq!(json["official"]["season"]["winPct"], 50.0);
        assert!(json["official"]["all"].is_null());
    }

    #[test]
    fn full_profiles_without_ranked_board_is_unranked() {
        let body = r#"{"platform_families_full_profiles":[{"platform_family":"pc","board_ids_full_profiles":[
            {"board_id":"casual","full_profiles":[{"profile":{"season_id":43,"rank":0},"season_statistics":{}}]}]}]}"#;
        let p = parse_full_profiles(body, AT).unwrap();
        assert_eq!((p.rank_name.as_str(), p.season.as_str(), p.kills), ("Unranked", "Y11S3", 0));
        assert!(p.official.season.is_none());
        assert!(matches!(parse_full_profiles("{}", AT), Err(UbiError::Unexpected(_))));
        assert!(matches!(parse_full_profiles("<html>", AT), Err(UbiError::Unexpected(_))));
    }

    #[test]
    fn rank_table_and_seasons() {
        assert_eq!(rank_info(0), ("Unranked".to_string(), Some("unranked".to_string())));
        assert_eq!(rank_info(1).0, "Copper V");
        assert_eq!(rank_info(36), ("Champion".to_string(), Some("champion".to_string())));
        assert_eq!(rank_info(37), ("Rank 37".to_string(), None));
        assert_eq!(rank_info(-1).1, None);
        assert_eq!(season_code(43), "Y11S3");
        assert_eq!(season_code(28), "Y7S4");
        assert_eq!(season_code(0), "");
        // Every icon key has a file in app/static/assets/ranks.
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../static/assets/ranks");
        for (_, key) in RANKS {
            assert!(dir.join(format!("{key}.png")).is_file(), "missing icon {key}");
        }
    }

    #[test]
    fn champion_position_is_kept() {
        let body = r#"{"platform_families_full_profiles":[{"platform_family":"pc","board_ids_full_profiles":[
            {"board_id":"ranked","full_profiles":[{"profile":{"max_rank":36,"max_rank_points":4700,"rank":36,
              "rank_points":4650,"season_id":43,"top_rank_position":512},"season_statistics":{"kills":1,"deaths":0,
              "match_outcomes":{"wins":1,"losses":0,"abandons":0}}}]}]}]}"#;
        let p = parse_full_profiles(body, AT).unwrap();
        assert_eq!((p.rank_name.as_str(), p.rank_icon.as_deref(), p.top_rank_position), ("Champion", Some("champion"), Some(512)));
        assert_eq!(p.official.season.unwrap().kd, 1.0, "zero deaths gives the kill count");
    }

    #[test]
    fn datadev_summary_sums_seasons() {
        let body = r#"{"profileData":{"11111111-2222-3333-4444-555555555555":{"platforms":{"PC":{"gameModes":{"ranked":{
            "type":"ranked","teamRoles":{"all":[
              {"type":"Seasonal","seasonYear":"Y11","seasonNumber":"S2","matchesPlayed":30,"matchesWon":18,"matchesLost":12,"kills":200,"death":150},
              {"type":"Seasonal","seasonYear":"Y11","seasonNumber":"S3","matchesPlayed":10,"matchesWon":4,"matchesLost":6,"kills":50,"death":50}
            ]}}}}}}}}"#;
        let t = parse_datadev_all(body, AT).unwrap();
        assert_eq!((t.scope, t.matches, t.wins, t.losses, t.kills, t.deaths), (Scope::All, 40, 22, 18, 250, 200));
        assert_eq!((t.kd, t.win_pct), (1.25, 55.0));
        assert!(parse_datadev_all(r#"{"profileData":{}}"#, AT).is_none());
        assert!(parse_datadev_all("Gone", AT).is_none());
    }

    #[test]
    fn official_only_for_plain_filters() {
        let account = Account { profile_id: "p1".into(), user_id: "p1".into(), username: "A".into() };
        let mut profile = parse_full_profiles(FULL_PROFILES, AT).unwrap();
        profile.official.all = Some(totals(Scope::All, 10, 5, 3, 2, 5, AT));
        let cache = CachedProfile { profile_id: "p1".into(), profile };
        let get = |f: &Filter| official_from_cache(Some(&account), Some(&cache), f);

        let season = Filter { range: Range::Season, ..Default::default() };
        assert_eq!(get(&season).unwrap().scope, Scope::Season);
        assert_eq!(get(&Filter::default()).unwrap().scope, Scope::All);
        assert_eq!(get(&Filter { mode: Mode::Ranked, ..Default::default() }).unwrap().scope, Scope::All);
        assert!(get(&Filter { range: Range::Last5, ..Default::default() }).is_none());
        assert!(get(&Filter { operators: vec!["ash".into()], ..Default::default() }).is_none());
        assert!(get(&Filter { maps: vec!["bank".into()], ..Default::default() }).is_none());
        assert!(get(&Filter { side: SideFilter::Attack, ..Default::default() }).is_none());
        assert!(get(&Filter { mode: Mode::Quick, ..Default::default() }).is_none());
        // Another account's cache is never shown; signed out shows nothing.
        let other = Account { profile_id: "p2".into(), ..account.clone() };
        assert!(official_from_cache(Some(&other), Some(&cache), &season).is_none());
        assert!(official_from_cache(None, Some(&cache), &season).is_none());
    }

    #[test]
    fn cached_official_reads_settings_only() {
        let conn = store::open(std::path::Path::new(":memory:")).unwrap();
        assert!(cached_official(&conn, &Filter::default()).is_none());
        let account = Account { profile_id: "p1".into(), user_id: "p1".into(), username: "A".into() };
        let cache = CachedProfile { profile_id: "p1".into(), profile: parse_full_profiles(FULL_PROFILES, AT).unwrap() };
        save_json(&conn, KEY_ACCOUNT, Some(&account));
        save_json(&conn, KEY_RANKED_CACHE, Some(&cache));
        let f = Filter { range: Range::Season, ..Default::default() };
        assert_eq!(cached_official(&conn, &f).unwrap().kills, 123);
        assert!(cached_official(&conn, &Filter::default()).is_none(), "no all-time totals cached");
        save_json::<Account>(&conn, KEY_ACCOUNT, None);
        assert!(cached_official(&conn, &f).is_none());
    }

    #[test]
    fn cache_ttl() {
        let c = CachedProfile { profile_id: "p".into(), profile: parse_full_profiles(FULL_PROFILES, AT).unwrap() };
        let at = parse_expiration(AT).unwrap();
        assert!(cache_fresh(&c, at + chrono::Duration::minutes(9)));
        assert!(!cache_fresh(&c, at + chrono::Duration::minutes(11)));
    }

    #[test]
    fn replay_player_match() {
        let a = Account { profile_id: "ABC".into(), user_id: "def".into(), username: "A".into() };
        assert_eq!(same_player(&a, "abc"), Some(true));
        assert_eq!(same_player(&a, "DEF"), Some(true));
        assert_eq!(same_player(&a, "other"), Some(false));
        assert_eq!(same_player(&a, ""), None);
        let conn = store::open(std::path::Path::new(":memory:")).unwrap();
        assert_eq!(matches_replay_player(&conn, &a), None, "no stored matches");
    }
}
