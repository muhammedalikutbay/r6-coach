mod arenyze;
mod clips;
mod report;
mod store;
mod summary;
mod tables;
mod v2;
mod watcher;

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::Mutex;
use tauri::{Emitter, Manager};

/// Tauri event sent after every import (manual or automatic), payload `store::RefreshResult`.
pub const EVENT_REPLAYS_UPDATED: &str = "replays-updated";

#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

/// Resolves the bundled r6-dissect sidecar. Tauri places `externalBin` files
/// next to the app executable (without the target-triple suffix), in dev and release alike.
fn sidecar_path() -> Result<PathBuf, String> {
    let exe = std::env::current_exe().map_err(|e| format!("cannot locate app exe: {e}"))?;
    let dir = exe.parent().ok_or("app exe has no parent folder")?;
    let name = if cfg!(windows) { "r6-dissect.exe" } else { "r6-dissect" };
    let path = dir.join(name);
    if path.is_file() {
        Ok(path)
    } else {
        Err(format!("sidecar not found: {}", path.display()))
    }
}

/// Runs the given r6-dissect executable on a match folder and returns its JSON output.
fn dissect_json(sidecar: &Path, path: &Path) -> Result<String, String> {
    log::info!("parse: {} {}", sidecar.display(), path.display());
    let mut cmd = Command::new(sidecar);
    cmd.arg(path)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    let out = cmd.output().map_err(|e| format!("cannot start r6-dissect: {e}"))?;
    if !out.status.success() {
        let err = String::from_utf8_lossy(&out.stderr);
        return Err(format!("r6-dissect failed ({}): {}", out.status, err.trim()));
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

fn dissect_match(sidecar: &Path, path: &Path) -> Result<r6k_core::Match, String> {
    let json = dissect_json(sidecar, path)?;
    r6k_core::match_from_json(&json).map_err(|e| format!("cannot convert match: {e}"))
}

fn run_parse(path: &str) -> Result<summary::MatchSummary, String> {
    let m = dissect_match(&sidecar_path()?, Path::new(path))?;
    Ok(summary::MatchSummary::from(&m))
}

#[tauri::command]
async fn parse_match(path: String) -> Result<summary::MatchSummary, String> {
    // Parsing is CPU/IO heavy; keep it off the main thread.
    tauri::async_runtime::spawn_blocking(move || run_parse(&path))
        .await
        .map_err(|e| format!("parse task failed: {e}"))?
}

type Cmd<T> = Result<T, String>;

fn lock(db: &store::Db) -> Cmd<std::sync::MutexGuard<'_, rusqlite::Connection>> {
    db.0.lock().map_err(|_| "database lock poisoned".to_string())
}

/// Saved replay folder, else the auto-detected one (which is then saved), else empty.
fn replay_dir_of(db: &store::Db) -> Cmd<String> {
    let conn = lock(db)?;
    let saved = store::get_setting(&conn, store::KEY_REPLAY_DIR).map_err(|e| e.to_string())?;
    if let Some(s) = saved.filter(|s| !s.trim().is_empty()) {
        return Ok(s);
    }
    match r6k_paths::find_replay_dir() {
        Some(p) => {
            let found = p.to_string_lossy().into_owned();
            log::info!("replay folder auto-detected: {found}");
            store::set_setting(&conn, store::KEY_REPLAY_DIR, &found).map_err(|e| e.to_string())?;
            Ok(found)
        }
        None => Ok(String::new()),
    }
}

/// State of a replay folder, as shown on the Settings page.
#[derive(serde::Serialize)]
#[serde(tag = "status", rename_all = "camelCase")]
enum ReplayDirInfo {
    Missing,
    Empty,
    #[serde(rename_all = "camelCase")]
    Ready { match_count: usize },
}

#[tauri::command]
fn inspect_replay_dir(path: String) -> ReplayDirInfo {
    use r6k_paths::ReplayDirStatus as S;
    match r6k_paths::inspect(Path::new(path.trim())) {
        S::Missing => ReplayDirInfo::Missing,
        S::Empty => ReplayDirInfo::Empty,
        S::Ready { match_count } => ReplayDirInfo::Ready { match_count },
    }
}

/// Only one import runs at a time (Update button and the watcher share it).
static REFRESH_GATE: Mutex<()> = Mutex::new(());

/// Imports the replay folder and emits `replays-updated`. Blocks while another import runs.
pub(crate) fn run_refresh(app: &tauri::AppHandle) -> Cmd<store::RefreshResult> {
    let _gate = REFRESH_GATE.lock().unwrap_or_else(|e| e.into_inner());
    let db = app.state::<store::Db>();
    let dir = replay_dir_of(&db)?;
    if dir.is_empty() {
        return Err("Replay folder is not set.".to_string());
    }
    let sidecar = sidecar_path()?;
    let result = store::refresh(&db, Path::new(&dir), |p| dissect_json(&sidecar, p))?;
    if let Err(e) = app.emit(EVENT_REPLAYS_UPDATED, &result) {
        log::warn!("cannot emit {EVENT_REPLAYS_UPDATED}: {e}");
    }
    Ok(result)
}

#[tauri::command]
async fn refresh(app: tauri::AppHandle) -> Cmd<store::RefreshResult> {
    tauri::async_runtime::spawn_blocking(move || {
        let result = run_refresh(&app)?;
        if result.pending > 0 || result.in_progress > 0 {
            app.state::<watcher::Watcher>().retry_later();
        }
        Ok(result)
    })
    .await
    .map_err(|e| format!("refresh task failed: {e}"))?
}

fn baseline() -> Cmd<r6k_core::Baseline> {
    Ok(r6k_core::Baseline::default()) // no external baseline since v3
}

#[tauri::command]
fn operator_table(db: tauri::State<store::Db>) -> Cmd<Vec<tables::OperatorRow>> {
    let matches = store::load_matches(&*lock(&db)?).map_err(|e| e.to_string())?;
    Ok(tables::operator_table(&baseline()?, &matches))
}

#[tauri::command]
fn map_table(db: tauri::State<store::Db>) -> Cmd<Vec<tables::MapRow>> {
    let matches = store::load_matches(&*lock(&db)?).map_err(|e| e.to_string())?;
    Ok(tables::map_table(&baseline()?, &matches))
}

#[tauri::command]
fn get_replay_dir(db: tauri::State<store::Db>) -> Cmd<String> {
    replay_dir_of(&db)
}

#[tauri::command]
fn set_replay_dir(
    db: tauri::State<store::Db>,
    watcher: tauri::State<watcher::Watcher>,
    path: String,
) -> Cmd<()> {
    store::set_setting(&*lock(&db)?, store::KEY_REPLAY_DIR, path.trim()).map_err(|e| e.to_string())?;
    watcher.rewatch();
    Ok(())
}

#[tauri::command]
fn reset_database(db: tauri::State<store::Db>) -> Cmd<()> {
    store::reset(&*lock(&db)?).map_err(|e| e.to_string())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    if let Err(e) = tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            let dir = app.path().app_data_dir()?;
            std::fs::create_dir_all(&dir)?;
            let conn = store::open(&dir.join("r6koc.db"))?;
            // Remove what the old Ubisoft sign-in left behind (once).
            arenyze::cleanup_legacy(&conn);
            let clip_settings = clips::load_settings(&conn);
            app.manage(store::Db(Mutex::new(conn)));
            // Kill clips (v4): off by default; the recorder thread idles until the user turns it on.
            app.manage(clips::init(app.handle(), clip_settings)?);
            let w = watcher::start(app.handle());
            app.manage(w);
            // Arenyze state; no network call until the user has entered a key and asks for rank.
            app.manage(arenyze::ArenyzeState::new());
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            parse_match,
            refresh,
            operator_table,
            map_table,
            get_replay_dir,
            set_replay_dir,
            inspect_replay_dir,
            reset_database,
            report::last_match_report,
            report::match_report,
            v2::query,
            v2::list_matches,
            v2::match_detail,
            v2::coach,
            arenyze::arenyze_status,
            arenyze::arenyze_set_key,
            arenyze::arenyze_clear_key,
            arenyze::online_profile,
            arenyze::arenyze_save_last_response,
            arenyze::arenyze_open_key_page,
            clips::get_clip_settings,
            clips::set_clip_settings,
            clips::clips_status,
            clips::list_clips,
            clips::set_clip_starred,
            clips::delete_all_clips,
            clips::show_clip_in_folder,
            clips::open_clips_folder
        ])
        .run(tauri::generate_context!())
    {
        eprintln!("error while running tauri application: {e}");
    }
}

#[cfg(test)]
mod real_data {
    use super::*;

    /// Needs the real replays and the built sidecar: `cargo test -- --ignored --nocapture`.
    #[test]
    #[ignore]
    fn refresh_real_replays() {
        let sidecar = Path::new(env!("CARGO_MANIFEST_DIR")).join("binaries/r6-dissect-x86_64-pc-windows-msvc.exe");
        let dir = Path::new(r"C:\Users\Kutbay\Desktop\R6Koc-veri\MatchReplay");
        let t = tempfile::tempdir().unwrap();
        let db = store::Db(std::sync::Mutex::new(store::open(&t.path().join("r6koc.db")).unwrap()));
        let parse = |p: &Path| dissect_json(&sidecar, p);
        let first = store::refresh(&db, dir, parse).unwrap();
        println!("first: {} new, failed {:?}", first.new_matches, first.failed);
        assert_eq!(first.new_matches, 30);
        let second = store::refresh(&db, dir, parse).unwrap();
        assert_eq!(second.new_matches, 0);
        // The full model survives the database round trip: 30 matches, and the tracked
        // player's Outback numbers (5 K / 4 D).
        let full = store::load_full_matches(&db.0.lock().unwrap()).unwrap();
        assert_eq!(full.len(), 30);
        let outback: Vec<_> = full
            .iter()
            .filter(|m| r6k_core::names::map_name(m.map_id) == "Outback")
            .collect();
        let (mut k, mut d) = (0, 0);
        for m in &outback {
            let me = m.player(&m.me_profile_id).expect("me in player list");
            assert_eq!(me.username, "H.A.N-01");
            for r in &m.rounds {
                for p in r.players.iter().filter(|p| p.profile_id == m.me_profile_id) {
                    k += p.kills;
                    d += p.deaths;
                }
            }
            assert!(m.rounds.iter().all(|r| !r.kills.is_empty() || r.players.iter().all(|p| p.kills == 0)));
        }
        println!("Outback: {} match(es), {k} K / {d} D", outback.len());
        assert_eq!((k, d), (5, 4));
        let matches = store::load_matches(&db.0.lock().unwrap()).unwrap();
        let b = r6k_core::Baseline::default();
        let ops = tables::operator_table(&b, &matches);
        let maps = tables::map_table(&b, &matches);
        for o in ops.iter().take(5) {
            println!("{o:?}");
        }
        for m in maps.iter().take(3) {
            println!("{m:?}");
        }
    }
}
