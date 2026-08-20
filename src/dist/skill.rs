//! The Claude Code skill this binary carries and writes out.
//!
//! The skill describes the commands this binary exposes, so it is true only of
//! the version it shipped with. Compiling it in leaves one artifact, so the two
//! cannot be different versions and a deployed copy is checked by comparing
//! bytes rather than a declared version.

use std::collections::BTreeSet;
use std::ffi::{OsStr, OsString};
use std::path::Path;

use super::DistError;

pub const SKILL_NAME: &str = "jira-confluence";

/// Every file the skill consists of, addressed relative to its own directory.
/// Adding one is a line here; editing one rebuilds the binary that carries it.
const FILES: &[(&str, &str)] = &[(
    "SKILL.md",
    include_str!("../../.claude/skills/jira-confluence/SKILL.md"),
)];

/// How a deployed copy stands against what this binary carries.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SkillState {
    /// Nothing is deployed. An install that took no skill must not acquire one
    /// from an update, so this is a fact to report rather than one to repair.
    Absent,
    /// Byte-identical to what this binary carries.
    Current,
    /// Deployed, but not what this binary carries — an older release's copy, or
    /// one edited in place.
    Stale,
}

impl SkillState {
    pub fn as_str(self) -> &'static str {
        match self {
            SkillState::Absent => "absent",
            SkillState::Current => "current",
            SkillState::Stale => "stale",
        }
    }
}

/// What one deploy wrote or removed.
#[derive(Debug, Default)]
pub struct Deployed {
    pub written: Vec<String>,
    /// Files found in the skill directory that this binary does not carry —
    /// left behind by an older release, and removed so the directory states
    /// this version and nothing else.
    pub pruned: Vec<String>,
}

pub fn state(dir: &Path) -> SkillState {
    if !dir.is_dir() {
        return SkillState::Absent;
    }
    // Only where this tool reconciles the directory: elsewhere a file it does
    // not carry is not a difference to report, because it is not one `deploy`
    // would remove.
    if reconcilable_files(dir).is_some_and(|found| found != carried_names()) {
        return SkillState::Stale;
    }
    for (relative, contents) in FILES {
        match std::fs::read(dir.join(relative)) {
            Ok(found) if found == contents.as_bytes() => {}
            _ => return SkillState::Stale,
        }
    }
    SkillState::Current
}

/// Write every carried file into `dir`, and remove the files there that this
/// binary does not carry.
pub fn deploy(dir: &Path) -> Result<Deployed, DistError> {
    let mut outcome = Deployed::default();

    if let Some(found) = reconcilable_files(dir) {
        for stale in found.difference(&carried_names()) {
            let path = dir.join(stale);
            std::fs::remove_file(&path)
                .map_err(|e| DistError::io(format!("removing {}", path.display()), e))?;
            outcome.pruned.push(stale.to_string_lossy().into_owned());
        }
    }

    for (relative, contents) in FILES {
        let path = dir.join(relative);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| DistError::io(format!("creating {}", parent.display()), e))?;
        }
        // `write` opens through a symlink, so a carried name pointed at
        // something else would have that file's contents replaced with the
        // skill. Only a link is unlinked, and `remove_file` never traverses —
        // an ordinary file stays a truncating write, which is what lets a
        // deploy still land in a directory the user made read-only.
        if std::fs::symlink_metadata(&path).is_ok_and(|meta| meta.file_type().is_symlink()) {
            std::fs::remove_file(&path).map_err(|e| {
                DistError::io(format!("replacing the link at {}", path.display()), e)
            })?;
        }
        std::fs::write(&path, contents)
            .map_err(|e| DistError::io(format!("writing {}", path.display()), e))?;
        outcome.written.push((*relative).to_string());
    }
    Ok(outcome)
}

/// Remove the deployed skill, reporting whether there was one.
///
/// `remove_dir_all` does not follow a symlink, so a skill directory the user
/// pointed elsewhere loses the link and nothing behind it. An emptied
/// `~/.claude/skills` goes too, but never `~/.claude` itself — that one belongs
/// to the agent, not to this tool.
pub fn remove(dir: &Path) -> Result<bool, DistError> {
    // `symlink_metadata`, not `exists`: the latter reports a dangling symlink
    // as nothing there, leaving the link behind and calling it removed.
    let Ok(meta) = std::fs::symlink_metadata(dir) else {
        return Ok(false);
    };
    let outcome = if meta.file_type().is_symlink() {
        std::fs::remove_file(dir)
    } else {
        std::fs::remove_dir_all(dir)
    };
    outcome.map_err(|e| DistError::io(format!("removing {}", dir.display()), e))?;
    let _ = dir.parent().map(std::fs::remove_dir);
    Ok(true)
}

fn carried_names() -> BTreeSet<OsString> {
    FILES
        .iter()
        .map(|(relative, _)| OsStr::new(relative).to_os_string())
        .collect()
}

/// The regular files in `dir` this tool reconciles — the set `deploy` deletes
/// from, and the one `state` measures against. `None` where the directory is
/// not this tool's to reconcile.
///
/// Deliberately shallow and symlink-blind, because every alternative is a way
/// the deletion reaches somewhere it was never pointed: a symlinked entry would
/// resolve the removal through it, and a skill directory the user redirected
/// into a dotfiles repository holds their files, not a deployment. There the
/// answer is `None` rather than an empty set — nothing to prune, and nothing
/// there counts as a difference either.
fn reconcilable_files(dir: &Path) -> Option<BTreeSet<OsString>> {
    if !std::fs::symlink_metadata(dir).is_ok_and(|meta| meta.file_type().is_dir()) {
        return None;
    }
    // Names stay `OsString`: a lossy `String` does not name the file it came
    // from, so pruning one would fail on a name this platform allows and take
    // every deploy down with it.
    Some(
        std::fs::read_dir(dir)
            .ok()?
            .flatten()
            .filter(|entry| {
                std::fs::symlink_metadata(entry.path()).is_ok_and(|meta| meta.file_type().is_file())
            })
            .map(|entry| entry.file_name())
            .collect(),
    )
}

/// Everything this binary carries, for a caller that reports rather than writes.
pub fn carried_files() -> Vec<&'static str> {
    FILES.iter().map(|(relative, _)| *relative).collect()
}

/// The version the carried skill declares, read from its YAML frontmatter.
pub fn carried_version() -> Option<&'static str> {
    let (_, skill_md) = FILES.iter().find(|(relative, _)| *relative == "SKILL.md")?;
    let mut lines = skill_md.lines();
    if lines.next()?.trim() != "---" {
        return None;
    }
    for line in lines {
        let line = line.trim();
        if line == "---" {
            return None;
        }
        if let Some(version) = line.strip_prefix("version:") {
            return Some(version.trim()).filter(|v| !v.is_empty());
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The one mechanism that keeps the skill and the binary from drifting at
    /// release time. A skill declaring another version makes every report about
    /// the installation read against the wrong thing.
    #[test]
    fn the_carried_skill_declares_this_binarys_version() {
        assert_eq!(
            carried_version(),
            Some(env!("CARGO_PKG_VERSION")),
            "SKILL.md's `version:` drifted from Cargo.toml — bump both together"
        );
    }

    #[test]
    fn the_skill_carries_the_file_an_agent_loads() {
        assert!(carried_files().contains(&"SKILL.md"));
    }

    /// `deployed_files` reads one directory level and compares file names, and
    /// it is the set `deploy` deletes from. A carried file addressed into a
    /// subdirectory would never match, so every deploy would delete it and
    /// write it back.
    #[test]
    fn every_carried_file_sits_directly_in_the_skill_directory() {
        for name in carried_files() {
            assert!(!name.contains('/'), "`{name}` is not a direct child");
        }
    }

    #[cfg(unix)]
    #[test]
    fn a_symlink_in_the_skill_directory_is_left_alone_rather_than_followed() {
        let root = tempfile::tempdir().unwrap();
        let victim = root.path().join("victim");
        std::fs::create_dir_all(&victim).unwrap();
        std::fs::write(victim.join("precious.txt"), "not ours").unwrap();

        let dir = root.path().join(".claude/skills").join(SKILL_NAME);
        std::fs::create_dir_all(&dir).unwrap();
        std::os::unix::fs::symlink(&victim, dir.join("link")).unwrap();

        let outcome = deploy(&dir).unwrap();
        assert!(outcome.pruned.is_empty(), "{:?}", outcome.pruned);
        assert!(
            victim.join("precious.txt").is_file(),
            "deploy reached through a symlink and deleted a file it does not own"
        );
        assert!(dir.join("link").is_symlink(), "the link itself was removed");
    }

    #[cfg(unix)]
    #[test]
    fn a_carried_name_pointed_elsewhere_has_its_target_left_alone() {
        let root = tempfile::tempdir().unwrap();
        let victim = root.path().join("zshrc");
        std::fs::write(&victim, "export FOO=1\n").unwrap();

        let dir = root.path().join(".claude/skills").join(SKILL_NAME);
        std::fs::create_dir_all(&dir).unwrap();
        // Pruning leaves a symlink alone, so the write is the path that would
        // otherwise reach through it — `fs::write` opens through a link.
        std::os::unix::fs::symlink(&victim, dir.join("SKILL.md")).unwrap();

        deploy(&dir).unwrap();
        assert_eq!(std::fs::read_to_string(&victim).unwrap(), "export FOO=1\n");
        assert_eq!(state(&dir), SkillState::Current);
    }

    /// Unlinking is conditional so this keeps working: replacing the contents
    /// of an existing file needs write permission on the file, not on the
    /// directory holding it.
    #[cfg(unix)]
    #[test]
    fn a_deploy_still_lands_in_a_directory_the_user_made_read_only() {
        use std::os::unix::fs::PermissionsExt;
        let root = tempfile::tempdir().unwrap();
        let dir = root.path().join(".claude/skills").join(SKILL_NAME);
        deploy(&dir).unwrap();
        std::fs::write(dir.join("SKILL.md"), "edited").unwrap();
        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o500)).unwrap();

        let outcome = deploy(&dir);
        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700)).unwrap();

        outcome.expect("a writable file in a read-only directory is still replaceable");
        assert_eq!(state(&dir), SkillState::Current);
    }

    #[cfg(unix)]
    #[test]
    fn a_dangling_skill_directory_link_is_removed_rather_than_reported_absent() {
        let root = tempfile::tempdir().unwrap();
        let skills = root.path().join(".claude/skills");
        std::fs::create_dir_all(&skills).unwrap();
        let dir = skills.join(SKILL_NAME);
        std::os::unix::fs::symlink(root.path().join("gone"), &dir).unwrap();

        assert!(
            remove(&dir).unwrap(),
            "the link was reported as nothing there"
        );
        assert!(!dir.is_symlink());
    }

    /// A skill directory symlinked into a dotfiles repository is the shape
    /// where a recursive prune is most costly: everything in the repository is
    /// a file this binary does not carry.
    #[cfg(unix)]
    #[test]
    fn a_symlinked_skill_directory_does_not_have_its_target_emptied() {
        let root = tempfile::tempdir().unwrap();
        let repo = root.path().join("dotfiles");
        std::fs::create_dir_all(repo.join("nested")).unwrap();
        std::fs::write(repo.join("README.md"), "mine").unwrap();
        std::fs::write(repo.join("nested/deep.txt"), "also mine").unwrap();

        let skills = root.path().join(".claude/skills");
        std::fs::create_dir_all(&skills).unwrap();
        let dir = skills.join(SKILL_NAME);
        std::os::unix::fs::symlink(&repo, &dir).unwrap();

        deploy(&dir).unwrap();
        assert!(repo.join("README.md").is_file());
        assert!(repo.join("nested/deep.txt").is_file());
        assert!(repo.join("SKILL.md").is_file(), "the skill was not written");
        // Files this tool would never prune are not a difference to report, so
        // a deploy that succeeded reads as current rather than stale forever.
        assert_eq!(state(&dir), SkillState::Current);
        std::fs::write(dir.join("SKILL.md"), "edited").unwrap();
        assert_eq!(state(&dir), SkillState::Stale);

        // And removing the skill takes the link, not what it points at.
        assert!(remove(&dir).unwrap());
        assert!(repo.join("README.md").is_file(), "the target was removed");
    }

    #[test]
    fn an_undeployed_skill_is_absent_rather_than_stale() {
        let home = tempfile::tempdir().unwrap();
        assert_eq!(
            state(&home.path().join(".claude/skills").join(SKILL_NAME)),
            SkillState::Absent
        );
    }

    #[test]
    fn a_deployed_copy_is_current_until_it_stops_matching_byte_for_byte() {
        let home = tempfile::tempdir().unwrap();
        let dir = home.path().join(".claude/skills").join(SKILL_NAME);

        let outcome = deploy(&dir).unwrap();
        assert_eq!(outcome.written, vec!["SKILL.md"]);
        assert!(outcome.pruned.is_empty());
        assert_eq!(state(&dir), SkillState::Current);

        // Editing the deployed copy is what a version string cannot see.
        std::fs::write(dir.join("SKILL.md"), "edited in place").unwrap();
        assert_eq!(state(&dir), SkillState::Stale);

        deploy(&dir).unwrap();
        assert_eq!(state(&dir), SkillState::Current);
    }

    #[test]
    fn a_file_an_older_release_left_behind_is_pruned() {
        let home = tempfile::tempdir().unwrap();
        let dir = home.path().join(".claude/skills").join(SKILL_NAME);
        deploy(&dir).unwrap();
        std::fs::write(dir.join("RETIRED.md"), "from an older release").unwrap();
        assert_eq!(state(&dir), SkillState::Stale);

        let outcome = deploy(&dir).unwrap();
        assert_eq!(outcome.pruned, vec!["RETIRED.md"]);
        assert_eq!(state(&dir), SkillState::Current);
    }

    #[test]
    fn removing_reports_whether_there_was_one_and_clears_what_it_emptied() {
        let home = tempfile::tempdir().unwrap();
        let dir = home.path().join(".claude/skills").join(SKILL_NAME);
        deploy(&dir).unwrap();

        assert!(remove(&dir).unwrap());
        assert_eq!(state(&dir), SkillState::Absent);
        assert!(
            !home.path().join(".claude/skills").exists(),
            "an emptied skills directory is left behind"
        );
        // `~/.claude` holds the agent's own state and is not this tool's to
        // remove, however empty this leaves it.
        assert!(home.path().join(".claude").is_dir());
        assert!(!remove(&dir).unwrap());
    }

    #[test]
    fn a_skills_directory_holding_another_skill_survives() {
        let home = tempfile::tempdir().unwrap();
        let skills = home.path().join(".claude/skills");
        let dir = skills.join(SKILL_NAME);
        deploy(&dir).unwrap();
        std::fs::create_dir_all(skills.join("someone-elses")).unwrap();

        remove(&dir).unwrap();
        assert!(skills.join("someone-elses").is_dir());
    }
}
