//! Full match model: all players, rounds and the kill feed (v2 spec section 1).
//! The tracked-player `Match` in `model.rs` stays as it is; this is the richer view.

use crate::convert::{dissect_local_to_utc, ConvertError};
use crate::model::Side;
use crate::raw::{RawMatch, RawPlayer, RawRound};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;

/// How a round ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WinCondition {
    /// The losing team was wiped out.
    Elimination,
    /// Defenders disabled the planted defuser (dissect: `DisabledDefuser`).
    DefuserDisabled,
    /// Bomb mode: the bomb was defused (dissect: `DefusedBomb`).
    BombDefused,
    /// Attackers planted and the defuser was not disabled.
    Planted,
    /// Defenders held until the clock ran out.
    Time,
    Unknown,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlayerInfo {
    pub profile_id: String,
    pub username: String,
    /// Stable for the whole match (sides swap, team indexes do not).
    pub team_index: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RoundPlayer {
    pub profile_id: String,
    pub team_index: usize,
    pub operator_id: u64,
    pub side: Side,
    pub kills: u32,
    /// 0 or 1.
    pub deaths: u32,
    pub headshots: u32,
    pub first_kill: bool,
    pub first_death: bool,
    pub survived: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KillEvent {
    /// Seconds left on the round clock (counts down, so larger = earlier).
    pub time_s: i64,
    /// Profile IDs (the username when dissect gave no profile ID).
    pub killer: String,
    pub victim: String,
    pub headshot: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FullRound {
    /// 1-based.
    pub number: u32,
    /// Team index of the round winner, if known.
    pub winner_team: Option<usize>,
    pub win_condition: WinCondition,
    pub site: String,
    /// Side of the tracked player this round.
    pub my_side: Side,
    pub players: Vec<RoundPlayer>,
    /// Sorted from the round start to its end (largest `time_s` first).
    pub kills: Vec<KillEvent>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FullMatch {
    pub match_id: String,
    /// Real UTC, same conversion as `Match::timestamp`.
    pub timestamp: String,
    pub match_type: String,
    pub gamemode: String,
    pub map_id: u64,
    pub score_us: u32,
    pub score_them: u32,
    pub won: bool,
    pub me_profile_id: String,
    pub players: Vec<PlayerInfo>,
    pub rounds: Vec<FullRound>,
}

impl FullMatch {
    pub fn player(&self, profile_id: &str) -> Option<&PlayerInfo> {
        self.players.iter().find(|p| p.profile_id == profile_id)
    }
    pub fn my_team_index(&self) -> Option<usize> {
        self.player(&self.me_profile_id).map(|p| p.team_index)
    }
}

pub fn read_full_match_file(path: &Path) -> Result<FullMatch, ConvertError> {
    let text = std::fs::read_to_string(path)
        .map_err(|e| ConvertError::Io(path.display().to_string(), e))?;
    full_match_from_json(&text)
}

pub fn full_match_from_json(json: &str) -> Result<FullMatch, ConvertError> {
    full_match_from_raw(&serde_json::from_str::<RawMatch>(json)?)
}

/// Key used for a player everywhere in the full model.
fn pid(p: &RawPlayer) -> String {
    if p.profile_id.is_empty() {
        p.username.clone()
    } else {
        p.profile_id.clone()
    }
}

fn side_of(role: Option<&str>) -> Side {
    match role {
        Some("Attack") => Side::Attack,
        Some("Defense") => Side::Defense,
        _ => Side::Unknown,
    }
}

pub fn full_match_from_raw(raw: &RawMatch) -> Result<FullMatch, ConvertError> {
    let first = raw.rounds.first().ok_or(ConvertError::NoRounds)?;
    let me_profile_id = first.recording_profile_id.clone();

    let mut players: Vec<PlayerInfo> = Vec::new();
    for r in &raw.rounds {
        for p in &r.players {
            let id = pid(p);
            if !players.iter().any(|q| q.profile_id == id) {
                players.push(PlayerInfo {
                    profile_id: id,
                    username: p.username.clone(),
                    team_index: p.team_index,
                });
            }
        }
    }

    let mut rounds: Vec<FullRound> = raw
        .rounds
        .iter()
        .map(|r| convert_round(r, &me_profile_id))
        .collect();
    rounds.sort_by_key(|r| r.number);

    // Same winner logic as `match_from_raw`: higher final score of the tracked team, else the
    // `won` flag of the last round.
    let my_team = players
        .iter()
        .find(|p| p.profile_id == me_profile_id)
        .map(|p| p.team_index);
    let last = raw.rounds.iter().max_by_key(|r| r.round_number).unwrap_or(first);
    let (score_us, score_them, won) = match my_team {
        Some(t) => {
            let mine = last.teams.get(t);
            let other = last.teams.get(1usize.saturating_sub(t));
            let (a, b) = (mine.map_or(0, |x| x.score), other.map_or(0, |x| x.score));
            (a, b, if a != b { a > b } else { mine.is_some_and(|x| x.won) })
        }
        None => {
            log::warn!("recording player not found in the player list");
            (0, 0, false)
        }
    };

    Ok(FullMatch {
        match_id: first.match_id.clone(),
        timestamp: dissect_local_to_utc(&first.timestamp),
        match_type: first.match_type.name.clone(),
        gamemode: first.gamemode.name.clone(),
        map_id: first.map.id,
        score_us,
        score_them,
        won,
        me_profile_id,
        players,
        rounds,
    })
}

fn convert_round(r: &RawRound, me_profile_id: &str) -> FullRound {
    let by_name: HashMap<&str, String> =
        r.players.iter().map(|p| (p.username.as_str(), pid(p))).collect();
    let id_of = |name: &str| by_name.get(name).cloned().unwrap_or_else(|| name.to_string());

    let mut kills: Vec<KillEvent> = r
        .match_feedback
        .iter()
        .filter(|e| e.kind.name == "Kill")
        .map(|e| KillEvent {
            time_s: e.time_in_seconds,
            killer: id_of(&e.username),
            victim: id_of(&e.target),
            headshot: e.headshot,
        })
        .collect();
    // Stable: ties keep the listed order. Larger time_s = earlier in the round.
    kills.sort_by_key(|k| std::cmp::Reverse(k.time_s));
    let first_kill = kills.first().map(|k| (k.killer.clone(), k.victim.clone()));

    let round_players: Vec<RoundPlayer> = r
        .players
        .iter()
        .map(|p| {
            let id = pid(p);
            let stats = r.stats.iter().find(|s| s.username == p.username);
            let died = stats.is_some_and(|s| s.died);
            RoundPlayer {
                team_index: p.team_index,
                side: side_of(r.teams.get(p.team_index).map(|t| t.role.as_str())),
                operator_id: p.operator.id,
                kills: stats.map_or(0, |s| s.kills),
                deaths: died as u32,
                headshots: stats.map_or(0, |s| s.headshots),
                first_kill: first_kill.as_ref().is_some_and(|(k, _)| *k == id),
                first_death: first_kill.as_ref().is_some_and(|(_, v)| *v == id),
                survived: !died,
                profile_id: id,
            }
        })
        .collect();

    let winner_team = r.teams.iter().position(|t| t.won);
    let my_team = r
        .players
        .iter()
        .find(|p| pid(p) == me_profile_id)
        .map(|p| p.team_index);
    let my_side = side_of(my_team.and_then(|t| r.teams.get(t)).map(|t| t.role.as_str()));

    FullRound {
        number: r.round_number + 1,
        winner_team,
        win_condition: win_condition(r, winner_team, &round_players),
        site: r.site.clone(),
        my_side,
        players: round_players,
        kills,
    }
}

fn win_condition(r: &RawRound, winner: Option<usize>, players: &[RoundPlayer]) -> WinCondition {
    let Some(w) = winner else {
        return WinCondition::Unknown;
    };
    match r.teams[w].win_condition.as_str() {
        "DisabledDefuser" => return WinCondition::DefuserDisabled,
        "DefusedBomb" => return WinCondition::BombDefused,
        "" => {}
        other => log::debug!("unknown dissect win condition {other:?}"),
    }
    // Is the whole losing team dead?
    let loser_all_dead = {
        let mut any = false;
        let mut all_dead = true;
        for p in &r.players {
            if p.team_index != w {
                any = true;
                let dead = players
                    .iter()
                    .any(|rp| rp.profile_id == pid(p) && !rp.survived);
                all_dead &= dead;
            }
        }
        any && all_dead
    };
    if loser_all_dead {
        return WinCondition::Elimination;
    }
    let planted = r
        .match_feedback
        .iter()
        .any(|e| e.kind.name == "DefuserPlantComplete");
    match side_of(Some(r.teams[w].role.as_str())) {
        Side::Attack if planted => WinCondition::Planted,
        // Attackers cannot win on time, so without a plant the last defender must be dead
        // (dissect sometimes misses a death flag).
        Side::Attack => WinCondition::Elimination,
        Side::Defense => WinCondition::Time,
        _ => WinCondition::Unknown,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const JSON: &str = r#"{"rounds":[{"timestamp":"2026-10-24T12:08:22Z","matchType":{"name":"Ranked","id":2},
"map":{"name":"X","id":7},"gamemode":{"name":"Bomb","id":1},"recordingProfileID":"a1","matchID":"m1",
"roundNumber":0,"site":"Lobby",
"teams":[{"score":1,"won":true,"role":"Attack"},{"score":0,"won":false,"role":"Defense"}],
"players":[
 {"profileID":"a1","username":"A1","teamIndex":0,"operator":{"id":11}},
 {"profileID":"a2","username":"A2","teamIndex":0,"operator":{"id":12}},
 {"profileID":"b1","username":"B1","teamIndex":1,"operator":{"id":21}},
 {"profileID":"b2","username":"B2","teamIndex":1,"operator":{"id":22}}],
"matchFeedback":[
 {"type":{"name":"Kill"},"username":"A1","target":"B1","headshot":true,"timeInSeconds":150},
 {"type":{"name":"Kill"},"username":"B2","target":"A2","timeInSeconds":120},
 {"type":{"name":"Kill"},"username":"A1","target":"B2","timeInSeconds":100}],
"stats":[{"username":"A1","kills":2,"died":false,"headshots":1},{"username":"A2","kills":0,"died":true},
 {"username":"B1","kills":0,"died":true},{"username":"B2","kills":1,"died":true}]}]}"#;

    #[test]
    fn builds_full_model() {
        let m = full_match_from_json(JSON).unwrap();
        assert_eq!(m.players.len(), 4);
        assert_eq!((m.score_us, m.score_them, m.won), (1, 0, true));
        assert_eq!(m.map_id, 7);
        let r = &m.rounds[0];
        assert_eq!(r.number, 1);
        assert_eq!(r.site, "Lobby");
        assert_eq!(r.my_side, Side::Attack);
        assert_eq!(r.winner_team, Some(0));
        assert_eq!(r.win_condition, WinCondition::Elimination);
        assert_eq!(r.kills.len(), 3);
        assert_eq!(r.kills[0].killer, "a1");
        assert!(r.kills[0].headshot);
        let a1 = r.players.iter().find(|p| p.profile_id == "a1").unwrap();
        assert!(a1.first_kill && a1.survived && a1.kills == 2);
        let b1 = r.players.iter().find(|p| p.profile_id == "b1").unwrap();
        assert!(b1.first_death && b1.deaths == 1);
    }
}
