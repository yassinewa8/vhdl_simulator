//! Headless command-line tool.
//!
//! Commands:
//! - `cli run <files...> --top <entity>`: analyse, elaborate and run

mod format;
mod run_command;

use std::process::ExitCode;

use clap::{Parser, Subcommand};

#[derive(Debug, Parser)]
#[command(
    name = "cli",
    version,
    about = "NVC Studio command-line tool (placeholder name)"
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Analyse, elaborate and run a design, printing parsed messages.
    Run(run_command::RunArgs),
}

fn main() -> ExitCode {
    match Cli::parse().command {
        Command::Run(args) => run_command::run(args),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::CommandFactory;

    #[test]
    fn command_line_definition_is_valid() {
        Cli::command().debug_assert();
    }

    #[test]
    fn parses_a_run_command() {
        let cli = Cli::try_parse_from([
            "cli", "run", "a.vhd", "b.vhd", "--top", "tb", "--std", "2019", "-g", "N=4",
            "--stop-time", "5us",
        ])
        .unwrap();
        let Command::Run(args) = cli.command;
        assert_eq!(args.files.len(), 2);
        assert_eq!(args.top, "tb");
        assert_eq!(args.standard, nvc::VhdlStandard::Vhdl2019);
        assert_eq!(args.generics, [("N".to_string(), "4".to_string())]);
        assert_eq!(args.stop_time, studio_core::SimTime::parse("5us"));
    }

    #[test]
    fn run_needs_files_and_top() {
        assert!(Cli::try_parse_from(["cli", "run", "--top", "tb"]).is_err());
        assert!(Cli::try_parse_from(["cli", "run", "a.vhd"]).is_err());
    }
}
