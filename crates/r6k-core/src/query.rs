//! v2 query engine (spec sections 3-4): the global filter, `query`, `list_matches` and
//! `match_detail`. Pure logic over [`FullMatch`]; the DTOs serialize to the TypeScript contract in
//! `app/src/lib/v2/types.ts` (camelCase, see `docs/v2-contract.md`).
//!
//! # Filter semantics
//! * `mode`, `maps` and `range` select **matches**. `last1/5/20` take the newest N matches that
//!   pass the other filters; `season` keeps matches from [`SEASON_START`] on.
//! * `side` and `operators` select **rounds** of the tracked player. All numbers inside the
//!   filtered set count only those rounds. A match with no qualifying round drops out of the set.
//!
//! # Source (`career` vs `replays`)
//! `career` (baseline merged with post-capture replays) is used only when the filter has no
//! operator and no map, `range` is `all` or `season`, and `mode` and `side` are not both set (the
//! baseline has no mode-by-side split). Everything else is computed from local replays.
//! The baseline has no round counts for gamemodes or the overall block, so in `career` mode the
//! round-based KPIs (kills/round, FK/FD, survival) come from the per-operator totals (side-aware),
//! while K/D, win % and matches come from the overall/gamemode block. Clutch % has no baseline
//! counterpart (only wins are stored) and always comes from replays. Trend and recent matches are
//! always replay based.

use crate::baseline::{Baseline, CounterSet};
use crate::derive::{is_traded, player_match_stats, round_clutches, PlayerMatchStats};
use crate::full::{FullMatch, FullRound, RoundPlayer, WinCondition};
use crate::model::{Match, Round, Side};
use crate::names;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// First instant (UTC, `YYYY-MM-DDTHH:MM:SS`) of the current ranked season. Matches before it
/// are outside `range = season`. Assumption: Y11S3 started on 2026-09-02; adjust when it moves.
pub const SEASON_START: &str = "2026-09-02T00:00:00";

/// Matches per `recentMatches` list.
pub const RECENT_MATCHES: usize = 10;
/// Window of the rolling K/D trend, in matches.
pub const TREND_WINDOW: usize = 5;

// ---------------------------------------------------------------- filter

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Range {
    Last1,
    Last5,
    Last20,
    Season,
    #[default]
    All,
}

impl Range {
    fn last_n(self) -> Option<usize> {
        match self {
            Range::Last1 => Some(1),
            Range::Last5 => Some(5),
            Range::Last20 => Some(20),
            Range::Season | Range::All => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Mode {
    #[default]
    All,
    Ranked,
    Unranked,
    Quick,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SideFilter {
    #[default]
    Both,
    Attack,
    Defense,
}

/// Side of an operator card or round.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TeamSide {
    Attack,
    Defense,
}

/// Global filter shared by every view. `operators` and `maps` hold lowercase `key`s.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Filter {
    pub range: Range,
    pub mode: Mode,
    pub side: SideFilter,
    pub operators: Vec<String>,
    pub maps: Vec<String>,
}

// ---------------------------------------------------------------- DTOs

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Source {
    Career,
    Replays,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Kpis {
    pub kd: f64,
    pub win_pct: f64,
    pub hs_pct: f64,
    pub kills_per_round: f64,
    pub fk_fd_ratio: f64,
    pub clutch_pct: f64,
    pub survival_pct: f64,
    pub rounds: u32,
    pub matches: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TrendPoint {
    pub index: u32,
    pub timestamp: String,
    pub match_id: String,
    pub kd: f64,
    pub won: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OperatorCard {
    pub key: String,
    pub name: String,
    pub side: TeamSide,
    pub rounds: u32,
    pub win_pct: f64,
    pub kd: f64,
    pub hs_pct: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub delta_kd: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MapCard {
    pub key: String,
    pub name: String,
    pub matches: u32,
    pub win_pct: f64,
    pub attack_win_pct: f64,
    pub defense_win_pct: f64,
    pub kd: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MatchRow {
    pub match_id: String,
    pub timestamp: String,
    pub map_key: String,
    pub map_name: String,
    pub mode: String,
    pub won: bool,
    pub score_us: u32,
    pub score_them: u32,
    pub kills: u32,
    pub deaths: u32,
    pub hs_pct: f64,
    pub badges: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct QueryResult {
    pub source: Source,
    pub match_count: u32,
    pub kpis: Kpis,
    pub trend: Vec<TrendPoint>,
    pub operators: Vec<OperatorCard>,
    pub maps: Vec<MapCard>,
    pub recent_matches: Vec<MatchRow>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScoreboardRow {
    pub username: String,
    pub is_me: bool,
    pub operators_by_round: Vec<String>,
    pub kills: u32,
    pub deaths: u32,
    pub hs_pct: f64,
    pub fk: u32,
    pub fd: u32,
    pub clutches_won: u32,
    pub clutches_lost: u32,
    pub multi_kills: u32,
    pub kost_pct: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct KillEventView {
    pub time_s: i64,
    pub killer: String,
    pub killer_operator: String,
    pub victim: String,
    pub victim_operator: String,
    pub headshot: bool,
    /// 0 = my team, 1 = other team.
    pub killer_team: u8,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RoundDetail {
    pub number: u32,
    /// 0 = my team, 1 = other team (also when the winner is unknown).
    pub winner_team: u8,
    pub win_condition: String,
    pub site: String,
    pub my_side: TeamSide,
    pub kills: Vec<KillEventView>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MatchDetail {
    #[serde(rename = "match")]
    pub match_row: MatchRow,
    /// `teams[0]` = my team, `teams[1]` = opponents.
    pub teams: [Vec<ScoreboardRow>; 2],
    pub rounds: Vec<RoundDetail>,
}

// ---------------------------------------------------------------- small helpers

fn r1(x: f64) -> f64 {
    (x * 10.0).round() / 10.0
}
fn r2(x: f64) -> f64 {
    (x * 100.0).round() / 100.0
}
/// Kills per death; zero deaths gives the kill count (same rule as the baseline).
fn ratio_kd(k: u64, d: u64) -> f64 {
    if d == 0 {
        k as f64
    } else {
        k as f64 / d as f64
    }
}
fn pct(n: u64, d: u64) -> f64 {
    if d == 0 {
        0.0
    } else {
        100.0 * n as f64 / d as f64
    }
}

/// Operator key for a round player; unknown IDs keep the raw ID.
fn op_key(id: u64) -> String {
    names::operator_key(id).unwrap_or_else(|| id.to_string())
}
fn op_name(key: &str) -> String {
    names::operator_name_by_key(key).unwrap_or_else(|| match key.parse::<u64>() {
        Ok(id) => format!("Operator #{id}"),
        Err(_) => pretty_slug(key),
    })
}
fn map_key_of(m: &FullMatch) -> String {
    names::map_key(m.map_id).unwrap_or_else(|| m.map_id.to_string())
}
fn map_name_of(key: &str) -> String {
    names::map_name_by_key(key).unwrap_or_else(|| match key.parse::<u64>() {
        Ok(id) => format!("Map #{id}"),
        Err(_) => pretty_slug(key),
    })
}
fn pretty_slug(s: &str) -> String {
    let mut out = String::new();
    for (i, w) in s.split(['-', '_', ' ']).filter(|w| !w.is_empty()).enumerate() {
        if i > 0 {
            out.push(' ');
        }
        let mut c = w.chars();
        if let Some(f) = c.next() {
            out.extend(f.to_uppercase());
            out.push_str(c.as_str());
        }
    }
    out
}
/// Lowercase alphanumerics only ("kafe-dostoyevsky" == "kafedostoyevsky").
fn norm_slug(s: &str) -> String {
    s.chars().filter(|c| c.is_ascii_alphanumeric()).map(|c| c.to_ascii_lowercase()).collect()
}

/// Mode of a dissect match type string; `None` for modes the filter does not know.
fn mode_of(match_type: &str) -> Option<Mode> {
    let t = match_type.to_lowercase();
    if t.contains("unranked") || t.contains("standard") {
        Some(Mode::Unranked)
    } else if t.contains("ranked") {
        Some(Mode::Ranked)
    } else if t.contains("quick") || t.contains("casual") {
        Some(Mode::Quick)
    } else {
        None
    }
}
fn mode_label(match_type: &str) -> String {
    match mode_of(match_type) {
        Some(Mode::Ranked) => "Ranked".to_string(),
        Some(Mode::Unranked) => "Unranked".to_string(),
        Some(Mode::Quick) => "Quick Match".to_string(),
        _ => match_type.to_string(),
    }
}
fn ts_prefix(s: &str) -> &str {
    s.get(..19).unwrap_or(s)
}

fn team_side(s: Side) -> Option<TeamSide> {
    match s {
        Side::Attack => Some(TeamSide::Attack),
        Side::Defense => Some(TeamSide::Defense),
        Side::Unknown => None,
    }
}

// ---------------------------------------------------------------- round-level selection

/// The tracked player's entry in `round` if the round passes the side and operator filters.
fn selected<'a>(f: &Filter, round: &'a FullRound, me: &str) -> Option<&'a RoundPlayer> {
    let p = round.players.iter().find(|p| p.profile_id == me)?;
    match f.side {
        SideFilter::Both => {}
        SideFilter::Attack if p.side != Side::Attack => return None,
        SideFilter::Defense if p.side != Side::Defense => return None,
        _ => {}
    }
    if !f.operators.is_empty() {
        let key = op_key(p.operator_id);
        if !f.operators.iter().any(|k| k.eq_ignore_ascii_case(&key)) {
            return None;
        }
    }
    Some(p)
}

fn has_round_filter(f: &Filter) -> bool {
    f.side != SideFilter::Both || !f.operators.is_empty()
}

/// Matches that pass the filter, oldest first.
///
/// Mode, map and season select matches; side and operator filters work per round, and a match
/// with no qualifying round is dropped. `last1/5/20` keep the newest N of what remains.
pub fn apply<'a>(f: &Filter, matches: &'a [FullMatch]) -> Vec<&'a FullMatch> {
    let mut v: Vec<&FullMatch> = matches
        .iter()
        .filter(|m| f.mode == Mode::All || mode_of(&m.match_type) == Some(f.mode))
        .filter(|m| {
            f.maps.is_empty() || {
                let k = map_key_of(m);
                f.maps.iter().any(|x| x.eq_ignore_ascii_case(&k))
            }
        })
        .filter(|m| f.range != Range::Season || ts_prefix(&m.timestamp) >= SEASON_START)
        .filter(|m| {
            !has_round_filter(f) || m.rounds.iter().any(|r| selected(f, r, &m.me_profile_id).is_some())
        })
        .collect();
    v.sort_by(|a, b| a.timestamp.cmp(&b.timestamp).then_with(|| a.match_id.cmp(&b.match_id)));
    if let Some(n) = f.range.last_n() {
        let drop = v.len().saturating_sub(n);
        v.drain(..drop);
    }
    v
}

/// Source for a filter (see the module docs).
pub fn source_for(f: &Filter) -> Source {
    let range_ok = matches!(f.range, Range::All | Range::Season);
    let split_ok = !(f.mode != Mode::All && f.side != SideFilter::Both);
    if f.operators.is_empty() && f.maps.is_empty() && range_ok && split_ok {
        Source::Career
    } else {
        Source::Replays
    }
}

// ---------------------------------------------------------------- tallies

#[derive(Debug, Clone, Copy, Default)]
struct Tally {
    rounds: u32,
    rounds_won: u32,
    kills: u32,
    deaths: u32,
    headshots: u32,
    fk: u32,
    fd: u32,
    survived: u32,
    kost: u32,
    clutch_won: u32,
    clutch_lost: u32,
}

impl Tally {
    fn add_round(&mut self, r: &FullRound, p: &RoundPlayer) {
        self.rounds += 1;
        self.rounds_won += (r.winner_team == Some(p.team_index)) as u32;
        self.kills += p.kills;
        self.deaths += p.deaths;
        self.headshots += p.headshots;
        self.fk += p.first_kill as u32;
        self.fd += p.first_death as u32;
        self.survived += p.survived as u32;
        let traded = p.deaths > 0 && is_traded(r, &p.profile_id);
        if p.kills > 0 || p.survived || traded {
            self.kost += 1;
        }
        for c in round_clutches(r).iter().filter(|c| c.profile_id == p.profile_id) {
            if c.won {
                self.clutch_won += 1;
            } else {
                self.clutch_lost += 1;
            }
        }
    }
    fn merge(&mut self, o: &Tally) {
        self.rounds += o.rounds;
        self.rounds_won += o.rounds_won;
        self.kills += o.kills;
        self.deaths += o.deaths;
        self.headshots += o.headshots;
        self.fk += o.fk;
        self.fd += o.fd;
        self.survived += o.survived;
        self.kost += o.kost;
        self.clutch_won += o.clutch_won;
        self.clutch_lost += o.clutch_lost;
    }
    fn kd(&self) -> f64 {
        ratio_kd(self.kills as u64, self.deaths as u64)
    }
    fn hs_pct(&self) -> f64 {
        pct(self.headshots as u64, self.kills as u64)
    }
}

fn tally_match(f: &Filter, m: &FullMatch) -> Tally {
    let mut t = Tally::default();
    for r in &m.rounds {
        if let Some(p) = selected(f, r, &m.me_profile_id) {
            t.add_round(r, p);
        }
    }
    t
}

// ---------------------------------------------------------------- query

/// Converts a full match to the tracked-player model so [`Baseline::merged`] can use it.
fn to_match(m: &FullMatch) -> Match {
    let rounds = m
        .rounds
        .iter()
        .filter_map(|r| {
            let p = r.players.iter().find(|p| p.profile_id == m.me_profile_id)?;
            Some(Round {
                number: r.number,
                side: p.side,
                operator_id: p.operator_id,
                kills: p.kills,
                deaths: p.deaths,
                assists: 0,
                headshots: p.headshots,
                first_blood: p.first_kill,
                first_death: p.first_death,
                planted: false,
                defused: false,
                won: r.winner_team == Some(p.team_index),
            })
        })
        .collect();
    Match {
        match_id: m.match_id.clone(),
        timestamp: m.timestamp.clone(),
        match_type: m.match_type.clone(),
        gamemode: m.gamemode.clone(),
        map_id: m.map_id,
        won: m.won,
        rounds,
    }
}

pub fn query(filter: &Filter, matches: &[FullMatch], baseline: &Baseline) -> QueryResult {
    let set = apply(filter, matches);
    let merged = baseline.merged(&matches.iter().map(to_match).collect::<Vec<_>>());
    let career_kd = ratio_kd(merged.overall.kills, merged.overall.deaths);

    // Replay-based numbers (always used for trend and recent matches).
    let mut total = Tally::default();
    let mut wins = 0u32;
    let mut per_match: Vec<Tally> = Vec::with_capacity(set.len());
    for m in &set {
        let t = tally_match(filter, m);
        total.merge(&t);
        wins += m.won as u32;
        per_match.push(t);
    }
    let kpis_replays = Kpis {
        kd: r2(total.kd()),
        win_pct: r1(pct(wins as u64, set.len() as u64)),
        hs_pct: r1(total.hs_pct()),
        kills_per_round: r2(if total.rounds == 0 { 0.0 } else { total.kills as f64 / total.rounds as f64 }),
        fk_fd_ratio: r2(ratio_kd(total.fk as u64, total.fd as u64)),
        clutch_pct: r1(pct(total.clutch_won as u64, (total.clutch_won + total.clutch_lost) as u64)),
        survival_pct: r1(pct(total.survived as u64, total.rounds as u64)),
        rounds: total.rounds,
        matches: set.len() as u32,
    };

    let trend: Vec<TrendPoint> = set
        .iter()
        .enumerate()
        .map(|(i, m)| {
            let from = (i + 1).saturating_sub(TREND_WINDOW);
            let (k, d) = per_match[from..=i]
                .iter()
                .fold((0u64, 0u64), |a, t| (a.0 + t.kills as u64, a.1 + t.deaths as u64));
            TrendPoint {
                index: i as u32 + 1,
                timestamp: m.timestamp.clone(),
                match_id: m.match_id.clone(),
                kd: r2(ratio_kd(k, d)),
                won: m.won,
            }
        })
        .collect();

    let recent_matches: Vec<MatchRow> =
        set.iter().rev().take(RECENT_MATCHES).map(|m| match_row(m)).collect();

    let source = source_for(filter);
    let (kpis, operators, maps, match_count) = match source {
        Source::Replays => (
            kpis_replays.clone(),
            replay_operator_cards(filter, &set, career_kd),
            replay_map_cards(filter, &set),
            kpis_replays.matches,
        ),
        Source::Career => {
            let kpis = career_kpis(filter, &merged, &kpis_replays);
            let count = kpis.matches;
            let ops = career_operator_cards(filter, &merged, kpis.kd);
            let maps = career_map_cards(filter, &merged);
            (kpis, ops, maps, count)
        }
    };

    QueryResult { source, match_count, kpis, trend, operators, maps, recent_matches }
}

fn replay_operator_cards(f: &Filter, set: &[&FullMatch], career_kd: f64) -> Vec<OperatorCard> {
    let mut by_key: BTreeMap<String, (Tally, Option<TeamSide>)> = BTreeMap::new();
    for m in set {
        for r in &m.rounds {
            if let Some(p) = selected(f, r, &m.me_profile_id) {
                let e = by_key.entry(op_key(p.operator_id)).or_default();
                e.0.add_round(r, p);
                e.1 = e.1.or(team_side(p.side));
            }
        }
    }
    let mut cards: Vec<OperatorCard> = by_key
        .into_iter()
        .map(|(key, (t, side))| OperatorCard {
            name: op_name(&key),
            side: side.unwrap_or(TeamSide::Defense),
            rounds: t.rounds,
            win_pct: r1(pct(t.rounds_won as u64, t.rounds as u64)),
            kd: r2(t.kd()),
            hs_pct: r1(t.hs_pct()),
            delta_kd: Some(r2(t.kd() - career_kd)),
            key,
        })
        .collect();
    cards.sort_by(|a, b| b.rounds.cmp(&a.rounds).then_with(|| a.key.cmp(&b.key)));
    cards
}

fn replay_map_cards(f: &Filter, set: &[&FullMatch]) -> Vec<MapCard> {
    #[derive(Default)]
    struct Acc {
        matches: u32,
        wins: u32,
        kills: u64,
        deaths: u64,
        atk: (u64, u64),
        def: (u64, u64),
    }
    let mut by_key: BTreeMap<String, Acc> = BTreeMap::new();
    for m in set {
        let a = by_key.entry(map_key_of(m)).or_default();
        a.matches += 1;
        a.wins += m.won as u32;
        for r in &m.rounds {
            if let Some(p) = selected(f, r, &m.me_profile_id) {
                a.kills += p.kills as u64;
                a.deaths += p.deaths as u64;
                let won = (r.winner_team == Some(p.team_index)) as u64;
                match p.side {
                    Side::Attack => {
                        a.atk.0 += 1;
                        a.atk.1 += won;
                    }
                    Side::Defense => {
                        a.def.0 += 1;
                        a.def.1 += won;
                    }
                    Side::Unknown => {}
                }
            }
        }
    }
    let mut cards: Vec<MapCard> = by_key
        .into_iter()
        .map(|(key, a)| MapCard {
            name: map_name_of(&key),
            matches: a.matches,
            win_pct: r1(pct(a.wins as u64, a.matches as u64)),
            attack_win_pct: r1(pct(a.atk.1, a.atk.0)),
            defense_win_pct: r1(pct(a.def.1, a.def.0)),
            kd: r2(ratio_kd(a.kills, a.deaths)),
            key,
        })
        .collect();
    cards.sort_by(|a, b| b.matches.cmp(&a.matches).then_with(|| a.key.cmp(&b.key)));
    cards
}

fn side_str(s: SideFilter) -> Option<&'static str> {
    match s {
        SideFilter::Both => None,
        SideFilter::Attack => Some("attacker"),
        SideFilter::Defense => Some("defender"),
    }
}

fn gamemode_key(m: Mode) -> Option<&'static str> {
    match m {
        Mode::All => None,
        Mode::Ranked => Some("pvp_ranked"),
        // The baseline has no unranked block; Tracker folds it into quickplay.
        Mode::Unranked | Mode::Quick => Some("pvp_quickplay"),
    }
}

fn career_kpis(f: &Filter, merged: &Baseline, replays: &Kpis) -> Kpis {
    // Operator totals (side-aware) carry the round counts the other blocks lack.
    let want = side_str(f.side);
    let mut ops = CounterSet::default();
    for c in merged.operators.values() {
        if want.is_none() || c.side.as_deref() == want {
            ops.rounds_played += c.rounds_played;
            ops.rounds_won += c.rounds_won;
            ops.kills += c.kills;
            ops.deaths += c.deaths;
            ops.headshots += c.headshots;
            ops.first_bloods += c.first_bloods;
            ops.first_deaths += c.first_deaths;
        }
    }
    // Block for K/D, win % and matches.
    let (kd, win_pct, matches) = if want.is_some() {
        // No per-side match counts for the whole career: use rounds.
        (ops.kd(), ops.round_win_pct(), merged.overall.matches_played)
    } else if let Some(gm) = gamemode_key(f.mode).and_then(|k| merged.gamemodes.get(k)) {
        (gm.kd(), gm.win_pct(), gm.matches_played)
    } else {
        (merged.overall.kd(), merged.overall.win_pct(), merged.overall.matches_played)
    };
    // Career headshot rate from the operator segments (overall.headshots is not usable).
    let career_hs = merged.career_headshot_pct();
    let hs = if want.is_none() && career_hs > 0.0 {
        career_hs
    } else {
        ops.headshot_pct()
    };
    Kpis {
        kd: r2(kd),
        win_pct: r1(win_pct),
        hs_pct: r1(hs),
        kills_per_round: r2(ops.kills_per_round()),
        fk_fd_ratio: r2(ratio_kd(ops.first_bloods, ops.first_deaths)),
        clutch_pct: replays.clutch_pct,
        survival_pct: r1(if ops.rounds_played == 0 {
            0.0
        } else {
            100.0 * (1.0 - ops.deaths as f64 / ops.rounds_played as f64)
        }),
        rounds: ops.rounds_played.min(u32::MAX as u64) as u32,
        matches: matches.min(u32::MAX as u64) as u32,
    }
}

fn career_operator_cards(f: &Filter, merged: &Baseline, career_kd: f64) -> Vec<OperatorCard> {
    let want = side_str(f.side);
    let mut cards: Vec<OperatorCard> = merged
        .operators
        .iter()
        .filter(|(_, c)| c.rounds_played > 0 && (want.is_none() || c.side.as_deref() == want))
        .map(|(key, c)| OperatorCard {
            key: key.clone(),
            name: op_name(key),
            side: if c.side.as_deref() == Some("attacker") { TeamSide::Attack } else { TeamSide::Defense },
            rounds: c.rounds_played.min(u32::MAX as u64) as u32,
            win_pct: r1(c.round_win_pct()),
            kd: r2(c.kd()),
            hs_pct: r1(c.headshot_pct()),
            delta_kd: Some(r2(c.kd() - career_kd)),
        })
        .collect();
    cards.sort_by(|a, b| b.rounds.cmp(&a.rounds).then_with(|| a.key.cmp(&b.key)));
    cards
}

fn career_map_cards(f: &Filter, merged: &Baseline) -> Vec<MapCard> {
    let want = side_str(f.side);
    let mut cards: Vec<MapCard> = merged
        .maps
        .iter()
        .filter(|(_, ms)| ms.total.matches_played > 0)
        .map(|(slug, ms)| {
            let key = norm_slug(slug);
            let c = match want {
                Some(s) => ms.sides.get(s).unwrap_or(&ms.total),
                None => &ms.total,
            };
            let side_win = |s: &str| ms.sides.get(s).map_or(0.0, |c| c.round_win_pct());
            MapCard {
                name: map_name_of(&key),
                matches: c.matches_played.min(u32::MAX as u64) as u32,
                win_pct: r1(c.win_pct()),
                attack_win_pct: r1(side_win("attacker")),
                defense_win_pct: r1(side_win("defender")),
                kd: r2(c.kd()),
                key,
            }
        })
        .collect();
    cards.sort_by(|a, b| b.matches.cmp(&a.matches).then_with(|| a.key.cmp(&b.key)));
    cards
}

// ---------------------------------------------------------------- match rows and detail

fn badges(s: &PlayerMatchStats) -> Vec<String> {
    let mut out = Vec::new();
    let mk = &s.multi_kills;
    let best = if mk.k5 > 0 {
        Some("5K")
    } else if mk.k4 > 0 {
        Some("4K")
    } else if mk.k3 > 0 {
        Some("3K")
    } else if mk.k2 > 0 {
        Some("2K")
    } else {
        None
    };
    if let Some(b) = best {
        out.push(b.to_string());
    }
    if let Some(c) = s.clutches.iter().filter(|c| c.won).max_by_key(|c| c.vs) {
        out.push(format!("1v{} Won", c.vs));
    } else if let Some(c) = s.clutches.iter().max_by_key(|c| c.vs) {
        out.push(format!("1v{} Lost", c.vs));
    }
    out
}

/// One match as a list row. Numbers are for the whole match, ignoring side/operator filters.
pub fn match_row(m: &FullMatch) -> MatchRow {
    let stats = player_match_stats(m);
    let me = stats.iter().find(|s| s.profile_id == m.me_profile_id);
    let key = map_key_of(m);
    MatchRow {
        match_id: m.match_id.clone(),
        timestamp: m.timestamp.clone(),
        map_name: map_name_of(&key),
        map_key: key,
        mode: mode_label(&m.match_type),
        won: m.won,
        score_us: m.score_us,
        score_them: m.score_them,
        kills: me.map_or(0, |s| s.kills),
        deaths: me.map_or(0, |s| s.deaths),
        hs_pct: r1(me.map_or(0.0, |s| s.headshot_pct())),
        badges: me.map(badges).unwrap_or_default(),
    }
}

/// A page of the match list, newest first. `page` is 0-based.
pub fn list_matches(f: &Filter, matches: &[FullMatch], page: u32, page_size: u32) -> Vec<MatchRow> {
    apply(f, matches)
        .into_iter()
        .rev()
        .skip(page as usize * page_size as usize)
        .take(page_size as usize)
        .map(match_row)
        .collect()
}

fn win_condition_text(w: WinCondition) -> &'static str {
    match w {
        WinCondition::Elimination => "Eliminated enemies",
        WinCondition::DefuserDisabled => "Disabled defuser",
        WinCondition::BombDefused => "Defused bomb",
        WinCondition::Planted => "Planted bomb",
        WinCondition::Time => "Time ran out",
        WinCondition::Unknown => "Unknown",
    }
}

pub fn match_detail(m: &FullMatch) -> MatchDetail {
    let my_team = m.my_team_index().unwrap_or(0);
    let stats = player_match_stats(m);
    let rel = |team: usize| -> u8 { (team != my_team) as u8 };

    let mut teams: [Vec<ScoreboardRow>; 2] = [Vec::new(), Vec::new()];
    for s in &stats {
        let ops: Vec<String> = m
            .rounds
            .iter()
            .filter_map(|r| r.players.iter().find(|p| p.profile_id == s.profile_id))
            .map(|p| op_key(p.operator_id))
            .collect();
        teams[rel(s.team_index) as usize].push(ScoreboardRow {
            username: s.username.clone(),
            is_me: s.profile_id == m.me_profile_id,
            operators_by_round: ops,
            kills: s.kills,
            deaths: s.deaths,
            hs_pct: r1(s.headshot_pct()),
            fk: s.first_kills,
            fd: s.first_deaths,
            clutches_won: s.clutches_won,
            clutches_lost: s.clutches_lost,
            multi_kills: s.multi_kills.total(),
            kost_pct: r1(s.kost_pct()),
        });
    }

    let name_of = |id: &str| -> String {
        m.player(id).map_or_else(|| id.to_string(), |p| p.username.clone())
    };
    let rounds = m
        .rounds
        .iter()
        .map(|r| {
            let in_round = |id: &str| r.players.iter().find(|p| p.profile_id == id);
            let kills = r
                .kills
                .iter()
                .map(|k| {
                    let killer = in_round(&k.killer);
                    let victim = in_round(&k.victim);
                    let killer_team = killer
                        .map(|p| p.team_index)
                        .or_else(|| m.player(&k.killer).map(|p| p.team_index))
                        .unwrap_or(my_team);
                    KillEventView {
                        time_s: k.time_s,
                        killer: name_of(&k.killer),
                        killer_operator: killer.map_or_else(|| "unknown".into(), |p| op_key(p.operator_id)),
                        victim: name_of(&k.victim),
                        victim_operator: victim.map_or_else(|| "unknown".into(), |p| op_key(p.operator_id)),
                        headshot: k.headshot,
                        killer_team: rel(killer_team),
                    }
                })
                .collect();
            let my_side = team_side(r.my_side).unwrap_or_else(|| {
                log::warn!("round {} has no known side for the tracked player", r.number);
                TeamSide::Attack
            });
            RoundDetail {
                number: r.number,
                winner_team: r.winner_team.map_or(1, rel),
                win_condition: win_condition_text(r.win_condition).to_string(),
                site: r.site.clone(),
                my_side,
                kills,
            }
        })
        .collect();

    MatchDetail { match_row: match_row(m), teams, rounds }
}

// ---------------------------------------------------------------- tests

#[cfg(test)]
mod tests {
    use super::*;
    use crate::full::PlayerInfo;

    fn op_id(key: &str) -> u64 {
        let v: serde_json::Value =
            serde_json::from_str(include_str!("../../../data/operators.json")).unwrap();
        v["operators"].as_array().unwrap().iter().find(|o| o["key"] == key).unwrap()["id"]
            .as_u64()
            .unwrap()
    }

    fn rp(id: &str, team: usize, op: u64, side: Side, kills: u32, died: bool) -> RoundPlayer {
        RoundPlayer {
            profile_id: id.into(),
            team_index: team,
            operator_id: op,
            side,
            kills,
            deaths: died as u32,
            headshots: 0,
            first_kill: false,
            first_death: false,
            survived: !died,
        }
    }

    fn round(n: u32, my_side: Side, op: u64, kills: u32, died: bool, won: bool) -> FullRound {
        let other = if my_side == Side::Attack { Side::Defense } else { Side::Attack };
        FullRound {
            number: n,
            winner_team: Some(if won { 0 } else { 1 }),
            win_condition: WinCondition::Elimination,
            site: "Lobby".into(),
            my_side,
            players: vec![
                rp("me", 0, op, my_side, kills, died),
                rp("en", 1, 1, other, 0, kills > 0),
            ],
            kills: vec![],
        }
    }

    /// Round 1: attack on Zofia (2 kills, survived, won). Round 2: defense on Jager (0 kills,
    /// died, lost).
    fn fixture(id: &str, ts: &str, mt: &str, map_id: u64) -> FullMatch {
        FullMatch {
            match_id: id.into(),
            timestamp: ts.into(),
            match_type: mt.into(),
            gamemode: "Bomb".into(),
            map_id,
            score_us: 1,
            score_them: 1,
            won: true,
            me_profile_id: "me".into(),
            players: vec![
                PlayerInfo { profile_id: "me".into(), username: "Me".into(), team_index: 0 },
                PlayerInfo { profile_id: "en".into(), username: "En".into(), team_index: 1 },
            ],
            rounds: vec![
                round(1, Side::Attack, op_id("zofia"), 2, false, true),
                round(2, Side::Defense, op_id("jager"), 0, true, false),
            ],
        }
    }

    fn filter(json: &str) -> Filter {
        serde_json::from_str(json).unwrap()
    }

    fn set() -> Vec<FullMatch> {
        (1..=7)
            .map(|i| fixture(&format!("m{i}"), &format!("2026-10-0{i}T10:00:00Z"), "Ranked", 407558616688))
            .collect()
    }

    #[test]
    fn filter_defaults_and_names() {
        let f = filter(r#"{"range":"last5","mode":"ranked","side":"attack","operators":["zofia"],"maps":[]}"#);
        assert_eq!(f.range, Range::Last5);
        assert_eq!(f.side, SideFilter::Attack);
        assert_eq!(filter("{}"), Filter::default());
    }

    #[test]
    fn range_and_mode() {
        let mut ms = set();
        ms[2].match_type = "Unranked".into();
        assert_eq!(apply(&filter(r#"{"range":"last5"}"#), &ms).len(), 5);
        assert_eq!(apply(&filter(r#"{"range":"last1"}"#), &ms)[0].match_id, "m7");
        // Mode first, then the newest N of what remains.
        let v = apply(&filter(r#"{"range":"last5","mode":"ranked"}"#), &ms);
        assert_eq!(v.len(), 5);
        assert!(v.iter().all(|m| m.match_id != "m3"));
        assert_eq!(apply(&filter(r#"{"mode":"unranked"}"#), &ms).len(), 1);
        assert_eq!(apply(&filter(r#"{"mode":"quick"}"#), &ms).len(), 0);
    }

    #[test]
    fn side_and_operator_filter_rounds() {
        let ms = set();
        let b = Baseline::default();
        let all = query(&filter("{}"), &ms, &b);
        assert_eq!((all.kpis.rounds, all.matches_total()), (14, 7));

        // Zofia only: the attack round (2 kills, 0 deaths) of every match.
        let z = query(&filter(r#"{"operators":["zofia"]}"#), &ms, &b);
        assert_eq!(z.kpis.rounds, 7);
        assert_eq!(z.kpis.kd, 14.0); // 14 kills / 0 deaths -> kills
        assert_eq!(z.kpis.survival_pct, 100.0);
        assert_eq!(z.operators.len(), 1);
        assert_eq!(z.operators[0].key, "zofia");
        assert_eq!(z.operators[0].side, TeamSide::Attack);
        assert!(z.operators[0].delta_kd.is_some());

        // Defense side only: the Jager round (0 kills, died).
        let d = query(&filter(r#"{"side":"defense"}"#), &ms, &b);
        assert_eq!(d.kpis.rounds, 7);
        assert_eq!(d.kpis.kd, 0.0);
        assert_eq!(d.operators[0].key, "jager");

        // Zofia and defense never meet: no round, no match.
        let none = query(&filter(r#"{"operators":["zofia"],"side":"defense"}"#), &ms, &b);
        assert_eq!((none.kpis.rounds, none.kpis.matches), (0, 0));
        assert!(none.trend.is_empty() && none.operators.is_empty());
    }

    #[test]
    fn source_selection() {
        let src = |j: &str| match source_for(&filter(j)) {
            Source::Career => "career",
            Source::Replays => "replays",
        };
        assert_eq!(src("{}"), "career");
        assert_eq!(src(r#"{"range":"season","mode":"ranked"}"#), "career");
        assert_eq!(src(r#"{"side":"attack"}"#), "career");
        assert_eq!(src(r#"{"mode":"ranked","side":"attack"}"#), "replays");
        assert_eq!(src(r#"{"range":"last5"}"#), "replays");
        assert_eq!(src(r#"{"operators":["zofia"]}"#), "replays");
        assert_eq!(src(r#"{"maps":["clubhouse"]}"#), "replays");
    }

    #[test]
    fn trend_is_rolling_and_recent_is_newest_first() {
        let ms = set();
        let r = query(&filter(r#"{"range":"last5"}"#), &ms, &Baseline::default());
        assert_eq!(r.match_count, 5);
        assert_eq!(r.trend.len(), 5);
        assert_eq!(r.trend[0].index, 1);
        assert_eq!(r.trend[4].match_id, "m7");
        assert_eq!(r.recent_matches[0].match_id, "m7");
        // 2 kills, 1 death per match.
        assert_eq!(r.trend[4].kd, 2.0);
    }

    #[test]
    fn list_pages() {
        let ms = set();
        let p0 = list_matches(&filter("{}"), &ms, 0, 5);
        let p1 = list_matches(&filter("{}"), &ms, 1, 5);
        assert_eq!(p0.len(), 5);
        assert_eq!(p1.len(), 2);
        assert_eq!(p0[0].match_id, "m7");
        assert_eq!(p1[1].match_id, "m1");
    }

    #[test]
    fn detail_shapes_and_json() {
        let d = match_detail(&fixture("m1", "2026-10-01T10:00:00Z", "Ranked", 1));
        assert_eq!(d.teams[0].len(), 1);
        assert!(d.teams[0][0].is_me);
        assert_eq!(d.teams[1][0].username, "En");
        assert_eq!(d.rounds.len(), 2);
        assert_eq!(d.rounds[0].winner_team, 0);
        assert_eq!(d.rounds[1].winner_team, 1);
        assert_eq!(d.rounds[0].win_condition, "Eliminated enemies");
        let v = serde_json::to_value(&d).unwrap();
        assert!(v["match"]["matchId"].is_string());
        assert!(v["teams"][0][0]["operatorsByRound"].is_array());
        assert_eq!(v["rounds"][1]["mySide"], "defense");
        // Unknown map id stays readable.
        assert_eq!(d.match_row.map_name, "Map #1");
    }

    impl QueryResult {
        fn matches_total(&self) -> u32 {
            self.kpis.matches
        }
    }

    // --- real data (needs the dissect cache; run with `cargo test -- --ignored`) ---

    fn load_real() -> Vec<FullMatch> {
        let dir = std::path::Path::new(r"C:\Users\Kutbay\Desktop\R6Koc-veri\dissect");
        let mut v: Vec<FullMatch> = std::fs::read_dir(dir)
            .unwrap()
            .filter_map(|e| e.ok().map(|e| e.path()))
            .filter(|p| p.extension().is_some_and(|x| x == "json"))
            .map(|p| crate::full::read_full_match_file(&p).unwrap())
            .collect();
        v.sort_by(|a, b| a.timestamp.cmp(&b.timestamp));
        v
    }

    #[test]
    #[ignore]
    fn real_data_acceptance() {
        let ms = load_real();
        assert_eq!(ms.len(), 30);
        let b = Baseline::embedded().unwrap();

        let last5 = query(&filter(r#"{"range":"last5"}"#), &ms, &b);
        assert_eq!(last5.match_count, 5);
        assert_eq!(last5.trend.len(), 5);

        let z = query(&filter(r#"{"operators":["zofia"]}"#), &ms, &b);
        assert_eq!(z.operators.len(), 1);
        assert_eq!(z.operators[0].key, "zofia");
        let zofia_rounds: u32 = ms
            .iter()
            .flat_map(|m| m.rounds.iter().map(move |r| (m, r)))
            .filter(|(m, r)| {
                r.players
                    .iter()
                    .any(|p| p.profile_id == m.me_profile_id && op_key(p.operator_id) == "zofia")
            })
            .count() as u32;
        assert_eq!(z.kpis.rounds, zofia_rounds);
        println!("zofia: {} rounds, K/D {}", z.kpis.rounds, z.kpis.kd);

        let id = ms.iter().find(|m| m.match_id.starts_with("f7a14123")).expect("match f7a14123");
        let d = match_detail(id);
        assert_eq!(d.teams[0].len() + d.teams[1].len(), 10);
    }
}
