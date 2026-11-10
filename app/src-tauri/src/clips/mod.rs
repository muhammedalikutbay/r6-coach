//! Kill clips (v4 spec, decision 3): an opt-in rolling recording of the game window, cut into
//! short clips around the tracked player's kills and deaths whenever a round's `.rec` arrives.
//!
//! * `recorder`: Windows Graphics Capture of the game window into one-minute segments (temp folder).
//! * `audio`: the game's sound (process loopback), muxed into the same segments.
//! * `buffer`: names, listing and pruning of those segments.
//! * `clock`: wall-clock stamp of every recorded frame (the cutter works on the wall clock).
//! * `timing`: round clock (from the `.rec`) to wall clock.
//! * `cutter`: stream-copy cut of a wall-clock window out of the segments into an MP4.
//! * `retention`: last N matches, disk cap, starred clips.
//! * `db`: the `clips` table.
//!
//! Everything is off by default; nothing is recorded until the user turns it on in Settings.

pub(crate) mod audio;
pub(crate) mod buffer;
pub(crate) mod clock;
pub(crate) mod cutter;
pub(crate) mod db;
pub(crate) mod recorder;
pub(crate) mod retention;
pub(crate) mod timing;

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex};

use r6k_core::raw::RawRound;
use rusqlite::Connection;
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager};

use crate::{lock, store, Cmd};
use buffer::Segment;
use timing::{Anchor, RoundPlan, Target, TargetKind};

/// Tauri event after clips were cut for a round, payload `ClipsUpdated`.
pub const EVENT_CLIPS_UPDATED: &str = "clips-updated";
/// Tauri event for things the user should know, payload `ClipsWarning`.
pub const EVENT_CLIPS_WARNING: &str = "clips-warning";
/// Settings key of the JSON `ClipSettings`.
pub const KEY_SETTINGS: &str = "clips_settings";

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum ClipKinds {
    #[default]
    Kills,
    Deaths,
    Both,
}

impl ClipKinds {
    fn wants(self, kind: TargetKind) -> bool {
        matches!((self, kind), (ClipKinds::Both, _) | (ClipKinds::Kills, TargetKind::Kill) | (ClipKinds::Deaths, TargetKind::Death))
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ClipSettings {
    /// Recording is off until the user turns it on.
    pub enabled: bool,
    pub kinds: ClipKinds,
    /// Keep the clips of this many newest matches; 0 = all.
    pub keep_matches: u32,
    /// Disk cap for all clips in GB; 0 = none.
    pub disk_cap_gb: f64,
    /// Seconds added to the kill time before the clip window is placed (may be negative: earlier).
    pub offset_s: f64,
    /// Clip window: seconds before the (offset) kill time ...
    pub before_s: f64,
    /// ... and after it.
    pub after_s: f64,
    /// Record the game's sound into the clips (the game process only, not Discord and the like).
    pub record_audio: bool,
}

/// Recommended defaults (new installs; values a user already saved are never overwritten): the
/// window is centred on the kill time moved 5 s earlier, 10 s before and none after.
pub const DEFAULT_BEFORE_S: f64 = 10.0;
pub const DEFAULT_AFTER_S: f64 = 0.0;
pub const DEFAULT_OFFSET_S: f64 = -5.0;

impl Default for ClipSettings {
    fn default() -> Self {
        Self {
            enabled: false,
            kinds: ClipKinds::Kills,
            keep_matches: 10,
            disk_cap_gb: 0.0,
            offset_s: DEFAULT_OFFSET_S,
            before_s: DEFAULT_BEFORE_S,
            after_s: DEFAULT_AFTER_S,
            record_audio: true,
        }
    }
}

impl ClipSettings {
    /// Clamps every value to its allowed range (the UI limits them too).
    pub fn normalized(mut self) -> Self {
        let clamp = |v: f64, lo: f64, hi: f64, d: f64| if v.is_finite() { v.clamp(lo, hi) } else { d };
        self.keep_matches = self.keep_matches.min(1000);
        self.disk_cap_gb = clamp(self.disk_cap_gb, 0.0, 2000.0, 0.0);
        self.offset_s = clamp(self.offset_s, -30.0, 30.0, DEFAULT_OFFSET_S);
        self.before_s = clamp(self.before_s, 2.0, 20.0, DEFAULT_BEFORE_S);
        self.after_s = clamp(self.after_s, 0.0, 10.0, DEFAULT_AFTER_S);
        self
    }

    fn policy(&self) -> retention::Policy {
        retention::Policy {
            keep_matches: self.keep_matches,
            disk_cap_bytes: (self.disk_cap_gb > 0.0).then(|| (self.disk_cap_gb * 1e9) as u64),
        }
    }
}

pub fn load_settings(conn: &Connection) -> ClipSettings {
    match store::get_setting(conn, KEY_SETTINGS) {
        Ok(Some(json)) => serde_json::from_str::<ClipSettings>(&json).map(ClipSettings::normalized).unwrap_or_else(|e| {
            log::warn!("clips: stored settings are not valid ({e}); using the defaults");
            ClipSettings::default()
        }),
        _ => ClipSettings::default(),
    }
}

// ---------------------------------------------------------------------------------------------
// Cutting a round

/// A clip that was cut.
#[derive(Clone, Debug)]
pub struct CutClip {
    pub target: Target,
    pub path: PathBuf,
    pub size: u64,
    pub lead_ms: i64,
    pub duration_ms: i64,
}

fn safe_name(s: &str) -> String {
    let t: String = s.chars().map(|c| if c.is_ascii_alphanumeric() || c == '-' || c == '_' { c } else { '_' }).collect();
    if t.is_empty() { "match".into() } else { t }
}

/// Where the clip of a target goes: `<clips>/<match id>/R<round>-<kind>-<clock>-<n>.mp4`.
fn clip_path(clips_dir: &Path, plan: &RoundPlan, t: &Target, n: usize) -> PathBuf {
    clips_dir.join(safe_name(&plan.match_id)).join(format!("R{:02}-{}-{}-{}.mp4", plan.round, t.kind.as_str(), t.time_s, n))
}

/// Cuts every target out of `segments`. Targets that were not recorded (less than half of their
/// window is covered) are skipped. Returns the clips and the reasons of the skipped ones.
pub fn cut_round(plan: &RoundPlan, targets: &[Target], settings: &ClipSettings, segments: &[Segment], clips_dir: &Path) -> (Vec<CutClip>, Vec<String>) {
    let (mut clips, mut skipped) = (Vec::new(), Vec::new());
    for (n, t) in targets.iter().enumerate() {
        let centre = t.wall_ms + (settings.offset_s * 1000.0) as i64;
        let (start, end) = (centre - (settings.before_s * 1000.0) as i64, centre + (settings.after_s * 1000.0) as i64);
        let cover = buffer::coverage_ms(segments, start, end);
        if cover * 2 < end - start {
            skipped.push(format!("R{} {} at {}s: not recorded", plan.round, t.kind.as_str(), t.time_s));
            continue;
        }
        let out = clip_path(clips_dir, plan, t, n);
        if let Some(dir) = out.parent() {
            if let Err(e) = std::fs::create_dir_all(dir) {
                skipped.push(format!("cannot create {}: {e}", dir.display()));
                continue;
            }
        }
        let sel = buffer::select_for_window(segments, start, end);
        match cutter::cut(&sel, start, end, centre, &out) {
            Ok(c) => {
                let size = std::fs::metadata(&out).map(|m| m.len()).unwrap_or(0);
                clips.push(CutClip { target: t.clone(), path: out, size, lead_ms: c.lead_ms, duration_ms: c.duration_ms });
            }
            Err(e) => skipped.push(format!("R{} {} at {}s: {e}", plan.round, t.kind.as_str(), t.time_s)),
        }
    }
    (clips, skipped)
}

/// What to cut for one round: the plan plus the wanted targets, or `None` when there is nothing.
pub fn plan_clips(raw: &RawRound, mtime_ms: i64, now_ms: i64, settings: &ClipSettings) -> Option<(RoundPlan, Vec<Target>)> {
    let plan = timing::plan_round(raw)?;
    let anchor = timing::choose_anchor(Some(mtime_ms), plan.start_time_ms, now_ms)?;
    log::info!(
        "clips: round {} of {}: anchor {}",
        plan.round,
        plan.match_id,
        match anchor {
            Anchor::RoundEnd(_) => "file time (live round)",
            Anchor::RoundStart(_) => "header start time (old round)",
        }
    );
    let targets: Vec<Target> = timing::targets(&plan, anchor).into_iter().filter(|t| settings.kinds.wants(t.kind)).collect();
    (!targets.is_empty()).then_some((plan, targets))
}

// ---------------------------------------------------------------------------------------------
// Retention

fn remove_clip_file(path: &str) {
    let p = Path::new(path);
    if let Err(e) = std::fs::remove_file(p) {
        if e.kind() != std::io::ErrorKind::NotFound {
            log::warn!("clips: cannot delete {path}: {e}");
        }
    }
    // Drop the match folder when it is empty now.
    if let Some(dir) = p.parent() {
        let _ = std::fs::remove_dir(dir);
    }
}

/// Deletes the clips the policy no longer keeps (files and rows). Returns the deleted count.
pub fn apply_retention(conn: &Connection, settings: &ClipSettings) -> Result<usize, String> {
    let rows = db::all(conn).map_err(|e| e.to_string())?;
    let doomed = retention::plan_deletions(&db::metas(&rows), settings.policy());
    for row in rows.iter().filter(|r| doomed.contains(&r.id)) {
        remove_clip_file(&row.path);
        db::delete(conn, row.id).map_err(|e| e.to_string())?;
    }
    if !doomed.is_empty() {
        log::info!("clips: retention removed {} clip(s)", doomed.len());
    }
    Ok(doomed.len())
}

// ---------------------------------------------------------------------------------------------
// App wiring

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ClipsUpdated {
    pub match_id: String,
    pub added: u32,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ClipsWarning {
    /// `blackFrames`
    pub kind: String,
    pub message: String,
}

pub const BLACK_FRAMES_MESSAGE: &str =
    "The game window looks black. Switch Rainbow Six Siege to borderless or windowed mode (Display settings) so that clips can be recorded.";

/// Tauri state.
pub struct ClipsState {
    settings: Arc<Mutex<ClipSettings>>,
    recorder: recorder::Recorder,
    jobs: Mutex<Sender<PathBuf>>,
    clips_dir: PathBuf,
}

impl ClipsState {
    fn enabled(&self) -> bool {
        self.settings.lock().map(|s| s.enabled).unwrap_or(false)
    }
}

/// Creates the recorder and the cutting thread. Call once at start-up, after the database is open.
pub fn init(app: &AppHandle, initial: ClipSettings, local: &Path) -> tauri::Result<ClipsState> {
    let (buffer_dir, clips_dir) = (local.join("clipbuffer"), local.join("clips"));
    std::fs::create_dir_all(&clips_dir)?;
    // The asset protocol (video playback) may read the clips folder wherever it is: the default
    // scope in tauri.conf.json only knows the regular one, not an `R6COACH_DATA_DIR` folder.
    if let Err(e) = app.asset_protocol_scope().allow_directory(&clips_dir, true) {
        log::warn!("clips: cannot allow the clips folder in the asset scope: {e}");
    }
    if !initial.enabled {
        // Leftovers of an earlier run (a crash while recording).
        buffer::clear(&buffer_dir);
    }
    let settings = Arc::new(Mutex::new(initial));
    let app_events = app.clone();
    let recorder = recorder::Recorder::start(
        buffer_dir,
        settings.clone(),
        recorder::game_matcher(),
        recorder::RecorderConfig::default(),
        Arc::new(move |e| match e {
            recorder::RecEvent::BlackFrames(true) => {
                log::warn!("clips: the capture is black; the game is probably in exclusive fullscreen");
                let _ = app_events.emit(EVENT_CLIPS_WARNING, ClipsWarning { kind: "blackFrames".into(), message: BLACK_FRAMES_MESSAGE.into() });
            }
            recorder::RecEvent::BlackFrames(false) => {}
        }),
    );
    let (tx, rx) = mpsc::channel::<PathBuf>();
    let (app2, settings2, recorder2, clips_dir2) = (app.clone(), settings.clone(), recorder.clone(), clips_dir.clone());
    if let Err(e) = std::thread::Builder::new().name("clip-cutter".into()).spawn(move || worker(app2, rx, settings2, recorder2, clips_dir2)) {
        log::error!("clips: cannot start the cutting thread: {e}");
    }
    Ok(ClipsState { settings, recorder, jobs: Mutex::new(tx), clips_dir })
}

/// Called by the replay watcher with every changed file. Queues the new round files.
pub fn on_replay_files(app: &AppHandle, mut paths: Vec<PathBuf>) {
    let Some(st) = app.try_state::<ClipsState>() else { return };
    if !st.enabled() {
        return;
    }
    paths.retain(|p| p.extension().is_some_and(|e| e.eq_ignore_ascii_case("rec")));
    paths.sort();
    paths.dedup();
    if let Ok(tx) = st.jobs.lock() {
        for p in paths {
            let _ = tx.send(p);
        }
    }
}

fn worker(app: AppHandle, rx: Receiver<PathBuf>, settings: Arc<Mutex<ClipSettings>>, recorder: recorder::Recorder, clips_dir: PathBuf) {
    // Round files already handled, by modification time (a file event can fire more than once).
    let mut seen: HashMap<PathBuf, i64> = HashMap::new();
    for path in rx {
        let s = settings.lock().map(|s| s.clone()).unwrap_or_default();
        if !s.enabled {
            continue;
        }
        match process_round_file(&app, &s, &recorder, &clips_dir, &path, &mut seen) {
            Ok(Some((match_id, added))) => {
                log::info!("clips: {} clip(s) cut for {}", added, path.display());
                let _ = app.emit(EVENT_CLIPS_UPDATED, ClipsUpdated { match_id, added });
            }
            Ok(None) => {}
            Err(e) => log::warn!("clips: {}: {e}", path.display()),
        }
    }
}

fn process_round_file(
    app: &AppHandle,
    settings: &ClipSettings,
    recorder: &recorder::Recorder,
    clips_dir: &Path,
    path: &Path,
    seen: &mut HashMap<PathBuf, i64>,
) -> Result<Option<(String, u32)>, String> {
    let mtime_ms = std::fs::metadata(path)
        .and_then(|m| m.modified())
        .map_err(|e| format!("cannot stat the round file: {e}"))?
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0);
    if seen.get(path) == Some(&mtime_ms) {
        return Ok(None);
    }
    if seen.len() > 2000 {
        seen.clear();
    }
    seen.insert(path.to_path_buf(), mtime_ms);

    let json = crate::dissect_json(&crate::sidecar_path()?, path)?;
    let raw: RawRound = serde_json::from_str(&json).map_err(|e| format!("not a round: {e}"))?;
    let Some((plan, targets)) = plan_clips(&raw, mtime_ms, recorder::now_ms(), settings) else {
        return Ok(None);
    };
    // Close the segment that is still being written, so the end of the round can be cut.
    recorder.flush();
    let (cut, skipped) = recorder.with_buffer(|dir| cut_round(&plan, &targets, settings, &buffer::list_segments(dir), clips_dir));
    for s in &skipped {
        log::info!("clips: skipped: {s}");
    }
    if cut.is_empty() {
        return Ok(None);
    }
    let db = app.state::<store::Db>();
    let conn = lock(&db)?;
    let now = recorder::now_ms();
    for c in &cut {
        let path = c.path.to_string_lossy();
        db::insert(
            &conn,
            &db::NewClip {
                match_id: &plan.match_id,
                round: plan.round,
                kind: c.target.kind.as_str(),
                time_s: c.target.time_s,
                killer: &c.target.killer,
                victim: &c.target.victim,
                path: &path,
                size: c.size,
                start_ms: c.lead_ms,
                duration_ms: c.duration_ms,
                created_ms: now,
            },
        )
        .map_err(|e| format!("cannot save a clip: {e}"))?;
    }
    apply_retention(&conn, settings)?;
    Ok(Some((plan.match_id.clone(), cut.len() as u32)))
}

// ---------------------------------------------------------------------------------------------
// Commands

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ClipsStatus {
    pub recording: recorder::RecStatus,
    pub buffer_bytes: u64,
    pub clips_bytes: u64,
    pub clips_count: u32,
    pub starred_count: u32,
    pub clips_dir: String,
}

#[tauri::command]
pub fn get_clip_settings(state: tauri::State<ClipsState>) -> ClipSettings {
    state.settings.lock().map(|s| s.clone()).unwrap_or_default()
}

#[tauri::command]
pub fn set_clip_settings(state: tauri::State<ClipsState>, db: tauri::State<store::Db>, settings: ClipSettings) -> Cmd<ClipSettings> {
    let settings = settings.normalized();
    let json = serde_json::to_string(&settings).map_err(|e| e.to_string())?;
    let conn = lock(&db)?;
    store::set_setting(&conn, KEY_SETTINGS, &json).map_err(|e| e.to_string())?;
    if let Ok(mut s) = state.settings.lock() {
        *s = settings.clone();
    }
    state.recorder.wake();
    // A smaller "last N matches" or disk cap takes effect now.
    apply_retention(&conn, &settings)?;
    Ok(settings)
}

#[tauri::command]
pub fn clips_status(state: tauri::State<ClipsState>, db: tauri::State<store::Db>) -> Cmd<ClipsStatus> {
    let (count, starred, bytes) = db::totals(&*lock(&db)?).map_err(|e| e.to_string())?;
    Ok(ClipsStatus {
        recording: state.recorder.status(),
        buffer_bytes: buffer::dir_size(&state.recorder.buffer_dir()),
        clips_bytes: bytes,
        clips_count: count,
        starred_count: starred,
        clips_dir: state.clips_dir.to_string_lossy().into_owned(),
    })
}

/// The clips of a match whose files still exist (rows of vanished files are dropped).
#[tauri::command]
pub fn list_clips(db: tauri::State<store::Db>, match_id: String) -> Cmd<Vec<db::ClipRow>> {
    let conn = lock(&db)?;
    let rows = db::for_match(&conn, &match_id).map_err(|e| e.to_string())?;
    let mut alive = Vec::with_capacity(rows.len());
    for r in rows {
        if Path::new(&r.path).is_file() {
            alive.push(r);
        } else {
            let _ = db::delete(&conn, r.id);
        }
    }
    Ok(alive)
}

#[tauri::command]
pub fn set_clip_starred(db: tauri::State<store::Db>, id: i64, starred: bool) -> Cmd<()> {
    db::set_starred(&*lock(&db)?, id, starred).map_err(|e| e.to_string())?;
    Ok(())
}

/// Deletes every clip, starred ones included. Returns how many were deleted.
#[tauri::command]
pub fn delete_all_clips(db: tauri::State<store::Db>) -> Cmd<u32> {
    let conn = lock(&db)?;
    let rows = db::all(&conn).map_err(|e| e.to_string())?;
    for r in &rows {
        remove_clip_file(&r.path);
        db::delete(&conn, r.id).map_err(|e| e.to_string())?;
    }
    log::info!("clips: deleted all {} clip(s)", rows.len());
    Ok(rows.len() as u32)
}

#[tauri::command]
pub fn show_clip_in_folder(db: tauri::State<store::Db>, id: i64) -> Cmd<()> {
    let row = db::get(&*lock(&db)?, id).map_err(|e| e.to_string())?.ok_or("clip not found")?;
    reveal(Path::new(&row.path))
}

#[tauri::command]
pub fn open_clips_folder(state: tauri::State<ClipsState>) -> Cmd<()> {
    std::fs::create_dir_all(&state.clips_dir).map_err(|e| e.to_string())?;
    reveal(&state.clips_dir)
}

/// The bytes of a clip. The in-app player falls back to this (a Blob URL) when the webview cannot
/// stream the file through the asset protocol.
#[tauri::command]
pub async fn read_clip_file(db: tauri::State<'_, store::Db>, id: i64) -> Cmd<tauri::ipc::Response> {
    let path = db::get(&*lock(&db)?, id).map_err(|e| e.to_string())?.ok_or("clip not found")?.path;
    let bytes = tauri::async_runtime::spawn_blocking(move || std::fs::read(&path).map_err(|e| format!("cannot read the clip {path}: {e}")))
        .await
        .map_err(|e| format!("read task failed: {e}"))??;
    Ok(tauri::ipc::Response::new(bytes))
}

/// Plays a clip in the default video player of the PC (last resort when the in-app player fails).
#[tauri::command]
pub fn open_clip_external(db: tauri::State<store::Db>, id: i64) -> Cmd<()> {
    let row = db::get(&*lock(&db)?, id).map_err(|e| e.to_string())?.ok_or("clip not found")?;
    let path = Path::new(&row.path);
    if !path.is_file() {
        return Err("The clip file is gone.".into());
    }
    // Explorer opens a file with its default program; it reports exit code 1 even when it worked.
    std::process::Command::new("explorer.exe")
        .arg(row.path.replace('/', "\\"))
        .spawn()
        .map(|_| ())
        .map_err(|e| format!("cannot open the default player: {e}"))
}

/// Opens Explorer on a folder, or on the folder with a file selected.
fn reveal(path: &Path) -> Cmd<()> {
    let mut cmd = std::process::Command::new("explorer.exe");
    if path.is_file() {
        // `/select,` and the path must be one argument; Explorer wants backslashes.
        cmd.arg(format!("/select,{}", path.display().to_string().replace('/', "\\")));
    } else {
        cmd.arg(path);
    }
    // Explorer reports exit code 1 even when it worked; only a failure to start is an error.
    cmd.spawn().map(|_| ()).map_err(|e| format!("cannot open Explorer: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::clips::cutter::tests::synthetic_segment;
    use timing::{Ev, EvKind};

    fn ev(kind: EvKind, c: i64, killer: &str, victim: &str) -> Ev {
        Ev { kind, counter_s: c, killer: killer.into(), victim: victim.into() }
    }

    /// One minute of "video" per segment is too much for a test; use 30 s pieces back to back.
    fn make_segments(dir: &Path, first_start: i64, pieces: u32) -> Vec<Segment> {
        std::fs::create_dir_all(dir).unwrap();
        (0..pieces)
            .map(|k| {
                let start = first_start + i64::from(k) * 30_000;
                let path = dir.join(buffer::segment_name(start, start + 30_000));
                synthetic_segment(&path, k as u8, 30, 900, 30, 0xab);
                Segment { start_ms: start, end_ms: start + 30_000, path }
            })
            .collect()
    }

    /// The settings the timing tests were written for: a 6 s + 2 s window right at the kill.
    fn classic(kinds: ClipKinds) -> ClipSettings {
        ClipSettings { kinds, offset_s: 0.0, before_s: 6.0, after_s: 2.0, ..Default::default() }
    }

    fn plan() -> RoundPlan {
        RoundPlan {
            match_id: "match/1".into(),
            round: 3,
            start_time_ms: 0,
            me_username: "me".into(),
            events: vec![ev(EvKind::Kill, 120, "me", "a"), ev(EvKind::Kill, 60, "b", "me"), ev(EvKind::Kill, 30, "c", "d")],
            ended_on_last_event: true,
        }
    }

    #[test]
    fn cuts_kills_and_deaths_around_the_synthetic_round_times() {
        let t = tempfile::tempdir().unwrap();
        let round_end = 5_000_000i64;
        // 150 s of recording ending at the round end.
        let segs = make_segments(&t.path().join("buf"), round_end - 150_000, 5);
        let clips_dir = t.path().join("clips");
        let settings = classic(ClipKinds::Both);
        let targets = timing::targets(&plan(), Anchor::RoundEnd(round_end));
        assert_eq!(targets.len(), 2);
        // Kill at 120 -> 90 s before the end; death at 60 -> 30 s before the end.
        assert_eq!(targets[0].wall_ms, round_end - 90_000);
        assert_eq!(targets[1].wall_ms, round_end - 30_000);

        let (clips, skipped) = cut_round(&plan(), &targets, &settings, &segs, &clips_dir);
        assert!(skipped.is_empty(), "{skipped:?}");
        assert_eq!(clips.len(), 2);
        let kill = &clips[0];
        assert!(kill.path.ends_with("match_1/R03-kill-120-0.mp4"), "{}", kill.path.display());
        assert!(kill.size > 0 && kill.path.is_file());
        // Window 6 s before .. 2 s after; the file starts at the keyframe at or before the window
        // start (one keyframe per second), so the lead-in is below one second.
        assert!(kill.lead_ms < 1000, "lead {}", kill.lead_ms);
        assert!((8_000..9_100).contains(&kill.duration_ms), "duration {}", kill.duration_ms);
        assert_eq!(clips[1].target.kind, TargetKind::Death);
    }

    #[test]
    fn the_offset_setting_moves_the_window() {
        let t = tempfile::tempdir().unwrap();
        let round_end = 6_000_000i64;
        let segs = make_segments(&t.path().join("buf"), round_end - 150_000, 5);
        let targets = timing::targets(&plan(), Anchor::RoundEnd(round_end));
        let plain = classic(ClipKinds::Kills);
        let shifted = ClipSettings { offset_s: 4.0, ..plain.clone() };
        let (a, _) = cut_round(&plan(), &targets[..1], &plain, &segs, &t.path().join("a"));
        let (b, _) = cut_round(&plan(), &targets[..1], &shifted, &segs, &t.path().join("b"));
        // Same length, but the shifted clip starts 4 s later in the recording, so its lead-in
        // inside the (keyframe aligned) file differs by the rest of those 4 s.
        assert_eq!(a.len() + b.len(), 2);
        assert!((a[0].lead_ms - b[0].lead_ms).abs() < 1000);
    }

    #[test]
    fn a_window_that_was_not_recorded_is_skipped() {
        let t = tempfile::tempdir().unwrap();
        let round_end = 7_000_000i64;
        // Only the last 20 s were recorded (the recording started late in the round).
        let dir = t.path().join("buf");
        std::fs::create_dir_all(&dir).unwrap();
        let start = round_end - 20_000;
        let p = dir.join(buffer::segment_name(start, round_end));
        synthetic_segment(&p, 9, 30, 600, 30, 0xab);
        let segs = vec![Segment { start_ms: start, end_ms: round_end, path: p }];
        let targets = timing::targets(&plan(), Anchor::RoundEnd(round_end));
        let settings = classic(ClipKinds::Both);
        let (clips, skipped) = cut_round(&plan(), &targets, &settings, &segs, &t.path().join("clips"));
        assert!(clips.is_empty());
        assert_eq!(skipped.len(), 2);
    }

    #[test]
    fn plan_clips_follows_the_kind_setting() {
        let raw: RawRound = serde_json::from_str(
            r#"{"recordingProfileID":"p1","matchID":"M","roundNumber":0,"startTime":1000,
            "teams":[{"role":"Attack"},{"role":"Defense"}],
            "players":[{"profileID":"p1","username":"me","teamIndex":0},{"profileID":"p2","username":"foe","teamIndex":1}],
            "matchFeedback":[
              {"type":{"name":"Kill"},"username":"me","target":"foe","timeInSeconds":100},
              {"type":{"name":"Kill"},"username":"foe","target":"me","timeInSeconds":50}]}"#,
        )
        .unwrap();
        let now = 10_000_000;
        let mtime = now - 5_000;
        let all = |kinds| ClipSettings { kinds, ..Default::default() };
        // startTime 1000 is far from the file time -> header anchor (old round); still cut.
        let (_, t) = plan_clips(&raw, mtime, now, &all(ClipKinds::Both)).unwrap();
        assert_eq!(t.len(), 2);
        let (_, t) = plan_clips(&raw, mtime, now, &all(ClipKinds::Kills)).unwrap();
        assert_eq!((t.len(), t[0].kind), (1, TargetKind::Kill));
        let (_, t) = plan_clips(&raw, mtime, now, &all(ClipKinds::Deaths)).unwrap();
        assert_eq!((t.len(), t[0].kind), (1, TargetKind::Death));
    }

    /// Needs the real replays and the built sidecar: `cargo test -p r6koc-app real_rounds -- --ignored --nocapture`.
    /// Every round file must give exactly the recording player's kills and deaths from the stats.
    #[test]
    #[ignore]
    fn real_rounds_give_the_players_kills_and_deaths() {
        let sidecar = Path::new(env!("CARGO_MANIFEST_DIR")).join("binaries/r6-dissect-x86_64-pc-windows-msvc.exe");
        let dir = Path::new("C:/Users/Kutbay/Desktop/R6Koc-veri/MatchReplay");
        let (mut rounds, mut kills, mut deaths, mut with_start, mut mismatches) = (0, 0, 0, 0, 0);
        for m in std::fs::read_dir(dir).unwrap().flatten().filter(|e| e.path().is_dir()) {
            for f in std::fs::read_dir(m.path()).unwrap().flatten().filter(|e| e.path().extension().is_some_and(|x| x == "rec")) {
                let json = crate::dissect_json(&sidecar, &f.path()).unwrap();
                let raw: RawRound = serde_json::from_str(&json).unwrap();
                rounds += 1;
                with_start += (raw.start_time > 0) as u32;
                // Pretend the file was just written: a copied file is not live, so use a fresh time.
                let now = raw.start_time + 240_000;
                let Some(plan) = timing::plan_round(&raw) else { continue };
                let anchor = timing::choose_anchor(Some(now - 1000), raw.start_time, now).unwrap();
                assert!(matches!(anchor, Anchor::RoundEnd(_)));
                let t = timing::targets(&plan, anchor);
                let (k, d) = (t.iter().filter(|x| x.kind == TargetKind::Kill).count() as u32, t.iter().filter(|x| x.kind == TargetKind::Death).count() as u32);
                let stats = raw.stats.iter().find(|s| s.username == plan.me_username);
                let (sk, sd) = stats.map_or((0, 0), |s| (s.kills, s.died as u32));
                if (k, d) != (sk, sd) {
                    mismatches += 1;
                    println!("{}: feed {k}/{d} vs stats {sk}/{sd}", f.path().display());
                }
                kills += k;
                deaths += d;
                // Walls are ordered like the events (earlier events earlier on the wall clock) within a timer.
                assert!(t.windows(2).all(|w| w[0].wall_ms <= w[1].wall_ms || w[0].time_s < w[1].time_s));
            }
        }
        println!("{rounds} rounds ({with_start} with startTime): {kills} kills, {deaths} deaths, {mismatches} round(s) differ from the stats");
        assert!(rounds > 0 && with_start == rounds);
    }

    #[test]
    fn settings_are_clamped_and_default_to_off() {
        let d = ClipSettings::default();
        assert!(!d.enabled && d.record_audio);
        // New installs: 10 s before, 0 s after, offset -5 s.
        assert_eq!((d.keep_matches, d.before_s, d.after_s, d.offset_s), (10, 10.0, 0.0, -5.0));
        let wild = ClipSettings { keep_matches: 99999, disk_cap_gb: f64::NAN, offset_s: 500.0, before_s: 0.0, after_s: 99.0, ..Default::default() }.normalized();
        assert_eq!((wild.keep_matches, wild.disk_cap_gb, wild.offset_s, wild.before_s, wild.after_s), (1000, 0.0, 30.0, 2.0, 10.0));
        let nan = ClipSettings { offset_s: f64::NAN, before_s: f64::NAN, after_s: f64::NAN, ..Default::default() }.normalized();
        assert_eq!((nan.offset_s, nan.before_s, nan.after_s), (-5.0, 10.0, 0.0));
        let parsed: ClipSettings = serde_json::from_str(r#"{"enabled":true,"kinds":"both","keepMatches":5}"#).unwrap();
        assert!(parsed.enabled && parsed.kinds == ClipKinds::Both && parsed.keep_matches == 5 && parsed.before_s == 10.0 && parsed.record_audio);
    }

    /// A user who already tuned the window (10 s before, 0 s after, offset -5) keeps it: their
    /// stored settings carry all three values, so the new defaults never touch them. Settings saved
    /// before the sound option existed get it switched on.
    #[test]
    fn stored_settings_keep_their_values_and_old_ones_gain_the_sound_option() {
        let conn = store::open_in_memory().unwrap();
        let old = r#"{"enabled":true,"kinds":"kills","keepMatches":10,"diskCapGb":0.0,"offsetS":-5.0,"beforeS":10.0,"afterS":0.0}"#;
        store::set_setting(&conn, KEY_SETTINGS, old).unwrap();
        let s = load_settings(&conn);
        assert_eq!((s.before_s, s.after_s, s.offset_s, s.record_audio), (10.0, 0.0, -5.0, true));
    }

    #[test]
    fn settings_round_trip_through_the_database() {
        let conn = store::open_in_memory().unwrap();
        assert_eq!(load_settings(&conn), ClipSettings::default());
        let s = ClipSettings { enabled: true, keep_matches: 20, ..Default::default() };
        store::set_setting(&conn, KEY_SETTINGS, &serde_json::to_string(&s).unwrap()).unwrap();
        assert_eq!(load_settings(&conn), s);
        store::set_setting(&conn, KEY_SETTINGS, "not json").unwrap();
        assert_eq!(load_settings(&conn), ClipSettings::default());
    }

    #[test]
    fn retention_deletes_files_and_rows_but_keeps_starred() {
        let t = tempfile::tempdir().unwrap();
        let conn = store::open_in_memory().unwrap();
        let mut ids = Vec::new();
        for (i, m) in ["m1", "m2", "m3"].iter().enumerate() {
            let dir = t.path().join(m);
            std::fs::create_dir_all(&dir).unwrap();
            let file = dir.join("c.mp4");
            std::fs::write(&file, b"clip").unwrap();
            let path = file.to_string_lossy().into_owned();
            ids.push(
                db::insert(
                    &conn,
                    &db::NewClip { match_id: m, round: 1, kind: "kill", time_s: 100, killer: "me", victim: "x", path: &path, size: 4, start_ms: 0, duration_ms: 1, created_ms: 1000 + i as i64 },
                )
                .unwrap(),
            );
        }
        db::set_starred(&conn, ids[0], true).unwrap();
        let settings = ClipSettings { keep_matches: 1, ..Default::default() };
        assert_eq!(apply_retention(&conn, &settings).unwrap(), 1);
        let left: Vec<String> = db::all(&conn).unwrap().into_iter().map(|c| c.match_id).collect();
        assert_eq!(left, vec!["m1".to_string(), "m3".to_string()]);
        assert!(!t.path().join("m2").exists());
        assert!(t.path().join("m1").join("c.mp4").is_file());
    }

    /// Makes a data folder for a real-app run: records the self-minimizing test window (with sound),
    /// cuts clips at fake kill times and puts them into a copy of the sample database, so that
    /// `R6COACH_DATA_DIR=<folder>` shows playable clips in the match view. Needs a desktop session.
    ///
    /// ```text
    /// $env:R6K_FIXTURE_DIR = "<folder>"; $env:R6K_SAMPLE_DB = "<copy of r6koc-sample.db>"
    /// cargo test -p r6koc-app --lib fixture_recorded_clips -- --ignored --nocapture
    /// ```
    #[cfg(windows)]
    #[test]
    #[ignore]
    fn fixture_recorded_clips() {
        use super::recorder::tests::spawn_test_window;
        use std::time::Duration;

        let dir = PathBuf::from(std::env::var("R6K_FIXTURE_DIR").expect("set R6K_FIXTURE_DIR to the folder the app will use"));
        std::fs::create_dir_all(&dir).unwrap();
        let db_path = dir.join("r6koc.db");
        if let Ok(sample) = std::env::var("R6K_SAMPLE_DB") {
            std::fs::copy(&sample, &db_path).expect("copy the sample database");
        }
        let conn = store::open(&db_path).unwrap();
        // The app run must not look at the real replay folder or at Windows credentials.
        let replays = dir.join("replays");
        std::fs::create_dir_all(&replays).unwrap();
        store::set_setting(&conn, store::KEY_REPLAY_DIR, &replays.to_string_lossy()).unwrap();
        store::set_setting(&conn, "arenyze_legacy_cleaned", "1").unwrap();

        // Real kills of the tracked player: the clips must match kill-feed rows to show up.
        let mut kills: Vec<(String, u32, i64, String, String)> = Vec::new();
        for m in store::load_full_matches(&conn).unwrap().iter().rev() {
            for r in &m.rounds {
                for k in r.kills.iter().filter(|k| k.killer == m.me_profile_id) {
                    let name = |id: &str| m.player(id).map(|p| p.username.clone()).unwrap_or_else(|| id.to_string());
                    kills.push((m.match_id.clone(), r.number, k.time_s, name(&k.killer), name(&k.victim)));
                }
            }
            if kills.len() >= 4 {
                break;
            }
            kills.clear();
        }
        assert!(kills.len() >= 4, "the sample database needs a match with four kills of the tracked player");

        let work = dir.join("rawbuffer");
        let _ = std::fs::remove_dir_all(&work);
        std::fs::create_dir_all(&work).unwrap();
        let (mut child, t0) = spawn_test_window(&dir, 9, 6, true);
        let settings = Arc::new(Mutex::new(ClipSettings { enabled: true, record_audio: true, ..Default::default() }));
        let rec = recorder::Recorder::start(
            work.clone(),
            settings,
            Box::new(|w| w.title == "R6K-CLIP-TEST"),
            recorder::RecorderConfig { segment_secs: 12, poll: Duration::from_millis(500), fps: std::env::var("R6K_FPS").ok().and_then(|v| v.parse().ok()).unwrap_or(recorder::CLIP_FPS) },
            Arc::new(|e| println!("event: {e:?}")),
        );
        std::thread::sleep(Duration::from_secs(34));
        let status = rec.status();
        println!("status: {status:?}");
        rec.flush();
        let segs = rec.with_buffer(|d| buffer::list_segments(d));
        for s in &segs {
            println!("segment {:.2}..{:.2} s  {}", (s.start_ms - t0) as f64 / 1000.0, (s.end_ms - t0) as f64 / 1000.0, s.path.display());
        }

        // Kills at fake times: before the minimized stretch (9..15 s), inside it, and after it.
        let offsets_s = [5.0f64, 12.0, 17.5, 26.0];
        let clips_settings = ClipSettings { kinds: ClipKinds::Kills, before_s: 2.0, after_s: 2.0, offset_s: 0.0, ..Default::default() };
        let clips_dir = dir.join("clips");
        let mut expected = Vec::new();
        for ((match_id, round, time_s, killer, victim), off) in kills.iter().zip(offsets_s) {
            let plan = RoundPlan { match_id: match_id.clone(), round: *round, start_time_ms: 0, me_username: killer.clone(), events: vec![], ended_on_last_event: true };
            let target = timing::Target { kind: TargetKind::Kill, time_s: *time_s, killer: killer.clone(), victim: victim.clone(), wall_ms: t0 + (off * 1000.0) as i64 };
            let (cut, skipped) = cut_round(&plan, &[target.clone()], &clips_settings, &segs, &clips_dir);
            println!("kill at {off} s: {} clip(s), skipped {skipped:?}", cut.len());
            for c in cut {
                let path = c.path.to_string_lossy().into_owned();
                db::insert(
                    &conn,
                    &db::NewClip {
                        match_id,
                        round: *round,
                        kind: "kill",
                        time_s: *time_s,
                        killer,
                        victim,
                        path: &path,
                        size: c.size,
                        start_ms: c.lead_ms,
                        duration_ms: c.duration_ms,
                        created_ms: recorder::now_ms(),
                    },
                )
                .unwrap();
                let origin = target.wall_ms - 2000 - c.lead_ms;
                expected.push(serde_json::json!({
                    "matchId": match_id, "round": round, "timeS": time_s, "path": path,
                    "killAtElapsedS": off, "originElapsedS": (origin - t0) as f64 / 1000.0, "originWallMs": origin,
                    "leadMs": c.lead_ms, "durationMs": c.duration_ms
                }));
            }
        }
        std::fs::write(dir.join("expected.json"), serde_json::to_string_pretty(&expected).unwrap()).unwrap();
        rec.shutdown();
        let _ = child.kill();
        let _ = child.wait();
        assert!(expected.len() >= 2, "expected clips before and after the minimized stretch");
    }
}
