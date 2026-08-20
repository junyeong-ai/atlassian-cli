//! Holding a downloaded archive to what the release published.

use std::path::Path;
use std::process::{Command, Stdio};

use sha2::{Digest, Sha256};

use super::DistError;
use super::release::REPO;

pub fn sha256_hex(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    hasher
        .finalize()
        .iter()
        .fold(String::with_capacity(64), |mut hex, byte| {
            use std::fmt::Write;
            let _ = write!(hex, "{byte:02x}");
            hex
        })
}

/// Hold `archive` to the digest published beside it.
///
/// The sidecar is paired with the archive by URL rather than by the filename
/// written inside it, so only the digest is read — a sidecar belonging to some
/// other artifact fails on the comparison either way. The digest is the one
/// thing here that must not be advisory: an archive that cannot be shown to be
/// the published one is not installed.
pub fn verify_sidecar(archive: &[u8], sidecar: &str, archive_name: &str) -> Result<(), DistError> {
    let published = sidecar
        .split_whitespace()
        .next()
        .map(str::to_ascii_lowercase)
        .filter(|digest| digest.len() == 64 && digest.chars().all(|c| c.is_ascii_hexdigit()))
        .ok_or_else(|| {
            DistError::Integrity(format!(
                "{archive_name}.sha256 does not carry a SHA-256 digest"
            ))
        })?;

    let found = sha256_hex(archive);
    if found != published {
        return Err(DistError::Integrity(format!(
            "{archive_name} hashes to {found}, but the release published {published}"
        )));
    }
    Ok(())
}

/// Hold an archive to the build provenance the release attested.
///
/// Opt-in, and hard-required once asked for: a missing `gh` or a failed
/// verification aborts rather than degrading to the checksum alone, which
/// shares its origin with the archive and so proves only that the download
/// arrived intact.
pub fn verify_attestation(staged: &Path) -> Result<(), DistError> {
    let output = Command::new("gh")
        .args(["attestation", "verify"])
        .arg(staged)
        .args(["--repo", REPO])
        .stdin(Stdio::null())
        .output()
        .map_err(|e| {
            DistError::Unsupported(format!(
                "attestation verification needs the `gh` CLI (https://cli.github.com): {e}"
            ))
        })?;
    if !output.status.success() {
        return Err(DistError::Integrity(format!(
            "no attestation from {REPO}'s release workflow matches this archive: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        )));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const EMPTY_SHA256: &str = "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855";

    #[test]
    fn hashing_matches_the_published_digest_format() {
        assert_eq!(sha256_hex(b""), EMPTY_SHA256);
        assert_eq!(sha256_hex(b"abc").len(), 64);
    }

    #[test]
    fn a_matching_sidecar_passes_in_the_shape_the_shell_tools_write() {
        // `sha256sum file > file.sha256` and `shasum -a 256` both write the
        // digest, whitespace, then the name.
        let sidecar = format!("{EMPTY_SHA256}  atlassian-cli-v0.10.0-x.tar.gz\n");
        assert!(verify_sidecar(b"", &sidecar, "atlassian-cli-v0.10.0-x.tar.gz").is_ok());
    }

    #[test]
    fn an_archive_that_is_not_the_published_one_is_refused() {
        let sidecar = format!("{EMPTY_SHA256}  archive.tar.gz\n");
        let err = verify_sidecar(b"tampered", &sidecar, "archive.tar.gz").unwrap_err();
        assert!(matches!(err, DistError::Integrity(_)), "{err}");
    }

    #[test]
    fn a_sidecar_without_a_digest_is_refused_rather_than_skipped() {
        for missing in ["", "\n", "not-a-digest  archive.tar.gz", "abc123  archive"] {
            let err = verify_sidecar(b"", missing, "archive.tar.gz").unwrap_err();
            assert!(
                matches!(err, DistError::Integrity(_)),
                "sidecar {missing:?} passed"
            );
        }
    }

    #[test]
    fn an_uppercase_digest_is_the_same_digest() {
        let sidecar = format!("{}  archive.tar.gz", EMPTY_SHA256.to_ascii_uppercase());
        assert!(verify_sidecar(b"", &sidecar, "archive.tar.gz").is_ok());
    }
}
