# Arenyze R6 API: reference for R6 Coach v4

Status: research note, written 2026-11-07 from public documentation only. No account was created, no key was obtained and no API call was made. Anything not seen in the sources is marked **unverified**.

Sources read:
- Docs index: https://docs.arenyze.com/r6 (all R6 endpoints on one page), https://docs.arenyze.com/authentication, https://docs.arenyze.com/limits, https://docs.arenyze.com/errors
- Plans: https://arenyze.com/premium
- Terms: https://arenyze.com/terms
- SDK: https://github.com/mazeor9/r6-data.js (README, `src/R6Client.js`, `src/resources/Players.js`, `src/resources/Game.js`, `src/utils/httpClient.js`)
- Repo: `app/src-tauri/src/ubisoft.rs` (rank table), `app/static/assets/ranks/` (icons)

Note: the SDK README calls this an "unofficial, fan-made SDK". Arenyze is a third-party tracker, not Ubisoft.

---

## 1. Authentication

- **Base URL (stats and catalogue):** `https://public-api.arenyze.com/r6/api` (SDK default, confirmed in `httpClient.js`).
- **Replay host:** `https://r6.arenyze.com/api` (SDK default; used only for match replay upload and lookup).
- **Header:** `api-key: <YOUR_API_KEY>`. Confirmed in the docs example for THE FINALS and in the SDK (`headers['api-key'] = apiKey`). The R6 docs also show `api-key` for `/me/usage`.
- Replay upload also accepts `Authorization: Bearer <key>`. The SDK does not use it. The docs say the Bearer form works for THE FINALS and Endfield in the auth page; for R6 replay upload it is listed as an alternative.
- **Getting a key (from the docs):**
  1. Sign in at arenyze.com.
  2. Open Profile, then the Developer section (`https://arenyze.com/profile#developer`).
  3. Create the key there. Each account can have only one active key.
- **Is it free?** The Free plan is listed as "€0, Free forever" and includes 2,000 calls/month. The authentication page does not say that a key requires a paid plan, so a free key is **implied but not explicitly confirmed**.
- **Key lifetime:** keys last 30, 60 or 90 days; the user is notified before expiry (auth page). Regenerating a key invalidates the old one at once.
- **Storage advice from the docs:** keep the key on a server; do not put it in a web page or mobile app. R6 Coach is a local desktop app where the user holds their own key, which is the same situation the docs warn about. This is a risk to note (see section 9). The app plan stores the key in Windows Credential Manager, which matches the docs' intent of not exposing it in files.
- **Rate-limited R6 endpoints** use `{"error": "..."}` messages such as "Missing api-key header" (see section 2.4).

---

## 2. Limits, quota and errors

### 2.1 Plans (https://arenyze.com/premium)

| Plan | Price | Monthly calls (all games, shared) | Per-minute limit | Replay storage | Other |
|---|---|---|---|---|---|
| Free | €0 | 2,000 | 30 / min | 15 matches | Flows: 1, daily |
| Pro | €3.99/mo, €39.99/yr | 50,000 | 120 / min | 75 matches | Flows: 5, hourly |
| Ultra | €6.99/mo, €69.99/yr | 100,000 | 240 / min | 200 matches | Flows: 10, every 30 min |

- The limits page also lists **Mega** (200,000 calls, 480/min) and **Enterprise** (500,000 calls, 600/min). Their prices are not on the premium page as read.
- Free plan split: THE FINALS and Endfield are capped at 1,000 calls each inside the shared 2,000. R6 has no separate free cap, so R6 can use the full 2,000.
- **Per-minute limit:** the docs say Rainbow Six "keeps its own per-minute, per-hour and per-day windows by endpoint". The per-hour and per-day values **are unverified**.

### 2.2 What counts as a call

- "Every successful call, R6, THE FINALS or Endfield, counts once toward the monthly limit." Only successful calls count. Failed calls (4xx/5xx) do not count, as the limits page implies.
- **Reset:** the counter resets on the 1st of each month at 00:00 UTC.
- **Quota behaviour when exhausted:** the 429 error shape is documented (section 2.3). What the API does on a per-minute or per-hour window overflow for R6 specifically is **unverified**.

### 2.3 Response headers

| Header | Meaning |
|---|---|
| `X-RateLimit-Limit` | Requests allowed per minute on this game |
| `X-RateLimit-Remaining` | Requests left in the current minute |
| `X-Quota-Limit` | Monthly calls on the plan, across all games |
| `X-Quota-Used` | Calls used this month, across all games |
| `Retry-After` | Seconds to wait after a 429 |

Use `X-Quota-Used` / `X-Quota-Limit` to show quota in the app. The "Back off on 429 using Retry-After" advice applies.

### 2.4 Errors

General format (docs errors page): JSON with a stable `error` code and a human `message`.

| HTTP | `error` code | Meaning |
|---|---|---|
| 400 | `BAD_REQUEST` | Missing or invalid parameter |
| 401 | `MISSING_API_KEY` | No key sent |
| 401 | `INVALID_API_KEY` | Key does not exist (e.g. regenerated) |
| 401 | `API_KEY_EXPIRED` | Key expired |
| 404 | `ENDPOINT_NOT_FOUND` | No public endpoint at that path |
| 404 | `PLAYER_NOT_FOUND` / `NOT_FOUND` | Player or item not in the tracker |
| 429 | `RATE_LIMITED` | Per-minute limit hit; wait `Retry-After` seconds |
| 429 | `MONTHLY_QUOTA_EXCEEDED` | Plan's monthly calls used up (all games) |
| 429 | `FREE_GAME_QUOTA_EXCEEDED` | Free plan's 1,000 calls for this game used up |
| 502 / 503 | `UPSTREAM_UNAVAILABLE` / `UNAVAILABLE` | Tracker or game service down; retry later |

Example 429 body (documented, values illustrative of the field names):
```json
{ "error": "MONTHLY_QUOTA_EXCEEDED", "message": "Monthly quota reached (shared by R6, THE FINALS and Endfield).",
  "plan": "free", "limit": 2000, "used": 2000, "reset_at": "<UTC timestamp>", "upgrade_url": "<premium page>" }
```

- **Rainbow Six endpoints** return a different, simpler shape: `{"error": "<text>"}`, e.g. "Missing api-key header", "Rate limit exceeded". The exact R6 error body for a 404 or 429 is **unverified**.
- The SDK `httpClient.js` throws `Error("Request failed with status code <n>")` with `response.status`, `response.data` and `response.headers`. It sets no timeout and has no retry. `Players.js` throws `Authentication error` on 401. Network errors propagate as-is.

### 2.5 Quota usage endpoint

`GET https://public-api.arenyze.com/r6/api/me/usage` (header `api-key`).
Response fields: `plan`, `limit`, `usage` (object with `total_calls`, `maps_calls`, `operators_calls`, `weapons_calls`, `skins_calls`, `ranks_calls`, `seasons_calls`, `stats_calls`, `search_calls`, `servicestatus_calls`, `attachment_calls`, `charms_calls`, `debug_calls`, `last_seen_at`).
- This call is itself a call and probably counts toward the monthly quota (**unverified**). Use it sparingly, e.g. once per app start in Settings, not on every refresh.
- Data types of these fields are not given on the page (numbers and a timestamp are assumed, **unverified**).

---

## 3. Endpoints

All paths below are relative to `https://public-api.arenyze.com/r6/api` unless noted. All are `GET` unless noted. Field types marked "assumed" are inferred from examples only.

### 3.1 Player endpoints

#### Profile V2 (`/v2/profile`)
- Params:
  - `nameOnPlatform` (string, required)
  - `platformType` (required): documented as `Uplay`, `psn` or `xbl`. The SDK README example uses `uplay`. Case sensitivity is **unverified**. Send `uplay` only after the docs answer this.
  - `platform_families` (optional): `pc`, `psn`, `xbl`. Inferred by the SDK if omitted.
- Response (top-level keys):
  - `player`: `nameOnPlatform`, `platformType`, `platformFamilies`
  - `stats`: `platform_families_full_profiles`
  - `account`: `level`, `xp`, `profilePicture`, `profiles`
  - `banned`: `isBanned`, `banAlerts`
  - `seasons`: `data` (`platformInfo`, `metadata`, `segments`)
  - `history`: `data` (`history`)
  - `meta`: `included`, `partial`, `errors`
- Example in the docs: `account` present, `seasons.data.segments` and `history.data` empty arrays, `meta` `{"included": ["stats","account","banned","seasons","history"], "partial": false, "errors": {}}`.
- **Rank fields:** the docs example contains **no** rank name, rank ID, RP, max rank, rank image, season K/D or W/L. The endpoint description says it returns "season segments and rank-points history", but the example shows those empty. Whether a populated player response contains these is **unverified**. This is the main open question.
- The `stats.platform_families_full_profiles` object is not described in the example, so its shape is **unverified**.

#### Full Stats V2 (`/v2/fullstats`)
- Params: `nameOnPlatform` (required), `platformType` (required), `seasonYear` (e.g. `Y11S3`, or `all`), `modes` (`Ranked`, `standard`, `casual`, or `all`).
- Response:
  - `player`: `nameOnPlatform`, `platformType`
  - `filters`: `seasonYear`, `modes`
  - `fullStats`: `seasonYear`, `seasonNumber`, `sessionType`, `operators`, `platform_families_full_profiles`, `data` (segments), `totalsHoursPlayed` (`hours`, `totalMs`, `segmentsCounted`)
- Useful for play time (`totalsHoursPlayed`). Segment field names inside `data` are **unverified** (the example is not shown in full).
- Season K/D, W/L and match count are **not** named as top-level fields. Whether they sit inside `segments` is **unverified**.

#### Operators V2 (`/v2/operators`)
- Params: `nameOnPlatform` (required), `platformType` (required), `seasonYear` (`Y8S1` through `Y11S4`, or `all`), `modes` (`All`, `ranked`, `standard`, `quick-match`, `dual-front`, `siege-cup`).
  - The SDK README also lists `unranked` and `casual`. The docs list does not. Whether those are valid for the API is **unverified**.
- Response:
  - `player`: `nameOnPlatform`, `platformType`
  - `filters`: `seasonYear`, `modes`
  - `operators`: `seasonYear`, `seasonNumber`, `platformType`, `sessionType`, `operators[]`
  - Each operator: `operator` (name), `side` (`Attacker`/`Defender`), `roundsPlayed`, `kd`, `kills`, `deaths`
- Example: `{"operator":"Twitch","side":"Attacker","roundsPlayed":47,"kd":1.08,"kills":38,"deaths":35}`.
- Note: the counter is `roundsPlayed`, not matches. Types assumed: numbers.
- Win counts and `winPct` per operator are **not** shown.

#### Leaderboard V2 (`/v2/leaderboard`)
- Params: `page` (1 to 50, default 1), `platform` (`Pc`, `psn` or `xbl`, default `pc`). The SDK example uses `pc`.
- Response: `platform`, `page`, `entries[]`, each with `id`, `kd`, `matchesPlayed`, `rankPoints`, `position`.
- This is the only documented endpoint that returns `rankPoints` and `position` with `matchesPlayed`. It gives Champion-style ranking for a page of players, not for the user's own profile by name. Whether `id` is a player name or an internal ID is **unverified**.

#### Ban status and level
- Ban: inside Profile V2 as `banned.isBanned` and `banned.banAlerts` (assumed boolean and array).
- Level / XP: inside Profile V2 as `account.level` and `account.xp`.
- Profile picture: `account.profilePicture` (URL or object: **unverified**).

### 3.2 Game catalogue endpoints (cacheable for a long time)

| Endpoint | Method and path | Params | Response |
|---|---|---|---|
| Operators | `GET /operators` | `name`, `safename`, `realname`, `birthplace`, `age`, `date_of_birth`, `season_introduced`, `health` (min), `speed`, `unit`, `country_code`, `roles` (`Attacker`/`Defender`), `side` | `name`, `safename`, `realname`, `birthplace`, `age`, `date_of_birth`, `season_introduced`, `health`, `speed`, `unit`, `country_code`, `roles`, `side`, `icon_url` |
| Weapons | `GET /weapons` | `name` | No example |
| Maps | `GET /maps` | `name`, `location`, `playlists`, `releaseDate`, `mapReworked` | `name`, `location`, `releaseDate`, `playlists`, `mapReworked` |
| Seasons | `GET /seasons` | `name`, `code` (e.g. `Y8S4`), `map`, `operators`, `weapons`, `description`, `startDate` | No example (SDK lists extra params `weapons`, `description`, `startDate` not in docs) |
| Ranks | `GET /ranks` | `version` (**required**: `v1`..`v7`), `name`, `min_mmr`, `max_mmr` | No example |
| Universal skins | `GET /universalSkins` | `name` | No example |
| Charms | `GET /charms` | `name`, `collection`, `rarity`, `availability`, `season` | No example |
| Attachments | `GET /attachment` | `name`, `style`, `rarity` | No example |
| Global search | `GET /searchAll` | `q` (**required**) | `query`, `summary` (`total`, `operators`, `weapons`, `maps`, `seasons`, `charms`, `attachments`), `results` (same keys) |
| Service status | `GET /servicestatus` (docs). SDK uses `/serviceStatus` | none | Array of `{name, status, services}` |

- **Ranks:** the SDK says `v7` covers Y11S2 onward ("Ranked 3.0"). The docs say only "Rank version (v1 to v7)". Which version matches the current season is **unverified**. The docs example is `/r6/api/ranks?version=v6&name=diamond`. The response shape is **unverified** (no example shown). Likely a list of rank tiers with MMR bounds; unverified.
- **Service status path:** docs say lowercase `servicestatus`, SDK says `serviceStatus`. Path case sensitivity is **unverified**. Use the docs path.
- **Seasons:** the docs and SDK differ (SDK has `weapons`, `description`, `startDate`). Use the union only after a check.

### 3.3 Other

- **Game stats V2** (`GET /v2/gamestats`): no params. Response: `steam` (`concurrent`, `estimate`), `crossPlatform` (`totalRegistered`, `monthlyActive`, `trendsEstimate`, `platforms` with `pc`, `playstation`, `xbox`), `ubisoft` (`onlineEstimate`), `lastUpdated`. Not useful for coaching. Could show a "game population" line; low priority.
- **Twitch stats V2** (`GET /v2/twitchstats`): not relevant to coaching. Skip.
- **Account and usage** (`GET /me/usage`): see section 2.5.
- **Match replay upload** (`POST https://r6.arenyze.com/api/replays/api/upload`):
  - Headers: `api-key` (recommended) or `Authorization: Bearer <key>`.
  - Form field `replayFiles`: up to 20 `.rec` files per request, all of one match.
  - Response: `matchID`, `rounds`, `match` (`match_id`, `replay_match_id`, `title`, `map`, `mode`, `match_type`, `rounds_count`, `blue_score`, `orange_score`), `quota` (`plan`, `limit`, `used`, `remaining`).
  - **Not for R6 Coach v4.** It would send the user's `.rec` files, which contain other players' names, to a third party. The vault rules forbid that. Listed for completeness only.
- **Get saved match** (`GET https://r6.arenyze.com/api/replays/api/matches/:matchId`): same headers; 404 for unknown `matchId`. Not used.

---

## 4. Rank data and icons

### 4.1 What the Arenyze docs give

- No documented player endpoint returns a **rank name, rank ID, rank image URL, or max rank** in a sample response. The Ranks endpoint gives tier tables by MMR, but without an example.
- The Leaderboard endpoint gives `rankPoints` and `position` for a page of players.
- The only image URL shown in the docs is `icon_url` on operators (game catalogue). No rank image URL is documented.
- **Conclusion:** the API does not, as documented, give a rank image. Rank icons must be mapped from a rank name or ID, if the player response has one. That is **unverified**.

### 4.2 Repo mapping (existing)

- `app/src-tauri/src/ubisoft.rs` has a 37-entry table: ID 0 to 36 mapped to a name and an icon key. The source says it comes from the danielwerg/r6data `src/ranks.ts` (seasons 28+), and matches siegeapi `ranks_v6`. Order: index 0 `Unranked`, 1 to 5 `Copper V`..`Copper I`, 6 to 10 `Bronze`, 11 to 15 `Silver`, 16 to 20 `Gold`, 21 to 25 `Platinum`, 26 to 30 `Emerald`, 31 to 35 `Diamond`, 36 `Champion`.
- Icons: `app/static/assets/ranks/` contains 37 PNGs: `unranked`, `champion`, and `<tier>-<1..5>` for copper, bronze, silver, gold, platinum, emerald and diamond. This matches the table one-to-one.
- The table is keyed by a **Ubisoft numeric rank ID**. The Arenyze docs do not show a numeric rank ID, so the table does not apply directly to Arenyze output.
- **Compatibility:** compatible only if Arenyze returns rank names in the same style (`Emerald II`) or the same numeric IDs. Neither is confirmed. Recommended approach: parse the name string into `tier` and `division` (e.g. `Emerald` and `II`), then look up `emerald-2.png`. Fall back to `unranked.png` or a text badge if no match. Do not hard-code Arenyze IDs.
- Division order in the repo table: `Emerald V` -> `emerald-5` is the lowest division of the tier, `Emerald I` -> `emerald-1` the highest. Keep this convention when parsing names.

### 4.3 Recommended steps for the coding task

1. Once the user has their own key, make one real Profile V2 call and save the response as a fixture, to see the real rank fields. This was not done in this research task.
2. If Profile V2 has no rank field, fall back to the Ranks catalogue (`/ranks?version=v7`) and match `min_mmr` / `max_mmr` to the player's MMR (MMR source **unverified**).
3. Keep the rank icon mapping inside the app, not from the API.

---

## 5. R6 Coach: what the Arenyze API can feed

Mapping the endpoints to app surfaces. "Verified" means the field is in a documented response. "Unverified" means the endpoint exists but the field is not shown.

| App surface | Arenyze source | Status |
|---|---|---|
| Overview: rank header (name, icon, RP) | Profile V2 rank fields, or Ranks catalogue | **Unverified.** Docs show no rank field. This is the key open question. |
| Overview: max rank | Profile V2 | **Unverified** |
| Overview: Champion position | Leaderboard V2 `position` (page 1 to 50 only) | Partly verified: works only for players on the first 50 pages. Likely not usable for the user's own rank without a page search. |
| KPI next to replay KPIs: season K/D | Full Stats V2 segments or Operators V2 totals | **Unverified** (Operators V2 gives per-operator K/D, not overall) |
| KPI: W/L, win % | Not seen in any sample | **Unverified** |
| KPI: matches played | Leaderboard `matchesPlayed` (leaderboard only); Full Stats segments | Partly unverified |
| KPI: rounds played | Operators V2 `roundsPlayed` (per operator) | Verified (per operator) |
| Operator cards: official operator stats | Operators V2 (`kd`, `kills`, `deaths`, `roundsPlayed`) | Verified. Filter by `seasonYear` and `modes`. |
| Operator cards: operator icon | Operators catalogue `icon_url` | Verified in the example field list (URL shape unverified) |
| Rank history chart | Profile V2 `history` (docs say "rank-points history") | **Unverified.** The example is empty. |
| Season segments | Profile V2 `seasons` / Full Stats V2 `data` | **Unverified** inner shape |
| Play time | Full Stats V2 `totalsHoursPlayed` | Verified field name; values assumed numeric |
| Ban status | Profile V2 `banned.isBanned` | Verified field name; boolean assumed |
| Level, profile picture | Profile V2 `account.level`, `account.profilePicture` | Verified field name |
| Quota meter in Settings | `/me/usage` (`usage`, `limit`) and response headers `X-Quota-*` | Verified field names |

### 5.1 Call budget (2,000 calls per month, Free plan)

Assumptions: one tracked player; calls only at app start (if cache is stale), on manual refresh, and after new matches (at most once per 10 min), per the v4 spec. Cache TTL 15 min.

- **One refresh** = Profile V2 + Operators V2 (ranked) = **2 calls**. Full Stats V2 adds one more.
- **Catalogue** (operators, ranks, maps): fetched once per app version and cached for weeks. Roughly 3 to 5 calls per install, then zero.
- **Quota check** (`/me/usage`): once per Settings open, not on a timer.
- **Budget:** 2,000 / 30 days = about 66 calls per day. At 2 calls per refresh that is about 30 refreshes per day, or about 1 per 30 minutes all day. With the 15-minute cache and "at most once per 10 minutes" import trigger, a realistic daily use (5 to 10 refreshes) is 10 to 20 calls per day, about 300 to 600 per month. The Free plan is enough. Keep a local counter from `X-Quota-Used` as a safety net.
- **Endpoints per session:** at app start about 2 (profile + operators), plus catalogue calls only when the cache is cold. A single session stays well inside the 30-per-minute limit.

Budget is an estimate; the real call cost of Full Stats V2 and the R6-specific windows are **unverified**.

---

## 6. Terms and use

From https://arenyze.com/terms (read 2026-11-07):

- **API use:** not stated in the Terms. The Terms link to developer docs, which were read separately.
- **Keys (storage or sharing):** not stated in the Terms. The docs say keep keys on the server and do not expose them in apps, and that each account has one active key.
- **Attribution / credit:** not stated. The Terms only say that original content on Arenyze is Arenyze's property.
- **Rate limits:** not stated in the Terms. The docs give the limits (section 2).
- **Data use:** the Terms say "Statistics shown on Arenyze trackers are derived from publicly available game data and third-party APIs." This does not say Arenyze has rights to the data for other uses. The data's upstream source is therefore not named.
- **Redistribution:** not stated.
- **Third-party apps:** not stated for the API. The Terms prohibit "Using any automated means (crawlers, scrapers) to access the Service without express permission" (section 4), and "Overloading, disrupting, or degrading the Service". Whether a desktop client calling the documented API counts as "automated means" is **unverified**; the API docs appear to allow it. Ask Arenyze or read the full docs before shipping.
- **Privacy:** https://arenyze.com/privacy was not read (out of scope; it is linked from the docs). Open question.

---

## 7. Known discrepancies between the sources

1. **Platform value case:** docs `Uplay`, SDK example `uplay`.
2. **Operators V2 modes:** docs list `All, ranked, standard, quick-match, dual-front, siege-cup`; SDK adds `unranked`, `casual`.
3. **Service status path:** docs `servicestatus`, SDK `serviceStatus`.
4. **Seasons params:** SDK adds `weapons`, `description`, `startDate` not in docs.
5. **Profile V2 description vs example:** description says rank-points history; example is empty.
6. **Error shape:** the errors page documents `{error, message, ...}` for THE FINALS / general, but R6 endpoints use `{"error": "<text>"}`.
7. **Free plan calls:** the premium page says "2,000 per month"; the limits page says the same. Consistent.

---

## 8. Open questions (unverified, to answer before coding)

1. Does Profile V2 for a real account (populated `seasons` and `history`) include rank name, RP, max rank, season K/D and W/L? (The docs sample is empty.)
2. Which rank table does Arenyze use, and does the rank name string match `Emerald II` style? Which `version` (`v1`..`v7`) matches the current season (Y11S3)?
3. Does `Full Stats V2` give season K/D, W/L and match count inside `segments`? Shape of `data` segments?
4. Exact R6 error body for 404 (player not found) and 429 (per-minute and monthly).
5. Per-hour and per-day R6 limits.
6. Does `/me/usage` count toward the monthly quota?
7. Is `platformType` case-sensitive (`uplay` vs `Uplay`)?
8. Does the Free key have full R6 access (no paid gate on any R6 endpoint)?
9. Does Arenyze allow a desktop client to hold a user-supplied key (the docs say keep keys server-side)?
10. Is there any attribution requirement for showing Arenyze data in the app?
11. What is the data source for `/ranks` MMR thresholds, and is it updated each season?
12. Rank history: is there a usable time series (`history.data.history`) in a real response?
13. Is Leaderboard `id` a player name or internal ID?
14. Are there hard caching or freshness rules (e.g. data updated every N minutes)? Not stated.

Each open question needs one real response from a key the user owns, saved as a fixture. Do not run these from the worker.
