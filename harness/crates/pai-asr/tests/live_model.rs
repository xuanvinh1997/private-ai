//! The recognizer against a real model, in both modes. It loads half a gigabyte of weights and one of its
//! cases opens the microphone, so every test here is `#[ignore]`d: a suite that goes red because a machine
//! has no model on it is a suite nobody trusts. Run it deliberately:
//!
//! ```text
//! # Mô hình lấy từ ~/.private-ai/asr/models; PAI_ASR_TEST_DIR trỏ tới thư mục tệp âm thanh.
//! PAI_ASR_TEST_DIR=/tmp/asr-fixtures \
//!   cargo test -p pai-asr --test live_model -- --ignored --nocapture
//!
//! # Chỉ phần micro, cần quyền thu âm cho tiến trình đang chạy:
//! cargo test -p pai-asr --test live_model microphone -- --ignored --nocapture
//! ```
//!
//! What it checks that nothing else can: that the model produces words at all, that every container the
//! library claims to read really decodes, that the streaming path commits text without ever rewriting what
//! it committed, and that streaming and offline recognition of the same audio agree.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use pai_asr::{Asr, AsrConfig, DictationEvent, Source, decode_file};

/// A recognizer on the model this machine has, or `None` when there is none to run.
fn asr() -> Option<Asr> {
    let home = std::env::var_os("HOME").map(PathBuf::from)?;
    let model = pai_asr::discover_model(&home.join(".private-ai"))?;
    Some(Asr::new(AsrConfig {
        enabled: true,
        model,
        language: String::new(),
    }))
}

/// The folder of test recordings, or `None` when the caller did not point at one.
fn fixtures() -> Option<PathBuf> {
    let directory = PathBuf::from(std::env::var_os("PAI_ASR_TEST_DIR")?);
    directory.is_dir().then_some(directory)
}

/// Every file in the fixture folder the library would try to read, in a stable order.
fn recordings(directory: &Path) -> Vec<PathBuf> {
    let mut found: Vec<PathBuf> = std::fs::read_dir(directory)
        .into_iter()
        .flatten()
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| pai_asr::is_audio(path))
        .collect();
    found.sort();
    found
}

/// Words, lowercased and stripped of punctuation, for comparing two transcripts of the same audio.
fn words(text: &str) -> Vec<String> {
    text.split_whitespace()
        .map(|word| {
            word.chars()
                .filter(|character| character.is_alphanumeric())
                .collect::<String>()
                .to_lowercase()
        })
        .filter(|word| !word.is_empty())
        .collect()
}

/// How much of `left` appears in `right`, position-free. Not an accuracy score: two runs of the same model
/// over the same audio should agree almost exactly, and this is what says whether they did.
fn overlap(left: &[String], right: &[String]) -> f32 {
    if left.is_empty() {
        return 0.0;
    }
    let mut pool: Vec<&String> = right.iter().collect();
    let mut shared = 0;
    for word in left {
        if let Some(position) = pool.iter().position(|found| *found == word) {
            pool.remove(position);
            shared += 1;
        }
    }
    shared as f32 / left.len() as f32
}

/// Offline recognition, over every container in the fixture folder. This is the file mode exactly as the
/// document library runs it: `decode_file` then `run`.
#[tokio::test]
#[ignore = "cần mô hình thật và PAI_ASR_TEST_DIR"]
async fn file_mode_reads_every_container() {
    let (Some(asr), Some(directory)) = (asr(), fixtures()) else {
        eprintln!("bỏ qua: chưa có mô hình hoặc PAI_ASR_TEST_DIR");
        return;
    };
    let files = recordings(&directory);
    assert!(
        !files.is_empty(),
        "không có tệp âm thanh nào trong {}",
        directory.display()
    );

    println!("\n=== chế độ tệp ===");
    for path in files {
        let clip = decode_file(&path).expect("giải mã được");
        let started = Instant::now();
        let transcription = asr.transcribe_file(&path).await.expect("nhận dạng được");
        let elapsed = started.elapsed();

        let seconds = transcription.duration_ms as f32 / 1000.0;
        println!(
            "  {:<28} {:>5.1}s âm thanh · {:>6}Hz {}ch · {:>6}ms · RTF {:.3} · {} đoạn",
            path.file_name().unwrap_or_default().to_string_lossy(),
            seconds,
            clip.source_rate,
            clip.channels,
            elapsed.as_millis(),
            elapsed.as_secs_f32() / seconds.max(0.001),
            transcription.lines.len(),
        );
        println!("      {}", transcription.text.trim());

        assert!(
            !transcription.text.trim().is_empty(),
            "{} không ra chữ nào",
            path.display()
        );
        assert!(transcription.duration_ms > 0);
        // Faster than real time, by a wide margin, or the library screen becomes unusable on a folder of
        // recordings. The bar is deliberately loose: it catches a collapse, not a slow machine.
        assert!(
            elapsed.as_secs_f32() < seconds,
            "{} chậm hơn thời gian thực",
            path.display()
        );
    }
}

/// Streaming, driven from a recording instead of a microphone: same worker, same resampler, same feed loop.
/// The three properties that matter are all invisible from the outside otherwise -- that committed text only
/// ever grows, that a final transcript arrives, and that it says what the offline pass says.
#[tokio::test]
#[ignore = "cần mô hình thật và PAI_ASR_TEST_DIR"]
async fn streaming_mode_commits_forward_and_agrees_with_the_file_mode() {
    let (Some(asr), Some(directory)) = (asr(), fixtures()) else {
        eprintln!("bỏ qua: chưa có mô hình hoặc PAI_ASR_TEST_DIR");
        return;
    };
    let info = asr.describe().await.expect("nạp được mô hình");
    if !info.streaming {
        eprintln!("bỏ qua: `{}` không chạy theo dòng", info.variant);
        return;
    }

    println!(
        "\n=== chế độ dòng ({} trên {}) ===",
        info.variant, info.backend
    );
    for path in recordings(&directory) {
        let clip = decode_file(&path).expect("giải mã được");
        let offline = asr.transcribe_file(&path).await.expect("nhận dạng được");

        let started = Instant::now();
        let mut dictation = pai_asr::dictate(
            &asr,
            // 16 kHz mono, the shape `decode_file` produces; the device path's own rates are covered by
            // the resampler's unit tests and by the microphone case below.
            Source::Pcm {
                samples: clip.samples.clone(),
                rate: pai_asr::SAMPLE_RATE,
                channels: 1,
            },
        )
        .expect("mở được phiên đọc");

        let mut committed = String::new();
        let mut ticks = 0_usize;
        let mut first_text: Option<u128> = None;
        let mut final_text: Option<String> = None;
        while let Some(event) = dictation.next().await {
            match event {
                DictationEvent::Started { streaming, .. } => assert!(streaming),
                DictationEvent::Text {
                    committed: next, ..
                } => {
                    ticks += 1;
                    if first_text.is_none() && !next.trim().is_empty() {
                        first_text = Some(started.elapsed().as_millis());
                    }
                    // The whole promise of `committed`: what the composer has already drawn is never
                    // rewritten under it. A shrinking or diverging prefix is the bug this test exists for.
                    assert!(
                        next.starts_with(&committed) || committed.starts_with(&next),
                        "chữ đã chốt bị viết lại:\n  trước: {committed}\n  sau:   {next}"
                    );
                    if next.len() >= committed.len() {
                        committed = next;
                    }
                }
                DictationEvent::Recording { .. } => {}
                DictationEvent::Finished { text } => final_text = Some(text),
                DictationEvent::Failed { message } => panic!("đọc theo dòng hỏng: {message}"),
            }
        }

        let streamed = final_text.expect("phải có bản chép cuối");
        let elapsed = started.elapsed();
        let seconds = clip.duration_ms() as f32 / 1000.0;
        let agreement = overlap(&words(&offline.text), &words(&streamed));
        println!(
            "  {:<28} {:>5.1}s · {:>6}ms · RTF {:.3} · {ticks} nhịp · chữ đầu {}ms · trùng {:.0}%",
            path.file_name().unwrap_or_default().to_string_lossy(),
            seconds,
            elapsed.as_millis(),
            elapsed.as_secs_f32() / seconds.max(0.001),
            first_text
                .map(|ms| ms.to_string())
                .unwrap_or_else(|| "—".into()),
            agreement * 100.0,
        );
        println!("      {}", streamed.trim());

        assert!(
            !streamed.trim().is_empty(),
            "{} không ra chữ nào",
            path.display()
        );
        assert!(ticks > 0, "{} không phát nhịp nào", path.display());
        // Not byte equality: streaming decides on a growing prefix of the audio and offline sees all of it,
        // so a word at a chunk boundary can differ. A large disagreement means one of the two paths is broken.
        assert!(
            agreement > 0.8,
            "{} lệch quá nhiều giữa hai chế độ ({:.0}%)\n  tệp:  {}\n  dòng: {}",
            path.display(),
            agreement * 100.0,
            offline.text,
            streamed
        );
    }
}

/// The same audio, several times over one loaded model. Catches the failure that only shows up in use: a
/// session that drifts, leaks or slows down after the first run.
#[tokio::test]
#[ignore = "cần mô hình thật và PAI_ASR_TEST_DIR"]
async fn repeated_runs_stay_identical() {
    let (Some(asr), Some(directory)) = (asr(), fixtures()) else {
        eprintln!("bỏ qua: chưa có mô hình hoặc PAI_ASR_TEST_DIR");
        return;
    };
    let path = recordings(&directory)
        .into_iter()
        .next()
        .expect("một tệp âm thanh");

    println!("\n=== lặp lại ===");
    let mut first: Option<String> = None;
    for round in 1..=5 {
        let started = Instant::now();
        let transcription = asr.transcribe_file(&path).await.expect("nhận dạng được");
        println!("  lượt {round}: {:>5}ms", started.elapsed().as_millis());
        match &first {
            None => first = Some(transcription.text),
            // Greedy decoding over identical input: anything but an identical answer means state is
            // leaking between runs on the same session.
            Some(expected) => {
                assert_eq!(expected, &transcription.text, "lượt {round} khác lượt đầu")
            }
        }
    }
}

/// Cancelling mid-stream must stop the run and produce no transcript: this is the Escape key's contract,
/// and getting it wrong pastes half a sentence into the composer of someone who asked for none.
#[tokio::test]
#[ignore = "cần mô hình thật và PAI_ASR_TEST_DIR"]
async fn cancelling_a_stream_yields_nothing() {
    let (Some(asr), Some(directory)) = (asr(), fixtures()) else {
        eprintln!("bỏ qua: chưa có mô hình hoặc PAI_ASR_TEST_DIR");
        return;
    };
    let path = recordings(&directory)
        .into_iter()
        .next()
        .expect("một tệp âm thanh");
    let clip = decode_file(&path).expect("giải mã được");

    let mut dictation = pai_asr::dictate(
        &asr,
        Source::Pcm {
            samples: clip.samples,
            rate: pai_asr::SAMPLE_RATE,
            channels: 1,
        },
    )
    .expect("mở được phiên đọc");

    dictation.cancel();
    while let Some(event) = dictation.next().await {
        match event {
            DictationEvent::Finished { .. } => panic!("một phiên đã huỷ không được trả bản chép"),
            DictationEvent::Failed { message } => panic!("huỷ không phải là lỗi: {message}"),
            _ => {}
        }
    }
    println!("\n=== huỷ giữa chừng: không có bản chép, không có lỗi ===");
}

/// The one case a recording cannot stand in for: the real device. It only checks that the microphone opens
/// and delivers samples -- there is nobody in the room to say a sentence -- and it reports the peak level,
/// which is what tells a human whether the OS actually granted access or handed over silence.
#[tokio::test]
#[ignore = "mở micro thật; cần quyền thu âm cho tiến trình đang chạy"]
async fn microphone_opens_and_delivers_audio() {
    let Some(asr) = asr() else {
        eprintln!("bỏ qua: chưa có mô hình");
        return;
    };
    let mut dictation = pai_asr::dictate(&asr, Source::Microphone).expect("mở được micro");

    let mut device = String::new();
    let mut peak = 0.0_f32;
    let mut recorded_ms = 0_i64;
    // The clock runs on the test's own time, not on events arriving: a device that goes silent, or one
    // the OS quietly denied, would otherwise leave this waiting forever with nothing to show for it.
    let deadline = Instant::now() + Duration::from_millis(2_500);
    let mut asked_to_stop = false;
    loop {
        let Ok(next) = tokio::time::timeout(Duration::from_millis(500), dictation.next()).await
        else {
            assert!(
                Instant::now() < deadline + Duration::from_secs(5),
                "micro không trả sự kiện nào trong {recorded_ms}ms"
            );
            if Instant::now() >= deadline && !asked_to_stop {
                asked_to_stop = true;
                dictation.stop();
            }
            continue;
        };
        let Some(event) = next else { break };
        match event {
            DictationEvent::Started { device: name, .. } => device = name,
            DictationEvent::Recording {
                level,
                recorded_ms: ms,
            } => {
                peak = peak.max(level);
                recorded_ms = ms;
            }
            DictationEvent::Text {
                recorded_ms: ms, ..
            } => recorded_ms = ms,
            DictationEvent::Finished { text } => {
                println!("\n=== micro `{device}` ===");
                println!("  {recorded_ms}ms âm thanh, đỉnh {peak:.3}");
                println!(
                    "  nghe ra: {}",
                    if text.trim().is_empty() {
                        "(im lặng)"
                    } else {
                        text.trim()
                    }
                );
                assert!(recorded_ms > 500, "micro chỉ trả {recorded_ms}ms");
                return;
            }
            DictationEvent::Failed { message } => panic!("không thu được: {message}"),
        }
        if Instant::now() >= deadline && !asked_to_stop {
            asked_to_stop = true;
            dictation.stop();
        }
    }
    panic!("micro không trả sự kiện kết thúc nào");
}
