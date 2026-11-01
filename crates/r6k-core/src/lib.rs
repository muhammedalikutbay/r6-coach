//! Pure match logic for R6Koc: turns the patched r6-dissect JSON into a
//! per-player match model. The only IO here is reading JSON files.

pub mod convert;
pub mod model;
pub mod names;
pub mod raw;

pub use convert::{match_from_json, match_from_raw, read_match_file, ConvertError};
pub use model::{Match, Round, Side};
