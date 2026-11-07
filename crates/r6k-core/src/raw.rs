//! Serde mirror of the patched r6-dissect match-folder output
//! (`{"rounds":[...], "stats":[...]}`). Only fields we use are listed;
//! everything is lenient so new dissect versions do not break parsing.

use serde::Deserialize;

#[derive(Debug, Deserialize)]
pub struct RawMatch {
    #[serde(default)]
    pub rounds: Vec<RawRound>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RawRound {
    /// Replay header game version, e.g. "Y11S3_Alpha04".
    #[serde(default)]
    pub game_version: String,
    #[serde(default)]
    pub timestamp: String,
    #[serde(default)]
    pub match_type: Named,
    #[serde(default)]
    pub map: Named,
    #[serde(default)]
    pub gamemode: Named,
    /// Bomb site / spawn description, e.g. "2F Master Bedroom, 2F Office".
    #[serde(default)]
    pub site: String,
    #[serde(default, rename = "recordingProfileID")]
    pub recording_profile_id: String,
    #[serde(default, rename = "matchID")]
    pub match_id: String,
    #[serde(default)]
    pub round_number: u32,
    #[serde(default)]
    pub teams: Vec<RawTeam>,
    #[serde(default)]
    pub players: Vec<RawPlayer>,
    #[serde(default)]
    pub match_feedback: Vec<RawFeedback>,
    #[serde(default)]
    pub stats: Vec<RawPlayerStats>,
}

#[derive(Debug, Default, Deserialize)]
pub struct Named {
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub id: u64,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RawTeam {
    #[serde(default)]
    pub score: u32,
    #[serde(default)]
    pub won: bool,
    /// "Attack" or "Defense".
    #[serde(default)]
    pub role: String,
    /// Only set by dissect for some endings ("DefusedBomb", "DisabledDefuser").
    #[serde(default)]
    pub win_condition: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RawPlayer {
    #[serde(default, rename = "profileID")]
    pub profile_id: String,
    #[serde(default)]
    pub username: String,
    #[serde(default)]
    pub team_index: usize,
    #[serde(default)]
    pub operator: Named,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RawFeedback {
    #[serde(default, rename = "type")]
    pub kind: Named,
    #[serde(default)]
    pub username: String,
    #[serde(default)]
    pub target: String,
    #[serde(default)]
    pub headshot: bool,
    #[serde(default)]
    pub time_in_seconds: i64,
}

#[derive(Debug, Deserialize)]
pub struct RawPlayerStats {
    #[serde(default)]
    pub username: String,
    #[serde(default)]
    pub kills: u32,
    #[serde(default)]
    pub died: bool,
    #[serde(default)]
    pub assists: u32,
    #[serde(default)]
    pub headshots: u32,
}
