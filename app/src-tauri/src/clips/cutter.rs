//! Cuts a clip out of finished buffer segments without re-encoding (pure Rust, `mp4` crate).
//!
//! Clips are looked up on the wall clock, not on the encoder's timeline: every frame of a segment
//! carries the wall time at which it arrived (the segment's `.clock` file, see `clock`). So a
//! stretch in which no frames were recorded (the game was minimized, the capture was restarted)
//! shows up as a hole in the wall clock and cannot move the clips that come after it. A clip never
//! spans a hole: it keeps the footage around the wanted moment only.
//!
//! The clip starts at the last keyframe at or before the wanted start, so the file carries a
//! little lead-in; `Cut::lead_ms` says where the wanted start is and the player seeks there. If
//! the window crosses a segment boundary the samples of the next segment are appended with their
//! times shifted by the wall-clock distance of the two segments. The audio track (if the segments
//! have one) is cut the same way, on the same timeline. The finished file gets its `moov` box in
//! front of the media data (fast start), which is what browsers like best.

use std::fs::File;
use std::io::{BufReader, BufWriter};
use std::path::Path;

use mp4::{AacConfig, AvcConfig, MediaConfig, Mp4Config, Mp4Reader, Mp4Sample, Mp4Track, Mp4Writer, TrackConfig, TrackType};

use super::buffer::Segment;
use super::clock::FrameClock;

/// A stretch without recorded frames longer than this splits the footage; a clip keeps only the
/// part on one side. Normal segment seams are far shorter.
pub const GAP_MS: i64 = 2_500;
/// How far from the wanted moment the nearest footage may be before the clip is skipped.
pub const MAX_MISS_MS: i64 = 3_000;

/// What was written.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Cut {
    /// Where the wanted start lies inside the file (the file starts at a keyframe before it).
    pub lead_ms: i64,
    /// Length of the whole file.
    pub duration_ms: i64,
    /// The clip has a sound track.
    pub audio: bool,
}

struct SampleMeta {
    dts: u64,
    is_sync: bool,
}

/// Decode time and keyframe flag of every sample, from the sample tables (no sample data read).
fn sample_table(track: &Mp4Track) -> Vec<SampleMeta> {
    let stbl = &track.trak.mdia.minf.stbl;
    let mut out = Vec::new();
    let mut t = 0u64;
    for e in &stbl.stts.entries {
        for _ in 0..e.sample_count {
            out.push(SampleMeta { dts: t, is_sync: stbl.stss.is_none() });
            t += u64::from(e.sample_delta);
        }
    }
    if let Some(stss) = &stbl.stss {
        for &id in &stss.entries {
            if let Some(m) = (id as usize).checked_sub(1).and_then(|i| out.get_mut(i)) {
                m.is_sync = true;
            }
        }
    }
    out
}

struct AudioTrack {
    track_id: u32,
    timescale: u64,
    config: AacConfig,
    metas: Vec<SampleMeta>,
}

struct Open {
    reader: Mp4Reader<BufReader<File>>,
    track_id: u32,
    timescale: u64,
    width: u16,
    height: u16,
    sps: Vec<u8>,
    pps: Vec<u8>,
    metas: Vec<SampleMeta>,
    /// Wall time (Unix ms) of every video sample.
    walls: Vec<i64>,
    audio: Option<AudioTrack>,
}

impl Open {
    fn same_format(&self, other: &Open) -> bool {
        (self.timescale, self.width, self.height) == (other.timescale, other.width, other.height) && self.sps == other.sps && self.pps == other.pps
    }

    fn same_audio(&self, other: &Open) -> bool {
        match (&self.audio, &other.audio) {
            (Some(a), Some(b)) => a.timescale == b.timescale && a.config == b.config,
            _ => false,
        }
    }
}

fn open_segment(seg: &Segment) -> Result<Open, String> {
    let path = &seg.path;
    let file = File::open(path).map_err(|e| format!("cannot open {}: {e}", path.display()))?;
    let size = file.metadata().map_err(|e| e.to_string())?.len();
    let reader = Mp4Reader::read_header(BufReader::new(file), size).map_err(|e| format!("cannot read {}: {e}", path.display()))?;
    let (track_id, timescale, width, height, sps, pps, metas) = {
        let track = reader
            .tracks()
            .values()
            .find(|t| t.track_type().ok() == Some(TrackType::Video))
            .ok_or_else(|| format!("{} has no video track", path.display()))?;
        (
            track.track_id(),
            u64::from(track.timescale()),
            track.width(),
            track.height(),
            track.sequence_parameter_set().map_err(|e| format!("no SPS: {e}"))?.to_vec(),
            track.picture_parameter_set().map_err(|e| format!("no PPS: {e}"))?.to_vec(),
            sample_table(track),
        )
    };
    if timescale == 0 || metas.is_empty() {
        return Err(format!("{} has no video samples", path.display()));
    }
    // Sound is optional: segments recorded without it carry an empty audio track (or none).
    let audio = reader
        .tracks()
        .values()
        .find(|t| t.track_type().ok() == Some(TrackType::Audio) && t.sample_count() > 0)
        .and_then(|t| {
            let config = AacConfig {
                bitrate: t.bitrate(),
                profile: t.audio_profile().ok()?,
                freq_index: t.sample_freq_index().ok()?,
                chan_conf: t.channel_config().ok()?,
            };
            let timescale = u64::from(t.timescale());
            (timescale > 0).then(|| AudioTrack { track_id: t.track_id(), timescale, config, metas: sample_table(t) })
        });
    // Wall time of each frame: through the clock file when there is one, else assume that the
    // encoder's timeline runs at real speed from the segment start.
    // If the encoder kept every frame, the i-th sample is the i-th stamped frame; otherwise look
    // the sample's time up on the stamped timeline.
    let clock = FrameClock::read(&seg.clock_path());
    let walls = metas
        .iter()
        .enumerate()
        .map(|(i, m)| match &clock {
            Some(c) if c.len() == metas.len() => c.wall_of(i).unwrap_or(seg.start_ms),
            Some(c) => c.wall_at((m.dts as i128 * 10_000_000 / timescale as i128) as i64).unwrap_or(seg.start_ms),
            None => seg.start_ms + (m.dts * 1000 / timescale) as i64,
        })
        .collect();
    Ok(Open { reader, track_id, timescale, width, height, sps, pps, metas, walls, audio })
}

/// Frame count and video length (ms) of a segment, from its sample tables.
#[allow(dead_code)]
pub fn probe(path: &Path) -> Result<(u32, i64), String> {
    let seg = Segment { start_ms: 0, end_ms: 0, path: path.to_path_buf() };
    let open = open_segment(&seg)?;
    let last = open.metas.last().map_or(0, |m| m.dts);
    Ok((open.metas.len() as u32, (last * 1000 / open.timescale) as i64))
}

/// One video frame of the footage, in wall-clock order.
#[derive(Clone, Copy, Debug)]
struct FrameRef {
    seg: usize,
    idx: usize,
    wall: i64,
    sync: bool,
}

/// A stretch of footage without a hole: `frames[range]`.
#[derive(Clone, Copy, Debug)]
struct Run {
    from: usize,
    to: usize,
}

/// Splits the frames into stretches at holes in the wall clock and at format changes.
fn split_runs(frames: &[FrameRef], format_break: impl Fn(usize) -> bool) -> Vec<Run> {
    let mut runs: Vec<Run> = Vec::new();
    for (i, f) in frames.iter().enumerate() {
        let new_run = match i.checked_sub(1).map(|p| &frames[p]) {
            None => true,
            Some(prev) => f.wall - prev.wall > GAP_MS || (f.seg != prev.seg && format_break(f.seg)),
        };
        match (new_run, runs.last_mut()) {
            (false, Some(r)) => r.to = i,
            _ => runs.push(Run { from: i, to: i }),
        }
    }
    runs
}

/// The stretch that holds the wanted moment (or lies nearest to it); the one with more footage
/// inside the window wins a tie. `None` when nothing is within `MAX_MISS_MS` of the moment.
fn pick_run(frames: &[FrameRef], runs: &[Run], start_ms: i64, end_ms: i64, anchor_ms: i64) -> Option<Run> {
    runs.iter()
        .filter_map(|r| {
            let (a, b) = (frames[r.from].wall, frames[r.to].wall);
            let miss = if anchor_ms < a { a - anchor_ms } else if anchor_ms > b { anchor_ms - b } else { 0 };
            let overlap = (b.min(end_ms) - a.max(start_ms)).max(0);
            (miss <= MAX_MISS_MS && overlap > 0).then_some((miss, -overlap, *r))
        })
        .min_by_key(|(miss, neg_overlap, _)| (*miss, *neg_overlap))
        .map(|(_, _, r)| r)
}

/// Cuts the wall-clock window `[start_ms, end_ms]` out of `segments` (oldest first, as returned by
/// `buffer::select_for_window`) into the MP4 file `out`. `anchor_ms` is the moment the clip is about
/// (the centre of the window): if the footage has a hole, the part around it is kept.
pub fn cut(segments: &[Segment], start_ms: i64, end_ms: i64, anchor_ms: i64, out: &Path) -> Result<Cut, String> {
    if segments.is_empty() || end_ms <= start_ms {
        return Err("nothing to cut".into());
    }
    let tmp = out.with_extension("part");
    let result = cut_into(segments, start_ms, end_ms, anchor_ms, &tmp);
    match result {
        Ok(c) => {
            std::fs::rename(&tmp, out).map_err(|e| format!("cannot finish clip: {e}"))?;
            Ok(c)
        }
        Err(e) => {
            let _ = std::fs::remove_file(&tmp);
            Err(e)
        }
    }
}

struct Out {
    ms: i64,
    sample: Mp4Sample,
}

fn cut_into(segments: &[Segment], start_ms: i64, end_ms: i64, anchor_ms: i64, tmp: &Path) -> Result<Cut, String> {
    // 1. Open the segments that can matter and list their frames on the wall clock.
    let mut opens: Vec<Open> = Vec::new();
    for seg in segments {
        // A segment that ended before the window (or starts after it) has nothing to give.
        if seg.end_ms < start_ms || seg.start_ms > end_ms {
            continue;
        }
        opens.push(open_segment(seg)?);
    }
    let mut frames: Vec<FrameRef> = Vec::new();
    for (s, o) in opens.iter().enumerate() {
        for (i, (&wall, m)) in o.walls.iter().zip(&o.metas).enumerate() {
            frames.push(FrameRef { seg: s, idx: i, wall, sync: m.is_sync });
        }
    }
    let runs = split_runs(&frames, |s| s > 0 && !opens[s].same_format(&opens[s - 1]));
    let run = pick_run(&frames, &runs, start_ms, end_ms, anchor_ms).ok_or("the window holds no recorded frames")?;
    let run_frames = &frames[run.from..=run.to];
    if runs.len() > 1 {
        log::info!(
            "clips: the footage has {} hole(s) near the window; keeping {} ms of it ({}..{})",
            runs.len() - 1,
            run_frames.last().map_or(0, |l| l.wall) - run_frames[0].wall,
            run_frames[0].wall,
            run_frames.last().map_or(0, |l| l.wall)
        );
    }

    // 2. First frame: the last keyframe at or before the start; failing that, the first one after.
    let begin = run_frames
        .iter()
        .rposition(|f| f.sync && f.wall <= start_ms)
        .or_else(|| run_frames.iter().position(|f| f.sync && f.wall <= end_ms))
        .ok_or("the window holds no recorded frames")?;
    let last = run_frames.iter().rposition(|f| f.wall <= end_ms).filter(|&l| l >= begin).ok_or("the window holds no recorded frames")?;
    let used = &run_frames[begin..=last];
    let origin_wall = used[0].wall;

    // 3. Video samples on the clip's timeline. `first_of[s]`: the first used frame of segment s.
    let first_seg = used[0].seg;
    let ts = opens[first_seg].timescale;
    let mut first_of: Vec<Option<usize>> = vec![None; opens.len()];
    for f in used {
        first_of[f.seg].get_or_insert(f.idx);
    }
    let mut video: Vec<Out> = Vec::with_capacity(used.len());
    let mut last_dts: Option<u64> = None;
    for f in used {
        let open = &mut opens[f.seg];
        let f0 = first_of[f.seg].unwrap_or(f.idx);
        let dts = open.metas[f.idx].dts;
        let base = if f.seg == first_seg { 0 } else { ((open.walls[f0] - origin_wall).max(0) as u64) * ts / 1000 };
        let mut new_dts = base + dts.saturating_sub(open.metas[f0].dts);
        if let Some(prev) = last_dts {
            new_dts = new_dts.max(prev + 1);
        }
        last_dts = Some(new_dts);
        let sample = open
            .reader
            .read_sample(open.track_id, f.idx as u32 + 1)
            .map_err(|e| format!("cannot read sample: {e}"))?
            .ok_or("sample missing")?;
        video.push(Out {
            ms: (new_dts * 1000 / ts) as i64,
            sample: Mp4Sample { start_time: new_dts, duration: sample.duration, rendering_offset: sample.rendering_offset, is_sync: sample.is_sync, bytes: sample.bytes },
        });
    }
    // A sample lasts until the next one starts (this absorbs a short hole at a segment seam); the
    // last one keeps its own length.
    for i in 0..video.len().saturating_sub(1) {
        let next = video[i + 1].sample.start_time;
        video[i].sample.duration = next.saturating_sub(video[i].sample.start_time).clamp(1, u64::from(u32::MAX)) as u32;
    }
    // Composition offsets (frame reordering of some hardware encoders) may be negative in the
    // source; a file without an edit list cannot carry that. Shift them all up so none is.
    let min_offset = video.iter().map(|v| v.sample.rendering_offset).min().unwrap_or(0);
    if min_offset < 0 {
        for v in &mut video {
            v.sample.rendering_offset = v.sample.rendering_offset.saturating_sub(min_offset);
        }
    }
    let end_ticks = video.last().map_or(0, |v| v.sample.start_time + u64::from(v.sample.duration));

    // 4. Sound: the samples that lie between the first and the last used video frame of each
    //    segment, on the same timeline (audio and video of one segment share the encoder's clock).
    let mut audio: Vec<Out> = Vec::new();
    let mut audio_cfg: Option<(u64, AacConfig)> = None;
    let used_segs: Vec<usize> = (0..opens.len()).filter(|&s| first_of[s].is_some()).collect();
    for &s in &used_segs {
        let (head, rest) = opens.split_at_mut(s);
        let open = &mut rest[0];
        if open.audio.is_none() || !(s == first_seg || head[first_seg].same_audio(open)) {
            continue;
        }
        let Some(track) = open.audio.as_ref() else { continue };
        let (track_id, a_ts, a_cfg) = (track.track_id, track.timescale, track.config.clone());
        let a_dts: Vec<u64> = track.metas.iter().map(|m| m.dts).collect();
        let f0 = first_of[s].unwrap_or(0);
        let f_last = used.iter().rev().find(|f| f.seg == s).map_or(f0, |f| f.idx);
        let v_start_ms = (open.metas[f0].dts * 1000 / open.timescale) as i64;
        let v_end_ms = (open.metas[f_last].dts * 1000 / open.timescale) as i64;
        let base_ms = if s == first_seg { 0 } else { (open.walls[f0] - origin_wall).max(0) };
        audio_cfg.get_or_insert((a_ts, a_cfg));
        for (i, &adts) in a_dts.iter().enumerate() {
            let a_ms = (adts * 1000 / a_ts) as i64;
            if a_ms < v_start_ms {
                continue;
            }
            if a_ms > v_end_ms {
                break;
            }
            let sample = open.reader.read_sample(track_id, i as u32 + 1).map_err(|e| format!("cannot read sound: {e}"))?.ok_or("sound sample missing")?;
            let start_time = (base_ms as u64) * a_ts / 1000 + adts.saturating_sub(v_start_ms as u64 * a_ts / 1000);
            audio.push(Out { ms: base_ms + (a_ms - v_start_ms), sample: Mp4Sample { start_time, duration: sample.duration, rendering_offset: 0, is_sync: true, bytes: sample.bytes } });
        }
    }
    // Keep the sound strictly increasing and let each sample last until the next one.
    for i in 1..audio.len() {
        let floor = audio[i - 1].sample.start_time + 1;
        if audio[i].sample.start_time < floor {
            audio[i].sample.start_time = floor;
        }
    }
    for i in 0..audio.len().saturating_sub(1) {
        let next = audio[i + 1].sample.start_time;
        audio[i].sample.duration = next.saturating_sub(audio[i].sample.start_time).clamp(1, u64::from(u32::MAX)) as u32;
    }

    // 5. Write the file, the two tracks interleaved in time.
    let open0 = &opens[first_seg];
    let file = File::create(tmp).map_err(|e| format!("cannot create clip: {e}"))?;
    let cfg = Mp4Config {
        major_brand: "isom".parse().map_err(|_| "bad brand")?,
        minor_version: 512,
        compatible_brands: ["isom", "iso2", "avc1", "mp41"].iter().filter_map(|b| b.parse().ok()).collect(),
        timescale: 1000,
    };
    let mut w = Mp4Writer::write_start(BufWriter::new(file), &cfg).map_err(|e| format!("cannot write clip: {e}"))?;
    w.add_track(&TrackConfig {
        track_type: TrackType::Video,
        timescale: open0.timescale as u32,
        language: "und".into(),
        media_conf: MediaConfig::AvcConfig(AvcConfig { width: open0.width, height: open0.height, seq_param_set: open0.sps.clone(), pic_param_set: open0.pps.clone() }),
    })
    .map_err(|e| format!("cannot write clip: {e}"))?;
    let video_count = video.len();
    let has_audio = !audio.is_empty() && audio_cfg.is_some();
    if let (true, Some((a_ts, a_cfg))) = (has_audio, &audio_cfg) {
        w.add_track(&TrackConfig { track_type: TrackType::Audio, timescale: *a_ts as u32, language: "und".into(), media_conf: MediaConfig::AacConfig(a_cfg.clone()) })
            .map_err(|e| format!("cannot write clip: {e}"))?;
    }
    let mut audio_iter = audio.into_iter().filter(|_| has_audio).peekable();
    for v in video {
        while audio_iter.peek().is_some_and(|a| a.ms <= v.ms) {
            if let Some(a) = audio_iter.next() {
                w.write_sample(2, &a.sample).map_err(|e| format!("cannot write clip: {e}"))?;
            }
        }
        w.write_sample(1, &v.sample).map_err(|e| format!("cannot write clip: {e}"))?;
    }
    for a in audio_iter {
        w.write_sample(2, &a.sample).map_err(|e| format!("cannot write clip: {e}"))?;
    }
    w.write_end().map_err(|e| format!("cannot finish clip: {e}"))?;
    drop(w);
    if let Err(e) = faststart(tmp) {
        // The file is still a valid MP4; only the start-up is a little slower.
        log::warn!("clips: cannot move the index to the front of {}: {e}", tmp.display());
    }
    // Never keep a clip that cannot be read back: better no clip than one that will not play.
    verify(tmp, video_count, has_audio).map_err(|e| format!("the clip file is not valid: {e}"))?;
    Ok(Cut { lead_ms: (start_ms - origin_wall).max(0), duration_ms: (end_ticks * 1000 / ts) as i64, audio: has_audio })
}

/// Reads a finished clip back: the tracks and sample counts must be what was written, and the
/// first and last video samples must be readable.
fn verify(path: &Path, video_samples: usize, audio: bool) -> Result<(), String> {
    let file = File::open(path).map_err(|e| e.to_string())?;
    let size = file.metadata().map_err(|e| e.to_string())?.len();
    let mut reader = Mp4Reader::read_header(BufReader::new(file), size).map_err(|e| e.to_string())?;
    let (vid, vn) = reader
        .tracks()
        .values()
        .find(|t| t.track_type().ok() == Some(TrackType::Video))
        .map(|t| (t.track_id(), t.sample_count()))
        .ok_or("no video track")?;
    if vn as usize != video_samples {
        return Err(format!("{vn} video samples instead of {video_samples}"));
    }
    for id in [1, vn] {
        reader.read_sample(vid, id).map_err(|e| e.to_string())?.ok_or("a video sample is missing")?;
    }
    if audio && !reader.tracks().values().any(|t| t.track_type().ok() == Some(TrackType::Audio) && t.sample_count() > 0) {
        return Err("the sound track is empty".into());
    }
    Ok(())
}

// ---------------------------------------------------------------------------------------------
// Fast start

const CONTAINERS: [&[u8; 4]; 5] = [b"moov", b"trak", b"mdia", b"minf", b"stbl"];

/// Moves the `moov` box in front of `mdat` (fixing the chunk offsets), so that a player can start
/// without reading the end of the file first. The file is left alone if it is not `ftyp mdat moov`.
pub fn faststart(path: &Path) -> Result<(), String> {
    let data = std::fs::read(path).map_err(|e| e.to_string())?;
    let boxes = top_level(&data)?;
    let kinds: Vec<&[u8]> = boxes.iter().map(|b| &data[b.0 + 4..b.0 + 8]).collect();
    if kinds != [b"ftyp".as_slice(), b"mdat", b"moov"] {
        return Err("not ftyp, mdat, moov".into());
    }
    let (ftyp, mdat, moov) = (&boxes[0], &boxes[1], &boxes[2]);
    let mut moov_bytes = data[moov.0..moov.0 + moov.1].to_vec();
    patch_offsets(&mut moov_bytes, 8, moov.1, moov.1 as u64)?;
    let mut out = Vec::with_capacity(data.len());
    out.extend_from_slice(&data[ftyp.0..ftyp.0 + ftyp.1]);
    out.extend_from_slice(&moov_bytes);
    out.extend_from_slice(&data[mdat.0..mdat.0 + mdat.1]);
    let tmp = path.with_extension("fast");
    std::fs::write(&tmp, &out).map_err(|e| e.to_string())?;
    std::fs::rename(&tmp, path).map_err(|e| {
        let _ = std::fs::remove_file(&tmp);
        e.to_string()
    })
}

/// `(offset, size)` of every top-level box.
fn top_level(data: &[u8]) -> Result<Vec<(usize, usize)>, String> {
    let mut out = Vec::new();
    let mut p = 0usize;
    while p + 8 <= data.len() {
        let mut size = u32::from_be_bytes([data[p], data[p + 1], data[p + 2], data[p + 3]]) as usize;
        if size == 1 {
            let big = data.get(p + 8..p + 16).ok_or("short box")?;
            size = u64::from_be_bytes(big.try_into().map_err(|_| "short box")?) as usize;
        } else if size == 0 {
            size = data.len() - p;
        }
        if size < 8 || p + size > data.len() {
            return Err("broken box size".into());
        }
        out.push((p, size));
        p += size;
    }
    Ok(out)
}

/// Adds `delta` to every `stco` / `co64` entry inside the container box `[start, end)` of `buf`
/// (`start` is the first byte after the container's header).
fn patch_offsets(buf: &mut [u8], start: usize, end: usize, delta: u64) -> Result<(), String> {
    let mut p = start;
    while p + 8 <= end {
        let size = u32::from_be_bytes([buf[p], buf[p + 1], buf[p + 2], buf[p + 3]]) as usize;
        if size < 8 || p + size > end {
            return Err("broken box size in moov".into());
        }
        let kind: [u8; 4] = [buf[p + 4], buf[p + 5], buf[p + 6], buf[p + 7]];
        if CONTAINERS.iter().any(|c| **c == kind) {
            patch_offsets(buf, p + 8, p + size, delta)?;
        } else if &kind == b"stco" || &kind == b"co64" {
            let wide = &kind == b"co64";
            let n = u32::from_be_bytes([buf[p + 12], buf[p + 13], buf[p + 14], buf[p + 15]]) as usize;
            let entry = if wide { 8 } else { 4 };
            if 16 + n * entry > size {
                return Err("broken chunk offset table".into());
            }
            for i in 0..n {
                let at = p + 16 + i * entry;
                if wide {
                    let v = u64::from_be_bytes(buf[at..at + 8].try_into().map_err(|_| "short")?) + delta;
                    buf[at..at + 8].copy_from_slice(&v.to_be_bytes());
                } else {
                    let v = u64::from(u32::from_be_bytes(buf[at..at + 4].try_into().map_err(|_| "short")?)) + delta;
                    let v = u32::try_from(v).map_err(|_| "chunk offset does not fit 32 bits")?;
                    buf[at..at + 4].copy_from_slice(&v.to_be_bytes());
                }
            }
        }
        p += size;
    }
    Ok(())
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use bytes::Bytes;
    use mp4::{AudioObjectType, ChannelConfig, SampleFreqIndex};

    pub const TS: u32 = 90_000;
    pub const ATS: u32 = 48_000;

    fn mp4_config() -> Mp4Config {
        Mp4Config {
            major_brand: "isom".parse().unwrap(),
            minor_version: 512,
            compatible_brands: vec!["isom".parse().unwrap(), "avc1".parse().unwrap()],
            timescale: 1000,
        }
    }

    /// Writes a synthetic H.264-looking MP4: `frames` samples at `fps`, a keyframe every `gop`
    /// frames. Sample payload = one length-prefixed fake NAL holding `[tag, frame index (u32)]`.
    /// With `audio` there is also an AAC track: 1024-sample frames covering the same time, payload
    /// `[tag, 0xA0, index (u32)]`.
    pub fn synthetic_segment_av(path: &Path, tag: u8, fps: u32, frames: u32, gop: u32, sps_tail: u8, audio: bool) {
        synthetic_segment_opts(path, tag, fps, frames, gop, sps_tail, audio, false);
    }

    /// With `reordered`, odd frames carry a composition offset of -1 frame... as encoders with
    /// B-frames do: offsets alternate between -2 and 0 frames.
    #[allow(clippy::too_many_arguments)]
    pub fn synthetic_segment_opts(path: &Path, tag: u8, fps: u32, frames: u32, gop: u32, sps_tail: u8, audio: bool, reordered: bool) {
        let mut w = Mp4Writer::write_start(BufWriter::new(File::create(path).unwrap()), &mp4_config()).unwrap();
        w.add_track(&TrackConfig {
            track_type: TrackType::Video,
            timescale: TS,
            language: "und".into(),
            media_conf: MediaConfig::AvcConfig(AvcConfig {
                width: 320,
                height: 180,
                seq_param_set: vec![0x67, 0x42, 0x00, 0x1e, sps_tail],
                pic_param_set: vec![0x68, 0xce, 0x38, 0x80],
            }),
        })
        .unwrap();
        if audio {
            w.add_track(&TrackConfig {
                track_type: TrackType::Audio,
                timescale: ATS,
                language: "und".into(),
                media_conf: MediaConfig::AacConfig(AacConfig {
                    bitrate: 192_000,
                    profile: AudioObjectType::AacLowComplexity,
                    freq_index: SampleFreqIndex::Freq48000,
                    chan_conf: ChannelConfig::Stereo,
                }),
            })
            .unwrap();
        }
        let step = TS / fps;
        for i in 0..frames {
            let mut body = vec![0, 0, 0, 5, tag];
            body.extend_from_slice(&i.to_be_bytes());
            w.write_sample(
                1,
                &Mp4Sample {
                    start_time: u64::from(i * step),
                    duration: step,
                    rendering_offset: if reordered && i % 2 == 1 { -2 * step as i32 } else { 0 },
                    is_sync: i % gop == 0,
                    bytes: Bytes::from(body),
                },
            )
            .unwrap();
        }
        if audio {
            let total_ms = u64::from(frames) * 1000 / u64::from(fps);
            let n = total_ms * u64::from(ATS) / 1000 / 1024;
            for i in 0..n {
                let mut body = vec![tag, 0xA0];
                body.extend_from_slice(&(i as u32).to_be_bytes());
                w.write_sample(2, &Mp4Sample { start_time: i * 1024, duration: 1024, rendering_offset: 0, is_sync: true, bytes: Bytes::from(body) }).unwrap();
            }
        }
        w.write_end().unwrap();
    }

    pub fn synthetic_segment(path: &Path, tag: u8, fps: u32, frames: u32, gop: u32, sps_tail: u8) {
        synthetic_segment_av(path, tag, fps, frames, gop, sps_tail, false);
    }

    /// `(tag, frame index, is_sync, duration)` of every video sample, the video timescale, and the
    /// same for sound (`None` when the file has no sound).
    #[allow(clippy::type_complexity)]
    fn read_all(path: &Path) -> (Vec<(u8, u32, bool, u32)>, u32, Option<Vec<(u8, u32, u32)>>) {
        let size = std::fs::metadata(path).unwrap().len();
        let mut r = Mp4Reader::read_header(BufReader::new(File::open(path).unwrap()), size).unwrap();
        let (vid, vn, vts) = {
            let t = r.tracks().values().find(|t| t.track_type().unwrap() == TrackType::Video).unwrap();
            (t.track_id(), t.sample_count(), t.timescale())
        };
        let aud = r.tracks().values().find(|t| t.track_type().unwrap() == TrackType::Audio).map(|t| (t.track_id(), t.sample_count()));
        let mut out = Vec::new();
        for i in 1..=vn {
            let s = r.read_sample(vid, i).unwrap().unwrap();
            let b = s.bytes;
            out.push((b[4], u32::from_be_bytes([b[5], b[6], b[7], b[8]]), s.is_sync, s.duration));
        }
        let sound = aud.map(|(id, n)| {
            (1..=n)
                .map(|i| {
                    let s = r.read_sample(id, i).unwrap().unwrap();
                    (s.bytes[0], u32::from_be_bytes([s.bytes[2], s.bytes[3], s.bytes[4], s.bytes[5]]), s.duration)
                })
                .collect()
        });
        (out, vts, sound)
    }

    fn seg(dir: &Path, start: i64, tag: u8, frames: u32) -> Segment {
        let end = start + i64::from(frames) * 1000 / 30;
        let path = dir.join(crate::clips::buffer::segment_name(start, end));
        synthetic_segment(&path, tag, 30, frames, 30, 0xab);
        Segment { start_ms: start, end_ms: end, path }
    }

    /// Writes the `.clock` of a 30 fps segment whose wall clock jumps by `jump_ms` after frame
    /// `jump_at` (the encoder's own timeline does not).
    fn write_clock(s: &Segment, frames: u32, jump_at: u32, jump_ms: i64) {
        let mut c = FrameClock::new();
        for i in 0..frames {
            let wall = s.start_ms + i64::from(i) * 1000 / 30 + if i >= jump_at { jump_ms } else { 0 };
            c.push(i64::from(i) * 10_000_000 / 30, wall);
        }
        c.write(&s.clock_path()).unwrap();
    }

    #[test]
    fn cuts_inside_one_segment_from_the_previous_keyframe() {
        let t = tempfile::tempdir().unwrap();
        // 10 s at 30 fps, keyframe every second.
        let a = seg(t.path(), 1_000_000, 1, 300);
        let out = t.path().join("clip.mp4");
        // Window 4.5 s .. 7.0 s into the segment.
        let c = cut(&[a], 1_004_500, 1_007_000, 1_005_750, &out).unwrap();
        let (samples, ts, sound) = read_all(&out);
        assert_eq!(ts, TS);
        assert!(sound.is_none() && !c.audio);
        // Starts at the keyframe of second 4 (frame 120), so the wanted start is 0.5 s in.
        assert_eq!(samples[0], (1, 120, true, TS / 30));
        assert_eq!(c.lead_ms, 500);
        // Last frame at or before 7.0 s: frame 210.
        assert_eq!(samples.last().unwrap().1, 210);
        assert_eq!(samples.len(), 91);
        assert_eq!(c.duration_ms, 91 * 1000 / 30);
    }

    #[test]
    fn stitches_two_segments_across_the_seam_with_a_gap() {
        let t = tempfile::tempdir().unwrap();
        let a = seg(t.path(), 2_000_000, 1, 90); // 3 s
        let b = seg(t.path(), 2_000_000 + 3_200, 2, 90); // starts 0.2 s after a ended
        let out = t.path().join("clip.mp4");
        let c = cut(&[a, b], 2_001_500, 2_004_000, 2_002_750, &out).unwrap();
        let (samples, _, _) = read_all(&out);
        // Keyframe at 1 s (frame 30 of a); b's frames follow, ending at 4.0 s wall = 0.8 s into b.
        assert_eq!(samples[0].1, 30);
        assert_eq!(samples[0].0, 1);
        let seam = samples.iter().position(|s| s.0 == 2).expect("second segment included");
        assert_eq!(samples[seam].1, 0);
        assert_eq!(samples[seam - 1].1, 89);
        // The gap is absorbed by the last frame before the seam, so no time is lost or doubled.
        let a_part = (90 - 30) * (TS / 30);
        assert_eq!(samples[..seam].iter().map(|s| s.3).sum::<u32>(), a_part + 200 * TS / 1000);
        assert_eq!(samples.last().unwrap().1, 24); // 0.8 s * 30 fps
        assert_eq!(c.lead_ms, 500);
        let total: u32 = samples.iter().map(|s| s.3).sum();
        assert_eq!(c.duration_ms, i64::from(total) * 1000 / i64::from(TS));
    }

    #[test]
    fn a_window_outside_the_recording_is_an_error() {
        let t = tempfile::tempdir().unwrap();
        let a = seg(t.path(), 3_000_000, 1, 90);
        let out = t.path().join("clip.mp4");
        assert!(cut(&[a.clone()], 3_100_000, 3_102_000, 3_101_000, &out).is_err());
        assert!(!out.exists());
        assert!(cut(&[], 0, 10, 5, &out).is_err());
        // Nothing but the failed attempt's partial file may be left behind.
        assert!(!out.with_extension("part").exists());
    }

    #[test]
    fn a_format_change_ends_the_clip_at_the_seam() {
        let t = tempfile::tempdir().unwrap();
        let a = seg(t.path(), 4_000_000, 1, 90);
        let b_path = t.path().join("seg-b.mp4");
        synthetic_segment(&b_path, 2, 30, 90, 30, 0xcd); // other SPS
        let b = Segment { start_ms: 4_003_000, end_ms: 4_006_000, path: b_path };
        let out = t.path().join("clip.mp4");
        // The moment lies in the first segment, so that is the footage that is kept.
        cut(&[a, b], 4_001_000, 4_005_000, 4_001_500, &out).unwrap();
        let (samples, _, _) = read_all(&out);
        assert!(samples.iter().all(|s| s.0 == 1));
    }

    /// The bug behind "clips drift after the game was minimized": the encoder's timeline held the
    /// frames back to back although 5 s of wall time passed between frame 60 and 61. Looking
    /// frames up on the wall clock puts the clip at the right place and keeps only footage that
    /// really lies on one side of the hole.
    #[test]
    fn a_hole_in_the_wall_clock_does_not_shift_later_clips() {
        let t = tempfile::tempdir().unwrap();
        let a = seg(t.path(), 5_000_000, 1, 300); // 10 s of video; the wall clock jumps 5 s after 2 s
        write_clock(&a, 300, 60, 5_000);
        // After the hole frame 90 (3 s on the encoder's timeline) is at wall 5_000_000 + 3_000 + 5_000.
        let out = t.path().join("late.mp4");
        let c = cut(&[a.clone()], 5_007_000, 5_011_000, 5_009_000, &out).unwrap();
        let (samples, _, _) = read_all(&out);
        // Wall 5_007_000 = encoder 2.0 s + (2.0 s past the jump) -> frame 60 + 30 - 5 s*30... compute:
        // wall = start + enc + 5000  =>  enc = 5_007_000 - 5_000_000 - 5_000 = 2_000 ms = frame 60.
        // The keyframe at or before it is frame 60 itself (gop 30), wall 5_007_000.
        assert_eq!(samples[0].1, 60);
        assert_eq!(c.lead_ms, 0);
        // End 5_011_000 -> enc 6_000 ms = frame 180.
        assert_eq!(samples.last().unwrap().1, 180);

        // A window that spans the hole keeps the side with the wanted moment: the part after it.
        let out2 = t.path().join("across.mp4");
        let c2 = cut(&[a.clone()], 5_000_500, 5_008_000, 5_007_500, &out2).unwrap();
        let (s2, _, _) = read_all(&out2);
        assert_eq!(s2[0].1, 60, "starts at the first keyframe after the hole");
        assert_eq!(c2.lead_ms, 0);
        assert!(s2.windows(2).all(|w| w[1].1 == w[0].1 + 1), "no frame is skipped or repeated");

        // ... and the part before it when the moment lies there.
        let out3 = t.path().join("before.mp4");
        cut(&[a.clone()], 5_000_500, 5_008_000, 5_001_000, &out3).unwrap();
        let (s3, _, _) = read_all(&out3);
        assert_eq!(s3[0].1, 0);
        assert_eq!(s3.last().unwrap().1, 59, "ends right before the hole");

        // A moment that was never recorded (deep inside the hole) skips the clip.
        let hole = cut(&[a], 5_004_000, 5_006_000, 5_005_000, &t.path().join("none.mp4"));
        assert!(hole.is_err(), "{hole:?}");
    }

    #[test]
    fn sound_is_cut_on_the_same_timeline() {
        let t = tempfile::tempdir().unwrap();
        let start = 7_000_000i64;
        let path = t.path().join(crate::clips::buffer::segment_name(start, start + 6_000));
        synthetic_segment_av(&path, 1, 30, 180, 30, 0xab, true);
        let a = Segment { start_ms: start, end_ms: start + 6_000, path };
        let path_b = t.path().join(crate::clips::buffer::segment_name(start + 6_100, start + 12_100));
        synthetic_segment_av(&path_b, 2, 30, 180, 30, 0xab, true);
        let b = Segment { start_ms: start + 6_100, end_ms: start + 12_100, path: path_b };
        let out = t.path().join("clip.mp4");
        // From 4.5 s to 8.0 s: keyframe at 4 s of a, the seam at 6 s, then 2 s of b.
        let c = cut(&[a, b], start + 4_500, start + 8_000, start + 6_000, &out).unwrap();
        assert!(c.audio);
        let (video, _, sound) = read_all(&out);
        let sound = sound.expect("a sound track");
        assert_eq!(video[0].1, 120);
        // The first sound frame starts at (or just after) the first video frame, from segment a.
        assert_eq!((sound[0].0, sound[0].1), (1, 4_000 * 48 / 1024 + u32::from((4_000 * 48) % 1024 != 0)));
        assert!(sound.iter().any(|s| s.0 == 2), "sound of the second segment follows the seam");
        // Sound reaches about as far as the video does.
        let v_ms: u32 = video.iter().map(|s| s.3).sum::<u32>() / (TS / 1000);
        let a_ms: u32 = sound.iter().map(|s| s.2).sum::<u32>() / (ATS / 1000);
        assert!((i64::from(v_ms) - i64::from(a_ms)).abs() < 200, "video {v_ms} ms, sound {a_ms} ms");
    }

    #[test]
    fn negative_composition_offsets_are_shifted_up_so_the_file_stays_valid() {
        let t = tempfile::tempdir().unwrap();
        let start = 9_000_000i64;
        let path = t.path().join(crate::clips::buffer::segment_name(start, start + 3_000));
        synthetic_segment_opts(&path, 1, 30, 90, 30, 0xab, false, true);
        let a = Segment { start_ms: start, end_ms: start + 3_000, path };
        let out = t.path().join("clip.mp4");
        cut(&[a], start + 1_000, start + 2_000, start + 1_500, &out).unwrap();
        let size = std::fs::metadata(&out).unwrap().len();
        let mut r = Mp4Reader::read_header(BufReader::new(File::open(&out).unwrap()), size).unwrap();
        let (id, n) = {
            let t = r.tracks().values().next().unwrap();
            (t.track_id(), t.sample_count())
        };
        let offsets: Vec<i32> = (1..=n).map(|i| r.read_sample(id, i).unwrap().unwrap().rendering_offset).collect();
        assert!(offsets.iter().all(|&o| o >= 0), "{offsets:?}");
        // The relative order is kept: the frames that were 2 frames earlier still are.
        assert_eq!(offsets[0] - offsets[1], 2 * (TS / 30) as i32);
    }

    #[test]
    fn faststart_puts_the_index_first_and_keeps_every_sample() {
        let t = tempfile::tempdir().unwrap();
        let a = seg(t.path(), 8_000_000, 1, 120);
        let out = t.path().join("clip.mp4");
        cut(&[a], 8_001_000, 8_003_000, 8_002_000, &out).unwrap();
        let data = std::fs::read(&out).unwrap();
        let kinds: Vec<String> = top_level(&data).unwrap().iter().map(|b| String::from_utf8_lossy(&data[b.0 + 4..b.0 + 8]).into_owned()).collect();
        assert_eq!(kinds, vec!["ftyp", "moov", "mdat"]);
        // Reading goes through the patched offsets: the payloads are still the right frames.
        let (samples, _, _) = read_all(&out);
        assert_eq!(samples.iter().map(|s| s.1).collect::<Vec<_>>(), (30..=90).collect::<Vec<u32>>());
        // A file that is not in the plain order is left alone.
        assert!(faststart(&out).is_err());
    }
}
