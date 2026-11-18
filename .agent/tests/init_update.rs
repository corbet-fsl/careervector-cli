//! User stories for `cv init` and `cv update`, driven through the built binary
//! with the real embedded file set.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use serde_json::Value;

const BIN: &str = env!("CARGO_BIN_EXE_cv");

fn scratch() -> tempfile::TempDir {
    let base = std::env::var_os("TMPDIR").map_or_else(std::env::temp_dir, PathBuf::from);
    tempfile::Builder::new()
        .prefix("cv-init-update-")
        .tempdir_in(base)
        .unwrap()
}

fn cv(directory: &Path, arguments: &[&str]) -> Output {
    Command::new(BIN)
        .args(arguments)
        .current_dir(directory)
        .output()
        .unwrap()
}

fn text(output: &Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    )
}

fn init(parent: &Path, name: &str) -> PathBuf {
    let output = cv(parent, &["init", name, "--no-git", "--no-build"]);
    assert!(output.status.success(), "{}", text(&output));
    parent.join(name)
}

fn manifest(root: &Path) -> Value {
    serde_json::from_str(&fs::read_to_string(root.join(".cv/manifest.json")).unwrap()).unwrap()
}

#[test]
fn a_new_user_inits_and_builds_the_showcase_offline_without_tool_sources() {
    let dir = scratch();
    let output = cv(dir.path(), &["init", "my-cv", "--no-git"]);
    assert!(output.status.success(), "{}", text(&output));
    let root = dir.path().join("my-cv");
    let pdf = root.join("outputs/cvl/cv/harvard/d-plus/en/ch/pdf/cv-4.pdf");
    assert!(pdf.is_file(), "a plain init renders the showcase");
    let recorded = manifest(&root);
    assert!(
        recorded["files"]
            .as_object()
            .unwrap()
            .keys()
            .all(|p| !p.contains("/pdf/")),
        "renders stay out of the manifest"
    );
    fs::remove_dir_all(root.join("outputs/cvl/cv/harvard/d-plus/en/ch/pdf")).unwrap();
    assert!(root.join("inputs/profile.toml").is_file());
    assert!(root.join("cv.json").is_file());
    assert!(!root.join(".agent/src").exists(), "the binary is the tool");
    assert!(!root.join("Cargo.toml").exists());
    assert!(!root.join(".git").exists(), "init never starts git unasked");
    assert!(!pdf.exists(), "--no-build style: nothing before a build");
    let build = cv(&root, &["build"]);
    assert!(build.status.success(), "{}", text(&build));
    assert!(pdf.is_file());
}

#[test]
fn an_edited_workspace_updates_with_no_edit_lost() {
    let dir = scratch();
    let root = init(dir.path(), "ws");
    let recorded = manifest(&root);
    let files = recorded["files"].as_object().unwrap();
    assert!(files.len() > 100);
    // Edit a record, a style and an opportunity file; delete a scaffold.
    let profile = root.join("inputs/profile.toml");
    fs::write(
        &profile,
        format!("{}\n# my edit\n", fs::read_to_string(&profile).unwrap()),
    )
    .unwrap();
    let scaffold = root.join(".agent/scaffolds/opportunity/outcome.md");
    fs::remove_file(&scaffold).unwrap();
    let before = fs::read(&profile).unwrap();

    let dry = cv(&root, &["update"]);
    assert!(dry.status.success(), "{}", text(&dry));
    assert!(text(&dry).contains("nothing was changed"));
    let apply = cv(&root, &["update", "--apply"]);
    assert!(apply.status.success(), "{}", text(&apply));
    assert_eq!(fs::read(&profile).unwrap(), before);
    assert!(!scaffold.exists(), "a deleted scaffold does not come back");
    assert!(
        !root.join("inputs/profile.toml.cv-new").exists(),
        "same version: nothing newer to offer"
    );
    assert!(
        manifest(&root)["deleted"]
            .as_array()
            .unwrap()
            .iter()
            .any(|path| path == ".agent/scaffolds/opportunity/outcome.md")
    );
}

#[test]
fn a_fork_style_checkout_without_a_manifest_is_adopted_and_keeps_its_edit() {
    let dir = scratch();
    let root = init(dir.path(), "fork");
    fs::remove_dir_all(root.join(".cv")).unwrap();
    let profile = root.join("inputs/profile.toml");
    fs::write(&profile, "schema_version = 1\n# a fork edit\n").unwrap();
    // The fork carries its own tool sources, which update never touches.
    fs::create_dir_all(root.join(".agent/src")).unwrap();
    fs::write(root.join(".agent/src/main.rs"), "fn main() {}\n").unwrap();
    let output = cv(&root, &["update", "--apply"]);
    assert!(output.status.success(), "{}", text(&output));
    assert!(text(&output).contains("adopting"));
    assert_eq!(
        fs::read_to_string(&profile).unwrap(),
        "schema_version = 1\n# a fork edit\n"
    );
    assert!(root.join("inputs/profile.toml.cv-new").is_file());
    assert_eq!(
        fs::read_to_string(root.join(".agent/src/main.rs")).unwrap(),
        "fn main() {}\n"
    );
    let after = manifest(&root);
    assert!(after["files"].get("cv.json").is_some());
    assert!(after["files"].get("inputs/profile.toml").is_none());
}

#[test]
fn the_wrong_directory_is_refused_both_ways() {
    let dir = scratch();
    let busy = dir.path().join("busy");
    fs::create_dir(&busy).unwrap();
    fs::write(busy.join("notes.txt"), "mine").unwrap();
    let refused = cv(dir.path(), &["init", "busy", "--no-git"]);
    assert!(!refused.status.success());
    assert!(text(&refused).contains("not empty"), "{}", text(&refused));
    assert!(!busy.join("cv.json").exists());

    let nowhere = dir.path().join("nowhere");
    fs::create_dir(&nowhere).unwrap();
    let update = cv(&nowhere, &["update"]);
    assert!(!update.status.success());
    assert!(
        text(&update).contains("not inside a cv workspace"),
        "{}",
        text(&update)
    );
}

#[test]
fn a_workspace_written_by_a_newer_cv_is_refused() {
    let dir = scratch();
    let root = init(dir.path(), "ws");
    let path = root.join(".cv/manifest.json");
    let mut value = manifest(&root);
    value["cv_version"] = Value::from("99.0.0");
    fs::write(&path, value.to_string()).unwrap();
    let output = cv(&root, &["update", "--apply"]);
    assert!(!output.status.success());
    assert!(text(&output).contains("newer"), "{}", text(&output));
}

#[test]
fn init_force_never_overwrites_an_existing_file() {
    let dir = scratch();
    let root = dir.path().join("keep");
    fs::create_dir_all(root.join("inputs")).unwrap();
    fs::write(root.join("inputs/profile.toml"), "mine = true\n").unwrap();
    // The user's profile is not a valid one, so the build at the end of init
    // fails: the workspace stays, the exit code is 3, and the file is untouched.
    let output = cv(dir.path(), &["init", "keep", "--force", "--no-git"]);
    assert_eq!(output.status.code(), Some(3), "{}", text(&output));
    assert!(text(&output).contains("Run `cv build`"));
    assert_eq!(
        fs::read_to_string(root.join("inputs/profile.toml")).unwrap(),
        "mine = true\n"
    );
    assert!(root.join("cv.json").is_file());
}

#[test]
fn the_release_fileset_digest_is_the_manifest_digest() {
    let dir = scratch();
    let root = init(dir.path(), "ws");
    let id = cv(&root, &["fileset-id"]);
    assert_eq!(
        text(&id).trim(),
        manifest(&root)["fileset_id"].as_str().unwrap()
    );
}
