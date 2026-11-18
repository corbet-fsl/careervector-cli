use std::fs;
use std::path::Path;

use super::*;
use crate::init::{Git, Manifest, init};

fn release<'a>(version: &'a str, schema: u64, files: &[(&'a str, &'a str)]) -> Release<'a> {
    Release {
        version,
        schema,
        files: files.iter().map(|(p, t)| (*p, t.as_bytes())).collect(),
    }
}

const CV9: &str = r#"{"schema_version": 9, "n": 1}"#;
const CV9B: &str = r#"{"schema_version": 9, "n": 2}"#;
static NONE: [Migration; 0] = [];

fn run(root: &Path, to: &Release<'_>, migrations: &'static [Migration]) -> Plan {
    let plan = plan(root, to, migrations).unwrap();
    apply(root, to, migrations, &plan).unwrap();
    plan
}

fn read(root: &Path, path: &str) -> String {
    fs::read_to_string(root.join(path)).unwrap()
}

#[test]
fn an_edited_workspace_keeps_every_edit_and_takes_every_untouched_update() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let v1 = release(
        "1.0.0",
        9,
        &[
            ("cv.json", CV9),
            ("inputs/profile.toml", "name = 'a'\n"),
            ("styles/s.typ", "#let s = 1\n"),
            ("opportunities/o/application.toml", "k = 1\n"),
            ("scaffold/x.md", "x\n"),
            ("docs/a.md", "docs v1\n"),
            ("docs/old.md", "old\n"),
            ("docs/old-edited.md", "old2\n"),
        ],
    );
    init(root, &v1, false, Git::No).unwrap();
    // The user edits three files and deletes one scaffold; they also edit a doc
    // that upstream is about to drop.
    fs::write(root.join("inputs/profile.toml"), "name = 'mine'\n").unwrap();
    fs::write(root.join("styles/s.typ"), "#let s = 99\n").unwrap();
    fs::write(root.join("opportunities/o/application.toml"), "k = 7\n").unwrap();
    fs::remove_file(root.join("scaffold/x.md")).unwrap();
    fs::write(root.join("docs/old-edited.md"), "mine\n").unwrap();

    let v2 = release(
        "1.1.0",
        9,
        &[
            ("cv.json", CV9B),
            ("inputs/profile.toml", "name = 'a2'\n"),
            ("styles/s.typ", "#let s = 2\n"),
            ("opportunities/o/application.toml", "k = 1\n"), // unchanged upstream
            ("scaffold/x.md", "x2\n"),
            ("docs/a.md", "docs v2\n"),
            ("docs/new.md", "new\n"),
        ],
    );
    let dry = plan(root, &v2, &NONE).unwrap();
    assert!(
        !root.join("docs/new.md").exists(),
        "a dry run writes nothing"
    );
    assert_eq!(read(root, "docs/a.md"), "docs v1\n");
    assert!(dry.items.contains(&("docs/a.md".into(), Action::Replace)));

    run(root, &v2, &NONE);
    // Untouched files updated; additions and removals applied.
    assert_eq!(read(root, "cv.json"), CV9B);
    assert_eq!(read(root, "docs/a.md"), "docs v2\n");
    assert_eq!(read(root, "docs/new.md"), "new\n");
    assert!(!root.join("docs/old.md").exists());
    // Edits kept, the new version beside them.
    assert_eq!(read(root, "inputs/profile.toml"), "name = 'mine'\n");
    assert_eq!(read(root, "inputs/profile.toml.cv-new"), "name = 'a2'\n");
    assert_eq!(read(root, "styles/s.typ"), "#let s = 99\n");
    assert_eq!(read(root, "styles/s.typ.cv-new"), "#let s = 2\n");
    assert_eq!(read(root, "opportunities/o/application.toml"), "k = 7\n");
    assert!(
        !root
            .join("opportunities/o/application.toml.cv-new")
            .exists()
    );
    // The deleted scaffold stays deleted; the edited dropped doc stays mine.
    assert!(!root.join("scaffold/x.md").exists());
    assert_eq!(read(root, "docs/old-edited.md"), "mine\n");
    let manifest = Manifest::load(root).unwrap().unwrap();
    assert_eq!(manifest.cv_version, "1.1.0");
    assert!(manifest.deleted.contains("scaffold/x.md"));
    assert!(!manifest.files.contains_key("docs/old-edited.md"));

    // A second update to the same version changes nothing and loses nothing.
    let again = run(root, &v2, &NONE);
    assert!(
        again
            .items
            .iter()
            .all(|(_, a)| matches!(a, Action::KeepDeleted | Action::KeepWriteNew))
    );
    assert_eq!(read(root, "inputs/profile.toml"), "name = 'mine'\n");
    assert!(!root.join("scaffold/x.md").exists());
}

#[test]
fn merging_the_new_version_by_hand_ends_the_conflict() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let v1 = release("1.0.0", 9, &[("cv.json", CV9), ("a.toml", "1\n")]);
    init(root, &v1, false, Git::No).unwrap();
    fs::write(root.join("a.toml"), "mine\n").unwrap();
    let v2 = release("1.1.0", 9, &[("cv.json", CV9), ("a.toml", "2\n")]);
    run(root, &v2, &NONE);
    fs::rename(root.join("a.toml.cv-new"), root.join("a.toml")).unwrap();
    run(root, &v2, &NONE);
    let manifest = Manifest::load(root).unwrap().unwrap();
    assert_eq!(manifest.files["a.toml"], sha256_hex(b"2\n"));
}

#[test]
fn a_fork_without_a_manifest_is_adopted_and_loses_no_edit() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    fs::write(root.join("cv.json"), CV9).unwrap();
    fs::create_dir_all(root.join(".agent/src")).unwrap();
    fs::write(root.join(".agent/src/main.rs"), "fn main() {}").unwrap();
    fs::write(root.join("same.toml"), "s\n").unwrap();
    fs::write(root.join("edited.toml"), "my edit\n").unwrap();
    let v = release(
        "1.0.0",
        9,
        &[
            ("cv.json", CV9),
            ("same.toml", "s\n"),
            ("edited.toml", "upstream\n"),
            ("fresh.toml", "f\n"),
        ],
    );
    let plan = run(root, &v, &NONE);
    assert!(plan.adopted);
    assert_eq!(read(root, "edited.toml"), "my edit\n");
    assert_eq!(read(root, "edited.toml.cv-new"), "upstream\n");
    assert_eq!(read(root, "fresh.toml"), "f\n");
    assert_eq!(
        read(root, ".agent/src/main.rs"),
        "fn main() {}",
        "the fork's own code is never touched"
    );
    let manifest = Manifest::load(root).unwrap().unwrap();
    assert!(manifest.files.contains_key("same.toml"));
    assert!(!manifest.files.contains_key("edited.toml"));
}

#[test]
fn outside_a_workspace_update_says_so() {
    let dir = tempfile::tempdir().unwrap();
    let error = find_workspace(dir.path()).unwrap_err().to_string();
    assert!(error.contains("not inside a cv workspace"), "{error}");
    assert!(error.contains("cv init"), "{error}");
}

#[test]
fn a_workspace_written_by_a_newer_cv_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let newer = release("2.0.0", 9, &[("cv.json", CV9), ("a.toml", "1\n")]);
    init(root, &newer, false, Git::No).unwrap();
    let older = release("1.9.0", 9, &[("cv.json", CV9), ("a.toml", "0\n")]);
    let error = plan(root, &older, &NONE).unwrap_err().to_string();
    assert!(error.contains("newer"), "{error}");
    assert_eq!(read(root, "a.toml"), "1\n");
}

#[test]
fn a_workspace_with_a_newer_schema_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    fs::write(root.join("cv.json"), r#"{"schema_version": 12}"#).unwrap();
    let v = release("1.0.0", 9, &[("cv.json", CV9)]);
    let error = plan(root, &v, &NONE).unwrap_err().to_string();
    assert!(error.contains("schema 12"), "{error}");
}

fn migrate_nine(root: &Path) -> Result<()> {
    let path = root.join("inputs.txt");
    let count: u32 = fs::read_to_string(&path).map_or(0, |t| t.trim().parse().unwrap());
    fs::write(path, format!("{}\n", count + 1))?;
    Ok(())
}
static NINE_TO_TEN: [Migration; 1] = [Migration {
    from: 9,
    name: "nine-to-ten",
    apply: migrate_nine,
}];

#[test]
fn migrations_run_forward_once_and_are_idempotent() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let v1 = release("1.0.0", 9, &[("cv.json", CV9)]);
    init(root, &v1, false, Git::No).unwrap();
    let v2 = release(
        "1.1.0",
        10,
        &[("cv.json", r#"{"schema_version": 10, "n": 1}"#)],
    );
    let dry = plan(root, &v2, &NINE_TO_TEN).unwrap();
    assert_eq!(dry.migrations, ["nine-to-ten"]);
    assert!(
        !root.join("inputs.txt").exists(),
        "a dry run migrates nothing"
    );
    run(root, &v2, &NINE_TO_TEN);
    assert_eq!(read(root, "inputs.txt"), "1\n");
    assert_eq!(workspace_schema(root).unwrap(), 10);
    run(root, &v2, &NINE_TO_TEN);
    assert_eq!(
        read(root, "inputs.txt"),
        "1\n",
        "already migrated: not run again"
    );
}

#[test]
fn a_kept_cv_json_is_still_migrated_in_place() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let v1 = release("1.0.0", 9, &[("cv.json", CV9)]);
    init(root, &v1, false, Git::No).unwrap();
    fs::write(
        root.join("cv.json"),
        "{\n  \"schema_version\": 9,\n  \"mine\": true\n}\n",
    )
    .unwrap();
    let v2 = release("1.1.0", 10, &[("cv.json", r#"{"schema_version": 10}"#)]);
    run(root, &v2, &NINE_TO_TEN);
    assert_eq!(
        read(root, "cv.json"),
        "{\n  \"schema_version\": 10,\n  \"mine\": true\n}\n"
    );
    assert!(root.join("cv.json.cv-new").exists());
}

#[test]
fn a_missing_migration_is_an_error_not_a_guess() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    fs::write(root.join("cv.json"), CV9).unwrap();
    let v = release("1.1.0", 11, &[("cv.json", r#"{"schema_version": 11}"#)]);
    assert!(plan(root, &v, &NINE_TO_TEN).is_err());
}
