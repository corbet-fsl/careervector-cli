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
