use std::process::Command;

#[test]
fn help_never_displays_authentication_environment_values() {
    let variables = [
        "ATLASSIAN_DOMAIN",
        "ATLASSIAN_EMAIL",
        "ATLASSIAN_API_TOKEN",
        "ATLASSIAN_CLIENT_ID",
        "ATLASSIAN_CLIENT_SECRET",
        "ATLASSIAN_CLOUD_ID",
    ];
    let mut command = Command::new(env!("CARGO_BIN_EXE_atlassian-cli"));
    command.arg("--help");
    for variable in variables {
        command.env(variable, format!("private-{variable}"));
    }
    let output = command.output().unwrap();
    assert!(output.status.success());
    assert!(output.stderr.is_empty());
    let help = String::from_utf8(output.stdout).unwrap();
    for variable in variables {
        assert!(help.contains(variable));
        assert!(!help.contains(&format!("private-{variable}")));
    }
}

#[test]
fn usage_errors_are_text_on_stderr_with_exit_code_two() {
    for arguments in [
        vec!["--unknown-option"],
        vec!["jira", "search", "project = PROJ", "--stream"],
        vec!["confluence", "search", "space = TEAM", "--stream"],
    ] {
        let output = Command::new(env!("CARGO_BIN_EXE_atlassian-cli"))
            .args(["--config", "missing-config-for-usage-test.toml"])
            .args(&arguments)
            .output()
            .unwrap();
        assert_eq!(output.status.code(), Some(2), "{arguments:?}");
        assert!(output.stdout.is_empty());
        let error = String::from_utf8(output.stderr).unwrap();
        assert!(error.contains("Usage:"), "{error}");
    }
}

#[test]
fn config_management_uses_the_explicit_file() {
    let dir = tempfile::tempdir().unwrap();
    let custom = dir.path().join("custom.toml");
    let project = dir.path().join(".atlassian.toml");
    std::fs::write(&project, "[default]\n").unwrap();
    let run = |arguments: &[&str]| {
        Command::new(env!("CARGO_BIN_EXE_atlassian-cli"))
            .current_dir(dir.path())
            .arg("--config")
            .arg(&custom)
            .args(arguments)
            .output()
            .unwrap()
    };
    let output = run(&["config", "init"]);
    assert!(output.status.success(), "{:?}", output.stderr);
    assert!(custom.is_file());
    assert_eq!(std::fs::read_to_string(&project).unwrap(), "[default]\n");
    let original = std::fs::read(&custom).unwrap();
    assert!(!run(&["config", "init"]).status.success());
    assert_eq!(std::fs::read(&custom).unwrap(), original);
    let output = run(&["config", "path"]);
    assert!(output.status.success());
    assert_eq!(
        String::from_utf8(output.stdout).unwrap().trim(),
        custom.to_str().unwrap()
    );
    let output = run(&["config", "path", "--global"]);
    assert!(output.status.success());
    assert_eq!(
        String::from_utf8(output.stdout).unwrap().trim(),
        atlassian_cli::Config::global_config_path()
            .unwrap()
            .to_str()
            .unwrap()
    );
    std::fs::remove_file(&custom).unwrap();
    assert!(run(&["config", "path"]).status.success());
    let output = run(&["config", "edit"]);
    assert_eq!(output.status.code(), Some(1));
    assert!(output.stdout.is_empty());
    assert!(serde_json::from_slice::<serde_json::Value>(&output.stderr).is_ok());
    let output = run(&["config", "list"]);
    assert_eq!(output.status.code(), Some(1));
    assert!(serde_json::from_slice::<serde_json::Value>(&output.stderr).is_ok());
}

#[cfg(unix)]
#[test]
fn config_edit_passes_the_selected_path_to_the_editor() {
    use std::os::unix::fs::PermissionsExt;

    let dir = tempfile::tempdir().unwrap();
    let custom = dir.path().join("custom config.toml");
    let project = dir.path().join(".atlassian.toml");
    for path in [&custom, &project] {
        std::fs::write(path, "[default]\n").unwrap();
    }
    let editor = dir.path().join("editor");
    std::fs::write(
        &editor,
        "#!/bin/sh\nprintf '\\n# editor invoked\\n' >> \"$1\"\n",
    )
    .unwrap();
    std::fs::set_permissions(&editor, std::fs::Permissions::from_mode(0o755)).unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_atlassian-cli"))
        .current_dir(dir.path())
        .args(["--config", custom.to_str().unwrap(), "config", "edit"])
        .env("EDITOR", &editor)
        .output()
        .unwrap();
    assert!(output.status.success(), "{:?}", output.stderr);
    assert!(
        std::fs::read_to_string(custom)
            .unwrap()
            .contains("# editor invoked")
    );
    assert_eq!(std::fs::read_to_string(project).unwrap(), "[default]\n");
}

#[cfg(unix)]
#[test]
fn config_edit_preserves_editor_arguments_and_option_like_filenames() {
    use std::os::unix::fs::PermissionsExt;

    let dir = tempfile::tempdir().unwrap();
    let editor = dir.path().join("fixture editor");
    std::fs::write(&editor, "#!/bin/sh\n[ \"$1\" = '--wait' ] || exit 2\nshift\n[ \"$#\" = 1 ] || exit 3\ncase \"$1\" in /*) ;; *) exit 4 ;; esac\nprintf '\\n# editor invoked\\n' >> \"$1\"\n").unwrap();
    std::fs::set_permissions(&editor, std::fs::Permissions::from_mode(0o755)).unwrap();
    let editor_command = format!(
        "{} --wait",
        shlex::try_quote(editor.to_str().unwrap()).unwrap()
    );
    for filename in [
        "--help",
        "--notes $(touch sentinel).toml",
        "space config.toml",
    ] {
        let path = dir.path().join(filename);
        std::fs::write(&path, "[default]\n").unwrap();
        let output = Command::new(env!("CARGO_BIN_EXE_atlassian-cli"))
            .current_dir(dir.path())
            .arg(format!("--config={filename}"))
            .args(["config", "edit"])
            .env("EDITOR", &editor_command)
            .output()
            .unwrap();
        assert!(output.status.success(), "{:?}", output.stderr);
        assert!(
            std::fs::read_to_string(path)
                .unwrap()
                .contains("# editor invoked")
        );
    }
    assert!(!dir.path().join("sentinel").exists());
}

#[test]
fn invalid_editor_commands_leave_the_config_untouched() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("config.toml");
    std::fs::write(&path, "[default]\n").unwrap();
    let values: Vec<std::ffi::OsString> = ["", "  ", "''", "editor '"]
        .into_iter()
        .map(Into::into)
        .collect();
    #[cfg(unix)]
    let values = {
        use std::os::unix::ffi::OsStringExt;
        let mut values = values;
        values.push(std::ffi::OsString::from_vec(vec![0xff]));
        values
    };
    for value in values {
        let output = Command::new(env!("CARGO_BIN_EXE_atlassian-cli"))
            .arg("--config")
            .arg(&path)
            .args(["config", "edit"])
            .env("EDITOR", value)
            .output()
            .unwrap();
        assert_eq!(output.status.code(), Some(1));
        assert!(output.stdout.is_empty());
        assert!(serde_json::from_slice::<serde_json::Value>(&output.stderr).is_ok());
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "[default]\n");
    }
}
