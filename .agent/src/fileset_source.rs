//! Which repository files a `cv init` workspace carries, and the digest that
//! identifies them. Shared by `build.rs` (which embeds the files) and the
//! release evidence (which proves the embedded set is the verified source).
use sha2::{Digest, Sha256};
use std::fmt::Write as _;
use std::fs;
use std::io;
use std::path::Path;

/// Directories that never ship: the tool's own sources, tests, scripts, CI and
/// build state. The installed binary already is the tool.
const EXCLUDED_PREFIXES: &[&str] = &[
    ".agent/src/",
    ".agent/tests/",
    ".agent/scripts/",
    ".agent/cache/",
    ".cargo/",
    ".ci/",
    ".cv/",
    ".forgejo/",
    ".git/",
    ".github/",
    ".moon/",
    ".ccid/",
    "target/",
];

/// Single files that never ship.
const EXCLUDED_FILES: &[&str] = &[
    ".agent/build.rs",
    ".agent/release-platforms.txt",
    // The record of released file sets; appending to it must not change the set.
    ".agent/fileset-releases.txt",
    ".prototools",
    "Cargo.lock",
    "Cargo.toml",
    "cv",
    "cv.cmd",
    "cv.ps1",
    "flake.lock",
    "flake.nix",
    "justfile",
    "moon.yml",
    "rust-toolchain.toml",
];

/// Whether a repository-relative, `/`-separated path belongs to the shipped
/// workspace. Rendered outputs (`preview/` and `pdf/` below `outputs/`) are
/// build products that `cv build` regenerates, so they never ship: a rebuilt
/// render would otherwise look like a user edit on every update.
#[must_use]
pub fn included(path: &str) -> bool {
    // `cv check` evaluates the skills against these cases.
    if path == ".agent/tests/skill-cases.json" {
        return true;
    }
    if EXCLUDED_FILES.contains(&path)
        || EXCLUDED_PREFIXES.iter().any(|p| {
            path.starts_with(p)
                || path
                    .strip_suffix('/')
                    .is_some_and(|d| p.strip_suffix('/') == Some(d))
                || p.strip_suffix('/') == Some(path)
        })
    {
        return false;
    }
    if path.starts_with("outputs/") {
        let mut segments = path.split('/');
        let _file = segments.next_back();
        if segments.any(|segment| segment == "preview" || segment == "pdf") {
            return false;
        }
    }
    true
}

/// The shipped files below `root`, sorted. Uses `git ls-files` in a checkout so
/// untracked local products never ship; a source archive without `.git` is
/// walked instead.
pub fn files(root: &Path) -> io::Result<Vec<String>> {
    let mut paths = if root.join(".git").exists() {
        tracked(root)?
    } else {
        let mut all = Vec::new();
        walk(root, root, &mut all)?;
        all
    };
    paths.retain(|path| included(path));
    paths.sort();
    paths.dedup();
    Ok(paths)
}

fn tracked(root: &Path) -> io::Result<Vec<String>> {
    let output = std::process::Command::new("git")
        .args(["ls-files", "-z"])
        .current_dir(root)
        .output()?;
    if !output.status.success() {
        return Err(io::Error::other("git ls-files failed"));
    }
    Ok(output
        .stdout
        .split(|byte| *byte == 0)
        .filter(|name| !name.is_empty())
        .map(|name| String::from_utf8_lossy(name).into_owned())
        // A tracked path that is gone (deleted, not yet staged) does not ship.
        .filter(|name| root.join(name).is_file())
        .collect())
}

fn walk(root: &Path, directory: &Path, paths: &mut Vec<String>) -> io::Result<()> {
    for entry in fs::read_dir(directory)? {
        let entry = entry?;
        let path = entry.path();
        let relative = path
            .strip_prefix(root)
            .expect("entry is below root")
            .to_string_lossy()
            .replace('\\', "/");
        if entry.file_type()?.is_dir() {
            if !EXCLUDED_PREFIXES.contains(&format!("{relative}/").as_str()) {
                walk(root, &path, paths)?;
            }
        } else if entry.file_type()?.is_file() {
            paths.push(relative);
        }
    }
    Ok(())
}

/// Lowercase hex sha256 of `bytes`.
#[must_use]
pub fn sha256_hex(bytes: &[u8]) -> String {
    let mut hex = String::with_capacity(64);
    for byte in Sha256::digest(bytes) {
        write!(hex, "{byte:02x}").expect("writing to a String");
    }
    hex
}

/// The identity of a file set: sha256 over `<path> <sha256>\n` lines in path
/// order. Equal for the build tree, the embedded set and a release archive.
#[must_use]
pub fn digest<'a>(entries: impl IntoIterator<Item = (&'a str, &'a [u8])>) -> String {
    let mut lines: Vec<(&str, String)> = entries
        .into_iter()
        .map(|(path, bytes)| (path, sha256_hex(bytes)))
        .collect();
    lines.sort();
    let mut text = String::new();
    for (path, hash) in &lines {
        writeln!(text, "{path} {hash}").expect("writing to a String");
    }
    sha256_hex(text.as_bytes())
}
