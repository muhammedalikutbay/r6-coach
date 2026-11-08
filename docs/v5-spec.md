# R6 Coach v5 — user test fixes, Home panel, operator and weapon screens

Status: user feedback 2026-11-09 after testing v4 on a gaming PC. Builds on `docs/v4-spec.md`.

## User feedback (2026-11-09)
1. Clips drift when the game is minimized (alt-tab); no drift otherwise. No audio in clips.
   The user's working clip settings: 10 s before, 0 s after, offset -5 s, game in borderless.
2. Overview keeps all its sections (Operators, Maps, Weapons, Round impact, Timing, Rivals, Recent
   matches); only the rank header moves out of Overview.
3. A **Home** panel like `https://r6.arenyze.com/stats?username=<name>&platform=uplay`: profile strip
   (avatar, name, platform, level, updated), current season card (rank icon, rank, RP, K/D, win rate,
   wins/losses/matches/kills-deaths), season peaks (highest rank season, best K/D season), season
   history list, season overview KPIs, playlist tabs (Ranked / Casual / Unranked / Event / Warmup) with
   per-playlist stats, current vs peak rank, and an operators table with season and playlist selectors.
4. Clips do not play: "This clip cannot be played".
5. Maps outside the ranked pool show as `Map #441842102525` (no name, no image).
6. Clips settings: inputs are disabled while the Clips switch is off.
7. Settings page content is left aligned; center it.
8. Some kill feed rows have no weapon name; find and fix the missing ids.
9. Operator and weapon views: dedicated screens with stats, and the related clips play there.
10. Take initiative where something is missing. At the end an independent agent reviews the UI.

## Real Arenyze response (from the user's PC, 2026-11-08)
Local copy, never commit it: `C:\Users\Kutbay\Desktop\R6Koc-wt\_v5\arenyze-real-response.json`.
Profile V2 top-level keys: `account`, `banned`, `history`, `meta`, `player`, `seasons`, `stats`.
- Rank: `stats.platform_families_full_profiles[0].board_ids_full_profiles[]` with `board_id`
  `ranked` | `standard` | `casual`; each `full_profiles[0]` has `season_id` and
  `profile {rank, rank_points, max_rank, max_rank_points, kills, deaths, wins, losses, abandon}`
  and `season_statistics {kills, deaths, match_outcomes {wins, losses, abandons}}`. `rank` is the
  Ubisoft 0..36 id (21 = Platinum V); the existing table applies.
- `history.data.history.data[]` = `[isoTime, {value: RP, metadata {rank: "Platinum 5", imageUrl, color}}]`.
- `seasons.data.metadata {currentSeason, clearanceLevel, battlepassLevel}`; `seasons.data.segments[]`
  Tracker-style, one per season: `attributes {season, gamemode, sessionType}`, `metadata {name,
  shortName "Y11S3", color, rankType}`, `stats {kdRatio, kills, deaths, matchesPlayed, matchesWon,
  matchesLost, rankPoints, maxRankPoints, timePlayed}`.
- `account {level, profilePicture, profiles[{nameOnPlatform, platformType, profileId}]}`.
- Operators V2: `operators.operators[]` with `operator, side, kills, deaths, kd, headshotPercent,
  roundsPlayed, wins, losses, winPercent, matchesPlayed, firstBloods, firstDeaths, clutches,
  clutchesLost, aces, kills1K..kills5K, timePlayedMs, ...`, `filters {modes, seasonYear}`.

## Safe real-app testing (new)
`R6COACH_DATA_DIR=<folder>` makes the app keep its database, clips, clip buffer and WebView2 data
under that folder instead of `%APPDATA%` / `%LOCALAPPDATA%`. Agents may run the real app only with
this variable set to a folder in their worktree or scratchpad, after checking that the real folders'
modification times do not change. A copy of the user's database for such runs:
`C:\Users\Kutbay\Desktop\R6Koc-wt\_v5\r6koc-sample.db` (copy it first; never commit it).

## Unknown ids found in the sample data (2026-11-09)
- Maps (Quick Match on the user's PC): 441842102525, 430788891316, 446546313105.
- Operators the parser does not know: game ids 444310693746, 456757346397 (their loadouts, and so
  their weapon names, are lost).
- Weapon item ids missing from `data/weapons.json`: 437464103147 (Dokkaebi primary),
  445433462931 (Rauora primary), 454107765692 (Wamai gadget).
- Kills with no slot: Fuze, Thorn, Caveira, Ace, Bandit, Wamai (likely gadget, ability or melee).
