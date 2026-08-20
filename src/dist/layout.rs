//! Where this installation's files are.
//!
//! Every path here is read from the module that owns it — the config directory
//! from `Config`, the credentials file from the token store — rather than
//! reassembled. A second derivation of `~/.config/atlassian-cli` is how a
//! command ends up reporting on one directory while another writes a different
//! one.

use std::path::{Path, PathBuf};

use super::DistError;
use super::skill::SKILL_NAME;
use crate::config::Config;

/// The installation the running binary belongs to.
#[derive(Debug, Clone)]
pub struct Installation {
    binary: PathBuf,
    home: Option<PathBuf>,
}

impl Installation {
    pub fn detect() -> Result<Self, DistError> {
        let binary =
            std::env::current_exe().map_err(|e| DistError::io("locating the running binary", e))?;
        Ok(Self {
            // Follows a shim: a version manager puts one on `PATH` and the real
            // file is what a report has to name and a removal has to unlink.
            // Where it cannot be resolved, `current_exe` still names a path to
            // the running binary — the shim, if there is one — which is a worse
            // answer than the real file and a better one than no installation.
            binary: binary.canonicalize().unwrap_or(binary),
            home: dirs::home_dir(),
        })
    }

    /// An installation at explicit paths, so a caller can be exercised against
    /// temporary directories instead of the machine it is running on.
    pub fn at(binary: PathBuf, home: Option<PathBuf>) -> Self {
        Self { binary, home }
    }

    pub fn binary(&self) -> &Path {
        &self.binary
    }

    /// `~/.claude/skills/jira-confluence`, or `None` where there is no home to
    /// hang it off.
    pub fn skill_dir(&self) -> Option<PathBuf> {
        Some(
            self.home
                .as_ref()?
                .join(".claude")
                .join("skills")
                .join(SKILL_NAME),
        )
    }

    pub fn config_dir(&self) -> Option<PathBuf> {
        Some(Config::global_config_dir_in(self.home.as_ref()?))
    }

    pub fn config_file(&self) -> Option<PathBuf> {
        Some(self.config_dir()?.join(Config::GLOBAL_CONFIG_FILE))
    }

    pub fn credentials_file(&self) -> Option<PathBuf> {
        Some(self.config_dir()?.join(crate::auth::CREDENTIALS_FILE))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_installation_names_the_binary_that_is_running() {
        let installation = Installation::detect().unwrap();
        assert!(
            installation.binary().is_file(),
            "{}",
            installation.binary().display()
        );
    }

    /// The skill directory, the config directory and the credentials file are
    /// three answers that have to agree with the modules that own them.
    #[test]
    fn every_owned_path_hangs_off_the_directory_its_owner_uses() {
        let installation = Installation::detect().unwrap();
        let Some(home) = dirs::home_dir() else {
            return;
        };
        assert_eq!(
            installation.skill_dir(),
            Some(home.join(".claude/skills/jira-confluence"))
        );
        assert_eq!(installation.config_dir(), Config::global_config_dir());
        assert_eq!(
            installation
                .config_file()
                .and_then(|f| f.parent().map(Path::to_path_buf)),
            Config::global_config_dir()
        );
        assert_eq!(
            installation
                .credentials_file()
                .and_then(|f| f.parent().map(Path::to_path_buf)),
            Config::global_config_dir()
        );
    }
}
