use std::path::Path;
use std::process::Command;

use anyhow::{Context, Result, ensure};

pub(crate) fn command(editor: &str, path: &Path) -> Result<Command> {
    let words = shlex::split(editor).context("EDITOR contains invalid quoting or escaping")?;
    let (program, arguments) = words.split_first().context("EDITOR is empty")?;
    ensure!(!program.is_empty(), "EDITOR executable is empty");
    let path = std::path::absolute(path).context("Failed to resolve the config file path")?;
    let mut command = Command::new(program);
    command.args(arguments).arg(path);
    Ok(command)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::OsStr;

    #[test]
    fn editor_commands_preserve_quoted_programs_and_arguments() {
        for (input, program, arguments) in [
            ("code --wait", "code", vec!["--wait"]),
            (
                "'/editor path/code' --wait 'two words'",
                "/editor path/code",
                vec!["--wait", "two words"],
            ),
            (
                r"'C:\Program Files\Code\Code.exe' --wait",
                r"C:\Program Files\Code\Code.exe",
                vec!["--wait"],
            ),
            (
                "editor '--literal=$HOME' '$(touch sentinel)'",
                "editor",
                vec!["--literal=$HOME", "$(touch sentinel)"],
            ),
        ] {
            let selected = Path::new("--help");
            let command = command(input, selected).unwrap();
            assert_eq!(command.get_program(), OsStr::new(program));
            let actual: Vec<_> = command.get_args().collect();
            assert_eq!(actual.len(), arguments.len() + 1);
            for (actual, expected) in actual.iter().zip(arguments) {
                assert_eq!(*actual, OsStr::new(expected));
            }
            let config = Path::new(actual.last().unwrap());
            assert!(config.is_absolute());
            assert_eq!(config.file_name(), Some(OsStr::new("--help")));
        }
    }

    #[test]
    fn invalid_editor_commands_are_rejected() {
        for input in ["", "  ", "''", "editor '", "editor \\"] {
            assert!(
                command(input, Path::new("config.toml")).is_err(),
                "{input:?}"
            );
        }
    }
}
