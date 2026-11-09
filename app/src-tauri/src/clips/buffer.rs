//! The rolling recording buffer: finished segment files in one temp folder.
//!
//! The recorder writes `seg-<start>.part` while a segment is open and renames it to
//! `seg-<start>-<end>.mp4` (wall-clock Unix ms) when the segment is finished. Only finished
//! segments are ever read or cut. `start` is the arrival time of the first frame and `end` that of
//! the last one, so the file name tells which wall-clock span a file covers; the `.clock` file next
//! to it (see `clock`) stamps every single frame.

use std::path::{Path, PathBuf};

/// Length of one segment.
pub const SEGMENT_SECS: u64 = 60;
/// How long finished segments are kept. A round lasts 5-6 minutes at most, so ten minutes is
/// enough to cut every round after its file arrives.
pub const KEEP_MS: i64 = 10 * 60_000;
/// Unfinished `.part` files older than this are leftovers of a crash.
pub const STALE_PART_MS: i64 = 15 * 60_000;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Segment {
    pub start_ms: i64,
    pub end_ms: i64,
    pub path: PathBuf,
}

impl Segment {
    /// The per-frame wall-clock stamps of this segment (may not exist for old segments).
    pub fn clock_path(&self) -> PathBuf {
        self.path.with_extension("clock")
    }
}

pub fn part_name(start_ms: i64) -> String {
    format!("seg-{start_ms}.part")
}

pub fn segment_name(start_ms: i64, end_ms: i64) -> String {
    format!("seg-{start_ms}-{end_ms}.mp4")
}

pub fn clock_name(start_ms: i64, end_ms: i64) -> String {
    format!("seg-{start_ms}-{end_ms}.clock")
}

/// `(start, end)` from a finished segment's file name.
pub fn parse_segment_name(name: &str) -> Option<(i64, i64)> {
    parse_span(name.strip_prefix("seg-")?.strip_suffix(".mp4")?)
}

fn parse_span(body: &str) -> Option<(i64, i64)> {
    let (a, b) = body.split_once('-')?;
    let (start, end) = (a.parse::<i64>().ok()?, b.parse::<i64>().ok()?);
    (end >= start).then_some((start, end))
}

/// Finished segments in `dir`, oldest first. Unknown files are ignored.
pub fn list_segments(dir: &Path) -> Vec<Segment> {
    let Ok(rd) = std::fs::read_dir(dir) else { return Vec::new() };
    let mut out: Vec<Segment> = rd
        .flatten()
        .filter_map(|e| {
            let name = e.file_name().to_string_lossy().into_owned();
            let (start_ms, end_ms) = parse_segment_name(&name)?;
            Some(Segment { start_ms, end_ms, path: e.path() })
        })
        .collect();
    out.sort_by_key(|s| s.start_ms);
    out
}

/// Segments that overlap the wall-clock window `[start, end]`, oldest first.
pub fn select_for_window(segments: &[Segment], start_ms: i64, end_ms: i64) -> Vec<Segment> {
    segments.iter().filter(|s| s.end_ms >= start_ms && s.start_ms <= end_ms).cloned().collect()
}

/// How much of the window `[start, end]` the segments cover, in ms (gaps between segments count
/// as not covered).
pub fn coverage_ms(segments: &[Segment], start_ms: i64, end_ms: i64) -> i64 {
    let mut covered = 0;
    let mut cursor = start_ms;
    for s in segments {
        let (a, b) = (s.start_ms.max(cursor), s.end_ms.min(end_ms));
        if b > a {
            covered += b - a;
            cursor = b;
        }
    }
    covered
}

/// Deletes finished segments older than `keep_ms` and leftover `.part` files. `active_start_ms` is
/// the start of the segment being written right now (never deleted). Returns the deleted count.
pub fn prune(dir: &Path, now_ms: i64, keep_ms: i64, active_start_ms: Option<i64>) -> usize {
    let Ok(rd) = std::fs::read_dir(dir) else { return 0 };
    let mut deleted = 0;
    for e in rd.flatten() {
        let name = e.file_name().to_string_lossy().into_owned();
        let clock_span = name.strip_prefix("seg-").and_then(|n| n.strip_suffix(".clock")).and_then(parse_span);
        let stale = if let Some((_, end)) = parse_segment_name(&name).or(clock_span) {
            now_ms - end > keep_ms
        } else if let Some(start) = name.strip_prefix("seg-").and_then(|n| n.strip_suffix(".part")).and_then(|n| n.parse::<i64>().ok()) {
            Some(start) != active_start_ms && now_ms - start > STALE_PART_MS
        } else {
            false
        };
        if stale && std::fs::remove_file(e.path()).is_ok() {
            deleted += 1;
        }
    }
    deleted
}

/// Total size of the files in `dir` (non-recursive).
pub fn dir_size(dir: &Path) -> u64 {
    std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|e| e.metadata().ok().filter(|m| m.is_file()).map(|m| m.len()))
        .sum()
}

/// Deletes every file in `dir` (used when recording is turned off). Returns the deleted count.
pub fn clear(dir: &Path) -> usize {
    let Ok(rd) = std::fs::read_dir(dir) else { return 0 };
    rd.flatten().filter(|e| e.path().is_file() && std::fs::remove_file(e.path()).is_ok()).count()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn seg(a: i64, b: i64) -> Segment {
        Segment { start_ms: a, end_ms: b, path: PathBuf::from(segment_name(a, b)) }
    }

    #[test]
    fn names_round_trip() {
        assert_eq!(parse_segment_name(&segment_name(1_000, 61_000)), Some((1_000, 61_000)));
        assert_eq!(parse_segment_name("seg-5.part"), None);
        assert_eq!(parse_segment_name("seg-9-3.mp4"), None);
        assert_eq!(parse_segment_name("other.mp4"), None);
    }

    #[test]
    fn selects_overlapping_segments_only() {
        let all = vec![seg(0, 60_000), seg(60_500, 120_000), seg(121_000, 180_000)];
        let got = select_for_window(&all, 55_000, 63_000);
        assert_eq!(got.iter().map(|s| s.start_ms).collect::<Vec<_>>(), vec![0, 60_500]);
        assert!(select_for_window(&all, 200_000, 210_000).is_empty());
    }

    #[test]
    fn coverage_skips_gaps() {
        let all = vec![seg(0, 60_000), seg(61_000, 120_000)];
        assert_eq!(coverage_ms(&all, 50_000, 70_000), 10_000 + 9_000);
        assert_eq!(coverage_ms(&all, 130_000, 140_000), 0);
        assert_eq!(coverage_ms(&all, -10_000, 10_000), 10_000);
    }

    #[test]
    fn prune_removes_old_segments_and_stale_parts() {
        let t = tempfile::tempdir().unwrap();
        let now = 100_000_000;
        let old = segment_name(now - 20 * 60_000, now - 19 * 60_000);
        let fresh = segment_name(now - 5 * 60_000, now - 4 * 60_000);
        let old_clock = clock_name(now - 20 * 60_000, now - 19 * 60_000);
        let fresh_clock = clock_name(now - 5 * 60_000, now - 4 * 60_000);
        let stale_part = part_name(now - 30 * 60_000);
        let active_part = part_name(now - 20 * 60_000);
        for n in [&old, &fresh, &old_clock, &fresh_clock, &stale_part, &active_part, &"notes.txt".to_string()] {
            std::fs::write(t.path().join(n), b"x").unwrap();
        }
        assert_eq!(prune(t.path(), now, KEEP_MS, Some(now - 20 * 60_000)), 3);
        let left: Vec<String> = std::fs::read_dir(t.path()).unwrap().flatten().map(|e| e.file_name().to_string_lossy().into_owned()).collect();
        assert!(left.contains(&fresh) && left.contains(&fresh_clock) && left.contains(&active_part) && left.contains(&"notes.txt".to_string()));
        assert!(!left.contains(&old) && !left.contains(&old_clock) && !left.contains(&stale_part));
        assert_eq!(list_segments(t.path()).len(), 1);
    }
}
