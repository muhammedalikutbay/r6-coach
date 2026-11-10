//! v5 detail screens (spec section 9): per-operator and per-weapon numbers, plus the list tables.
//!
//! Everything honours the global [`Filter`]. The operator screens override `operators` with the
//! operator being looked at (the list screens clear it); the weapon screens keep the filter as is,
//! so an operator filter narrows the weapon numbers to that operator's rounds.
//!
//! The detail structs also carry the kill and death events of the selected rounds
//! ([`ClipEvent`], never serialized). The app shell matches them against the `clips` table, which
//! the core cannot see.

use crate::derive::{is_traded, round_clutches};
use crate::full::{FullMatch, FullRound, RoundPlayer};
use crate::metrics::{EARLY_S, LATE_S};
use crate::model::Side;
use crate::names;
use crate::query::{apply, selected, Filter, TeamSide, TrendPoint, TREND_WINDOW};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::sync::OnceLock;

const LOADOUTS_JSON: &str = include_str!("../data/operator-loadouts.json");

/// Matches listed on an operator screen.
pub const MAX_MATCHES: usize = 30;
/// Rivals listed per direction.
pub const MAX_RIVALS: usize = 8;
/// Recent kills listed on a weapon screen.
pub const MAX_RECENT_KILLS: usize = 20;

// ---------------------------------------------------------------- loadouts

/// Weapon key -> operator keys that carry it (primary or secondary), from `data/operator-loadouts.json`.
fn loadout_table() -> &'static HashMap<String, BTreeSet<String>> {
    static T: OnceLock<HashMap<String, BTreeSet<String>>> = OnceLock::new();
    T.get_or_init(|| {
        let mut out: HashMap<String, BTreeSet<String>> = HashMap::new();
        match serde_json::from_str::<HashMap<String, serde_json::Value>>(LOADOUTS_JSON) {
            Ok(m) => {
                for (op, v) in m {
                    let Ok(list) = serde_json::from_value::<Vec<String>>(v) else { continue };
                    for w in list {
                        out.entry(weapon_key(&w)).or_default().insert(op.clone());
                    }
                }
            }
            Err(e) => log::error!("embedded operator loadout table is invalid: {e}"),
        }
        out
    })
}

/// Operator keys that carry the weapon in their loadout, plus `seen` (operators that got a kill with
/// it, in case the static table misses one). Sorted by operator name.
pub fn loadout_operators(weapon: &str, seen: &BTreeSet<String>) -> Vec<String> {
    let mut keys: BTreeSet<String> = loadout_table().get(&weapon_key(weapon)).cloned().unwrap_or_default();
    keys.extend(seen.iter().cloned());
    let mut v: Vec<String> = keys.into_iter().collect();
    v.sort_by_key(|k| op_name(k));
    v
}

// ---------------------------------------------------------------- small helpers

fn r1(x: f64) -> f64 {
    (x * 10.0).round() / 10.0
}
fn r2(x: f64) -> f64 {
    (x * 100.0).round() / 100.0
}
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
fn per(n: u64, d: u64) -> f64 {
    if d == 0 {
        0.0
    } else {
        n as f64 / d as f64
    }
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
        Ok(id) => crate::names::unknown_map_name(id),
        Err(_) => pretty_slug(key),
    })
}

fn team_side(s: Side) -> Option<TeamSide> {
    match s {
        Side::Attack => Some(TeamSide::Attack),
        Side::Defense => Some(TeamSide::Defense),
        Side::Unknown => None,
    }
}

/// URL-safe key of a weapon: lowercase alphanumerics joined by single dashes
/// (`416-C CARBINE` -> `416-c-carbine`).
pub fn weapon_key(name: &str) -> String {
    let mut out = String::new();
    for c in name.chars() {
        if c.is_alphanumeric() {
            out.extend(c.to_lowercase());
        } else if !out.ends_with('-') && !out.is_empty() {
            out.push('-');
        }
    }
    while out.ends_with('-') {
        out.pop();
    }
    out
}

/// Username of a profile id in this match (the id itself when unknown).
fn username(m: &FullMatch, profile_id: &str) -> String {
    m.player(profile_id).map_or_else(|| profile_id.to_string(), |p| p.username.clone())
}

fn team_map(r: &FullRound) -> HashMap<&str, usize> {
    r.players.iter().map(|p| (p.profile_id.as_str(), p.team_index)).collect()
}

fn operator_of<'a>(r: &'a FullRound, profile_id: &str) -> Option<&'a RoundPlayer> {
    r.players.iter().find(|p| p.profile_id == profile_id)
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
    fn kd(&self) -> f64 {
        ratio_kd(self.kills as u64, self.deaths as u64)
    }
    fn hs_pct(&self) -> f64 {
        pct(self.headshots as u64, self.kills as u64)
    }
    fn win_pct(&self) -> f64 {
        pct(self.rounds_won as u64, self.rounds as u64)
    }
}

// ---------------------------------------------------------------- DTOs

/// A kill or a death of the tracked player in a selected round (for matching with clips).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ClipEvent {
    pub match_id: String,
    /// 1-based.
    pub round: u32,
    pub time_s: i64,
    /// `kill` | `death`
    pub kind: String,
    /// Usernames, as the `clips` table stores them. The killer is empty for a death without one.
    pub killer: String,
    pub victim: String,
    pub headshot: bool,
    pub weapon: Option<String>,
    pub map_key: String,
    /// Operator the tracked player used.
    pub operator_key: String,
    /// Operator of the other player (victim of a kill, killer of a death); empty when unknown.
    pub enemy_operator_key: String,
}

/// The numbers of one operator (or one slice of it) under the current filter.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DetailStats {
    pub matches: u32,
    pub rounds: u32,
    pub rounds_won: u32,
    pub win_pct: f64,
    pub kills: u32,
    pub deaths: u32,
    pub kd: f64,
    pub kills_per_round: f64,
    pub hs_pct: f64,
    pub first_kills: u32,
    pub first_deaths: u32,
    pub kost_pct: f64,
    pub survival_pct: f64,
    pub clutch_won: u32,
    pub clutch_lost: u32,
    pub clutch_pct: f64,
}

impl DetailStats {
    fn from(t: &Tally, matches: u32) -> Self {
        DetailStats {
            matches,
            rounds: t.rounds,
            rounds_won: t.rounds_won,
            win_pct: r1(t.win_pct()),
            kills: t.kills,
            deaths: t.deaths,
            kd: r2(t.kd()),
            kills_per_round: r2(per(t.kills as u64, t.rounds as u64)),
            hs_pct: r1(t.hs_pct()),
            first_kills: t.fk,
            first_deaths: t.fd,
            kost_pct: r1(pct(t.kost as u64, t.rounds as u64)),
            survival_pct: r1(pct(t.survived as u64, t.rounds as u64)),
            clutch_won: t.clutch_won,
            clutch_lost: t.clutch_lost,
            clutch_pct: r1(pct(t.clutch_won as u64, (t.clutch_won + t.clutch_lost) as u64)),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SideRow {
    pub side: TeamSide,
    pub rounds: u32,
    pub win_pct: f64,
    pub kd: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MapBreakdown {
    pub key: String,
    pub name: String,
    pub rounds: u32,
    pub win_pct: f64,
    pub kills: u32,
    pub deaths: u32,
    pub kd: f64,
}

/// One weapon used by the operator (operator screen) or listed overall (weapon list).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WeaponUse {
    pub key: String,
    pub name: String,
    pub slot: Option<String>,
    pub kills: u32,
    pub hs_pct: f64,
    /// Not a named item: an operator-and-slot label (`Noor primary`). Shown dimmed.
    #[serde(default)]
    pub unnamed: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RivalCount {
    pub operator_key: String,
    pub name: String,
    pub count: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OperatorMatch {
    pub match_id: String,
    pub timestamp: String,
    pub map_key: String,
    pub map_name: String,
    pub won: bool,
    pub score_us: u32,
    pub score_them: u32,
    /// Rounds played with this operator in the match.
    pub rounds: u32,
    pub kills: u32,
    pub deaths: u32,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OperatorDetail {
    pub key: String,
    pub name: String,
    /// `None` when the operator has no rounds under the filter.
    pub side: Option<TeamSide>,
    pub stats: DetailStats,
    pub sides: Vec<SideRow>,
    pub maps: Vec<MapBreakdown>,
    pub weapons: Vec<WeaponUse>,
    /// Enemy operators this operator killed most.
    pub most_killed: Vec<RivalCount>,
    /// Enemy operators that killed this operator most.
    pub most_killed_by: Vec<RivalCount>,
    /// Newest first.
    pub matches: Vec<OperatorMatch>,
    /// Rolling K/D (window [`TREND_WINDOW`] matches), oldest first.
    pub trend: Vec<TrendPoint>,
    #[serde(skip)]
    pub events: Vec<ClipEvent>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WeaponOperator {
    pub key: String,
    pub name: String,
    pub kills: u32,
    pub hs_pct: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WeaponMap {
    pub key: String,
    pub name: String,
    pub kills: u32,
}

/// Kills by round clock: early (at least [`EARLY_S`] s left), late (at most [`LATE_S`] s left), the rest mid.
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct KillTiming {
    pub early: u32,
    pub mid: u32,
    pub late: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RecentKill {
    pub match_id: String,
    pub timestamp: String,
    pub map_key: String,
    pub map_name: String,
    pub round: u32,
    pub time_s: i64,
    pub operator_key: String,
    pub operator_name: String,
    pub victim_operator_key: String,
    pub victim_operator_name: String,
    pub headshot: bool,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WeaponDetail {
    pub key: String,
    pub name: String,
    pub slot: Option<String>,
    #[serde(default)]
    pub unnamed: bool,
    pub kills: u32,
    pub headshots: u32,
    pub hs_pct: f64,
    /// Rounds the tracked player spent on an operator that made a kill with this weapon.
    pub rounds: u32,
    pub kills_per_round: f64,
    pub operators: Vec<WeaponOperator>,
    /// Operators that carry the weapon in their loadout (not only the ones that got kills).
    #[serde(default)]
    pub loadout_operators: Vec<String>,
    pub maps: Vec<WeaponMap>,
    pub timing: KillTiming,
    /// Newest first.
    pub recent_kills: Vec<RecentKill>,
    #[serde(skip)]
    pub events: Vec<ClipEvent>,
}

/// One row of the operator list table.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OperatorRow {
    pub key: String,
    pub name: String,
    pub side: TeamSide,
    pub matches: u32,
    pub rounds: u32,
    pub win_pct: f64,
    pub kills: u32,
    pub deaths: u32,
    pub kd: f64,
    pub kills_per_round: f64,
    pub hs_pct: f64,
    pub kost_pct: f64,
}

/// One row of the weapon list table.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WeaponRow {
    pub key: String,
    pub name: String,
    pub slot: Option<String>,
    #[serde(default)]
    pub unnamed: bool,
    pub kills: u32,
    pub headshots: u32,
    pub hs_pct: f64,
    /// Different operators that got a kill with it.
    pub operators: u32,
    /// Operators that carry it in their loadout.
    #[serde(default)]
    pub loadout_operators: Vec<String>,
    /// Operators the tracked player got a kill with it as (a subset of `loadout_operators`).
    #[serde(default)]
    pub used_operators: Vec<String>,
}

// ---------------------------------------------------------------- lists

/// Every operator the tracked player used under the filter (the filter's own operator list is ignored).
pub fn operator_list(filter: &Filter, matches: &[FullMatch]) -> Vec<OperatorRow> {
    let mut f = filter.clone();
    f.operators.clear();
    let set = apply(&f, matches);
    // key -> (tally, side, matches)
    let mut by_key: BTreeMap<String, (Tally, Option<TeamSide>, BTreeSet<&str>)> = BTreeMap::new();
    for m in &set {
        for r in &m.rounds {
            if let Some(p) = selected(&f, r, &m.me_profile_id) {
                let e = by_key.entry(op_key(p.operator_id)).or_default();
                e.0.add_round(r, p);
                e.1 = e.1.or(team_side(p.side));
                e.2.insert(m.match_id.as_str());
            }
        }
    }
    let mut rows: Vec<OperatorRow> = by_key
        .into_iter()
        .map(|(key, (t, side, ms))| OperatorRow {
            name: op_name(&key),
            side: side.unwrap_or(TeamSide::Defense),
            matches: ms.len() as u32,
            rounds: t.rounds,
            win_pct: r1(t.win_pct()),
            kills: t.kills,
            deaths: t.deaths,
            kd: r2(t.kd()),
            kills_per_round: r2(per(t.kills as u64, t.rounds as u64)),
            hs_pct: r1(t.hs_pct()),
            kost_pct: r1(pct(t.kost as u64, t.rounds as u64)),
            key,
        })
        .collect();
    rows.sort_by(|a, b| b.rounds.cmp(&a.rounds).then_with(|| a.key.cmp(&b.key)));
    rows
}

/// Label of a kill's weapon for the operator that made it (`Noor primary` for an unknown item).
pub fn kill_weapon_label(k: &crate::full::KillEvent, operator_id: u64) -> Option<crate::weapons::WeaponLabel> {
    let raw = k.weapon.as_deref()?;
    let op = names::operator_key(operator_id).map(|key| op_name(&key));
    Some(crate::weapons::label(raw, op.as_deref()))
}

/// The tracked player's kills in a round that hit an enemy (team kills dropped).
fn my_kills<'a>(r: &'a FullRound, me: &'a str) -> impl Iterator<Item = &'a crate::full::KillEvent> + 'a {
    let my_team = operator_of(r, me).map(|p| p.team_index);
    let teams = team_map(r);
    r.kills.iter().filter(move |k| {
        k.killer == me && match (teams.get(k.victim.as_str()), my_team) {
            (Some(t), Some(mine)) => *t != mine,
            _ => true,
        }
    })
}

/// Every weapon the tracked player got a kill with under the filter.
pub fn weapon_list(filter: &Filter, matches: &[FullMatch]) -> Vec<WeaponRow> {
    let set = apply(filter, matches);
    #[derive(Default)]
    struct Acc {
        name: String,
        unnamed: bool,
        slot: Option<String>,
        kills: u32,
        hs: u32,
        ops: BTreeSet<String>,
    }
    let mut by_key: BTreeMap<String, Acc> = BTreeMap::new();
    for m in &set {
        for r in &m.rounds {
            let Some(p) = selected(filter, r, &m.me_profile_id) else { continue };
            for k in my_kills(r, &m.me_profile_id) {
                let Some(label) = kill_weapon_label(k, p.operator_id) else { continue };
                let a = by_key.entry(weapon_key(&label.name)).or_default();
                if a.name.is_empty() {
                    a.name = label.name;
                    a.unnamed = label.unnamed;
                }
                a.slot = a.slot.clone().or_else(|| k.weapon_slot.clone());
                a.kills += 1;
                a.hs += k.headshot as u32;
                a.ops.insert(op_key(p.operator_id));
            }
        }
    }
    let mut rows: Vec<WeaponRow> = by_key
        .into_iter()
        .filter(|(key, _)| !key.is_empty())
        .map(|(key, a)| WeaponRow {
            loadout_operators: loadout_operators(&key, &a.ops),
            used_operators: a.ops.iter().cloned().collect(),
            key,
            name: a.name,
            unnamed: a.unnamed,
            slot: a.slot,
            kills: a.kills,
            headshots: a.hs,
            hs_pct: r1(pct(a.hs as u64, a.kills as u64)),
            operators: a.ops.len() as u32,
        })
        .collect();
    rows.sort_by(|a, b| b.kills.cmp(&a.kills).then_with(|| a.key.cmp(&b.key)));
    rows
}

// ---------------------------------------------------------------- operator detail

fn rivals(counts: BTreeMap<String, u32>) -> Vec<RivalCount> {
    let mut v: Vec<RivalCount> = counts
        .into_iter()
        .map(|(k, count)| RivalCount { name: op_name(&k), operator_key: k, count })
        .collect();
    v.sort_by(|a, b| b.count.cmp(&a.count).then_with(|| a.operator_key.cmp(&b.operator_key)));
    v.truncate(MAX_RIVALS);
    v
}

/// Everything about one operator (`key` is the lowercase operator key) under the filter.
pub fn operator_detail(filter: &Filter, matches: &[FullMatch], key: &str) -> OperatorDetail {
    let key = key.to_lowercase();
    let mut f = filter.clone();
    f.operators = vec![key.clone()];
    let set = apply(&f, matches);

    let mut total = Tally::default();
    let mut side: Option<TeamSide> = None;
    let mut by_side: [Tally; 2] = Default::default();
    let mut by_map: BTreeMap<String, Tally> = BTreeMap::new();
    let mut weapons: BTreeMap<String, (String, bool, Option<String>, u32, u32)> = BTreeMap::new();
    let mut killed: BTreeMap<String, u32> = BTreeMap::new();
    let mut killed_by: BTreeMap<String, u32> = BTreeMap::new();
    let mut events: Vec<ClipEvent> = Vec::new();
    let mut per_match: Vec<Tally> = Vec::new();
    let mut rows: Vec<OperatorMatch> = Vec::new();
    let mut played = 0u32;

    for m in &set {
        let me = m.me_profile_id.as_str();
        let map_key = map_key_of(m);
        let mut mt = Tally::default();
        for r in &m.rounds {
            let Some(p) = selected(&f, r, me) else { continue };
            total.add_round(r, p);
            mt.add_round(r, p);
            by_map.entry(map_key.clone()).or_default().add_round(r, p);
            side = side.or(team_side(p.side));
            match p.side {
                Side::Attack => by_side[0].add_round(r, p),
                Side::Defense => by_side[1].add_round(r, p),
                Side::Unknown => {}
            }
            let teams = team_map(r);
            for k in my_kills(r, me) {
                if let Some(label) = kill_weapon_label(k, p.operator_id) {
                    let e = weapons
                        .entry(weapon_key(&label.name))
                        .or_insert_with(|| (label.name.clone(), label.unnamed, None, 0, 0));
                    e.2 = e.2.clone().or_else(|| k.weapon_slot.clone());
                    e.3 += 1;
                    e.4 += k.headshot as u32;
                }
                let victim_op = operator_of(r, &k.victim).map(|q| op_key(q.operator_id));
                if let Some(vo) = &victim_op {
                    *killed.entry(vo.clone()).or_default() += 1;
                }
                events.push(ClipEvent {
                    match_id: m.match_id.clone(),
                    round: r.number,
                    time_s: k.time_s,
                    kind: "kill".into(),
                    killer: username(m, me),
                    victim: username(m, &k.victim),
                    headshot: k.headshot,
                    weapon: kill_weapon_label(k, p.operator_id).map(|l| l.display),
                    map_key: map_key.clone(),
                    operator_key: key.clone(),
                    enemy_operator_key: victim_op.unwrap_or_default(),
                });
            }
            if let Some(dk) = r.kills.iter().find(|k| k.victim == me) {
                let killer = operator_of(r, &dk.killer);
                let enemy = killer.filter(|_| teams.get(dk.killer.as_str()).is_some_and(|t| *t != p.team_index));
                if let Some(q) = enemy {
                    *killed_by.entry(op_key(q.operator_id)).or_default() += 1;
                }
                events.push(ClipEvent {
                    match_id: m.match_id.clone(),
                    round: r.number,
                    time_s: dk.time_s,
                    kind: "death".into(),
                    killer: if dk.killer.is_empty() { String::new() } else { username(m, &dk.killer) },
                    victim: username(m, me),
                    headshot: dk.headshot,
                    weapon: dk.weapon.as_deref().map(|raw| {
                        let op = killer.and_then(|q| names::operator_key(q.operator_id)).map(|k| op_name(&k));
                        crate::weapons::label(raw, op.as_deref()).display
                    }),
                    map_key: map_key.clone(),
                    operator_key: key.clone(),
                    enemy_operator_key: enemy.map(|q| op_key(q.operator_id)).unwrap_or_default(),
                });
            }
        }
        if mt.rounds > 0 {
            played += 1;
            rows.push(OperatorMatch {
                match_id: m.match_id.clone(),
                timestamp: m.timestamp.clone(),
                map_name: map_name_of(&map_key),
                map_key,
                won: m.won,
                score_us: m.score_us,
                score_them: m.score_them,
                rounds: mt.rounds,
                kills: mt.kills,
                deaths: mt.deaths,
            });
            per_match.push(mt);
        }
    }

    // `set` is oldest first and every entry has rounds, so `rows`, `per_match` and the trend line up.
    let played_matches: Vec<&&FullMatch> = set.iter().filter(|m| rows.iter().any(|x| x.match_id == m.match_id)).collect();
    let trend: Vec<TrendPoint> = played_matches
        .iter()
        .enumerate()
        .map(|(i, m)| {
            let from = (i + 1).saturating_sub(TREND_WINDOW);
            let (k, d) = per_match[from..=i].iter().fold((0u64, 0u64), |a, t| (a.0 + t.kills as u64, a.1 + t.deaths as u64));
            TrendPoint { index: i as u32 + 1, timestamp: m.timestamp.clone(), match_id: m.match_id.clone(), kd: r2(ratio_kd(k, d)), won: m.won }
        })
        .collect();

    let sides = [(TeamSide::Attack, &by_side[0]), (TeamSide::Defense, &by_side[1])]
        .into_iter()
        .filter(|(_, t)| t.rounds > 0)
        .map(|(s, t)| SideRow { side: s, rounds: t.rounds, win_pct: r1(t.win_pct()), kd: r2(t.kd()) })
        .collect();

    let mut maps: Vec<MapBreakdown> = by_map
        .into_iter()
        .map(|(key, t)| MapBreakdown {
            name: map_name_of(&key),
            rounds: t.rounds,
            win_pct: r1(t.win_pct()),
            kills: t.kills,
            deaths: t.deaths,
            kd: r2(t.kd()),
            key,
        })
        .collect();
    maps.sort_by(|a, b| b.rounds.cmp(&a.rounds).then_with(|| a.key.cmp(&b.key)));

    let mut weapon_rows: Vec<WeaponUse> = weapons
        .into_iter()
        .filter(|(k, _)| !k.is_empty())
        .map(|(key, (name, unnamed, slot, kills, hs))| WeaponUse { key, name, unnamed, slot, kills, hs_pct: r1(pct(hs as u64, kills as u64)) })
        .collect();
    weapon_rows.sort_by(|a, b| b.kills.cmp(&a.kills).then_with(|| a.key.cmp(&b.key)));

    rows.reverse();
    rows.truncate(MAX_MATCHES);

    OperatorDetail {
        name: op_name(&key),
        key,
        side,
        stats: DetailStats::from(&total, played),
        sides,
        maps,
        weapons: weapon_rows,
        most_killed: rivals(killed),
        most_killed_by: rivals(killed_by),
        matches: rows,
        trend,
        events,
    }
}

// ---------------------------------------------------------------- weapon detail

/// Everything about one weapon (`key` from [`weapon_key`]) under the filter.
pub fn weapon_detail(filter: &Filter, matches: &[FullMatch], key: &str) -> WeaponDetail {
    let key = key.to_lowercase();
    let set = apply(filter, matches);

    let mut out = WeaponDetail { key: key.clone(), name: pretty_slug(&key), ..Default::default() };
    let mut ops: BTreeMap<String, (u32, u32)> = BTreeMap::new();
    let mut maps: BTreeMap<String, u32> = BTreeMap::new();
    let mut op_rounds: BTreeMap<String, u32> = BTreeMap::new();
    let mut recent: Vec<RecentKill> = Vec::new();

    for m in &set {
        let me = m.me_profile_id.as_str();
        let map_key = map_key_of(m);
        for r in &m.rounds {
            let Some(p) = selected(filter, r, me) else { continue };
            let my_op = op_key(p.operator_id);
            *op_rounds.entry(my_op.clone()).or_default() += 1;
            for k in my_kills(r, me) {
                let Some(label) = kill_weapon_label(k, p.operator_id) else { continue };
                if weapon_key(&label.name) != key {
                    continue;
                }
                if out.kills == 0 {
                    out.name = label.name.clone();
                    out.unnamed = label.unnamed;
                }
                out.slot = out.slot.take().or_else(|| k.weapon_slot.clone());
                out.kills += 1;
                out.headshots += k.headshot as u32;
                let e = ops.entry(my_op.clone()).or_default();
                e.0 += 1;
                e.1 += k.headshot as u32;
                *maps.entry(map_key.clone()).or_default() += 1;
                if k.time_s >= EARLY_S {
                    out.timing.early += 1;
                } else if k.time_s <= LATE_S {
                    out.timing.late += 1;
                } else {
                    out.timing.mid += 1;
                }
                let victim_op = operator_of(r, &k.victim).map(|q| op_key(q.operator_id)).unwrap_or_default();
                recent.push(RecentKill {
                    match_id: m.match_id.clone(),
                    timestamp: m.timestamp.clone(),
                    map_name: map_name_of(&map_key),
                    map_key: map_key.clone(),
                    round: r.number,
                    time_s: k.time_s,
                    operator_name: op_name(&my_op),
                    operator_key: my_op.clone(),
                    victim_operator_name: if victim_op.is_empty() { String::new() } else { op_name(&victim_op) },
                    victim_operator_key: victim_op.clone(),
                    headshot: k.headshot,
                });
                out.events.push(ClipEvent {
                    match_id: m.match_id.clone(),
                    round: r.number,
                    time_s: k.time_s,
                    kind: "kill".into(),
                    killer: username(m, me),
                    victim: username(m, &k.victim),
                    headshot: k.headshot,
                    weapon: Some(label.display.clone()),
                    map_key: map_key.clone(),
                    operator_key: my_op.clone(),
                    enemy_operator_key: victim_op,
                });
            }
        }
    }

    out.hs_pct = r1(pct(out.headshots as u64, out.kills as u64));
    out.rounds = ops.keys().map(|k| op_rounds.get(k).copied().unwrap_or(0)).sum();
    out.kills_per_round = r2(per(out.kills as u64, out.rounds as u64));

    out.operators = ops
        .into_iter()
        .map(|(key, (kills, hs))| WeaponOperator { name: op_name(&key), key, kills, hs_pct: r1(pct(hs as u64, kills as u64)) })
        .collect();
    out.operators.sort_by(|a, b| b.kills.cmp(&a.kills).then_with(|| a.key.cmp(&b.key)));
    let seen: BTreeSet<String> = out.operators.iter().map(|o| o.key.clone()).collect();
    out.loadout_operators = loadout_operators(&key, &seen);

    out.maps = maps.into_iter().map(|(key, kills)| WeaponMap { name: map_name_of(&key), key, kills }).collect();
    out.maps.sort_by(|a, b| b.kills.cmp(&a.kills).then_with(|| a.key.cmp(&b.key)));

    recent.sort_by(|a, b| b.timestamp.cmp(&a.timestamp).then_with(|| b.round.cmp(&a.round)).then_with(|| a.time_s.cmp(&b.time_s)));
    recent.truncate(MAX_RECENT_KILLS);
    out.recent_kills = recent;
    out
}

// ---------------------------------------------------------------- tests

#[cfg(test)]
mod tests {
    use super::*;
    use crate::full::{KillEvent, PlayerInfo, WinCondition};

    fn pl(id: &str, team: usize, op: u64, side: Side, kills: u32, died: bool) -> RoundPlayer {
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

    fn kill(t: i64, killer: &str, victim: &str, hs: bool, weapon: Option<(&str, &str)>) -> KillEvent {
        KillEvent {
            time_s: t,
            killer: killer.into(),
            victim: victim.into(),
            headshot: hs,
            weapon_id: None,
            weapon: weapon.map(|w| w.0.to_string()),
            weapon_slot: weapon.map(|w| w.1.to_string()),
        }
    }

    /// Unknown operator IDs keep their raw ID as key, which keeps the fixture independent of the tables.
    fn two_known_ops() -> (u64, u64, String, String) {
        (1001, 1002, "1001".into(), "1002".into())
    }

    /// Match with two rounds. Round 1: `me` (op A, attack) kills f1 (op B) with R4-C (headshot, early)
    /// and dies to f2 late. Round 2: `me` switches to op B and kills f2 with a pistol.
    fn fixture() -> (FullMatch, String, String) {
        let (a, b, ka, kb) = two_known_ops();
        let mut me1 = pl("me", 0, a, Side::Attack, 1, true);
        me1.headshots = 1;
        me1.first_kill = true;
        let mate = pl("mate", 0, b, Side::Attack, 0, true);
        let f1 = pl("f1", 1, b, Side::Defense, 0, true);
        let f2 = pl("f2", 1, a, Side::Defense, 1, false);
        let r1 = FullRound {
            number: 1,
            start_time_ms: 0,
            winner_team: Some(1),
            win_condition: WinCondition::Elimination,
            site: String::new(),
            my_side: Side::Attack,
            players: vec![me1, mate, f1, f2],
            kills: vec![
                kill(170, "me", "f1", true, Some(("R4-C", "primary"))),
                kill(20, "f2", "me", false, None),
            ],
        };
        let me2 = pl("me", 0, b, Side::Attack, 1, false);
        let f3 = pl("f2", 1, a, Side::Defense, 0, true);
        let r2 = FullRound {
            number: 2,
            start_time_ms: 0,
            winner_team: Some(0),
            win_condition: WinCondition::Elimination,
            site: String::new(),
            my_side: Side::Attack,
            players: vec![me2, f3],
            kills: vec![kill(90, "me", "f2", false, Some(("P12", "secondary")))],
        };
        let m = FullMatch {
            match_id: "m1".into(),
            timestamp: "2026-11-01T10:00:00".into(),
            match_type: "Ranked".into(),
            gamemode: "Bomb".into(),
            map_id: 0,
            score_us: 1,
            score_them: 1,
            won: true,
            me_profile_id: "me".into(),
            players: vec![
                PlayerInfo { profile_id: "me".into(), username: "Me".into(), team_index: 0 },
                PlayerInfo { profile_id: "f1".into(), username: "Foe1".into(), team_index: 1 },
                PlayerInfo { profile_id: "f2".into(), username: "Foe2".into(), team_index: 1 },
            ],
            rounds: vec![r1, r2],
            season: String::new(),
        };
        (m, ka, kb)
    }

    #[test]
    fn weapon_keys_are_slugs() {
        assert_eq!(weapon_key("416-C CARBINE"), "416-c-carbine");
        assert_eq!(weapon_key("R4-C"), "r4-c");
        assert_eq!(weapon_key("  P12 "), "p12");
    }

    #[test]
    fn operator_detail_counts_rounds_rivals_and_events() {
        let (m, ka, kb) = fixture();
        let d = operator_detail(&Filter::default(), &[m], &ka);
        assert_eq!(d.key, ka);
        assert_eq!(d.stats.rounds, 1);
        assert_eq!((d.stats.kills, d.stats.deaths), (1, 1));
        assert_eq!(d.stats.hs_pct, 100.0);
        assert_eq!(d.stats.first_kills, 1);
        assert_eq!(d.stats.matches, 1);
        assert_eq!(d.side, Some(TeamSide::Attack));
        assert_eq!(d.weapons.len(), 1);
        assert_eq!(d.weapons[0].name, "R4-C");
        // Killed op B once, was killed by op A (f2) once.
        assert_eq!(d.most_killed[0].operator_key, kb);
        assert_eq!(d.most_killed_by[0].operator_key, ka);
        assert_eq!(d.maps.len(), 1);
        assert_eq!(d.matches.len(), 1);
        assert_eq!(d.trend.len(), 1);
        let kinds: Vec<&str> = d.events.iter().map(|e| e.kind.as_str()).collect();
        assert_eq!(kinds, ["kill", "death"]);
        assert_eq!((d.events[0].killer.as_str(), d.events[0].victim.as_str()), ("Me", "Foe1"));
        assert_eq!((d.events[1].killer.as_str(), d.events[1].victim.as_str()), ("Foe2", "Me"));
    }

    #[test]
    fn operator_detail_for_unused_operator_is_empty() {
        let (m, _, _) = fixture();
        let d = operator_detail(&Filter::default(), &[m], "no-such-operator");
        assert_eq!(d.stats.rounds, 0);
        assert!(d.side.is_none() && d.events.is_empty() && d.matches.is_empty());
    }

    #[test]
    fn weapon_detail_splits_by_operator_and_time() {
        let (m, ka, _) = fixture();
        let d = weapon_detail(&Filter::default(), &[m], "r4-c");
        assert_eq!((d.name.as_str(), d.slot.as_deref()), ("R4-C", Some("primary")));
        assert_eq!((d.kills, d.headshots), (1, 1));
        // Only operator A used it, and A played one round.
        assert_eq!(d.rounds, 1);
        assert_eq!(d.kills_per_round, 1.0);
        assert_eq!(d.operators[0].key, ka);
        assert_eq!(d.timing.early, 1);
        assert_eq!(d.timing.mid + d.timing.late, 0);
        assert_eq!(d.recent_kills.len(), 1);
        assert_eq!(d.events.len(), 1);
        assert_eq!(d.events[0].weapon.as_deref(), Some("R4-C"));
    }

    #[test]
    fn loadout_table_is_valid_and_complete() {
        assert!(!loadout_table().is_empty());
        // Every weapon in the table is a real weapon name of data/weapons.json.
        let raw: HashMap<String, serde_json::Value> = serde_json::from_str(include_str!("../../../data/weapons.json")).unwrap();
        let known: BTreeSet<String> = raw.values().filter_map(|v| v["name"].as_str()).map(weapon_key).collect();
        let missing: Vec<&String> = loadout_table().keys().filter(|k| !known.contains(*k)).collect();
        assert!(missing.is_empty(), "unknown weapons in operator-loadouts.json: {missing:?}");
        for op in loadout_table().values().flatten() {
            assert!(names::operator_name_by_key(op).is_some(), "unknown operator {op}");
        }
        let r4c = loadout_operators("R4-C", &BTreeSet::new());
        assert!(r4c.contains(&"ash".to_string()) && r4c.contains(&"ram".to_string()));
        // Observed operators are added.
        let with_seen = loadout_operators("r4-c", &BTreeSet::from(["zzz-unknown".to_string()]));
        assert!(with_seen.contains(&"zzz-unknown".to_string()));
        assert!(loadout_operators("no-such-gun", &BTreeSet::new()).is_empty());
    }

    #[test]
    fn lists_ignore_operator_filter_or_follow_it() {
        let (m, ka, _) = fixture();
        let f = Filter { operators: vec![ka.clone()], ..Default::default() };
        // The operator list always shows every operator.
        assert_eq!(operator_list(&f, std::slice::from_ref(&m)).len(), 2);
        // The weapon list follows the filter: A only had the R4-C kill.
        let w = weapon_list(&f, std::slice::from_ref(&m));
        assert_eq!(w.len(), 1);
        assert_eq!(w[0].key, "r4-c");
        let all = weapon_list(&Filter::default(), &[m]);
        assert_eq!(all.len(), 2);
    }
}
