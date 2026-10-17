#![cfg(unix)]

use std::path::Path;
use std::process::{Command, Output};

fn run(directory: &Path, script: &str) -> Output {
    Command::new("bash")
        .args(["-c", &format!("source \"$1\"\n{script}"), "installer-test"])
        .arg(Path::new(env!("CARGO_MANIFEST_DIR")).join("scripts/install.sh"))
        .env("INSTALL_DIR", directory.join("bin"))
        .env("ATLASSIAN_CLI_VERSION", env!("CARGO_PKG_VERSION"))
        .env("TEST_ROOT", directory)
        .output()
        .unwrap()
}

fn executable(path: &Path, script: &str) {
    use std::os::unix::fs::PermissionsExt;
    std::fs::write(path, script).unwrap();
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).unwrap();
}

fn candidate(directory: &Path, version: &str) {
    executable(
        &directory.join("candidate"),
        &format!(
            "#!/bin/sh\ncase \"$1\" in --version) echo 'atlassian-cli {version}';; *) exit 2;; esac\n"
        ),
    );
}

#[test]
fn failed_or_mismatched_candidates_preserve_the_existing_installation() {
    for script in [
        "#!/bin/sh\nexit 126\n",
        "#!/bin/sh\necho atlassian-cli 99.0.0\n",
    ] {
        let directory = tempfile::tempdir().unwrap();
        let binary = directory.path().join("bin/atlassian-cli");
        std::fs::create_dir(binary.parent().unwrap()).unwrap();
        executable(&binary, "#!/bin/sh\necho old-installation\n");
        let previous = std::fs::read(&binary).unwrap();
        executable(&directory.path().join("candidate"), script);
        let output = run(
            directory.path(),
            "install_binary \"$TEST_ROOT/candidate\" \"$VERSION\"",
        );
        assert!(!output.status.success());
        assert_eq!(std::fs::read(&binary).unwrap(), previous);
        assert_eq!(
            std::fs::read_dir(binary.parent().unwrap()).unwrap().count(),
            1
        );
    }
}

#[test]
fn a_verified_candidate_replaces_the_installation() {
    let directory = tempfile::Builder::new()
        .prefix("installer space ")
        .tempdir()
        .unwrap();
    candidate(directory.path(), env!("CARGO_PKG_VERSION"));
    let output = run(
        directory.path(),
        "install_binary \"$TEST_ROOT/candidate\" \"$VERSION\"",
    );
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        std::fs::read(directory.path().join("bin/atlassian-cli")).unwrap(),
        std::fs::read(directory.path().join("candidate")).unwrap()
    );
    assert_eq!(
        std::fs::read_dir(directory.path().join("bin"))
            .unwrap()
            .count(),
        1
    );
}

#[test]
fn a_failed_source_build_never_installs_a_stale_artifact() {
    let directory = tempfile::tempdir().unwrap();
    let stale = directory.path().join("target/release/atlassian-cli");
    std::fs::create_dir_all(stale.parent().unwrap()).unwrap();
    executable(&stale, "#!/bin/sh\necho stale\n");
    let output = run(
        directory.path(),
        r#"
PROJECT_ROOT="$TEST_ROOT"
IS_CHECKOUT=true
cargo() { printf 'path+file:///review#%s\n' "$VERSION"; }
cargo_build_release() { return 101; }
build_from_source
install_binary "$BINARY_PATH" "$BINARY_VERSION"
"#,
    );
    assert_eq!(output.status.code(), Some(101));
    assert!(!directory.path().join("bin").exists());
    assert_eq!(
        std::fs::read_to_string(stale).unwrap(),
        "#!/bin/sh\necho stale\n"
    );
}

#[test]
fn source_installation_uses_the_checkout_version() {
    let directory = tempfile::tempdir().unwrap();
    std::fs::write(
        directory.path().join("Cargo.toml"),
        format!(
            "[package]\nname = \"atlassian-cli\"\nversion = \"{}\"\nedition = \"2024\"\n",
            env!("CARGO_PKG_VERSION")
        ),
    )
    .unwrap();
    std::fs::write(
        directory.path().join("Cargo.lock"),
        format!(
            "version = 4\n[[package]]\nname = \"atlassian-cli\"\nversion = \"{}\"\n",
            env!("CARGO_PKG_VERSION")
        ),
    )
    .unwrap();
    std::fs::create_dir(directory.path().join("src")).unwrap();
    std::fs::write(
        directory.path().join("src/main.rs"),
        "fn main() { println!(\"atlassian-cli {}\", env!(\"CARGO_PKG_VERSION\")); }",
    )
    .unwrap();
    let output = run(
        directory.path(),
        r#"
PROJECT_ROOT="$TEST_ROOT"
IS_CHECKOUT=true
VERSION=99.0.0
export CARGO_BUILD_TARGET=not-an-installation-target
build_from_source
install_binary "$BINARY_PATH" "$BINARY_VERSION"
"#,
    );
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(directory.path().join("bin/atlassian-cli").is_file());
}

#[test]
fn downloads_are_cleaned_after_success_and_failure() {
    for valid_checksum in [true, false] {
        let directory = tempfile::tempdir().unwrap();
        let assets = directory.path().join("assets");
        std::fs::create_dir(&assets).unwrap();
        candidate(directory.path(), env!("CARGO_PKG_VERSION"));
        std::fs::copy(
            directory.path().join("candidate"),
            directory.path().join("atlassian-cli"),
        )
        .unwrap();
        let archive = format!(
            "atlassian-cli-v{}-test-target.tar.gz",
            env!("CARGO_PKG_VERSION")
        );
        assert!(
            Command::new("tar")
                .args(["-czf"])
                .arg(assets.join(&archive))
                .arg("-C")
                .arg(directory.path())
                .arg("atlassian-cli")
                .status()
                .unwrap()
                .success()
        );
        let checksum =
            atlassian_cli::dist::verify::sha256_hex(&std::fs::read(assets.join(&archive)).unwrap());
        std::fs::write(
            assets.join(format!("{archive}.sha256")),
            format!(
                "{}  {archive}\n",
                if valid_checksum { &checksum } else { "invalid" }
            ),
        )
        .unwrap();
        let output = run(
            directory.path(),
            r#"
IS_CHECKOUT=false
detect_platform() { echo test-target; }
verify_attestation() { return 0; }
curl() { cp "$TEST_ROOT/assets/${2##*/}" .; }
mktemp() {
    case "$1" in
        *atlassian-cli-install*) mkdir "$TEST_ROOT/download"; echo "$TEST_ROOT/download" ;;
        *) command mktemp "$@" ;;
    esac
}
main
"#,
        );
        assert_eq!(
            output.status.success(),
            valid_checksum,
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(!directory.path().join("download").exists());
        assert_eq!(
            directory.path().join("bin/atlassian-cli").exists(),
            valid_checksum
        );
    }
}
