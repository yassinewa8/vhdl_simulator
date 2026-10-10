//! `cli install`: install OSVVM, UVVM or vendor libraries (e.g. Quartus) with nvc's
//! install scripts, or list what can be installed and what already is.

use std::path::PathBuf;
use std::process::ExitCode;

use clap::Args;
use nvc::{
    CancelToken, InstallOptions, NvcInstall, default_library_dir, default_tcllib_dir, find_bash, find_quartus,
    install_package, installable_packages, installed_libraries, locate_nvc, quartus_root_from,
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

    /// Quartus installation folder for `install quartus` (the `quartus`
    /// folder or the one above it). Default: QUARTUS_ROOTDIR, else the
    /// newest Quartus in the usual places (C:\\intelFPGA_lite\\... etc.).
    #[arg(long)]
    pub quartus: Option<PathBuf>,

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
        Some(package) => {
            let options = InstallOptions {
                tcllib_dir: args.tcllib.or_else(default_tcllib_dir),
                quartus_root: quartus_root(package, args.quartus),
            };
            install_one(&install, package, &options)
        }
    }
}

/// The Quartus folder for a `quartus` install: the one given (a folder that
/// isn't Quartus is kept, so the install says why), or one found.
fn quartus_root(package: &str, given: Option<PathBuf>) -> Option<PathBuf> {
    match given {
        Some(dir) => Some(quartus_root_from(&dir).unwrap_or(dir)),
        None if package == "quartus" => find_quartus(),
        None => None,
    }
}

fn install_one(install: &NvcInstall, package: &str, options: &InstallOptions) -> ExitCode {
    let Some(bash) = find_bash() else {
        eprintln!("error: {}", nvc::InstallError::NoBash);
        return ExitCode::FAILURE;
    };
    println!("Installing {package} using {}", bash.program.display());
    if let Some(dir) = &options.tcllib_dir {
        println!("Tcllib: {}", dir.display());
    }
    if let Some(dir) = &options.quartus_root {
        println!("Quartus: {}", dir.display());
    }

    // As elsewhere in the CLI, Ctrl+C reaches the install directly.
    let result = install_package(install, &bash, package, options, &CancelToken::new(), &mut |line| {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quartus_folder_given_or_only_looked_for_quartus() {
        let base = std::env::temp_dir().join("nvc-studio-tests").join("cli_quartus_root");
        let _ = std::fs::remove_dir_all(&base);
        let root = base.join("quartus");
        std::fs::create_dir_all(root.join("eda").join("sim_lib")).unwrap();

        // The folder above `quartus` is accepted, and so is `quartus` itself.
        assert_eq!(quartus_root("quartus", Some(base.clone())), Some(root.clone()));
        assert_eq!(quartus_root("quartus", Some(root.clone())), Some(root));
        // A wrong folder is kept, so the install can say what's wrong with it.
        let wrong = base.join("nothing here");
        assert_eq!(quartus_root("quartus", Some(wrong.clone())), Some(wrong));
        // Other packages don't go looking for Quartus.
        assert_eq!(quartus_root("osvvm", None), None);
    }
}
