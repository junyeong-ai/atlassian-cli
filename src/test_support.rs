//! Preconditions the tests of both crates in this package assert.
//!
//! The library and the binary are separate crates, so a `#[cfg(test)]` item in
//! one is not there for the other. This file is compiled into each — declared
//! as a module in `lib.rs`, by `#[path]` in `main.rs` — so a precondition both
//! state has one wording and cannot come to hold in only one of them.

/// Assert file modes refuse this process.
///
/// Several tests establish that an unreachable path is reported rather than
/// read as an absent one, and they make it unreachable by mode. Root bypasses
/// modes, so there the operation succeeds and the assertion reports as a defect
/// what is really its own unmet premise.
///
/// The property is probed rather than inferred from the user id: a mode is set
/// and the read it forbids is attempted, which answers for a filesystem that
/// does not enforce modes as well as for a process that outranks them. Stated
/// rather than skipped — a suite that quietly drops these reports a coverage it
/// does not have.
#[cfg(unix)]
pub fn require_enforced_modes() {
    use std::os::unix::fs::PermissionsExt;

    let dir = tempfile::tempdir().expect("a temporary directory");
    let closed = dir.path().join("closed");
    std::fs::write(&closed, b"probe").expect("a file to close");
    std::fs::set_permissions(&closed, std::fs::Permissions::from_mode(0o000))
        .expect("the mode to be set");

    assert!(
        std::fs::read(&closed).is_err(),
        "file modes do not refuse this process — it is running as root, or on a filesystem \
         that does not enforce them. The tests that make a path unreachable by mode need a \
         process that modes apply to; run the suite as a normal user."
    );
}
