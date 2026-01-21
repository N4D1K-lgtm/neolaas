/// Crate version from Cargo.toml
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Git commit SHA (short)
pub const GIT_SHA: &str = env!("VERGEN_GIT_SHA");

/// Git commit timestamp
pub const GIT_COMMIT_TIMESTAMP: &str = env!("VERGEN_GIT_COMMIT_TIMESTAMP");

/// Git branch name
pub const GIT_BRANCH: &str = env!("VERGEN_GIT_BRANCH");

/// Whether the working tree had uncommitted changes
pub const GIT_DIRTY: &str = env!("VERGEN_GIT_DIRTY");

/// Rust compiler version used to build
pub const RUSTC_VERSION: &str = env!("VERGEN_RUSTC_SEMVER");

/// Build timestamp
pub const BUILD_TIMESTAMP: &str = env!("VERGEN_BUILD_TIMESTAMP");

/// Target triple
pub const TARGET: &str = env!("VERGEN_CARGO_TARGET_TRIPLE");

/// Protocol version string for libp2p identify.
pub const PROTOCOL_VERSION: &str = concat!("/neolaas/", env!("CARGO_PKG_VERSION"));

/// Returns full version string with git metadata.
/// Format: <version> (<git_sha>) [dirty]
pub fn full_version() -> String {
    let dirty = if GIT_DIRTY == "true" { " dirty" } else { "" };
    format!("{VERSION} ({GIT_SHA}{dirty})")
}

/// Returns detailed build information for diagnostics.
pub fn build_info() -> String {
    format!(
        "neolaas-server {}\n\
         commit: {} ({})\n\
         branch: {}\n\
         built:  {}\n\
         rustc:  {}\n\
         target: {}",
        VERSION, GIT_SHA, GIT_COMMIT_TIMESTAMP, GIT_BRANCH, BUILD_TIMESTAMP, RUSTC_VERSION, TARGET
    )
}
