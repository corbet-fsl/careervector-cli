//! One policy for the names cv accepts, generates and ships: a name that works on every
//! platform cv runs on (Linux, macOS, Windows), in a shell, in a URL and in Git.
//!
//! The rules are those of the strictest platform, Windows, plus Unicode normalisation (macOS
//! stores names decomposed) and case-insensitivity (Windows and macOS). cv never renames or
//! deletes what the user made: it refuses, reports, and suggests a valid name.
use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

use anyhow::{Result, bail};
use unicode_normalization::{UnicodeNormalization as _, is_nfc};

use crate::workspace::Workspace;

/// The longest workspace-relative path cv ships or creates, in characters: with a workspace root
/// of up to about 60 characters the whole path stays under Windows' 260.
pub const MAX_PATH_CHARS: usize = 200;
/// The longest single name, in bytes (the limit of every common file system).
pub const MAX_NAME_BYTES: usize = 255;
/// The longest key cv generates names from (opportunity organisation and position, style,
/// document), so that the paths below it stay within [`MAX_PATH_CHARS`].
pub const MAX_KEY_CHARS: usize = 48;

/// The length of a profile's last name token when the profile can not be read.
pub const ASSUMED_LAST_NAME_CHARS: usize = 24;

const RESERVED_CHARACTERS: &str = "/\\:*?\"<>|";

/// Why `name` is not a portable file or directory name, or `None`.
#[must_use]
pub fn component_problem(name: &str) -> Option<String> {
    component_problem_with(name, true)
}

/// [`component_problem`], optionally leaving out the Unicode normal form rule: the names a user
/// gives their own files are not held to NFC (macOS and Finder write NFD); the names cv
/// generates are.
fn component_problem_with(name: &str, require_nfc: bool) -> Option<String> {
    if name.is_empty() {
        return Some("is empty".to_owned());
    }
    if name == "." || name == ".." {
        return Some("is a directory reference".to_owned());
    }
    if let Some(bad) = name
        .chars()
        .find(|c| RESERVED_CHARACTERS.contains(*c) || c.is_control())
    {
        return Some(format!(
            "contains {}, which some systems do not allow in a name",
            describe(bad)
        ));
    }
    if name.starts_with(' ') || name.ends_with(' ') {
        return Some("starts or ends with a space".to_owned());
    }
    if name.ends_with('.') {
        return Some("ends with a dot".to_owned());
    }
    if is_device_name(name) {
        return Some(
            "is a reserved Windows device name (CON, PRN, AUX, NUL, COM0-9, LPT0-9)".to_owned(),
        );
    }
    if name.len() > MAX_NAME_BYTES {
        return Some(format!("is longer than {MAX_NAME_BYTES} bytes"));
    }
    if require_nfc && !is_nfc(name) {
        return Some(format!(
            "is not in Unicode normal form NFC (use {:?})",
            name.nfc().collect::<String>()
        ));
    }
    None
}

fn describe(c: char) -> String {
    if c.is_control() {
        format!("the control character U+{:04X}", u32::from(c))
    } else {
        format!("{c:?}")
    }
}

/// `CON`, `NUL.txt`, `com1`, `LPT¹.log` ...: Windows reserves the stem, with or without an extension.
fn is_device_name(name: &str) -> bool {
    let stem = name
        .split('.')
        .next()
        .unwrap_or_default()
        .trim_end()
        .to_uppercase();
    if ["CON", "PRN", "AUX", "NUL"].contains(&stem.as_str()) {
        return true;
    }
    let mut chars = stem.chars();
    let prefix: String = chars.by_ref().take(3).collect();
    let rest: Vec<char> = chars.collect();
    (prefix == "COM" || prefix == "LPT")
        && rest.len() == 1
        && (rest[0].is_ascii_digit() || matches!(rest[0], '\u{b9}' | '\u{b2}' | '\u{b3}'))
}

/// Why `name` is not a portable name for something cv generates or takes as a key
/// (an opportunity organisation or position, a style, a document): `[a-z0-9][a-z0-9._-]*`, a
/// portable name, at most [`MAX_KEY_CHARS`] characters.
#[must_use]
pub fn key_problem(name: &str) -> Option<String> {
    if let Some(problem) = component_problem(name) {
        return Some(problem);
    }
    let mut chars = name.chars();
    let first_ok = chars
        .next()
        .is_some_and(|c| c.is_ascii_lowercase() || c.is_ascii_digit());
    if !first_ok
        || !name
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || "._-".contains(c))
    {
        return Some("may only use lowercase letters, digits, '.', '_' and '-', and must start with a letter or digit".to_owned());
    }
    if name.chars().count() > MAX_KEY_CHARS {
        return Some(format!("is longer than {MAX_KEY_CHARS} characters"));
    }
    None
}

/// A key for `label` (for example "organisation"), or an error that says what is wrong and
/// suggests a valid one.
pub fn key(name: &str, label: &str) -> Result<()> {
    if let Some(problem) = key_problem(name) {
        bail!(
            "invalid {label} key: {name:?}. It {problem}. A valid one: {:?}",
            suggest_key(name)
        );
    }
    Ok(())
}

/// A key close to `name`: lowercase ASCII, anything else a hyphen, device names suffixed.
#[must_use]
pub fn suggest_key(name: &str) -> String {
    let mut out = String::new();
    for c in name.nfc().flat_map(char::to_lowercase) {
        if c.is_ascii_lowercase() || c.is_ascii_digit() || c == '.' || c == '_' {
            out.push(c);
        } else if !out.ends_with('-') {
            out.push('-');
        }
    }
    let mut out = out
        .trim_matches(|c| c == '-' || c == '.' || c == '_')
        .to_owned();
    if out.is_empty() {
        "name".clone_into(&mut out);
    }
    if is_device_name(&out) {
        out.push_str("-1");
    }
    out.chars()
        .take(MAX_KEY_CHARS)
        .collect::<String>()
        .trim_end_matches(['-', '.', '_'])
        .to_owned()
}

/// The longest workspace-relative path cv generates for an opportunity, in characters:
/// `opportunities/<org>/<pos>/typst/<CV|CL>_<Last>_<Org>_<Pos>` plus room for an extension
/// or a page suffix such as `-4-1.png`.
#[must_use]
pub fn generated_path_chars(organisation: &str, position: &str, last_name: usize) -> usize {
    let (org, pos) = (organisation.chars().count(), position.chars().count());
    let directory = "opportunities/".len() + org + 1 + pos + "/typst/".len();
    let file = "CV_".len() + last_name + 1 + org + 1 + pos + 16;
    directory + file
}

/// The keys of a NEW opportunity: portable keys, and short enough that every path cv will
/// generate for it stays within [`MAX_PATH_CHARS`] (the file names carry both keys and the
/// last name of the profile twice over). Existing opportunities are never held to this.
pub fn new_opportunity_keys(organisation: &str, position: &str, last_name: usize) -> Result<()> {
    key(organisation, "organisation")?;
    key(position, "position")?;
    let longest = generated_path_chars(organisation, position, last_name);
    if longest > MAX_PATH_CHARS {
        bail!(
            "the organisation and position keys are too long: the files cv would generate for \
             {organisation}/{position} go up to {longest} characters (limit {MAX_PATH_CHARS}, so that a \
             workspace root of up to 60 characters stays under Windows' 260). Shorten the two keys by \
             at least {} characters in total.",
            (longest - MAX_PATH_CHARS).div_ceil(2)
        );
    }
    Ok(())
}

/// A name close to `name` that [`component_problem`] accepts.
#[must_use]
pub fn suggest_component(name: &str) -> String {
    let mut out: String = name
        .nfc()
        .map(|c| {
            if RESERVED_CHARACTERS.contains(c) || c.is_control() {
                '-'
            } else {
                c
            }
        })
        .collect();
    out = out.trim().trim_end_matches('.').to_owned();
    if out.is_empty() || out == "." || out == ".." {
        "name".clone_into(&mut out);
    }
    if is_device_name(&out) {
        out = format!("_{out}");
    }
    out
}

/// Why the workspace-relative path `relative` (`/`-separated) is not portable, or `None`: every
/// component portable, no empty one, at most [`MAX_PATH_CHARS`] characters.
#[must_use]
pub fn path_problem(relative: &str) -> Option<String> {
    if relative.contains('\\') {
        return Some("contains a backslash".to_owned());
    }
    for part in relative.split('/') {
        if let Some(problem) = component_problem(part) {
            return Some(format!("{part:?} {problem}"));
        }
    }
    let length = relative.chars().count();
    if length > MAX_PATH_CHARS {
        return Some(format!(
            "is {length} characters long, more than the {MAX_PATH_CHARS} that keep it under Windows' 260 with a workspace root of up to 60"
        ));
    }
    None
}

/// Pairs of names that differ only in case (or in normalisation form): two files on Linux, one
/// on Windows and macOS.
#[must_use]
pub fn case_collisions<'a>(names: &[&'a str]) -> Vec<(&'a str, &'a str)> {
    let mut seen: BTreeMap<String, &str> = BTreeMap::new();
    let mut found = Vec::new();
    for name in names {
        let folded: String = name.nfc().flat_map(char::to_lowercase).collect();
        match seen.get(&folded) {
            Some(first) if first != name => found.push((*first, *name)),
            Some(_) => {}
            None => {
                seen.insert(folded, name);
            }
        }
    }
    found
}

/// `cv check`: the names under `inputs/` and `opportunities/` that are not portable, the paths
/// that are too long and the case or normalisation collisions, one line each with a fix. They
/// are warnings, never errors (`cv check` and `cv setup` go on): they are the user's own files,
/// and cv never renames or deletes them. A name in NFD alone is not reported (macOS writes it);
/// two names that are the same after normalisation are.
pub fn workspace_name_findings(workspace: &Workspace) -> Result<Vec<String>> {
    let mut problems = Vec::new();
    for top in ["inputs", "opportunities"] {
        walk(&workspace.path(top), top, &mut problems)?;
    }
    Ok(problems)
}

fn walk(directory: &Path, relative: &str, problems: &mut Vec<String>) -> Result<()> {
    let Ok(entries) = fs::read_dir(directory) else {
        return Ok(());
    };
    // (lossy display name, the real name, is a directory, valid UTF-8)
    let mut names: Vec<(String, std::ffi::OsString, bool, bool)> = Vec::new();
    for entry in entries {
        let entry = entry?;
        let real = entry.file_name();
        names.push((
            real.to_string_lossy().into_owned(),
            real.clone(),
            entry.file_type()?.is_dir(),
            real.to_str().is_some(),
        ));
    }
    names.sort();
    let plain: Vec<&str> = names.iter().map(|(name, ..)| name.as_str()).collect();
    for (first, second) in case_collisions(&plain) {
        problems.push(format!(
            "{relative}/: {first:?} and {second:?} differ only in case or normalisation; they are one file on Windows and macOS. Rename one of them."
        ));
    }
    for (name, real, is_dir, utf8) in &names {
        let path = format!("{relative}/{name}");
        if !utf8 {
            problems.push(format!(
                "{path}: the name is not valid UTF-8 (shown here with replacement characters); other systems and tools can not read it. Rename it; cv never renames your files."
            ));
        } else if let Some(problem) = component_problem_with(name, false) {
            problems.push(format!(
                "{path}: the name {problem}. Rename it, for example to {:?}; cv never renames your files.",
                suggest_component(name)
            ));
        } else if let Some(problem) = path_problem(&path).filter(|p| p.contains("characters long"))
        {
            problems.push(format!("{path}: the path {problem}. Shorten a name in it."));
        }
        if *is_dir {
            walk(&directory.join(real), &path, problems)?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn portable_names_are_accepted() {
        for good in [
            "console.toml",
            "aux-notes.md",
            "com10",
            "COM10.txt",
            "lpt",
            "auxiliary",
            ".gitignore",
            "a.b.c",
            "CV_Corbet.pdf",
            "r\u{e9}sum\u{e9}.md",
            "content.toml",
            "2026-11-19 notes.md",
        ] {
            assert_eq!(component_problem(good), None, "{good:?}");
        }
    }

    #[test]
    fn names_that_cause_trouble_somewhere_are_refused() {
        for bad in [
            "con.txt",
            "CON",
            "aux.",
            "nul.tar.gz",
            "com1",
            "LPT9.log",
            "com\u{b9}",
            "LPT\u{b2}.txt",
            "COM\u{b3}",
            "a\\b",
            "a/b",
            "a:b",
            "a*b",
            "a?b",
            "a\"b",
            "a<b",
            "a>b",
            "a|b",
            "x\u{1}y",
            "x\u{7f}",
            "x\ty",
            "",
            ".",
            "..",
            " lead",
            "trail ",
            "dot.",
        ] {
            assert!(component_problem(bad).is_some(), "{bad:?} must be refused");
        }
        assert!(component_problem(&"a".repeat(256)).is_some());
        assert!(component_problem(&"a".repeat(255)).is_none());
        // 255 bytes, not characters: 128 two-byte characters are 256 bytes.
        assert!(component_problem(&"\u{e9}".repeat(128)).is_some());
    }

    #[test]
    fn a_name_that_is_not_nfc_is_refused_with_the_nfc_form() {
        let problem = component_problem("e\u{301}.md").unwrap();
        assert!(
            problem.contains("NFC") && problem.contains("\u{e9}.md"),
            "{problem}"
        );
        assert_eq!(component_problem("\u{e9}.md"), None);
    }

    #[test]
    fn keys_are_lowercase_portable_and_short() {
        for good in ["acme", "my-corp_2", "a.b", "d-plus", "0day"] {
            assert_eq!(key_problem(good), None, "{good:?}");
        }
        for bad in [
            "Acme",
            "-x",
            "_x",
            "con",
            "nul",
            "my corp",
            "caf\u{e9}",
            &"x".repeat(49),
        ] {
            assert!(key_problem(bad).is_some(), "{bad:?}");
        }
        assert!(
            key("Acme Corp", "organisation")
                .unwrap_err()
                .to_string()
                .contains("\"acme-corp\"")
        );
        assert_eq!(suggest_key("My Corp!"), "my-corp");
        assert_eq!(suggest_key("CON"), "con-1");
        assert_eq!(suggest_key("Caf\u{e9}"), "caf");
        assert_eq!(key_problem(&suggest_key("  ??  ")), None);
    }

    #[test]
    fn suggested_components_are_portable() {
        for bad in ["a:b", "con.txt", "x ", "dot.", "..", "", "p|q*r"] {
            assert_eq!(
                component_problem(&suggest_component(bad)),
                None,
                "{bad:?} -> {:?}",
                suggest_component(bad)
            );
        }
    }

    #[test]
    fn case_and_normalisation_collisions_are_found() {
        assert_eq!(
            case_collisions(&["README.md", "other", "Readme.md"]),
            vec![("README.md", "Readme.md")]
        );
        assert_eq!(case_collisions(&["\u{e9}", "e\u{301}"]).len(), 1);
        assert_eq!(case_collisions(&["a", "b", "a"]), vec![]);
    }

    #[test]
    fn a_path_is_at_most_200_characters() {
        let at_limit = format!("{}/{}", "a".repeat(100), "b".repeat(99));
        assert_eq!(at_limit.chars().count(), 200);
        assert_eq!(path_problem(&at_limit), None);
        let over = format!("{at_limit}c");
        assert!(path_problem(&over).unwrap().contains("201 characters"));
        assert!(path_problem("a\\b").is_some());
        assert!(
            path_problem("a//b").is_some(),
            "an empty component is refused"
        );
    }

    #[test]
    fn every_shipped_path_is_portable_and_short() {
        let release = crate::init::Release::embedded();
        let paths: Vec<&str> = release.files.keys().copied().collect();
        let mut longest = 0;
        for path in &paths {
            assert_eq!(path_problem(path), None, "{path}");
            longest = longest.max(path.chars().count());
        }
        // With a workspace root of 60 characters the longest shipped file stays under 260.
        assert!(longest + 61 <= 260, "{longest}");
        // No two shipped paths in one directory differ only in case.
        let mut by_dir: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
        for path in &paths {
            let (dir, name) = path.rsplit_once('/').unwrap_or(("", path));
            by_dir.entry(dir).or_default().push(name);
        }
        for (dir, names) in by_dir {
            assert!(case_collisions(&names).is_empty(), "{dir}");
        }
    }

    fn workspace() -> (tempfile::TempDir, Workspace) {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("cv.json"), "{}").unwrap();
        for top in ["inputs", "opportunities"] {
            fs::create_dir_all(dir.path().join(top)).unwrap();
        }
        let workspace = Workspace::at(dir.path()).unwrap();
        (dir, workspace)
    }

    #[test]
    fn cv_check_warns_about_trouble_with_a_fix_and_changes_nothing() {
        let (dir, workspace) = workspace();
        let inputs = dir.path().join("inputs");
        fs::write(inputs.join("profile.toml"), "ok").unwrap();
        // Windows can not even create `con.txt` (it opens the console device), and its file
        // system holds only one of `Notes.md` and `notes.md`: those cases need another platform.
        if !cfg!(windows) {
            fs::write(inputs.join("con.txt"), "x").unwrap();
        }
        if cfg!(target_os = "linux") {
            fs::write(inputs.join("Notes.md"), "x").unwrap();
            fs::write(inputs.join("notes.md"), "x").unwrap();
            // The same name in NFC and NFD: a collision after normalisation.
            fs::write(inputs.join("\u{e9}.md"), "x").unwrap();
            fs::write(inputs.join("e\u{301}.md"), "x").unwrap();
        }
        // A path over 260 characters: written in the form Windows accepts.
        let deep = dir.path().join(format!(
            "opportunities/{}",
            vec!["d".repeat(40); 5].join("/")
        ));
        let deep_io = crate::init::io_path(&deep);
        fs::create_dir_all(&deep_io).unwrap();
        fs::write(deep_io.join("application.toml"), "x").unwrap();
        let findings = workspace_name_findings(&workspace).unwrap().join("\n");
        if !cfg!(windows) {
            assert!(
                findings.contains("inputs/con.txt")
                    && findings.contains("reserved Windows device name"),
                "{findings}"
            );
            assert!(
                findings.contains("_con.txt") && findings.contains("never renames"),
                "{findings}"
            );
        }
        if cfg!(target_os = "linux") {
            assert!(
                findings.contains("differ only in case or normalisation"),
                "{findings}"
            );
            // Some file systems (ZFS with normalisation, APFS) fold the NFC and the NFD spelling
            // into one file; the second collision is checked only where both exist.
            let both = fs::read_dir(&inputs)
                .unwrap()
                .filter_map(Result::ok)
                .filter(|e| {
                    let name = e.file_name().to_string_lossy().into_owned();
                    Path::new(&name).extension().is_some_and(|e| e == "md")
                        && (name.starts_with("\u{e9}") || name.starts_with("e\u{301}"))
                })
                .count();
            if both == 2 {
                assert!(
                    findings.matches("differ only in case").count() >= 2,
                    "{findings}"
                );
            }
        }
        assert!(
            findings.contains("characters long") && findings.contains("Shorten"),
            "{findings}"
        );
        // Everything is still there.
        assert!(inputs.join("profile.toml").exists());
    }

    #[cfg(unix)]
    #[test]
    fn a_name_that_is_not_utf8_is_a_warning_with_its_lossy_form() {
        use std::os::unix::ffi::OsStrExt as _;
        let (dir, workspace) = workspace();
        let bad = std::ffi::OsStr::from_bytes(b"cv-\xff\xfe.pdf");
        if let Err(error) = fs::write(dir.path().join("inputs").join(bad), "x") {
            // A file system that only holds UTF-8 names (ZFS with utf8only) can not have such a
            // file; there is nothing to report there.
            eprintln!("skipped: this file system refuses a non-UTF-8 name: {error}");
            return;
        }
        // ... and inside a directory with such a name, which is still walked.
        let dir_bad = dir
            .path()
            .join("opportunities")
            .join(std::ffi::OsStr::from_bytes(b"acme-\xff"));
        fs::create_dir_all(&dir_bad).unwrap();
        fs::write(dir_bad.join("con.txt"), "x").unwrap();
        let findings = workspace_name_findings(&workspace).unwrap();
        let joined = findings.join("\n");
        assert!(
            joined.contains("inputs/cv-\u{fffd}\u{fffd}.pdf") && joined.contains("not valid UTF-8"),
            "{joined}"
        );
        assert!(
            joined.contains("acme-\u{fffd}") && joined.contains("con.txt"),
            "walked into it: {joined}"
        );
        assert!(
            dir.path().join("inputs").join(bad).exists(),
            "nothing is renamed"
        );
    }

    #[test]
    fn a_name_in_nfd_alone_is_not_reported_for_a_user_file() {
        let (dir, workspace) = workspace();
        // What Finder and HFS+ write: not NFC, but nothing else wrong.
        fs::write(dir.path().join("inputs/Zeugnis Mu\u{308}ller.pdf"), "x").unwrap();
        assert_eq!(
            workspace_name_findings(&workspace).unwrap(),
            Vec::<String>::new()
        );
        // cv's own generated names are held to NFC.
        assert!(
            component_problem("Zeugnis Mu\u{308}ller.pdf")
                .unwrap()
                .contains("NFC")
        );
    }

    #[test]
    fn new_opportunity_keys_are_budgeted_so_every_generated_path_fits() {
        // Short keys with a normal last name are fine.
        new_opportunity_keys("acme", "strategy-lead", 6).unwrap();
        // The longest accepted pair for a 6-letter last name just fits ...
        let org = "o".repeat(37);
        let pos = "p".repeat(38);
        assert!(generated_path_chars(&org, &pos, 6) <= MAX_PATH_CHARS);
        new_opportunity_keys(&org, &pos, 6).unwrap();
        // ... one more character on a key does not, and the message says by how much.
        let too_long = "p".repeat(40);
        let error = new_opportunity_keys(&org, &too_long, 6)
            .unwrap_err()
            .to_string();
        assert!(
            error.contains("too long") && error.contains("at least 2 characters"),
            "{error}"
        );
        // A long last name shrinks the budget; 48+48 never fits.
        assert!(new_opportunity_keys(&"o".repeat(48), &"p".repeat(48), 6).is_err());
        assert!(new_opportunity_keys(&org, &pos, 40).is_err());
        // And the keys themselves must be portable.
        assert!(new_opportunity_keys("con", "lead", 6).is_err());
        assert!(new_opportunity_keys("Acme", "lead", 6).is_err());
    }

    #[test]
    fn a_portable_workspace_passes_and_so_does_this_repository() {
        let (dir, workspace) = workspace();
        fs::write(dir.path().join("inputs/profile.toml"), "ok").unwrap();
        fs::create_dir_all(dir.path().join("opportunities/acme/lead")).unwrap();
        assert_eq!(
            workspace_name_findings(&workspace).unwrap(),
            Vec::<String>::new()
        );
        let repository = Workspace::at(Path::new(env!("CARGO_MANIFEST_DIR"))).unwrap();
        assert_eq!(
            workspace_name_findings(&repository).unwrap(),
            Vec::<String>::new()
        );
    }
}
