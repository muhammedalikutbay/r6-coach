//! The game's sound for the clips.
//!
//! Windows 10 (build 19041, version 2004) and newer can capture the sound of one process tree
//! ("process loopback", `ActivateAudioInterfaceAsync` with
//! `AUDIOCLIENT_ACTIVATION_TYPE_PROCESS_LOOPBACK`): only the game is recorded, not Discord, music or
//! browser tabs. Where that is not available the sound of the default playback device is recorded
//! instead (system loopback).
//!
//! The capture thread only fills a queue of 48 kHz / 16 bit / stereo PCM. The video capture callback
//! drains it into the encoder (`send_audio_buffer`) frame by frame and pads the gaps with silence,
//! so the sound stays glued to the video timeline whatever the game does (`plan_block`).

use std::collections::VecDeque;
use std::sync::Mutex;

use serde::Serialize;

pub const SAMPLE_RATE: u32 = 48_000;
pub const CHANNELS: u32 = 2;
/// 16 bit stereo.
pub const BYTES_PER_FRAME: usize = 4;
/// What the queue keeps when nobody drains it.
const MAX_QUEUE_FRAMES: usize = SAMPLE_RATE as usize * 5;
/// Silence is only inserted when the sound is more than this far behind the video (20 ms).
const PAD_SLACK_FRAMES: u64 = 960;
/// Sound that runs ahead of the video by more than this (0.5 s) is trimmed.
const MAX_AHEAD_FRAMES: u64 = 24_000;
/// One block of silence never exceeds this (10 s): a longer stall starts a new segment anyway.
const MAX_PAD_FRAMES: u64 = SAMPLE_RATE as u64 * 10;

/// Where the sound of the current recording comes from.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub enum AudioMode {
    /// Not recorded (switched off, or nothing is being recorded).
    #[default]
    Off,
    /// The game process only.
    Game,
    /// Everything the PC plays (this Windows has no process loopback).
    System,
    /// Sound was wanted but could not be started; the clips are silent.
    Failed,
}

/// PCM bytes captured but not yet encoded.
#[derive(Default)]
pub struct PcmQueue {
    inner: Mutex<VecDeque<u8>>,
}

impl PcmQueue {
    pub fn push(&self, bytes: &[u8]) {
        if let Ok(mut q) = self.inner.lock() {
            q.extend(bytes.iter().copied());
            let cap = MAX_QUEUE_FRAMES * BYTES_PER_FRAME;
            if q.len() > cap {
                let excess = q.len() - cap;
                q.drain(..excess);
            }
        }
    }

    /// Everything queued so far (whole frames only).
    pub fn take_all(&self) -> Vec<u8> {
        let Ok(mut q) = self.inner.lock() else { return Vec::new() };
        let whole = q.len() - q.len() % BYTES_PER_FRAME;
        q.drain(..whole).collect()
    }

    pub fn clear(&self) {
        if let Ok(mut q) = self.inner.lock() {
            q.clear();
        }
    }
}

/// How to hand a block of captured sound to the encoder so that it matches the video clock.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BlockPlan {
    /// Frames of silence to put in front of the block.
    pub pad_frames: u64,
    /// Frames to drop from the front of the block (the sound runs ahead of the video).
    pub drop_frames: u64,
}

/// `sent`: sound frames already given to the encoder in this segment. `incoming`: frames in the
/// block. `target`: how many frames the video clock says should exist by now.
pub fn plan_block(sent: u64, incoming: u64, target: u64) -> BlockPlan {
    let have = sent + incoming;
    if have + PAD_SLACK_FRAMES < target {
        return BlockPlan { pad_frames: (target - have).min(MAX_PAD_FRAMES), drop_frames: 0 };
    }
    if have > target + MAX_AHEAD_FRAMES {
        return BlockPlan { pad_frames: 0, drop_frames: (have - target - MAX_AHEAD_FRAMES).min(incoming) };
    }
    BlockPlan { pad_frames: 0, drop_frames: 0 }
}

/// Sound frames that belong to `elapsed_100ns` on the video clock.
pub fn frames_for(elapsed_100ns: i64) -> u64 {
    (elapsed_100ns.max(0) as i128 * i128::from(SAMPLE_RATE) / 10_000_000) as u64
}

pub use platform::AudioCapture;

#[cfg(windows)]
mod platform {
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::{mpsc, Arc, Condvar, Mutex};
    use std::thread::JoinHandle;
    use std::time::Duration;

    use windows::core::{implement, Interface, Ref, HRESULT};
    use windows::Win32::Foundation::{CloseHandle, HANDLE};
    use windows::Win32::Media::Audio::{
        eConsole, eRender, ActivateAudioInterfaceAsync, IActivateAudioInterfaceAsyncOperation, IActivateAudioInterfaceCompletionHandler,
        IActivateAudioInterfaceCompletionHandler_Impl, IAudioCaptureClient, IAudioClient, IMMDeviceEnumerator, MMDeviceEnumerator,
        AUDCLNT_SHAREMODE_SHARED, AUDCLNT_STREAMFLAGS_AUTOCONVERTPCM, AUDCLNT_STREAMFLAGS_EVENTCALLBACK, AUDCLNT_STREAMFLAGS_LOOPBACK,
        AUDCLNT_STREAMFLAGS_SRC_DEFAULT_QUALITY, AUDIOCLIENT_ACTIVATION_PARAMS, AUDIOCLIENT_ACTIVATION_PARAMS_0,
        AUDIOCLIENT_ACTIVATION_TYPE_PROCESS_LOOPBACK, AUDIOCLIENT_PROCESS_LOOPBACK_PARAMS, PROCESS_LOOPBACK_MODE_INCLUDE_TARGET_PROCESS_TREE,
        VIRTUAL_AUDIO_DEVICE_PROCESS_LOOPBACK, WAVEFORMATEX,
    };
    use windows::Win32::System::Com::StructuredStorage::PROPVARIANT;
    use windows::Win32::System::Com::{CoCreateInstance, CoInitializeEx, CoUninitialize, CLSCTX_ALL, COINIT_MULTITHREADED};
    use windows::Win32::System::Threading::{CreateEventW, WaitForSingleObject};
    use windows::Win32::System::Variant::VT_BLOB;

    use super::{AudioMode, PcmQueue, BYTES_PER_FRAME, CHANNELS, SAMPLE_RATE};

    /// A running sound capture. Dropping it stops the thread.
    pub struct AudioCapture {
        stop: Arc<AtomicBool>,
        thread: Option<JoinHandle<()>>,
        mode: AudioMode,
    }

    impl AudioCapture {
        /// Starts capturing the sound of process `pid` (and its children) into `queue`; falls back
        /// to the default playback device when process loopback is not available.
        pub fn start(pid: u32, queue: Arc<PcmQueue>) -> Result<AudioCapture, String> {
            let stop = Arc::new(AtomicBool::new(false));
            let (tx, rx) = mpsc::channel::<Result<AudioMode, String>>();
            let (stop2, queue2) = (stop.clone(), queue);
            let thread = std::thread::Builder::new()
                .name("clip-audio".into())
                .spawn(move || run(pid, queue2, stop2, tx))
                .map_err(|e| format!("cannot start the sound thread: {e}"))?;
            match rx.recv_timeout(Duration::from_secs(10)) {
                Ok(Ok(mode)) => Ok(AudioCapture { stop, thread: Some(thread), mode }),
                Ok(Err(e)) => {
                    let _ = thread.join();
                    Err(e)
                }
                Err(_) => {
                    stop.store(true, Ordering::SeqCst);
                    Err("the sound capture did not start in time".into())
                }
            }
        }

        pub fn mode(&self) -> AudioMode {
            self.mode
        }
    }

    impl Drop for AudioCapture {
        fn drop(&mut self) {
            self.stop.store(true, Ordering::SeqCst);
            if let Some(t) = self.thread.take() {
                let _ = t.join();
            }
        }
    }

    /// Flags the activation as done; `ActivateAudioInterfaceAsync` calls this on a worker thread.
    #[implement(IActivateAudioInterfaceCompletionHandler)]
    struct Done(Arc<(Mutex<bool>, Condvar)>);

    impl IActivateAudioInterfaceCompletionHandler_Impl for Done_Impl {
        fn ActivateCompleted(&self, _operation: Ref<'_, IActivateAudioInterfaceAsyncOperation>) -> windows::core::Result<()> {
            let (m, cv) = &*self.0;
            if let Ok(mut done) = m.lock() {
                *done = true;
            }
            cv.notify_all();
            Ok(())
        }
    }

    fn format() -> WAVEFORMATEX {
        WAVEFORMATEX {
            wFormatTag: 1, // WAVE_FORMAT_PCM
            nChannels: CHANNELS as u16,
            nSamplesPerSec: SAMPLE_RATE,
            nAvgBytesPerSec: SAMPLE_RATE * BYTES_PER_FRAME as u32,
            nBlockAlign: BYTES_PER_FRAME as u16,
            wBitsPerSample: 16,
            cbSize: 0,
        }
    }

    /// The audio client that records only `pid` and its children (Windows 10 2004+).
    fn process_client(pid: u32) -> Result<IAudioClient, String> {
        let mut params = AUDIOCLIENT_ACTIVATION_PARAMS {
            ActivationType: AUDIOCLIENT_ACTIVATION_TYPE_PROCESS_LOOPBACK,
            Anonymous: AUDIOCLIENT_ACTIVATION_PARAMS_0 {
                ProcessLoopbackParams: AUDIOCLIENT_PROCESS_LOOPBACK_PARAMS {
                    TargetProcessId: pid,
                    ProcessLoopbackMode: PROCESS_LOOPBACK_MODE_INCLUDE_TARGET_PROCESS_TREE,
                },
            },
        };
        // The PROPVARIANT only points at `params`; it must never free it.
        let mut pv = std::mem::ManuallyDrop::new(PROPVARIANT::default());
        unsafe {
            let inner = &mut pv.Anonymous.Anonymous;
            inner.vt = VT_BLOB;
            inner.Anonymous.blob.cbSize = std::mem::size_of::<AUDIOCLIENT_ACTIVATION_PARAMS>() as u32;
            inner.Anonymous.blob.pBlobData = (&mut params as *mut AUDIOCLIENT_ACTIVATION_PARAMS).cast::<u8>();
        }
        let signal = Arc::new((Mutex::new(false), Condvar::new()));
        let handler: IActivateAudioInterfaceCompletionHandler = Done(signal.clone()).into();
        let op = unsafe { ActivateAudioInterfaceAsync(VIRTUAL_AUDIO_DEVICE_PROCESS_LOOPBACK, &IAudioClient::IID, Some(&*pv as *const PROPVARIANT), &handler) }
            .map_err(|e| format!("process loopback is not available: {e}"))?;
        {
            let (m, cv) = &*signal;
            let guard = m.lock().map_err(|_| "lock poisoned")?;
            let (done, _) = cv.wait_timeout_while(guard, Duration::from_secs(5), |d| !*d).map_err(|_| "lock poisoned")?;
            if !*done {
                return Err("process loopback did not answer".into());
            }
        }
        let mut hr = HRESULT(0);
        let mut unknown: Option<windows::core::IUnknown> = None;
        unsafe { op.GetActivateResult(&mut hr, &mut unknown) }.map_err(|e| format!("process loopback failed: {e}"))?;
        hr.ok().map_err(|e| format!("process loopback failed: {e}"))?;
        unknown.ok_or("process loopback returned nothing")?.cast::<IAudioClient>().map_err(|e| e.to_string())
    }

    /// The audio client of the default playback device (records everything that is played).
    fn system_client() -> Result<IAudioClient, String> {
        unsafe {
            let enumerator: IMMDeviceEnumerator = CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL).map_err(|e| e.to_string())?;
            let device = enumerator.GetDefaultAudioEndpoint(eRender, eConsole).map_err(|e| format!("no playback device: {e}"))?;
            device.Activate::<IAudioClient>(CLSCTX_ALL, None).map_err(|e| e.to_string())
        }
    }

    struct Session {
        client: IAudioClient,
        capture: IAudioCaptureClient,
        event: HANDLE,
    }

    fn open(client: IAudioClient, process: bool) -> Result<Session, String> {
        let fmt = format();
        // System loopback may need the engine to convert the mix format; a process loopback
        // client already delivers the requested format.
        let flags = AUDCLNT_STREAMFLAGS_LOOPBACK
            | AUDCLNT_STREAMFLAGS_EVENTCALLBACK
            | if process { 0 } else { AUDCLNT_STREAMFLAGS_AUTOCONVERTPCM | AUDCLNT_STREAMFLAGS_SRC_DEFAULT_QUALITY };
        unsafe {
            client.Initialize(AUDCLNT_SHAREMODE_SHARED, flags, 2_000_000, 0, &fmt, None).map_err(|e| format!("cannot set up the sound capture: {e}"))?;
            let event = CreateEventW(None, false, false, None).map_err(|e| e.to_string())?;
            let setup = (|| -> windows::core::Result<IAudioCaptureClient> {
                client.SetEventHandle(event)?;
                let capture: IAudioCaptureClient = client.GetService()?;
                client.Start()?;
                Ok(capture)
            })();
            match setup {
                Ok(capture) => Ok(Session { client, capture, event }),
                Err(e) => {
                    let _ = CloseHandle(event);
                    Err(format!("cannot start the sound capture: {e}"))
                }
            }
        }
    }

    fn run(pid: u32, queue: Arc<PcmQueue>, stop: Arc<AtomicBool>, ready: mpsc::Sender<Result<AudioMode, String>>) {
        unsafe {
            let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
        }
        let opened = process_client(pid).and_then(|c| open(c, true)).map(|s| (s, AudioMode::Game)).or_else(|e| {
            log::info!("clips: {e}; recording the sound of the default playback device instead");
            system_client().and_then(|c| open(c, false)).map(|s| (s, AudioMode::System))
        });
        let (session, mode) = match opened {
            Ok(x) => x,
            Err(e) => {
                let _ = ready.send(Err(e));
                unsafe { CoUninitialize() };
                return;
            }
        };
        log::info!("clips: recording the sound ({mode:?})");
        let _ = ready.send(Ok(mode));
        while !stop.load(Ordering::SeqCst) {
            unsafe {
                // The event is not signalled while nothing plays; the timeout keeps the loop alive.
                WaitForSingleObject(session.event, 100);
                if drain(&session.capture, &queue).is_err() {
                    log::warn!("clips: the sound capture stopped");
                    break;
                }
            }
        }
        unsafe {
            let _ = session.client.Stop();
            let _ = CloseHandle(session.event);
            CoUninitialize();
        }
    }

    /// Moves every available packet into the queue.
    unsafe fn drain(capture: &IAudioCaptureClient, queue: &PcmQueue) -> windows::core::Result<()> {
        loop {
            let mut next = unsafe { capture.GetNextPacketSize()? };
            if next == 0 {
                return Ok(());
            }
            while next > 0 {
                let (mut data, mut frames, mut flags) = (std::ptr::null_mut::<u8>(), 0u32, 0u32);
                unsafe { capture.GetBuffer(&mut data, &mut frames, &mut flags, None, None)? };
                let bytes = frames as usize * BYTES_PER_FRAME;
                // AUDCLNT_BUFFERFLAGS_SILENT: the buffer content is to be treated as silence.
                if flags & 0x2 != 0 || data.is_null() {
                    queue.push(&vec![0u8; bytes]);
                } else {
                    queue.push(unsafe { std::slice::from_raw_parts(data, bytes) });
                }
                unsafe { capture.ReleaseBuffer(frames)? };
                next = unsafe { capture.GetNextPacketSize()? };
            }
        }
    }
}

#[cfg(not(windows))]
mod platform {
    use std::sync::Arc;

    use super::{AudioMode, PcmQueue};

    pub struct AudioCapture;

    impl AudioCapture {
        pub fn start(_pid: u32, _queue: Arc<PcmQueue>) -> Result<AudioCapture, String> {
            Err("Sound capture is only available on Windows.".into())
        }
        pub fn mode(&self) -> AudioMode {
            AudioMode::Off
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn silence_fills_what_the_game_did_not_play() {
        // 1 s into the video, nothing captured yet: a second of silence.
        assert_eq!(plan_block(0, 0, 48_000), BlockPlan { pad_frames: 48_000, drop_frames: 0 });
        // Half a second captured, 1 s due: pad the other half.
        assert_eq!(plan_block(0, 24_000, 48_000), BlockPlan { pad_frames: 24_000, drop_frames: 0 });
        // A few ms behind is not worth a pad.
        assert_eq!(plan_block(47_000, 500, 48_000), BlockPlan { pad_frames: 0, drop_frames: 0 });
        // A long stall (video resumed after 30 s) is capped.
        assert_eq!(plan_block(0, 0, 30 * 48_000).pad_frames, 10 * 48_000);
    }

    #[test]
    fn sound_that_runs_ahead_is_trimmed() {
        assert_eq!(plan_block(48_000, 4_800, 50_000), BlockPlan { pad_frames: 0, drop_frames: 0 });
        // 2 s of backlog on top of 1 s: 1.5 s too much; never drop more than arrived.
        assert_eq!(plan_block(48_000, 96_000, 48_000), BlockPlan { pad_frames: 0, drop_frames: 96_000 - 24_000 });
        assert_eq!(plan_block(200_000, 100, 48_000).drop_frames, 100);
    }

    #[test]
    fn frames_follow_the_video_clock() {
        assert_eq!(frames_for(10_000_000), 48_000);
        assert_eq!(frames_for(5_000_000), 24_000);
        assert_eq!(frames_for(-5), 0);
    }

    #[test]
    fn the_queue_hands_out_whole_frames_and_caps_its_size() {
        let q = PcmQueue::default();
        q.push(&[1, 2, 3, 4, 5, 6]);
        assert_eq!(q.take_all(), vec![1, 2, 3, 4]);
        q.push(&[7, 8]);
        assert_eq!(q.take_all(), vec![5, 6, 7, 8]);
        q.push(&vec![9u8; MAX_QUEUE_FRAMES * BYTES_PER_FRAME + 400]);
        assert_eq!(q.take_all().len(), MAX_QUEUE_FRAMES * BYTES_PER_FRAME);
        q.push(&[1, 2, 3, 4]);
        q.clear();
        assert!(q.take_all().is_empty());
    }
}
