Task tracking: Beads (`bd`). Follow the `beads-workflow` skill.

# R6Koc

A desktop gift app for one Rainbow Six Siege (Siege X) player (Ubisoft name `H.A.N-01`). After each match he presses "Güncelle"; the app reads new local match replays, compares the last match with his own history and shows short Turkish advice.

Design notes (Turkish) live in the Obsidian vault: `C:\Users\Kutbay\Desktop\ObsidianVault\Projeler\R6Koc\AI\` (`03-mimari-ve-plan.md` is the main one).

## Stack
- Tauri 2 desktop app, Rust backend, Svelte 5 + TypeScript + Vite frontend (`app/`).
- Rust Cargo workspace: `crates/r6k-core` (pure logic), `crates/r6k-cli` (dev CLI), `app/src-tauri`.
- SQLite via `rusqlite` with the `bundled` feature.
- Replay parsing: vendored fork of r6-dissect (Go, MIT) in `tools/r6-dissect/`, shipped as a Tauri sidecar exe.

## Data (never commit)
`C:\Users\Kutbay\Desktop\R6Koc-veri\` holds the real data and stays outside git, because it contains other players' names:
- `MatchReplay\` — 30 match folders, 168 `.rec` round files (Ranked, Oct 24–31 2026).
- `tracker\` — one-time Tracker Network JSON captured by the user in his browser (see `_manifest.json`).
- `tools\r6-dissect\` — upstream r6-dissect v0.24.0 Windows build (reference only).
Only derived, player-only data (e.g. `data/baseline.json`) and anonymized test fixtures may go into the repo.
The app itself must never send requests to Tracker Network or Ubisoft.

## Rules
- Everything in English: code, identifiers, comments, UI text, advice text, the birthday welcome. (User decision, 2026-11-01.) Beads issues and vault notes stay Turkish.
- `main` is the only long-lived branch; one commit per task. Parallel workers run in git worktrees (`C:\Users\Kutbay\Desktop\R6Koc-wt\<issue>`, branch `task/<issue>`); the orchestrator rebases and fast-forwards them into `main`. Touch only the files your task names.
- Build environment: Windows 11 SDK + MSVC are installed. Run `cargo` and `npm run tauri ...` from PowerShell (Git Bash's `link.exe` shadows MSVC's). Never kill processes by image name; other workers run in parallel.
- Log where it helps (the `log` crate in Rust, `console` in the frontend).
- Keep verification light (user decision, 2026-11-01): `cargo test` for new pure logic, `npm run check`, and a quick run when needed. The user does the detailed testing at the end of each milestone. No installer until the user says the app is done; milestone builds are standalone exes (`npm run tauri build -- --no-bundle`).
- v2 work follows `docs/v2-spec.md` (Tracker-like UI, one global filter bar, operator/map images, coach v2).
- Unknown operator or map IDs must never crash anything: keep the raw ID and show a readable fallback.

## Lessons
- Dev runs, built exes and the installed app share one app data folder (`%APPDATA%\com.kutbay.r6koc` with `r6koc.db`, `%LOCALAPPDATA%\com.kutbay.r6koc`). Scenarios that call `reset_database` or cleanup that deletes these folders also hit the user's own copy. Before such a test, check that no `R6 Coach.exe` / `r6koc-app.exe` started by the user is running; if one is, do not reset or delete.
- r6-dissect binary offsets break on every Siege season. Operator swaps were lost in Y11 (about 11-16% wrong operators, found by comparing with Tracker). After a game update, re-compare replay operators with the Tracker JSON before trusting new data (see `tools/r6-dissect/UPSTREAM.md`, patch 4).
