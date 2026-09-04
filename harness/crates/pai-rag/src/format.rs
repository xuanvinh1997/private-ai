//! Document format.
//! This list must match `Format::as_str` and the `DocumentFormat` union in
//! `ui/src/lib/protocol.ts`.

use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Format {
    Pdf,
    /// `.docx` is supported natively. Other Office formats are currently rejected.
    Office,
    /// Raster image whose text is extracted by the configured vision model.
    Image,
    Html,
    /// `.csv`, `.tsv`, `.json`, `.xml`, `.yaml` — structured, but read out as text.
    Data,
    Markdown,
    Code,
    Text,
}

impl Format {
    pub fn as_str(self) -> &'static str {
        match self {
            Format::Pdf => "pdf",
            Format::Office => "office",
            Format::Image => "image",
            Format::Html => "html",
            Format::Data => "data",
            Format::Markdown => "markdown",
            Format::Code => "code",
            Format::Text => "text",
        }
    }

    /// From persisted strings; unknown formats fall back to [`Format::Text`].
    pub fn parse(name: &str) -> Format {
        match name.trim().to_ascii_lowercase().as_str() {
            "pdf" => Format::Pdf,
            "office" | "docx" | "xlsx" | "pptx" => Format::Office,
            "image" => Format::Image,
            "html" => Format::Html,
            "data" | "csv" => Format::Data,
            "markdown" => Format::Markdown,
            "code" => Format::Code,
            _ => Format::Text,
        }
    }
}
