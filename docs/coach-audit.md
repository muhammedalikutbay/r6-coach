# Coach data audit (r6k-ljo.2)

Research only. No code was changed. This audit answers one question from `docs/v4-spec.md` (decision 2: the coach compares everything we can get from `.rec` replays, ranked by importance): which data does the replay parser extract, which does the Rust core read or derive, which does the UI show, and which does the coach compare.

## Method and sample

- Parser: `tools/r6-dissect/dissect/*.go`, read from source, then checked against real output. The patched binary (`R6Koc\target\release\r6-dissect.exe`, JSON mode) was run on all 30 match folders of the local replay set (168 round files, one ranked week, season `Y11S3`, 39 rounds with a planted bomb). Outputs were written to the session scratchpad only. Nothing from the replay data is in this repository.
- Core: `crates/r6k-core/src/{raw,full,derive,stats,query,coach,advice}.rs`.
- UI: `app/src/routes/{overview,matches,coach}/+page.svelte`, `app/src/lib/v2/components/*.svelte`, `app/src/lib/v2/types.ts`.
- Counts below are from the 168-round sample unless stated otherwise.

## 1. Data coverage table

Legend: Yes = present; No = absent; "Dead" = present in the output but always empty or zero.

| # | Data | Parser output | Core reads | Core derives | UI shows | Coach compares |
|---|------|---------------|------------|--------------|----------|----------------|
| 1 | Match id, map, match type, game mode, timestamp | Yes | Yes (`FullMatch`) | Map key, mode filter, local time | Match list, map cards, mode label | Map cards only (filter, not compared) |
| 2 | Season (from game version) | Yes (`gameVersion`) | Yes (`season_of`) | Newest-season filter | Not shown | No |
| 3 | Round number, rounds per match, overtime fields | Yes | Round number only | Round order | Round tabs | No |
| 4 | Team role (Attack / Defense) | Yes | Yes | `my_side`, side per player | Side icon, round side label | Attack and defense round win % |
| 5 | Team won flag and team score | Yes | Yes | Round winner, match score, match won | Match score, Won/Lost per round | Win %, rounds |
| 6 | Team `winCondition` (`DefusedBomb`, `DisabledDefuser`) | Yes, but set on only 39 of 336 team-rounds | Yes | Round win condition (elimination, planted, defused, disabled, time) | Round icon and text | No |
| 7 | Round site | Yes, 168/168 non-empty, 36 distinct strings | Yes (`site`) | Nothing | Round detail header | No |
| 8 | Player list (profile id, name, team index) | Yes | Yes | Player table, "my team" | Scoreboard | Indirectly (filters) |
| 9 | Operator per round (final value after swaps) | Yes | Yes (`operator_id`) | Operator K/D, win %, rounds | Scoreboard operator icons, operator cards | Operator K/D insight cards |
| 10 | Player spawn text (`players[].spawn`) | Yes | No | No | No | No |
| 11 | Kill events (killer, victim, round time, headshot) | Yes, 1,192 kills, 620 headshots | Yes | FK/FD, multi-kills, clutches, traded deaths, KOST, K/D, HS% | Kill feed, scoreboard, KPIs | K/D, kills per round, HS %, FK/FD, multi-kill rounds, clutch % |
| 12 | Kill weapon: `weaponId`, `weaponSlot`, `weaponItemId` | Yes; item id on 1,149 kills, slot on 1,179 | Yes | Weapon name via table (1,132 of 1,192 kills named) | Weapon name in kill feed; slot as tooltip | No |
| 13 | Death events without a killer (`Death`) | Yes, 7 | No (death comes from `stats.died`) | Nothing | No | No |
| 14 | Per-round kills, died flag, headshots | Yes | Yes | Deaths, survival, KOST | Scoreboard, KPIs | Survival % and rates |
| 15 | Per-round assists | Dead: field present, 2 non-zero of 1,669 rows | Read in `raw.rs`, not used | None | No | No |
| 16 | Per-round score | Dead: field present, 0 in every row | Not read | None | No | No |
| 17 | Per-round 1vX (`1vX`) | Dead: field never present in output | Not read | Clutches come from the core | 1vX column (from clutches) | Clutch % |
| 18 | Operator swap events | Yes, 188 (`timeInSeconds` 2 to 42, i.e. early in the round, likely prep) | Not read | None (only the final operator is used) | No | No |
| 19 | Defuser plant complete (with player name) | Yes, 39 | Kind only, for the "planted" win condition | Round planted flag | No | No |
| 20 | Defuser disable complete | Yes, 18 | Not read | None | No | No |
| 21 | Defuser start (plant or disable) | Absent from output (0) | No | No | No | No |
| 22 | Locate objective, BattlEye, player left, other | Not emitted: gated off for Y9S1 and later (`feedback.go`) | No | No | No | No |
| 23 | Trade (traded deaths) | Derived from kill events | Yes (`is_traded`) | Traded deaths per player (`PlayerMatchStats`) | Not shown (not in `ScoreboardRow`) | No |
| 24 | KOST | Derived | Yes | `kost_pct` | Scoreboard column only | No |
| 25 | Survival % | Derived | Yes | `survival_pct` | Not shown on overview (field exists in `Kpis`) | Survival % |
| 26 | Clutches (1vX, won or lost, opponent count) | Derived | Yes (`round_clutches`) | Clutch %, best clutch badge | Badge in match list, 1vX column | Clutch % |
| 27 | Multi-kill rounds (2K to 5K) | Derived | Yes | Multi-kill rounds | Badge and multi column | Multi-kill rounds % |
| 28 | FK / FD | Derived | Yes | Ratio FK/FD (not a rate) | Overview KPI, scoreboard | FK/FD ratio |
| 29 | Attack and defense round win % | Derived | Yes | Per-side round wins | Map cards (ATK/DEF) | ATK and DEF win % |
| 30 | Map win %, map K/D | Derived | Yes | Map cards | Overview map cards | Map round-win insight cards |
| 31 | Kill time (`timeInSeconds`) | Yes | Yes (`time_s`) | Ordering, trade window | Kill feed time | No |
| 32 | Loadouts (carried weapons, gadgets) | Parsed in `loadout.go` to classify kill weapons; inventory not output | No | No | No | No |
| 33 | Nemesis (who kills you most), most-killed rival | Derivable from kill events | Not computed | No | No | No |
| 34 | Death by enemy operator | Derivable (killer operator at kill time) | Not computed | No | No | No |
| 35 | Plant and defuse credit per player | Name on event (19, 20) | Not read | No | No | No |

Coach comparisons today (`coach.rs`, `METRICS`): K/D, kills per round, HS %, win %, FK/FD, clutch %, survival %, multi-kill rounds %, attack round win %, defense round win %. Plus operator K/D cards, map round-win cards, and a best or worst recent match card (`info_cards`). Each metric has a minimum sample (focus: 5 rounds for K/D, KPR, survival, multi and HS kills; 3 matches for win; 4 duels for FK/FD; 2 clutches; 6 rounds per side). The reference needs twice that.

Stale comments to fix when the parser fields are used: the header of `coach.rs` ("no weapon or assist claims (the parser has neither)") and `advice.rs` ("no weapon data") are out of date: the parser now emits weapon fields.

## 2. Gaps ranked by coaching value

Smoothing and the sample gate: a card needs at least `N_min` events in the focus (the reference needs about twice that). Wording becomes firm at 20 rounds, matching `Confidence` in `stats.rs`. Rates are binomial: at `N_min = 10`, the 95% interval is about plus or minus 30 points, so cards must say "so far" and show the counts.

### High value

**G1. Opening duel (entry) win rate, per side.**
- Formula: `entry_win = fk / (fk + fd)` over rounds where the player was in the first kill or first death. Per side: split by `my_side`.
- Source: `RoundPlayer.first_kill`, `first_death`, `side`. Already derived; not exposed as a rate.
- Minimum: 10 duels in focus, 20 in reference.
- Sentence: "You won 6 of 16 opening duels on attack (38%), against 52% usually. Your first contact is costing rounds."
- Why high: direct link to round outcome, controllable. The current `FK/FD` ratio has no sample logic and returns `fk` when `fd = 0`.

**G2. Trade rate and trades given.**
- Formula: `traded_pct = traded_deaths / deaths`; `trades_given = my kills V where V killed a teammate at time t_d with 0 <= t_d - t_kill <= 10`.
- Source: `derive::is_traded` (exists, 10 s window), kill events. `traded_deaths` is computed in `PlayerMatchStats` but not exposed to query or UI.
- Minimum: 10 deaths (traded rate); 10 teammate deaths (trades given).
- Sentence: "Only 3 of your 12 deaths were traded (25%), against 40% usually. Stay within trading distance on your entries."
- Why high: teamwork and positioning, a common coaching point, and the logic is already written and tested.

**G3. Round impact (KOST) and no-impact rounds.**
- Formula: `KOST = rounds with kill, survival or traded death / rounds`. No-impact rate = `died and no kill and no trade / rounds`.
- Source: derived, already in `kost_pct`. Shown only in the scoreboard column.
- Minimum: 10 rounds in focus, 20 in reference.
- Sentence: "In 7 of 20 rounds (35%) you died with no kill and no trade. Your usual is 22%."
- Why high: one number summarises impact over all rounds, including deaths that cost the team.

**G4. Side split per map (attack and defense win rate per map).**
- Formula: `win_side_map = rounds won on side S on map M / rounds on side S on map M`.
- Source: `FullRound.my_side`, `winner_team`, `map_id`. Map cards already show ATK/DEF per map, but the coach does not compare per map and side.
- Minimum: 10 rounds per (map, side) in focus, 20 in reference.
- Sentence: "On a given map your defense wins 4 of 10 rounds (40%), against 58% usually. Check your setups on that map."
- Why high: map and side are the main structure of Siege play, and the data for it is complete.

**G5. How rounds are lost (round end type, by side).**
- Formula: for lost rounds on side S, the share by end type: `elimination`, `time`, `planted`, `defused`, `disabled`.
- Source: `FullRound.win_condition` (derived: elimination, time, planted, defuser events). The data is complete for the winner side because `win_condition` is derived for every round.
- Minimum: 10 lost rounds on side S in focus.
- Sentence: "6 of your 9 lost attack rounds ended on time: the bomb was not planted. Plan a faster first contact on attack."
- Why high: separates "we lost fights" from "we ran out of time", which tells the player what to fix.

### Medium value

**G6. Death cost: team round win rate when you die.**
- Formula: `after_death_win = rounds won by my team / rounds where I died`; compare with `after_survive_win` (rounds where I survived).
- Source: `RoundPlayer.deaths`, `winner_team`.
- Minimum: 10 deaths in focus, 20 in reference.
- Sentence: "Your team won only 3 of 11 rounds after you died early (27%), against 41% usually."
- Caveat: not causal; the early-death timing (G9) should be checked first.

**G7. Weapon usage and headshot rate by weapon (kills only).**
- Formula: `hs_weapon = headshot kills with weapon W / kills with weapon W`; `share_primary = primary kills / all kills`; `share_secondary` similarly.
- Source: `KillEvent.weapon`, `weapon_slot`, `headshot`. Weapon names cover 95% of kills (1,132 of 1,192). Slot is known on 99% of kills (13 unknown).
- Minimum: 20 kills per weapon for HS%, 30 kills overall for slot shares.
- Limitation: weapon-level K/D is not possible: the parser has no weapon for deaths (killer weapon only). Only kill-side metrics can be coached.
- Sentence: "Your primary R4-C gives 58% headshots over 24 kills, against 40% usually."
- Why medium: useful, but a single weapon's kill sample is small and it does not show the cause of deaths.

**G8. Site-level defense results.**
- Formula: `defense_win_site = defense rounds won on site S / defense rounds on site S`.
- Source: `FullRound.site`, `my_side`, `winner_team`.
- Minimum: 10 defense rounds per site. In this sample 36 distinct sites share 168 rounds, so almost no site reaches 10. Only aggregate by map (G4) at the moment.
- Why medium: strong coaching signal when the data is large enough (a few weeks of ranked), not yet.

**G9. Time buckets of deaths and kills.**
- Formula: `early_death_share = deaths with time_s >= 150 / deaths` (first 30 s of a 3-minute timer; the largest kill time in the sample is 173 s); `late_death_share = deaths with time_s <= 30 / deaths`.
- Source: `KillEvent.time_s`. Round timer length to be confirmed on the rec before the cards ship.
- Minimum: 10 deaths.
- Sentence: "4 of your 10 deaths came in the first 30 seconds, against 25% usually. Consider a slower first peek."

**G10. Enemy operator that kills you most (nemesis operator).**
- Formula: `deaths_by_op_O = my deaths where killer operator is O`; compare with the share expected from how often O was on the enemy team.
- Source: kill events plus operator per round (both present).
- Minimum: 5 deaths to a single operator (counts, not rates; no per-player names in cards).
- Sentence: "Enemy operator X killed you in 4 of your 10 deaths this week."
- Why medium: useful for pick decisions, but noisy and it depends on enemy picks. It is about another player's pick, so keep it at operator level.

**G11. Clutch success by number of opponents (1v1 to 1v5).**
- Formula: `clutch_win_vs_N = clutches won with N opponents / clutches with N opponents`.
- Source: `Clutch.vs`, `won` (derived).
- Minimum: 3 clutch situations per bucket.
- Why medium: clutch % already exists overall; splitting by N shows whether the problem is 1v1 or 1v3.

### Low value

**G12. Operator swap habits.** Swap rate per round, and win % in rounds with a swap compared with rounds without. Source: `OperatorSwap` events (188). Minimum: 15 rounds per bucket. Low, because prep-phase swaps have a small direct effect and the data is noisy.

**G13. Plant and defuse credit.** `plants_by_me / attack rounds with a planted bomb`, `disables_by_me / defense rounds with a defuser event`. Source: `DefuserPlantComplete` and `DefuserDisableComplete` carry the player name (not read yet). Minimum: 10 opportunities. Caveat: 4 plant events at time under 5 s, and 2 disable events under 5 s; check the timer logic before using them.

**G14. Team composition.** Win % by number of attackers or defenders with a given role on the player's team. Source: operators per player per round (present). Minimum: 15 rounds per bucket. Low: confounded by map and side.

**G15. Death without killer.** 7 events; likely fall damage or self-kills, and the core already treats them via `died`. No coaching value; ignore.

### Parser gaps (data we should have, but the parser does not output yet)

These are not coaching gaps today, but they block higher-value metrics.

- **P1. Per-round score (`score`) is always 0.** If the in-game score is recovered (it is an ACS-like all-round metric), it would be the strongest single impact measure. High value once fixed. Look at `scoreboard.go` (`readScoreboardScore`) first.
- **P2. Assists are almost always 0** (2 non-zero of 1,669 rows). Assists matter for support roles. Look at `readScoreboardAssists`.
- **P3. `1vX` per round is never emitted.** The core derives clutches itself, so no loss; keep the derived version.
- **P4. Defuser start events are absent** (0 in output). Needed for the plant and defuse timing (not needed for G13).
- **P5. Locate objective, BattlEye, player left and other feedback are gated off for Y9S1 and later.** Disconnect detection is not possible on the current season. Low priority.
- **P6. Loadouts** are read internally (`loadout.go`) only to classify kill weapons. Carried weapons and gadgets per round would enable a loadout-to-win comparison; not in this turn.
- **P7. Player spawn** (`players[].spawn`) is output but not read; it could back a defender spawn split.

Not in this turn (per the task): positions and other `.rec` content outside the parser (for example, player locations). They are not verified here.

## 3. Importance score for coach insights

The current code (`coach.rs`, `metric_candidate`, `operator_candidates`, `map_candidates`) already ranks cards by `score = |relative change| * n / (n + K)`, with `K = 10` rounds (`K_MATCH = 3` for matches). That is a good base: it is explainable and it rewards larger samples. Three changes make it comparable across metric types:

**Score = E x C x W**, with a gate that must pass first.

1. **Gate.** Keep the card only if `n >= N_min` and `|change| >= T`. `T` is 15% relative for ratios (the current `REL_THRESHOLD`) and 5 percentage points for rates.
2. **Effect E = min(1, |change| / S)**, where `S` is the change that counts as a full effect: 10 points for rates (win %, duels, KOST, trade %, side win %), 0.25 for K/D, 0.15 for kills per round. Capping at 1 stops one huge but small-sample swing from dominating, and makes rates and ratios comparable.
3. **Confidence C = n / (n + N_min).** This uses the gate minimum as the smoothing constant. C is 0.5 at the gate, 0.75 at three times the gate, and 0.9 at nine times the gate.
4. **Weight W** (controllability and link to round outcome):
   - 1.5: opening duels (G1), trades (G2).
   - 1.2: KOST (G3), side split by map (G4), round end type (G5).
   - 1.0: death cost (G6), weapon HS% (G7), death timing (G9), site results (G8).
   - 0.8: K/D, win %, map and operator K/D cards (outcome-heavy, teammates affect them).
   - 0.6: nemesis operator (G10), swaps (G12), plant and defuse credit (G13), team composition (G14).
   - 0.7: clutch by opponents (G11).
5. **Dedupe by family.** At most one card per family: duel, trade, impact, side, round-end, weapon, time, map and operator, outcome. This avoids K/D, KPR and survival all saying the same thing.
6. **Selection.** Top 3 "work on" and top 1 to 2 "praise" by score, then info cards. Ties go to the larger `n`, then to the metric name.

Worked example (illustrative numbers, not measured):

| Card | Change | n | N_min | E | C | W | Score |
|------|--------|---|-------|---|---|---|-------|
| Opening duels, attack: 38% vs 52% usual | -14 pts | 16 | 10 | 1.00 | 0.62 | 1.5 | 0.92 |
| Trades: 25% vs 40% usual | -15 pts | 12 | 10 | 1.00 | 0.55 | 1.5 | 0.83 |
| Defense win on one map: 40% vs 58% usual | -18 pts | 10 | 10 | 1.00 | 0.50 | 1.2 | 0.60 |
| K/D 1.40 vs 1.80 usual | -0.40 | 25 rounds | 10 | 1.00 | 0.71 | 0.8 | 0.57 |

Reading: the duel and trade cards outrank the K/D card even though the K/D change is the same size on the capped scale, because they are closer to the round outcome and their weight is higher.

Note on wording: every card keeps the existing evidence line ("x of n, against y usually") and the existing confidence prefix (describe below 5 rounds, careful below 20).

## 4. Recommended next steps (for the coding task, not this one)

1. Expose `traded_deaths` and `kost` in the query and UI (G2, G3); they are already computed.
2. Add the entry-duel rate and per-map side split (G1, G4); both need only derived data.
3. Fix parser fields P1 and P2 (score, assists), then re-run this audit.
4. Replace the coach's ranking with E x C x W and the family dedupe (section 3).
5. Update the stale "no weapon" comments in `coach.rs` and `advice.rs`.
