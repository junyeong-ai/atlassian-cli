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
