//! Deep per-round metrics of the tracked player (coach audit gaps G1-G11): opening duels per
//! side, trades, KOST, how rounds are lost, death cost, weapon use, death timing, the enemy
//! operators that kill the player, clutches by opponent count, and side results per map and site.
//!
//! [`Deep`] is an additive tally over rounds that pass the global filter (the same
//! `selected` rule as the query engine). The coach compares two of them (focus vs reference); the
//! Overview shows one through [`DeepStats`]. Everything is derived from data the parser already
//! emits; nothing here guesses.

use crate::derive::{is_traded, round_clutches, TRADE_WINDOW_S};
use crate::full::{FullMatch, FullRound, RoundPlayer, WinCondition};
use crate::model::Side;
use crate::names;
use crate::query::{selected, Filter};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap};

/// A death at or after this clock reading (seconds left) came in the first 30 s of a 3 min round.
pub const EARLY_S: i64 = 150;
/// A kill or death at or before this clock reading came in the last 30 s of the round.
pub const LATE_S: i64 = 30;

/// Successes `x` out of `n` trials.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Rate {
    pub x: u32,
    pub n: u32,
}

impl Rate {
    pub fn add(&mut self, hit: bool) {
        self.n += 1;
        self.x += hit as u32;
    }
    pub fn merge(&mut self, o: &Rate) {
        self.x += o.x;
        self.n += o.n;
    }
    /// Percent, 0 when there is no trial.
    pub fn pct(&self) -> f64 {
        if self.n == 0 {
            0.0
        } else {
            100.0 * self.x as f64 / self.n as f64
        }
    }
    pub fn plus(self, o: Rate) -> Rate {
        Rate { x: self.x + o.x, n: self.n + o.n }
    }
}

/// How lost rounds on one side ended (the winner's end type).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct LostEnds {
    pub elimination: u32,
    pub time: u32,
    pub planted: u32,
    pub defused: u32,
    pub other: u32,
}

impl LostEnds {
    pub fn total(&self) -> u32 {
        self.elimination + self.time + self.planted + self.defused + self.other
    }
    fn add(&mut self, w: WinCondition) {
        match w {
            WinCondition::Elimination => self.elimination += 1,
            WinCondition::Time => self.time += 1,
            WinCondition::Planted => self.planted += 1,
            WinCondition::DefuserDisabled | WinCondition::BombDefused => self.defused += 1,
            WinCondition::Unknown => self.other += 1,
        }
    }
    fn merge(&mut self, o: &LostEnds) {
        self.elimination += o.elimination;
        self.time += o.time;
        self.planted += o.planted;
        self.defused += o.defused;
        self.other += o.other;
    }
}

/// Kills per weapon slot.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct SlotKills {
    pub primary: u32,
    pub secondary: u32,
    pub gadget: u32,
    pub ability: u32,
    pub melee: u32,
    pub explosion: u32,
    pub unknown: u32,
}

impl SlotKills {
    pub fn total(&self) -> u32 {
        self.primary + self.secondary + self.gadget + self.ability + self.melee + self.explosion + self.unknown
    }
    fn merge(&mut self, o: &SlotKills) {
        self.primary += o.primary;
        self.secondary += o.secondary;
        self.gadget += o.gadget;
        self.ability += o.ability;
        self.melee += o.melee;
        self.explosion += o.explosion;
        self.unknown += o.unknown;
    }
}

/// Kills and headshots with one named weapon.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct WeaponTally {
    pub slot: Option<String>,
    pub kills: u32,
    pub headshots: u32,
}

/// Index of a side in the `[attack, defense]` arrays; `None` for an unknown side.
pub fn side_idx(s: Side) -> Option<usize> {
    match s {
        Side::Attack => Some(0),
        Side::Defense => Some(1),
        Side::Unknown => None,
    }
}

/// Additive tally of the deep metrics over rounds.
#[derive(Debug, Clone, Default)]
pub struct Deep {
    pub rounds: u32,
    /// Opening duels per side `[attack, defense]`: `x` = first kills, `n` = first kills + first deaths.
    pub entry: [Rate; 2],
    /// `x` = deaths that a teammate traded within the trade window, `n` = deaths.
    pub traded: Rate,
    /// `x` = enemy kills of a teammate that the player traded, `n` = such kills while the player
    /// was still alive.
    pub trades_given: Rate,
    /// Rounds with a kill, survival or a traded death.
    pub kost: Rate,
    /// Team round wins in rounds the player died.
    pub after_death: Rate,
    /// Team round wins in rounds the player survived.
    pub after_survive: Rate,
    /// Lost rounds by end type, `[attack, defense]`.
    pub lost: [LostEnds; 2],
    pub slot_kills: SlotKills,
    /// `x` = headshots, `n` = kills made with a primary weapon.
    pub primary_hs: Rate,
    pub weapons: BTreeMap<String, WeaponTally>,
    /// Deaths in the first 30 s (`x`) of deaths with a known time (`n`).
    pub early_death: Rate,
    /// Deaths in the last 30 s.
    pub late_death: Rate,
    pub early_kill: Rate,
    pub late_kill: Rate,
    /// Deaths by the killer's operator key.
    pub deaths_by_op: BTreeMap<String, u32>,
    /// Rounds each operator key was on the enemy team.
    pub faced_by_op: BTreeMap<String, u32>,
    /// Clutch results by opponents alive: index 0 = 1v1 ... index 4 = 1v5 or more.
    pub clutch: [Rate; 5],
    /// Round wins per `(map key, side index)`.
    pub map_side: BTreeMap<(String, usize), Rate>,
    /// Defense round wins per `(map key, site)`.
    pub site: BTreeMap<(String, String), Rate>,
}

fn op_key(id: u64) -> String {
    names::operator_key(id).unwrap_or_else(|| id.to_string())
}

impl Deep {
    /// Builds the tally over `set`, counting only the rounds that pass the filter.
    pub fn build(f: &Filter, set: &[&FullMatch]) -> Deep {
        let mut d = Deep::default();
        for m in set {
            let map_key = names::map_key(m.map_id).unwrap_or_else(|| m.map_id.to_string());
            for r in &m.rounds {
                if let Some(p) = selected(f, r, &m.me_profile_id) {
                    d.add_round(&map_key, r, p);
                }
            }
        }
        d
    }

    pub fn add_round(&mut self, map_key: &str, r: &FullRound, p: &RoundPlayer) {
        let me = p.profile_id.as_str();
        let won = r.winner_team == Some(p.team_index);
        let idx = side_idx(p.side);
        self.rounds += 1;

        // Opening duels.
        if let Some(i) = idx {
            if p.first_kill || p.first_death {
                self.entry[i].add(p.first_kill);
            }
        }

        // Who is on which team.
        let team_of: HashMap<&str, usize> =
            r.players.iter().map(|q| (q.profile_id.as_str(), q.team_index)).collect();
        let my_team = p.team_index;
        let my_death = r.kills.iter().find(|k| k.victim == me);

        // Trades and KOST.
        let traded = p.deaths > 0 && is_traded(r, me);
        if p.deaths > 0 {
            self.traded.add(traded);
        }
        self.kost.add(p.kills > 0 || p.survived || traded);

        // Trades given: enemy kills of a teammate while the player was alive.
        for k in &r.kills {
            if k.victim == me
                || team_of.get(k.victim.as_str()) != Some(&my_team)
                || team_of.get(k.killer.as_str()).is_none_or(|t| *t == my_team)
            {
                continue;
            }
            let alive = match my_death {
                Some(dk) => dk.time_s < k.time_s,
                // Died without a kill event: unknown, do not count the chance.
                None => p.survived,
            };
            if !alive {
                continue;
            }
            let given = r.kills.iter().any(|m| {
                m.killer == me
                    && m.victim == k.killer
                    && (0..=TRADE_WINDOW_S).contains(&(k.time_s - m.time_s))
            });
            self.trades_given.add(given);
        }

        // Death cost.
        if p.deaths > 0 {
            self.after_death.add(won);
        } else {
            self.after_survive.add(won);
        }

        // How lost rounds ended.
        if let (Some(i), Some(w)) = (idx, r.winner_team) {
            if w != p.team_index {
                self.lost[i].add(r.win_condition);
            }
            self.map_side.entry((map_key.to_string(), i)).or_default().add(won);
            if i == 1 && !r.site.trim().is_empty() {
                self.site.entry((map_key.to_string(), r.site.trim().to_string())).or_default().add(won);
            }
        }

        // The player's kills: weapons and timing.
        for k in r.kills.iter().filter(|k| k.killer == me) {
            if team_of.get(k.victim.as_str()) == Some(&my_team) {
                continue; // team kill
            }
            let slot = k.weapon_slot.as_deref();
            match slot {
                Some("primary") => {
                    self.slot_kills.primary += 1;
                    self.primary_hs.add(k.headshot);
                }
                Some("secondary") => self.slot_kills.secondary += 1,
                Some("gadget") => self.slot_kills.gadget += 1,
                Some("ability") => self.slot_kills.ability += 1,
                Some("melee") => self.slot_kills.melee += 1,
                Some("explosion") => self.slot_kills.explosion += 1,
                _ => self.slot_kills.unknown += 1,
            }
            if let Some(label) = crate::details::kill_weapon_label(k, p.operator_id) {
                let w = self.weapons.entry(label.name).or_default();
                w.slot = w.slot.clone().or_else(|| slot.map(String::from));
                w.kills += 1;
                w.headshots += k.headshot as u32;
            }
            self.early_kill.add(k.time_s >= EARLY_S);
            self.late_kill.add(k.time_s <= LATE_S);
        }

        // The player's death: timing and the killer's operator.
        if let Some(dk) = my_death {
            self.early_death.add(dk.time_s >= EARLY_S);
            self.late_death.add(dk.time_s <= LATE_S);
            if team_of.get(dk.killer.as_str()).is_some_and(|t| *t != my_team) {
                if let Some(killer) = r.players.iter().find(|q| q.profile_id == dk.killer) {
                    *self.deaths_by_op.entry(op_key(killer.operator_id)).or_default() += 1;
                }
            }
        }
        for q in r.players.iter().filter(|q| q.team_index != my_team) {
            *self.faced_by_op.entry(op_key(q.operator_id)).or_default() += 1;
        }

        // Clutches.
        for c in round_clutches(r).iter().filter(|c| c.profile_id == me) {
            let i = (c.vs.clamp(1, 5) - 1) as usize;
            self.clutch[i].add(c.won);
        }
    }

    pub fn merge(&mut self, o: &Deep) {
        self.rounds += o.rounds;
        for i in 0..2 {
            self.entry[i].merge(&o.entry[i]);
            self.lost[i].merge(&o.lost[i]);
        }
        self.traded.merge(&o.traded);
        self.trades_given.merge(&o.trades_given);
        self.kost.merge(&o.kost);
        self.after_death.merge(&o.after_death);
        self.after_survive.merge(&o.after_survive);
        self.slot_kills.merge(&o.slot_kills);
        self.primary_hs.merge(&o.primary_hs);
        for (k, w) in &o.weapons {
            let e = self.weapons.entry(k.clone()).or_default();
            e.slot = e.slot.clone().or_else(|| w.slot.clone());
            e.kills += w.kills;
            e.headshots += w.headshots;
        }
        self.early_death.merge(&o.early_death);
        self.late_death.merge(&o.late_death);
        self.early_kill.merge(&o.early_kill);
        self.late_kill.merge(&o.late_kill);
        for (k, v) in &o.deaths_by_op {
            *self.deaths_by_op.entry(k.clone()).or_default() += v;
        }
        for (k, v) in &o.faced_by_op {
            *self.faced_by_op.entry(k.clone()).or_default() += v;
        }
        for i in 0..5 {
            self.clutch[i].merge(&o.clutch[i]);
        }
        for (k, v) in &o.map_side {
            self.map_side.entry(k.clone()).or_default().merge(v);
        }
        for (k, v) in &o.site {
            self.site.entry(k.clone()).or_default().merge(v);
        }
    }

    /// Opening duels over both sides.
    pub fn entry_all(&self) -> Rate {
        self.entry[0].plus(self.entry[1])
    }
    /// Clutches against two or more opponents.
    pub fn clutch_multi(&self) -> Rate {
        self.clutch[1..].iter().fold(Rate::default(), |a, r| a.plus(*r))
    }
    pub fn clutch_all(&self) -> Rate {
        self.clutch.iter().fold(Rate::default(), |a, r| a.plus(*r))
    }
}

// ---------------------------------------------------------------- Overview DTOs

fn r1(x: f64) -> f64 {
    (x * 10.0).round() / 10.0
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RateView {
    pub x: u32,
    pub n: u32,
    pub pct: f64,
}

impl From<Rate> for RateView {
    fn from(r: Rate) -> Self {
        RateView { x: r.x, n: r.n, pct: r1(r.pct()) }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EndsView {
    pub lost: u32,
    pub elimination: u32,
    pub time: u32,
    pub planted: u32,
    pub defused: u32,
}

impl From<LostEnds> for EndsView {
    fn from(l: LostEnds) -> Self {
        EndsView {
            lost: l.total(),
            elimination: l.elimination,
            time: l.time,
            planted: l.planted,
            defused: l.defused + l.other,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WeaponView {
    pub name: String,
    /// `primary` | `secondary` | ... ; `None` when unknown.
    pub slot: Option<String>,
    pub kills: u32,
    pub hs_pct: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RivalView {
    pub operator_key: String,
    pub name: String,
    /// Times this enemy operator killed the player.
    pub deaths: u32,
    /// Rounds this operator was on the enemy team.
    pub faced: u32,
    /// `deaths / faced` in percent.
    pub pct: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ClutchView {
    /// Opponents alive (5 = five or more).
    pub vs: u32,
    pub won: u32,
    pub total: u32,
    pub pct: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SlotView {
    pub primary: u32,
    pub secondary: u32,
    pub gadget: u32,
    pub ability: u32,
    #[serde(default)]
    pub melee: u32,
    #[serde(default)]
    pub explosion: u32,
    pub unknown: u32,
}

/// Compact Overview numbers for the sections "Round impact", "Weapons", "Timing", "Rivals".
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeepStats {
    pub rounds: u32,
    /// Opening duel win rate over both sides (`x` won of `n` duels).
    pub entry: RateView,
    pub entry_attack: RateView,
    pub entry_defense: RateView,
    /// Deaths a teammate traded.
    pub traded: RateView,
    /// Teammate deaths the player traded.
    pub trades_given: RateView,
    pub kost: RateView,
    /// Rounds that ended with a death, no kill and no trade.
    pub no_impact: RateView,
    /// Team round wins when the player died.
    pub after_death_win: RateView,
    /// Team round wins when the player survived.
    pub after_survive_win: RateView,
    pub lost_attack: EndsView,
    pub lost_defense: EndsView,
    pub slot_kills: SlotView,
    pub weapons: Vec<WeaponView>,
    pub early_death: RateView,
    pub late_death: RateView,
    pub early_kill: RateView,
    pub late_kill: RateView,
    pub rivals: Vec<RivalView>,
    pub clutch_by_opponents: Vec<ClutchView>,
}

/// Weapons listed on the Overview.
const MAX_WEAPONS: usize = 6;
/// Rival operators listed on the Overview.
const MAX_RIVALS: usize = 5;

impl Deep {
    pub fn view(&self) -> DeepStats {
        let mut weapons: Vec<WeaponView> = self
            .weapons
            .iter()
            .map(|(name, w)| WeaponView {
                name: name.clone(),
                slot: w.slot.clone(),
                kills: w.kills,
                hs_pct: r1(if w.kills == 0 { 0.0 } else { 100.0 * w.headshots as f64 / w.kills as f64 }),
            })
            .collect();
        weapons.sort_by(|a, b| b.kills.cmp(&a.kills).then_with(|| a.name.cmp(&b.name)));
        weapons.truncate(MAX_WEAPONS);

        let mut rivals: Vec<RivalView> = self
            .deaths_by_op
            .iter()
            .filter(|(_, d)| **d >= 2)
            .map(|(key, d)| {
                let faced = self.faced_by_op.get(key).copied().unwrap_or(0);
                RivalView {
                    operator_key: key.clone(),
                    name: names::operator_name_by_key(key).unwrap_or_else(|| match key.parse::<u64>() {
                        Ok(id) => format!("Operator #{id}"),
                        Err(_) => key.clone(),
                    }),
                    deaths: *d,
                    faced,
                    pct: r1(if faced == 0 { 0.0 } else { 100.0 * *d as f64 / faced as f64 }),
                }
            })
            .collect();
        rivals.sort_by(|a, b| b.deaths.cmp(&a.deaths).then_with(|| a.operator_key.cmp(&b.operator_key)));
        rivals.truncate(MAX_RIVALS);

        let clutch_by_opponents = self
            .clutch
            .iter()
            .enumerate()
            .map(|(i, r)| ClutchView { vs: i as u32 + 1, won: r.x, total: r.n, pct: r1(r.pct()) })
            .collect();

        DeepStats {
            rounds: self.rounds,
            entry: self.entry_all().into(),
            entry_attack: self.entry[0].into(),
            entry_defense: self.entry[1].into(),
            traded: self.traded.into(),
            trades_given: self.trades_given.into(),
            kost: self.kost.into(),
            no_impact: Rate { x: self.kost.n - self.kost.x, n: self.kost.n }.into(),
            after_death_win: self.after_death.into(),
            after_survive_win: self.after_survive.into(),
            lost_attack: self.lost[0].into(),
            lost_defense: self.lost[1].into(),
            slot_kills: SlotView {
                primary: self.slot_kills.primary,
                secondary: self.slot_kills.secondary,
                gadget: self.slot_kills.gadget,
                ability: self.slot_kills.ability,
                melee: self.slot_kills.melee,
                explosion: self.slot_kills.explosion,
                unknown: self.slot_kills.unknown,
            },
            weapons,
            early_death: self.early_death.into(),
            late_death: self.late_death.into(),
            early_kill: self.early_kill.into(),
            late_kill: self.late_kill.into(),
            rivals,
            clutch_by_opponents,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::full::KillEvent;

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

    fn kill(t: i64, killer: &str, victim: &str, hs: bool, slot: Option<&str>, weapon: Option<&str>) -> KillEvent {
        KillEvent {
            time_s: t,
            killer: killer.into(),
            victim: victim.into(),
            headshot: hs,
            weapon_id: None,
            weapon: weapon.map(String::from),
            weapon_slot: slot.map(String::from),
        }
    }

    /// me + mate (team 0, attack) vs f1 + f2 (team 1). Mate dies at 160, I trade at 155, then I
    /// die at 40 to f2.
    fn round() -> FullRound {
        let mut me = pl("me", 0, 1, Side::Attack, 1, true);
        me.first_kill = false;
        let mut mate = pl("mate", 0, 2, Side::Attack, 0, true);
        mate.first_death = true;
        let mut f1 = pl("f1", 1, 3, Side::Defense, 1, true);
        f1.first_kill = true;
        let f2 = pl("f2", 1, 4, Side::Defense, 1, false);
        FullRound {
            number: 1,
            start_time_ms: 0,
            winner_team: Some(1),
            win_condition: WinCondition::Time,
            site: "Bar".into(),
            my_side: Side::Attack,
            players: vec![me, mate, f1, f2],
            kills: vec![
                kill(160, "f1", "mate", false, None, None),
                kill(155, "me", "f1", true, Some("primary"), Some("R4-C")),
                kill(40, "f2", "me", false, None, None),
            ],
        }
    }

    fn tally(r: &FullRound) -> Deep {
        let mut d = Deep::default();
        let p = r.players.iter().find(|p| p.profile_id == "me").unwrap();
        d.add_round("bank", r, p);
        d
    }

    #[test]
    fn trades_death_cost_and_loss_type() {
        let d = tally(&round());
        // Mate died to f1 and I killed f1 five seconds later: one trade given out of one chance.
        assert_eq!(d.trades_given, Rate { x: 1, n: 1 });
        // My own death at 40 had no teammate left to trade it.
        assert_eq!(d.traded, Rate { x: 0, n: 1 });
        // I got a kill, so the round counts for KOST.
        assert_eq!(d.kost, Rate { x: 1, n: 1 });
        assert_eq!(d.after_death, Rate { x: 0, n: 1 });
        assert_eq!(d.lost[0].time, 1);
        assert_eq!(d.lost[0].total(), 1);
        assert_eq!(d.lost[1].total(), 0);
        assert_eq!(d.map_side[&("bank".to_string(), 0)], Rate { x: 0, n: 1 });
        // Attack side, so no defense site result.
        assert!(d.site.is_empty());
    }

    #[test]
    fn weapons_timing_and_rival_operator() {
        let d = tally(&round());
        assert_eq!(d.slot_kills.primary, 1);
        assert_eq!(d.primary_hs, Rate { x: 1, n: 1 });
        assert_eq!(d.weapons["R4-C"], WeaponTally { slot: Some("primary".into()), kills: 1, headshots: 1 });
        // The kill came at 155 s left (early), the death at 40 s left (neither early nor late).
        assert_eq!(d.early_kill, Rate { x: 1, n: 1 });
        assert_eq!(d.early_death, Rate { x: 0, n: 1 });
        assert_eq!(d.late_death, Rate { x: 0, n: 1 });
        // f2 (operator 4) killed me; both enemy operators were faced.
        let key4 = op_key(4);
        assert_eq!(d.deaths_by_op[&key4], 1);
        assert_eq!(d.faced_by_op.values().sum::<u32>(), 2);
    }

    #[test]
    fn entry_duels_and_clutches_and_merge() {
        let mut r = round();
        r.players.iter_mut().find(|p| p.profile_id == "me").unwrap().first_kill = true;
        let mut d = tally(&r);
        // Mate is dead and I am the last alive against f1 and f2: a 1v2 that was lost.
        assert_eq!(d.clutch[1], Rate { x: 0, n: 1 });
        assert_eq!(d.entry[0], Rate { x: 1, n: 1 });
        let other = tally(&r);
        d.merge(&other);
        assert_eq!(d.rounds, 2);
        assert_eq!(d.entry[0], Rate { x: 2, n: 2 });
        assert_eq!(d.clutch_multi(), Rate { x: 0, n: 2 });
        assert_eq!(d.weapons["R4-C"].kills, 2);
    }

    #[test]
    fn defense_site_and_view() {
        let mut r = round();
        r.my_side = Side::Defense;
        for p in &mut r.players {
            p.side = if p.team_index == 0 { Side::Defense } else { Side::Attack };
        }
        r.winner_team = Some(0);
        r.win_condition = WinCondition::Elimination;
        let d = tally(&r);
        assert_eq!(d.site[&("bank".to_string(), "Bar".to_string())], Rate { x: 1, n: 1 });
        let v = d.view();
        assert_eq!(v.rounds, 1);
        assert_eq!(v.no_impact.x, 0);
        assert_eq!(v.weapons.len(), 1);
        assert_eq!(v.clutch_by_opponents.len(), 5);
        let json = serde_json::to_value(&v).unwrap();
        for k in ["entryAttack", "tradesGiven", "afterDeathWin", "lostAttack", "earlyDeath", "rivals", "clutchByOpponents"] {
            assert!(json.get(k).is_some(), "missing {k}");
        }
    }

    #[test]
    fn dead_player_cannot_get_a_trade_chance() {
        // I die first at 160, a teammate dies at 100 to f1: I was not alive, so no chance.
        let mut r = round();
        r.kills = vec![
            kill(160, "f1", "me", false, None, None),
            kill(100, "f1", "mate", false, None, None),
        ];
        let d = tally(&r);
        assert_eq!(d.trades_given, Rate::default());
    }
}
