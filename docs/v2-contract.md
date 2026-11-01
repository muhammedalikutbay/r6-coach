# v2 data contract (Rust side)

The TypeScript source of truth is `app/src/lib/v2/types.ts`. The Rust structs mirror it one to one.

## Serialization
- Every struct and enum: `#[derive(Serialize, Deserialize)]` with `#[serde(rename_all = "camelCase")]`.
- Unit enums (`range`, `mode`, `side`, `source`, `kind`, `side` of a round) serialize as lowercase strings: `last1 | last5 | last20 | season | all`, `all | ranked | unranked | quick`, `both | attack | defense`, `career | replays`, `praise | improve | info`. Use `#[serde(rename_all = "lowercase")]`.
- Timestamps are ISO 8601 UTC strings. `MatchRow.mode` is a display string ("Ranked", "Unranked", "Quick Match", or the raw value if unknown).
- `operators` / `maps` in `Filter`, and `key` everywhere, are lowercase ids without accents (`jager`, not `Jäger`). The display `name` is separate. Unknown ids must never fail: keep the raw id as key and use a readable fallback name.
- `Option<T>` fields (`deltaKd`, `operatorKey`, `mapKey`) are omitted when `None` (`skip_serializing_if = "Option::is_none"`).
- Numbers: percentages are 0–100 (not 0–1) with at most 1 decimal; ratios (K/D etc.) at most 2 decimals. Rounding happens in Rust.
- `killerTeam` and `winnerTeam` are `0` (the tracked player's team) or `1`; model them as `u8`.

## Commands (Tauri, snake_case names, camelCase argument keys)
| Command | Arguments | Returns |
|---|---|---|
| `query` | `filter: Filter` | `QueryResult` |
| `list_matches` | `filter: Filter, page: u32` | `Vec<MatchRow>` (newest first, page size 20) |
| `match_detail` | `match_id: String` (JS key `matchId`) | `MatchDetail` |
| `coach` | `filter: Filter` | `CoachReport` |

Tauri maps the JS key `matchId` to the Rust parameter `match_id` automatically. Errors are returned as `Result<_, String>` with a short readable message.

## Shapes (summary)
- `Filter { range, mode, side, operators: [key], maps: [key] }`
- `QueryResult { source, matchCount, kpis, trend[], operators[], maps[], recentMatches[] }`
  - `Kpis { kd, winPct, hsPct, killsPerRound, fkFdRatio, clutchPct, survivalPct, rounds, matches }`
  - `TrendPoint { index, timestamp, matchId, kd, won }` (oldest first; `kd` is a rolling K/D over up to 5 matches)
  - `OperatorCard { key, name, side, rounds, winPct, kd, hsPct, deltaKd? }`
  - `MapCard { key, name, matches, winPct, attackWinPct, defenseWinPct, kd }`
- `MatchRow { matchId, timestamp, mapKey, mapName, mode, won, scoreUs, scoreThem, kills, deaths, hsPct, badges[] }`
- `MatchDetail { match: MatchRow, teams: [ScoreboardRow[], ScoreboardRow[]], rounds: RoundDetail[] }` (`teams[0]` is the tracked player's team)
  - `ScoreboardRow { username, isMe, operatorsByRound[], kills, deaths, hsPct, fk, fd, clutchesWon, clutchesLost, multiKills, kostPct }`
  - `RoundDetail { number, winnerTeam, winCondition, site, mySide, kills[] }`
  - `KillEventView { timeS, killer, killerOperator, victim, victimOperator, headshot, killerTeam }`
- `CoachReport { summary, insights[], strengths[], workOn[], comparisons[], focusLabel, referenceLabel }`
  - `Insight { kind, title, text, evidence, operatorKey?, mapKey? }`
  - `Comparison { metric, focus, reference, delta, better }` (`better` already accounts for metrics where lower is better)

## Semantics
- `source = career` when the filter only uses mode, side and `range = all` (baseline + replays); otherwise `replays`. The UI shows "based on N matches from replays" using `matchCount`.
- No assists, weapons or RP anywhere in the contract (the parser cannot provide them).
- Fields the data cannot provide must be omitted from the type, not filled with fake values.
