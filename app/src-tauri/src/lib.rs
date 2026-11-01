mod store;
mod summary;
mod tables;

use std::path::{Path, PathBuf};
use tauri::Manager;
use std::process::{Command, Stdio};

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

/// Runs the given r6-dissect executable on a match folder and converts the output.
fn dissect_match(sidecar: &Path, path: &Path) -> Result<r6k_core::Match, String> {
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
    let json = String::from_utf8_lossy(&out.stdout);
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

/// Saved replay folder, else the auto-detected one, else empty.
fn replay_dir_of(db: &store::Db) -> Cmd<String> {
    let saved = store::get_setting(&*lock(db)?, store::KEY_REPLAY_DIR).map_err(|e| e.to_string())?;
    Ok(saved
        .filter(|s| !s.trim().is_empty())
        .or_else(|| r6k_paths::find_replay_dir().map(|p| p.to_string_lossy().into_owned()))
        .unwrap_or_default())
}

#[tauri::command]
async fn refresh(app: tauri::AppHandle) -> Cmd<store::RefreshResult> {
    tauri::async_runtime::spawn_blocking(move || {
        let db = app.state::<store::Db>();
        let dir = replay_dir_of(&db)?;
        if dir.is_empty() {
            return Err("Replay folder is not set.".to_string());
        }
        let sidecar = sidecar_path()?;
        store::refresh(&db, Path::new(&dir), |p| dissect_match(&sidecar, p))
    })
    .await
    .map_err(|e| format!("refresh task failed: {e}"))?
}

fn baseline() -> Cmd<r6k_core::Baseline> {
    r6k_core::Baseline::embedded().map_err(|e| format!("baseline is invalid: {e}"))
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
fn set_replay_dir(db: tauri::State<store::Db>, path: String) -> Cmd<()> {
    store::set_setting(&*lock(&db)?, store::KEY_REPLAY_DIR, path.trim()).map_err(|e| e.to_string())
}

#[tauri::command]
fn reset_database(db: tauri::State<store::Db>) -> Cmd<()> {
    store::reset(&*lock(&db)?).map_err(|e| e.to_string())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    if let Err(e) = tauri::Builder::default()
        .setup(|app| {
            let dir = app.path().app_data_dir()?;
            std::fs::create_dir_all(&dir)?;
            let conn = store::open(&dir.join("r6koc.db"))?;
            app.manage(store::Db(std::sync::Mutex::new(conn)));
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            parse_match,
            refresh,
            operator_table,
            map_table,
            get_replay_dir,
            set_replay_dir,
            reset_database
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
        let parse = |p: &Path| dissect_match(&sidecar, p);
        let first = store::refresh(&db, dir, parse).unwrap();
        println!("first: {} new, failed {:?}", first.new_matches, first.failed);
        assert_eq!(first.new_matches, 30);
        let second = store::refresh(&db, dir, parse).unwrap();
        assert_eq!(second.new_matches, 0);
        let matches = store::load_matches(&db.0.lock().unwrap()).unwrap();
        let b = r6k_core::Baseline::embedded().unwrap();
        let ops = tables::operator_table(&b, &matches);
        let maps = tables::map_table(&b, &matches);
        for o in ops.iter().take(5) {
            println!("{o:?}");
        }
        for m in maps.iter().take(3) {
            println!("{m:?}");
        }
        assert_eq!(ops[0].rounds, b.top_operators(1)[0].1.rounds_played);
    }
}
