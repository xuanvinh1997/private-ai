//! Composing config layers.
//!
//! The plugin tree decides what the application consists of, so a silent failure here
//! shows up as "that feature disappeared" rather than as an error message. Which is why
//! every operation that does not match is a named error, not a skipped step.

use pai_core::{ConfigError, Layer, Patch, Row, compose};
use serde_json::json;

fn row(id: &str, plugin: &str) -> Row {
    Row {
        id: id.into(),
        plugin: plugin.into(),
        config: json!({}),
        disabled: false,
    }
}

#[test]
fn an_upper_layer_replaces_the_whole_config_block_rather_than_merging() {
    let base = Layer::base(
        "base",
        vec![Row {
            config: json!({ "a": 1, "b": 2 }),
            ..row("fs", "fs")
        }],
    );
    let user = Layer::new(
        "user",
        vec![Patch::Replace {
            id: "fs".into(),
            config: json!({ "a": 9 }),
        }],
    );

    let composed = compose(&[base, user]).expect("composes");
    // With a merge, `b` survives and the patch author has no way to remove it.
    assert_eq!(composed.rows[0].config, json!({ "a": 9 }));
}

#[test]
fn disabling_does_not_delete_and_stays_visible_in_the_dump() {
    let base = Layer::base("base", vec![row("shell", "shell"), row("fs", "fs")]);
    let user = Layer::new("user", vec![Patch::Disable { id: "shell".into() }]);

    let composed = compose(&[base, user]).expect("composes");
    assert_eq!(composed.active().count(), 1);
    // A missing row quietly comes back the day somebody reorders the layers; a disabled
    // row is always visible.
    //
    // The `[tắt]` marker is deliberately Vietnamese: `settings/harness.ts` parses this
    // exact string, so it is a wire contract rather than display text.
    assert!(composed.dump().contains("shell: shell [tắt]"));
}

#[test]
fn the_dump_says_who_touched_which_row() {
    let base = Layer::base("base.yaml", vec![row("fs", "fs")]);
    let mid = Layer::new(
        "profile.yaml",
        vec![Patch::Replace {
            id: "fs".into(),
            config: json!({ "roots": [] }),
        }],
    );
    let user = Layer::new("home.yaml", vec![Patch::Disable { id: "fs".into() }]);

    let composed = compose(&[base, mid, user]).expect("composes");
    let dump = composed.dump();
    assert!(
        dump.contains("base.yaml → profile.yaml → home.yaml"),
        "missing trail:\n{dump}"
    );
}

#[test]
fn inserting_a_duplicate_id_is_an_error_not_a_silent_overwrite() {
    let base = Layer::base("base", vec![row("fs", "fs")]);
    let other = Layer::base("other", vec![row("fs", "fs-other")]);

    // Whoever wrote the layer almost certainly meant `replace`; swallowing this leaves them
    // hunting all afternoon for why their config has no effect.
    let err = compose(&[base, other]).expect_err("must be an error");
    assert!(matches!(err, ConfigError::Duplicate { .. }), "{err}");
}

#[test]
fn targeting_a_row_that_does_not_exist_is_a_named_error() {
    let base = Layer::base("base", vec![row("fs", "fs")]);
    let user = Layer::new("user", vec![Patch::Disable { id: "shel".into() }]);

    let err = compose(&[base, user]).expect_err("must be an error");
    // The error has to name both the layer and the string that was typed — a one-character
    // typo is ordinary, and "nothing happened" is the worst possible answer to it.
    let text = err.to_string();
    assert!(text.contains("user") && text.contains("shel"), "{text}");
}

#[test]
fn row_order_follows_insertion_order() {
    let base = Layer::base("base", vec![row("a", "a"), row("b", "b")]);
    let more = Layer::base("more", vec![row("c", "c")]);

    let composed = compose(&[base, more]).expect("composes");
    let ids: Vec<&str> = composed.rows.iter().map(|row| row.id.as_str()).collect();
    assert_eq!(ids, vec!["a", "b", "c"]);
}
