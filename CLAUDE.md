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
- Single branch `main`; one commit per task.
- Log where it helps (the `log` crate in Rust, `console` in the frontend).
- Verify with `cargo test`, the CLI against the real data folder, and the running app (`npm run tauri dev`).
- Unknown operator or map IDs must never crash anything: keep the raw ID and show a readable fallback.
