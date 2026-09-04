//! Dictation: the microphone into text, live.
//!
//! Three threads meet here, and the split is deliberate. The audio callback belongs to the OS and
//! must never block, so it does nothing but hand its buffer to a channel. A worker thread owns the
//! capture stream (`cpal::Stream` is not `Send` on macOS), the recognizer session and the
//! resampler, and it is the only thread that touches the model. The caller sees neither: it holds
//! a [`Dictation`] and reads events.
//!
//! PCM never crosses the IPC bridge. The UI asks for dictation and receives text.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, RecvTimeoutError, SyncSender, sync_channel};
use std::time::Duration;

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{FromSample, SampleFormat};
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender, unbounded_channel};
use transcribe_cpp::{CommitPolicy, StreamOptions};

use crate::audio::{LiveResampler, SAMPLE_RATE};
use crate::engine::Asr;
use crate::error::AsrError;

/// How many device buffers may queue before the audio callback starts dropping them. The callback
/// cannot block and cannot allocate, so the queue is bounded and overflow is counted, not waited on:
/// dropping 20 ms of audio beats stalling CoreAudio.
const QUEUE_DEPTH: usize = 64;

/// How often the worker reports the clock and the microphone level, in milliseconds. Fast enough that the
/// level meter follows a syllable, slow enough that it is a handful of small messages a second.
const TICK_MS: i64 = 100;

/// What the UI hears while dictation runs.
#[derive(Clone, Debug, PartialEq)]
pub enum DictationEvent {
    /// Capture started; the name is the device the user is actually being recorded through.
    Started { device: String, streaming: bool },
    /// The text moved. `committed` never shrinks; `tentative` is replaced on every tick.
    Text {
        committed: String,
        tentative: String,
        recorded_ms: i64,
    },
    /// The heartbeat of a running dictation: the clock, and how loud the microphone is right now.
    ///
    /// It is sent by both paths and does not wait for words. Text can stall for many reasons -- silence, a
    /// muted device, a model that only speaks at the end -- and a frozen clock beside a live microphone
    /// reads as a broken feature, so time and level are reported on their own schedule.
    Recording {
        recorded_ms: i64,
        /// Peak amplitude since the previous tick, `0.0` to `1.0`. Peak rather than RMS: it answers
        /// "did the microphone hear that" in one tick, which is what the meter is asked.
        level: f32,
    },
    /// The final transcript. Always the last event of a successful run.
    Finished { text: String },
    /// Dictation stopped because something broke. Also the last event.
    Failed { message: String },
}

/// Where a dictation's audio comes from.
///
/// The microphone, in the app. The other arm exists because the streaming path is otherwise only
/// reachable by a human speaking into a room: with it, the same worker -- the same resampler, the same
/// feed loop, the same commit/tentative plumbing -- can be driven from a recording and checked.
#[derive(Debug)]
pub enum Source {
    /// The default input device.
    Microphone,
    /// Audio already in memory, handed over in device-sized blocks as if a device had produced it.
    /// `rate` and `channels` describe the samples as given, so a 48 kHz stereo buffer exercises the
    /// downmix and the resampler exactly as a real device does.
    Pcm {
        /// Interleaved samples in [-1, 1].
        samples: Vec<f32>,
        rate: u32,
        channels: u16,
    },
}

/// The two buttons of a dictation, detached from its event stream. Separate because the code that
/// pumps events owns the [`Dictation`] and is busy awaiting it, while stop and cancel arrive from
/// somewhere else entirely -- a command handler, a closing window.
#[derive(Clone, Debug)]
pub struct DictationControl {
    stopping: Arc<AtomicBool>,
    cancelled: Arc<AtomicBool>,
}

impl DictationControl {
    /// Finish: flush what is buffered, finalize the stream, emit [`DictationEvent::Finished`].
    pub fn stop(&self) {
        self.stopping.store(true, Ordering::SeqCst);
    }

    /// Abandon: stop the microphone and emit nothing further. The text is thrown away, which is
    /// what a user pressing Escape means.
    pub fn cancel(&self) {
        self.cancelled.store(true, Ordering::SeqCst);
        self.stopping.store(true, Ordering::SeqCst);
    }
}

/// A dictation in flight. Dropping it stops the microphone and waits for the worker to let go of the
/// model.
#[derive(Debug)]
pub struct Dictation {
    control: DictationControl,
    events: UnboundedReceiver<DictationEvent>,
    /// Taken by `Drop`, which joins it. Without the join the worker can still be releasing its session
    /// while the process exits, and ggml's Metal backend aborts when its device is torn down with
    /// buffers still live -- a crash on quit, right after a dictation.
    worker: Option<std::thread::JoinHandle<()>>,
}

impl Dictation {
    /// A handle that can stop this dictation without holding it.
    pub fn control(&self) -> DictationControl {
        self.control.clone()
    }

    pub fn stop(&self) {
        self.control.stop();
    }

    pub fn cancel(&self) {
        self.control.cancel();
    }

    /// The next event, or `None` once the worker is done.
    pub async fn next(&mut self) -> Option<DictationEvent> {
        self.events.recv().await
    }
}

impl Drop for Dictation {
    fn drop(&mut self) {
        // A window closed mid-dictation must not leave the microphone recording.
        self.cancel();
        // Then wait. The worker notices the flag within one read timeout, so this is a beat at most, and
        // what it buys is that the session and the device are released before this returns.
        if let Some(worker) = self.worker.take()
            && worker.join().is_err()
        {
            tracing::warn!("luồng đọc chính tả kết thúc bất thường");
        }
    }
}

/// Open the source and start recognizing. Everything that can fail without a thread running -- no model,
/// no microphone, a device format we cannot read -- fails here, so the caller's error is the real one
/// rather than an event arriving later on a channel.
pub fn start(asr: &Asr, source: Source) -> Result<Dictation, AsrError> {
    let model = asr.model()?;
    let streaming = model.capabilities().supports_streaming;
    let run_options = asr.run_options();

    let (input, name, source_rate, channels) = match source {
        Source::Microphone => {
            let host = cpal::default_host();
            let device = host
                .default_input_device()
                .ok_or_else(|| AsrError::Unavailable("máy không có thiết bị thu âm nào".into()))?;
            let name = device.name().unwrap_or_else(|_| "micro".into());
            let supported = device.default_input_config().map_err(|error| {
                AsrError::Unavailable(format!("không đọc được cấu hình của `{name}`: {error}"))
            })?;
            let sample_format = supported.sample_format();
            let config: cpal::StreamConfig = supported.into();
            let rate = config.sample_rate.0;
            let channels = config.channels;
            (
                Input::Device {
                    device,
                    config,
                    sample_format,
                },
                name,
                rate,
                channels,
            )
        }
        Source::Pcm {
            samples,
            rate,
            channels,
        } => (
            Input::Pcm { samples },
            "bản ghi có sẵn".to_string(),
            rate,
            channels,
        ),
    };

    let stopping = Arc::new(AtomicBool::new(false));
    let cancelled = Arc::new(AtomicBool::new(false));
    let (events, receiver) = unbounded_channel();

    let worker = Worker {
        model,
        streaming,
        run_options,
        input,
        name,
        source_rate,
        channels,
        stopping: stopping.clone(),
        cancelled: cancelled.clone(),
        events,
    };
    // A plain OS thread, not a Tokio blocking task: this one runs for as long as the user holds the
    // button down, and a Tokio worker is the wrong thing to occupy for minutes.
    let handle = std::thread::Builder::new()
        .name("pai-asr-dictation".into())
        .spawn(move || worker.run())
        .map_err(|error| {
            AsrError::Unavailable(format!("không tạo được luồng đọc chính tả: {error}"))
        })?;

    Ok(Dictation {
        control: DictationControl {
            stopping,
            cancelled,
        },
        events: receiver,
        worker: Some(handle),
    })
}

/// The resolved source: a live device, or blocks waiting in memory.
enum Input {
    Device {
        device: cpal::Device,
        config: cpal::StreamConfig,
        sample_format: SampleFormat,
    },
    Pcm {
        samples: Vec<f32>,
    },
}

struct Worker {
    model: transcribe_cpp::Model,
    streaming: bool,
    run_options: transcribe_cpp::RunOptions,
    input: Input,
    name: String,
    source_rate: u32,
    channels: u16,
    stopping: Arc<AtomicBool>,
    cancelled: Arc<AtomicBool>,
    events: UnboundedSender<DictationEvent>,
}

impl Worker {
    fn run(self) {
        let events = self.events.clone();
        let cancelled = self.cancelled.clone();
        if let Err(error) = self.drive() {
            // A cancelled dictation is not a failure; the user asked for silence.
            if !cancelled.load(Ordering::SeqCst) {
                let _ = events.send(DictationEvent::Failed {
                    message: error.to_string(),
                });
            }
        }
    }

    fn drive(mut self) -> Result<(), AsrError> {
        let (sender, audio) = sync_channel::<Vec<f32>>(QUEUE_DEPTH);
        // Held to the end of the call: dropping a `cpal::Stream` stops the device, and dropping it early
        // would end the recording at the first sample.
        let stream = self.open(sender)?;
        self.emit(DictationEvent::Started {
            device: self.name.clone(),
            streaming: self.streaming,
        });

        let result = if self.streaming {
            self.recognize_live(&audio)
        } else {
            self.recognize_at_the_end(&audio)
        };
        // Stop the device before the last event: the microphone light going out is what tells the
        // user the app let go of it.
        drop(stream);
        result
    }

    /// Start whichever source this is, and return the handle that must outlive the recognition loop.
    /// A device returns its stream; memory returns nothing, because its feeder thread ends by dropping
    /// the sender, which is what tells the loop the input is over.
    fn open(&mut self, sender: SyncSender<Vec<f32>>) -> Result<Option<cpal::Stream>, AsrError> {
        // `Input::Pcm` is consumed here: the samples are moved to the feeder thread rather than copied,
        // and the worker has no use for them afterwards.
        let block = (self.source_rate as usize / 10 * self.channels.max(1) as usize).max(160);
        match std::mem::replace(&mut self.input, Input::Pcm { samples: Vec::new() }) {
            Input::Pcm { samples } => {
                std::thread::Builder::new()
                    .name("pai-asr-feed".into())
                    .spawn(move || {
                        for chunk in samples.chunks(block) {
                            // A closed receiver means the run ended; stop feeding rather than block.
                            if sender.send(chunk.to_vec()).is_err() {
                                return;
                            }
                        }
                    })
                    .map_err(|error| {
                        AsrError::Unavailable(format!("không tạo được luồng nạp âm thanh: {error}"))
                    })?;
                Ok(None)
            }
            Input::Device {
                device,
                config,
                sample_format,
            } => {
                let stream = capture(&device, &config, sample_format, sender)?;
                stream.play().map_err(|error| {
                    AsrError::Unavailable(format!("không bật được micro: {error}"))
                })?;
                Ok(Some(stream))
            }
        }
    }


    /// The streaming path: feed as the audio arrives and publish the committed/tentative split.
    fn recognize_live(&self, audio: &Receiver<Vec<f32>>) -> Result<(), AsrError> {
        let mut session = self
            .model
            .session()
            .map_err(|error| AsrError::Engine(format!("không mở được phiên nhận dạng: {error}")))?;
        let mut stream = session
            .stream(
                &self.run_options,
                &StreamOptions {
                    // The library's family-specific stable prefix: what it commits, it keeps, which
                    // is the whole reason a composer can show this text as you speak.
                    commit_policy: CommitPolicy::Auto,
                    ..Default::default()
                },
            )
            .map_err(|error| AsrError::Engine(format!("không mở được luồng nhận dạng: {error}")))?;

        let mut resampler = LiveResampler::new(self.source_rate, self.channels)?;
        let mut recorded = 0_usize;
        let mut clock = Clock::default();
        while let Some(buffer) = self.next_buffer(audio) {
            clock.hear(&buffer);
            let pcm = resampler.push(&buffer)?;
            if pcm.is_empty() {
                continue;
            }
            recorded += pcm.len();
            let update = stream
                .feed(&pcm)
                .map_err(|error| AsrError::Engine(format!("nhận dạng thất bại: {error}")))?;
            // The tick goes out first: a word that arrives late still arrives with the right clock behind it.
            if let Some(tick) = clock.tick(elapsed_ms(recorded)) {
                self.emit(tick);
            }
            if update.committed_changed || update.tentative_changed {
                let text = stream.text();
                self.emit(DictationEvent::Text {
                    committed: text.committed,
                    tentative: text.tentative,
                    recorded_ms: elapsed_ms(recorded),
                });
            }
        }
        if self.cancelled.load(Ordering::SeqCst) {
            stream.reset();
            return Ok(());
        }

        let tail = resampler.drain()?;
        if !tail.is_empty() {
            let _ = stream.feed(&tail);
        }
        stream
            .finalize()
            .map_err(|error| AsrError::Engine(format!("không chốt được bản ghi: {error}")))?;
        let text = stream.text();
        // `full` is the authoritative hypothesis; committed traded authority for stability while
        // the words were still moving, and that trade ends here.
        self.emit(DictationEvent::Finished {
            text: text.full.trim().to_owned(),
        });
        Ok(())
    }

    /// The offline path, for a model with no streaming hooks (a Whisper GGUF, say): hold the audio,
    /// then transcribe once at the end. No live text -- but dictation still works, which is the
    /// difference between "your model cannot do this" and a feature that silently is not there.
    fn recognize_at_the_end(&self, audio: &Receiver<Vec<f32>>) -> Result<(), AsrError> {
        let mut resampler = LiveResampler::new(self.source_rate, self.channels)?;
        let mut samples: Vec<f32> = Vec::new();
        let mut clock = Clock::default();
        while let Some(buffer) = self.next_buffer(audio) {
            clock.hear(&buffer);
            samples.extend(resampler.push(&buffer)?);
            if let Some(tick) = clock.tick(elapsed_ms(samples.len())) {
                self.emit(tick);
            }
        }
        if self.cancelled.load(Ordering::SeqCst) {
            return Ok(());
        }
        samples.extend(resampler.drain()?);
        if samples.is_empty() {
            self.emit(DictationEvent::Finished {
                text: String::new(),
            });
            return Ok(());
        }

        let mut session = self
            .model
            .session()
            .map_err(|error| AsrError::Engine(format!("không mở được phiên nhận dạng: {error}")))?;
        let result = session
            .run(&samples, &self.run_options)
            .map_err(|error| AsrError::Engine(format!("nhận dạng thất bại: {error}")))?;
        self.emit(DictationEvent::Finished {
            text: result.text.trim().to_owned(),
        });
        Ok(())
    }

    /// The next device buffer, or `None` when the user asked to stop and nothing is left to read.
    /// The timeout is what makes "stop" responsive on a silent microphone.
    fn next_buffer(&self, audio: &Receiver<Vec<f32>>) -> Option<Vec<f32>> {
        loop {
            if self.cancelled.load(Ordering::SeqCst) {
                return None;
            }
            if self.stopping.load(Ordering::SeqCst) {
                // Stop means "no more audio from here on", not "drop what I just said": hand over
                // whatever the device already queued, then end.
                //
                // Checked here rather than only when the read times out, which is where it used to
                // live: a live microphone never stops producing -- silence is audio too -- so the
                // timeout never came, the flag was never read, and pressing stop did nothing at all.
                // Non-blocking on purpose, for the same reason: waiting for a quiet moment on an open
                // device is waiting forever.
                return audio.try_recv().ok();
            }
            match audio.recv_timeout(Duration::from_millis(100)) {
                Ok(buffer) => return Some(buffer),
                // No audio and nobody asked to stop: keep waiting. The timeout exists so cancellation
                // is noticed on a device that has gone silent or been unplugged.
                Err(RecvTimeoutError::Timeout) => {}
                // The source ran out on its own -- a recording played to its end.
                Err(RecvTimeoutError::Disconnected) => return None,
            }
        }
    }

    fn emit(&self, event: DictationEvent) {
        // A closed receiver means the UI went away; the loop's own stop flags end the run.
        let _ = self.events.send(event);
    }
}

/// The clock and level meter both paths report from: it accumulates the peak of every device buffer and
/// hands back a [`DictationEvent::Recording`] no more than once per [`TICK_MS`].
#[derive(Default)]
struct Clock {
    announced_ms: i64,
    peak: f32,
}

impl Clock {
    /// Fold one device buffer into the running peak. Raw device samples, before resampling, so the meter
    /// shows what the microphone delivered rather than what survived the conversion.
    fn hear(&mut self, buffer: &[f32]) {
        for sample in buffer {
            let level = sample.abs();
            if level > self.peak {
                self.peak = level;
            }
        }
    }

    /// A tick if one is due, and the peak resets with it; `None` means it is not time yet.
    fn tick(&mut self, recorded_ms: i64) -> Option<DictationEvent> {
        if recorded_ms - self.announced_ms < TICK_MS {
            return None;
        }
        self.announced_ms = recorded_ms;
        let level = self.peak.min(1.0);
        self.peak = 0.0;
        Some(DictationEvent::Recording { recorded_ms, level })
    }
}

/// Build the capture stream for whatever sample format the device speaks, converting to `f32` inside the
/// callback -- the cheapest possible conversion, and the only work done on the audio thread.
fn capture(
    device: &cpal::Device,
    config: &cpal::StreamConfig,
    sample_format: SampleFormat,
    sender: SyncSender<Vec<f32>>,
) -> Result<cpal::Stream, AsrError> {
    match sample_format {
        SampleFormat::F32 => build::<f32>(device, config, sender),
        SampleFormat::I16 => build::<i16>(device, config, sender),
        SampleFormat::U16 => build::<u16>(device, config, sender),
        SampleFormat::I8 => build::<i8>(device, config, sender),
        SampleFormat::I32 => build::<i32>(device, config, sender),
        SampleFormat::F64 => build::<f64>(device, config, sender),
        other => Err(AsrError::Unavailable(format!(
            "micro trả về định dạng mẫu `{other}` chưa hỗ trợ"
        ))),
    }
}

fn build<T>(
    device: &cpal::Device,
    config: &cpal::StreamConfig,
    sender: SyncSender<Vec<f32>>,
) -> Result<cpal::Stream, AsrError>
where
    T: cpal::SizedSample + Send + 'static,
    f32: FromSample<T>,
{
    device
        .build_input_stream(
            config,
            move |data: &[T], _: &cpal::InputCallbackInfo| {
                let buffer: Vec<f32> = data.iter().map(|value| value.to_sample::<f32>()).collect();
                // Never block the audio thread: a full queue means the recognizer is behind, and the
                // honest response is to lose this buffer rather than the stream.
                if sender.try_send(buffer).is_err() {
                    tracing::debug!("hàng đợi thu âm đầy, bỏ một khối");
                }
            },
            |error| tracing::warn!(%error, "lỗi luồng thu âm"),
            None,
        )
        .map_err(|error| AsrError::Unavailable(format!("không mở được micro: {error}")))
}

fn elapsed_ms(samples: usize) -> i64 {
    (samples as i64) * 1_000 / SAMPLE_RATE as i64
}
