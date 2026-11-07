# R6 Coach v4 — Arenyze rank, kill clips, full-data coach

Status: user decisions 2026-11-07. Builds on `docs/v3-spec.md`.

## User decisions (2026-11-07)
1. **Ubisoft sign-in is removed.** Rank and other online values come from the Arenyze API
   (`docs.arenyze.com/r6`) with the user's **own** free API key, entered in Settings. No key = the app
   works fully offline, the rank header shows how to add a key. Never embed a shared key.
2. **Coach compares everything.** Every value we can get from `.rec` replays is shown as a stat and
   compared. Coaching is not limited to a few metrics; insights are ranked by importance.
3. **Kill clips.** While the game runs the app records the game window into a short rolling buffer.
   When a round's `.rec` arrives (the game writes it at round end), the app cuts a short clip around
   each of the tracked player's kills (and deaths, user option) and deletes the buffer. Clips are kept
   for the last N matches (5 / 10 / 20 / all) plus an optional disk cap; starred clips are never
   deleted. Recording is **off by default**; the user turns it on in Settings.
   Timing does not need to be perfect (user: "do not fuss over it"); a wider window and a user
   offset setting absorb the error.
4. No Overwolf, no screen/HUD reading, no game memory or network reading. Times come only from `.rec`.

## Network rule (replaces the v3 Ubisoft rule)
The app talks only to the Arenyze API, only when the user has entered their own key, only for the
tracked player's own profile (username from the newest replay + platform `uplay`). Never Tracker,
never Ubisoft. The key is stored in Windows Credential Manager (`keyring`), never logged.
Respect the free plan: 2,000 calls/month, 30/min. Cache results (default 15 min) and never call on a
timer in the background; only on app start (if cache is stale), on manual refresh, and after new
matches are imported (at most once per 10 min).

## Research inputs (written by the research tasks, read by the coding tasks)
- `docs/arenyze-api.md` — endpoints, request/response fields, rank fields, limits, errors.
- `docs/coach-audit.md` — every value available from `.rec` (parsed and derivable) vs what the UI
  shows and what the coach compares; gaps ranked by coaching value.

## Kill clip timing (from `.rec`, analysed 2026-11-07 on 12 matches / 75 rounds)
- Header `starttime` (Unix ms) = header `datetime`; one per round file.
- Kill `timeInSeconds` counts **down** on the round timer in real seconds (fit slope -1.07).
- Prep-phase events use the prep timer (e.g. operator swap at 0:09). After a defuser plant the timer
  shows the defuser countdown; handle with the plant event time.
- Offset from `starttime` to action start is not exact from data (about ±11 s across rounds).
  Approach: live, the round file appears at round end, so anchor on the **file write time** (round
  end) and the round-ending event time, and convert each kill backwards. Fall back to `starttime` +
  a default offset for imported old rounds. Expose `clip offset (s)` in Settings (default 0) and use a
  default window of 6 s before / 2 s after the kill.
