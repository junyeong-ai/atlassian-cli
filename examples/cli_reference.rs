use clap::CommandFactory;

#[allow(dead_code)]
#[path = "../src/cli.rs"]
mod cli;
#[path = "support/cli_reference.rs"]
mod cli_reference;

fn main() {
    print!("{}", cli_reference::render(cli::Cli::command()));
}
