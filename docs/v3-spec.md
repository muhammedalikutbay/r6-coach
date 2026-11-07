# R6 Coach v3 — for every player

Status: user decisions 2026-11-07. v2 (release 2.0.0) was built for one player and used a one-time
Tracker capture (`data/baseline.json`) for "This season" / "All time". From v3 the app is for **any**
Siege player on the PC: the tracked player is always the replay's recording player, and all numbers
come from that player's own data.

## User decisions (2026-11-07)
1. Season and All time must show the active user's own data, never the old `H.A.N-01` capture.
   Source: stored replays **plus** official Ubisoft totals when the user is signed in.
2. Fix the Operators / Maps search placeholder (mojibake `â€¦`).
3. Add date and time-of-day filtering.
4. Refresh runs by itself when the replay folder changes. Unfinished matches must not cause errors
   or be counted half-done.
5. Overview: OPERATORS, MAPS and RECENT MATCHES sections collapse and expand.
6. Match detail kill feed shows the weapon of each kill.
7. Rank info at the top of Overview. Source: the user's own Ubisoft sign-in inside the app.
8. As much as possible comes from `.rec` replays and is **stored** (the database outlives replays the
   game or the user deletes).

## Rules that change
- The app may talk to Ubisoft **only** for the signed-in user's own rank/stats, only after the user
  signs in. Never Tracker Network. No other network calls.
- Passwords are never stored or logged. Keep only Ubisoft's remember-me ticket, in Windows Credential
  Manager (`keyring` crate). Never log tickets, emails or passwords.
- `data/baseline.json` and the Tracker-based "career" source are retired.

## Shared contract (all workers code against this; serde `rename_all = "camelCase"`)

### Filter (Rust `r6k_core::query::Filter`, TS `Filter`)
New optional fields, all default `null` (= no limit):
```ts
dateFrom: string | null;   // local date 'YYYY-MM-DD', inclusive
dateTo: string | null;     // local date 'YYYY-MM-DD', inclusive
hourFrom: number | null;   // local hour 0-23, inclusive
hourTo: number | null;     // local hour 0-23, inclusive; hourFrom > hourTo wraps past midnight (22 -> 2)
tzOffsetMin: number;       // minutes to ADD to UTC to get local time (= -new Date().getTimezoneOffset()); set by api.ts on every call, not by the UI
```
Order: mode/maps/season/date/hour select matches first, then `last1/5/20` takes the newest N of those.
`range: 'season'` = matches whose `season` equals the newest season among all stored matches.

### Match model
- `FullMatch.season: String` — from the replay `gameVersion` prefix, e.g. `Y11S3_Alpha04` -> `Y11S3`; empty if unknown.
- `KillEvent` new fields: `weaponId: Option<u64>` (raw asset id from the kill packet),
  `weapon: Option<String>` (display name, e.g. `R4-C`), `weaponSlot: Option<String>`
  (`primary` | `secondary` | `gadget` | `ability`). All `None` when unknown; never guess.
- r6-dissect JSON: kill events gain `"weaponId": <u64>` and `"weaponSlot": "<slot>"` (omitted when unknown).
- TS `KillEventView` gains `weapon: string | null`, `weaponSlot: string | null`.

### QueryResult
`source` becomes `'replays'` only. New optional field `official: OfficialTotals | null` (filled by the
app shell when signed in and the filter is plain `season`/`all` with no operator/map/date filters):
```ts
interface OfficialTotals { scope: 'season' | 'all'; kd: number; winPct: number; kills: number; deaths: number; wins: number; losses: number; matches: number; fetchedAt: string }
```

### Rank (Tauri commands, module `app/src-tauri/src/ubisoft.rs`)
```ts
ubi_status() -> UbiStatus
ubi_login(email: string, password: string) -> UbiLoginResult
ubi_login_2fa(code: string) -> UbiLoginResult
ubi_logout() -> void
ranked_profile(force: boolean) -> RankedProfile | null   // null when not signed in; cached 10 min unless force

interface UbiStatus { signedIn: boolean; username: string | null; profileId: string | null; matchesReplayPlayer: boolean | null }
interface UbiLoginResult { status: 'ok' | 'needs2fa' | 'error'; message: string | null; user: UbiStatus | null }
interface RankedProfile {
  season: string;             // e.g. 'Y11S3' or Ubisoft season number as text
  rankName: string;           // e.g. 'Emerald II', 'Unranked'
  rankPoints: number;         // RP
  maxRankName: string; maxRankPoints: number;
  topRankPosition: number | null; // Champion position, if any
  kills: number; deaths: number; wins: number; losses: number; abandons: number;
  rankIcon: string | null;    // local asset key under /assets/ranks/<key>.png, or null
  fetchedAt: string;          // ISO UTC
  official: { season: OfficialTotals | null; all: OfficialTotals | null };
}
```
`matchesReplayPlayer` is false when the signed-in profile id differs from the newest replay's recording profile id (UI shows a warning).

### Auto refresh (app shell)
- The backend watches the replay folder (`notify` crate), debounces 5 s, then runs the same import as
  the Update button in a background thread.
- After each import (manual or automatic) it emits the Tauri event `replays-updated` with payload
  `RefreshResult` `{ newMatches, updatedMatches, pending, failed }`. The frontend bumps `refreshTick` on it.
- A folder is imported only when no file in it changed for 60 s. A match that has not ended is stored
  as `in_progress` and excluded from all queries. If the folder changes later it is re-parsed and
  replaced. A folder quiet for 20 min without an end becomes `abandoned` and is counted (left match).
  A parse error on a folder still being written is `pending`, never `failed`.

### Storage (schema v4)
`matches` keeps one row per match with: `match_id`, `folder`, `fingerprint` (file names + sizes),
`status` (`complete` | `in_progress` | `abandoned`), `season`, `timestamp`, `raw_json` (the r6-dissect
output, compressed) and `full_json` (serialized `FullMatch`). Queries load `full_json`. When the
parser changes, `full_json` is rebuilt from `raw_json` without the `.rec` files. Migration from v3
re-imports from the replay folder once (v3 rows have no raw JSON), keeping settings.
