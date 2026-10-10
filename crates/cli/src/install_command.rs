//! `cli install`: install OSVVM, UVVM or vendor libraries with nvc's
//! install scripts, or list what can be installed and what already is.

use std::path::PathBuf;
use std::process::ExitCode;

use clap::Args;
use nvc::{
    CancelToken, NvcInstall, default_library_dir, default_tcllib_dir, find_bash, install_package,
    installable_packages, installed_libraries, locate_nvc,
};

#[derive(Debug, Args)]
pub struct InstallArgs {
    /// Package to install, e.g. osvvm or uvvm. Leave out to list packages
    /// and installed libraries.
    pub package: Option<String>,

    /// Folder of Tcllib packages for the OSVVM install (TCLLIBPATH).
    /// Default: ~/.cache/nvc-studio/tcllib/modules if it exists.
    #[arg(long)]
    pub tcllib: Option<PathBuf>,

    /// Path to the nvc program. Default: search PATH.
    #[arg(long)]
    pub nvc: Option<PathBuf>,
}

pub fn run(args: InstallArgs) -> ExitCode {
    let install = match locate_nvc(args.nvc.as_deref()) {
        Ok(install) => install,
        Err(error) => {
            eprintln!("error: {error}");
            return ExitCode::FAILURE;
        }
    };
    match &args.package {
        None => {
            print!("{}", describe_libraries(&install));
            ExitCode::SUCCESS
        }
        Some(package) => install_one(&install, package, args.tcllib.or_else(default_tcllib_dir)),
    }
}

fn install_one(install: &NvcInstall, package: &str, tcllib: Option<PathBuf>) -> ExitCode {
    let Some(bash) = find_bash() else {
        eprintln!("error: {}", nvc::InstallError::NoBash);
        return ExitCode::FAILURE;
    };
    println!("Installing {package} using {}", bash.program.display());
    if let Some(dir) = &tcllib {
        println!("Tcllib: {}", dir.display());
    }

    // As elsewhere in the CLI, Ctrl+C reaches the install directly.
    let result = install_package(install, &bash, package, tcllib.as_deref(), &CancelToken::new(), &mut |line| {
        println!("{line}")
    });
    match result {
        Ok(process) if process.succeeded() => {
            println!("Installed {package} ({:.0} s)", process.duration.as_secs_f64());
            ExitCode::SUCCESS
        }
        Ok(process) => {
            let how = match process.exit_code {
                _ if process.cancelled => "cancelled".to_string(),
                Some(code) => format!("exit code {code}"),
                None => "stopped without an exit code".to_string(),
            };
            println!("Install of {package} FAILED ({how})");
            ExitCode::FAILURE
        }
        Err(error) => {
            eprintln!("error: {error}");
            ExitCode::FAILURE
        }
    }
}

/// What can be installed and what is installed already.
fn describe_libraries(install: &NvcInstall) -> String {
    let mut text = format!("Packages nvc can install: {}\n", installable_packages(install).join(", "));
    match default_library_dir() {
        Some(dir) => {
            let installed = installed_libraries(&dir);
            if installed.is_empty() {
                text.push_str(&format!("Nothing installed yet in {}\n", dir.display()));
            } else {
                text.push_str(&format!("Installed in {}:\n", dir.display()));
                for library in installed {
                    text.push_str(&format!("  {library}\n"));
                }
            }
        }
        None => text.push_str("Could not find your home folder.\n"),
    }
    text
}
