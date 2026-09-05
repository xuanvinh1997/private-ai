//! Bytes to text.
//!
//! A server that lies about its charset, or says nothing at all, is the common case rather than
//! the exception, so the decision is a ladder and the rungs are ordered by how much they can be
//! trusted: a BOM is a fact about the bytes, the header is a claim by the server, a `<meta>` is a
//! claim by the author, and UTF-8 is the guess that is right most of the time. Nothing here ever
//! fails: `encoding_rs` substitutes U+FFFD and says it did, which is a better answer for a model
//! than an error about byte 0x93.

use encoding_rs::{Encoding, UTF_8};

/// How far into the bytes a `<meta charset>` is worth looking for. The HTML spec's own prescan
/// stops at 1024 bytes, but a modern `<head>` full of preload links routinely pushes the meta tag
/// past that, and 4 KiB is still nothing to scan.
const META_PRESCAN: usize = 4096;

/// The result of decoding, plus enough about how it was decoded to explain a bad result.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Decoded {
    pub text: String,
    /// The encoding actually used, by canonical name; surfaced so a mojibake report names a suspect.
    pub encoding: &'static str,
    /// True when at least one byte sequence was replaced by U+FFFD.
    pub lossy: bool,
}

/// Decode `bytes`, with `declared` being the `charset` the server claimed, if any.
pub fn decode(bytes: &[u8], declared: Option<&str>) -> Decoded {
    let encoding = Encoding::for_bom(bytes)
        .map(|(encoding, _)| encoding)
        .or_else(|| declared.and_then(|label| Encoding::for_label(label.as_bytes())))
        .or_else(|| sniff_meta(bytes))
        .unwrap_or(UTF_8);
    // `decode` strips a leading BOM itself, so the BOM never reaches the model as U+FEFF.
    let (text, _, lossy) = encoding.decode(bytes);
    Decoded {
        text: text.into_owned(),
        encoding: encoding.name(),
        lossy,
    }
}

/// Look for a `charset=` the document declares about itself.
///
/// Deliberately not a parser: the prefix is scanned for the literal word, because at this point
/// the bytes cannot be decoded yet and so cannot be handed to an HTML parser. Every `charset=`
/// occurrence is tried in turn rather than only the first, since the first hit is sometimes inside
/// a `Content-Security-Policy` or a comment.
fn sniff_meta(bytes: &[u8]) -> Option<&'static Encoding> {
    let limit = bytes.len().min(META_PRESCAN);
    // Lossy is fine: every label worth finding is ASCII, and ASCII survives any of these encodings.
    let head = String::from_utf8_lossy(&bytes[..limit]).to_ascii_lowercase();
    let mut rest = head.as_str();
    loop {
        let at = rest.find("charset")?;
        rest = &rest[at + "charset".len()..];
        let Some(after) = rest.trim_start().strip_prefix('=') else {
            continue;
        };
        let value = after.trim_start().trim_start_matches(['"', '\'']);
        let end = value
            .find(|c: char| !(c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == ':'))
            .unwrap_or(value.len());
        if end == 0 {
            continue;
        }
        if let Some(encoding) = Encoding::for_label(&value.as_bytes()[..end]) {
            return Some(encoding);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bom_thang_moi_khai_bao_sai() {
        // UTF-8 BOM in front of text the server wrongly calls latin-1.
        let mut bytes = vec![0xEF, 0xBB, 0xBF];
        bytes.extend_from_slice("Xin chào".as_bytes());
        let decoded = decode(&bytes, Some("windows-1252"));
        assert_eq!(decoded.text, "Xin chào");
        assert_eq!(decoded.encoding, "UTF-8");
    }

    #[test]
    fn dung_charset_may_chu_khai() {
        // 0xE9 is `é` in windows-1252 and invalid on its own in UTF-8.
        let bytes = [b'c', b'a', b'f', 0xE9];
        let decoded = decode(&bytes, Some("windows-1252"));
        assert_eq!(decoded.text, "café");
        assert!(!decoded.lossy);
    }

    #[test]
    fn doc_meta_khi_header_im_lang() {
        let mut bytes = b"<html><head><meta charset=\"windows-1252\"></head><body>caf".to_vec();
        bytes.push(0xE9);
        bytes.extend_from_slice(b"</body></html>");
        let decoded = decode(&bytes, None);
        assert_eq!(decoded.encoding, "windows-1252");
        assert!(decoded.text.contains("café"));
    }

    #[test]
    fn khong_biet_gi_thi_mac_dinh_utf8_va_bao_mat_mat() {
        let decoded = decode(&[b'a', 0xFF, b'b'], None);
        assert_eq!(decoded.encoding, "UTF-8");
        assert!(decoded.lossy);
        assert!(decoded.text.contains('\u{FFFD}'));
    }
}
