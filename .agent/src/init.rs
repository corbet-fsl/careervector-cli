//! `cv init`: write the complete working-example workspace that is embedded in
//! this binary, offline, and record what was written in `.cv/manifest.json`.
//!
//! The manifest is what lets `cv update` (see [`crate::update`]) tell a file
//! the user edited from one that still is exactly what `cv init` wrote.
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io::{IsTerminal as _, Write as _};
use std::path::{Component, Path, PathBuf};

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};

use crate::fileset_source::sha256_hex;

mod embedded {
    include!(concat!(env!("OUT_DIR"), "/fileset.rs"));
}

/// Workspace-relative path of the manifest.
pub const MANIFEST: &str = ".cv/manifest.json";
/// The manifest format this binary reads and writes.
pub const MANIFEST_VERSION: u64 = 1;
/// The version of this binary.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// The digest of the embedded file set (`cv fileset-id`).
pub const FILESET_ID: &str = embedded::ID;

/// One cv release's worth of workspace: what `cv init` writes and `cv update`
/// converges to. The embedded set is [`Release::embedded`]; tests build others.
pub struct Release<'a> {
    pub version: &'a str,
    pub schema: u64,
    pub files: BTreeMap<&'a str, &'a [u8]>,
}

impl Release<'static> {
    /// The file set compiled into this binary.
    #[must_use]
    pub fn embedded() -> Self {
        let files: BTreeMap<&str, &[u8]> = embedded::FILES.iter().copied().collect();
        let schema = files
            .get("cv.json")
            .and_then(|bytes| schema_of(bytes))
            .expect("the embedded cv.json carries a schema_version");
        Self {
            version: VERSION,
            schema,
            files,
        }
    }
}

/// `.cv/manifest.json`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Manifest {
    pub format: String,
    pub manifest_version: u64,
    /// The cv version that last wrote or updated the workspace.
    pub cv_version: String,
    /// Digest of the file set that version shipped.
    pub fileset_id: String,
    /// Shipped file to the sha256 of the bytes cv wrote (or adopted) there.
    pub files: BTreeMap<String, String>,
    /// Shipped files the user deleted; `cv update` never brings them back.
    #[serde(default, skip_serializing_if = "BTreeSet::is_empty")]
    pub deleted: BTreeSet<String>,
    /// `<name>.cv-new` files cv wrote, with the hash it wrote. Only a file that
    /// still has that hash is replaced or removed later; a `.cv-new` the user
    /// started merging in, or wrote themselves, is never touched.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub new_files: BTreeMap<String, String>,
}

impl Manifest {
    #[must_use]
    pub fn new(release: &Release<'_>) -> Self {
        Self {
            format: "cv-manifest".to_owned(),
            manifest_version: MANIFEST_VERSION,
            cv_version: release.version.to_owned(),
            fileset_id: crate::fileset_source::digest(
                release.files.iter().map(|(path, bytes)| (*path, *bytes)),
            ),
            files: BTreeMap::new(),
            deleted: BTreeSet::new(),
            new_files: BTreeMap::new(),
        }
    }

    pub fn load(root: &Path) -> Result<Option<Self>> {
        let path = safe_path(root, MANIFEST)?;
        if !path.is_file() {
            return Ok(None);
        }
        let text =
            fs::read_to_string(&path).with_context(|| format!("cannot read {}", path.display()))?;
        let manifest: Self = serde_json::from_str(&text)
            .with_context(|| format!("{} is not a valid cv manifest", path.display()))?;
        if manifest.format != "cv-manifest" {
            bail!("{} is not a cv manifest", path.display());
        }
        for file in manifest
            .files
            .keys()
            .chain(&manifest.deleted)
            .chain(manifest.new_files.keys())
        {
            safe_relative(file)?;
        }
        Ok(Some(manifest))
    }

    pub fn save(&self, root: &Path) -> Result<()> {
        let path = safe_path(root, MANIFEST)?;
        let mut text = serde_json::to_string_pretty(self)?;
        text.push('\n');
        write_file(&path, text.as_bytes())
    }
}

/// `schema_version` of a `cv.json`.
#[must_use]
pub fn schema_of(bytes: &[u8]) -> Option<u64> {
    serde_json::from_slice::<serde_json::Value>(bytes)
        .ok()?
        .get("schema_version")?
        .as_u64()
}

/// A manifest path must stay inside the workspace: plain relative components.
pub fn safe_relative(path: &str) -> Result<PathBuf> {
    let relative = Path::new(path);
    if path.is_empty()
        || relative
            .components()
            .any(|component| !matches!(component, Component::Normal(_)))
    {
        bail!("unsafe path in the cv manifest: {path:?}");
    }
    Ok(relative.to_path_buf())
}

/// Write through a fresh, exclusively created temporary file in the same
/// directory and an atomic rename, so a crash never leaves a half-written file
/// and no existing path (a planted link, a user's file) is ever written through.
pub(crate) fn write_file(path: &Path, bytes: &[u8]) -> Result<()> {
    use std::io::Write as _;
    let parent = path.parent().context("a path without a parent")?;
    fs::create_dir_all(parent).with_context(|| format!("cannot create {}", parent.display()))?;
    let mut temporary = tempfile::NamedTempFile::new_in(parent)
        .with_context(|| format!("cannot create a temporary file in {}", parent.display()))?;
    temporary
        .write_all(bytes)
        .with_context(|| format!("cannot write {}", path.display()))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        // A temporary file is private; keep the target's mode, else a normal file's.
        let mode = fs::metadata(path).map_or(0o644, |m| m.permissions().mode() & 0o777);
        temporary
            .as_file()
            .set_permissions(fs::Permissions::from_mode(mode))?;
    }
    temporary
        .persist(path)
        .map_err(|error| anyhow::anyhow!("cannot replace {}: {}", path.display(), error.error))?;
    Ok(())
}

/// `root` joined with `relative`, after proving that no component of the path
/// below `root` is a symlink and that its parent stays inside `root`. Every
/// write and delete of init and update goes through this; a path that fails
/// is refused by name and nothing is changed.
pub(crate) fn safe_path(root: &Path, relative: &str) -> Result<PathBuf> {
    let relative_path = safe_relative(relative)?;
    let mut current = root.to_path_buf();
    for component in relative_path.components() {
        current.push(component);
        match fs::symlink_metadata(&current) {
            Ok(metadata) if metadata.file_type().is_symlink() => {
                bail!(
                    "refusing {relative}: {} is a symbolic link (nothing was changed)",
                    current.display()
                );
            }
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => break,
            Err(error) => {
                return Err(error).with_context(|| format!("cannot inspect {}", current.display()));
            }
        }
    }
    let target = root.join(&relative_path);
    let mut ancestor = target.parent();
    while let Some(directory) = ancestor {
        if directory.exists() {
            let canonical = directory.canonicalize()?;
            if !canonical.starts_with(root.canonicalize()?) {
                bail!(
                    "refusing {relative}: it resolves outside the workspace (nothing was changed)"
                );
            }
            break;
        }
        ancestor = directory.parent();
    }
    Ok(target)
}

/// Whether to run `git init` after writing.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Git {
    Yes,
    No,
    /// Ask on a terminal, skip otherwise.
    Ask,
}

/// What `init` did.
#[derive(Debug, Default)]
pub struct InitReport {
    pub root: PathBuf,
    pub written: usize,
    pub kept: Vec<String>,
    pub git: bool,
}

/// Write `release` into `target`.
///
/// A non-empty directory is refused unless `force`; even then no existing file
/// is ever overwritten. An existing file that already equals the shipped one is
/// recorded; one that differs stays the user's and is listed.
pub fn init(target: &Path, release: &Release<'_>, force: bool, git: Git) -> Result<InitReport> {
    if target.join(MANIFEST).is_file() {
        bail!(
            "{} is already a cv workspace; run `cv update` to bring it to this version",
            target.display()
        );
    }
    if target.exists() {
        if !target.is_dir() {
            bail!("{} exists and is not a directory", target.display());
        }
        if !force && !directory_is_empty(target)? {
            bail!(
                "{} is not empty; choose an empty directory (or pass --force, which still never overwrites an existing file)",
                target.display()
            );
        }
    } else {
        fs::create_dir_all(target)
            .with_context(|| format!("cannot create {}", target.display()))?;
    }
    let root = target
        .canonicalize()
        .with_context(|| format!("cannot resolve {}", target.display()))?;
    for path in release.files.keys() {
        safe_path(&root, path)?;
    }
    let mut manifest = Manifest::new(release);
    let mut report = InitReport {
        root: root.clone(),
        ..InitReport::default()
    };
    for (path, bytes) in &release.files {
        let destination = safe_path(&root, path)?;
        let hash = sha256_hex(bytes);
        if destination.exists() {
            if destination.is_file() && sha256_hex(&fs::read(&destination)?) == hash {
                manifest.files.insert((*path).to_owned(), hash);
            } else {
                report.kept.push((*path).to_owned());
            }
            continue;
        }
        write_file(&destination, bytes)?;
        manifest.files.insert((*path).to_owned(), hash);
        report.written += 1;
    }
    manifest.save(&root)?;
    report.git = maybe_git_init(&root, git)?;
    Ok(report)
}

/// An empty directory, or one that holds only a fresh `.git`.
fn directory_is_empty(directory: &Path) -> Result<bool> {
    for entry in fs::read_dir(directory)? {
        if entry?.file_name() != ".git" {
            return Ok(false);
        }
    }
    Ok(true)
}

fn maybe_git_init(root: &Path, git: Git) -> Result<bool> {
    if root.join(".git").exists() {
        return Ok(false);
    }
    let wanted = match git {
        Git::Yes => true,
        Git::No => false,
        Git::Ask => {
            if std::io::stdin().is_terminal() && std::io::stderr().is_terminal() {
                eprint!("Run `git init` in {}? [y/N] ", root.display());
                std::io::stderr().flush()?;
                let mut answer = String::new();
                std::io::stdin().read_line(&mut answer)?;
                matches!(answer.trim(), "y" | "Y" | "yes")
            } else {
                false
            }
        }
    };
    if !wanted {
        return Ok(false);
    }
    // Never commits: the user decides what history starts with.
    match std::process::Command::new("git")
        .arg("init")
        .arg("--quiet")
        .current_dir(root)
        .status()
    {
        Ok(status) if status.success() => Ok(true),
        _ => {
            eprintln!("git init failed or git is not installed; skipped.");
            Ok(false)
        }
    }
}

/// Print the `init` summary.
pub fn print_report(report: &InitReport, release: &Release<'_>) {
    println!(
        "cv {}: wrote {} files into {}",
        release.version,
        report.written,
        report.root.display()
    );
    for path in &report.kept {
        println!("kept your existing {path}");
    }
    if report.git {
        println!("ran git init (nothing is committed)");
    }
    println!("next: cd {}", report.root.display());
}

#[cfg(test)]
mod tests {
    use super::*;

    pub(crate) fn release<'a>(version: &'a str, files: &[(&'a str, &'a str)]) -> Release<'a> {
        Release {
            version,
            schema: 9,
            files: files
                .iter()
                .map(|(path, text)| (*path, text.as_bytes()))
                .collect(),
        }
    }

    const CV_JSON: &str = r#"{"schema_version": 9}"#;

    #[test]
    fn init_writes_files_and_a_manifest_with_their_hashes() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("my-cv");
        let r = release(
            "1.0.0",
            &[("cv.json", CV_JSON), ("inputs/a.toml", "a = 1\n")],
        );
        let report = init(&target, &r, false, Git::No).unwrap();
        assert_eq!(report.written, 2);
        assert_eq!(
            fs::read_to_string(target.join("inputs/a.toml")).unwrap(),
            "a = 1\n"
        );
        let manifest = Manifest::load(&target).unwrap().unwrap();
        assert_eq!(manifest.cv_version, "1.0.0");
        assert_eq!(manifest.files["inputs/a.toml"], sha256_hex(b"a = 1\n"));
        assert!(
            !target.join(".git").exists(),
            "init never starts git unasked"
        );
    }

    #[test]
    fn init_refuses_a_non_empty_directory_and_force_never_overwrites() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("inputs.txt"), "mine").unwrap();
        fs::create_dir_all(dir.path().join("inputs")).unwrap();
        fs::write(dir.path().join("inputs/a.toml"), "mine = 1\n").unwrap();
        let r = release(
            "1.0.0",
            &[("cv.json", CV_JSON), ("inputs/a.toml", "a = 1\n")],
        );
        let error = init(dir.path(), &r, false, Git::No)
            .unwrap_err()
            .to_string();
        assert!(error.contains("not empty"), "{error}");
        let report = init(dir.path(), &r, true, Git::No).unwrap();
        assert_eq!(report.kept, ["inputs/a.toml"]);
        assert_eq!(
            fs::read_to_string(dir.path().join("inputs/a.toml")).unwrap(),
            "mine = 1\n"
        );
        assert!(
            !Manifest::load(dir.path())
                .unwrap()
                .unwrap()
                .files
                .contains_key("inputs/a.toml")
        );
    }

    #[test]
    fn init_accepts_a_directory_with_only_git() {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir(dir.path().join(".git")).unwrap();
        let r = release("1.0.0", &[("cv.json", CV_JSON)]);
        init(dir.path(), &r, false, Git::No).unwrap();
    }

    #[test]
    fn manifest_paths_cannot_leave_the_workspace() {
        assert!(safe_relative("../x").is_err());
        assert!(safe_relative("/etc/passwd").is_err());
        assert!(safe_relative("a/../b").is_err());
        assert!(safe_relative("a/b.toml").is_ok());
    }

    #[test]
    fn the_embedded_set_is_the_shipped_tree_and_leaves_out_the_tool() {
        let release = Release::embedded();
        assert!(release.files.contains_key("cv.json"));
        assert!(release.files.contains_key("inputs/profile.toml"));
        assert!(
            release
                .files
                .keys()
                .all(|p| crate::fileset_source::included(p))
        );
        assert!(!release.files.keys().any(|p| p.starts_with(".agent/src/")));
        let digest = crate::fileset_source::digest(release.files.iter().map(|(p, b)| (*p, *b)));
        assert_eq!(digest, FILESET_ID);
        let root = Path::new(env!("CARGO_MANIFEST_DIR"));
        let on_disk = crate::fileset_source::files(root).unwrap();
        let embedded: Vec<&str> = release.files.keys().copied().collect();
        assert_eq!(
            on_disk, embedded,
            "rebuild: the embedded set drifted from the tree"
        );
    }
}
