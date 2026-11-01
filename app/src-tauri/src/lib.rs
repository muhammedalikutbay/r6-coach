mod summary;

use std::path::PathBuf;
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

/// Runs r6-dissect on a match folder and converts the output to the frontend summary.
fn run_parse(path: &str) -> Result<summary::MatchSummary, String> {
    let sidecar = sidecar_path()?;
    log::info!("parse_match: {} {}", sidecar.display(), path);
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
    let m = r6k_core::match_from_json(&json).map_err(|e| format!("cannot convert match: {e}"))?;
    Ok(summary::MatchSummary::from(&m))
}

#[tauri::command]
async fn parse_match(path: String) -> Result<summary::MatchSummary, String> {
    // Parsing is CPU/IO heavy; keep it off the main thread.
    tauri::async_runtime::spawn_blocking(move || run_parse(&path))
        .await
        .map_err(|e| format!("parse task failed: {e}"))?
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    if let Err(e) = tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![parse_match])
        .run(tauri::generate_context!())
    {
        eprintln!("error while running tauri application: {e}");
    }
}
