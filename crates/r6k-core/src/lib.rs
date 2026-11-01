//! Pure match logic for R6Koc: turns the patched r6-dissect JSON into a
//! per-player match model. The only IO here is reading JSON files.

pub mod advice;
pub mod baseline;
pub mod convert;
pub mod derive;
pub mod full;
pub mod model;
pub mod names;
pub mod query;
pub mod raw;
pub mod session;
pub mod stats;

pub use advice::{advise, Advice, AdviceKind, TOPIC_HEADSHOT};
pub use baseline::{Baseline, CounterSet, MapStats};
pub use convert::{
    dissect_local_to_utc, match_from_json, match_from_raw, read_match_file, utc_to_local_display,
    ConvertError,
};
pub use derive::{player_match_stats, round_clutches, Clutch, MultiKills, PlayerMatchStats};
pub use full::{
    full_match_from_json, full_match_from_raw, read_full_match_file, FullMatch, FullRound, KillEvent,
    PlayerInfo, RoundPlayer, WinCondition,
};
pub use model::{Match, Round, Side};
pub use session::{advise_session, last_session, session_ending_at, summarize, SessionSummary};
