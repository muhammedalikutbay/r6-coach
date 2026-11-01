//! Match model for the tracked player only.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Side {
    Attack,
    Defense,
    Unknown,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Round {
    /// 1-based round number within the match (dissect counts from 0).
    pub number: u32,
    pub side: Side,
    pub operator_id: u64,
    pub kills: u32,
    pub deaths: u32,
    pub assists: u32,
    pub headshots: u32,
    pub first_blood: bool,
    pub first_death: bool,
    pub planted: bool,
    pub defused: bool,
    pub won: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Match {
    pub match_id: String,
    /// Timestamp of the first round as written by dissect. It ends in "Z" but is the
    /// recording PC's local time (it matches the folder name), not real UTC.
    pub timestamp: String,
    pub match_type: String,
    pub gamemode: String,
    pub map_id: u64,
    pub won: bool,
    pub rounds: Vec<Round>,
}

impl Match {
    pub fn kills(&self) -> u32 {
        self.rounds.iter().map(|r| r.kills).sum()
    }
    pub fn deaths(&self) -> u32 {
        self.rounds.iter().map(|r| r.deaths).sum()
    }
    pub fn assists(&self) -> u32 {
        self.rounds.iter().map(|r| r.assists).sum()
    }
    pub fn headshots(&self) -> u32 {
        self.rounds.iter().map(|r| r.headshots).sum()
    }
    /// Headshot percentage of kills (0.0 when there are no kills).
    pub fn headshot_pct(&self) -> f64 {
        let k = self.kills();
        if k == 0 {
            0.0
        } else {
            100.0 * self.headshots() as f64 / k as f64
        }
    }
}
