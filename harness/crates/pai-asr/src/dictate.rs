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
    /// A model that cannot stream is buffering; there is no text yet, only elapsed time.
    Recording { recorded_ms: i64 },
    /// The final transcript. Always the last event of a successful run.
    Finished { text: String },
    /// Dictation stopped because something broke. Also the last event.
    Failed { message: String },
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

/// A dictation in flight. Dropping it stops the microphone.
#[derive(Debug)]
pub struct Dictation {
    control: DictationControl,
    events: UnboundedReceiver<DictationEvent>,
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
    }
}

/// Open the default input device and start recognizing. Fails before any thread is spawned when
/// there is no model or no microphone, so the caller's error is the real one.
pub fn start(asr: &Asr) -> Result<Dictation, AsrError> {
    let model = asr.model()?;
    let streaming = model.capabilities().supports_streaming;
    let run_options = asr.run_options();

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
    let source_rate = config.sample_rate.0;
    let channels = config.channels;

    let stopping = Arc::new(AtomicBool::new(false));
    let cancelled = Arc::new(AtomicBool::new(false));
    let (events, receiver) = unbounded_channel();

    let worker = Worker {
        model,
        streaming,
        run_options,
        device,
        config,
        sample_format,
        name,
        source_rate,
        channels,
        stopping: stopping.clone(),
        cancelled: cancelled.clone(),
        events,
    };
    // A plain OS thread, not a Tokio blocking task: this one runs for as long as the user holds the
    // button down, and a Tokio worker is the wrong thing to occupy for minutes.
    std::thread::Builder::new()
        .name("pai-asr-dictation".into())
        .spawn(move || worker.run())
        .map_err(|error| AsrError::Unavailable(format!("không tạo được luồng đọc chính tả: {error}")))?;

    Ok(Dictation {
        control: DictationControl {
            stopping,
            cancelled,
        },
        events: receiver,
    })
}

struct Worker {
    model: transcribe_cpp::Model,
    streaming: bool,
    run_options: transcribe_cpp::RunOptions,
    device: cpal::Device,
    config: cpal::StreamConfig,
    sample_format: SampleFormat,
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

    fn drive(self) -> Result<(), AsrError> {
        let (sender, audio) = sync_channel::<Vec<f32>>(QUEUE_DEPTH);
        let stream = self.capture(sender)?;
        stream
            .play()
            .map_err(|error| AsrError::Unavailable(format!("không bật được micro: {error}")))?;
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

    /// Build the capture stream for whatever sample format the device speaks, converting to `f32`
    /// inside the callback -- the cheapest possible conversion, and the only work done there.
    fn capture(&self, sender: SyncSender<Vec<f32>>) -> Result<cpal::Stream, AsrError> {
        let on_error = |error| tracing::warn!(%error, "lỗi luồng thu âm");
        let build = |sender: SyncSender<Vec<f32>>| match self.sample_format {
            SampleFormat::F32 => self.build::<f32>(sender, on_error),
            SampleFormat::I16 => self.build::<i16>(sender, on_error),
            SampleFormat::U16 => self.build::<u16>(sender, on_error),
            SampleFormat::I8 => self.build::<i8>(sender, on_error),
            SampleFormat::I32 => self.build::<i32>(sender, on_error),
            SampleFormat::F64 => self.build::<f64>(sender, on_error),
            other => Err(AsrError::Unavailable(format!(
                "micro trả về định dạng mẫu `{other}` chưa hỗ trợ"
            ))),
        };
        build(sender)
    }

    fn build<T>(
        &self,
        sender: SyncSender<Vec<f32>>,
        on_error: impl FnMut(cpal::StreamError) + Send + 'static,
    ) -> Result<cpal::Stream, AsrError>
    where
        T: cpal::SizedSample + Send + 'static,
        f32: FromSample<T>,
    {
        self.device
            .build_input_stream(
                &self.config,
                move |data: &[T], _: &cpal::InputCallbackInfo| {
                    let buffer: Vec<f32> = data.iter().map(|value| value.to_sample::<f32>()).collect();
                    // Never block the audio thread: a full queue means the recognizer is behind,
                    // and the honest response is to lose this buffer rather than the stream.
                    if sender.try_send(buffer).is_err() {
                        tracing::debug!("hàng đợi thu âm đầy, bỏ một khối");
                    }
                },
                on_error,
                None,
            )
            .map_err(|error| AsrError::Unavailable(format!("không mở được micro: {error}")))
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
        while let Some(buffer) = self.next_buffer(audio) {
            let pcm = resampler.push(&buffer)?;
            if pcm.is_empty() {
                continue;
            }
            recorded += pcm.len();
            let update = stream
                .feed(&pcm)
                .map_err(|error| AsrError::Engine(format!("nhận dạng thất bại: {error}")))?;
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
        let mut announced_ms = 0_i64;
        while let Some(buffer) = self.next_buffer(audio) {
            samples.extend(resampler.push(&buffer)?);
            let recorded_ms = elapsed_ms(samples.len());
            // One tick a second: the UI needs a clock, not a firehose.
            if recorded_ms - announced_ms >= 1_000 {
                announced_ms = recorded_ms;
                self.emit(DictationEvent::Recording { recorded_ms });
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
            match audio.recv_timeout(Duration::from_millis(100)) {
                Ok(buffer) => return Some(buffer),
                Err(RecvTimeoutError::Timeout) => {
                    if self.stopping.load(Ordering::SeqCst) {
                        return None;
                    }
                }
                Err(RecvTimeoutError::Disconnected) => return None,
            }
        }
    }

    fn emit(&self, event: DictationEvent) {
        // A closed receiver means the UI went away; the loop's own stop flags end the run.
        let _ = self.events.send(event);
    }
}

fn elapsed_ms(samples: usize) -> i64 {
    (samples as i64) * 1_000 / SAMPLE_RATE as i64
}
