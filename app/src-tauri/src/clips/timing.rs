//! Puts the events of one round on the wall clock (v4 spec, "Kill clip timing").
//!
//! A `.rec` file only knows the round clock: it counts down in real seconds (180 s of action in
//! every mode we clip), and after a defuser plant the same field shows the defuser countdown
//! (45 s). The wall clock comes from one of two anchors:
//!
//! * `RoundEnd`: the game writes the file when the round ends, so its modification time is the
//!   end of the round. Events are converted backwards from the event that ended the round.
//! * `RoundStart`: the header `starttime` plus a default prep offset. Used for old or imported
//!   rounds whose file time says nothing about the round.
//!
//! The result is not exact (the user accepted that); the clip window and the "Clip offset" setting
//! absorb the error.

use r6k_core::raw::RawRound;

/// Action time of a round in seconds (the round clock starts here and counts down).
pub const ROUND_LEN_S: i64 = 180;
/// Defuser countdown in seconds; the round clock shows it after the plant.
pub const DEFUSER_LEN_S: i64 = 45;
/// Default offset from the header `starttime` to the start of the action (prep phase plus load).
pub const PREP_S: i64 = 50;
/// A round file older than this is not "live": the rolling buffer cannot hold it anyway.
pub const LIVE_MAX_AGE_MS: i64 = 20 * 60_000;
/// A plausible time from the header `starttime` to the end of the round (file write).
pub const ROUND_MIN_MS: i64 = 30_000;
pub const ROUND_MAX_MS: i64 = 15 * 60_000;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EvKind {
    Kill,
    /// A death without a killer (fall, own grenade, disconnect).
    Death,
    PlantComplete,
    DisableComplete,
}

/// One feed event in the order the replay lists them (chronological).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Ev {
    pub kind: EvKind,
    /// Value of the round clock when the event happened.
    pub counter_s: i64,
    pub killer: String,
    pub victim: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Anchor {
    /// The round ended at this wall time (Unix ms): the file write time.
    RoundEnd(i64),
    /// The header `starttime` (Unix ms).
    RoundStart(i64),
}

/// Picks the anchor: a fresh file with a plausible distance to the header start means the file
/// time is the round end; otherwise fall back to the header start; otherwise nothing.
pub fn choose_anchor(mtime_ms: Option<i64>, start_ms: i64, now_ms: i64) -> Option<Anchor> {
    if let Some(m) = mtime_ms {
        let fresh = now_ms - m <= LIVE_MAX_AGE_MS && m <= now_ms + 60_000;
        let plausible = start_ms <= 0 || (ROUND_MIN_MS..=ROUND_MAX_MS).contains(&(m - start_ms));
        if fresh && plausible {
            return Some(Anchor::RoundEnd(m));
        }
    }
    (start_ms > 0).then_some(Anchor::RoundStart(start_ms))
}

fn ms(s: i64) -> i64 {
    s * 1000
}

/// Wall time (Unix ms) of every event, in the same order as `events`.
///
/// `ended_on_last_event`: the round ended right at the last listed event (a team was wiped out, or
/// the defuser was disabled). When false the round ended by the clock running out (action clock
/// at 0, or the defuser countdown at 0 after a plant). Only used with `RoundEnd`.
pub fn wall_times(events: &[Ev], anchor: Anchor, ended_on_last_event: bool) -> Vec<i64> {
    let plant = events.iter().position(|e| e.kind == EvKind::PlantComplete);
    match anchor {
        Anchor::RoundStart(start) => {
            let pre = |c: i64| start + ms(PREP_S) + ms(ROUND_LEN_S - c);
            let plant_wall = plant.map(|i| pre(events[i].counter_s));
            events
                .iter()
                .enumerate()
                .map(|(i, e)| match (plant, plant_wall) {
                    (Some(p), Some(pw)) if i > p => pw + ms(DEFUSER_LEN_S - e.counter_s),
                    _ => pre(e.counter_s),
                })
                .collect()
        }
        Anchor::RoundEnd(end) => match plant {
            None => {
                let c_end = if ended_on_last_event { events.last().map_or(0, |e| e.counter_s) } else { 0 };
                events.iter().map(|e| end - ms(e.counter_s - c_end)).collect()
            }
            Some(p) => {
                let c_plant = events[p].counter_s;
                // The plant moment such that the round ends at `end`.
                let plant_wall = match events.get(p + 1..).and_then(|post| post.last()) {
                    Some(last) if ended_on_last_event => end - ms(DEFUSER_LEN_S - last.counter_s),
                    _ => end - ms(DEFUSER_LEN_S),
                };
                events
                    .iter()
                    .enumerate()
                    .map(|(i, e)| if i > p { plant_wall + ms(DEFUSER_LEN_S - e.counter_s) } else { plant_wall - ms(e.counter_s - c_plant) })
                    .collect()
            }
        },
    }
}

/// The part of a round the clips need, extracted from the parser output.
#[derive(Clone, Debug)]
pub struct RoundPlan {
    pub match_id: String,
    /// 1-based, like `FullRound::number`.
    pub round: u32,
    pub start_time_ms: i64,
    pub me_username: String,
    pub events: Vec<Ev>,
    pub ended_on_last_event: bool,
}

/// Reads the recording player's view of one round. `None` when the recording player is not in the
/// player list.
pub fn plan_round(r: &RawRound) -> Option<RoundPlan> {
    let me = r.players.iter().find(|p| !r.recording_profile_id.is_empty() && p.profile_id == r.recording_profile_id)?;
    let mut events = Vec::new();
    for f in &r.match_feedback {
        let kind = match f.kind.name.as_str() {
            "Kill" => EvKind::Kill,
            "Death" => EvKind::Death,
            "DefuserPlantComplete" => EvKind::PlantComplete,
            "DefuserDisableComplete" => EvKind::DisableComplete,
            _ => continue,
        };
        let (killer, victim) = match kind {
            EvKind::Kill => (f.username.clone(), f.target.clone()),
            EvKind::Death => (String::new(), f.username.clone()),
            _ => (String::new(), String::new()),
        };
        events.push(Ev { kind, counter_s: f.time_in_seconds, killer, victim });
    }
    let ended_on_last_event = ended_on_last_event(r, &events);
    Some(RoundPlan {
        match_id: r.match_id.clone(),
        round: r.round_number + 1,
        start_time_ms: r.start_time,
        me_username: me.username.clone(),
        events,
        ended_on_last_event,
    })
}

/// Did the round end exactly at its last event? True when the last event disabled the defuser,
/// when a team was wiped out before the plant, or when the defenders were wiped out after it.
/// (Attackers dying after the plant do not end the round.)
fn ended_on_last_event(r: &RawRound, events: &[Ev]) -> bool {
    let Some(last) = events.last() else { return false };
    let planted = events.iter().any(|e| e.kind == EvKind::PlantComplete);
    if last.kind == EvKind::DisableComplete {
        return true;
    }
    if !matches!(last.kind, EvKind::Kill | EvKind::Death) {
        return false;
    }
    let team_of = |name: &str| r.players.iter().find(|p| p.username == name).map(|p| p.team_index);
    for team in 0..2usize {
        let size = r.players.iter().filter(|p| p.team_index == team).count();
        if size == 0 {
            continue;
        }
        let mut dead: Vec<&str> = events
            .iter()
            .filter(|e| matches!(e.kind, EvKind::Kill | EvKind::Death))
            .map(|e| e.victim.as_str())
            .filter(|v| team_of(v) == Some(team))
            .collect();
        dead.sort_unstable();
        dead.dedup();
        if dead.len() < size {
            continue;
        }
        let role = r.teams.get(team).map(|t| t.role.as_str()).unwrap_or("");
        // Attackers wiped out after the plant: the defuser clock decides, not this kill.
        if planted && role == "Attack" {
            continue;
        }
        return true;
    }
    false
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TargetKind {
    Kill,
    Death,
}

impl TargetKind {
    pub fn as_str(self) -> &'static str {
        match self {
            TargetKind::Kill => "kill",
            TargetKind::Death => "death",
        }
    }
}

/// One moment worth a clip.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Target {
    pub kind: TargetKind,
    /// Round clock value of the event (the same number the kill feed shows).
    pub time_s: i64,
    pub killer: String,
    pub victim: String,
    /// Unix ms.
    pub wall_ms: i64,
}

/// The recording player's kills and deaths of the round, on the wall clock.
pub fn targets(plan: &RoundPlan, anchor: Anchor) -> Vec<Target> {
    let walls = wall_times(&plan.events, anchor, plan.ended_on_last_event);
    plan.events
        .iter()
        .zip(walls)
        .filter_map(|(e, wall_ms)| {
            let kind = match e.kind {
                EvKind::Kill if e.killer == plan.me_username && e.victim != plan.me_username => TargetKind::Kill,
                EvKind::Kill | EvKind::Death if e.victim == plan.me_username => TargetKind::Death,
                _ => return None,
            };
            Some(Target { kind, time_s: e.counter_s, killer: e.killer.clone(), victim: e.victim.clone(), wall_ms })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kill(c: i64, killer: &str, victim: &str) -> Ev {
        Ev { kind: EvKind::Kill, counter_s: c, killer: killer.into(), victim: victim.into() }
    }
    fn ev(kind: EvKind, c: i64) -> Ev {
        Ev { kind, counter_s: c, killer: String::new(), victim: String::new() }
    }

    #[test]
    fn anchor_is_the_file_time_for_a_fresh_plausible_file() {
        let now = 10_000_000;
        let start = now - 200_000;
        assert_eq!(choose_anchor(Some(now - 2_000), start, now), Some(Anchor::RoundEnd(now - 2_000)));
        // Imported old round: the file time is far from the live clock -> header start.
        assert_eq!(choose_anchor(Some(now - 3_600_000), now - 3_800_000, now), Some(Anchor::RoundStart(now - 3_800_000)));
        // Copied file: modified long after the round started -> header start.
        assert_eq!(choose_anchor(Some(now - 1_000), now - 5_000_000, now), Some(Anchor::RoundStart(now - 5_000_000)));
        // No header start but a fresh file: the file time alone.
        assert_eq!(choose_anchor(Some(now - 1_000), 0, now), Some(Anchor::RoundEnd(now - 1_000)));
        assert_eq!(choose_anchor(None, 0, now), None);
    }

    #[test]
    fn elimination_round_ends_at_the_last_event() {
        let events = vec![kill(150, "a", "b"), kill(100, "c", "d"), kill(40, "a", "e")];
        let w = wall_times(&events, Anchor::RoundEnd(1_000_000), true);
        assert_eq!(w, vec![1_000_000 - 110_000, 1_000_000 - 60_000, 1_000_000]);
    }

    #[test]
    fn timed_out_round_counts_down_to_zero() {
        let events = vec![kill(150, "a", "b"), kill(40, "a", "e")];
        let w = wall_times(&events, Anchor::RoundEnd(1_000_000), false);
        assert_eq!(w, vec![1_000_000 - 150_000, 1_000_000 - 40_000]);
    }

    #[test]
    fn plant_switches_to_the_defuser_clock() {
        // Plant at 29 s left; post-plant kills at 44 s and 30 s of the 45 s defuser clock.
        let events = vec![kill(60, "a", "b"), ev(EvKind::PlantComplete, 29), kill(44, "c", "d"), kill(30, "a", "e")];
        // Ended on the last kill (defenders wiped out): it is the end.
        let w = wall_times(&events, Anchor::RoundEnd(2_000_000), true);
        assert_eq!(w[3], 2_000_000);
        assert_eq!(w[2], 2_000_000 - 14_000);
        let plant_wall = 2_000_000 - 15_000;
        assert_eq!(w[1], plant_wall);
        assert_eq!(w[0], plant_wall - 31_000);
        // Ended by the bomb going off: the plant was 45 s before the end.
        let w = wall_times(&events, Anchor::RoundEnd(2_000_000), false);
        assert_eq!(w[1], 2_000_000 - 45_000);
        assert_eq!(w[0], 2_000_000 - 45_000 - 31_000);
    }

    #[test]
    fn header_start_anchor_adds_the_prep_offset() {
        let events = vec![kill(150, "a", "b"), ev(EvKind::PlantComplete, 100), kill(40, "c", "d")];
        let w = wall_times(&events, Anchor::RoundStart(5_000_000), false);
        assert_eq!(w[0], 5_000_000 + 50_000 + 30_000);
        assert_eq!(w[1], 5_000_000 + 50_000 + 80_000);
        assert_eq!(w[2], w[1] + 5_000);
    }

    #[test]
    fn targets_pick_the_recording_players_kills_and_deaths() {
        let plan = RoundPlan {
            match_id: "m".into(),
            round: 1,
            start_time_ms: 0,
            me_username: "me".into(),
            events: vec![kill(150, "me", "x"), kill(120, "y", "z"), kill(90, "w", "me"), ev(EvKind::Death, 60)],
            ended_on_last_event: false,
        };
        let t = targets(&plan, Anchor::RoundEnd(1_000_000));
        assert_eq!(t.len(), 2);
        assert_eq!((t[0].kind, t[0].time_s, t[0].victim.as_str()), (TargetKind::Kill, 150, "x"));
        assert_eq!((t[1].kind, t[1].time_s, t[1].killer.as_str()), (TargetKind::Death, 90, "w"));
    }

    fn raw_round(json: &str) -> RawRound {
        serde_json::from_str(json).unwrap()
    }

    #[test]
    fn plan_round_reads_the_parser_output() {
        let r = raw_round(
            r#"{"recordingProfileID":"p1","matchID":"M1","roundNumber":2,"startTime":1700000000000,
            "teams":[{"role":"Attack"},{"role":"Defense"}],
            "players":[{"profileID":"p1","username":"me","teamIndex":0},{"profileID":"p2","username":"foe","teamIndex":1}],
            "matchFeedback":[
              {"type":{"name":"OperatorSwap"},"username":"me","timeInSeconds":8},
              {"type":{"name":"Kill"},"username":"me","target":"foe","timeInSeconds":100}]}"#,
        );
        let plan = plan_round(&r).unwrap();
        assert_eq!((plan.match_id.as_str(), plan.round, plan.start_time_ms), ("M1", 3, 1_700_000_000_000));
        assert_eq!(plan.events.len(), 1);
        // The only defender died -> a team was wiped out right at the last event.
        assert!(plan.ended_on_last_event);
    }

    #[test]
    fn attackers_dying_after_the_plant_do_not_end_the_round() {
        let r = raw_round(
            r#"{"recordingProfileID":"p1","matchID":"M1","roundNumber":0,
            "teams":[{"role":"Attack"},{"role":"Defense"}],
            "players":[{"profileID":"p1","username":"me","teamIndex":0},{"profileID":"p2","username":"foe","teamIndex":1}],
            "matchFeedback":[
              {"type":{"name":"DefuserPlantComplete"},"username":"me","timeInSeconds":30},
              {"type":{"name":"Kill"},"username":"foe","target":"me","timeInSeconds":20}]}"#,
        );
        assert!(!plan_round(&r).unwrap().ended_on_last_event);
    }

    #[test]
    fn unknown_recording_player_gives_no_plan() {
        let r = raw_round(r#"{"recordingProfileID":"nobody","players":[{"profileID":"p1","username":"me"}]}"#);
        assert!(plan_round(&r).is_none());
    }
}
