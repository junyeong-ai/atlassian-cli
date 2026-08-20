//! Deciding whether to replace the running binary, and doing it.

use std::cmp::Ordering;
use std::path::{Path, PathBuf};

use semver::Version;

use super::DistError;
use super::release::ReleaseClient;
use super::target::ReleaseTarget;

/// What replacing the binary would amount to.
///
/// Decided from versions alone — no I/O, no network — so the rule that governs
/// whether an update happens is the part a test can exhaust.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Decision {
    AlreadyCurrent(Version),
    Replace {
        from: Version,
        to: Version,
    },
    /// The release that answered is older than the running binary.
    ///
    /// Refused rather than performed, because this is what a yanked or
    /// rolled-back release looks like — and what a stale answer from the
    /// trailing web view looks like, which is the same shape and arrives
    /// without warning. Going back is a deliberate act, so it takes a version
    /// named on the command line.
    RefusedDowngrade {
        running: Version,
        offered: Version,
    },
}

/// `requested` is a version named on the command line, which is deliberate in
/// either direction; `latest` is what the release channel answered, which is
/// only ever followed forward.
pub fn decide(
    running: &Version,
    requested: Option<&Version>,
    latest: Option<&Version>,
    force: bool,
) -> Option<Decision> {
    let target = requested.or(latest)?;

    Some(match precedence(target, running) {
        Ordering::Less if requested.is_none() => Decision::RefusedDowngrade {
            running: running.clone(),
            offered: target.clone(),
        },
        Ordering::Equal if !force => Decision::AlreadyCurrent(running.clone()),
        _ => Decision::Replace {
            from: running.clone(),
            to: target.clone(),
        },
    })
}

/// Semver precedence, which is not `Version`'s own ordering.
///
/// `Ord` has to agree with `Eq`, and `Eq` separates two builds of one version —
/// so `Version` orders by build metadata, which the specification says carries
/// no precedence. Comparing the pair with that field cleared is the crate's
/// ordering for everything that counts, minus the one field that must not:
/// without it `1.2.3+build.5` reads as neither older than nor the same as
/// `1.2.3`, which resolves to an update onto the version already installed.
fn precedence(a: &Version, b: &Version) -> Ordering {
    fn bare(version: &Version) -> Version {
        Version {
            build: semver::BuildMetadata::EMPTY,
            ..version.clone()
        }
    }
    bare(a).cmp(&bare(b))
}

/// A private directory for the archive and what is unpacked from it.
///
/// Exclusive to this process, because the system temp directory is
/// world-writable and a release archive's name is fully predictable: a
/// pre-created symlink at that path would decide what got truncated, and the
/// file handed to `gh attestation verify` could be swapped between the write
/// and the check.
pub struct Staging(tempfile::TempDir);

impl Staging {
    pub fn new() -> Result<Self, DistError> {
        let dir = tempfile::Builder::new()
            .prefix("atlassian-cli-update-")
            .tempdir()
            .map_err(|e| DistError::io("creating a private staging directory", e))?;
        restrict_to_owner(dir.path())?;
        Ok(Staging(dir))
    }

    pub fn write(&self, name: &str, bytes: &[u8]) -> Result<PathBuf, DistError> {
        let path = self.0.path().join(name);
        std::fs::write(&path, bytes)
            .map_err(|e| DistError::io(format!("staging {}", path.display()), e))?;
        Ok(path)
    }
}

/// The binary a release published for `target`, proven to be the one it
/// published.
///
/// The checksum is not advisory: bytes that cannot be shown to be the released
/// ones are never handed back for installation. Attestation is opt-in and, once
/// asked for, equally hard — the checksum shares its origin with the archive,
/// so on its own it proves only that the download arrived intact.
pub async fn fetch_verified_binary(
    client: &ReleaseClient,
    target: &ReleaseTarget,
    version: &Version,
    staging: &Staging,
    verify_attestations: bool,
) -> Result<Vec<u8>, DistError> {
    let archive_name = target.archive_name(version);
    let url = client.asset_url(version, &archive_name);

    let archive = client.fetch(&url).await?;
    let sidecar = client.fetch_text(&format!("{url}.sha256")).await?;
    super::verify_sidecar(&archive, &sidecar, &archive_name)?;

    if verify_attestations {
        super::verify_attestation(&staging.write(&archive_name, &archive)?)?;
    }

    super::read_from_tar_gz(&archive, target.binary)?
        .ok_or_else(|| DistError::Integrity(format!("{archive_name} holds no {}", target.binary)))
}

/// The version a binary reports when asked.
///
/// Executing it is the only check that answers the question an update actually
/// has — does this file run, on this kernel, with this architecture, past this
/// platform's code signing — and it answers all of them at once. Comparing
/// bytes or reading a header would each answer one.
pub fn version_of(binary: &Path) -> Result<Version, DistError> {
    let output = std::process::Command::new(binary)
        .arg("--version")
        .stdin(std::process::Stdio::null())
        .output()
        .map_err(|e| DistError::io(format!("running {}", binary.display()), e))?;
    if !output.status.success() {
        return Err(DistError::Integrity(format!(
            "{} exited {} when asked for its version",
            binary.display(),
            output.status
        )));
    }
    let printed = String::from_utf8_lossy(&output.stdout);
    let token = printed
        .split_whitespace()
        .next_back()
        .ok_or_else(|| DistError::Integrity(format!("{} printed no version", binary.display())))?;
    Version::parse(token).map_err(|e| {
        DistError::Integrity(format!(
            "{} printed `{token}`, which is not a version: {e}",
            binary.display()
        ))
    })
}

/// Replace the running executable with the binary staged at `staged`.
///
/// The staged file is made executable and asked for its version BEFORE
/// anything is replaced, so a download that will not run on this machine ends
/// with nothing touched. There is no rollback path here because there is
/// nothing to roll back from: an installation whose binary does not run has no
/// way left to repair itself, and the way to guarantee that never happens is to
/// not create it.
pub fn install(staged: &Path, expected: &Version) -> Result<(), DistError> {
    make_executable(staged)?;

    let found = version_of(staged)?;
    if &found != expected {
        return Err(DistError::Integrity(format!(
            "the downloaded binary reports {found}, not {expected} — nothing was replaced"
        )));
    }

    self_replace::self_replace(staged).map_err(|e| match e.kind() {
        std::io::ErrorKind::PermissionDenied => DistError::io(
            "replacing the running binary — the directory holding it belongs to another user; \
             re-run with the privileges that installed it",
            e,
        ),
        _ => DistError::io("replacing the running binary", e),
    })
}

#[cfg(unix)]
fn make_executable(path: &Path) -> Result<(), DistError> {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755))
        .map_err(|e| DistError::io(format!("making {} executable", path.display()), e))
}

#[cfg(not(unix))]
fn make_executable(_path: &Path) -> Result<(), DistError> {
    Ok(())
}

/// `tempfile` creates the directory exclusively under a random name, which is
/// what closes the pre-created-symlink hole; the mode it lands on is the
/// process umask's, so the narrower one is set here rather than assumed.
#[cfg(unix)]
fn restrict_to_owner(path: &Path) -> Result<(), DistError> {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700))
        .map_err(|e| DistError::io(format!("restricting {}", path.display()), e))
}

#[cfg(not(unix))]
fn restrict_to_owner(_path: &Path) -> Result<(), DistError> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn v(s: &str) -> Version {
        Version::parse(s).unwrap()
    }

    #[test]
    fn the_same_version_is_already_current() {
        assert_eq!(
            decide(&v("0.10.0"), None, Some(&v("0.10.0")), false),
            Some(Decision::AlreadyCurrent(v("0.10.0")))
        );
    }

    #[test]
    fn force_replaces_the_same_version() {
        assert_eq!(
            decide(&v("0.10.0"), None, Some(&v("0.10.0")), true),
            Some(Decision::Replace {
                from: v("0.10.0"),
                to: v("0.10.0"),
            })
        );
    }

    #[test]
    fn a_newer_release_is_replaced_into() {
        assert_eq!(
            decide(&v("0.10.0"), None, Some(&v("0.11.0")), false),
            Some(Decision::Replace {
                from: v("0.10.0"),
                to: v("0.11.0"),
            })
        );
    }

    /// A rolled-back release and a stale answer from the trailing web view are
    /// the same shape, and neither is a reason to install older code unasked.
    #[test]
    fn a_channel_that_answers_older_is_refused() {
        assert_eq!(
            decide(&v("0.11.0"), None, Some(&v("0.10.0")), false),
            Some(Decision::RefusedDowngrade {
                running: v("0.11.0"),
                offered: v("0.10.0"),
            })
        );
    }

    #[test]
    fn a_named_version_goes_back_when_that_is_what_was_named() {
        assert_eq!(
            decide(&v("0.11.0"), Some(&v("0.10.0")), Some(&v("0.11.0")), false),
            Some(Decision::Replace {
                from: v("0.11.0"),
                to: v("0.10.0"),
            })
        );
    }

    /// Precedence is semver's, not the string's: `0.3.10` is above `0.3.9`, and
    /// a final release is above its own pre-release — so moving off
    /// `1.2.3-rc.1` onto `1.2.3` is the upgrade it is, not a refused downgrade.
    #[test]
    fn precedence_is_semver_precedence() {
        assert!(matches!(
            decide(&v("0.3.9"), None, Some(&v("0.3.10")), false),
            Some(Decision::Replace { .. })
        ));
        assert!(matches!(
            decide(&v("1.2.3-rc.1"), None, Some(&v("1.2.3")), false),
            Some(Decision::Replace { .. })
        ));
        assert!(matches!(
            decide(&v("1.2.3"), None, Some(&v("1.2.3-rc.2")), false),
            Some(Decision::RefusedDowngrade { .. })
        ));
        // Build metadata carries no precedence, so it is the same version.
        assert_eq!(
            decide(&v("1.2.3"), None, Some(&v("1.2.3+build.5")), false),
            Some(Decision::AlreadyCurrent(v("1.2.3")))
        );
    }

    #[test]
    fn nothing_to_decide_without_a_version_from_either_side() {
        assert_eq!(decide(&v("0.10.0"), None, None, false), None);
    }

    #[test]
    fn this_binary_reports_a_version_the_update_path_can_read() {
        // Step 4 of an update compares what the staged binary prints against
        // the release it was downloaded from, so the format `--version` emits
        // is part of the update contract rather than cosmetic.
        let printed = format!("atlassian-cli {}", env!("CARGO_PKG_VERSION"));
        let token = printed.split_whitespace().next_back().unwrap();
        assert_eq!(
            Version::parse(token).unwrap(),
            Version::parse(env!("CARGO_PKG_VERSION")).unwrap()
        );
    }

    #[cfg(unix)]
    #[test]
    fn a_binary_that_does_not_run_replaces_nothing() {
        let staging = Staging::new().unwrap();
        let staged = staging
            .write("atlassian-cli", b"not an executable at all")
            .unwrap();
        let err = install(&staged, &v("0.11.0")).unwrap_err();
        assert!(matches!(err, DistError::Integrity(_)), "{err}");
    }

    #[cfg(unix)]
    #[test]
    fn a_binary_reporting_another_version_replaces_nothing() {
        let staging = Staging::new().unwrap();
        let staged = staging
            .write("atlassian-cli", b"#!/bin/sh\necho 'atlassian-cli 0.9.0'\n")
            .unwrap();
        let err = install(&staged, &v("0.11.0")).unwrap_err();
        assert!(
            err.to_string().contains("reports 0.9.0, not 0.11.0"),
            "{err}"
        );
    }

    #[cfg(unix)]
    #[test]
    fn a_staged_binary_is_asked_for_its_version_by_running_it() {
        let staging = Staging::new().unwrap();
        let staged = staging
            .write("atlassian-cli", b"#!/bin/sh\necho 'atlassian-cli 0.11.0'\n")
            .unwrap();
        make_executable(&staged).unwrap();
        assert_eq!(version_of(&staged).unwrap(), v("0.11.0"));
    }

    fn tar_gz(name: &str, bytes: &[u8]) -> Vec<u8> {
        let mut builder = tar::Builder::new(flate2::write::GzEncoder::new(
            Vec::new(),
            flate2::Compression::fast(),
        ));
        let mut header = tar::Header::new_gnu();
        header.set_size(bytes.len() as u64);
        header.set_mode(0o755);
        header.set_cksum();
        builder.append_data(&mut header, name, bytes).unwrap();
        builder.into_inner().unwrap().finish().unwrap()
    }

    const TARGET: ReleaseTarget = ReleaseTarget {
        triple: "aarch64-apple-darwin",
        binary: "atlassian-cli",
    };

    /// Serve a release whose archive holds `member`, with a sidecar carrying
    /// `digest_of` — passing different bytes there is how a tampered download
    /// is simulated.
    async fn serve_release(member: &str, digest_of: Option<&[u8]>) -> (MockServer, Vec<u8>) {
        let server = MockServer::start().await;
        let archive = tar_gz(member, b"the released binary");
        let digest = super::super::sha256_hex(digest_of.unwrap_or(&archive));
        let name = TARGET.archive_name(&Version::new(0, 10, 0));
        let base = format!("/{}/releases/download/v0.10.0", super::super::REPO);

        Mock::given(wiremock::matchers::method("GET"))
            .and(wiremock::matchers::path(format!("{base}/{name}")))
            .respond_with(ResponseTemplate::new(200).set_body_bytes(archive.clone()))
            .mount(&server)
            .await;
        Mock::given(wiremock::matchers::method("GET"))
            .and(wiremock::matchers::path(format!("{base}/{name}.sha256")))
            .respond_with(ResponseTemplate::new(200).set_body_string(format!("{digest}  {name}\n")))
            .mount(&server)
            .await;
        (server, archive)
    }

    #[tokio::test]
    async fn a_release_hands_back_the_binary_it_published() {
        let (server, _) = serve_release("atlassian-cli", None).await;
        let client = ReleaseClient::at(&server.uri()).unwrap();
        let staging = Staging::new().unwrap();

        let binary =
            fetch_verified_binary(&client, &TARGET, &Version::new(0, 10, 0), &staging, false)
                .await
                .unwrap();
        assert_eq!(binary, b"the released binary");
    }

    #[tokio::test]
    async fn an_archive_that_does_not_match_its_checksum_is_never_handed_back() {
        let (server, _) = serve_release("atlassian-cli", Some(b"some other archive")).await;
        let client = ReleaseClient::at(&server.uri()).unwrap();
        let staging = Staging::new().unwrap();

        let err = fetch_verified_binary(&client, &TARGET, &Version::new(0, 10, 0), &staging, false)
            .await
            .unwrap_err();
        assert!(matches!(err, DistError::Integrity(_)), "{err}");
    }

    #[tokio::test]
    async fn an_archive_without_the_binary_fails_rather_than_installing_something_else() {
        let (server, _) = serve_release("README", None).await;
        let client = ReleaseClient::at(&server.uri()).unwrap();
        let staging = Staging::new().unwrap();

        let err = fetch_verified_binary(&client, &TARGET, &Version::new(0, 10, 0), &staging, false)
            .await
            .unwrap_err();
        assert!(err.to_string().contains("holds no atlassian-cli"), "{err}");
    }

    #[cfg(unix)]
    #[test]
    fn staging_is_private_to_this_process() {
        use std::os::unix::fs::PermissionsExt;
        let staging = Staging::new().unwrap();
        let mode = std::fs::metadata(staging.0.path())
            .unwrap()
            .permissions()
            .mode();
        assert_eq!(
            mode & 0o777,
            0o700,
            "the staging directory is readable by others"
        );
    }
}
