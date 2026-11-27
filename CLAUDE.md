Task tracking: Beads (`bd`). Follow the `beads-workflow` skill.

# R6Koc

A free, open-source desktop stats and coaching app for Rainbow Six Siege (Siege X) on PC (see `README.md`). It started as a personal project for one player; from v3 (user decision, 2026-11-07) it works for everyone: the tracked player is the replay's recording player. New local match replays are imported automatically; the app shows Tracker-like stats and compares recent matches with the player's own history.

v3 spec and shared contract: `docs/v3-spec.md`.

Design notes (Turkish) live in the maintainer's private notes, outside this repository. The specs in `docs/` are the public record of the decisions.

## Stack
- Tauri 2 desktop app, Rust backend, Svelte 5 + TypeScript + Vite frontend (`app/`).
- Rust Cargo workspace: `crates/r6k-core` (pure logic), `crates/r6k-cli` (dev CLI), `app/src-tauri`.
- SQLite via `rusqlite` with the `bundled` feature.
- Replay parsing: vendored fork of r6-dissect (Go, MIT) in `tools/r6-dissect/`, shipped as a Tauri sidecar exe.

## Data (never commit)
A folder next to the repository (the maintainer calls it `R6Koc-veri`) holds the real data and stays outside git, because it contains other players' names:
- `MatchReplay\` — 30 match folders, 168 `.rec` round files (Ranked, Oct 24–31 2026).
- `tracker\` — one-time Tracker Network JSON captured by the user in his browser (see `_manifest.json`).
- `tools\r6-dissect\` — upstream r6-dissect v0.24.0 Windows build (reference only).
Only derived, player-only data (e.g. `data/baseline.json`) and anonymized test fixtures may go into the repo.
The app never sends requests to Tracker Network or Ubisoft. Since v4 (user decision, 2026-11-07) it may call the Arenyze API (docs/arenyze-api.md) only for the tracked player's own profile, and only with the user's own API key (Windows Credential Manager, never logged, never embedded). Since v6 (user decision, 2026-11-10) the Home panel may also load the one profile image URL that Arenyze returns (`account.profilePicture`, on `avatars.ubisoft.com`); no other Ubisoft request.

## Rules
- Everything in English: code, identifiers, comments, UI text, advice text, the birthday welcome. (User decision, 2026-11-01.) Beads issues and vault notes stay Turkish.
- `main` is the only long-lived branch; one commit per task. Parallel workers run in git worktrees (`<repo>-wt/<issue>`, branch `task/<issue>`); the orchestrator rebases and fast-forwards them into `main`. Touch only the files your task names.
- Build environment: Windows 11 SDK + MSVC are installed. Run `cargo` and `npm run tauri ...` from PowerShell (Git Bash's `link.exe` shadows MSVC's). Never kill processes by image name; other workers run in parallel.
- Log where it helps (the `log` crate in Rust, `console` in the frontend).
- Keep verification light (user decision, 2026-11-01): `cargo test` for new pure logic, `npm run check`, and a quick run when needed. The user does the detailed testing at the end of each milestone. No installer until the user says the app is done; milestone builds are standalone exes (`npm run tauri build -- --no-bundle`).
- v3 work follows `docs/v3-spec.md`; v2 background in `docs/v2-spec.md` (Tracker-like UI, one global filter bar, operator/map images, coach v2).
- Unknown operator or map IDs must never crash anything: keep the raw ID and show a readable fallback.

## Lessons
- Never launch `r6koc-app.exe` (debug or release) without `R6COACH_DATA_DIR`: without it the app opens the user's real app data. Setting `APPDATA` / `LOCALAPPDATA` does not isolate it, because Tauri resolves these folders through the Windows known-folder API. `R6COACH_DATA_DIR=<folder>` (a folder in your worktree, e.g. `target\appdata`) moves the database, clips, clip buffer and WebView2 data there; it is verified (2026-11-09: file times of `%APPDATA%\com.kutbay.r6koc` and `%LOCALAPPDATA%\com.kutbay.r6koc` did not change over a full run). Copy a sample database (never committed; `tools/demo-data/sanitize.mjs` makes an anonymized one) into that folder for realistic data, and point its `replay_dir` setting at an empty folder. To drive the UI, start the app with `WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS=--remote-debugging-port=9222` and talk to it over the DevTools protocol. Test backend logic with unit tests and temp databases. (Incident 2026-11-07, clips task.)
- Clip timeline: the Media Foundation encoder converts frames to the declared frame rate. If the game delivers fewer frames than declared, the video timeline drifts by up to a second from the real one; faster input is dropped cleanly. So `CLIP_FPS` (30) must stay below what a PC sustains; every segment also has a `.clock` file with the wall time of every frame and the cutter looks clips up on it.
- Dev runs, built exes and the installed app share one app data folder (`%APPDATA%\com.kutbay.r6koc` with `r6koc.db`, `%LOCALAPPDATA%\com.kutbay.r6koc`). Scenarios that call `reset_database` or cleanup that deletes these folders also hit the user's own copy. Before such a test, check that no `R6 Coach.exe` / `r6koc-app.exe` started by the user is running; if one is, do not reset or delete.
- r6-dissect binary offsets break on every Siege season. Operator swaps were lost in Y11 (about 11-16% wrong operators, found by comparing with Tracker). After a game update, re-compare replay operators with the Tracker JSON before trusting new data (see `tools/r6-dissect/UPSTREAM.md`, patch 4).
