//! Three tools, one rule for all of them: document content is untrusted input, not instructions.
//! All three declare `returns_untrusted_content`, so the registry injects the warning into
//! their descriptions, and all three are `read_only` — ingesting is a human drag-and-drop.

pub mod list;
pub mod read;
pub mod search;

use crate::library::Hit;

/// One chunk, rendered for the model; the `[title #ordinal]` prefix is what makes it citable.
pub(crate) fn render(hit: &Hit) -> String {
    let heading = match &hit.heading {
        Some(heading) => format!(" — {heading}"),
        None => String::new(),
    };
    format!("[{} #{}{}]\n{}", hit.title, hit.ordinal, heading, hit.text)
}
