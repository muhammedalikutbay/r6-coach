# R6 Coach v2 — product and technical spec

Status: approved direction from the user (2026-11-01). v1 works but the user found the design weak and the content thin.
Goal: a desktop app that feels like r6.tracker.network for ONE player (`H.A.N-01`), built from local match replays + the one-time Tracker baseline, with a built-in coach that compares the last match and the last 5 matches against the player's own history.

Reference screenshots of Tracker (overview, operators, maps, match scoreboard, rounds, trends): `C:\Users\Kutbay\Desktop\ObsidianVault\Projeler\R6Koc\AI\tracker-referans\`.

## Working rules for this phase
- No installer until the user says the app is done. For milestone checks build a standalone exe (`npm run tauri build -- --no-bundle`).
- Keep tests light: compile, `cargo test` for new pure logic, `npm run check`. No long UI scenarios per task; the user tests at the end of each milestone.
- Everything user-facing in English. Code and comments in English.

## 1. Data we can show (all from replays unless noted)
Per match (all 10 players, not just the tracked one):
- map, mode, date, score, win/loss, duration if available
- scoreboard per player: operator picks per round, K, D, HS%, first kills (FK), first deaths (FD), multi-kills (2K–5K), clutches (1vX won/lost), survived rounds, KOST
- per round: winning team, win condition (elimination, defuse, plant, time), site, kill feed (time, killer → victim, headshot, killer/victim operator)
- NOT available: assists (parser gives 0), weapons, RP changes. Never show fake values; hide those columns.
Career/baseline (Tracker, one-time): per operator, per map (attack/defense), per mode, overall. Use it for "career" numbers and as the prior for the coach.

## 2. Assets
- Operator badges and map images: one-time download of the Tracker CDN images listed in the Tracker JSON (`operatorImageUrl`, `mapImageUrl` in `R6Koc-veri\tracker\profile_*_segments_*.json`) into `app/static/assets/operators/<key>.png` and `app/static/assets/maps/<key>.jpg` via a script `tools/fetch-assets/fetch.mjs`. The app never fetches at runtime. Private, non-distributed gift; keep a `CREDITS.md` noting images belong to Ubisoft.
- Operators/maps missing an image fall back to a neutral placeholder with initials.

## 3. Information architecture (one global filter, few views)
A persistent **filter bar** at the top of every view controls everything:
- Range: Last match · Last 5 · Last 20 · This season · All time
- Mode: All · Ranked · Unranked/Standard · Quick Match
- Side: Both · Attack · Defense
- Operators: searchable multi-select with badges (chips)
- Maps: searchable multi-select with thumbnails
Filter state lives in one store and is shared by all views; chips show active filters with one-click clear.

Views (left rail, icons + labels):
1. **Overview** — KPI cards (K/D, win %, HS %, kills/round, FK/FD ratio, clutch %), a K/D trend line, top operators grid (badge, rounds, win %, K/D, mini bar), maps grid (image card, attack/defense win bars), recent matches list. Clicking an operator or map card adds it to the filter (no separate operator/map pages).
2. **Matches** — match list (Tracker style rows: map image strip, mode, score, K/D/HS, badges like 3K / 1v2 won) grouped by day with day totals; clicking opens a **match detail** panel: Scoreboard (10 players, both teams, operator picks per round), Rounds (round strip + per-round table + kill feed with operator badges).
3. **Coach** — see section 5.
4. **Settings** — replay folder, rebuild database, welcome screen.

## 4. Backend API (Tauri commands)
- `query(filter) -> QueryResult` — single entry point for Overview: `{ source, kpis, trend[], operators[], maps[], recentMatches[] }`.
  `source` tells the UI where numbers come from: `career` (baseline + post-capture replays) when only mode/side/range-all filters are set; `replays` when the filter needs breakdowns the baseline lacks (e.g. operator AND map, or Last N). Show a small "based on N matches from replays" hint.
- `list_matches(filter, page) -> MatchRow[]`
- `match_detail(match_id) -> MatchDetail` (scoreboard + rounds + kill feed)
- `coach(filter) -> CoachReport` (section 5)
- existing: `refresh`, `get/set_replay_dir`, `inspect_replay_dir`, `reset_database`.
Storage: SQLite schema v3 adds `match_players`, `round_players`, `rounds` (win condition, site), `kill_events`. Migration clears and re-ingests (replays are the source of truth).

## 5. Coach v2
Compare a **focus set** against a **reference set** with smoothing (existing `stats.rs`):
- Last match vs player's average (same filter)
- Last 5 matches vs the previous 20 (and vs career)
- Per operator and per map inside the focus set vs that operator's/map's history
Output `CoachReport`: summary line, 3–6 insight cards (`kind: praise | improve | info`, title, text, evidence numbers, optional operator/map key for a badge), a "strengths" and a "work on" list (top 3 each), and comparison rows for a small table (metric, focus, reference, delta, direction).
Metrics: K/D, kills/round, HS %, win %, FK/FD, clutch %, survival %, multi-kill rounds, per-side win %.
Rules stay honest: minimum sample sizes, no weapon or assist claims, plurals correct.

## 6. Visual design
- Modern dark UI close to Tracker's feel: near-black background, layered dark cards, one accent color (Siege orange `#F5A623`-ish or Tracker red), green/red for good/bad deltas, generous spacing, rounded-xl cards, subtle borders, tabular numbers.
- Stack: Tailwind CSS v4 + shadcn-svelte (bits-ui) components + lucide-svelte icons + a light chart lib (LayerChart or uPlot). Keep SvelteKit SPA + adapter-static.
- Responsive from 1100×720 down to 900×600. Loading skeletons, empty states, error toasts.

## 7. Milestones (user tests at the end of each)
- **M1 Data layer v2**: schema v3 + full match model (all players, rounds, kill feed, derived stats), `query` / `list_matches` / `match_detail` commands, asset fetch.
- **M2 New UI**: design system, filter bar, Overview, Matches + match detail, Settings restyle. Ends with a standalone exe for the user.
- **M3 Coach v2**: comparison engine + Coach view + trend chart. Ends with a standalone exe for the user.
- **Release** (only when the user approves): installer via `tools/build-installer.ps1`.
