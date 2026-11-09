//! Wall-clock stamps of the recorded frames.
//!
//! The encoder writes its own timeline into a segment, and that timeline can differ from real time
//! (frames that were dropped or held back while the game was minimized, covered or busy). So the
//! recorder also writes, next to every segment, a small `.clock` file with one pair per frame:
//! the frame's capture time on the encoder's timeline (100 ns units from the first frame) and the
//! wall-clock time (Unix ms) at which the frame arrived. The cutter looks clips up on the wall
//! clock through these pairs instead of trusting `segment start + sample time`.

use std::path::Path;

/// Capture timestamps are in 100 ns units (the Windows `TimeSpan` tick).
pub const TICKS_PER_MS: i64 = 10_000;

/// Frame stamps of one segment, in frame order.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct FrameClock {
    /// `(time on the encoder's timeline in 100 ns, wall time in Unix ms)`.
    pairs: Vec<(i64, i64)>,
}

impl FrameClock {
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds a frame. Both values never go backwards (a late or reordered stamp is clamped).
    pub fn push(&mut self, ticks: i64, wall_ms: i64) {
        let (t, w) = match self.pairs.last() {
            Some(&(lt, lw)) => (ticks.max(lt), wall_ms.max(lw)),
            None => (ticks.max(0), wall_ms),
        };
        self.pairs.push((t, w));
    }

    pub fn len(&self) -> usize {
        self.pairs.len()
    }

    /// Builds the clock of a segment from raw observations `(time on the encoder's timeline in
    /// 100 ns, wall time at which the frame was handed to us)`.
    ///
    /// A frame is handed to us some time after it was captured, and that delay jumps around
    /// (a busy encoder, a slow start-up). The capture times themselves are exact and run at real
    /// speed, so the wall time of a frame is its capture time plus one constant: the smallest
    /// delay seen in the segment. Real holes (no frames for a while) stay holes.
    pub fn from_arrivals(arrivals: &[(i64, i64)]) -> Self {
        let offset = arrivals.iter().map(|&(t, w)| w - t / TICKS_PER_MS).min();
        let mut c = Self::new();
        if let Some(off) = offset {
            for &(t, _) in arrivals {
                c.push(t, t / TICKS_PER_MS + off);
            }
        }
        c
    }

    /// Wall time of the `i`-th recorded frame.
    pub fn wall_of(&self, i: usize) -> Option<i64> {
        self.pairs.get(i).map(|p| p.1)
    }

    pub fn first_wall(&self) -> Option<i64> {
        self.pairs.first().map(|p| p.1)
    }

    pub fn last_wall(&self) -> Option<i64> {
        self.pairs.last().map(|p| p.1)
    }

    /// Wall time (Unix ms) of the moment `ticks` on the encoder's timeline. Between two stamped
    /// frames it interpolates; outside the stamped range it extends the nearest frame at real speed.
    pub fn wall_at(&self, ticks: i64) -> Option<i64> {
        let first = self.pairs.first()?;
        let last = self.pairs.last()?;
        if ticks <= first.0 {
            return Some(first.1 + (ticks - first.0) / TICKS_PER_MS);
        }
        if ticks >= last.0 {
            return Some(last.1 + (ticks - last.0) / TICKS_PER_MS);
        }
        // First pair strictly after `ticks`.
        let hi = self.pairs.partition_point(|p| p.0 <= ticks);
        let (a, b) = (self.pairs[hi - 1], self.pairs[hi]);
        if b.0 == a.0 {
            return Some(a.1);
        }
        let frac = (ticks - a.0) as f64 / (b.0 - a.0) as f64;
        Some(a.1 + ((b.1 - a.1) as f64 * frac).round() as i64)
    }

    /// Binary form: little-endian `i64` pairs, no header (the file name says what it is).
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(self.pairs.len() * 16);
        for &(t, w) in &self.pairs {
            out.extend_from_slice(&t.to_le_bytes());
            out.extend_from_slice(&w.to_le_bytes());
        }
        out
    }

    pub fn from_bytes(bytes: &[u8]) -> Self {
        let pairs = bytes
            .chunks_exact(16)
            .map(|c| {
                let (t, w) = c.split_at(8);
                (i64::from_le_bytes(t.try_into().unwrap_or([0; 8])), i64::from_le_bytes(w.try_into().unwrap_or([0; 8])))
            })
            .collect();
        Self { pairs }
    }

    pub fn write(&self, path: &Path) -> std::io::Result<()> {
        std::fs::write(path, self.to_bytes())
    }

    /// `None` when the file is missing or empty (segments from before the clock existed).
    pub fn read(path: &Path) -> Option<Self> {
        let c = Self::from_bytes(&std::fs::read(path).ok()?);
        (c.len() > 0).then_some(c)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn clock(pairs: &[(i64, i64)]) -> FrameClock {
        let mut c = FrameClock::new();
        for &(t, w) in pairs {
            c.push(t * TICKS_PER_MS, w);
        }
        c
    }

    #[test]
    fn maps_encoder_time_to_wall_time_and_interpolates() {
        // 3 frames 100 ms apart on the encoder timeline, but the wall clock jumped 5 s in between.
        let c = clock(&[(0, 1_000), (100, 1_100), (200, 6_200)]);
        assert_eq!(c.wall_at(0), Some(1_000));
        assert_eq!(c.wall_at(50 * TICKS_PER_MS), Some(1_050));
        assert_eq!(c.wall_at(150 * TICKS_PER_MS), Some(3_650));
        assert_eq!(c.wall_at(200 * TICKS_PER_MS), Some(6_200));
        // Before the first and after the last frame the nearest frame is extended at real speed.
        assert_eq!(c.wall_at(-30 * TICKS_PER_MS), Some(970));
        assert_eq!(c.wall_at(260 * TICKS_PER_MS), Some(6_260));
        assert_eq!(FrameClock::new().wall_at(0), None);
    }

    #[test]
    fn arrival_delay_jitter_is_removed_but_holes_stay() {
        // Frames captured every 16 ms; they reached us with a varying delay (the first ones very
        // late: the encoder was starting), then nothing for 4 s (minimized), then more frames.
        let step = 16;
        let mut raw = Vec::new();
        for i in 0..5 {
            raw.push((i * step * TICKS_PER_MS, 10_000 + i * step + 300 - i * 70)); // 300 ms late, catching up
        }
        for i in 5..10 {
            raw.push((i * step * TICKS_PER_MS, 10_000 + i * step + 4)); // 4 ms late: the real delay
        }
        for i in 10..13 {
            let t = (i * step + 4_000) * TICKS_PER_MS; // 4 s hole
            raw.push((t, 10_000 + i * step + 4_000 + 6));
        }
        let c = FrameClock::from_arrivals(&raw);
        assert_eq!(c.len(), 13);
        assert_eq!(c.wall_of(0), Some(10_004));
        assert_eq!(c.wall_of(4), Some(10_004 + 4 * step));
        assert_eq!(c.wall_of(9), Some(10_004 + 9 * step));
        // The hole is kept (4 s between frame 9 and frame 10 plus the normal step).
        assert_eq!(c.wall_of(10).unwrap() - c.wall_of(9).unwrap(), step + 4_000);
        assert_eq!(FrameClock::from_arrivals(&[]).len(), 0);
    }

    #[test]
    fn stamps_never_go_backwards() {
        let mut c = FrameClock::new();
        c.push(100, 5_000);
        c.push(50, 4_990);
        assert_eq!(c.pairs, vec![(100, 5_000), (100, 5_000)]);
        assert_eq!(c.last_wall(), Some(5_000));
    }

    #[test]
    fn round_trips_through_a_file() {
        let t = tempfile::tempdir().unwrap();
        let p = t.path().join("seg.clock");
        let c = clock(&[(0, 10), (16, 26), (33, 43)]);
        c.write(&p).unwrap();
        assert_eq!(FrameClock::read(&p), Some(c));
        assert_eq!(FrameClock::read(&t.path().join("missing.clock")), None);
        std::fs::write(&p, b"").unwrap();
        assert_eq!(FrameClock::read(&p), None);
    }
}
