//! Three tools, one rule for all of them: document content is untrusted input, not instructions.
//! All three declare `returns_untrusted_content`, so the registry injects the warning into
//! their descriptions, and all three are `read_only` — ingesting is a human drag-and-drop.

pub mod list;
pub mod read;
pub mod search;

use crate::library::Hit;

/// What one mount of the library calls itself. The same three tools serve a document project's library and a
/// code project's attachment shelf; only the names and the noun change. It is one struct rather than three
/// string constants because a description that names a sibling tool this mount does not have sends the model
/// to a tool that is not there.
#[derive(Clone, Copy, Debug)]
pub struct Vocab {
    pub search: &'static str,
    pub read: &'static str,
    pub list: &'static str,
    /// The collection, as a noun phrase a sentence can be built around.
    pub what: &'static str,
    /// One item of it.
    pub item: &'static str,
}

/// A document project's library: the folder the user chose.
pub const DOCS: Vocab = Vocab {
    search: "docs.search",
    read: "docs.read",
    list: "docs.list",
    what: "thư viện tài liệu của dự án",
    item: "tài liệu",
};

/// A code project's attachment shelf: the PDFs, images and DOCX files attached to its conversations, already
/// extracted. Named apart from `docs.*` so a code project's tool list never claims to hold the user's library.
pub const ATTACHMENTS: Vocab = Vocab {
    search: "attachment.search",
    read: "attachment.read",
    list: "attachment.list",
    what: "những tệp người dùng đã đính kèm vào cuộc trò chuyện",
    item: "tệp đính kèm",
};

/// One chunk, rendered for the model; the `[title #ordinal]` prefix is what makes it citable.
pub(crate) fn render(hit: &Hit) -> String {
    let heading = match &hit.heading {
        Some(heading) => format!(" — {heading}"),
        None => String::new(),
    };
    format!("[{} #{}{}]\n{}", hit.title, hit.ordinal, heading, hit.text)
}
