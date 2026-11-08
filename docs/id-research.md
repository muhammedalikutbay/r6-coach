# Unknown Siege IDs: research notes (r6k-2a6.1)

Scope: the unknown ids listed in `docs/v5-spec.md` ("Unknown ids found in the sample data"), plus the
full current id gaps in the repo tables. Every mapping below has a source. Anything not confirmed is
marked **unverified**. No ids were invented: an id is only listed as a candidate when a source names
the item, and the id itself is still unverified in that case.

Status legend: **confirmed** = a source ties this exact id to the name; **candidate** = a source names
the item but nothing ties this exact id to it; **unknown** = no source found.

## 1. Map ids

| Id | Status | Finding | Source |
|---|---|---|---|
| 441842102525 | unknown | Not in `data/maps.json`, not in the upstream Go map table. | repo tables; redraskal `map_string.go` |
| 430788891316 | unknown | Same as above. | same |
| 446546313105 | unknown | Same as above. | same |

- Upstream redraskal `dissect/map_string.go` (master) lists 35 map ids. None of the three unknown ids is in it.
  Source: https://raw.githubusercontent.com/redraskal/r6-dissect/master/dissect/map_string.go
- The `wnc-replay/replay-tool` `analysis/names.go` (main branch) has a `// Maps` section, but its map
  entries use a different hex id space and none of the three ids appears. Its only map entry
  matched in our search is `0x644D04D2B4: "Theme Park"`, which is a different id space.
  Source: https://raw.githubusercontent.com/wnc-replay/replay-tool/main/analysis/names.go
- Quick Match pool for Y11 could not be confirmed. The Siege.gg Quick Match article is from an older season.
  Source: https://siege.gg/news/quick-match (outdated, not used for ids)
- Ranked pool 2026 (Hotspawn) confirms Calypso Casino as a new Y11S2 map. It gives no id.
  Source: https://www.hotspawn.com/rainbow-six/news/rainbow-six-siege-x-ranked-map-pool-for-2026

Candidate map names for the three unknown ids: **none that a source ties to an id. All three are unverified.**
A later replay with a known map name and its matchID in Tracker data is the only way to confirm them.

### Full map id gaps in the repo

Repo table: `data/maps.json` (41 ids, the app's source). Upstream Go table: `tools/r6-dissect/dissect/map_string.go` (35 ids).

- Ids in `data/maps.json` but not in the upstream Go table (added by `tools/gen-name-tables`, not by upstream):
  398899676157 (Fortress), 409325881472 (Villa), 434715462383 (Oregon), 436375283234 (Coastline),
  441408792952 (Kanal), 441825219764 (Outback).
- Ids in the upstream Go table but not in `data/maps.json`: none.
- Ids in neither table: 441842102525, 430788891316, 446546313105 (see above).

Map keys in `data/maps.json` with no image in `app/static/assets/maps/`:
`bartlettu`, `herefordbase`, `stadium2020`, `stadiumbravo`.

Image files with no key in `data/maps.json`: `calypsocasino.jpg` (Calypso Casino, Y11S2 map, id
unknown) and `stadiumalpha.jpg` (name and id unknown; may be the same map as `stadiumbravo`, **unverified**).

## 2. Operator game ids

| Id | Status | Finding | Source |
|---|---|---|---|
| 444310693746 | confirmed name (Solid Snake, attacker) | Already in `data/operators.json` and `app/src/lib/v2/data/operators.json` (added by `tools/gen-name-tables`, matched against Tracker). Not in upstream `operator_string.go` or `operator_roles.go`. | https://www.ubisoft.com/en-ca/game/rainbow-six/siege/game-info/operators/solid-snake ; https://news.ubisoft.com/en-us/article/7wYca8Fw674NP93tZe9uDe ; `data/operators.json` |
| 456757346397 | confirmed name (Noor, defender) | Same as above. Not in upstream tables. | https://www.ubisoft.com/en-gb/game/rainbow-six/siege/game-info/operators/noor ; https://news.ubisoft.com/en-gb/article/4qqpGJZSWrrS3Ko2hYvviK/rainbow-six-siege-operation-split-fire-new-operator-noor-3v3-arcade-mode-wasteland-circuit-event-and-more |

- The id-to-name link for both comes from the repo's Tracker-matched table (`tools/gen-name-tables/gen.mjs`).
  The Ubisoft and news pages confirm the operator name and side, not the id.
- Solid Snake was added on 3 March 2026 (Operation Silent Hunt). Noor was added in Operation Split Fire (Y11S3).
- Solid Snake's gadget is inconsistent across sources: news post and guides say Soliton Radar MKIII,
  the Ubisoft AU operator page lists a Frag Grenade. **Unverified** which is correct for the loadout.

### Full operator id gaps in the repo

- `tools/r6-dissect/dissect/operator_string.go` (upstream Go table): 77 ids.
- `data/operators.json`: 79 ids.
- In the Go table but not in `operators.json`: none.
- In `operators.json` but not in the Go table: **444310693746 (Solid Snake)** and **456757346397 (Noor)**.
  The Go parser (`operator_roles.go`, `operator_string.go`) does not know these, which is the reason
  for the panic described in `r6k-zrz.2`. Upstream `operator_roles.go` also has no entries for them
  (checked on the redraskal master branch).
- Operator keys in `data/operators.json` without a badge in `app/static/assets/operators/`: none.
- Badge files without a key in `operators.json`: none.

Note on id spaces: `game_operators.go` (generated from wnc-replay) uses hex loadout block item ids
(for example `0x37802D169C: "KALI"`), which is a different id space from the header operator ids above.
It contains no entry for Solid Snake or Noor, so their loadout blocks cannot be named from that table.
It also contains `0x64D62DB918: "DIAMOND"`, which has no matching operator in the app tables. Not
investigated further.

## 3. Weapon and gadget item ids

Repo table: `data/weapons.json` (814 entries). Kinds in that file come from wnc-replay `analysis/names.go`.

| Id | Status | Candidate item (kind) | Basis | Source |
|---|---|---|---|---|
| 437464103147 | candidate, id unverified | XK23 (Dokkaebi primary, weapon) | Designers' notes: Dokkaebi's primary in Y11S2 is the XK23. The id is not found in any public table we checked. | https://allthings.how/dokkaebis-jegeo-payload-and-xk23-explained-year-11-season-2/ ; https://www.ubisoft.com/en-us/game/rainbow-six/siege/news-updates/2MNhboDBmsQKkWb5p8zqsg |
| 445433462931 | candidate, id unverified | XK23 (Rauora primary, weapon) | Same source says Sens and Rauora also receive the XK23. Another source describes Rauora's original primary as 417 Marksman Rifle / M249. The two sources conflict for Rauora, so this is **unverified**. | https://allthings.how/dokkaebis-jegeo-payload-and-xk23-explained-year-11-season-2/ |
| 454107765692 | candidate, id unverified | MAG-NET SYSTEM (Wamai, kind ability) | Mag-NET is Wamai's ability and is in `data/weapons.json` under other ids (for example 238373638654, 238373639961). A source calls it Wamai's gadget/ability, not a secondary gadget. | https://scienceinsights.org/what-does-wamai-do-mag-net-ability-explained/ (low quality; cross-checked with the repo table) |

- `XK23` is not in `data/weapons.json` (checked by name). Adding it needs its id, which is unknown.
- Wamai's secondary gadget options changed in Y11S2.2 (shield added, impacts replaced by C4), so the
  Wamai gadget id may be a secondary gadget rather than Mag-NET. **Unverified.**
- Side finding, not an id: `data/weapons.json` marks `238373641120` (X-KAIROS) as `"kind": "weapon"`.
  Sources identify X-KAIROS as Hibana's gadget, so the kind looks wrong. Not changed here (no code changes in this task).
  Source: https://www.ubisoft.com/en-us/game/rainbow-six/siege/news-updates/2MNhboDBmsQKkWb5p8zqsg ; https://ginx.tv (Ace guide, which says X-KAIROS is Hibana's)

## 4. Kills with no weapon slot

No replay data was used (the user's real replays must not be committed). The mechanisms below come from
public guides. None is confirmed against a replay kill record, so all are **unverified** as to what the
parser reports.

| Operator | Ability or gadget named in sources | Likely cause of a kill without a weapon slot | Status |
|---|---|---|---|
| Fuze | Cluster Charge (ability; `238373638674` in weapons.json) | Explosion from the cluster charge. | unverified. Source: https://ubisoft.com/en-us/game/rainbow-six/siege/game-info/operators/fuze |
| Thorn | Razorbloom Shell (gadget; not in weapons.json) | Explosion from the deployed shells. | unverified. Source: https://www.pcgamer.com/rainbow-six-siege-gadget-tricks |
| Caveira | Silent Step (gadget/ability; `238373638724` in weapons.json). It is not a weapon. | Kill by interrogation, which is not a weapon. | unverified. Source: https://www.redbull.com/ie-en/rainbow-six-siege-caveira-guide |
| Ace | S.E.L.M.A. Aqua Breacher (gadget; not in weapons.json). Ace's secondary is the P9, not X-KAIROS. | Explosion from the breaching gadget. | unverified. Source: https://www.ginx.tv/en/?p=5828 ; https://liquipedia.net/rainbowsix/Ace |
| Bandit | Battery (gadget; not in weapons.json). Sources describe it as electrifying walls and gadgets, not as a direct damage source. | Unknown. Possibly not a direct kill source. | unknown |
| Wamai | Mag-NET System (ability; `238373639961` in weapons.json). It redirects grenades. | Unknown. A redirected grenade is credited to its owner, not to Wamai. | unknown |

## 5. Map image licensing

- The current images in `app/static/assets/` came from the Tracker CDN (`tools/fetch-assets/fetch.mjs`).
  Operator badges and map images are Ubisoft game art. No open licence is named by the Tracker CDN or by Ubisoft.
- No openly licensed source with these images was found. Sources checked: the Ubisoft game-info pages
  (text and art, all rights reserved by Ubisoft as far as we could see) and the wnc-replay and
  redraskal repos (code only, MIT, no art).
- **Unverified:** whether Ubisoft's press kit allows reuse of these images for a personal, non-commercial app.
  Not checked. Until that is confirmed, keep the existing images as they are and do not add new ones from the CDN.

## Sources

- https://raw.githubusercontent.com/redraskal/r6-dissect/master/dissect/map_string.go
- https://raw.githubusercontent.com/redraskal/r6-dissect/master/dissect/operator_string.go
- https://raw.githubusercontent.com/redraskal/r6-dissect/master/dissect/operator_roles.go
- https://raw.githubusercontent.com/wnc-replay/replay-tool/main/analysis/names.go
- https://github.com/wnc-replay/replay-tool (main branch, 16 commits; commit hash not read)
- https://www.ubisoft.com/en-ca/game/rainbow-six/siege/game-info/operators/solid-snake
- https://news.ubisoft.com/en-us/article/7wYca8Fw674NP93tZe9uDe
- https://www.ubisoft.com/en-gb/game/rainbow-six/siege/game-info/operators/noor
- https://news.ubisoft.com/en-gb/article/4qqpGJZSWrrS3Ko2hYvviK/rainbow-six-siege-operation-split-fire-new-operator-noor-3v3-arcade-mode-wasteland-circuit-event-and-more
- https://allthings.how/dokkaebis-jegeo-payload-and-xk23-explained-year-11-season-2/
- https://www.ubisoft.com/en-us/game/rainbow-six/siege/news-updates/2MNhboDBmsQKkWb5p8zqsg
- https://scienceinsights.org/what-does-wamai-do-mag-net-ability-explained/
- https://www.ginx.tv/en/?p=5828
- https://liquipedia.net/rainbowsix/Ace
- https://ubisoft.com/en-us/game/rainbow-six/siege/game-info/operators/fuze
- https://www.redbull.com/ie-en/rainbow-six-siege-caveira-guide
- https://www.pcgamer.com/rainbow-six-siege-gadget-tricks
- https://www.hotspawn.com/rainbow-six/news/rainbow-six-siege-x-ranked-map-pool-for-2026
- https://siege.gg/news/quick-match (outdated; not used for ids)
- Repo: `data/maps.json`, `data/operators.json`, `data/weapons.json`, `tools/r6-dissect/dissect/{map_string,operator_string,game_operators}.go`, `tools/gen-name-tables/gen.mjs`, `tools/gen-weapons/main.go`, `tools/fetch-assets/fetch.mjs`, `docs/arenyze-api.md` (lists /maps and /weapons endpoints with fields only, no id lists)
