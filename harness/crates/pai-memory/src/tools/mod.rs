//! The four tools, and the rendering they share.
//!
//! Four instead of the nine the MCP server exposes. The merges are deliberate:
//! `create_entities` + `add_observations` + `create_relations` become one `memory.remember`,
//! because recording a single fact ("Vinh works at Acme") otherwise costs three round trips and
//! three tool descriptions in the prompt; and `read_graph` + `open_nodes` become one
//! `memory.read`, because they are the same query differing only by a WHERE clause. The three
//! `delete_*` calls become `memory.forget` for the same reason. What is *not* merged is reading
//! from writing: that line is what `ToolMeta::read_only` means, and a tool that sometimes writes
//! would have to declare itself mutating always.

use std::sync::Arc;

use parking_lot::Mutex;

use crate::graph::{Entity, Graph, Relation};

pub mod forget;
pub mod read;
pub mod remember;
pub mod search;

/// The handle the tools share. [`Graph`] is not internally synchronised (a write is a
/// transaction and needs `&mut`), so the lock lives here — one lock for one SQLite file.
pub type SharedGraph = Arc<Mutex<Graph>>;

/// Byte ceiling for anything rendered back to the model — `String::len` is bytes, and in
/// Vietnamese a byte budget is the conservative reading of a character budget. 20k bytes is
/// roughly 5k tokens: enough for a real slice of a graph, small enough that a memory read cannot
/// evict the conversation it exists to serve.
pub const MAX_CHARS: usize = 20_000;

/// How many observations of one entity are shown before the rest are summarised. An entity with
/// two hundred observations is a log, and a log does not belong in a search result.
pub const MAX_OBSERVATIONS: usize = 12;

/// Per-line ceiling for one observation. `memory.remember` refuses anything this long today, but
/// a file written by an older build — or by a hand-edited database — must not be able to hand the
/// model a megabyte on one line.
const MAX_OBSERVATION_BYTES: usize = 2_400;

/// Render entities and the edges among them, stopping at [`MAX_CHARS`].
///
/// Returns the text and how many entities actually fit; the caller says so out loud, because a
/// silently truncated graph reads to the model as a complete one.
pub fn render(entities: &[Entity], relations: &[Relation]) -> (String, usize) {
    let mut text = String::new();
    let mut shown = 0usize;

    for entity in entities {
        let mut block = String::new();
        let kind = if entity.kind.trim().is_empty() {
            String::new()
        } else {
            format!(" ({})", entity.kind)
        };
        block.push_str(&format!("## {}{}\n", clip(&entity.name, 200), kind));
        for body in &entity.observations {
            block.push_str(&format!("- {}\n", clip(body, MAX_OBSERVATION_BYTES)));
        }
        let hidden = entity
            .observations_total
            .saturating_sub(entity.observations.len() as i64);
        if hidden > 0 {
            block.push_str(&format!("- (còn {hidden} quan sát nữa chưa hiện)\n"));
        }
        if entity.observations.is_empty() && hidden == 0 {
            block.push_str("- (chưa có quan sát nào)\n");
        }
        block.push('\n');

        // Stop before the overflow, not after: half an entity is worse than one fewer entity.
        // The first block is the exception — dropping it would return nothing at all — so it is
        // clipped instead, which is why the ceiling holds even for one enormous entity.
        if text.len() + block.len() > MAX_CHARS {
            if !text.is_empty() {
                break;
            }
            text.push_str(&clip(&block, MAX_CHARS));
            shown += 1;
            break;
        }
        text.push_str(&block);
        shown += 1;
    }

    // Only edges between entities that actually made it into the text; anything else points at a
    // node the model cannot see.
    if shown > 0 && !relations.is_empty() {
        let visible: std::collections::HashSet<&str> = entities
            .iter()
            .take(shown)
            .map(|entity| entity.name.as_str())
            .collect();
        let lines: Vec<String> = relations
            .iter()
            .filter(|edge| {
                visible.contains(edge.from.as_str()) && visible.contains(edge.to.as_str())
            })
            .map(|edge| format!("- {} --{}--> {}", edge.from, edge.verb, edge.to))
            .collect();
        if !lines.is_empty() {
            let block = format!("### Quan hệ\n{}\n", lines.join("\n"));
            if text.len() + block.len() <= MAX_CHARS {
                text.push_str(&block);
            } else {
                text.push_str("### Quan hệ\n(đã bỏ qua vì kết quả đã đầy)\n");
            }
        }
    }

    (text.trim_end().to_string(), shown)
}

/// Cut `text` to at most `limit` bytes, at a character boundary, saying so when it cuts.
///
/// `String::truncate` panics on a boundary in the middle of a Vietnamese character, and this runs
/// on strings the model wrote, so the boundary walk is not optional.
fn clip(text: &str, limit: usize) -> std::borrow::Cow<'_, str> {
    if text.len() <= limit {
        return std::borrow::Cow::Borrowed(text);
    }
    const NOTICE: &str = "… (đã cắt)";
    // Leave room for the notice, so the result still fits the budget it was clipped to meet.
    let room = limit.saturating_sub(NOTICE.len());
    let mut end = room;
    while end > 0 && !text.is_char_boundary(end) {
        end -= 1;
    }
    std::borrow::Cow::Owned(format!("{}{NOTICE}", &text[..end]))
}

/// The cancellation check every tool body runs before taking the lock.
///
/// Only before: one call is one SQLite transaction over a batch that is already capped, so there
/// is no long-running loop to interleave a second check with. The point is the lock — a result
/// nobody will read must not queue behind, or ahead of, one that will.
pub fn cancelled(call: &pai_tools::Invocation) -> bool {
    call.cancel_token().is_cancelled()
}

#[cfg(test)]
mod tests {
    use super::*;
    use pai_tools::{Invocation, Tool, ToolError, ToolName};
    use serde_json::{Value, json};

    use crate::tools::forget::MemoryForget;
    use crate::tools::read::MemoryRead;
    use crate::tools::remember::MemoryRemember;
    use crate::tools::search::MemorySearch;

    fn shared() -> SharedGraph {
        Arc::new(Mutex::new(Graph::in_memory().expect("graph in memory")))
    }

    fn call(name: &str, arguments: Value) -> Invocation {
        let arguments = arguments
            .as_object()
            .cloned()
            .expect("tham số phải là một object");
        Invocation::new(ToolName::new(name), "test-call", arguments)
    }

    async fn run(tool: &dyn Tool, arguments: Value) -> String {
        let name = tool.schema().name.as_str().to_string();
        tool.execute(&call(&name, arguments))
            .await
            .expect("tool chạy được")
            .content
    }

    fn seed() -> Value {
        json!({
            "entities": [
                { "name": "Vinh", "kind": "người", "observations": ["Thích trả lời ngắn gọn"] },
                { "name": "Private AI", "kind": "dự án", "observations": ["Viết bằng Rust"] }
            ],
            "relations": [{ "from": "Vinh", "verb": "phát triển", "to": "Private AI" }]
        })
    }

    #[tokio::test]
    async fn write_then_search_then_forget() {
        let graph = shared();
        let remember = MemoryRemember::new(graph.clone());
        let search = MemorySearch::new(graph.clone());
        let read = MemoryRead::new(graph.clone());
        let forget = MemoryForget::new(graph.clone());

        let written = run(&remember, seed()).await;
        assert!(written.contains("2 thực thể mới"), "{written}");
        assert!(written.contains("1 quan hệ mới"), "{written}");

        let found = run(&search, json!({ "query": "Vinh" })).await;
        assert!(found.contains("## Vinh (người)"), "{found}");
        assert!(!found.contains("đã cắt"), "{found}");

        // Both ends are in the read, so the edge must show up.
        let whole = run(&read, json!({})).await;
        assert!(whole.contains("Vinh --phát triển--> Private AI"), "{whole}");

        let gone = run(&forget, json!({ "entities": ["Vinh"] })).await;
        assert!(gone.contains("1 thực thể"), "{gone}");
        let after = run(&search, json!({ "query": "Vinh" })).await;
        assert!(after.contains("Không có gì khớp"), "{after}");
        assert!(
            after.contains("1 thực thể"),
            "trả lời phải nói đồ thị còn gì: {after}"
        );
    }

    #[tokio::test]
    async fn read_by_name_says_what_to_do_when_the_name_is_wrong() {
        let graph = shared();
        run(&MemoryRemember::new(graph.clone()), seed()).await;
        let miss = run(&MemoryRead::new(graph), json!({ "names": ["Vihn"] })).await;
        assert!(miss.contains("Không tìm thấy"), "{miss}");
        assert!(miss.contains("memory.search"), "{miss}");
    }

    #[tokio::test]
    async fn a_big_graph_is_truncated_and_says_so() {
        let graph = shared();
        let remember = MemoryRemember::new(graph.clone());
        // Enough text that the render budget bites well before the entity limit does.
        let bodies: Vec<String> = (0..30)
            .map(|n| format!("Một câu sự thật khá dài để chiếm chỗ, số {n}, viết dài ra cho đủ."))
            .collect();
        for n in 0..40 {
            run(
                &remember,
                json!({ "entities": [{ "name": format!("Thực thể {n}"), "kind": "thử", "observations": bodies }] }),
            )
            .await;
        }

        let text = run(&MemoryRead::new(graph.clone()), json!({ "limit": 100 })).await;
        assert!(
            text.len() <= MAX_CHARS + 500,
            "quá trần: {} ký tự",
            text.len()
        );
        assert!(text.contains("đã cắt"), "phải nói rõ là đã cắt");
        // Per-entity observations are capped at the query, not just at render time.
        assert!(
            text.contains("còn 18 quan sát nữa chưa hiện"),
            "{}",
            text.chars().take(400).collect::<String>()
        );

        let hits = run(
            &MemorySearch::new(graph),
            json!({ "query": "sự thật", "limit": 40 }),
        )
        .await;
        assert!(
            hits.len() <= MAX_CHARS + 500,
            "quá trần: {} ký tự",
            hits.len()
        );
    }

    #[tokio::test]
    async fn a_partly_wrong_name_list_names_the_misses() {
        let graph = shared();
        run(&MemoryRemember::new(graph.clone()), seed()).await;
        let mixed = run(
            &MemoryRead::new(graph),
            json!({ "names": ["Vinh", "Vihn", "Không Tồn Tại"] }),
        )
        .await;
        // The hit is still rendered, and the two misses must not be silent.
        assert!(mixed.contains("## Vinh (người)"), "{mixed}");
        assert!(mixed.contains("Vihn"), "{mixed}");
        assert!(mixed.contains("Không Tồn Tại"), "{mixed}");
    }

    #[tokio::test]
    async fn one_enormous_observation_cannot_get_in_or_out() {
        let graph = shared();
        // In: the write is refused rather than trimmed, so a half-fact is never stored.
        let err = MemoryRemember::new(graph.clone())
            .execute(&call(
                MemoryRemember::NAME,
                json!({ "entities": [{
                    "name": "Nhật ký",
                    "kind": "ghi chép",
                    "observations": ["Tiếng Việt có dấu ".repeat(4_000)],
                }] }),
            ))
            .await
            .expect_err("quan sát quá dài phải bị từ chối");
        assert!(matches!(err, ToolError::Invalid(_)), "{err}");
        assert_eq!(graph.lock().stats().expect("stats").observations, 0);

        // Out: a graph written by an older build still has to fit the ceiling, and clipping a
        // multi-byte string at a byte offset must not panic. One huge line exercises the per-line
        // clip; a full page of them exercises the whole-block clip, which is the only place the
        // very first entity can be cut.
        let long = "Tiếng Việt có dấu ".repeat(10_000);
        let page: Vec<String> = (0..MAX_OBSERVATIONS)
            .map(|n| format!("{n} {}", "Tiếng Việt có dấu ".repeat(120)))
            .collect();
        graph
            .lock()
            .remember(
                &[
                    crate::graph::EntityInput {
                        name: "Nhật ký".to_string(),
                        kind: "ghi chép".to_string(),
                        observations: vec![long],
                    },
                    crate::graph::EntityInput {
                        name: "Sổ tay".to_string(),
                        kind: "ghi chép".to_string(),
                        observations: page,
                    },
                ],
                &[],
            )
            .expect("ghi thẳng qua graph");
        let text = run(&MemoryRead::new(graph), json!({})).await;
        // The body is clipped to `MAX_CHARS`; the slack is the "here is what was cut" line the
        // caller appends afterwards, which is the whole point of clipping out loud.
        assert!(
            text.len() <= MAX_CHARS + 500,
            "quá trần: {} byte",
            text.len()
        );
        assert!(
            text.contains("đã cắt"),
            "không thấy dấu cắt trong {} byte",
            text.len()
        );
    }

    #[tokio::test]
    async fn oversized_batches_are_refused_before_the_lock() {
        let graph = shared();
        let flood: Vec<String> = (0..600).map(|n| format!("Câu số {n}")).collect();
        let err = MemoryRemember::new(graph.clone())
            .execute(&call(
                MemoryRemember::NAME,
                json!({ "entities": [{ "name": "Ai Đó", "observations": flood }] }),
            ))
            .await
            .expect_err("600 quan sát trong một lần gọi phải bị từ chối");
        assert!(matches!(err, ToolError::Invalid(_)), "{err}");
        // `MAX_ITEMS` alone would have let this through, so nothing may have been written.
        assert_eq!(graph.lock().stats().expect("stats").entities, 0);

        // Names go into one `IN (...)`; past SQLite's variable limit that is an unreadable error.
        let names: Vec<String> = (0..500).map(|n| format!("Tên {n}")).collect();
        let err = MemoryRead::new(graph)
            .execute(&call(MemoryRead::NAME, json!({ "names": names })))
            .await
            .expect_err("500 tên phải bị từ chối");
        assert!(matches!(err, ToolError::Invalid(_)), "{err}");
    }

    #[tokio::test]
    async fn the_newest_observations_are_the_ones_that_survive() {
        let graph = shared();
        let remember = MemoryRemember::new(graph.clone());
        for n in 0..40 {
            run(
                &remember,
                json!({ "entities": [{ "name": "Vinh", "observations": [format!("Sự thật số {n}")] }] }),
            )
            .await;
        }
        let text = run(&MemoryRead::new(graph), json!({ "names": ["Vinh"] })).await;
        // A fact learned last must be readable; a memory that can only recall its first dozen
        // sentences is worse than no memory at all.
        assert!(text.contains("Sự thật số 39"), "{text}");
        assert!(!text.contains("Sự thật số 0\n"), "{text}");
        assert!(text.contains("còn 28 quan sát nữa chưa hiện"), "{text}");
    }

    #[tokio::test]
    async fn empty_and_malformed_arguments_are_rejected_not_guessed() {
        let graph = shared();
        let remember = MemoryRemember::new(graph.clone());
        let err = remember
            .execute(&call(MemoryRemember::NAME, json!({})))
            .await
            .expect_err("gọi rỗng phải lỗi");
        assert!(matches!(err, ToolError::Invalid(_)), "{err}");

        let err = MemorySearch::new(graph.clone())
            .execute(&call(MemorySearch::NAME, json!({ "query": "   " })))
            .await
            .expect_err("query trống phải lỗi");
        assert!(matches!(err, ToolError::Invalid(_)), "{err}");

        let err = MemoryForget::new(graph)
            .execute(&call(MemoryForget::NAME, json!({ "entities": "Vinh" })))
            .await
            .expect_err("kiểu sai phải lỗi");
        assert!(matches!(err, ToolError::Invalid(_)), "{err}");
    }

    #[tokio::test]
    async fn a_cancelled_call_does_not_touch_the_graph() {
        let graph = shared();
        let remember = MemoryRemember::new(graph.clone());
        let invocation = call(MemoryRemember::NAME, seed());
        invocation.cancel_token().cancel();

        let err = remember
            .execute(&invocation)
            .await
            .expect_err("lệnh đã huỷ phải lỗi");
        assert!(matches!(err, ToolError::Failed(_)), "{err}");
        assert_eq!(graph.lock().stats().expect("stats").entities, 0);
    }

    #[test]
    fn meta_says_what_each_tool_really_does() {
        let graph = shared();
        for meta in [
            MemorySearch::new(graph.clone()).meta(),
            MemoryRead::new(graph.clone()).meta(),
        ] {
            assert!(!meta.mutating, "tool đọc không được khai là mutating");
        }
        for meta in [
            MemoryRemember::new(graph.clone()).meta(),
            MemoryForget::new(graph.clone()).meta(),
        ] {
            assert!(meta.mutating, "tool ghi phải khai mutating");
        }
        // Nothing here reaches the network, and nothing here is written by a stranger.
        for meta in [
            MemorySearch::new(graph.clone()).meta(),
            MemoryRead::new(graph.clone()).meta(),
            MemoryRemember::new(graph.clone()).meta(),
            MemoryForget::new(graph).meta(),
        ] {
            assert!(!meta.leaves_device);
            assert!(!meta.returns_untrusted_content);
        }
    }

    #[test]
    fn tool_names_survive_the_wire_encoding() {
        // A `__` in a name makes the dotted form unrecoverable and the registry refuses it, so a
        // renamed tool must fail here rather than silently vanish at startup.
        let graph = shared();
        for name in [
            MemorySearch::new(graph.clone()).schema().name,
            MemoryRead::new(graph.clone()).schema().name,
            MemoryRemember::new(graph.clone()).schema().name,
            MemoryForget::new(graph).schema().name,
        ] {
            assert!(name.round_trips(), "{name} không mã hoá ngược được");
        }
    }
}
