pub mod auth;
pub mod client;
pub mod config;
pub mod confluence;
pub mod dist;
pub mod filter;
pub(crate) mod http_utils;
pub mod jira;
pub mod markdown;
pub(crate) mod query_utils;
pub(crate) mod response;

#[cfg(test)]
pub mod test_utils;

/// Whether anything is at this path.
///
/// `Path::exists` answers false to two different questions — nothing there, and
/// could not tell — and resolves a symlink besides, so a dangling one reads as
/// nothing there. Every caller here decides something on the answer: which
/// config a command runs against, and what an uninstall removes. Absence is
/// concluded from the two definite answers only — the name is not there, or the
/// name is inside something that is not a directory.
pub fn path_present(path: &std::path::Path) -> anyhow::Result<bool> {
    match std::fs::symlink_metadata(path) {
        Ok(_) => Ok(true),
        Err(e)
            if matches!(
                e.kind(),
                std::io::ErrorKind::NotFound | std::io::ErrorKind::NotADirectory
            ) =>
        {
            Ok(false)
        }
        Err(e) => Err(anyhow::Error::from(e).context(format!("Failed to read {path:?}"))),
    }
}

pub use auth::{AuthConfig, AuthStrategy};
pub use client::ApiClient;
pub use client::ApiError;
pub use client::Service;
pub use config::CliOverrides;
pub use config::Config;
