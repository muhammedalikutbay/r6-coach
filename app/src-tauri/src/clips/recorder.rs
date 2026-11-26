//! Rolling recording of the game window (opt-in).
//!
//! A background thread polls the window list every few seconds. While recording is enabled and a
//! `RainbowSix*` window is open and not minimized, it captures that window with the Windows
//! Graphics Capture API (the same thing OBS "Window Capture" uses: no injection, no memory
//! reading) and encodes it with the hardware H.264 encoder into one-minute segment files in a
//! temp folder. Old segments are deleted after ten minutes. Nothing is downloaded at run time.
//!
//! Every frame is stamped with the wall-clock time it was captured at (a `.clock` file next to the
//! segment), and the game's sound is muxed into the same file (`audio`).
//!
//! Only the capture path is Windows specific; everything that decides *what* to cut lives in the
//! other `clips` modules and is tested without a window.

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicI64, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::time::Duration;

use serde::Serialize;

use super::audio::AudioMode;
use super::buffer;
use super::ClipSettings;

pub fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum RecState {
    /// Recording is switched off.
    Off,
    /// Switched on, the game is not running (or has no capturable window).
    Waiting,
    Recording,
    /// The game window is minimized: nothing is recorded.
    Minimized,
    Error,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RecStatus {
    pub state: RecState,
    pub game_running: bool,
    /// The first seconds of the capture were black or empty: the game is probably in exclusive
    /// fullscreen. The user must switch to borderless / windowed.
    pub black_frames: bool,
    pub message: String,
    /// Where the clips' sound comes from.
    pub audio: AudioMode,
}

impl Default for RecStatus {
    fn default() -> Self {
        Self { state: RecState::Off, game_running: false, black_frames: false, message: String::new(), audio: AudioMode::Off }
    }
}

/// Something the app should tell the user.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RecEvent {
    /// The capture shows (or no longer shows) only black frames. Sent once per game session.
    BlackFrames(bool),
}

/// A top-level window, as seen by the matcher.
#[derive(Clone, Debug)]
pub struct WindowInfo {
    pub hwnd: isize,
    pub pid: u32,
    /// Executable file name, e.g. `RainbowSix.exe`.
    pub exe: String,
    /// Only used by tests to find their own window.
    #[allow(dead_code)]
    pub title: String,
    pub width: i32,
    pub height: i32,
    pub minimized: bool,
}

pub type Matcher = Box<dyn Fn(&WindowInfo) -> bool + Send + Sync>;

/// The Siege window: any `RainbowSix*.exe` (`RainbowSix.exe`, `RainbowSix_Vulkan.exe`, ...).
pub fn game_matcher() -> Matcher {
    Box::new(|w| w.exe.to_ascii_lowercase().starts_with("rainbowsix") && w.width >= 640 && w.height >= 360)
}

#[derive(Clone, Copy, Debug)]
pub struct RecorderConfig {
    pub segment_secs: u64,
    pub poll: Duration,
    /// Frame rate of the clips, and the rate the encoder is told to expect.
    ///
    /// Media Foundation converts whatever arrives to this constant rate and re-times the stream
    /// when frames come slower (measured: content up to 1 s off after the game's frame rate
    /// dropped). So the recorder never lets it see an irregular stream: it sends at most this many
    /// frames per second (`SlotLimiter`) and stamps the n-th one at `n / fps`; the real time of
    /// every sent frame goes to the `.clock` file and the cutter builds the clip's timeline from
    /// it. 30 fps is plenty for kill clips.
    pub fps: u32,
}

/// See `RecorderConfig::fps`.
pub const CLIP_FPS: u32 = 30;

/// Decides which frames of the capture go to the encoder so that it sees a constant frame rate.
///
/// Frames are offered with their capture time (100 ns, since the first frame of the segment). A
/// frame is sent when its capture time reaches the next slot (a quarter of a slot early still
/// counts, so a frame that is a hair early is not skipped for a whole slot); faster frames are
/// dropped. When the game is slower than the slots every frame is sent, and the slots start over
/// from that frame. The n-th sent frame is stamped `n / fps` (`stamp`), a perfect constant-rate
/// stream, so that Media Foundation keeps exactly one sample per frame and never re-times anything;
/// the real time of each sent frame goes to the `.clock` file instead.
#[cfg_attr(not(windows), allow(dead_code))]
#[derive(Clone, Copy, Debug)]
struct SlotLimiter {
    fps: u32,
    interval: i64,
    next_slot: i64,
    sent: u64,
}

#[cfg_attr(not(windows), allow(dead_code))]
impl SlotLimiter {
    fn new(fps: u32) -> Self {
        let fps = fps.max(1);
        Self { fps, interval: 10_000_000 / i64::from(fps), next_slot: 0, sent: 0 }
    }

    /// `Some(n)`: send this frame as the n-th (0-based); `None`: drop it.
    fn offer(&mut self, capture_ticks: i64) -> Option<u64> {
        if capture_ticks + self.interval / 4 < self.next_slot {
            return None;
        }
        let n = self.sent;
        self.sent += 1;
        self.next_slot += self.interval;
        if self.next_slot <= capture_ticks {
            // The game is slower than the slots: restart the slots from this frame.
            self.next_slot = capture_ticks + self.interval;
        }
        Some(n)
    }

    /// The stamp (100 ns) of the n-th sent frame.
    fn stamp(&self, n: u64) -> i64 {
        (n as i128 * 10_000_000 / i128::from(self.fps)) as i64
    }
}

impl Default for RecorderConfig {
    fn default() -> Self {
        Self { segment_secs: buffer::SEGMENT_SECS, poll: Duration::from_secs(3), fps: CLIP_FPS }
    }
}

struct Shared {
    dir: PathBuf,
    status: Mutex<RecStatus>,
    /// Held while the buffer is read (cut) or pruned.
    buffer_lock: Mutex<()>,
    /// Finishes the segment being written; set while a capture runs.
    flush_fn: Mutex<Option<Arc<dyn Fn() + Send + Sync>>>,
    /// Wall time of the last frame the capture delivered (0 = none yet).
    last_frame_ms: AtomicI64,
    /// Wall time of the segment being written (0 = none).
    active_start_ms: AtomicI64,
    shutdown: AtomicBool,
    wake: (Mutex<bool>, Condvar),
}

/// Handle to the recorder thread. Cloning shares it.
#[derive(Clone)]
pub struct Recorder {
    shared: Arc<Shared>,
}

impl Recorder {
    /// Starts the recorder thread. It does nothing until `settings.enabled` is true.
    pub fn start(
        dir: PathBuf,
        settings: Arc<Mutex<ClipSettings>>,
        matcher: Matcher,
        config: RecorderConfig,
        on_event: Arc<dyn Fn(RecEvent) + Send + Sync>,
    ) -> Recorder {
        let shared = Arc::new(Shared {
            dir,
            status: Mutex::new(RecStatus::default()),
            buffer_lock: Mutex::new(()),
            flush_fn: Mutex::new(None),
            last_frame_ms: AtomicI64::new(0),
            active_start_ms: AtomicI64::new(0),
            shutdown: AtomicBool::new(false),
            wake: (Mutex::new(false), Condvar::new()),
        });
        let s2 = shared.clone();
        if let Err(e) = std::thread::Builder::new()
            .name("clip-recorder".into())
            .spawn(move || run(s2, settings, matcher, config, on_event))
        {
            log::error!("clips: cannot start the recorder thread: {e}");
        }
        Recorder { shared }
    }

    pub fn status(&self) -> RecStatus {
        self.shared.status.lock().map(|s| s.clone()).unwrap_or_default()
    }

    /// Re-reads the settings now instead of at the next poll.
    pub fn wake(&self) {
        let (m, cv) = &self.shared.wake;
        if let Ok(mut flag) = m.lock() {
            *flag = true;
            cv.notify_all();
        }
    }

    /// Stops the recorder thread (used by tests; the app lets the process exit end it).
    #[allow(dead_code)]
    pub fn shutdown(&self) {
        self.shared.shutdown.store(true, Ordering::SeqCst);
        self.wake();
    }

    /// Finishes the segment being written so that it can be cut. Blocks briefly.
    pub fn flush(&self) {
        let f = self.shared.flush_fn.lock().ok().and_then(|g| g.clone());
        if let Some(f) = f {
            f();
        }
    }

    /// Runs `f` with the buffer protected from pruning.
    pub fn with_buffer<T>(&self, f: impl FnOnce(&std::path::Path) -> T) -> T {
        let _guard = self.shared.buffer_lock.lock().unwrap_or_else(|e| e.into_inner());
        f(&self.shared.dir)
    }

    pub fn buffer_dir(&self) -> PathBuf {
        self.shared.dir.clone()
    }
}

fn set_status(shared: &Shared, f: impl FnOnce(&mut RecStatus)) {
    if let Ok(mut s) = shared.status.lock() {
        f(&mut s);
    }
}

fn run(
    shared: Arc<Shared>,
    settings: Arc<Mutex<ClipSettings>>,
    matcher: Matcher,
    config: RecorderConfig,
    on_event: Arc<dyn Fn(RecEvent) + Send + Sync>,
) {
    if let Err(e) = std::fs::create_dir_all(&shared.dir) {
        log::error!("clips: cannot create the buffer folder {}: {e}", shared.dir.display());
        set_status(&shared, |s| {
            s.state = RecState::Error;
            s.message = format!("Cannot create the buffer folder: {e}");
        });
        return;
    }
    let mut platform = platform::Platform::new();
    let mut was_enabled = false;
    let mut warned = false;
    let mut last_error = String::new();
    let mut session_start_ms = 0i64;
    while !shared.shutdown.load(Ordering::SeqCst) {
        let enabled = settings.lock().map(|s| s.enabled).unwrap_or(false);
        if !enabled {
            platform.stop_capture(&shared);
            if was_enabled {
                // Turned off: the buffer has no use any more.
                let _g = shared.buffer_lock.lock().unwrap_or_else(|e| e.into_inner());
                buffer::clear(&shared.dir);
                log::info!("clips: recording switched off; buffer cleared");
            }
            set_status(&shared, |s| *s = RecStatus::default());
            warned = false;
        } else {
            let windows = platform.list_windows();
            let target = windows.iter().filter(|w| matcher(w)).max_by_key(|w| i64::from(w.width) * i64::from(w.height)).cloned();
            match target {
                None => {
                    platform.stop_capture(&shared);
                    set_status(&shared, |s| {
                        *s = RecStatus { state: RecState::Waiting, ..RecStatus::default() };
                    });
                    warned = false;
                }
                Some(w) if w.minimized => {
                    platform.stop_capture(&shared);
                    set_status(&shared, |s| {
                        s.state = RecState::Minimized;
                        s.game_running = true;
                        s.message = "The game is minimized; nothing is recorded.".into();
                    });
                }
                Some(w) => {
                    let want_audio = settings.lock().map(|s| s.record_audio).unwrap_or(true);
                    if platform.needs_restart(w.hwnd, want_audio) {
                        platform.stop_capture(&shared);
                    }
                    if !platform.is_capturing() {
                        shared.last_frame_ms.store(0, Ordering::SeqCst);
                        match platform.start_capture(&shared, &w, config, want_audio) {
                            Ok(()) => {
                                session_start_ms = now_ms();
                                log::info!("clips: recording {} ({}x{}) pid {}", w.exe, w.width, w.height, w.pid);
                                last_error.clear();
                            }
                            Err(e) => {
                                if e != last_error {
                                    log::warn!("clips: cannot start the capture: {e}");
                                    last_error = e.clone();
                                }
                                set_status(&shared, |s| {
                                    s.state = RecState::Error;
                                    s.game_running = true;
                                    s.message = e;
                                });
                            }
                        }
                    }
                    if platform.is_capturing() {
                        // No frame at all for a while also means "exclusive fullscreen".
                        let silent = shared.last_frame_ms.load(Ordering::SeqCst) == 0 && now_ms() - session_start_ms > 15_000;
                        let black = silent || platform.black_flag();
                        let audio = platform.audio_mode();
                        set_status(&shared, |s| {
                            s.state = RecState::Recording;
                            s.game_running = true;
                            s.message.clear();
                            s.black_frames = black;
                            s.audio = audio;
                        });
                        if black && !warned {
                            warned = true;
                            on_event(RecEvent::BlackFrames(true));
                        }
                    }
                }
            }
            let active = shared.active_start_ms.load(Ordering::SeqCst);
            let _g = shared.buffer_lock.lock().unwrap_or_else(|e| e.into_inner());
            buffer::prune(&shared.dir, now_ms(), buffer::KEEP_MS, (active != 0).then_some(active));
        }
        was_enabled = enabled;
        // Sleep until the next poll or until the settings change.
        let (m, cv) = &shared.wake;
        if let Ok(guard) = m.lock() {
            let (mut g, _) = cv.wait_timeout_while(guard, config.poll, |woken| !*woken && !shared.shutdown.load(Ordering::SeqCst)).unwrap_or_else(|e| e.into_inner());
            *g = false;
        }
    }
    platform.stop_capture(&shared);
}

#[cfg(windows)]
mod platform {
    use std::collections::HashMap;
    use std::sync::atomic::Ordering;
    use std::sync::Arc;
    use std::time::Instant;

    use windows::Win32::Foundation::{CloseHandle, HWND};
    use windows::Win32::System::Threading::{OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION};
    use windows::Win32::UI::WindowsAndMessaging::IsIconic;
    use windows_capture::capture::{CaptureControl, Context, GraphicsCaptureApiHandler};
    use windows_capture::encoder::{AudioSettingsBuilder, ContainerSettingsBuilder, VideoEncoder, VideoSettingsBuilder, VideoSettingsSubType};
    use windows_capture::frame::Frame;
    use windows_capture::graphics_capture_api::InternalCaptureControl;
    use windows_capture::settings::{
        ColorFormat, CursorCaptureSettings, DirtyRegionSettings, DrawBorderSettings, MinimumUpdateIntervalSettings, SecondaryWindowSettings, Settings,
    };
    use windows_capture::window::Window;

    use super::super::audio::{self, AudioCapture, AudioMode, PcmQueue};
    use super::super::buffer;
    use super::super::clock::FrameClock;
    use super::{now_ms, RecorderConfig, Shared, SlotLimiter, WindowInfo};

    type Err = Box<dyn std::error::Error + Send + Sync>;

    /// First seconds of a capture in which "only black" counts as a problem.
    const BLACK_WINDOW_MS: i64 = 25_000;

    struct ActiveSeg {
        encoder: VideoEncoder,
        start_ms: i64,
        started: Instant,
        part: std::path::PathBuf,
        width: u32,
        height: u32,
        frames: u32,
        /// Capture time (100 ns, the system-relative clock) of the first frame.
        first_ts: Option<i64>,
        /// Per sent frame: `(stamp the encoder got, capture time since the first frame in 100 ns,
        /// wall ms when the frame reached us)`.
        sent: Vec<(i64, i64, i64)>,
        limiter: SlotLimiter,
        /// Sound frames handed to the encoder so far.
        audio_frames: u64,
    }

    struct BlackCheck {
        started: Instant,
        last_sample: Option<Instant>,
        samples: u32,
        saw_content: bool,
        /// Shared with the poll loop.
        flag: Arc<std::sync::atomic::AtomicBool>,
    }

    impl BlackCheck {
        fn due(&self) -> bool {
            let every = if (self.started.elapsed().as_millis() as i64) < BLACK_WINDOW_MS { 1000 } else { 30_000 };
            self.last_sample.is_none_or(|t| t.elapsed().as_millis() >= every)
        }

        fn sample(&mut self, frame: &mut Frame) {
            self.last_sample = Some(Instant::now());
            let content = match frame.buffer() {
                Ok(mut buf) => {
                    let (w, h, pitch) = (buf.width() as usize, buf.height() as usize, buf.row_pitch() as usize);
                    let raw = buf.as_raw_buffer();
                    has_content(raw, w, h, pitch)
                }
                Err(e) => {
                    log::debug!("clips: cannot read a frame for the black check: {e}");
                    return;
                }
            };
            self.samples += 1;
            if content {
                self.saw_content = true;
                self.flag.store(false, Ordering::SeqCst);
            } else if !self.saw_content && (self.started.elapsed().as_millis() as i64) >= BLACK_WINDOW_MS {
                self.flag.store(true, Ordering::SeqCst);
            }
        }
    }

    /// True when a grid of sample points holds anything but (near) black. 4 bytes per pixel.
    pub(super) fn has_content(raw: &[u8], width: usize, height: usize, pitch: usize) -> bool {
        if width == 0 || height == 0 {
            return false;
        }
        for gy in 0..18 {
            for gx in 0..32 {
                let (x, y) = (gx * width / 32 + width / 64, gy * height / 18 + height / 36);
                let i = y * pitch + x * 4;
                if let Some(px) = raw.get(i..i + 3) {
                    if px.iter().any(|&c| c > 12) {
                        return true;
                    }
                }
            }
        }
        false
    }

    struct Capturer {
        dir: std::path::PathBuf,
        shared: Arc<Shared>,
        segment_secs: u64,
        fps: u32,
        seg: Option<ActiveSeg>,
        black: BlackCheck,
        /// Captured game sound, when it is being recorded.
        audio: Option<Arc<PcmQueue>>,
    }

    struct Flags {
        shared: Arc<Shared>,
        config: RecorderConfig,
        black_flag: Arc<std::sync::atomic::AtomicBool>,
        audio: Option<Arc<PcmQueue>>,
    }

    fn bitrate_for(w: u32, h: u32) -> u32 {
        let scale = (f64::from(w) * f64::from(h)) / (1920.0 * 1080.0);
        (10_000_000.0 * scale).clamp(4_000_000.0, 30_000_000.0) as u32
    }

    impl Capturer {
        fn begin_segment(&mut self, w: u32, h: u32) -> Result<(), Err> {
            let (ew, eh) = ((w + 1) & !1, (h + 1) & !1);
            let start_ms = now_ms();
            let part = self.dir.join(buffer::part_name(start_ms));
            // Sound that was captured while the previous segment was being closed belongs to no
            // segment: start from now (the video clock pads what is missing with silence).
            if let Some(q) = &self.audio {
                q.clear();
            }
            let encoder = VideoEncoder::new(
                VideoSettingsBuilder::new(ew, eh).sub_type(VideoSettingsSubType::H264).frame_rate(self.fps).bitrate(bitrate_for(ew, eh)),
                AudioSettingsBuilder::default().disabled(self.audio.is_none()),
                ContainerSettingsBuilder::default(),
                &part,
            )?;
            self.shared.active_start_ms.store(start_ms, Ordering::SeqCst);
            self.seg = Some(ActiveSeg {
                encoder,
                start_ms,
                started: Instant::now(),
                part,
                width: w,
                height: h,
                frames: 0,
                first_ts: None,
                sent: Vec::new(),
                limiter: SlotLimiter::new(self.fps),
                audio_frames: 0,
            });
            Ok(())
        }

        /// Closes the segment being written and publishes it under its final name.
        fn finish_segment(&mut self) {
            let Some(seg) = self.seg.take() else { return };
            self.shared.active_start_ms.store(0, Ordering::SeqCst);
            if let Err(e) = seg.encoder.finish() {
                log::warn!("clips: cannot finish a segment: {e}");
                let _ = std::fs::remove_file(&seg.part);
                return;
            }
            if seg.frames == 0 {
                let _ = std::fs::remove_file(&seg.part);
                return;
            }
            // The segment spans the first to the last frame it holds (not the moment the capture
            // was stopped, which can be seconds after the last frame when the game is minimized).
            let clock = FrameClock::from_sent(&seg.sent);
            let start_ms = clock.first_wall().unwrap_or(seg.start_ms);
            let end_ms = clock.last_wall().unwrap_or_else(now_ms).max(start_ms);
            // The clock goes first: a finished segment must never be visible without it.
            if let Err(e) = clock.write(&self.dir.join(buffer::clock_name(start_ms, end_ms))) {
                log::warn!("clips: cannot write the frame clock: {e}");
            }
            let done = self.dir.join(buffer::segment_name(start_ms, end_ms));
            if let Err(e) = std::fs::rename(&seg.part, &done) {
                log::warn!("clips: cannot rename a segment: {e}");
                let _ = std::fs::remove_file(&seg.part);
                let _ = std::fs::remove_file(self.dir.join(buffer::clock_name(start_ms, end_ms)));
            }
        }
    }

    impl Drop for Capturer {
        fn drop(&mut self) {
            self.finish_segment();
        }
    }

    impl GraphicsCaptureApiHandler for Capturer {
        type Flags = Flags;
        type Error = Err;

        fn new(ctx: Context<Self::Flags>) -> Result<Self, Self::Error> {
            let Flags { shared, config, black_flag, audio } = ctx.flags;
            Ok(Capturer {
                dir: shared.dir.clone(),
                shared,
                segment_secs: config.segment_secs,
                fps: config.fps,
                seg: None,
                black: BlackCheck { started: Instant::now(), last_sample: None, samples: 0, saw_content: false, flag: black_flag },
                audio,
            })
        }

        fn on_frame_arrived(&mut self, frame: &mut Frame, _control: InternalCaptureControl) -> Result<(), Self::Error> {
            let arrived_ms = now_ms();
            let (fw, fh) = (frame.width(), frame.height());
            let rotate = self
                .seg
                .as_ref()
                .is_some_and(|s| s.started.elapsed().as_secs() >= self.segment_secs || (s.width, s.height) != (fw, fh));
            if rotate {
                self.finish_segment();
            }
            if self.seg.is_none() {
                self.begin_segment(fw, fh)?;
            }
            if let Some(seg) = self.seg.as_mut() {
                // Capture time since the first frame of the segment (the first offered frame is
                // always sent, so that is the first frame of the file).
                let ts = frame.timestamp().map(|t| t.Duration).unwrap_or(0);
                let rel = (ts - *seg.first_ts.get_or_insert(ts)).max(0);
                let slot = seg.limiter.offer(rel);
                if let Some(n) = slot {
                    let stamp = seg.limiter.stamp(n);
                    seg.encoder.send_frame_at(frame, stamp)?;
                    seg.frames += 1;
                    seg.sent.push((stamp, rel, arrived_ms));
                }
                // The sound stays on the real capture clock: its time 0 is the first frame's wall time.
                if let (Some(queue), Some(_)) = (&self.audio, slot) {
                    let pcm = queue.take_all();
                    let incoming = (pcm.len() / audio::BYTES_PER_FRAME) as u64;
                    let plan = audio::plan_block(seg.audio_frames, incoming, audio::frames_for(rel));
                    let keep = &pcm[(plan.drop_frames as usize * audio::BYTES_PER_FRAME).min(pcm.len())..];
                    let mut block = vec![0u8; plan.pad_frames as usize * audio::BYTES_PER_FRAME];
                    block.extend_from_slice(keep);
                    if !block.is_empty() {
                        seg.encoder.send_audio_buffer(&block, 0)?;
                        seg.audio_frames += (block.len() / audio::BYTES_PER_FRAME) as u64;
                    }
                }
            }
            self.shared.last_frame_ms.store(arrived_ms, Ordering::SeqCst);
            if self.black.due() {
                self.black.sample(frame);
            }
            Ok(())
        }

        fn on_closed(&mut self) -> Result<(), Self::Error> {
            self.finish_segment();
            Ok(())
        }
    }

    pub struct Platform {
        control: Option<CaptureControl<Capturer, Err>>,
        hwnd: isize,
        black_flag: Arc<std::sync::atomic::AtomicBool>,
        exe_cache: HashMap<u32, String>,
        /// The sound capture that goes with the current video capture (if any).
        sound: Option<AudioCapture>,
        /// Whether the current capture was started with sound wanted.
        sound_wanted: bool,
        /// Sound was wanted but could not be started.
        sound_failed: bool,
    }

    impl Platform {
        pub fn new() -> Self {
            Platform {
                control: None,
                hwnd: 0,
                black_flag: Arc::new(std::sync::atomic::AtomicBool::new(false)),
                exe_cache: HashMap::new(),
                sound: None,
                sound_wanted: false,
                sound_failed: false,
            }
        }

        pub fn audio_mode(&self) -> AudioMode {
            match (&self.sound, self.sound_wanted && self.sound_failed) {
                (Some(s), _) => s.mode(),
                (None, true) => AudioMode::Failed,
                (None, false) => AudioMode::Off,
            }
        }

        pub fn is_capturing(&self) -> bool {
            self.control.is_some()
        }

        pub fn black_flag(&self) -> bool {
            self.black_flag.load(Ordering::SeqCst)
        }

        /// The capture thread ended (window closed) or the target window changed.
        pub fn needs_restart(&self, hwnd: isize, want_audio: bool) -> bool {
            self.control.as_ref().is_some_and(|c| c.is_finished() || self.hwnd != hwnd || self.sound_wanted != want_audio)
        }

        pub fn list_windows(&mut self) -> Vec<WindowInfo> {
            let Ok(list) = Window::enumerate() else { return Vec::new() };
            if self.exe_cache.len() > 512 {
                self.exe_cache.clear();
            }
            let mut out = Vec::new();
            for w in list {
                let Ok(pid) = w.process_id() else { continue };
                let hwnd = w.as_raw_hwnd();
                let exe = self.exe_cache.entry(pid).or_insert_with(|| exe_name(pid).unwrap_or_default()).clone();
                if exe.is_empty() {
                    continue;
                }
                let Ok(r) = w.rect() else { continue };
                out.push(WindowInfo {
                    hwnd: hwnd as isize,
                    pid,
                    exe,
                    title: w.title().unwrap_or_default(),
                    width: r.right - r.left,
                    height: r.bottom - r.top,
                    minimized: unsafe { IsIconic(HWND(hwnd)).as_bool() },
                });
            }
            out
        }

        pub fn start_capture(&mut self, shared: &Arc<Shared>, w: &WindowInfo, config: RecorderConfig, want_audio: bool) -> Result<(), String> {
            self.black_flag.store(false, Ordering::SeqCst);
            // Sound first, so that the first segment already has it. A failure only costs the sound.
            let queue = Arc::new(PcmQueue::default());
            self.sound_wanted = want_audio;
            self.sound_failed = false;
            self.sound = None;
            if want_audio {
                match AudioCapture::start(w.pid, queue.clone()) {
                    Ok(s) => self.sound = Some(s),
                    Err(e) => {
                        log::warn!("clips: no sound in the clips: {e}");
                        self.sound_failed = true;
                    }
                }
            }
            let window = Window::from_raw_hwnd(w.hwnd as *mut std::ffi::c_void);
            let settings = Settings::new(
                window,
                CursorCaptureSettings::WithoutCursor,
                DrawBorderSettings::WithoutBorder,
                SecondaryWindowSettings::Default,
                MinimumUpdateIntervalSettings::Default,
                DirtyRegionSettings::Default,
                ColorFormat::Bgra8,
                Flags { shared: shared.clone(), config, black_flag: self.black_flag.clone(), audio: self.sound.is_some().then(|| queue.clone()) },
            );
            let control = Capturer::start_free_threaded(settings).map_err(|e| {
                self.sound = None;
                e.to_string()
            })?;
            let cb = control.callback();
            if let Ok(mut f) = shared.flush_fn.lock() {
                *f = Some(Arc::new(move || cb.lock().finish_segment()));
            }
            self.control = Some(control);
            self.hwnd = w.hwnd;
            Ok(())
        }

        pub fn stop_capture(&mut self, shared: &Shared) {
            if let Ok(mut f) = shared.flush_fn.lock() {
                *f = None;
            }
            if let Some(control) = self.control.take() {
                let cb = control.callback();
                // Finish the open segment ourselves so it is kept as a normal segment.
                cb.lock().finish_segment();
                if let Err(e) = control.stop() {
                    log::debug!("clips: stopping the capture: {e}");
                }
                log::info!("clips: recording stopped");
            }
            // The sound follows the video: no capture, no sound thread.
            self.sound = None;
            self.sound_failed = false;
            shared.active_start_ms.store(0, Ordering::SeqCst);
        }
    }

    fn exe_name(pid: u32) -> Option<String> {
        unsafe {
            let h = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid).ok()?;
            let mut buf = [0u16; 520];
            let mut len = buf.len() as u32;
            let ok = QueryFullProcessImageNameW(h, PROCESS_NAME_WIN32, windows::core::PWSTR(buf.as_mut_ptr()), &mut len).is_ok();
            let _ = CloseHandle(h);
            if !ok {
                return None;
            }
            let path = String::from_utf16_lossy(&buf[..len as usize]);
            path.rsplit(['\\', '/']).next().map(str::to_string)
        }
    }
}

#[cfg(not(windows))]
mod platform {
    use std::sync::Arc;

    use super::{RecorderConfig, Shared, WindowInfo};

    /// Recording needs the Windows Graphics Capture API; elsewhere it never starts.
    pub struct Platform;

    impl Platform {
        pub fn new() -> Self {
            Platform
        }
        pub fn is_capturing(&self) -> bool {
            false
        }
        pub fn black_flag(&self) -> bool {
            false
        }
        pub fn needs_restart(&self, _hwnd: isize, _want_audio: bool) -> bool {
            false
        }
        pub fn audio_mode(&self) -> super::AudioMode {
            super::AudioMode::Off
        }
        pub fn list_windows(&mut self) -> Vec<WindowInfo> {
            Vec::new()
        }
        pub fn start_capture(&mut self, _s: &Arc<Shared>, _w: &WindowInfo, _c: RecorderConfig, _audio: bool) -> Result<(), String> {
            Err("Recording is only available on Windows.".into())
        }
        pub fn stop_capture(&mut self, _s: &Shared) {}
    }
}

#[cfg(all(test, windows))]
pub(crate) mod tests {
    use super::*;

    #[test]
    fn black_check_sees_content_and_black() {
        let (w, h, pitch) = (64usize, 36usize, 64 * 4);
        let black = vec![0u8; pitch * h];
        assert!(!platform::has_content(&black, w, h, pitch));
        let mut lit = black.clone();
        for px in lit.chunks_mut(4) {
            px[0] = 90;
            px[1] = 60;
            px[2] = 30;
        }
        assert!(platform::has_content(&lit, w, h, pitch));
        assert!(!platform::has_content(&[], 0, 0, 0));
    }

    /// A 5 s mono 48 kHz 16 bit sine (440 Hz) the test window plays in a loop, so that the game
    /// sound capture has something to hear.
    fn write_tone_wav(path: &std::path::Path) {
        let (rate, secs) = (48_000u32, 5u32);
        let n = rate * secs;
        let mut data = Vec::with_capacity(n as usize * 2);
        for i in 0..n {
            let v = ((i as f64 * 440.0 * std::f64::consts::TAU / f64::from(rate)).sin() * 12_000.0) as i16;
            data.extend_from_slice(&v.to_le_bytes());
        }
        let mut out = Vec::new();
        out.extend_from_slice(b"RIFF");
        out.extend_from_slice(&(36 + data.len() as u32).to_le_bytes());
        out.extend_from_slice(b"WAVEfmt ");
        out.extend_from_slice(&16u32.to_le_bytes());
        out.extend_from_slice(&1u16.to_le_bytes()); // PCM
        out.extend_from_slice(&1u16.to_le_bytes()); // mono
        out.extend_from_slice(&rate.to_le_bytes());
        out.extend_from_slice(&(rate * 2).to_le_bytes());
        out.extend_from_slice(&2u16.to_le_bytes());
        out.extend_from_slice(&16u16.to_le_bytes());
        out.extend_from_slice(b"data");
        out.extend_from_slice(&(data.len() as u32).to_le_bytes());
        out.extend_from_slice(&data);
        std::fs::write(path, out).unwrap();
    }

    /// A WinForms window whose background colour encodes the wall clock: red = whole seconds modulo
    /// 8 in steps of 32 (big steps survive video compression), green = the fraction (0..255), so a
    /// decoded frame tells when it was on screen (given a rough idea of it). It writes its
    /// start (Unix ms) into `t0.txt` and can minimize itself for a while.
    pub(crate) fn spawn_test_window(dir: &std::path::Path, minimize_at_s: u32, minimize_for_s: u32, with_sound: bool) -> (std::process::Child, i64) {
        spawn_test_window_slow(dir, minimize_at_s, minimize_for_s, with_sound, None)
    }

    /// Like `spawn_test_window`; `slow = Some((from_s, to_s, ms))` repaints only every `ms` between
    /// the two seconds (and as fast as possible otherwise).
    pub(crate) fn spawn_test_window_slow(
        dir: &std::path::Path,
        minimize_at_s: u32,
        minimize_for_s: u32,
        with_sound: bool,
        slow: Option<(u32, u32, u32)>,
    ) -> (std::process::Child, i64) {
        use std::process::{Command, Stdio};
        let script = dir.join("win.ps1");
        std::fs::write(
            &script,
            r#"param([string]$T0File, [int]$MinAt, [int]$MinFor, [int]$Freeze, [string]$Wav, [int]$SlowFrom = 0, [int]$SlowTo = 0, [int]$SlowMs = 80)
Add-Type -AssemblyName System.Windows.Forms
Add-Type -AssemblyName System.Drawing
$f = New-Object Windows.Forms.Form
$f.Text = 'R6K-CLIP-TEST'; $f.Width = 800; $f.Height = 450
$f.StartPosition = 'Manual'; $f.Left = 120; $f.Top = 120
$l = New-Object Windows.Forms.Label
$l.Dock = 'Fill'; $l.TextAlign = 'MiddleCenter'
$l.Font = New-Object Drawing.Font('Consolas', 24)
$f.Controls.Add($l)
if ($Wav) { $player = New-Object Media.SoundPlayer $Wav; $player.PlayLooping() }
# Warm up the clock call (the first call is slow), then pair the stopwatch with the wall clock
# at the first tick, so that "elapsed" and "wall" cannot disagree by the start-up time.
$null = [DateTimeOffset]::UtcNow.ToUnixTimeMilliseconds()
$sw = [Diagnostics.Stopwatch]::StartNew()
$script:written = $false
$update = {
  $ms = $sw.ElapsedMilliseconds
  if (-not $script:written) {
    $script:written = $true
    $wall = [DateTimeOffset]::UtcNow.ToUnixTimeMilliseconds()
    [IO.File]::WriteAllText($T0File, [string]($wall - $sw.ElapsedMilliseconds))
  }
  if ($Freeze -gt 0 -and $ms -ge $MinAt * 1000 -and $ms -lt ($MinAt + $MinFor) * 1000) { return }
  $l.Text = ('{0:N2}' -f ($ms / 1000))
  # The colour is the wall clock itself (Unix ms): no start time to get wrong.
  $w = [DateTimeOffset]::UtcNow.ToUnixTimeMilliseconds()
  $sec = [math]::Floor($w / 1000)
  $l.BackColor = [Drawing.Color]::FromArgb(255, [int](($sec % 8) * 32), [int]((($w % 1000) * 255) / 1000), 90)
  if ($MinAt -gt 0) {
    if ($ms -ge $MinAt * 1000 -and $ms -lt ($MinAt + $MinFor) * 1000) {
      if ($f.WindowState -ne 'Minimized') { $f.WindowState = 'Minimized' }
    } elseif ($ms -ge ($MinAt + $MinFor) * 1000 -and $f.WindowState -eq 'Minimized') {
      $f.WindowState = 'Normal'
    }
  }
}
if ($env:R6K_FAST) {
  # Repaint as fast as the desktop lets us (more than 60 frames per second reach the capture).
  $f.Show()
  # Optional slow phase (seconds SlowFrom..SlowTo): one repaint every SlowMs, like a game whose
  # frame rate dropped after alt-tab. The window stays visible.
  while ($f.Visible) {
    if ($SlowTo -gt 0) {
      $e = $sw.ElapsedMilliseconds
      if ($e -ge $SlowFrom * 1000 -and $e -lt $SlowTo * 1000) { Start-Sleep -Milliseconds $SlowMs }
    }
    & $update; [Windows.Forms.Application]::DoEvents()
  }
} else {
  $t = New-Object Windows.Forms.Timer
  $t.Interval = 10
  $t.Add_Tick($update)
  $t.Start()
  [Windows.Forms.Application]::Run($f)
}
"#,
        )
        .unwrap();
        let t0_file = dir.join("t0.txt");
        let _ = std::fs::remove_file(&t0_file);
        let mut cmd = Command::new("powershell");
        cmd.args(["-NoProfile", "-ExecutionPolicy", "Bypass", "-File"])
            .arg(&script)
            .arg("-T0File")
            .arg(&t0_file)
            .args(["-MinAt", &minimize_at_s.to_string(), "-MinFor", &minimize_for_s.to_string(), "-Freeze", if std::env::var("R6K_FREEZE").is_ok() { "1" } else { "0" }]);
        if let Some((from, to, ms)) = slow {
            cmd.env("R6K_FAST", "1").args(["-SlowFrom", &from.to_string(), "-SlowTo", &to.to_string(), "-SlowMs", &ms.to_string()]);
        }
        if with_sound {
            let wav = dir.join("tone.wav");
            write_tone_wav(&wav);
            cmd.arg("-Wav").arg(&wav);
        }
        let child = cmd
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .expect("start the test window");
        for _ in 0..100 {
            if let Ok(s) = std::fs::read_to_string(&t0_file) {
                if let Ok(t0) = s.trim().parse::<i64>() {
                    return (child, t0);
                }
            }
            std::thread::sleep(Duration::from_millis(100));
        }
        panic!("the test window did not start");
    }

    /// Records the test window while it minimizes itself (seconds 8..14) and prints how the
    /// segments line up with the wall clock. `cargo test -p r6koc-app clips::recorder::tests::minimize_gap -- --ignored --nocapture`.
    #[test]
    #[ignore]
    fn minimize_gap() {
        let tmp = tempfile::tempdir().unwrap();
        let (mut child, t0) = spawn_test_window(tmp.path(), 8, 6, false);
        let settings = Arc::new(Mutex::new(ClipSettings { enabled: true, ..Default::default() }));
        let buffer_dir = tmp.path().join("buffer");
        let rec = Recorder::start(
            buffer_dir.clone(),
            settings,
            Box::new(|w| w.title == "R6K-CLIP-TEST"),
            RecorderConfig { segment_secs: 60, poll: Duration::from_millis(500), fps: std::env::var("R6K_FPS").ok().and_then(|v| v.parse().ok()).unwrap_or(CLIP_FPS) },
            Arc::new(|e| println!("event: {e:?}")),
        );
        std::thread::sleep(Duration::from_secs(26));
        rec.flush();
        let segs = rec.with_buffer(|d| buffer::list_segments(d));
        for s in &segs {
            let (frames, video_ms) = crate::clips::cutter::probe(&s.path).unwrap();
            println!(
                "segment wall {:.2}..{:.2} s ({:.2} s), video {} frames / {:.2} s",
                (s.start_ms - t0) as f64 / 1000.0,
                (s.end_ms - t0) as f64 / 1000.0,
                (s.end_ms - s.start_ms) as f64 / 1000.0,
                frames,
                video_ms as f64 / 1000.0
            );
        }
        rec.shutdown();
        let _ = child.kill();
        let _ = child.wait();
    }

    fn find_ffmpeg() -> Option<PathBuf> {
        if let Ok(p) = std::env::var("R6K_FFMPEG") {
            return Some(PathBuf::from(p));
        }
        let pk = PathBuf::from(std::env::var("LOCALAPPDATA").ok()?).join("Microsoft").join("WinGet").join("Packages");
        for e in std::fs::read_dir(pk).ok()?.flatten() {
            if !e.file_name().to_string_lossy().starts_with("Gyan.FFmpeg") {
                continue;
            }
            for b in std::fs::read_dir(e.path()).ok()?.flatten() {
                let exe = b.path().join("bin").join("ffmpeg.exe");
                if exe.exists() {
                    return Some(exe);
                }
            }
        }
        None
    }

    /// Wall time (Unix ms) that a test-window colour shows, resolved against the estimate `est`
    /// (red = whole seconds mod 8 in steps of 32, green = the fraction of the second).
    pub(crate) fn wall_from_colour(r: u8, g: u8, est: i64) -> i64 {
        let sec = ((f64::from(r) / 32.0).round() as i64).rem_euclid(8);
        let frac = (f64::from(g) * 1000.0 / 255.0).round() as i64;
        let in_cycle = sec * 1000 + frac;
        let base = (est - in_cycle).div_euclid(8000) * 8000;
        [base - 8000, base, base + 8000, base + 16_000].iter().map(|b| b + in_cycle).min_by_key(|w| (w - est).abs()).unwrap_or(est)
    }

    /// Decodes a clip with ffmpeg: `(pts seconds, shown wall time)` per frame. `file_wall` is the
    /// wall time of the file's time 0. Three patches of the label background are read; a frame
    /// counts only when two of them agree (a single patch now and then shows a stray colour).
    fn decode_clip(ffmpeg: &std::path::Path, clip: &std::path::Path, file_wall: i64) -> Vec<(f64, i64)> {
        use std::process::{Command, Stdio};
        let mut per_patch: Vec<Vec<(f64, i64)>> = Vec::new();
        for (x, y) in [(24, 80), (40, 380), (760, 200)] {
            let out = Command::new(ffmpeg)
                .args(["-v", "info", "-nostats", "-i"])
                .arg(clip)
                .args(["-vf", &format!("crop=8:8:{x}:{y},scale=1:1:flags=area,showinfo"), "-fps_mode", "passthrough", "-an", "-f", "rawvideo", "-pix_fmt", "rgb24", "-"])
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .output()
                .expect("run ffmpeg");
            let log = String::from_utf8_lossy(&out.stderr);
            let pts: Vec<f64> = log
                .lines()
                .filter(|l| l.contains("Parsed_showinfo"))
                .filter_map(|l| l.split("pts_time:").nth(1)?.split_whitespace().next()?.parse().ok())
                .collect();
            if std::env::var("R6K_DRIFT_DEBUG").is_ok() {
                println!("  patch ({x},{y}): {:?}", out.stdout.chunks_exact(3).take(20).map(|p| (p[0], p[1], p[2])).collect::<Vec<_>>());
            }
            per_patch.push(
                out.stdout
                    .chunks_exact(3)
                    .zip(pts)
                    // Blue is always about 90; the form sometimes flashes its grey background (240).
                    .filter(|(px, _)| (60..=120).contains(&px[2]))
                    .map(|(px, t)| (t, wall_from_colour(px[0], px[1], file_wall + (t * 1000.0) as i64)))
                    .collect(),
            );
        }
        let mut out = Vec::new();
        for &(t, w0) in &per_patch[0] {
            let at = |p: usize| per_patch[p].iter().find(|(t2, _)| (t2 - t).abs() < 1e-6).map(|x| x.1);
            let agree = |a: i64, b: Option<i64>| b.is_some_and(|b| (a - b).abs() <= 60);
            if agree(w0, at(1)) || agree(w0, at(2)) {
                out.push((t, w0));
            }
        }
        out
    }

    /// Records the test window with a slow phase (about 12 fps from second 8 to 16, as after an
    /// alt-tab) and checks where clips cut before, inside and after it put the content on the
    /// wall clock: error = (file wall + frame pts) - the wall time the frame shows.
    /// `cargo test -p r6koc-app clips::recorder::tests::fps_drop_drift -- --ignored --nocapture`
    /// (`R6K_FFMPEG` = path of ffmpeg.exe; found in the WinGet folder otherwise).
    #[test]
    #[ignore]
    fn fps_drop_drift() {
        let Some(ffmpeg) = find_ffmpeg() else {
            println!("SKIPPED: ffmpeg not found (set R6K_FFMPEG)");
            return;
        };
        let tmp = tempfile::tempdir().unwrap();
        let out_dir = std::env::var("R6K_CLIP_E2E_OUT").map(PathBuf::from).unwrap_or_else(|_| tmp.path().join("out"));
        std::fs::create_dir_all(&out_dir).unwrap();
        // `R6K_DRIFT_SOUND=1`: the window also plays a tone; each clip is checked for gaps in it.
        let sound = std::env::var("R6K_DRIFT_SOUND").is_ok();
        let (mut child, t0) = spawn_test_window_slow(tmp.path(), 0, 0, sound, Some((8, 16, 80)));
        let settings = Arc::new(Mutex::new(ClipSettings { enabled: true, record_audio: sound, ..Default::default() }));
        let rec = Recorder::start(
            tmp.path().join("buffer"),
            settings,
            Box::new(|w| w.title == "R6K-CLIP-TEST"),
            RecorderConfig { segment_secs: 60, poll: Duration::from_millis(500), fps: CLIP_FPS },
            Arc::new(|e| println!("event: {e:?}")),
        );
        std::thread::sleep(Duration::from_millis((t0 + 31_000 - now_ms()).max(1000) as u64));
        rec.flush();
        let segs = rec.with_buffer(|d| buffer::list_segments(d));
        for s in &segs {
            let (frames, video_ms) = crate::clips::cutter::probe(&s.path).unwrap();
            println!("segment wall {:.2}..{:.2} s, video {} frames / {:.2} s", (s.start_ms - t0) as f64 / 1000.0, (s.end_ms - t0) as f64 / 1000.0, frames, video_ms as f64 / 1000.0);
        }
        let mut worst = 0.0f64;
        for at in [5i64, 12, 20, 26] {
            let (a, b) = (t0 + (at - 1) * 1000, t0 + (at + 1) * 1000);
            let sel = rec.with_buffer(|_| buffer::select_for_window(&segs, a, b));
            let clip = out_dir.join(format!("drift-{at}.mp4"));
            let cut = match crate::clips::cutter::cut(&sel, a, b, (a + b) / 2, &clip) {
                Ok(c) => c,
                Err(e) => {
                    println!("clip around +{at} s: no clip ({e})");
                    continue;
                }
            };
            let file_wall = a - cut.lead_ms;
            let frames = decode_clip(&ffmpeg, &clip, file_wall);
            if std::env::var("R6K_DRIFT_DEBUG").is_ok() {
                for (i, (t, shown)) in frames.iter().enumerate() {
                    println!("  #{i} pts {t:.3} s shown wall +{:.3} s err {:+.0} ms", (*shown - t0) as f64 / 1000.0, (file_wall as f64 + t * 1000.0) - *shown as f64);
                }
            }
            let mut err: Vec<f64> = frames.iter().map(|(t, shown)| (file_wall as f64 + t * 1000.0) - *shown as f64).collect();
            err.sort_by(|x, y| x.partial_cmp(y).unwrap());
            let mut abs: Vec<f64> = err.iter().map(|e| e.abs()).collect();
            abs.sort_by(|x, y| x.partial_cmp(y).unwrap());
            let med = |v: &[f64]| v.get(v.len() / 2).copied().unwrap_or(f64::NAN);
            println!(
                "clip around +{at:>2} s: {} frames, {} ms file, error median {:+.0} ms, |median| {:.0} ms, |p90| {:.0} ms, |max| {:.0} ms",
                err.len(),
                cut.duration_ms,
                med(&err),
                med(&abs),
                abs.get(abs.len() * 9 / 10).copied().unwrap_or(f64::NAN),
                abs.last().copied().unwrap_or(f64::NAN)
            );
            worst = worst.max(med(&abs));
            if sound {
                // The tone plays without a pause: silence in the clip means sound that is missing
                // or placed on the wrong time.
                let out = std::process::Command::new(&ffmpeg)
                    .args(["-v", "info", "-nostats", "-i"])
                    .arg(&clip)
                    .args(["-map", "0:a", "-af", "silencedetect=n=-45dB:d=0.05", "-f", "null", "-"])
                    .output()
                    .expect("run ffmpeg");
                let log = String::from_utf8_lossy(&out.stderr);
                let silences: Vec<&str> = log.lines().filter(|l| l.contains("silence_start") || l.contains("silence_end")).collect();
                let audio_s = log.lines().filter_map(|l| l.split("time=").nth(1)).filter_map(|r| r.split_whitespace().next()).last().unwrap_or("none");
                println!("    sound: track {} , audio length {audio_s}, silence events: {}", cut.audio, silences.len());
                for l in silences.iter().take(6) {
                    println!("      {}", l.trim());
                }
            }
        }
        println!("worst |median| error: {worst:.0} ms");
        rec.shutdown();
        let _ = child.kill();
        let _ = child.wait();
    }

    /// Records a real window for ~20 s and cuts clips out of the buffer. Needs a desktop session:
    /// `cargo test -p r6koc-app clips::recorder::tests::records_a_test_window -- --ignored --nocapture`.
    /// Set `R6K_CLIP_E2E_OUT` to keep the clips in a folder you can open.
    #[test]
    #[ignore]
    fn records_a_test_window() {
        use std::process::{Command, Stdio};
        let tmp = tempfile::tempdir().unwrap();
        let out_dir = std::env::var("R6K_CLIP_E2E_OUT").map(PathBuf::from).unwrap_or_else(|_| tmp.path().join("out"));
        std::fs::create_dir_all(&out_dir).unwrap();
        let script = tmp.path().join("win.ps1");
        std::fs::write(
            &script,
            r#"Add-Type -AssemblyName System.Windows.Forms
Add-Type -AssemblyName System.Drawing
$f = New-Object Windows.Forms.Form
$f.Text = 'R6K-CLIP-TEST'; $f.Width = 800; $f.Height = 450
$f.StartPosition = 'Manual'; $f.Left = 120; $f.Top = 120
$l = New-Object Windows.Forms.Label
$l.Dock = 'Fill'; $l.TextAlign = 'MiddleCenter'
$l.Font = New-Object Drawing.Font('Consolas', 72)
$f.Controls.Add($l)
$sw = [Diagnostics.Stopwatch]::StartNew()
$t = New-Object Windows.Forms.Timer
$t.Interval = 16
$t.Add_Tick({
  $l.Text = ('{0:N2}' -f $sw.Elapsed.TotalSeconds)
  $c = [int](($sw.ElapsedMilliseconds / 8) % 200) + 40
  $l.BackColor = [Drawing.Color]::FromArgb(255, $c, 80, 200 - $c / 2)
})
$t.Start()
[Windows.Forms.Application]::Run($f)
"#,
        )
        .unwrap();
        let mut child = Command::new("powershell")
            .args(["-NoProfile", "-ExecutionPolicy", "Bypass", "-File"])
            .arg(&script)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .expect("start the test window");
        std::thread::sleep(Duration::from_secs(4));

        let settings = Arc::new(Mutex::new(ClipSettings { enabled: true, ..Default::default() }));
        let buffer_dir = tmp.path().join("buffer");
        let rec = Recorder::start(
            buffer_dir.clone(),
            settings,
            Box::new(|w| w.title == "R6K-CLIP-TEST"),
            // Short segments so that the cut crosses a segment seam.
            RecorderConfig { segment_secs: 8, poll: Duration::from_millis(500), fps: CLIP_FPS },
            Arc::new(|e| println!("event: {e:?}")),
        );
        let t0 = now_ms();
        std::thread::sleep(Duration::from_secs(22));
        let status = rec.status();
        println!("status after 22 s: {status:?}");
        assert_eq!(status.state, RecState::Recording, "the test window was not captured");
        rec.flush();
        let segs = rec.with_buffer(|d| buffer::list_segments(d));
        println!("segments: {:?}", segs.iter().map(|s| (s.start_ms - t0, s.end_ms - t0)).collect::<Vec<_>>());
        assert!(segs.len() >= 2, "expected at least two finished segments");

        // Cut 6 s before / 2 s after the moment 14 s into the recording (crosses a seam).
        let (a, b) = (t0 + 8_000, t0 + 16_000);
        let cover = rec.with_buffer(|_| buffer::coverage_ms(&segs, a, b));
        println!("coverage of the window: {cover} ms");
        let sel = buffer::select_for_window(&segs, a, b);
        let out = out_dir.join("e2e-clip.mp4");
        let cut = crate::clips::cutter::cut(&sel, a, b, (a + b) / 2, &out).expect("cut");
        println!("cut: {cut:?}, file {} bytes: {}", std::fs::metadata(&out).unwrap().len(), out.display());
        // Keep one raw segment too for comparison.
        let _ = std::fs::copy(&segs[0].path, out_dir.join("e2e-segment0.mp4"));
        rec.shutdown();
        let _ = child.kill();
        let _ = child.wait();
        assert!(cut.duration_ms > 6_000);
    }
}

#[cfg(test)]
mod limiter_tests {
    use super::SlotLimiter;

    const MS: i64 = 10_000;

    /// Offers frames every `step_ticks` from `from` to `to` ticks; returns the sent `(time, n)`.
    fn run(l: &mut SlotLimiter, from: i64, to: i64, step_ticks: i64) -> Vec<(i64, u64)> {
        let mut out = Vec::new();
        let mut t = from;
        while t < to {
            if let Some(n) = l.offer(t) {
                out.push((t, n));
            }
            t += step_ticks;
        }
        out
    }

    #[test]
    fn a_fast_stream_is_cut_down_to_the_clip_rate() {
        let mut l = SlotLimiter::new(30);
        // 144 fps for 2 s: the sent count is about 30 per second, spaced about 33 ms.
        let sent = run(&mut l, 0, 2_000 * MS, 10_000_000 / 144);
        assert!((59..=61).contains(&sent.len()), "{}", sent.len());
        for w in sent.windows(2) {
            let d = (w[1].0 - w[0].0) / MS;
            assert!((27..=41).contains(&d), "gap {d} ms");
        }
        // Numbered without holes.
        assert!(sent.iter().enumerate().all(|(i, &(_, n))| n == i as u64));
    }

    #[test]
    fn a_sixty_fps_stream_keeps_every_second_frame() {
        let mut l = SlotLimiter::new(30);
        let sent = run(&mut l, 0, 1_000 * MS, 10_000_000 / 60);
        assert!((29..=31).contains(&sent.len()), "{}", sent.len());
    }

    #[test]
    fn a_slow_stream_sends_every_frame_and_recovers() {
        let mut l = SlotLimiter::new(30);
        let mut sent = run(&mut l, 0, 1_000 * MS, 10_000_000 / 60);
        let before = sent.len();
        // 12 fps for 3 s: every frame goes through (the first one may land a hair before its slot).
        let slow = run(&mut l, 1_000 * MS, 4_000 * MS, 83 * MS);
        assert!((36..=37).contains(&slow.len()), "{}", slow.len());
        assert!(slow.windows(2).all(|w| w[1].0 - w[0].0 == 83 * MS));
        let slow_len = slow.len();
        sent.extend(slow);
        // Fast again: back to about 30 per second.
        let fast = run(&mut l, 4_000 * MS, 5_000 * MS, 10_000_000 / 60);
        assert!((28..=31).contains(&fast.len()), "{}", fast.len());
        assert_eq!(fast[0].1, (before + slow_len) as u64);
    }

    #[test]
    fn stamps_are_a_constant_rate_stream() {
        let l = SlotLimiter::new(30);
        assert_eq!(l.stamp(0), 0);
        assert_eq!(l.stamp(30), 10_000_000);
        assert_eq!(l.stamp(3), 1_000_000);
        // No rounding drift over an hour of frames.
        assert_eq!(l.stamp(30 * 3600), 3600 * 10_000_000);
    }
}
