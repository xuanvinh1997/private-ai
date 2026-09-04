//! Kiểm tra tay: giải mã một tệp âm thanh rồi đọc nó bằng mô hình đã chọn.
//!
//!     cargo run -p pai-asr --example smoke -- <model.gguf> <audio>

use std::path::PathBuf;

use pai_asr::{Asr, AsrConfig};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args().skip(1);
    let model = PathBuf::from(args.next().expect("đường dẫn model .gguf"));
    let audio = PathBuf::from(args.next().expect("đường dẫn tệp âm thanh"));

    let asr = Asr::new(AsrConfig {
        enabled: true,
        model,
        language: args.next().unwrap_or_default(),
    });
    println!("{:#?}", asr.describe().await?);

    let started = std::time::Instant::now();
    let transcription = asr.transcribe_file(&audio).await?;
    println!(
        "\n{} ms âm thanh, đọc trong {} ms\n",
        transcription.duration_ms,
        started.elapsed().as_millis()
    );
    println!("{}", transcription.to_markdown("Kiểm tra"));
    Ok(())
}
