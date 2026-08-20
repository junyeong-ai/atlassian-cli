//! What an installation of this tool consists of, and how it replaces or
//! removes itself.
//!
//! Everything the update path needs runs in-process: the download is the
//! binary's own reqwest/rustls stack, the checksum is `sha2`, the archive is
//! read by `flate2`/`tar`. Nothing is shelled out to, so an update works
//! wherever the binary itself does — the one exception is the opt-in build
//! provenance check, which is `gh attestation verify` because that is the
//! command a person would run by hand to answer the same question.

pub mod archive;
pub mod layout;
pub mod release;
pub mod skill;
pub mod target;
pub mod update;
pub mod verify;

pub use layout::Installation;
pub use release::{ReleaseClient, parse_tag};
pub use target::ReleaseTarget;
pub use update::{Decision, Staging, decide, fetch_verified_binary, install};

use semver::Version;

#[derive(Debug)]
pub enum DistError {
    /// The running platform, or the running installation, is outside what this
    /// path supports. Always names the alternative, because the caller cannot
    /// discover it from a refusal.
    Unsupported(String),
    Network(String),
    Release(String),
    /// The bytes that arrived are not the bytes the release published. Never a
    /// warning: an unverified binary is the one thing this must not install.
    Integrity(String),
    Io {
        context: String,
        source: std::io::Error,
    },
}

impl DistError {
    pub(crate) fn io(context: impl Into<String>, source: std::io::Error) -> Self {
        Self::Io {
            context: context.into(),
            source,
        }
    }
}

impl std::fmt::Display for DistError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DistError::Unsupported(message)
            | DistError::Network(message)
            | DistError::Release(message)
            | DistError::Integrity(message) => f.write_str(message),
            // Not the source: `source()` exposes it, and anyhow's chain
            // rendering would print it a second time.
            DistError::Io { context, .. } => f.write_str(context),
        }
    }
}

impl std::error::Error for DistError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            DistError::Io { source, .. } => Some(source),
            _ => None,
        }
    }
}

/// The version of the binary this code is compiled into — what every comparison
/// in this module is made against.
pub fn current_version() -> Version {
    Version::parse(env!("CARGO_PKG_VERSION")).expect("the crate version is semver")
}
