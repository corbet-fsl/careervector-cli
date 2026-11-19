//! Deterministic core for ccvl workspaces.

pub mod application;
pub mod archive;
pub mod bundle;
pub use ccvl_engine as core;
pub mod check;
pub mod cli;
pub mod compare;
pub mod content;
pub mod downstream;
mod engine_package;
#[allow(dead_code)] // shared with build.rs; the tree walk is only used there and in tests
pub mod fileset_source;
pub mod format;
pub mod init;
pub mod layout;
pub mod lint;
pub mod measure;
pub mod opportunity;
pub mod ownership;
pub mod paper;
pub mod pdf;
pub mod portable;
pub mod private_check;
pub mod public;
pub mod release_evidence;
pub mod render;
pub mod review;
pub mod runtime;
mod runtime_source;
pub mod settings;
pub mod skills;
pub mod stations;
pub mod styles;
pub mod update;
pub mod workspace;

pub use workspace::Workspace;

mod watch;

#[cfg(test)]
mod test_support;
