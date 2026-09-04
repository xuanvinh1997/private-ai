//! Document format.
//! This list must match `Format::as_str`, the `format` labels from the Python extractor,
//! and the `DocumentFormat` union in `ui/src/lib/protocol.ts`. Grouped by how it is read.

use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Format {
    Pdf,
    /// `.docx`, `.xlsx`, `.pptx` and relatives — read through markitdown.
    Office,
    /// Images, read by a vision model. Only present once a vision model is selected.
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

    /// From the wire string; unknown formats fall back to [`Format::Text`] so a new Python label never blanks the list.
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
