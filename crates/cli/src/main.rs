//! Headless command-line tool.
//!
//! Commands:
//! - `cli run <files...> --top <entity>`: analyse, elaborate and run
//! - `cli build [project]`: compile a project in dependency order
//! - `cli install [package]`: install OSVVM, UVVM etc., or list them

mod build_command;
mod format;
mod install_command;
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
    /// Compile a project from its project.toml, in dependency order,
    /// recompiling only what changed.
    Build(build_command::BuildArgs),
    /// Install OSVVM, UVVM or vendor libraries with nvc's install scripts.
    /// Without a package name, lists what can be installed.
    Install(install_command::InstallArgs),
}

fn main() -> ExitCode {
    match Cli::parse().command {
        Command::Run(args) => run_command::run(args),
        Command::Build(args) => build_command::run(args),
        Command::Install(args) => install_command::run(args),
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
        let Command::Run(args) = cli.command else {
            panic!("expected a run command");
        };
        assert_eq!(args.files.len(), 2);
        assert_eq!(args.top, "tb");
        assert_eq!(args.standard, nvc::VhdlStandard::Vhdl2019);
        assert_eq!(args.generics, [("N".to_string(), "4".to_string())]);
        assert_eq!(args.stop_time, studio_core::SimTime::parse("5us"));
    }

    #[test]
    fn parses_a_build_command() {
        let cli = Cli::try_parse_from(["cli", "build", "my/project.toml", "--force"]).unwrap();
        let Command::Build(args) = cli.command else {
            panic!("expected a build command");
        };
        assert_eq!(args.project, std::path::PathBuf::from("my/project.toml"));
        assert!(args.force);
        assert!(!args.dry_run);

        // With no path, the current folder is used.
        let cli = Cli::try_parse_from(["cli", "build", "--dry-run"]).unwrap();
        let Command::Build(args) = cli.command else {
            panic!("expected a build command");
        };
        assert_eq!(args.project, std::path::PathBuf::from("."));
        assert!(args.dry_run);
    }

    #[test]
    fn parses_an_install_command() {
        let cli = Cli::try_parse_from(["cli", "install", "osvvm", "--tcllib", "C:/tcllib"]).unwrap();
        let Command::Install(args) = cli.command else {
            panic!("expected an install command");
        };
        assert_eq!(args.package.as_deref(), Some("osvvm"));
        assert_eq!(args.tcllib, Some(std::path::PathBuf::from("C:/tcllib")));

        let cli = Cli::try_parse_from(["cli", "install"]).unwrap();
        let Command::Install(args) = cli.command else {
            panic!("expected an install command");
        };
        assert_eq!(args.package, None);
    }

    #[test]
    fn run_needs_files_and_top() {
        assert!(Cli::try_parse_from(["cli", "run", "--top", "tb"]).is_err());
        assert!(Cli::try_parse_from(["cli", "run", "a.vhd"]).is_err());
    }
}
