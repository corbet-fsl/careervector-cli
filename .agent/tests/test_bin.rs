//! Where the `cv` binary under test is.
//!
//! Cargo bakes the build host's path into `CARGO_BIN_EXE_cv`. The Windows suite is
//! built on Linux and run in an offline Windows clone, where that path does not exist,
//! so `windows-clone-suite.ps1` names the copy of the binary that travelled with the
//! tests in `CCVL_TEST_BIN`. Everywhere else the baked-in path is used.
use std::path::PathBuf;
use std::sync::LazyLock;

pub static BIN: LazyLock<PathBuf> = LazyLock::new(|| {
    std::env::var_os("CCVL_TEST_BIN")
        .filter(|value| !value.is_empty())
        .map_or_else(|| PathBuf::from(env!("CARGO_BIN_EXE_cv")), PathBuf::from)
});

/// Whether Git is installed. Only Windows (the offline test clone) or a run with
/// `CCVL_ALLOW_NO_GIT` may skip the tests that drive Git; elsewhere a missing Git fails
/// the test loudly instead of passing it silently.
#[allow(dead_code)] // each test crate that includes this file uses a different part
pub fn git_or_skip() -> bool {
    let installed = std::process::Command::new("git")
        .arg("--version")
        .output()
        .is_ok_and(|output| output.status.success());
    if installed {
        return true;
    }
    assert!(
        cfg!(windows) || std::env::var_os("CCVL_ALLOW_NO_GIT").is_some(),
        "Git is required by this test and is not installed (set CCVL_ALLOW_NO_GIT to skip where that is expected)"
    );
    eprintln!("skipped: git is not installed on this machine");
    false
}
