//! The Claude Code skill this binary carries and writes out.
//!
//! The skill describes the commands this binary exposes, so it is true only of
//! the version it shipped with. Compiling it in rather than fetching it beside
//! the binary is what makes "the deployed skill matches the binary" hold by
//! construction: there is one artifact, and a deploy writes the copy. What is
//! left to check is whether a deployed copy still equals what this binary
//! carries, which is a byte comparison rather than a version string — and
//! therefore has no reading in which it is merely probably right.

use std::collections::BTreeSet;
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
    let carried: BTreeSet<&str> = FILES.iter().map(|(relative, _)| *relative).collect();
    if deployed_files(dir) != carried.iter().map(|r| r.to_string()).collect() {
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

/// Write every carried file into `dir`, and remove anything there that this
/// binary does not carry.
pub fn deploy(dir: &Path) -> Result<Deployed, DistError> {
    let mut outcome = Deployed::default();
    let carried: BTreeSet<String> = FILES
        .iter()
        .map(|(relative, _)| (*relative).to_string())
        .collect();

    for stale in deployed_files(dir).difference(&carried) {
        let path = dir.join(stale);
        std::fs::remove_file(&path)
            .map_err(|e| DistError::io(format!("removing {}", path.display()), e))?;
        outcome.pruned.push(stale.clone());
    }

    for (relative, contents) in FILES {
        let path = dir.join(relative);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| DistError::io(format!("creating {}", parent.display()), e))?;
        }
        std::fs::write(&path, contents)
            .map_err(|e| DistError::io(format!("writing {}", path.display()), e))?;
        outcome.written.push((*relative).to_string());
    }
    Ok(outcome)
}

/// Remove the deployed skill, reporting whether there was one. Empty ancestors
/// go too, up to but never including the home directory: a `~/.claude` this
/// tool emptied is one it created.
pub fn remove(dir: &Path) -> Result<bool, DistError> {
    if !dir.exists() {
        return Ok(false);
    }
    std::fs::remove_dir_all(dir)
        .map_err(|e| DistError::io(format!("removing {}", dir.display()), e))?;
    prune_empty_ancestors(dir, 2);
    Ok(true)
}

/// Every file under `dir`, addressed the way `FILES` addresses them.
fn deployed_files(dir: &Path) -> BTreeSet<String> {
    let mut found = BTreeSet::new();
    collect(dir, dir, &mut found);
    found
}

fn collect(root: &Path, dir: &Path, into: &mut BTreeSet<String>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect(root, &path, into);
        } else if let Ok(relative) = path.strip_prefix(root) {
            into.insert(relative.to_string_lossy().replace('\\', "/"));
        }
    }
}

fn prune_empty_ancestors(from: &Path, levels: usize) {
    let home = dirs::home_dir();
    let mut current = from.parent().map(Path::to_path_buf);
    for _ in 0..levels {
        let Some(dir) = current else { return };
        if home.as_deref() == Some(dir.as_path()) || std::fs::remove_dir(&dir).is_err() {
            return;
        }
        current = dir.parent().map(Path::to_path_buf);
    }
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
