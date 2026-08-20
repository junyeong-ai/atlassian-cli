//! The platforms a release publishes an archive for.

use semver::Version;

/// One published release target.
///
/// `triple` is the `{target}` in `atlassian-cli-v{version}-{target}`, so this
/// table and the `Release` workflow's build matrix name the same set or an
/// update is a 404 — the first thing an update run would hit. A test holds the
/// two together rather than trusting them to be edited in step.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ReleaseTarget {
    pub triple: &'static str,
    /// The binary's name inside the archive, which is also what it is called
    /// once installed.
    pub binary: &'static str,
}

const UNIX_BINARY: &str = "atlassian-cli";
const WINDOWS_BINARY: &str = "atlassian-cli.exe";

impl ReleaseTarget {
    pub const ALL: [ReleaseTarget; 5] = [
        ReleaseTarget {
            triple: "x86_64-unknown-linux-gnu",
            binary: UNIX_BINARY,
        },
        ReleaseTarget {
            triple: "aarch64-unknown-linux-gnu",
            binary: UNIX_BINARY,
        },
        ReleaseTarget {
            triple: "x86_64-apple-darwin",
            binary: UNIX_BINARY,
        },
        ReleaseTarget {
            triple: "aarch64-apple-darwin",
            binary: UNIX_BINARY,
        },
        ReleaseTarget {
            triple: "x86_64-pc-windows-msvc",
            binary: WINDOWS_BINARY,
        },
    ];

    /// The target whose archive runs on this machine, or `None` where the
    /// release publishes none.
    pub fn current() -> Option<&'static ReleaseTarget> {
        let triple = if cfg!(all(target_arch = "x86_64", target_os = "linux")) {
            "x86_64-unknown-linux-gnu"
        } else if cfg!(all(target_arch = "aarch64", target_os = "linux")) {
            "aarch64-unknown-linux-gnu"
        } else if cfg!(all(target_arch = "x86_64", target_os = "macos")) {
            "x86_64-apple-darwin"
        } else if cfg!(all(target_arch = "aarch64", target_os = "macos")) {
            "aarch64-apple-darwin"
        } else if cfg!(all(target_arch = "x86_64", target_os = "windows")) {
            "x86_64-pc-windows-msvc"
        } else {
            return None;
        };
        Self::ALL.iter().find(|target| target.triple == triple)
    }

    /// Why this machine has no archive, phrased as what to do instead. Callers
    /// reach for this only where [`current`](Self::current) answered `None`, so
    /// it has no triple to name.
    pub fn unsupported_reason() -> String {
        format!(
            "no published release for {}/{} — build from a checkout with `cargo build --release`",
            std::env::consts::OS,
            std::env::consts::ARCH,
        )
    }

    /// The archive this target's release publishes for `version`.
    ///
    /// Every target ships a `.tar.gz`, Windows included, so the update path is
    /// one code path on every platform. The `.zip` published alongside it is
    /// for people downloading by hand.
    pub fn archive_name(&self, version: &Version) -> String {
        format!("atlassian-cli-v{version}-{}.tar.gz", self.triple)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_triple_is_named_once() {
        let mut triples: Vec<&str> = ReleaseTarget::ALL.iter().map(|t| t.triple).collect();
        triples.sort_unstable();
        let mut unique = triples.clone();
        unique.dedup();
        assert_eq!(triples, unique, "a triple appears twice in the table");
    }

    #[test]
    fn this_build_resolves_to_an_archive_the_release_publishes() {
        // Every platform the suite runs on is one the release builds for; a
        // host outside the table is what `unsupported_reason` exists for.
        let target = ReleaseTarget::current().expect("test hosts are release targets");
        assert!(ReleaseTarget::ALL.contains(target));
    }

    #[test]
    fn archive_name_matches_what_the_release_workflow_packages() {
        let target = ReleaseTarget {
            triple: "aarch64-apple-darwin",
            binary: UNIX_BINARY,
        };
        assert_eq!(
            target.archive_name(&Version::parse("0.10.0").unwrap()),
            "atlassian-cli-v0.10.0-aarch64-apple-darwin.tar.gz"
        );
    }

    /// The table and the release matrix are two statements of one set. When
    /// they disagree the download is a 404 — an error that appears only in
    /// production, on the one command that is supposed to repair things.
    #[test]
    fn every_target_is_built_by_the_release_workflow() {
        let workflow = std::fs::read_to_string(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/.github/workflows/release.yml"
        ))
        .expect("the release workflow is part of the checkout");
        for target in ReleaseTarget::ALL {
            assert!(
                workflow.contains(target.triple),
                "release.yml builds no `{}`, so its archive would 404",
                target.triple
            );
        }
    }
}
