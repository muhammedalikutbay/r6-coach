//! Cuts a clip out of finished buffer segments without re-encoding (pure Rust, `mp4` crate).
//!
//! The clip starts at the last keyframe at or before the wanted start, so the file carries a
//! little lead-in; `Cut::lead_ms` says where the wanted start is and the player seeks there. If
//! the window crosses a segment boundary the samples of the next segment are appended with their
//! times shifted by the wall-clock distance of the two segments.

use std::fs::File;
use std::io::{BufReader, BufWriter};
use std::path::Path;

use mp4::{AvcConfig, MediaConfig, Mp4Config, Mp4Reader, Mp4Sample, Mp4Track, Mp4Writer, TrackConfig, TrackType};

use super::buffer::Segment;

/// What was written.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Cut {
    /// Where the wanted start lies inside the file (the file starts at a keyframe before it).
    pub lead_ms: i64,
    /// Length of the whole file.
    pub duration_ms: i64,
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

struct Open {
    reader: Mp4Reader<BufReader<File>>,
    track_id: u32,
    timescale: u64,
    width: u16,
    height: u16,
    sps: Vec<u8>,
    pps: Vec<u8>,
    metas: Vec<SampleMeta>,
}

fn open_segment(path: &Path) -> Result<Open, String> {
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
    Ok(Open { reader, track_id, timescale, width, height, sps, pps, metas })
}

/// Cuts the wall-clock window `[start_ms, end_ms]` out of `segments` (oldest first, as returned by
/// `buffer::select_for_window`) into the MP4 file `out`.
pub fn cut(segments: &[Segment], start_ms: i64, end_ms: i64, out: &Path) -> Result<Cut, String> {
    if segments.is_empty() || end_ms <= start_ms {
        return Err("nothing to cut".into());
    }
    let tmp = out.with_extension("part");
    let result = cut_into(segments, start_ms, end_ms, &tmp);
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

fn cut_into(segments: &[Segment], start_ms: i64, end_ms: i64, tmp: &Path) -> Result<Cut, String> {
    let mut writer: Option<Mp4Writer<BufWriter<File>>> = None;
    // Format of the first segment; later segments must match it.
    let mut format: Option<(u64, u16, u16, Vec<u8>, Vec<u8>)> = None;
    // Wall time of the first written sample, the shift of the first segment (ticks) and the
    // sample waiting for its duration to be fixed up.
    let mut origin_ms = 0i64;
    let mut first_seg_start = 0i64;
    let mut dts0 = 0u64;
    let mut pending: Option<Mp4Sample> = None;
    let mut end_ticks = 0u64;
    let mut timescale = 1000u64;
    let mut done = false;

    let mut last_wall = i64::MIN;
    for seg in segments {
        if done {
            break;
        }
        // A segment that ended before the window (or starts after it) has nothing to give.
        if seg.end_ms < start_ms || seg.start_ms > end_ms {
            continue;
        }
        let mut open = open_segment(&seg.path)?;
        if let Some(f) = &format {
            if (open.timescale, open.width, open.height) != (f.0, f.1, f.2) || open.sps != f.3 || open.pps != f.4 {
                log::info!("clips: segment {} has another video format; clip ends at the seam", seg.path.display());
                break;
            }
        }
        let first_idx = if writer.is_none() {
            let rel = (start_ms - seg.start_ms).max(0) as u64;
            let want = rel * open.timescale / 1000;
            open.metas
                .iter()
                .rposition(|m| m.is_sync && m.dts <= want)
                .or_else(|| open.metas.iter().position(|m| m.is_sync))
                .unwrap_or(0)
        } else {
            0
        };
        let ts = open.timescale;
        for idx in first_idx..open.metas.len() {
            let dts = open.metas[idx].dts;
            if seg.start_ms + (dts * 1000 / ts) as i64 > end_ms {
                done = true;
                break;
            }
            if writer.is_none() {
                first_seg_start = seg.start_ms;
                dts0 = dts;
                origin_ms = seg.start_ms + (dts * 1000 / ts) as i64;
                timescale = ts;
                format = Some((ts, open.width, open.height, open.sps.clone(), open.pps.clone()));
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
                    timescale: ts as u32,
                    language: "und".into(),
                    media_conf: MediaConfig::AvcConfig(AvcConfig {
                        width: open.width,
                        height: open.height,
                        seq_param_set: open.sps.clone(),
                        pic_param_set: open.pps.clone(),
                    }),
                })
                .map_err(|e| format!("cannot write clip: {e}"))?;
                writer = Some(w);
            }
            let sample = open
                .reader
                .read_sample(open.track_id, idx as u32 + 1)
                .map_err(|e| format!("cannot read sample: {e}"))?
                .ok_or("sample missing")?;
            // Position in the clip, in ticks.
            let shift = (seg.start_ms - first_seg_start) * ts as i64 / 1000;
            let new_dts = (shift + dts as i64 - dts0 as i64).max(0) as u64;
            let w = writer.as_mut().expect("writer exists");
            if let Some(mut prev) = pending.take() {
                prev.duration = new_dts.saturating_sub(prev.start_time).max(1).min(u64::from(u32::MAX)) as u32;
                w.write_sample(1, &prev).map_err(|e| format!("cannot write clip: {e}"))?;
            }
            last_wall = seg.start_ms + (dts * 1000 / ts) as i64;
            end_ticks = new_dts + u64::from(sample.duration);
            pending = Some(Mp4Sample {
                start_time: new_dts,
                duration: sample.duration,
                rendering_offset: sample.rendering_offset,
                is_sync: sample.is_sync,
                bytes: sample.bytes,
            });
        }
    }
    let mut w = writer.ok_or("the window holds no recorded frames")?;
    if last_wall < start_ms {
        // Only the lead-in before the window was there: the window itself was not recorded.
        return Err("the window holds no recorded frames".into());
    }
    if let Some(last) = pending.take() {
        w.write_sample(1, &last).map_err(|e| format!("cannot write clip: {e}"))?;
    }
    w.write_end().map_err(|e| format!("cannot finish clip: {e}"))?;
    Ok(Cut { lead_ms: (start_ms - origin_ms).max(0), duration_ms: (end_ticks * 1000 / timescale) as i64 })
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use bytes::Bytes;

    pub const TS: u32 = 90_000;

    /// Writes a synthetic H.264-looking MP4: `frames` samples at `fps`, a keyframe every `gop`
    /// frames. Sample payload = one length-prefixed fake NAL holding `[tag, frame index (u32)]`.
    pub fn synthetic_segment(path: &Path, tag: u8, fps: u32, frames: u32, gop: u32, sps_tail: u8) {
        let cfg = Mp4Config {
            major_brand: "isom".parse().unwrap(),
            minor_version: 512,
            compatible_brands: vec!["isom".parse().unwrap(), "avc1".parse().unwrap()],
            timescale: 1000,
        };
        let mut w = Mp4Writer::write_start(BufWriter::new(File::create(path).unwrap()), &cfg).unwrap();
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
        let step = TS / fps;
        for i in 0..frames {
            let mut body = vec![0, 0, 0, 5, tag];
            body.extend_from_slice(&i.to_be_bytes());
            w.write_sample(
                1,
                &Mp4Sample { start_time: u64::from(i * step), duration: step, rendering_offset: 0, is_sync: i % gop == 0, bytes: Bytes::from(body) },
            )
            .unwrap();
        }
        w.write_end().unwrap();
    }

    fn read_all(path: &Path) -> (Vec<(u8, u32, bool, u32)>, u32) {
        let size = std::fs::metadata(path).unwrap().len();
        let mut r = Mp4Reader::read_header(BufReader::new(File::open(path).unwrap()), size).unwrap();
        let (id, n) = {
            let t = r.tracks().values().next().unwrap();
            (t.track_id(), t.sample_count())
        };
        let mut out = Vec::new();
        for i in 1..=n {
            let s = r.read_sample(id, i).unwrap().unwrap();
            let b = s.bytes;
            out.push((b[4], u32::from_be_bytes([b[5], b[6], b[7], b[8]]), s.is_sync, s.duration));
        }
        (out, r.tracks().values().next().unwrap().timescale())
    }

    fn seg(dir: &Path, start: i64, tag: u8, frames: u32) -> Segment {
        let end = start + i64::from(frames) * 1000 / 30;
        let path = dir.join(crate::clips::buffer::segment_name(start, end));
        synthetic_segment(&path, tag, 30, frames, 30, 0xab);
        Segment { start_ms: start, end_ms: end, path }
    }

    #[test]
    fn cuts_inside_one_segment_from_the_previous_keyframe() {
        let t = tempfile::tempdir().unwrap();
        // 10 s at 30 fps, keyframe every second.
        let a = seg(t.path(), 1_000_000, 1, 300);
        let out = t.path().join("clip.mp4");
        // Window 4.5 s .. 7.0 s into the segment.
        let c = cut(&[a], 1_004_500, 1_007_000, &out).unwrap();
        let (samples, ts) = read_all(&out);
        assert_eq!(ts, TS);
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
        let c = cut(&[a, b], 2_001_500, 2_004_000, &out).unwrap();
        let (samples, _) = read_all(&out);
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
        assert!(cut(&[a.clone()], 3_100_000, 3_102_000, &out).is_err());
        assert!(!out.exists());
        assert!(cut(&[], 0, 10, &out).is_err());
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
        cut(&[a, b], 4_001_000, 4_005_000, &out).unwrap();
        let (samples, _) = read_all(&out);
        assert!(samples.iter().all(|s| s.0 == 1));
    }
}
