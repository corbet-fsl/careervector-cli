//! `cv update`: bring a workspace to this binary's version, the way dpkg treats
//! conffiles.
//!
//! For every shipped file, three hashes decide: the one `cv` recorded in the
//! manifest (what it last wrote), the one on disk now, and the one this binary
//! ships. A file the user never touched is replaced; a file the user changed is
//! kept and the new version is written beside it as `<name>.cv-new`. Nothing
//! the user edited, and nothing the user deleted, is ever lost or restored.
use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};

use crate::fileset_source::sha256_hex;
use crate::init::{
    MANIFEST, MANIFEST_VERSION, Manifest, Release, safe_path, schema_of, write_file,
};

/// Suffix of the new version placed beside a file the user changed.
pub const NEW_SUFFIX: &str = ".cv-new";

/// One forward schema migration: moves a workspace from schema `from` to
/// `from + 1`. Must be idempotent on already-migrated data.
pub struct Migration {
    pub from: u64,
    pub name: &'static str,
    pub apply: fn(&Path) -> Result<()>,
}

/// Every migration this binary knows, oldest first. `cv.json` schema 9 is the
/// first schema; the profile, stations and application records migrate here
/// too when their versions move.
pub const MIGRATIONS: &[Migration] = &[];

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Action {
    /// New in this version: written.
    Add,
    /// Untouched by the user: replaced with the new version.
    Replace,
    /// Changed by the user and different upstream: kept, new version beside it.
    KeepWriteNew,
    /// The user deleted it: stays deleted.
    KeepDeleted,
    /// Removed upstream and untouched: deleted.
    Remove,
    /// Removed upstream but changed by the user: kept.
    KeepRemoved,
    /// Dropped upstream and already gone: only the bookkeeping entry goes.
    Forget,
    /// The user's file already equals the new version: just recorded.
    Record,
}

#[derive(Debug)]
pub struct Plan {
    /// Files that need no action are left out except for `unchanged`.
    pub items: Vec<(String, Action)>,
    pub unchanged: usize,
    pub migrations: Vec<&'static str>,
    /// There was no manifest: the workspace is a fork or a hand-made copy.
    pub adopted: bool,
    pub from_version: Option<String>,
    pub to_version: String,
}

impl Plan {
    fn count(&self, wanted: &Action) -> usize {
        self.items
            .iter()
            .filter(|(_, action)| action == wanted)
            .count()
    }
}

fn hash_of(path: &Path) -> Result<Option<String>> {
    if path.is_file() {
        Ok(Some(sha256_hex(&fs::read(path).with_context(|| {
            format!("cannot read {}", path.display())
        })?)))
    } else {
        Ok(None)
    }
}

/// Dotted numeric version, ignoring any `-pre`/`+build` tail.
fn version_key(version: &str) -> Vec<u64> {
    version
        .split(['-', '+'])
        .next()
        .unwrap_or_default()
        .split('.')
        .map(|part| part.parse().unwrap_or(0))
        .collect()
}

fn newer(a: &str, b: &str) -> bool {
    let (mut a, mut b) = (version_key(a), version_key(b));
    let len = a.len().max(b.len());
    a.resize(len, 0);
    b.resize(len, 0);
    a > b
}

/// The workspace root above `start`, or the reason there is none.
pub fn find_workspace(start: &Path) -> Result<PathBuf> {
    crate::workspace::find_root(start).ok_or_else(|| {
        anyhow::anyhow!(
            "{} is not inside a cv workspace (no cv.json found); run `cv init <dir>` to create one",
            start.display()
        )
    })
}

fn workspace_schema(root: &Path) -> Result<u64> {
    let bytes = fs::read(root.join("cv.json")).context("cannot read cv.json")?;
    schema_of(&bytes).context("cv.json has no schema_version")
}

/// Work out what bringing `root` to `release` would do. Writes nothing.
pub fn plan(root: &Path, release: &Release<'_>, migrations: &'static [Migration]) -> Result<Plan> {
    let root = &root
        .canonicalize()
        .context("cannot resolve the workspace")?;
    let manifest = Manifest::load(root)?;
    if let Some(manifest) = &manifest {
        if manifest.manifest_version > MANIFEST_VERSION {
            bail!(
                "this workspace's manifest (format {}) is newer than this cv understands; install a newer cv",
                manifest.manifest_version
            );
        }
        if manifest.cv_version == release.version
            && manifest.fileset_id != Manifest::new(release).fileset_id
        {
            bail!(
                "this workspace was written by a cv {} with a different file set than this cv {}; \
                 they are the same version number but not the same release, so cv cannot tell which is newer. \
                 Install the cv release that wrote it, or the next version (nothing was changed)",
                manifest.cv_version,
                release.version
            );
        }
        if newer(&manifest.cv_version, release.version) {
            bail!(
                "this workspace was written by cv {}, which is newer than this cv {}; install a newer cv (nothing was changed)",
                manifest.cv_version,
                release.version
            );
        }
    }
    let schema = workspace_schema(root)?;
    if schema > release.schema {
        bail!(
            "this workspace uses schema {schema}, but this cv {} only knows schema {}; install a newer cv (nothing was changed)",
            release.version,
            release.schema
        );
    }
    let pending: Vec<&'static Migration> = migrations
        .iter()
        .filter(|m| m.from >= schema && m.from < release.schema)
        .collect();
    for step in schema..release.schema {
        if !pending.iter().any(|m| m.from == step) {
            bail!(
                "no migration from schema {step} to {}; install a newer cv",
                step + 1
            );
        }
    }

    let empty = Manifest::new(release);
    let recorded = manifest.as_ref().map_or(&empty.files, |m| &m.files);
    let deleted: BTreeSet<&String> = manifest.iter().flat_map(|m| &m.deleted).collect();
    let mut plan = Plan {
        items: Vec::new(),
        unchanged: 0,
        migrations: pending.iter().map(|m| m.name).collect(),
        adopted: manifest.is_none(),
        from_version: manifest.as_ref().map(|m| m.cv_version.clone()),
        to_version: release.version.to_owned(),
    };
    let mut paths: BTreeSet<&str> = release.files.keys().copied().collect();
    paths.extend(recorded.keys().map(String::as_str));
    for path in paths {
        let current = hash_of(&safe_path(root, path)?)?;
        let old = recorded.get(path);
        let new = release.files.get(path).map(|bytes| sha256_hex(bytes));
        let action = match (&new, old, &current) {
            // Shipped now.
            (Some(_), Some(_), None) => Some(Action::KeepDeleted),
            (Some(new), Some(old), Some(current)) => {
                if current == new {
                    (current != old).then_some(Action::Record)
                } else if current == old {
                    Some(Action::Replace)
                } else if new == old {
                    None // the user's edit; upstream did not move
                } else {
                    Some(Action::KeepWriteNew)
                }
            }
            (Some(_), None, None) => {
                if deleted.contains(&path.to_owned()) {
                    Some(Action::KeepDeleted)
                } else {
                    Some(Action::Add)
                }
            }
            (Some(new), None, Some(current)) => {
                if current == new {
                    Some(Action::Record)
                } else {
                    Some(Action::KeepWriteNew)
                }
            }
            // Gone upstream.
            (None, Some(_), None) => Some(Action::Forget),
            (None, Some(old), Some(current)) => Some(if current == old {
                Action::Remove
            } else {
                Action::KeepRemoved
            }),
            (None, None, _) => None,
        };
        match action {
            Some(action) => plan.items.push((path.to_owned(), action)),
            None => plan.unchanged += 1,
        }
    }
    Ok(plan)
}

/// What `apply` did, for the summary.
#[derive(Debug)]
pub struct Applied {
    pub migrated: Vec<&'static str>,
    /// `(file, where its new version was written)`.
    pub new_versions: Vec<(String, String)>,
}

/// Carry out `plan`. The manifest is written last, so an interrupted update
/// is simply run again.
pub fn apply(
    root: &Path,
    release: &Release<'_>,
    migrations: &'static [Migration],
    plan: &Plan,
) -> Result<Applied> {
    let root = &root
        .canonicalize()
        .context("cannot resolve the workspace")?;
    let old_schema = workspace_schema(root)?;
    let mut manifest = Manifest::load(root)?.unwrap_or_else(|| Manifest::new(release));
    let fresh = Manifest::new(release);
    manifest.cv_version = fresh.cv_version;
    manifest.fileset_id = fresh.fileset_id;
    manifest.manifest_version = MANIFEST_VERSION;
    // Refuse before the first write: one unsafe path changes nothing.
    for (path, _) in &plan.items {
        safe_path(root, path)?;
    }
    let mut new_versions = Vec::new();
    for (path, action) in &plan.items {
        let destination = safe_path(root, path)?;
        let new = release.files.get(path.as_str());
        match action {
            Action::Add | Action::Replace => {
                let bytes = new.expect("shipped file");
                write_file(&destination, bytes)?;
                clear_new(root, path, &mut manifest)?;
                manifest.files.insert(path.clone(), sha256_hex(bytes));
                manifest.deleted.remove(path);
            }
            Action::Record => {
                manifest
                    .files
                    .insert(path.clone(), sha256_hex(new.expect("shipped file")));
                clear_new(root, path, &mut manifest)?;
            }
            Action::KeepWriteNew => {
                // The recorded hash stays: the next update must still see an edit.
                let written = place_new(root, path, new.expect("shipped file"), &mut manifest)?;
                new_versions.push((path.clone(), written));
            }
            Action::KeepDeleted => {
                manifest.files.remove(path);
                manifest.deleted.insert(path.clone());
            }
            Action::Remove => {
                remove_if_present(&destination)?;
                remove_empty_parents(root, &destination);
                clear_new(root, path, &mut manifest)?;
                manifest.files.remove(path);
            }
            Action::Forget => {
                manifest.files.remove(path);
            }
            Action::KeepRemoved => {
                // Now the user's own file; cv stops tracking it.
                manifest.files.remove(path);
            }
        }
    }
    // Every shipped file left untouched and unreported already matches the
    // recorded hash; adoption records the ones that equal the new version.
    // Files the user never touched, to re-record if a migration rewrites them.
    let mut untouched = Vec::new();
    for (path, recorded) in &manifest.files {
        if hash_of(&safe_path(root, path)?)?.as_deref() == Some(recorded.as_str()) {
            untouched.push(path.clone());
        }
    }
    let mut migrated = Vec::new();
    for step in migrations
        .iter()
        .filter(|m| m.from >= old_schema && m.from < release.schema)
    {
        (step.apply)(root).with_context(|| format!("migration {} failed", step.name))?;
        migrated.push(step.name);
    }
    if workspace_schema(root)? < release.schema {
        set_schema(&root.join("cv.json"), release.schema)?;
    }
    // A migration that rewrites an untouched file must not make it look edited.
    for path in untouched {
        if let Some(now) = hash_of(&safe_path(root, &path)?)? {
            manifest.files.insert(path, now);
        }
    }
    manifest.save(root)?;
    Ok(Applied {
        migrated,
        new_versions,
    })
}

/// Write the new version of `path` beside it. `<path>.cv-new` is reused only
/// if cv wrote it and it is unchanged; otherwise `.cv-new.2`, `.3`, ... so that
/// nothing the user wrote or merged is overwritten. Returns the name written.
fn place_new(root: &Path, path: &str, bytes: &[u8], manifest: &mut Manifest) -> Result<String> {
    let mut attempt = 1;
    loop {
        let name = if attempt == 1 {
            format!("{path}{NEW_SUFFIX}")
        } else {
            format!("{path}{NEW_SUFFIX}.{attempt}")
        };
        let target = safe_path(root, &name)?;
        let reusable = match hash_of(&target)? {
            None => !target.exists(),
            Some(hash) => manifest.new_files.get(&name) == Some(&hash),
        };
        if reusable {
            write_file(&target, bytes)?;
            manifest.new_files.insert(name.clone(), sha256_hex(bytes));
            return Ok(name);
        }
        attempt += 1;
    }
}

/// Remove the `.cv-new` files cv wrote for `path` that are still exactly as
/// written; one the user changed or wrote is left alone. Forgets all records.
fn clear_new(root: &Path, path: &str, manifest: &mut Manifest) -> Result<()> {
    let base = format!("{path}{NEW_SUFFIX}");
    let names: Vec<String> = manifest
        .new_files
        .keys()
        .filter(|name| **name == base || name.starts_with(&format!("{base}.")))
        .cloned()
        .collect();
    for name in names {
        let target = safe_path(root, &name)?;
        if hash_of(&target)?.as_deref() == manifest.new_files.get(&name).map(String::as_str) {
            remove_if_present(&target)?;
        }
        manifest.new_files.remove(&name);
    }
    Ok(())
}

fn remove_if_present(path: &Path) -> Result<()> {
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error).with_context(|| format!("cannot remove {}", path.display())),
    }
}

fn remove_empty_parents(root: &Path, file: &Path) {
    let mut directory = file.parent();
    while let Some(current) = directory {
        if current == root || fs::remove_dir(current).is_err() {
            break;
        }
        directory = current.parent();
    }
}

/// Set `schema_version` in place, keeping the rest of `cv.json` byte for byte.
fn set_schema(path: &Path, schema: u64) -> Result<()> {
    let text = fs::read_to_string(path)?;
    let pattern = regex::Regex::new(r#"("schema_version"\s*:\s*)\d+"#)?;
    if !pattern.is_match(&text) {
        bail!("cv.json has no schema_version to migrate");
    }
    let updated = pattern.replace(&text, format!("${{1}}{schema}"));
    write_file(path, updated.as_bytes())?;
    Ok(())
}

/// After `--apply`: where each new version was written, and the fork note.
pub fn print_applied(applied: &Applied) {
    for (file, written) in &applied.new_versions {
        println!("new version of {file} written as {written}");
    }
}

/// Print the plan: what would happen, grouped, with the files that need the user.
pub fn print_plan(plan: &Plan, applied: bool) {
    let verb = |planned: &str, done: &str| {
        if applied {
            done.to_owned()
        } else {
            planned.to_owned()
        }
    };
    match &plan.from_version {
        Some(from) => println!("cv update: workspace {from} -> {}", plan.to_version),
        None => println!(
            "cv update: no {MANIFEST} here; adopting this workspace at cv {} (every file that differs from the shipped one is treated as yours)",
            plan.to_version
        ),
    }
    if plan.adopted {
        println!(
            "note: files this version ships that are missing here are added, because cv cannot tell a deletion from an older checkout; delete them again if you did not want them"
        );
    }
    let group = |action: Action, heading: String| {
        let files: Vec<&str> = plan
            .items
            .iter()
            .filter(|(_, a)| *a == action)
            .map(|(p, _)| p.as_str())
            .collect();
        if !files.is_empty() {
            println!("{heading} ({}):", files.len());
            for file in files.iter().take(15) {
                println!("  {file}");
            }
            if files.len() > 15 {
                println!("  ... and {} more", files.len() - 15);
            }
        }
    };
    group(
        Action::Replace,
        verb(
            "will update (untouched by you)",
            "updated (untouched by you)",
        ),
    );
    group(Action::Add, verb("will add (new)", "added (new)"));
    group(
        Action::Remove,
        verb(
            "will remove (dropped upstream, untouched by you)",
            "removed (dropped upstream, untouched by you)",
        ),
    );
    group(
        Action::KeepWriteNew,
        verb(
            "you changed these; yours stay, the new version goes beside them as <name>.cv-new",
            "you changed these; yours were kept, the new version is beside them as <name>.cv-new",
        ),
    );
    group(
        Action::KeepRemoved,
        "dropped upstream but changed by you; kept as yours".to_owned(),
    );
    group(
        Action::KeepDeleted,
        "you deleted these; they stay deleted".to_owned(),
    );
    if !plan.migrations.is_empty() {
        println!(
            "{}: {}",
            verb("will migrate", "migrated"),
            plan.migrations.join(", ")
        );
    }
    println!(
        "summary: {} updated, {} added, {} removed, {} kept as yours ({} .cv-new), {} unchanged",
        plan.count(&Action::Replace),
        plan.count(&Action::Add),
        plan.count(&Action::Remove),
        plan.count(&Action::KeepWriteNew) + plan.count(&Action::KeepRemoved),
        plan.count(&Action::KeepWriteNew),
        plan.unchanged + plan.count(&Action::Record) + plan.count(&Action::KeepDeleted)
    );
    if !applied {
        println!("nothing was changed; run `cv update --apply` to apply this plan");
    } else if plan.count(&Action::KeepWriteNew) > 0 {
        println!(
            "compare each <name>.cv-new with your file, merge what you want, then delete the .cv-new"
        );
    }
}

#[cfg(test)]
mod tests;
