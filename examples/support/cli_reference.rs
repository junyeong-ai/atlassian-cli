use std::fmt::Write;

pub fn render(mut command: clap::Command) -> String {
    command = command.color(clap::ColorChoice::Never);
    command.build();
    let mut output = String::from(
        "# CLI reference\n\nGenerated from the command definitions. Run `cargo run --locked --example cli_reference > docs/cli.md` to refresh.\n\nRoot options go before the subcommand. Run `atlassian-cli <command> --help` for local help.\n",
    );
    write_command(&command, command.get_name(), &mut output);
    output
}

fn write_command(command: &clap::Command, path: &str, output: &mut String) {
    writeln!(output, "\n## `{path}`\n\n```text").unwrap();
    for line in command.clone().render_long_help().to_string().lines() {
        writeln!(output, "{}", line.trim_end()).unwrap();
    }
    writeln!(output, "```").unwrap();
    for child in command
        .get_subcommands()
        .filter(|child| !child.is_hide_set() && child.get_name() != "help")
    {
        write_command(child, &format!("{path} {}", child.get_name()), output);
    }
}
