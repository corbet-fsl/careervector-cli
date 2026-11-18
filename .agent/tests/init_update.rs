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

mod regressions {
    //! Data-loss edges found in review: `.cv-new` files, symlinks, equal
    //! versions with another file set, and migrations.
    use std::fs;
    use std::path::Path;

    use careervector_cli::init::{Git, Manifest, Release, init};
    use careervector_cli::update::{Migration, apply, plan};

    fn release<'a>(version: &'a str, files: &[(&'a str, &'a str)]) -> Release<'a> {
        Release {
            version,
            schema: 9,
            files: files
                .iter()
                .map(|(path, text)| (*path, text.as_bytes()))
                .collect(),
        }
    }

    static NONE: [Migration; 0] = [];
    const CV9: &str = r#"{"schema_version": 9}"#;

    fn run(root: &Path, to: &Release<'_>, migrations: &'static [Migration]) {
        let planned = plan(root, to, migrations).unwrap();
        apply(root, to, migrations, &planned).unwrap();
    }

    fn read(root: &Path, path: &str) -> Option<String> {
        fs::read_to_string(root.join(path)).ok()
    }

    fn workspace(files: &[(&str, &str)]) -> (tempfile::TempDir, std::path::PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("w");
        let mut all = vec![("cv.json", CV9)];
        all.extend_from_slice(files);
        init(&root, &release("1.0.0", &all), false, Git::No).unwrap();
        (dir, root)
    }

    #[test]
    fn p1_a_merge_in_progress_inside_cv_new_is_not_overwritten() {
        let (_dir, w) = workspace(&[("a.txt", "v1")]);
        fs::write(w.join("a.txt"), "mine").unwrap();
        run(
            &w,
            &release("1.0.1", &[("cv.json", CV9), ("a.txt", "v2")]),
            &NONE,
        );
        assert_eq!(read(&w, "a.txt.cv-new").as_deref(), Some("v2"));
        fs::write(w.join("a.txt.cv-new"), "merged by hand").unwrap();
        run(
            &w,
            &release("1.0.2", &[("cv.json", CV9), ("a.txt", "v3")]),
            &NONE,
        );
        assert_eq!(read(&w, "a.txt.cv-new").as_deref(), Some("merged by hand"));
        assert_eq!(read(&w, "a.txt.cv-new.2").as_deref(), Some("v3"));
        assert_eq!(read(&w, "a.txt").as_deref(), Some("mine"));
    }

    #[test]
    fn an_untouched_cv_new_of_cv_is_reused_not_multiplied() {
        let (_dir, w) = workspace(&[("a.txt", "v1")]);
        fs::write(w.join("a.txt"), "mine").unwrap();
        run(
            &w,
            &release("1.0.1", &[("cv.json", CV9), ("a.txt", "v2")]),
            &NONE,
        );
        run(
            &w,
            &release("1.0.2", &[("cv.json", CV9), ("a.txt", "v3")]),
            &NONE,
        );
        assert_eq!(read(&w, "a.txt.cv-new").as_deref(), Some("v3"));
        assert_eq!(read(&w, "a.txt.cv-new.2"), None);
    }

    #[test]
    fn p1b_a_cv_new_the_person_wrote_survives_an_untouched_file_update() {
        let (_dir, w) = workspace(&[("a.txt", "v1")]);
        fs::write(w.join("a.txt.cv-new"), "my own notes").unwrap();
        run(
            &w,
            &release("1.0.1", &[("cv.json", CV9), ("a.txt", "v2")]),
            &NONE,
        );
        assert_eq!(read(&w, "a.txt").as_deref(), Some("v2"));
        assert_eq!(read(&w, "a.txt.cv-new").as_deref(), Some("my own notes"));
    }

    #[test]
    fn a_cv_new_of_cv_is_cleaned_up_once_the_file_is_untouched_again() {
        let (_dir, w) = workspace(&[("a.txt", "v1")]);
        fs::write(w.join("a.txt"), "mine").unwrap();
        run(
            &w,
            &release("1.0.1", &[("cv.json", CV9), ("a.txt", "v2")]),
            &NONE,
        );
        fs::write(w.join("a.txt"), "v2").unwrap(); // merged by hand to the new version
        run(
            &w,
            &release("1.0.2", &[("cv.json", CV9), ("a.txt", "v2")]),
            &NONE,
        );
        assert_eq!(read(&w, "a.txt.cv-new"), None);
        assert!(Manifest::load(&w).unwrap().unwrap().new_files.is_empty());
    }

    #[cfg(unix)]
    #[test]
    fn p2_init_force_refuses_to_write_through_a_dangling_link() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("w");
        let outside = dir.path().join("outside.txt");
        fs::create_dir(&root).unwrap();
        std::os::unix::fs::symlink(&outside, root.join("a.txt")).unwrap();
        let error = init(
            &root,
            &release("1.0.0", &[("cv.json", CV9), ("a.txt", "v1")]),
            true,
            Git::No,
        )
        .unwrap_err()
        .to_string();
        assert!(
            error.contains("a.txt") && error.contains("symbolic link"),
            "{error}"
        );
        assert!(!outside.exists());
        assert!(!root.join("cv.json").exists(), "nothing else was written");
    }

    #[cfg(unix)]
    #[test]
    fn p9_a_symlinked_directory_and_a_hostile_manifest_cannot_delete_outside() {
        let (dir, w) = workspace(&[]);
        let victim_dir = dir.path().join("home-config");
        fs::create_dir_all(&victim_dir).unwrap();
        fs::write(victim_dir.join("settings.ini"), "").unwrap();
        std::os::unix::fs::symlink(&victim_dir, w.join("evil")).unwrap();
        let mut manifest = Manifest::load(&w).unwrap().unwrap();
        // The recorded hash is that of the empty file, as a hostile fork would write it.
        manifest
            .files
            .insert("evil/settings.ini".into(), sha_empty());
        manifest.save(&w).unwrap();
        let to = release("1.0.0", &[("cv.json", CV9)]);
        let error = plan(&w, &to, &NONE).unwrap_err().to_string();
        assert!(
            error.contains("evil/settings.ini") && error.contains("symbolic link"),
            "{error}"
        );
        assert!(victim_dir.join("settings.ini").exists());
    }

    fn sha_empty() -> String {
        "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855".to_owned()
    }

    #[test]
    fn p6_the_same_version_with_another_file_set_is_refused() {
        let (_dir, w) = workspace(&[("a.txt", "newer build")]);
        let other = release("1.0.0", &[("cv.json", CV9), ("a.txt", "older build")]);
        let error = plan(&w, &other, &NONE).unwrap_err().to_string();
        assert!(error.contains("different file set"), "{error}");
        assert_eq!(read(&w, "a.txt").as_deref(), Some("newer build"));
    }

    fn reformat(root: &Path) -> anyhow::Result<()> {
        fs::write(root.join("a.txt"), "v1 reformatted")?;
        Ok(())
    }
    static REFORMAT: [Migration; 1] = [Migration {
        from: 9,
        name: "reformat",
        apply: reformat,
    }];

    #[test]
    fn p7_a_migrated_untouched_file_is_not_treated_as_edited_afterwards() {
        let (_dir, w) = workspace(&[("a.txt", "v1")]);
        let mut next = release(
            "1.1.0",
            &[("cv.json", r#"{"schema_version": 10}"#), ("a.txt", "v1")],
        );
        next.schema = 10;
        run(&w, &next, &REFORMAT);
        assert_eq!(read(&w, "a.txt").as_deref(), Some("v1 reformatted"));
        let mut later = release(
            "1.2.0",
            &[("cv.json", r#"{"schema_version": 10}"#), ("a.txt", "v2")],
        );
        later.schema = 10;
        run(&w, &later, &REFORMAT);
        assert_eq!(
            read(&w, "a.txt").as_deref(),
            Some("v2"),
            "still untouched, so replaced"
        );
        assert_eq!(read(&w, "a.txt.cv-new"), None);
    }

    #[cfg(unix)]
    #[test]
    fn p10_a_planted_cv_tmp_link_is_not_written_through() {
        let (dir, w) = workspace(&[("a.txt", "v1")]);
        let outside = dir.path().join("outside.txt");
        fs::write(&outside, "precious").unwrap();
        std::os::unix::fs::symlink(&outside, w.join("a.txt.cv-tmp")).unwrap();
        run(
            &w,
            &release("1.0.1", &[("cv.json", CV9), ("a.txt", "v2")]),
            &NONE,
        );
        assert_eq!(read(&w, "a.txt").as_deref(), Some("v2"));
        assert_eq!(fs::read_to_string(&outside).unwrap(), "precious");
    }

    #[test]
    fn p11_a_persons_own_cv_tmp_file_is_left_alone() {
        let (_dir, w) = workspace(&[("a.txt", "v1")]);
        fs::write(w.join("a.txt.cv-tmp"), "my scratch").unwrap();
        run(
            &w,
            &release("1.0.1", &[("cv.json", CV9), ("a.txt", "v2")]),
            &NONE,
        );
        assert_eq!(read(&w, "a.txt").as_deref(), Some("v2"));
        assert_eq!(read(&w, "a.txt.cv-tmp").as_deref(), Some("my scratch"));
    }

    #[test]
    fn the_release_record_refuses_a_version_released_with_another_file_set() {
        use careervector_cli::release_evidence::check_fileset_releases;
        let ok = |record: &str, v: &str, d: &str| check_fileset_releases(record, v, d).is_ok();
        // The newest line must be this release.
        assert!(ok("0.1.0 aaaa\n", "0.1.0", "aaaa"));
        assert!(!ok("", "0.1.0", "aaaa"), "an empty record is not a record");
        assert!(!ok("0.1.0 aaaa\n", "0.2.0", "bbbb"), "unrecorded release");
        // An earlier line of the same version needs the same digest.
        assert!(!ok("0.2.0 aaaa\n0.2.0 bbbb\n", "0.2.0", "bbbb"));
        // A new digest needs a strictly greater version.
        assert!(ok("0.1.0 aaaa\n0.2.0 bbbb\n", "0.2.0", "bbbb"));
        assert!(!ok("0.2.0 aaaa\n0.1.5 bbbb\n", "0.1.5", "bbbb"));
        assert!(!ok("0.2.0 aaaa\n0.2.0 bbbb\n", "0.2.0", "bbbb"));
    }
}
