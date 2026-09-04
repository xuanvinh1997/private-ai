use serde::{Deserialize, Serialize};

/// Section assigned to text preceding the document's first heading.
pub const DEFAULT_SECTION: &str = "Nội dung";
const MAX_SECTION_TITLE: usize = 240;

/// One chunk, including the location data needed for a citation.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Chunk {
    pub ordinal: usize,
    pub text: String,
    pub section: String,
    /// Zero means the source format has no notion of pages.
    pub page: u32,
}

/// The exact text sent to the embedder by the Python implementation.
pub fn embedding_text_for(section: &str, text: &str) -> String {
    if !section.is_empty() && section != DEFAULT_SECTION {
        format!("{section}\n\n{text}")
    } else {
        text.to_owned()
    }
}

#[derive(Clone, Debug)]
struct Unit {
    text: String,
    section: String,
    page: u32,
    flush: bool,
}

/// Splits text by page, Markdown heading, paragraph, sentence, then character boundary.
///
/// Sizes are counted in Unicode scalar values, matching Python's `len` for normal document
/// text and avoiding accidental byte-based cuts inside Vietnamese text.
#[derive(Clone, Debug)]
pub struct SectionAwareSplitter {
    chunk_size: usize,
    chunk_overlap: usize,
    default_section: String,
}

impl Default for SectionAwareSplitter {
    fn default() -> Self {
        Self::new(1_400, 180)
    }
}

impl SectionAwareSplitter {
    pub fn new(chunk_size: usize, chunk_overlap: usize) -> Self {
        Self::with_default_section(chunk_size, chunk_overlap, DEFAULT_SECTION)
    }

    pub fn with_default_section(
        chunk_size: usize,
        chunk_overlap: usize,
        default_section: impl Into<String>,
    ) -> Self {
        let chunk_size = chunk_size.max(1);
        Self {
            chunk_size,
            chunk_overlap: chunk_overlap.min(chunk_size - 1),
            default_section: default_section.into(),
        }
    }

    pub fn split_text(&self, text: &str) -> Vec<String> {
        self.split(text)
            .into_iter()
            .map(|chunk| chunk.text)
            .collect()
    }

    pub fn split(&self, text: &str) -> Vec<Chunk> {
        self.pack(self.units(text))
    }

    fn units(&self, text: &str) -> Vec<Unit> {
        let mut section = self.default_section.clone();
        let mut page = 0;
        let mut units = Vec::new();
        let mut page_turned = false;

        for block in blocks(text) {
            let stripped = block.trim();
            if let Some(number) = page_marker(stripped) {
                page = number;
                page_turned = true;
                continue;
            }

            if let Some(title) = heading(stripped) {
                section = truncate_chars(title.trim(), MAX_SECTION_TITLE);
                if section.is_empty() {
                    section.clone_from(&self.default_section);
                }
                units.push(Unit {
                    text: stripped.to_owned(),
                    section: section.clone(),
                    page,
                    flush: true,
                });
                page_turned = false;
                continue;
            }

            for piece in self.fit(stripped) {
                units.push(Unit {
                    text: piece,
                    section: section.clone(),
                    page,
                    flush: page_turned,
                });
                page_turned = false;
            }
        }
        units
    }

    fn fit(&self, block: &str) -> Vec<String> {
        if char_len(block) <= self.chunk_size {
            return vec![block.to_owned()];
        }

        let mut output = Vec::new();
        for sentence in sentences(block) {
            let sentence = sentence.trim();
            if sentence.is_empty() {
                continue;
            }
            if char_len(sentence) <= self.chunk_size {
                output.push(sentence.to_owned());
            } else {
                output.extend(self.hard_split(sentence));
            }
        }
        output
    }

    fn hard_split(&self, text: &str) -> Vec<String> {
        let chars: Vec<char> = text.chars().collect();
        let mut output = Vec::new();
        let mut start = 0;

        while start < chars.len() {
            let mut end = (start + self.chunk_size).min(chars.len());
            if end < chars.len() {
                let search_start = start + self.chunk_size * 3 / 4;
                if let Some(space) = (search_start..end).rev().find(|&at| chars[at] == ' ')
                    && space > start
                {
                    end = space;
                }
            }
            let piece: String = chars[start..end].iter().collect();
            let piece = piece.trim();
            if !piece.is_empty() {
                output.push(piece.to_owned());
            }
            start = if end > start {
                end
            } else {
                start + self.chunk_size
            };
        }
        output
    }

    fn pack(&self, units: Vec<Unit>) -> Vec<Chunk> {
        let mut chunks = Vec::new();
        let mut open_units = Vec::new();
        let mut carry = String::new();
        let mut filled = 0;
        let min_fill = self.chunk_size / 3;

        for unit in units {
            let too_full = filled + char_len(&unit.text) > self.chunk_size;
            let new_section = unit.flush && filled >= min_fill;
            if !open_units.is_empty() && (too_full || new_section) {
                carry = self.flush(&mut chunks, &mut open_units, &carry);
                filled = char_len(&carry);
            }
            filled += char_len(&unit.text);
            open_units.push(unit);
        }
        self.flush(&mut chunks, &mut open_units, &carry);
        chunks
    }

    fn flush(&self, chunks: &mut Vec<Chunk>, units: &mut Vec<Unit>, carry: &str) -> String {
        if units.is_empty() {
            return carry.to_owned();
        }
        let body = units
            .iter()
            .map(|unit| unit.text.as_str())
            .collect::<Vec<_>>()
            .join("\n\n");
        let text = if carry.is_empty() {
            body
        } else {
            format!("{carry}\n\n{body}").trim().to_owned()
        };
        chunks.push(Chunk {
            ordinal: chunks.len(),
            text: text.clone(),
            section: units[0].section.clone(),
            page: units[0].page,
        });
        units.clear();
        self.overlap_tail(&text)
    }

    fn overlap_tail(&self, text: &str) -> String {
        let chars: Vec<char> = text.chars().collect();
        if self.chunk_overlap == 0 || chars.len() <= self.chunk_overlap {
            return String::new();
        }
        let tail = &chars[chars.len() - self.chunk_overlap..];
        let start = tail
            .iter()
            .position(|character| *character == ' ')
            .map_or(0, |space| space + 1);
        tail[start..].iter().collect::<String>().trim().to_owned()
    }
}

fn char_len(text: &str) -> usize {
    text.chars().count()
}

fn truncate_chars(text: &str, limit: usize) -> String {
    text.chars().take(limit).collect()
}

fn heading(line: &str) -> Option<&str> {
    let hashes = line.bytes().take_while(|byte| *byte == b'#').count();
    if !(1..=6).contains(&hashes) {
        return None;
    }
    let rest = &line[hashes..];
    let first = rest.chars().next()?;
    if !first.is_whitespace() {
        return None;
    }
    let title = rest.trim_start();
    (!title.is_empty()).then_some(title)
}

fn page_marker(line: &str) -> Option<u32> {
    let rest = line.strip_prefix("<!--")?.trim_start();
    let rest = rest.strip_prefix("pai-page:")?;
    let digits = rest.bytes().take_while(u8::is_ascii_digit).count();
    if digits == 0 {
        return None;
    }
    let (number, suffix) = rest.split_at(digits);
    if suffix.trim_start() != "-->" {
        return None;
    }
    number.parse().ok()
}

fn blocks(text: &str) -> Vec<String> {
    let mut output = Vec::new();
    let mut open_lines = Vec::new();

    let close = |output: &mut Vec<String>, open_lines: &mut Vec<String>| {
        if !open_lines.is_empty() {
            output.push(open_lines.join("\n"));
            open_lines.clear();
        }
    };

    for line in text.lines() {
        let stripped = line.trim();
        if stripped.is_empty() {
            close(&mut output, &mut open_lines);
        } else if page_marker(stripped).is_some() || heading(stripped).is_some() {
            close(&mut output, &mut open_lines);
            output.push(stripped.to_owned());
        } else {
            open_lines.push(line.trim_end().to_owned());
        }
    }
    close(&mut output, &mut open_lines);
    output
}

fn sentences(text: &str) -> Vec<String> {
    let chars: Vec<char> = text.chars().collect();
    let mut output = Vec::new();
    let mut start = 0;
    let mut at = 0;

    while at + 1 < chars.len() {
        if matches!(chars[at], '.' | '!' | '?' | '…' | ';') && chars[at + 1].is_whitespace() {
            let mut next = at + 1;
            while next < chars.len() && chars[next].is_whitespace() {
                next += 1;
            }
            output.push(chars[start..=at].iter().collect());
            start = next;
            at = next;
        } else {
            at += 1;
        }
    }
    output.push(chars[start..].iter().collect());
    output
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn carries_section_and_page_like_python_splitter() {
        let input = "Mở đầu đủ dài để tách.\n\n# Phần một\n\nNội dung trang một.\n\n<!-- pai-page:2 -->\n\nNội dung trang hai.";
        let chunks = SectionAwareSplitter::new(24, 0).split(input);

        assert_eq!(
            chunks,
            vec![
                Chunk {
                    ordinal: 0,
                    text: "Mở đầu đủ dài để tách.".into(),
                    section: DEFAULT_SECTION.into(),
                    page: 0,
                },
                Chunk {
                    ordinal: 1,
                    text: "# Phần một".into(),
                    section: "Phần một".into(),
                    page: 0,
                },
                Chunk {
                    ordinal: 2,
                    text: "Nội dung trang một.".into(),
                    section: "Phần một".into(),
                    page: 0,
                },
                Chunk {
                    ordinal: 3,
                    text: "Nội dung trang hai.".into(),
                    section: "Phần một".into(),
                    page: 2,
                },
            ]
        );
    }

    #[test]
    fn hard_cut_counts_unicode_characters_not_utf8_bytes() {
        let chunks = SectionAwareSplitter::new(4, 0).split("áéíóú");
        assert_eq!(chunks[0].text, "áéíó");
        assert_eq!(chunks[1].text, "ú");
    }

    #[test]
    fn overlap_slides_to_a_word_boundary() {
        let chunks = SectionAwareSplitter::new(12, 6).split("alpha beta gamma delta");
        assert_eq!(chunks[0].text, "alpha beta");
        assert_eq!(chunks[1].text, "beta\n\ngamma delta");
    }

    #[test]
    fn embedding_input_skips_only_the_default_section() {
        assert_eq!(embedding_text_for(DEFAULT_SECTION, "body"), "body");
        assert_eq!(embedding_text_for("API", "body"), "API\n\nbody");
    }

    #[test]
    fn sentence_overlap_matches_python_reference() {
        let chunks = SectionAwareSplitter::new(16, 3)
            .split("Một câu ngắn. Câu thứ hai dài hơn nhiều; chốt.");
        let bodies: Vec<_> = chunks.iter().map(|chunk| chunk.text.as_str()).collect();
        assert_eq!(
            bodies,
            [
                "Một câu ngắn.",
                "ắn.\n\nCâu thứ hai dài",
                "dài\n\nhơn nhiều;",
                "ều;\n\nchốt.",
            ]
        );
    }

    #[test]
    fn short_headings_share_a_chunk_like_python_reference() {
        let chunks = SectionAwareSplitter::new(60, 0).split("# A\n\nx\n\n# B\n\ny");
        assert_eq!(chunks.len(), 1);
        assert_eq!(chunks[0].text, "# A\n\nx\n\n# B\n\ny");
        assert_eq!(chunks[0].section, "A");
    }

    #[test]
    fn accepts_whitespace_inside_page_marker() {
        let chunks = SectionAwareSplitter::new(10, 0)
            .split("first text\n\n<!--   pai-page:7   -->\n\nsecond text");
        assert_eq!(chunks.len(), 3);
        assert_eq!(chunks[1].text, "second tex");
        assert_eq!(chunks[1].page, 7);
        assert_eq!(chunks[2].text, "t");
        assert_eq!(chunks[2].page, 7);
    }
}
