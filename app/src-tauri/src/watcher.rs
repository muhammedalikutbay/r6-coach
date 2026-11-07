//! Auto refresh (v3 spec): watches the replay folder recursively, debounces changes for 5 s and
//! runs the same import as the Update button on a background thread. While a match is still
//! being recorded (pending / in progress) it tries again by itself after a while, even when no
//! file changes.

use notify_debouncer_mini::notify::{RecommendedWatcher, RecursiveMode};
use notify_debouncer_mini::{new_debouncer, DebounceEventResult, Debouncer};
use std::path::Path;
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::time::{Duration, Instant};
use tauri::{AppHandle, Manager};

use crate::store;

/// Quiet time after the last file event before an import starts.
pub const DEBOUNCE: Duration = Duration::from_secs(5);
/// Extra import after a refresh that left pending or in-progress matches (60-90 s window).
pub const RETRY_AFTER: Duration = Duration::from_secs(75);

enum Msg {
    /// Re-read the replay folder setting, watch it again, then refresh.
    Rewatch,
    /// Run an import now.
    Refresh,
    /// A manual import left unfinished work: make sure a retry is scheduled.
    RetryLater,
}

/// Tauri state: the handle to the watcher thread.
pub struct Watcher(Sender<Msg>);

impl Watcher {
    /// Call after the replay folder setting changed.
    pub fn rewatch(&self) {
        self.send(Msg::Rewatch);
    }

    /// Call after a manual import that left pending or in-progress matches.
    pub fn retry_later(&self) {
        self.send(Msg::RetryLater);
    }

    fn send(&self, msg: Msg) {
        if self.0.send(msg).is_err() {
            log::warn!("replay watcher thread is not running");
        }
    }
}

/// Starts the watcher thread. It watches the current replay folder and runs one import right
/// away (picks up matches played while the app was closed, and the one-time v4 re-import).
pub fn start(app: &AppHandle) -> Watcher {
    let (tx, rx) = mpsc::channel();
    let (app2, tx2) = (app.clone(), tx.clone());
    if let Err(e) = std::thread::Builder::new()
        .name("replay-watcher".into())
        .spawn(move || worker(app2, tx2, rx))
    {
        log::error!("cannot start the replay watcher: {e}");
    }
    let w = Watcher(tx);
    w.rewatch();
    w
}

fn worker(app: AppHandle, tx: Sender<Msg>, rx: Receiver<Msg>) {
    // Dropping the debouncer stops the old watch.
    let mut debouncer: Option<Debouncer<RecommendedWatcher>> = None;
    let mut retry_at: Option<Instant> = None;
    loop {
        let msg = match retry_at {
            Some(at) => match rx.recv_timeout(at.saturating_duration_since(Instant::now())) {
                Ok(m) => m,
                Err(RecvTimeoutError::Timeout) => {
                    log::info!("auto refresh: retrying unfinished matches");
                    Msg::Refresh
                }
                Err(RecvTimeoutError::Disconnected) => return,
            },
            None => match rx.recv() {
                Ok(m) => m,
                Err(_) => return,
            },
        };
        if let Msg::RetryLater = msg {
            retry_at.get_or_insert_with(|| Instant::now() + RETRY_AFTER);
            continue;
        }
        // Coalesce whatever queued up meanwhile into this one run.
        let mut rewatch = matches!(msg, Msg::Rewatch);
        while let Ok(m) = rx.try_recv() {
            rewatch |= matches!(m, Msg::Rewatch);
        }
        if rewatch {
            // Stop the old watch before starting the new one.
            drop(debouncer.take());
            debouncer = watch(&app, &tx);
        }
        retry_at = match crate::run_refresh(&app) {
            Ok(r) if r.pending > 0 || r.in_progress > 0 => Some(Instant::now() + RETRY_AFTER),
            Ok(_) => None,
            Err(e) => {
                log::warn!("auto refresh: {e}");
                None
            }
        };
    }
}

/// Watches the saved replay folder. `None` when it is not set or does not exist (logged).
fn watch(app: &AppHandle, tx: &Sender<Msg>) -> Option<Debouncer<RecommendedWatcher>> {
    let dir = match crate::replay_dir_of(&app.state::<store::Db>()) {
        Ok(d) if !d.trim().is_empty() => d,
        Ok(_) => {
            log::info!("replay watcher: no replay folder set; not watching");
            return None;
        }
        Err(e) => {
            log::warn!("replay watcher: cannot read the replay folder setting: {e}");
            return None;
        }
    };
    let path = Path::new(&dir);
    if !path.is_dir() {
        log::warn!("replay watcher: folder {dir} does not exist; not watching");
        return None;
    }
    let tx = tx.clone();
    let app = app.clone();
    let handler = move |res: DebounceEventResult| match res {
        Ok(events) if !events.is_empty() => {
            log::debug!("replay watcher: {} change(s)", events.len());
            // Kill clips: the game writes a round file when the round ends, so a changed `.rec`
            // means "cut the clips of that round now" (ignored unless the user enabled clips).
            crate::clips::on_replay_files(&app, events.iter().map(|e| e.path.clone()).collect());
            let _ = tx.send(Msg::Refresh);
        }
        Ok(_) => {}
        Err(e) => log::warn!("replay watcher error: {e:?}"),
    };
    let mut d = match new_debouncer(DEBOUNCE, handler) {
        Ok(d) => d,
        Err(e) => {
            log::warn!("replay watcher: cannot create watcher: {e}");
            return None;
        }
    };
    if let Err(e) = d.watcher().watch(path, RecursiveMode::Recursive) {
        log::warn!("replay watcher: cannot watch {dir}: {e}");
        return None;
    }
    log::info!("replay watcher: watching {dir}");
    Some(d)
}
