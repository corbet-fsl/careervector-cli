//! Command-level integration tests: every `cv` command run as a command.
//!
//! Each test copies the repository workspace into fresh scratch space outside
//! the repository tree and drives the built `cv` binary with `--root`. The
//! binary is built once by Cargo (`CARGO_BIN_EXE_cv`) and reused by every test.
//! The copies keep the exact bytes of every runtime source file, so the
//! binary's embedded runtime identity verifies against them.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::time::{Duration, Instant};

use careervector_cli::{Workspace, archive, content};
use serde_json::{Value, json};

const BIN: &str = env!("CARGO_BIN_EXE_cv");

fn repository() -> &'static Path {
    Path::new(env!("CARGO_MANIFEST_DIR"))
}

/// Scratch space for workspace copies and command outputs. `TMPDIR` points at
/// CLI-scratch on worker machines; anything outside the repository tree works.
fn scratch_base() -> PathBuf {
    std::env::var_os("TMPDIR").map_or_else(std::env::temp_dir, PathBuf::from)
}

/// Copy the repository workspace (minus `.git` and `target/`) to fresh scratch
/// space.
fn copy_workspace(prefix: &str) -> (tempfile::TempDir, PathBuf) {
    let temp = tempfile::Builder::new()
        .prefix(prefix)
        .tempdir_in(scratch_base())
        .unwrap();
    let root = temp.path().to_path_buf();
    for entry in walkdir::WalkDir::new(repository()).min_depth(1) {
        let entry = entry.unwrap();
        let relative = entry.path().strip_prefix(repository()).unwrap();
        if relative.starts_with(".git") || relative.starts_with("target") {
            continue;
        }
        let destination = root.join(relative);
        if entry.file_type().is_dir() {
            fs::create_dir_all(&destination).unwrap();
        } else if entry.file_type().is_file() {
            if let Some(parent) = destination.parent() {
                fs::create_dir_all(parent).unwrap();
            }
            fs::copy(entry.path(), &destination).unwrap();
        }
    }
    (temp, root)
}

fn run(root: &Path, args: &[&str]) -> Output {
    Command::new(BIN)
        .arg("--root")
        .arg(root)
        .args(args)
        .output()
        .unwrap()
}

/// The binary with a workspace: `render-style` resolves its files against
/// the working directory.
fn run_cwd(root: &Path, args: &[&str]) -> Output {
    Command::new(BIN)
        .current_dir(root)
        .args(args)
        .output()
        .unwrap()
}

fn stdout_text(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout).into_owned()
}

fn stderr_text(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
}

fn read_json(path: &Path) -> Value {
    serde_json::from_str(&fs::read_to_string(path).unwrap()).unwrap()
}

fn read_toml(path: &Path) -> toml::Value {
    toml::from_str(&fs::read_to_string(path).unwrap()).unwrap()
}

fn pdf_pages(path: &Path) -> usize {
    lopdf::Document::load(path).unwrap().get_pages().len()
}

// ---------------------------------------------------------------------------
// runtime-id, bootstrap, doctor
// ---------------------------------------------------------------------------

#[test]
fn runtime_id_prints_the_embedded_identity() {
    let (_temp, root) = copy_workspace("ccvl-cli-runtime-id-");
    let output = run(&root, &["runtime-id"]);
    assert_eq!(output.status.code(), Some(0), "{}", stderr_text(&output));
    let id = stdout_text(&output);
    assert_eq!(id.lines().count(), 1, "{id}");
    let id = id.trim();
    assert_eq!(id.len(), 64, "{id}");
    assert!(id.chars().all(|c| c.is_ascii_hexdigit()), "{id}");
}

#[test]
fn runtime_id_rejects_extra_arguments() {
    let (_temp, root) = copy_workspace("ccvl-cli-runtime-id-args-");
    let output = run(&root, &["runtime-id", "extra"]);
    assert_eq!(output.status.code(), Some(2), "{}", stderr_text(&output));
    assert!(
        stderr_text(&output).contains("unexpected argument"),
        "{}",
        stderr_text(&output)
    );
}

#[test]
fn bootstrap_describes_the_dependency_free_setup() {
    let (_temp, root) = copy_workspace("ccvl-cli-bootstrap-");
    let output = run(&root, &["bootstrap"]);
    assert_eq!(output.status.code(), Some(0), "{}", stderr_text(&output));
    let text = stdout_text(&output);
    assert!(text.contains("cv bootstrap plan"), "{text}");
    assert!(text.contains("runtime dependencies: none"), "{text}");
}

#[test]
fn bootstrap_needs_a_workspace() {
    let missing = scratch_base().join("ccvl-cli-no-workspace-here");
    let _ = fs::remove_dir_all(&missing);
    let output = run(&missing, &["bootstrap"]);
    assert_eq!(output.status.code(), Some(1), "{}", stderr_text(&output));
    assert!(
        stderr_text(&output).contains("cannot resolve workspace"),
        "{}",
        stderr_text(&output)
    );
}

#[test]
fn doctor_reports_the_embedded_runtime() {
    let (_temp, root) = copy_workspace("ccvl-cli-doctor-");
    let output = run(&root, &["doctor"]);
    assert_eq!(output.status.code(), Some(0), "{}", stderr_text(&output));
    let text = stdout_text(&output);
    assert!(text.contains("Typst engine: embedded"), "{text}");
    assert!(text.contains("fonts: 16 embedded files"), "{text}");
    assert!(
        text.contains("external runtime dependencies: none"),
        "{text}"
    );
}

#[test]
fn doctor_needs_a_workspace() {
    let missing = scratch_base().join("ccvl-cli-no-workspace-doctor");
    let _ = fs::remove_dir_all(&missing);
    let output = run(&missing, &["doctor"]);
    assert_eq!(output.status.code(), Some(1), "{}", stderr_text(&output));
    assert!(
        stderr_text(&output).contains("cannot resolve workspace"),
        "{}",
        stderr_text(&output)
    );
}

// ---------------------------------------------------------------------------
// list-documents, list-styles, explain-style
// ---------------------------------------------------------------------------

#[test]
fn list_documents_reports_every_variant_as_json() {
    let (_temp, root) = copy_workspace("ccvl-cli-list-documents-");
    let output = run(&root, &["list-documents"]);
    assert_eq!(output.status.code(), Some(0), "{}", stderr_text(&output));
    let documents: Value = serde_json::from_str(&stdout_text(&output)).unwrap();
    let entries = documents.as_array().unwrap();
    assert!(!entries.is_empty(), "{documents}");
    for entry in entries {
        for field in ["document", "style", "substyle", "locale", "pages", "output"] {
            assert!(entry.get(field).is_some(), "{entry}");
        }
    }
}

#[test]
fn list_documents_rejects_extra_arguments() {
    let (_temp, root) = copy_workspace("ccvl-cli-list-documents-args-");
    let output = run(&root, &["list-documents", "extra"]);
    assert_eq!(output.status.code(), Some(2), "{}", stderr_text(&output));
}

#[test]
fn list_styles_reports_the_workspace_defaults() {
    let (_temp, root) = copy_workspace("ccvl-cli-list-styles-");
    let output = run(&root, &["list-styles"]);
    assert_eq!(output.status.code(), Some(0), "{}", stderr_text(&output));
    let styles: Value = serde_json::from_str(&stdout_text(&output)).unwrap();
    assert_eq!(styles["cv"]["default"]["style"], json!("harvard"));
    assert!(styles["cl"]["styles"].as_array().is_some(), "{styles}");
}

#[test]
fn list_styles_rejects_extra_arguments() {
    let (_temp, root) = copy_workspace("ccvl-cli-list-styles-args-");
    let output = run(&root, &["list-styles", "extra"]);
    assert_eq!(output.status.code(), Some(2), "{}", stderr_text(&output));
}

#[test]
fn explain_style_reports_the_merged_settings() {
    let (_temp, root) = copy_workspace("ccvl-cli-explain-style-");
    let output = run(&root, &["explain-style", "cv", "en-ch"]);
    assert_eq!(output.status.code(), Some(0), "{}", stderr_text(&output));
    let explanation: Value = serde_json::from_str(&stdout_text(&output)).unwrap();
    assert_eq!(explanation["document"], json!("cv"));
    assert_eq!(explanation["locale"], json!("en-ch"));
}

#[test]
fn explain_style_rejects_unknown_documents() {
    let (_temp, root) = copy_workspace("ccvl-cli-explain-style-doc-");
    let output = run(&root, &["explain-style", "bogus", "en-ch"]);
    assert_eq!(output.status.code(), Some(1), "{}", stderr_text(&output));
    assert!(
        stderr_text(&output).contains("unknown document \"bogus\""),
        "{}",
        stderr_text(&output)
    );
}

// ---------------------------------------------------------------------------
// check, public-check, profile-status
// ---------------------------------------------------------------------------

#[test]
fn check_passes_the_whole_workspace() {
    let (_temp, root) = copy_workspace("ccvl-cli-check-");
    let output = run(&root, &["check"]);
    assert_eq!(output.status.code(), Some(0), "{}", stderr_text(&output));
    assert!(
        stdout_text(&output).contains(
            "All data, station, source, skill, font, reproducibility, CV, cover-letter, and output document checks passed."
        ),
        "{}",
        stdout_text(&output)
    );
}

#[test]
fn check_scopes_documents_and_rejects_unknown_styles() {
    let (_temp, root) = copy_workspace("ccvl-cli-check-style-");
    let output = run(&root, &["check", "--style", "cv/harvard"]);
    assert_eq!(output.status.code(), Some(0), "{}", stderr_text(&output));
    assert!(
        stdout_text(&output).contains("(documents limited to cv/harvard)"),
        "{}",
        stdout_text(&output)
    );
    let output = run(&root, &["check", "--style", "cv/bogus"]);
    assert_eq!(output.status.code(), Some(1), "{}", stderr_text(&output));
    assert!(
        stderr_text(&output).contains("--style \"cv/bogus\" does not name an existing style"),
        "{}",
        stderr_text(&output)
    );
}

#[test]
fn public_check_verifies_the_publishable_tree() {
    let (_temp, root) = copy_workspace("ccvl-cli-public-check-");
    let artifacts = scratch_base().join(format!("ccvl-cli-artifacts-{}", std::process::id()));
    let _ = fs::remove_dir_all(&artifacts);
    let output = run(
        &root,
        &["public-check", "--artifacts", artifacts.to_str().unwrap()],
    );
    assert_eq!(output.status.code(), Some(0), "{}", stderr_text(&output));
    assert!(
        stdout_text(&output).contains("Public-boundary checks passed."),
        "{}",
        stdout_text(&output)
    );
    assert!(
        fs::read_dir(&artifacts).unwrap().count() > 0,
        "no artifacts retained"
    );
    fs::remove_dir_all(&artifacts).unwrap();
}

#[test]
fn public_check_refuses_an_existing_artifact_directory() {
    let (_temp, root) = copy_workspace("ccvl-cli-public-check-dir-");
    let artifacts =
        scratch_base().join(format!("ccvl-cli-artifacts-exists-{}", std::process::id()));
    fs::create_dir_all(&artifacts).unwrap();
    let output = run(
        &root,
        &["public-check", "--artifacts", artifacts.to_str().unwrap()],
    );
    assert_eq!(output.status.code(), Some(1), "{}", stderr_text(&output));
    assert!(
        stderr_text(&output).contains("artifact directory already exists"),
        "{}",
        stderr_text(&output)
    );
    fs::remove_dir_all(&artifacts).unwrap();
}

#[test]
fn profile_status_reports_a_ready_station_plan() {
    let (_temp, root) = copy_workspace("ccvl-cli-profile-status-");
    let output = run(&root, &["profile-status"]);
    assert_eq!(output.status.code(), Some(0), "{}", stderr_text(&output));
    assert!(
        stdout_text(&output).contains("CV station plan: READY"),
        "{}",
        stdout_text(&output)
    );
}

#[test]
fn profile_status_needs_its_plan_file() {
    let (_temp, root) = copy_workspace("ccvl-cli-profile-status-plan-");
    let output = run(&root, &["profile-status", "missing.toml"]);
    assert_eq!(output.status.code(), Some(1), "{}", stderr_text(&output));
    assert!(
        stderr_text(&output).contains("cannot resolve missing.toml"),
        "{}",
        stderr_text(&output)
    );
}

// ---------------------------------------------------------------------------
// measure, build, build-cv, build-cl, build-output
// ---------------------------------------------------------------------------

#[test]
fn measure_reports_passing_lines() {
    let (_temp, root) = copy_workspace("ccvl-cli-measure-");
    let output = run(&root, &["measure", "--style", "cv/cluster"]);
    assert_eq!(output.status.code(), Some(0), "{}", stderr_text(&output));
    assert!(
        stdout_text(&output).contains("PASS cv cluster/"),
        "{}",
        stdout_text(&output)
    );
}

#[test]
fn measure_rejects_unknown_styles() {
    let (_temp, root) = copy_workspace("ccvl-cli-measure-style-");
    let output = run(&root, &["measure", "--style", "cv/bogus"]);
    assert_eq!(output.status.code(), Some(1), "{}", stderr_text(&output));
    assert!(
        stderr_text(&output).contains("--style \"cv/bogus\" does not name an existing style"),
        "{}",
        stderr_text(&output)
    );
}

#[test]
fn build_renders_every_registered_variant() {
    let (_temp, root) = copy_workspace("ccvl-cli-build-");
    let listed = run(&root, &["list-documents"]);
    assert_eq!(listed.status.code(), Some(0));
    let expected = serde_json::from_str::<Value>(&stdout_text(&listed))
        .unwrap()
        .as_array()
        .unwrap()
        .len();
    let output = run(&root, &["build"]);
    assert_eq!(output.status.code(), Some(0), "{}", stderr_text(&output));
    let rendered = stdout_text(&output)
        .lines()
        .filter(|line| line.starts_with("Rendered "))
        .count();
    assert_eq!(rendered, expected, "{}", stdout_text(&output));
}

#[test]
fn build_refuses_a_broken_leaf() {
    let (_temp, root) = copy_workspace("ccvl-cli-build-broken-");
    fs::remove_file(root.join("outputs/cvl/cv/harvard/standard/en/ch/content.toml")).unwrap();
    let output = run(&root, &["build"]);
    assert_eq!(output.status.code(), Some(1), "{}", stderr_text(&output));
    assert!(
        stderr_text(&output).contains("incomplete cv/harvard/standard/en-ch leaf"),
        "{}",
        stderr_text(&output)
    );
}

#[test]
fn build_cv_renders_the_default_cv() {
    let (_temp, root) = copy_workspace("ccvl-cli-build-cv-");
    let destination = scratch_base().join(format!("ccvl-cli-cv-{}.pdf", std::process::id()));
    let _ = fs::remove_file(&destination);
    let output = run(
        &root,
        &[
            "build-cv",
            "en-ch",
            "--output",
            destination.to_str().unwrap(),
        ],
    );
    assert_eq!(output.status.code(), Some(0), "{}", stderr_text(&output));
    assert!(
        stdout_text(&output).contains(&format!("Rendered {}", destination.display())),
        "{}",
        stdout_text(&output)
    );
    assert!(pdf_pages(&destination) >= 1, "no pages rendered");
    fs::remove_file(&destination).unwrap();
}

#[test]
fn build_cv_rejects_unknown_styles() {
    let (_temp, root) = copy_workspace("ccvl-cli-build-cv-style-");
    let output = run(&root, &["build-cv", "en-ch", "--style", "bogus"]);
    assert_eq!(output.status.code(), Some(1), "{}", stderr_text(&output));
    assert!(
        stderr_text(&output).contains("unknown cv style \"bogus\""),
        "{}",
        stderr_text(&output)
    );
}

#[test]
fn build_cl_renders_the_default_letter() {
    let (_temp, root) = copy_workspace("ccvl-cli-build-cl-");
    let destination = scratch_base().join(format!("ccvl-cli-cl-{}.pdf", std::process::id()));
    let _ = fs::remove_file(&destination);
    let output = run(
        &root,
        &[
            "build-cl",
            "en-ch",
            "--output",
            destination.to_str().unwrap(),
        ],
    );
    assert_eq!(output.status.code(), Some(0), "{}", stderr_text(&output));
    assert!(
        stdout_text(&output).contains(&format!("Rendered {}", destination.display())),
        "{}",
        stdout_text(&output)
    );
    assert_eq!(pdf_pages(&destination), 1, "letter must stay one page");
    fs::remove_file(&destination).unwrap();
}

#[test]
fn build_cl_rejects_unknown_locales() {
    let (_temp, root) = copy_workspace("ccvl-cli-build-cl-locale-");
    let output = run(&root, &["build-cl", "en-xx"]);
    assert_eq!(output.status.code(), Some(1), "{}", stderr_text(&output));
    assert!(
        stderr_text(&output).contains("no cl leaf for harvard/frame/en-xx"),
        "{}",
        stderr_text(&output)
    );
}

#[test]
fn build_output_renders_a_generic_document() {
    let (_temp, root) = copy_workspace("ccvl-cli-build-output-");
    let destination = scratch_base().join(format!("ccvl-cli-flyer-{}.pdf", std::process::id()));
    let _ = fs::remove_file(&destination);
    let output = run(
        &root,
        &[
            "build-output",
            "flyers",
            "en-ch",
            "--output",
            destination.to_str().unwrap(),
        ],
    );
    assert_eq!(output.status.code(), Some(0), "{}", stderr_text(&output));
    assert!(
        stdout_text(&output).contains(&format!("Rendered {}", destination.display())),
        "{}",
        stdout_text(&output)
    );
    assert_eq!(pdf_pages(&destination), 1, "flyer must stay one page");
    fs::remove_file(&destination).unwrap();
}

#[test]
fn build_output_rejects_unknown_documents_and_page_counts() {
    let (_temp, root) = copy_workspace("ccvl-cli-build-output-doc-");
    let output = run(&root, &["build-output", "bogus", "en-ch"]);
    assert_eq!(output.status.code(), Some(1), "{}", stderr_text(&output));
    assert!(
        stderr_text(&output).contains("unknown document \"bogus\""),
        "{}",
        stderr_text(&output)
    );
    let output = run(&root, &["build-output", "flyers", "en-ch", "--pages", "2"]);
    assert_eq!(output.status.code(), Some(1), "{}", stderr_text(&output));
    assert!(
        stderr_text(&output).contains("flyers/scaffold: unsupported page count 2; expected [1]"),
        "{}",
        stderr_text(&output)
    );
}

// ---------------------------------------------------------------------------
// new-opportunity, measure-opportunity, build-opportunity
// ---------------------------------------------------------------------------

/// Single-segment keys (`lead`) collide with the interim hyphenated output
/// scheme when the stale-output cleanup runs, so the flow uses a hyphenated
/// position key like every real posting.
#[test]
fn new_opportunity_creates_a_keyed_record() {
    let (_temp, root) = copy_workspace("ccvl-cli-new-opportunity-");
    let output = run(&root, &["new-opportunity", "acme", "platform-lead"]);
    assert_eq!(output.status.code(), Some(0), "{}", stderr_text(&output));
    assert!(
        stdout_text(&output).contains("Created opportunities/acme/platform-lead/application.toml"),
        "{}",
        stdout_text(&output)
    );
    assert!(
        root.join("opportunities/acme/platform-lead/application.toml")
            .is_file()
    );
    assert!(
        root.join("opportunities/acme/platform-lead/posting.md")
            .is_file()
    );
}

#[test]
fn new_opportunity_never_overwrites_and_validates_keys() {
    let (_temp, root) = copy_workspace("ccvl-cli-new-opportunity-dup-");
    let first = run(&root, &["new-opportunity", "acme", "platform-lead"]);
    assert_eq!(first.status.code(), Some(0), "{}", stderr_text(&first));
    let output = run(&root, &["new-opportunity", "acme", "platform-lead"]);
    assert_eq!(output.status.code(), Some(1), "{}", stderr_text(&output));
    assert!(
        stderr_text(&output)
            .contains("refusing to overwrite existing opportunity: opportunities/acme/platform-lead/application.toml"),
        "{}",
        stderr_text(&output)
    );
    let output = run(&root, &["new-opportunity", "ACME", "platform-lead"]);
    assert_eq!(output.status.code(), Some(1), "{}", stderr_text(&output));
    assert!(
        stderr_text(&output).contains("invalid organisation key: \"ACME\""),
        "{}",
        stderr_text(&output)
    );
}

/// Fill the record the new command created: the resolved wording of an
/// existing showcase leaf plus the opportunity envelope.
fn fill_opportunity_record(root: &Path, organisation: &str, position: &str, with_letter: bool) {
    let workspace = Workspace::at(root).unwrap();
    let record_path = root.join(format!(
        "opportunities/{organisation}/{position}/application.toml"
    ));
    let mut record = read_toml(&record_path);
    let cv = content::read_record(
        &workspace,
        "outputs/cvl/cv/harvard/d-plus/en/ch/content.toml",
    )
    .unwrap();
    record["cv"]["summary"] = toml::Value::String(cv["cv"]["summary"].as_str().unwrap().to_owned());
    record["options"]["language"] = toml::Value::String("en-ch".into());
    record["options"]["application_date"] = toml::Value::String("2026-09".into());
    record["job"]["title"] = toml::Value::String("Platform Lead".into());
    record["job"]["organization"] = toml::Value::String("Acme".into());
    record["job"]["location"] = toml::Value::String("Switzerland".into());
    if with_letter {
        let cl = content::read_record(
            &workspace,
            "outputs/cvl/cl/harvard/frame/en/ch/content.toml",
        )
        .unwrap();
        // The showcase closing line invites a test outline, which opportunity
        // records forbid; name a first conversation instead at the same width.
        let fixed = toml::to_string(&cl["cl"]).unwrap().replace(
            "send me a short outline by email and suggest a time to speak.",
            "I welcome a first conversation about priorities and next steps.",
        );
        record["cl"] = toml::from_str(&fixed).unwrap();
    }
    fs::write(
        &record_path,
        format!("{}\n", toml::to_string(&record).unwrap()),
    )
    .unwrap();
}

#[test]
fn opportunity_flow_from_blank_record_to_measured_pdfs() {
    let (_temp, root) = copy_workspace("ccvl-cli-opportunity-flow-");
    let created = run(
        &root,
        &[
            "new-opportunity",
            "--no-cover-letter",
            "acme",
            "platform-lead",
        ],
    );
    assert_eq!(created.status.code(), Some(0), "{}", stderr_text(&created));
    // The blank summary refuses to build, with the record-relative message.
    let refused = run(&root, &["build-opportunity", "acme", "platform-lead"]);
    assert_eq!(refused.status.code(), Some(1), "{}", stderr_text(&refused));
    assert!(
        stderr_text(&refused).contains(
            "opportunities/acme/platform-lead/application.toml.cv.summary: a rendered summary cannot be empty"
        ),
        "{}",
        stderr_text(&refused)
    );
    fill_opportunity_record(&root, "acme", "platform-lead", false);
    let measured = run(&root, &["measure-opportunity", "acme", "platform-lead"]);
    assert_eq!(
        measured.status.code(),
        Some(0),
        "{}",
        stderr_text(&measured)
    );
    assert!(
        stdout_text(&measured).contains("PASS CV acme/platform-lead: 163 measured lines"),
        "{}",
        stdout_text(&measured)
    );
    let built = run(&root, &["build-opportunity", "acme", "platform-lead"]);
    assert_eq!(built.status.code(), Some(0), "{}", stderr_text(&built));
    let pdf = root.join("opportunities/acme/platform-lead/pdfs/CV_Corbet_Acme_Platform_Lead.pdf");
    assert!(
        stdout_text(&built).contains(&format!("Rendered {}", pdf.display())),
        "{}",
        stdout_text(&built)
    );
    assert!(pdf.is_file(), "CV pdf was not written");
    assert_eq!(pdf_pages(&pdf), 4, "CV must keep its four pages");
}

#[test]
fn build_opportunity_with_a_cover_letter_renders_both_documents() {
    let (_temp, root) = copy_workspace("ccvl-cli-opportunity-letter-");
    let created = run(&root, &["new-opportunity", "acme", "full-case"]);
    assert_eq!(created.status.code(), Some(0), "{}", stderr_text(&created));
    fill_opportunity_record(&root, "acme", "full-case", true);
    let measured = run(&root, &["measure-opportunity", "acme", "full-case"]);
    assert_eq!(
        measured.status.code(),
        Some(0),
        "{}",
        stderr_text(&measured)
    );
    assert!(
        stdout_text(&measured).contains("PASS cover letter acme/full-case: 33 measured lines"),
        "{}",
        stdout_text(&measured)
    );
    let built = run(&root, &["build-opportunity", "acme", "full-case"]);
    assert_eq!(built.status.code(), Some(0), "{}", stderr_text(&built));
    let cv = root.join("opportunities/acme/full-case/pdfs/CV_Corbet_Acme_Full_Case.pdf");
    let cl = root.join("opportunities/acme/full-case/pdfs/CL_Corbet_Acme_Full_Case.pdf");
    assert!(cv.is_file() && cl.is_file(), "{}", stdout_text(&built));
    assert_eq!(pdf_pages(&cv), 4, "CV must keep its four pages");
    assert_eq!(pdf_pages(&cl), 1, "letter must stay one page");
}

#[test]
fn build_opportunity_single_segment_keys_keep_fresh_outputs() {
    // Regression: `acme/lead` has no hyphen or underscore, so the previous
    // interim hyphenated filename equals the current one. The stale cleanup
    // must not delete the just-written PDF and .typ.
    let (_temp, root) = copy_workspace("ccvl-cli-opportunity-single-");
    let created = run(
        &root,
        &["new-opportunity", "--no-cover-letter", "acme", "lead"],
    );
    assert_eq!(created.status.code(), Some(0), "{}", stderr_text(&created));
    fill_opportunity_record(&root, "acme", "lead", false);
    let built = run(&root, &["build-opportunity", "acme", "lead"]);
    assert_eq!(built.status.code(), Some(0), "{}", stderr_text(&built));
    let pdf = root.join("opportunities/acme/lead/pdfs/CV_Corbet_Acme_Lead.pdf");
    let typ = root.join("opportunities/acme/lead/typst/CV_Corbet_Acme_Lead.typ");
    assert!(
        stdout_text(&built).contains(&format!("Rendered {}", pdf.display())),
        "{}",
        stdout_text(&built)
    );
    assert!(pdf.is_file(), "CV pdf was deleted by stale cleanup");
    assert!(typ.is_file(), "CV typ was deleted by stale cleanup");
    assert_eq!(pdf_pages(&pdf), 4, "CV must keep its four pages");
}

#[test]
fn measure_opportunity_rejects_unknown_records() {
    let (_temp, root) = copy_workspace("ccvl-cli-measure-opportunity-");
    let output = run(&root, &["measure-opportunity", "acme", "nosuchrole"]);
    assert_eq!(output.status.code(), Some(1), "{}", stderr_text(&output));
    assert!(
        stderr_text(&output).contains(
            "opportunity record does not exist: opportunities/acme/nosuchrole/application.toml"
        ),
        "{}",
        stderr_text(&output)
    );
}

// ---------------------------------------------------------------------------
// compare
// ---------------------------------------------------------------------------

#[test]
fn compare_renders_variants_side_by_side() {
    let (_temp, root) = copy_workspace("ccvl-cli-compare-");
    let output = run(&root, &["compare", "harvard/d-plus", "harvard/compact"]);
    assert_eq!(output.status.code(), Some(0), "{}", stderr_text(&output));
    assert!(
        stdout_text(&output).starts_with("Compared 2 CV variants: "),
        "{}",
        stdout_text(&output)
    );
    assert!(
        stdout_text(&output).contains("Layout: interleaved, 4 rows (pages 1-4) x 2 columns"),
        "{}",
        stdout_text(&output)
    );
}

#[test]
fn compare_rejects_unknown_substyles() {
    let (_temp, root) = copy_workspace("ccvl-cli-compare-style-");
    let output = run(&root, &["compare", "harvard/d-plus", "harvard/bogus"]);
    assert_eq!(output.status.code(), Some(1), "{}", stderr_text(&output));
    assert!(
        stderr_text(&output).contains("comparison item harvard/bogus: unknown cv substyle"),
        "{}",
        stderr_text(&output)
    );
}

// ---------------------------------------------------------------------------
// watch-cv, watch-cl, watch-opportunity
// ---------------------------------------------------------------------------

/// Run `watch-*` until it has printed two rebuilds (`outputs_per_build`
/// "Rendered " lines each: one for `watch-cv`/`watch-cl`, two — PDF plus
/// resolved Typst copy — for `watch-opportunity`), touching the profile once
/// the first rebuild lands. The deadline only bounds a stuck watcher; a
/// healthy one rebuilds within seconds of the touch.
fn watch_rebuilds(root: &Path, args: &[&str], outputs_per_build: usize, log: &Path) {
    let profile = root.join("inputs/profile.toml");
    let mut child = Command::new(BIN)
        .arg("--root")
        .arg(root)
        .args(args)
        .stdout(fs::File::create(log).unwrap())
        .stderr(fs::File::create(log.with_extension("err")).unwrap())
        .spawn()
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(120);
    let mut touched = false;
    loop {
        if Instant::now() > deadline {
            let _ = child.kill();
            panic!("watch did not rebuild in time: {}", log.display());
        }
        let text = fs::read_to_string(log).unwrap_or_default();
        let rendered = text
            .lines()
            .filter(|line| line.contains("Rendered "))
            .count();
        if rendered >= 2 * outputs_per_build {
            break;
        }
        if rendered >= outputs_per_build && !touched {
            let mut profile_text = fs::read_to_string(&profile).unwrap();
            profile_text.push('\n');
            fs::write(&profile, &profile_text).unwrap();
            touched = true;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    let _ = child.kill();
    let _ = child.wait();
    assert!(touched, "watch rebuilt twice without any input change");
    let text = fs::read_to_string(log).unwrap();
    assert!(text.contains("Watching inputs for "), "{text}");
}

#[test]
fn watch_cv_sees_one_rebuild_then_stops() {
    let (_temp, root) = copy_workspace("ccvl-cli-watch-cv-");
    let log = scratch_base().join(format!("ccvl-cli-watch-cv-{}.log", std::process::id()));
    watch_rebuilds(&root, &["watch-cv", "en-ch"], 1, &log);
}

#[test]
fn watch_cl_sees_one_rebuild_then_stops() {
    let (_temp, root) = copy_workspace("ccvl-cli-watch-cl-");
    let log = scratch_base().join(format!("ccvl-cli-watch-cl-{}.log", std::process::id()));
    watch_rebuilds(&root, &["watch-cl", "en-ch"], 1, &log);
}

#[test]
fn watch_opportunity_sees_one_rebuild_then_stops() {
    let (_temp, root) = copy_workspace("ccvl-cli-watch-opportunity-");
    let created = run(
        &root,
        &[
            "new-opportunity",
            "--no-cover-letter",
            "acme",
            "platform-lead",
        ],
    );
    assert_eq!(created.status.code(), Some(0), "{}", stderr_text(&created));
    fill_opportunity_record(&root, "acme", "platform-lead", false);
    let log = scratch_base().join(format!("ccvl-cli-watch-opp-{}.log", std::process::id()));
    watch_rebuilds(
        &root,
        &["watch-opportunity", "acme", "platform-lead"],
        2,
        &log,
    );
}

#[test]
fn watch_opportunity_rejects_invalid_keys_before_watching() {
    let (_temp, root) = copy_workspace("ccvl-cli-watch-opp-key-");
    let output = run(&root, &["watch-opportunity", "ACME", "platform-lead"]);
    assert_eq!(output.status.code(), Some(1), "{}", stderr_text(&output));
    assert!(
        stderr_text(&output).contains("invalid organisation key: \"ACME\""),
        "{}",
        stderr_text(&output)
    );
}

// ---------------------------------------------------------------------------
// fmt
// ---------------------------------------------------------------------------

#[test]
fn fmt_check_passes_a_formatted_tree() {
    let (_temp, root) = copy_workspace("ccvl-cli-fmt-check-");
    let output = run(&root, &["fmt", "--check"]);
    assert_eq!(output.status.code(), Some(0), "{}", stderr_text(&output));
}

#[test]
fn fmt_reports_nothing_to_do_on_a_formatted_tree() {
    let (_temp, root) = copy_workspace("ccvl-cli-fmt-");
    let output = run(&root, &["fmt"]);
    assert_eq!(output.status.code(), Some(0), "{}", stderr_text(&output));
    assert!(
        stdout_text(&output).contains("Formatted 0 Typst source(s)."),
        "{}",
        stdout_text(&output)
    );
}

#[test]
fn fmt_check_names_the_unformatted_source() {
    let (_temp, root) = copy_workspace("ccvl-cli-fmt-dirty-");
    let typst = root.join("outputs/cvl/cv/harvard/standard/en/ch/typst/cv.typ");
    let mut text = fs::read_to_string(&typst).unwrap();
    text.push_str("\n#let   x=1\n");
    fs::write(&typst, &text).unwrap();
    let output = run(&root, &["fmt", "--check"]);
    assert_eq!(output.status.code(), Some(1), "{}", stderr_text(&output));
    assert!(
        stderr_text(&output).contains(
            "Typst sources require formatting: outputs/cvl/cv/harvard/standard/en/ch/typst/cv.typ"
        ),
        "{}",
        stderr_text(&output)
    );
}

// ---------------------------------------------------------------------------
// review
// ---------------------------------------------------------------------------

fn review_spec() -> Value {
    json!({
        "purpose": {"kind": "general", "description": "Smoke review of the showcase CV leaf"},
        "actor_id": "smoke-actor",
        "model": "external-smoke-critic",
        "max_input_tokens": 1000,
        "max_output_tokens": 1000,
        "time_limit_seconds": 300,
        "documents": [{
            "id": "cv", "document": "cv", "locale": "en-ch", "pages": 4,
            "application": "outputs/cvl/cv/harvard/d-plus/en/ch/content.toml",
            "profile": "inputs/profile.toml",
            "style": "harvard", "substyle": "d-plus", "paper": null,
        }],
        "sources": [{"id": "smoke", "path": "evidence.txt", "role": "confirmation"}],
        "rubric": [".agent/docs/editorial.md"],
        "editable": [],
    })
}

/// A prepared review run plus its critic result, written through the same
/// files the binary reads.
fn prepare_review_run(root: &Path, run_dir: &str) {
    fs::write(root.join("evidence.txt"), "review evidence\n").unwrap();
    fs::write(
        root.join("spec.json"),
        serde_json::to_string(&review_spec()).unwrap(),
    )
    .unwrap();
    let prepared = run(root, &["review", "prepare", "spec.json", run_dir]);
    assert_eq!(
        prepared.status.code(),
        Some(0),
        "{}",
        stderr_text(&prepared)
    );
    let status: Value = serde_json::from_str(&stdout_text(&prepared)).unwrap();
    assert_eq!(status["state"], json!("incomplete"));
}

fn write_critic_result(root: &Path, run_dir: &str) {
    let state: Value = read_json(&root.join(run_dir).join("state.json"));
    let manifest: Value = read_json(&root.join(run_dir).join("revision-0/manifest.json"));
    let artifacts = manifest["artifacts"]
        .as_array()
        .unwrap()
        .iter()
        .map(|artifact| artifact["id"].clone())
        .collect::<Vec<_>>();
    let result = json!({
        "schema_version": 1,
        "revision": state["revision"],
        "manifest_sha256": state["manifest_sha256"],
        "reviewer": {
            "agent_id": "critic-1",
            "model": manifest["spec"]["model"],
            "fresh_context": true,
        },
        "coverage": {
            "artifacts_read": artifacts,
            "unavailable": [],
            "not_applicable": {"posting-specific": "No vacancy for a general CV"},
            "evidence_pass": true,
            "presentation_pass": true,
        },
        "findings": [],
        "regression_checked": true,
        "changed_artifacts_reviewed": [],
        "input_tokens": 100,
        "output_tokens": 100,
    });
    fs::write(
        root.join("result.json"),
        serde_json::to_string(&result).unwrap(),
    )
    .unwrap();
}

#[test]
fn review_runs_prepare_status_submit_and_cancel() {
    let (_temp, root) = copy_workspace("ccvl-cli-review-");
    let run_dir = ".agent/cache/review/it01";
    prepare_review_run(&root, run_dir);
    let status = run(&root, &["review", "status", run_dir]);
    assert_eq!(status.status.code(), Some(0), "{}", stderr_text(&status));
    assert!(
        stdout_text(&status).contains("awaiting independent critic result"),
        "{}",
        stdout_text(&status)
    );
    write_critic_result(&root, run_dir);
    let submitted = run(&root, &["review", "submit", run_dir, "result.json"]);
    assert_eq!(
        submitted.status.code(),
        Some(0),
        "{}",
        stderr_text(&submitted)
    );
    assert!(
        stdout_text(&submitted).contains("\"state\": \"ready\""),
        "{}",
        stdout_text(&submitted)
    );
    let begun = run(&root, &["review", "begin-revision", run_dir]);
    assert_eq!(begun.status.code(), Some(0), "{}", stderr_text(&begun));
    assert!(
        stdout_text(&begun).contains("\"corrections_used\": 1"),
        "{}",
        stdout_text(&begun)
    );
    let revised = run(&root, &["review", "prepare-revision", run_dir]);
    assert_eq!(revised.status.code(), Some(0), "{}", stderr_text(&revised));
    assert!(
        stdout_text(&revised).contains("\"revision\": 1"),
        "{}",
        stdout_text(&revised)
    );
    let cancelled = run(&root, &["review", "cancel", run_dir]);
    assert_eq!(
        cancelled.status.code(),
        Some(0),
        "{}",
        stderr_text(&cancelled)
    );
    assert!(
        stdout_text(&cancelled).contains("run cancelled"),
        "{}",
        stdout_text(&cancelled)
    );
}

#[test]
fn review_prepare_never_reuses_a_run_directory() {
    let (_temp, root) = copy_workspace("ccvl-cli-review-prepare-dup-");
    let run_dir = ".agent/cache/review/it01";
    prepare_review_run(&root, run_dir);
    let output = run(&root, &["review", "prepare", "spec.json", run_dir]);
    assert_eq!(output.status.code(), Some(1), "{}", stderr_text(&output));
    assert!(
        stderr_text(&output).contains("review run already exists; choose a new run ID"),
        "{}",
        stderr_text(&output)
    );
}

#[test]
fn review_status_only_accepts_review_directories() {
    let (_temp, root) = copy_workspace("ccvl-cli-review-status-path-");
    let output = run(&root, &["review", "status", "outputs/cvl/review/run"]);
    assert_eq!(output.status.code(), Some(1), "{}", stderr_text(&output));
    assert!(
        stderr_text(&output).contains(
            "review run must be .agent/cache/review/<id> or opportunities/<organisation>/<position>/review/<id>"
        ),
        "{}",
        stderr_text(&output)
    );
}

#[test]
fn review_submit_never_accepts_a_second_result() {
    let (_temp, root) = copy_workspace("ccvl-cli-review-submit-twice-");
    let run_dir = ".agent/cache/review/it01";
    prepare_review_run(&root, run_dir);
    write_critic_result(&root, run_dir);
    let first = run(&root, &["review", "submit", run_dir, "result.json"]);
    assert_eq!(first.status.code(), Some(0), "{}", stderr_text(&first));
    let output = run(&root, &["review", "submit", run_dir, "result.json"]);
    assert_eq!(output.status.code(), Some(1), "{}", stderr_text(&output));
    assert!(
        stderr_text(&output).contains("this revision already has an immutable review result"),
        "{}",
        stderr_text(&output)
    );
}

#[test]
fn review_prepare_revision_needs_a_reserved_correction() {
    let (_temp, root) = copy_workspace("ccvl-cli-review-prepare-rev-");
    let run_dir = ".agent/cache/review/it01";
    prepare_review_run(&root, run_dir);
    let output = run(&root, &["review", "prepare-revision", run_dir]);
    assert_eq!(output.status.code(), Some(1), "{}", stderr_text(&output));
    assert!(
        stderr_text(&output).contains("reserve a correction with begin-revision before editing"),
        "{}",
        stderr_text(&output)
    );
}

#[test]
fn review_begin_revision_needs_a_rendered_candidate_first() {
    let (_temp, root) = copy_workspace("ccvl-cli-review-begin-twice-");
    let run_dir = ".agent/cache/review/it01";
    prepare_review_run(&root, run_dir);
    // No critic result yet, so no correction can be reserved.
    let output = run(&root, &["review", "begin-revision", run_dir]);
    assert_eq!(output.status.code(), Some(1), "{}", stderr_text(&output));
    assert!(
        stderr_text(&output)
            .contains("review the rendered candidate before reserving another correction"),
        "{}",
        stderr_text(&output)
    );
}

// ---------------------------------------------------------------------------
// skill-eval
// ---------------------------------------------------------------------------

fn skill_decisions(cases: &Value, wrong_skill: bool) -> Value {
    let decisions = cases["cases"]
        .as_array()
        .unwrap()
        .iter()
        .enumerate()
        .map(|(index, case)| {
            let required = case["required"]
                .as_array()
                .map(|required| {
                    required
                        .iter()
                        .filter_map(Value::as_str)
                        .collect::<std::collections::BTreeSet<_>>()
                })
                .unwrap_or_default();
            let assessments = case["options"]
                .as_array()
                .unwrap()
                .iter()
                .map(|option| {
                    let id = option["id"].as_str().unwrap();
                    json!({
                        "id": id,
                        "applicable": required.contains(id),
                        "reason": "offline fixture assessment",
                    })
                })
                .collect::<Vec<_>>();
            let expected = case["skill"].as_str().unwrap();
            let skill = if wrong_skill && index == 0 {
                if expected == "ccvl-review" {
                    "ccvl-install"
                } else {
                    "ccvl-review"
                }
            } else {
                expected
            };
            json!({
                "case_id": case["id"],
                "skill": skill,
                "reason": "offline fixture decision",
                "assessments": assessments,
            })
        })
        .collect::<Vec<_>>();
    json!({"decisions": decisions})
}

#[test]
fn skill_eval_scores_a_saved_response_offline() {
    let (_temp, root) = copy_workspace("ccvl-cli-skill-eval-");
    let cases = read_json(&root.join(".agent/tests/skill-cases.json"));
    let response_file =
        scratch_base().join(format!("ccvl-cli-response-{}.json", std::process::id()));
    fs::write(
        &response_file,
        serde_json::to_string(&skill_decisions(&cases, false)).unwrap(),
    )
    .unwrap();
    let report = scratch_base().join(format!("ccvl-cli-report-{}.json", std::process::id()));
    let output = run(
        &root,
        &[
            "skill-eval",
            "--response-file",
            response_file.to_str().unwrap(),
            "--output",
            report.to_str().unwrap(),
        ],
    );
    assert_eq!(output.status.code(), Some(0), "{}", stderr_text(&output));
    assert!(
        stdout_text(&output).contains(&format!(
            "Skill evaluation report: {} (passed)",
            report.display()
        )),
        "{}",
        stdout_text(&output)
    );
    let report = read_json(&report);
    assert_eq!(report["status"], json!("passed"));
    assert_eq!(report["source"], json!("response-file"));
}

#[test]
fn skill_eval_reports_a_missing_response_as_a_configuration_error() {
    let (_temp, root) = copy_workspace("ccvl-cli-skill-eval-missing-");
    let missing = scratch_base().join("ccvl-cli-no-such-response.json");
    let _ = fs::remove_file(&missing);
    let report = scratch_base().join(format!(
        "ccvl-cli-report-missing-{}.json",
        std::process::id()
    ));
    let output = run(
        &root,
        &[
            "skill-eval",
            "--response-file",
            missing.to_str().unwrap(),
            "--output",
            report.to_str().unwrap(),
        ],
    );
    assert_eq!(output.status.code(), Some(2), "{}", stderr_text(&output));
    assert!(
        stdout_text(&output).contains("(configuration_error)"),
        "{}",
        stdout_text(&output)
    );
    let report = read_json(&report);
    assert_eq!(report["status"], json!("configuration_error"));
    assert!(
        report["provider_note"]
            .as_str()
            .unwrap()
            .contains("cannot read JSON file"),
        "{report}"
    );
}

#[test]
fn skill_eval_fails_a_misrouted_decision() {
    let (_temp, root) = copy_workspace("ccvl-cli-skill-eval-failed-");
    let cases = read_json(&root.join(".agent/tests/skill-cases.json"));
    let response_file =
        scratch_base().join(format!("ccvl-cli-response-bad-{}.json", std::process::id()));
    fs::write(
        &response_file,
        serde_json::to_string(&skill_decisions(&cases, true)).unwrap(),
    )
    .unwrap();
    let report = scratch_base().join(format!("ccvl-cli-report-bad-{}.json", std::process::id()));
    let output = run(
        &root,
        &[
            "skill-eval",
            "--response-file",
            response_file.to_str().unwrap(),
            "--output",
            report.to_str().unwrap(),
        ],
    );
    assert_eq!(output.status.code(), Some(1), "{}", stderr_text(&output));
    assert!(
        stdout_text(&output).contains("(failed)"),
        "{}",
        stdout_text(&output)
    );
    assert_eq!(read_json(&report)["status"], json!("failed"));
}

// ---------------------------------------------------------------------------
// export-style, render-style
// ---------------------------------------------------------------------------

#[test]
fn export_style_writes_a_versioned_bundle() {
    let (_temp, root) = copy_workspace("ccvl-cli-export-style-");
    let bundle = scratch_base().join(format!("ccvl-cli-bundle-{}.json", std::process::id()));
    let _ = fs::remove_file(&bundle);
    let output = run(
        &root,
        &[
            "export-style",
            "cl",
            "en-ch",
            "--style",
            "harvard",
            "--substyle",
            "frame",
            "--output",
            bundle.to_str().unwrap(),
        ],
    );
    assert_eq!(output.status.code(), Some(0), "{}", stderr_text(&output));
    assert!(
        stdout_text(&output).starts_with("cl/harvard/frame/en-ch/1/a4 sha256:"),
        "{}",
        stdout_text(&output)
    );
    let bundle_text = fs::read_to_string(&bundle).unwrap();
    assert!(bundle_text.contains("\"schema_version\""), "{bundle_text}");
}

#[test]
fn export_style_refuses_candidate_sources() {
    let (_temp, root) = copy_workspace("ccvl-cli-export-style-sources-");
    let bundle = scratch_base().join(format!("ccvl-cli-bundle-no-{}.json", std::process::id()));
    let output = run(
        &root,
        &[
            "export-style",
            "cv",
            "en-ch",
            "--style",
            "harvard",
            "--substyle",
            "standard",
            "--output",
            bundle.to_str().unwrap(),
        ],
    );
    assert_eq!(output.status.code(), Some(1), "{}", stderr_text(&output));
    assert!(
        stderr_text(&output)
            .contains("this style embeds candidate source files; move them to document inputs before exporting a reusable bundle"),
        "{}",
        stderr_text(&output)
    );
}

#[test]
fn render_style_compiles_an_exported_bundle_standalone() {
    let (_temp, root) = copy_workspace("ccvl-cli-render-style-");
    let bundle = scratch_base().join(format!("ccvl-cli-bundle-cv-{}.json", std::process::id()));
    let exported = run(
        &root,
        &[
            "export-style",
            "cv",
            "en-ch",
            "--style",
            "modern",
            "--substyle",
            "timeline",
            "--output",
            bundle.to_str().unwrap(),
        ],
    );
    assert_eq!(
        exported.status.code(),
        Some(0),
        "{}",
        stderr_text(&exported)
    );
    // The consumer record carries resolved wording: no shared `wording` key.
    let workspace = Workspace::at(&root).unwrap();
    let resolved = content::read_record(
        &workspace,
        "outputs/cvl/cv/modern/timeline/en/ch/content.toml",
    )
    .unwrap();
    let application =
        scratch_base().join(format!("ccvl-cli-application-{}.toml", std::process::id()));
    fs::write(
        &application,
        format!("{}\n", toml::to_string(&resolved).unwrap()),
    )
    .unwrap();
    let document = scratch_base().join(format!("ccvl-cli-rendered-{}.pdf", std::process::id()));
    let _ = fs::remove_file(&document);
    let profile = root.join("inputs/profile.toml");
    let output = run_cwd(
        &root,
        &[
            "render-style",
            "--bundle",
            bundle.to_str().unwrap(),
            "--application",
            application.to_str().unwrap(),
            "--profile",
            profile.to_str().unwrap(),
            "--output",
            document.to_str().unwrap(),
        ],
    );
    assert_eq!(
        output.status.code(),
        Some(0),
        "{}{}",
        stdout_text(&output),
        stderr_text(&output)
    );
    assert!(stdout_text(&output).is_empty(), "{}", stdout_text(&output));
    assert_eq!(pdf_pages(&document), 1, "rendered bundle must be one page");
}

#[test]
fn render_style_needs_its_bundle_file() {
    let (_temp, root) = copy_workspace("ccvl-cli-render-style-bundle-");
    let missing = scratch_base().join("ccvl-cli-no-such-bundle.json");
    let _ = fs::remove_file(&missing);
    let document = scratch_base().join(format!("ccvl-cli-rendered-no-{}.pdf", std::process::id()));
    let output = run_cwd(
        &root,
        &[
            "render-style",
            "--bundle",
            missing.to_str().unwrap(),
            "--application",
            "outputs/cvl/cv/modern/timeline/en/ch/content.toml",
            "--profile",
            "inputs/profile.toml",
            "--output",
            document.to_str().unwrap(),
        ],
    );
    assert_eq!(output.status.code(), Some(1), "{}", stderr_text(&output));
    assert!(
        stderr_text(&output).contains("No such file or directory"),
        "{}",
        stderr_text(&output)
    );
}

// ---------------------------------------------------------------------------
// downstream-check
// ---------------------------------------------------------------------------

fn git(root: &Path, args: &[&str]) {
    let output = Command::new("git")
        .args(args)
        .current_dir(root)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

fn downstream_fixture(prefix: &str) -> (tempfile::TempDir, PathBuf) {
    let (temp, root) = copy_workspace(prefix);
    git(&root, &["init", "-q", "-b", "main"]);
    git(&root, &["config", "user.name", "ccvl test"]);
    git(&root, &["config", "user.email", "ccvl@example.invalid"]);
    git(&root, &["config", "commit.gpgsign", "false"]);
    git(&root, &["add", "-A"]);
    git(&root, &["commit", "-qm", "upstream"]);
    let upstream = String::from_utf8(
        Command::new("git")
            .args(["rev-parse", "HEAD"])
            .current_dir(&root)
            .output()
            .unwrap()
            .stdout,
    )
    .unwrap();
    let upstream = upstream.trim().to_owned();
    git(
        &root,
        &[
            "update-ref",
            "refs/remotes/upstream/main",
            upstream.as_str(),
        ],
    );
    git(
        &root,
        &[
            "remote",
            "add",
            "upstream",
            "https://forge.example.invalid/x/careervector-cli.git",
        ],
    );
    fs::create_dir_all(root.join("downstream")).unwrap();
    fs::write(root.join("downstream/memo.md"), "private memo\n").unwrap();
    fs::write(
        root.join("ccvl-downstream.json"),
        concat!(
            "{\"schema_version\": 1, \"upstream\": {\"remote\": \"upstream\", ",
            "\"url\": \"https://forge.example.invalid/x/careervector-cli.git\", ",
            "\"branch\": \"main\"}, ",
            "\"allowed_paths\": [\"ccvl-downstream.json\", \"downstream/\"]}"
        ),
    )
    .unwrap();
    git(
        &root,
        &["add", "ccvl-downstream.json", "downstream/memo.md"],
    );
    git(&root, &["commit", "-qm", "private delta"]);
    (temp, root)
}

#[test]
fn downstream_check_passes_an_owned_delta() {
    let (_temp, root) = downstream_fixture("ccvl-cli-downstream-");
    let output = run(
        &root,
        &[
            "downstream-check",
            "--upstream-ref",
            "refs/remotes/upstream/main",
        ],
    );
    assert_eq!(output.status.code(), Some(0), "{}", stderr_text(&output));
    assert!(
        stdout_text(&output).contains(
            "Downstream boundary passed: 2 owned path(s) differ from refs/remotes/upstream/main."
        ),
        "{}",
        stdout_text(&output)
    );
}

#[test]
fn downstream_check_rejects_upstream_owned_edits() {
    let (_temp, root) = downstream_fixture("ccvl-cli-downstream-edit-");
    fs::write(root.join("README.md"), "downstream edit\n").unwrap();
    git(&root, &["add", "README.md"]);
    git(&root, &["commit", "-qm", "forbidden"]);
    let output = run(
        &root,
        &[
            "downstream-check",
            "--upstream-ref",
            "refs/remotes/upstream/main",
        ],
    );
    assert_eq!(output.status.code(), Some(1), "{}", stderr_text(&output));
    assert!(
        stderr_text(&output).contains("downstream modifies upstream-owned paths: README.md"),
        "{}",
        stderr_text(&output)
    );
}

#[test]
fn downstream_check_needs_its_policy_file() {
    let (_temp, root) = copy_workspace("ccvl-cli-downstream-policy-");
    let output = run(&root, &["downstream-check"]);
    assert_eq!(output.status.code(), Some(1), "{}", stderr_text(&output));
    assert!(
        stderr_text(&output).contains("cannot resolve ccvl-downstream.json"),
        "{}",
        stderr_text(&output)
    );
}

// ---------------------------------------------------------------------------
// setup
// ---------------------------------------------------------------------------

#[test]
fn setup_verifies_the_ready_workspace() {
    let (_temp, root) = copy_workspace("ccvl-cli-setup-");
    let output = run(&root, &["setup"]);
    assert_eq!(output.status.code(), Some(0), "{}", stderr_text(&output));
    assert!(
        stdout_text(&output).contains("cv is ready. No external runtime or fonts are required."),
        "{}",
        stdout_text(&output)
    );
}

#[test]
fn setup_reports_the_first_failing_gate() {
    let (_temp, root) = copy_workspace("ccvl-cli-setup-broken-");
    fs::remove_file(root.join("inputs/profile.toml")).unwrap();
    let output = run(&root, &["setup"]);
    assert_eq!(output.status.code(), Some(1), "{}", stderr_text(&output));
    assert!(
        stderr_text(&output).contains("cv.json: missing referenced file inputs/profile.toml"),
        "{}",
        stderr_text(&output)
    );
}

// ---------------------------------------------------------------------------
// readable Typst errors (ctypst formats message plus file and line)
// ---------------------------------------------------------------------------

/// An overflowing summary must read as its message, never as the `Debug`
/// struct of the whole diagnostic list.
fn assert_readable_typst_error(output: &Output) {
    assert_eq!(output.status.code(), Some(1), "{}", stderr_text(output));
    let text = stderr_text(output);
    assert!(
        text.contains("application.cv.summary cannot pack into 5 lines"),
        "{text}"
    );
    assert!(text.contains(".typ:"), "{text}");
    assert!(!text.contains("SourceDiagnostic"), "{text}");
    assert!(!text.contains("span:"), "{text}");
}

#[test]
fn measure_opportunity_reads_the_fitting_failure() {
    let (_temp, root) = copy_workspace("ccvl-cli-readable-opportunity-");
    let created = run(
        &root,
        &[
            "new-opportunity",
            "--no-cover-letter",
            "acme",
            "overflow-case",
        ],
    );
    assert_eq!(created.status.code(), Some(0), "{}", stderr_text(&created));
    let workspace = Workspace::at(&root).unwrap();
    let cv = content::read_record(
        &workspace,
        "outputs/cvl/cv/harvard/d-plus/en/ch/content.toml",
    )
    .unwrap();
    let summary = cv["cv"]["summary"].as_str().unwrap();
    let overflowing = [summary, summary, summary, summary].join(" ");
    let record_path = root.join("opportunities/acme/overflow-case/application.toml");
    let mut record = read_toml(&record_path);
    record["cv"]["summary"] = toml::Value::String(overflowing);
    record["options"]["language"] = toml::Value::String("en-ch".into());
    record["options"]["application_date"] = toml::Value::String("2026-09".into());
    fs::write(
        &record_path,
        format!("{}\n", toml::to_string(&record).unwrap()),
    )
    .unwrap();
    let output = run(&root, &["measure-opportunity", "acme", "overflow-case"]);
    assert_readable_typst_error(&output);
}

#[test]
fn measure_reads_the_fitting_failure_on_a_workspace_leaf() {
    let (_temp, root) = copy_workspace("ccvl-cli-readable-leaf-");
    let workspace = Workspace::at(&root).unwrap();
    let cv = content::read_record(
        &workspace,
        "outputs/cvl/cv/harvard/d-plus/en/ch/content.toml",
    )
    .unwrap();
    let summary = cv["cv"]["summary"].as_str().unwrap();
    let overflowing = [summary, summary, summary, summary].join(" ");
    let wording = root.join("outputs/cvl/cv/harvard/content/en/ch/wording.toml");
    let mut shared = read_toml(&wording);
    shared["cv"]["summary"] = toml::Value::String(overflowing);
    fs::write(&wording, format!("{}\n", toml::to_string(&shared).unwrap())).unwrap();
    let output = run(&root, &["measure", "--style", "cv/harvard"]);
    assert_readable_typst_error(&output);
}

// ---------------------------------------------------------------------------
// archive error branches (unit-level, through the public library surface)
// ---------------------------------------------------------------------------

#[test]
fn archive_env_round_trips_process_pairs() {
    let env = archive::Env::from_pairs([("CCVL_TEST_A", "1"), ("CCVL_TEST_B", "2")]);
    assert_eq!(env.get("CCVL_TEST_A"), Some("1"));
    assert_eq!(env.nonempty("CCVL_TEST_MISSING"), None);
    assert_eq!(
        env.first_nonempty(&["CCVL_TEST_MISSING", "CCVL_TEST_B"]),
        Some("2")
    );
    let process = archive::Env::from_process();
    assert!(process.get("PATH").is_some() || process.pairs().count() > 0);
    let mut owned = env;
    owned.set("CCVL_TEST_A", "3");
    assert_eq!(owned.get("CCVL_TEST_A"), Some("3"));
    owned.remove("CCVL_TEST_B");
    assert_eq!(owned.get("CCVL_TEST_B"), None);
    assert!(owned.pairs().any(|(key, _)| key == "CCVL_TEST_A"));
}

#[test]
fn archive_read_rejects_garbage_and_missing_files() {
    // A short read is an empty archive; a full block with a bad checksum fails.
    assert!(archive::read(&[b'x'; 512]).is_err());
    assert!(archive::file_sha256(&scratch_base().join("ccvl-cli-no-such-file")).is_err());
}

#[test]
fn archive_write_read_round_trip_keeps_comment_modes_and_names() {
    let bytes = archive::write_with_comment(
        Some("test-commit"),
        [
            ("a.txt", 0o644, b"hello".as_slice()),
            ("b/c.txt", 0o755, b"world".as_slice()),
        ],
    );
    let parsed = archive::read(&bytes).unwrap();
    assert_eq!(parsed.comment.as_deref(), Some("test-commit"));
    assert_eq!(parsed.entries.len(), 2);
    assert!(archive::unsafe_member_path("../escape"));
    assert!(!archive::unsafe_member_path("b/c.txt"));
    let entries = parsed
        .entries
        .iter()
        .map(|entry| entry.name.clone())
        .collect::<Vec<_>>();
    assert_eq!(entries, ["a.txt", "b/c.txt"]);
}

#[test]
fn archive_tar_gz_writes_a_readable_gzip_member() {
    let path = scratch_base().join(format!("ccvl-cli-archive-{}.tar.gz", std::process::id()));
    let _ = fs::remove_file(&path);
    archive::write_tar_gz(&path, [("a.txt", 0o644, b"hello".as_slice())]).unwrap();
    let bytes = fs::read(&path).unwrap();
    assert_eq!(&bytes[..2], b"\x1f\x8b");
    let mut decoder = flate2::read::GzDecoder::new(&bytes[..]);
    let mut tar = Vec::new();
    std::io::Read::read_to_end(&mut decoder, &mut tar).unwrap();
    assert_eq!(archive::read(&tar).unwrap().entries.len(), 1);
    assert!(archive::write_tar_gz(&path, [("a.txt", 0o644, b"hello".as_slice())]).is_err());
    fs::remove_file(&path).unwrap();
}

#[test]
fn archive_command_runs_and_absolute_resolves() {
    let root = scratch_base();
    assert_eq!(
        archive::command_text(&root, Path::new("true"), &[]).unwrap(),
        ""
    );
    assert!(archive::command_text(&root, Path::new("false"), &[]).is_err());
    assert!(archive::command_text(&root, Path::new("ccvl-cli-no-such-program"), &[]).is_err());
    let resolved = archive::absolute(Path::new("relative-entry")).unwrap();
    assert!(
        resolved.ends_with("relative-entry"),
        "{}",
        resolved.display()
    );
}

#[test]
fn archive_dumps_sorts_keys_and_indents() {
    let value = json!({"b": 1, "a": [true, "x"]});
    assert_eq!(
        archive::dumps(&value, None),
        "{\"a\": [true, \"x\"], \"b\": 1}"
    );
    let indented = archive::dumps(&value, Some(2));
    assert!(indented.contains("\n  \"a\""), "{indented}");
}
