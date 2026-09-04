//! Project identity.

use pai_project::{ProjectKind, ProjectStore, SqliteProjectStore};
use rusqlite::Connection;
use tempfile::TempDir;

fn store() -> SqliteProjectStore {
    SqliteProjectStore::open_in_memory().expect("opens")
}

#[test]
fn two_ways_into_one_directory_are_one_project() {
    let dir = TempDir::new().expect("temp dir");
    let root = dir.path().canonicalize().expect("canonicalises");
    std::fs::create_dir_all(root.join("child")).expect("create a subdirectory");
    let store = store();

    let direct = store.touch(&root).expect("opens");
    // The same directory reached another way; without canonicalisation this becomes a second row.
    let round_about = store.touch(&root.join("child").join("..")).expect("opens");

    assert_eq!(direct.id, round_about.id);
    assert_eq!(store.list().expect("lists").len(), 1);
}

#[test]
fn reopening_moves_a_project_to_the_top() {
    let (a, b) = (TempDir::new().unwrap(), TempDir::new().unwrap());
    let store = store();
    let first = store.touch(a.path()).expect("opens");
    std::thread::sleep(std::time::Duration::from_millis(5));
    store.touch(b.path()).expect("opens");
    std::thread::sleep(std::time::Duration::from_millis(5));
    store.touch(a.path()).expect("reopens");

    // Most recent first — the order people think in when reopening a project.
    assert_eq!(store.list().expect("lists")[0].id, first.id);
}

#[test]
fn forgetting_a_project_never_touches_the_disk() {
    let dir = TempDir::new().expect("temp dir");
    let marker = dir.path().join("still-here.txt");
    std::fs::write(&marker, "còn").expect("write a file");
    let store = store();
    let project = store.touch(dir.path()).expect("opens");

    store.forget(&project.id).expect("forgets");
    assert!(store.list().expect("lists").is_empty());
    // Getting this wrong destroys the user's work.
    assert!(
        marker.exists(),
        "forgetting from the list deleted the directory"
    );
    assert!(
        store.forget(&project.id).is_err(),
        "forgetting twice has to be reported"
    );
}

#[test]
fn a_path_that_is_not_a_directory_is_refused() {
    let dir = TempDir::new().expect("temp dir");
    let file = dir.path().join("a.txt");
    std::fs::write(&file, "x").expect("write a file");
    assert!(store().touch(&file).is_err());
    assert!(store().touch(&dir.path().join("does-not-exist")).is_err());
}

/// An existing database lacks the two new columns and must survive: nothing can rebuild the project list.
#[test]
fn an_old_schema_gains_columns_in_place_and_loses_no_rows() {
    let conn = Connection::open_in_memory().expect("opens a connection");
    conn.execute_batch(
        "CREATE TABLE projects (
           id             TEXT    PRIMARY KEY,
           path           TEXT    NOT NULL UNIQUE,
           name           TEXT    NOT NULL,
           last_opened_at INTEGER NOT NULL
         ) STRICT;
         INSERT INTO projects VALUES ('mot', '/nha/mot', 'mot', 10);
         INSERT INTO projects VALUES ('hai', '/nha/hai', 'hai', 20);",
    )
    .expect("builds the old schema");

    let store = SqliteProjectStore::from_connection(conn).expect("opens on the old db");
    let rows = store.list().expect("lists");

    assert_eq!(rows.len(), 2, "the migration lost a row");
    for project in &rows {
        assert_eq!(
            project.kind,
            ProjectKind::Code,
            "old rows must be source projects"
        );
        assert_eq!(project.origin, None, "old rows were cloned from nowhere");
    }
    assert_eq!(rows[0].id, "hai", "the most recent still has to come first");
}

/// Reopening a document project must not turn it into a source project, as a careless `ON CONFLICT DO UPDATE` would.
#[test]
fn touch_preserves_the_kind_of_an_existing_row() {
    let dir = TempDir::new().expect("temp dir");
    let store = store();
    let created = store
        .create(
            dir.path(),
            ProjectKind::Docs,
            Some("https://vi.du/tai-lieu.git"),
        )
        .expect("creates");

    let reopened = store.touch(dir.path()).expect("reopens");

    assert_eq!(reopened.id, created.id, "it still has to be one project");
    assert_eq!(
        reopened.kind,
        ProjectKind::Docs,
        "reopening changed the kind"
    );
    assert_eq!(
        reopened.origin.as_deref(),
        Some("https://vi.du/tai-lieu.git"),
        "reopening forgot where it came from"
    );
    assert!(reopened.last_opened_at >= created.last_opened_at);
}

#[test]
fn create_and_list_return_the_right_kind_and_origin() {
    let (code, docs) = (TempDir::new().expect("temp"), TempDir::new().expect("temp"));
    let store = store();
    store
        .create(code.path(), ProjectKind::Code, None)
        .expect("creates");
    store
        .create(docs.path(), ProjectKind::Docs, Some("https://vi.du/x.git"))
        .expect("creates");

    let rows = store.list().expect("lists");
    let find = |kind| {
        rows.iter()
            .find(|project| project.kind == kind)
            .expect("must be present")
    };
    assert_eq!(find(ProjectKind::Code).origin, None);
    assert_eq!(
        find(ProjectKind::Docs).origin.as_deref(),
        Some("https://vi.du/x.git")
    );
    assert_eq!(rows.len(), 2);
}

/// Re-adding a cloned directory by hand must not erase its origin: the manual path does not know the URL.
#[test]
fn re_adding_by_hand_does_not_erase_the_origin() {
    let dir = TempDir::new().expect("temp dir");
    let store = store();
    store
        .create(dir.path(), ProjectKind::Code, Some("https://vi.du/x.git"))
        .expect("cloned in");

    let again = store
        .create(dir.path(), ProjectKind::Code, None)
        .expect("re-added by hand");

    assert_eq!(again.origin.as_deref(), Some("https://vi.du/x.git"));
}

/// Unlike `touch`, `create` takes a kind the user just stated, so the new one has to win.
#[test]
fn create_states_the_kind_explicitly_so_the_new_one_wins() {
    let dir = TempDir::new().expect("temp dir");
    let store = store();
    let first = store
        .create(dir.path(), ProjectKind::Docs, None)
        .expect("creates");
    let second = store
        .create(dir.path(), ProjectKind::Code, None)
        .expect("restates the kind");

    assert_eq!(second.id, first.id, "it still has to be one project");
    assert_eq!(second.kind, ProjectKind::Code);
}

/// `set_kind` is the only way out of a wrongly recorded kind, since `touch` preserves it.
#[test]
fn set_kind_is_the_way_out_of_the_dead_end() {
    let dir = TempDir::new().expect("temp dir");
    let store = store();
    let wrong = store
        .create(dir.path(), ProjectKind::Docs, None)
        .expect("creates");

    let fixed = store
        .set_kind(&wrong.id, ProjectKind::Code)
        .expect("the kind changes");
    assert_eq!(fixed.kind, ProjectKind::Code);
    assert_eq!(
        store.get(&wrong.id).expect("reads back").kind,
        ProjectKind::Code
    );

    // And reopening afterwards keeps the corrected kind.
    assert_eq!(
        store.touch(dir.path()).expect("reopens").kind,
        ProjectKind::Code
    );
}

/// Every path reports a missing id by name; a raw "query returned no rows" says nothing useful.
#[test]
fn a_nonexistent_id_is_named_in_every_error() {
    let store = store();
    for err in [
        store
            .get("no-such-id")
            .expect_err("must be an error")
            .to_string(),
        store
            .forget("no-such-id")
            .expect_err("must be an error")
            .to_string(),
        store
            .set_kind("no-such-id", ProjectKind::Docs)
            .expect_err("must be an error")
            .to_string(),
    ] {
        assert!(
            err.contains("no-such-id"),
            "the error does not name the id: {err}"
        );
    }
}

/// An unknown kind reads back as `code`: a wrong label is one click to fix, a dropped row loses a project.
#[test]
fn an_unknown_kind_in_the_database_reads_back_as_source() {
    let conn = Connection::open_in_memory().expect("opens a connection");
    conn.execute_batch(
        "CREATE TABLE projects (
           id             TEXT    PRIMARY KEY,
           path           TEXT    NOT NULL UNIQUE,
           name           TEXT    NOT NULL,
           last_opened_at INTEGER NOT NULL,
           kind           TEXT    NOT NULL DEFAULT 'code',
           origin         TEXT
         ) STRICT;
         INSERT INTO projects VALUES ('odd', '/home/odd', 'odd', 10, 'ban-do', NULL);",
    )
    .expect("builds a row with an unknown kind");

    let store = SqliteProjectStore::from_connection(conn).expect("opens");
    let rows = store
        .list()
        .expect("one unknown kind must not fail the whole call");

    assert_eq!(rows.len(), 1, "lost a row over an unreadable label");
    assert_eq!(rows[0].kind, ProjectKind::Code);
}
