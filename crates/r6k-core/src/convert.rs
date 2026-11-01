//! Raw dissect output -> `Match` for the tracked (recording) player.

use crate::model::{Match, Round, Side};
use crate::raw::{RawFeedback, RawMatch, RawPlayer, RawRound};
use chrono::{LocalResult, NaiveDateTime, TimeZone, Utc};
use std::path::Path;

#[derive(Debug, thiserror::Error)]
pub enum ConvertError {
    #[error("cannot read {0}: {1}")]
    Io(String, std::io::Error),
    #[error("invalid dissect JSON: {0}")]
    Json(#[from] serde_json::Error),
    #[error("match has no rounds")]
    NoRounds,
}

pub fn read_match_file(path: &Path) -> Result<Match, ConvertError> {
    let text = std::fs::read_to_string(path)
        .map_err(|e| ConvertError::Io(path.display().to_string(), e))?;
    match_from_json(&text)
}

pub fn match_from_json(json: &str) -> Result<Match, ConvertError> {
    match_from_raw(&serde_json::from_str::<RawMatch>(json)?)
}

pub fn match_from_raw(raw: &RawMatch) -> Result<Match, ConvertError> {
    let first = raw.rounds.first().ok_or(ConvertError::NoRounds)?;
    let mut rounds: Vec<Round> = raw.rounds.iter().filter_map(convert_round).collect();
    rounds.sort_by_key(|r| r.number);

    // Winner = tracked team with the higher final score; fall back to the last round.
    let won = raw
        .rounds
        .last()
        .and_then(|r| {
            let me = tracked(r)?;
            let mine = r.teams.get(me.team_index)?;
            let other = r.teams.get(1usize.saturating_sub(me.team_index))?;
            Some(if mine.score != other.score {
                mine.score > other.score
            } else {
                mine.won
            })
        })
        .unwrap_or(false);

    Ok(Match {
        match_id: first.match_id.clone(),
        timestamp: dissect_local_to_utc(&first.timestamp),
        match_type: first.match_type.name.clone(),
        gamemode: first.gamemode.name.clone(),
        map_id: first.map.id,
        won,
        rounds,
    })
}

const TS_PARSE: &str = "%Y-%m-%dT%H:%M:%S";
const TS_FORMAT: &str = "%Y-%m-%dT%H:%M:%SZ";

/// Converts a dissect timestamp to real UTC. Dissect writes the recording PC's local wall
/// clock but appends a misleading `Z`; this reads it in the system time zone (the app runs on
/// the recording PC). Unparseable input is returned unchanged.
pub fn dissect_local_to_utc(ts: &str) -> String {
    dissect_local_to_utc_in(ts, &chrono::Local)
}

/// Same as [`dissect_local_to_utc`] with an explicit time zone (injectable for tests).
pub fn dissect_local_to_utc_in<Tz: TimeZone>(ts: &str, tz: &Tz) -> String {
    let Ok(naive) = NaiveDateTime::parse_from_str(ts.trim_end_matches('Z'), TS_PARSE) else {
        log::warn!("unparseable replay timestamp {ts:?}; kept as is");
        return ts.to_string();
    };
    match tz.from_local_datetime(&naive) {
        LocalResult::Single(t) | LocalResult::Ambiguous(t, _) => {
            t.with_timezone(&Utc).format(TS_FORMAT).to_string()
        }
        LocalResult::None => {
            log::warn!("replay timestamp {ts:?} does not exist locally (DST gap); kept as is");
            ts.to_string()
        }
    }
}

/// Formats a model (UTC) timestamp as `YYYY-MM-DD HH:MM` in the system time zone, for humans.
pub fn utc_to_local_display(ts: &str) -> String {
    utc_to_display_in(ts, &chrono::Local)
}

/// Same as [`utc_to_local_display`] with an explicit time zone.
pub fn utc_to_display_in<Tz: TimeZone>(ts: &str, tz: &Tz) -> String
where
    Tz::Offset: std::fmt::Display,
{
    match NaiveDateTime::parse_from_str(ts.trim_end_matches('Z'), TS_PARSE) {
        Ok(n) => Utc.from_utc_datetime(&n).with_timezone(tz).format("%Y-%m-%d %H:%M").to_string(),
        Err(_) => ts.replace('T', " ").chars().take(16).collect(),
    }
}

fn tracked(r: &RawRound) -> Option<&RawPlayer> {
    r.players
        .iter()
        .find(|p| p.profile_id == r.recording_profile_id)
}

fn convert_round(r: &RawRound) -> Option<Round> {
    let me = match tracked(r) {
        Some(p) => p,
        None => {
            log::warn!(
                "round {}: recording player not in player list, skipped",
                r.round_number
            );
            return None;
        }
    };
    let stats = r.stats.iter().find(|s| s.username == me.username);
    let team = r.teams.get(me.team_index);
    let side = match team.map(|t| t.role.as_str()) {
        Some("Attack") => Side::Attack,
        Some("Defense") => Side::Defense,
        _ => Side::Unknown,
    };
    let (first_blood, first_death) = first_kill_flags(&r.match_feedback, &me.username);
    let did = |kind: &str| {
        r.match_feedback
            .iter()
            .any(|e| e.kind.name == kind && e.username == me.username)
    };
    Some(Round {
        number: r.round_number + 1,
        side,
        operator_id: me.operator.id,
        kills: stats.map_or(0, |s| s.kills),
        deaths: stats.map_or(0, |s| s.died as u32),
        assists: stats.map_or(0, |s| s.assists),
        headshots: stats.map_or(0, |s| s.headshots),
        first_blood,
        first_death,
        planted: did("DefuserPlantComplete"),
        defused: did("DefuserDisableComplete"),
        won: team.is_some_and(|t| t.won),
    })
}

/// The round clock counts down, so the Kill event with the largest
/// `timeInSeconds` is the round's first kill. Returns
/// `(player got first kill, player was first killed)`.
pub fn first_kill_flags(feedback: &[RawFeedback], username: &str) -> (bool, bool) {
    let mut best: Option<&RawFeedback> = None;
    for e in feedback.iter().filter(|e| e.kind.name == "Kill") {
        // strict `>` keeps the earliest listed event on ties
        if best.is_none_or(|b| e.time_in_seconds > b.time_in_seconds) {
            best = Some(e);
        }
    }
    match best {
        Some(e) => (e.username == username, e.target == username),
        None => (false, false),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kill(by: &str, target: &str, t: i64) -> String {
        format!(
            r#"{{"type":{{"name":"Kill","id":0}},"username":"{by}","target":"{target}","timeInSeconds":{t}}}"#
        )
    }

    fn round_json(feedback: &[String]) -> String {
        format!(
            r#"{{"rounds":[{{"timestamp":"2026-10-24T12:08:22Z","matchType":{{"name":"Ranked","id":2}},
"map":{{"name":"X","id":999}},"gamemode":{{"name":"Bomb","id":1}},"recordingProfileID":"me-id","matchID":"m1",
"roundNumber":0,"teams":[{{"score":1,"won":true,"role":"Attack"}},{{"score":0,"won":false,"role":"Defense"}}],
"players":[{{"profileID":"me-id","username":"Me","teamIndex":0,"operator":{{"name":"?","id":424242}}}}],
"matchFeedback":[{}],
"stats":[{{"username":"Me","kills":2,"died":true,"assists":1,"headshots":1}}]}}]}}"#,
            feedback.join(",")
        )
    }

    #[test]
    fn dissect_local_time_becomes_utc() {
        use chrono::FixedOffset;
        let plus3 = FixedOffset::east_opt(3 * 3600).unwrap();
        assert_eq!(
            dissect_local_to_utc_in("2026-10-31T00:56:11Z", &plus3),
            "2026-10-30T21:56:11Z"
        );
        let minus5 = FixedOffset::west_opt(5 * 3600).unwrap();
        assert_eq!(
            dissect_local_to_utc_in("2026-10-31T22:30:00Z", &minus5),
            "2026-11-01T03:30:00Z"
        );
        assert_eq!(dissect_local_to_utc_in("garbage", &plus3), "garbage");
    }

    #[test]
    fn utc_displays_in_local_zone() {
        use chrono::FixedOffset;
        let plus3 = FixedOffset::east_opt(3 * 3600).unwrap();
        assert_eq!(utc_to_display_in("2026-10-30T21:56:11Z", &plus3), "2026-10-31 00:56");
    }

    #[test]
    fn first_blood_is_largest_time_in_seconds() {
        // Listed out of order on purpose: the 100s kill is the first one.
        let fb = vec![
            kill("A", "B", 40),
            kill("Me", "C", 100),
            kill("D", "Me", 70),
        ];
        let m = match_from_json(&round_json(&fb)).unwrap();
        assert!(m.rounds[0].first_blood);
        assert!(!m.rounds[0].first_death);
    }

    #[test]
    fn first_death_when_killed_first() {
        let fb = vec![kill("A", "Me", 110), kill("Me", "C", 60)];
        let m = match_from_json(&round_json(&fb)).unwrap();
        assert!(m.rounds[0].first_death);
        assert!(!m.rounds[0].first_blood);
    }

    #[test]
    fn unknown_operator_id_does_not_panic() {
        let m = match_from_json(&round_json(&[])).unwrap();
        assert_eq!(m.rounds[0].operator_id, 424242);
        assert_eq!(
            crate::names::operator_name(m.rounds[0].operator_id),
            "Operator #424242"
        );
        assert_eq!(m.rounds[0].number, 1);
        assert_eq!(m.rounds[0].side, Side::Attack);
        assert!(m.won);
        assert_eq!((m.kills(), m.deaths(), m.headshots()), (2, 1, 1));
    }
}
