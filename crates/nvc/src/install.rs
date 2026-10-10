//! Installing third-party VHDL libraries (OSVVM, UVVM, vendor libraries)
//! with the install scripts that come with nvc.
//!
//! nvc normally does this with `nvc --install PKG`, which runs the bash
//! script `<nvc>/libexec/nvc/install-PKG.sh`. We run that same script
//! ourselves instead, because nvc 1.23.0 on Windows passes the script path
//! to bash without quotes: when nvc is installed under `C:\Program Files`,
//! bash only sees `C:\Program` and the install fails. The scripts belong to
//! the user's nvc installation; like nvc itself, we only ever run them as
//! separate programs (see CLAUDE.md).
//!
//! The scripts need bash and git, and the OSVVM one also needs `tclsh`
//! with the Tcllib `fileutil` package. On Windows these come from Git for
//! Windows (or MSYS2). Libraries are compiled into `~/.nvc/lib`, which nvc
//! searches by default, so projects can use them without any `-L` option.
//!
//! Vendor libraries (e.g. `quartus`) download nothing: the script compiles
//! the simulation sources that come with the vendor's tool, so it needs to
//! be told where that tool is installed (see [`InstallOptions`]).

use std::ffi::OsString;
use std::fmt;
use std::path::{Path, PathBuf};

use crate::locate::{NvcInstall, candidates_on_path};
use crate::runner::{CancelToken, ProcessResult, run_process_with_env};

/// A bash we can run the install scripts with, plus any environment
/// variables it needs to find git and tclsh.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Bash {
    pub program: PathBuf,
    pub env: Vec<(String, OsString)>,
}

/// Finds a bash suitable for nvc's install scripts, or `None`.
///
/// On Windows: Git for Windows, then MSYS2. `bash` on PATH is *not* used
/// there, because `C:\Windows\System32\bash.exe` is WSL, which can't run a
/// Windows nvc. Elsewhere: `bash` on PATH.
pub fn find_bash() -> Option<Bash> {
    let candidates = if cfg!(windows) {
        let program_files = std::env::var_os("ProgramFiles").map(PathBuf::from);
        let path = std::env::var_os("PATH").unwrap_or_default();
        windows_bash_candidates(program_files.as_deref(), Path::new("C:\\msys64"), &path)
    } else {
        let path = std::env::var_os("PATH").unwrap_or_default();
        candidates_on_path(&path, "bash")
            .into_iter()
            .map(|program| Bash {
                program,
                env: Vec::new(),
            })
            .collect()
    };
    candidates.into_iter().find(|bash| bash.program.is_file())
}

/// Places to look for bash on Windows, best first.
fn windows_bash_candidates(program_files: Option<&Path>, msys2_root: &Path, path: &OsString) -> Vec<Bash> {
    let mut candidates = Vec::new();

    // Git for Windows' `bin\bash.exe` is a launcher that sets up PATH so
    // git and tclsh (in its ucrt64 folder) are found. Its `usr\bin\bash.exe`
    // doesn't, so must not be used.
    if let Some(program_files) = program_files {
        candidates.push(Bash {
            program: program_files.join("Git").join("bin").join("bash.exe"),
            env: Vec::new(),
        });
    }

    // MSYS2's bash needs its tool folders put on PATH by hand.
    let mut search_path = vec![msys2_root.join("ucrt64").join("bin"), msys2_root.join("usr").join("bin")];
    search_path.extend(std::env::split_paths(path));
    let joined = std::env::join_paths(search_path).unwrap_or_else(|_| path.clone());
    candidates.push(Bash {
        program: msys2_root.join("usr").join("bin").join("bash.exe"),
        env: vec![
            ("MSYSTEM".to_string(), OsString::from("UCRT64")),
            ("PATH".to_string(), joined),
        ],
    });
    candidates
}

/// The folder holding nvc's install scripts: `<prefix>/libexec/nvc`, where
/// nvc itself is `<prefix>/bin/nvc`.
pub fn libexec_dir(install: &NvcInstall) -> PathBuf {
    // Take the parent of `bin` directly rather than writing `bin/..`:
    // Linux only accepts `bin/..` if `bin` really exists, Windows doesn't
    // care, and a test once passed on one and failed on the other.
    let prefix = install.program.parent().and_then(Path::parent).unwrap_or(Path::new("."));
    prefix.join("libexec").join("nvc")
}

/// Packages this nvc can install (`osvvm`, `uvvm`, `vivado`, ...), sorted.
pub fn installable_packages(install: &NvcInstall) -> Vec<String> {
    let Ok(entries) = std::fs::read_dir(libexec_dir(install)) else {
        return Vec::new();
    };
    let mut packages: Vec<String> = entries
        .filter_map(Result::ok)
        .filter_map(|entry| {
            let name = entry.file_name().to_string_lossy().into_owned();
            let package = name.strip_prefix("install-")?.strip_suffix(".sh")?;
            Some(package.to_string())
        })
        .collect();
    packages.sort();
    packages
}

/// Where installed libraries go and where nvc looks for them by default:
/// `~/.nvc/lib`.
pub fn default_library_dir() -> Option<PathBuf> {
    std::env::home_dir().map(|home| home.join(".nvc").join("lib"))
}

/// Compiled libraries in `dir`, as folder names such as `OSVVM.08`
/// (the suffix is the VHDL standard: nvc picks `.08` for VHDL-2008).
pub fn installed_libraries(dir: &Path) -> Vec<String> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut libraries: Vec<String> = entries
        .filter_map(Result::ok)
        // Every nvc library folder contains a `_NVC_LIB` marker file.
        .filter(|entry| entry.path().join("_NVC_LIB").is_file())
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .collect();
    libraries.sort();
    libraries
}

/// Where we keep a copy of Tcllib for the OSVVM install, if it's there:
/// `~/.cache/nvc-studio/tcllib/modules`. (How users get it is decided in
/// Phase 7; for now it's downloaded by hand.)
pub fn default_tcllib_dir() -> Option<PathBuf> {
    let dir = std::env::home_dir()?
        .join(".cache")
        .join("nvc-studio")
        .join("tcllib")
        .join("modules");
    dir.is_dir().then_some(dir)
}

/// Where a Quartus install keeps its VHDL simulation sources, relative to
/// its `quartus` folder: older versions use the first, Quartus 22.1 and
/// later (which ship Questa instead of ModelSim) the second.
const QUARTUS_SIM_SOURCES: [&str; 2] = ["eda/sim_lib", "questa_fse/intel/vhdl/src/altera"];

/// True if `dir` is a Quartus `quartus` folder with simulation sources,
/// i.e. a valid `QUARTUS_ROOTDIR`.
pub fn is_quartus_root(dir: &Path) -> bool {
    QUARTUS_SIM_SOURCES.iter().any(|sources| dir.join(sources).is_dir())
}

/// Turns a folder the user picked into the `quartus` folder the install
/// needs. People often pick the folder above it (e.g.
/// `C:\intelFPGA_lite\23.1std`), so that's accepted too.
pub fn quartus_root_from(picked: &Path) -> Option<PathBuf> {
    [picked.to_path_buf(), picked.join("quartus")]
        .into_iter()
        .find(|dir| is_quartus_root(dir))
}

/// Finds an installed Quartus: the `QUARTUS_ROOTDIR` environment variable
/// (set by the Quartus installer on some systems), otherwise the newest
/// version in the usual install folders.
pub fn find_quartus() -> Option<PathBuf> {
    if let Some(dir) = std::env::var_os("QUARTUS_ROOTDIR").and_then(|dir| quartus_root_from(Path::new(&dir))) {
        return Some(dir);
    }
    let bases: Vec<PathBuf> = if cfg!(windows) {
        ["C:\\intelFPGA_lite", "C:\\intelFPGA", "C:\\intelFPGA_pro", "C:\\altera_lite", "C:\\altera", "C:\\altera_pro"]
            .iter()
            .map(PathBuf::from)
            .collect()
    } else {
        let home = std::env::home_dir().unwrap_or_default();
        ["intelFPGA_lite", "intelFPGA", "intelFPGA_pro", "altera_lite", "altera", "altera_pro"]
            .iter()
            .flat_map(|name| [home.join(name), Path::new("/opt").join(name)])
            .collect()
    };
    quartus_in(&bases)
}

/// The newest Quartus in `bases`, which hold one folder per version
/// (`<base>/<version>/quartus`). Versions such as `23.1std` and `24.2`
/// sort correctly as text, apart from very old ones (`9.1` vs `13.0`),
/// so they're compared number by number.
fn quartus_in(bases: &[PathBuf]) -> Option<PathBuf> {
    let mut found: Vec<(Vec<u32>, PathBuf)> = Vec::new();
    for base in bases {
        let Ok(entries) = std::fs::read_dir(base) else { continue };
        for entry in entries.filter_map(Result::ok) {
            let root = entry.path().join("quartus");
            if is_quartus_root(&root) {
                found.push((version_numbers(&entry.file_name().to_string_lossy()), root));
            }
        }
    }
    found.sort();
    found.pop().map(|(_, root)| root)
}

/// `"23.1std"` -> `[23, 1]`: the numbers at the start of a version folder.
fn version_numbers(name: &str) -> Vec<u32> {
    name.split('.')
        .map_while(|part| {
            let digits: String = part.chars().take_while(char::is_ascii_digit).collect();
            digits.parse().ok()
        })
        .collect()
}

/// Settings some install scripts need.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct InstallOptions {
    /// A folder containing Tcllib packages, given to tclsh as `TCLLIBPATH`;
    /// the OSVVM install needs it if the system Tcl lacks Tcllib.
    pub tcllib_dir: Option<PathBuf>,
    /// The Quartus `quartus` folder (`QUARTUS_ROOTDIR`); needed by the
    /// `quartus` install.
    pub quartus_root: Option<PathBuf>,
}

/// Why an install couldn't start.
#[derive(Debug)]
pub enum InstallError {
    UnknownPackage { package: String, available: Vec<String> },
    NoBash,
    /// The `quartus` install was asked for without a usable Quartus folder.
    NoQuartus { given: Option<PathBuf> },
    /// nvc's Quartus script splits the Quartus path at spaces.
    QuartusPathHasSpaces(PathBuf),
    CannotStart(std::io::Error),
}

impl fmt::Display for InstallError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            InstallError::UnknownPackage { package, available } => write!(
                f,
                "nvc has no install script for '{package}'. Available: {}",
                available.join(", ")
            ),
            InstallError::NoBash => write!(
                f,
                "installing libraries needs bash and git. On Windows, install Git for \
                 Windows from https://git-scm.com (it includes both)."
            ),
            InstallError::NoQuartus { given: None } => write!(
                f,
                "could not find Quartus. Choose its install folder (the one containing \
                 `quartus`, e.g. C:\\intelFPGA_lite\\23.1std)."
            ),
            InstallError::NoQuartus { given: Some(dir) } => write!(
                f,
                "{} is not a Quartus install folder: it has no simulation sources \
                 (quartus/eda/sim_lib or quartus/questa_fse/intel/vhdl/src/altera).",
                dir.display()
            ),
            InstallError::QuartusPathHasSpaces(dir) => write!(
                f,
                "nvc's Quartus install script can't handle spaces in the Quartus folder ({}). \
                 Reinstall Quartus in a folder without spaces, such as the default C:\\intelFPGA_lite.",
                dir.display()
            ),
            InstallError::CannotStart(error) => write!(f, "could not start the install: {error}"),
        }
    }
}

impl std::error::Error for InstallError {}

/// Runs nvc's install script for `package` (e.g. `"osvvm"`), passing each
/// line of output to `on_line` as it arrives.
///
/// Downloads go to `~/.cache/nvc` and libraries to `~/.nvc/lib`.
pub fn install_package(
    install: &NvcInstall,
    bash: &Bash,
    package: &str,
    options: &InstallOptions,
    cancel: &CancelToken,
    on_line: &mut dyn FnMut(&str),
) -> Result<ProcessResult, InstallError> {
    let available = installable_packages(install);
    if !available.iter().any(|name| name == package) {
        return Err(InstallError::UnknownPackage {
            package: package.to_string(),
            available,
        });
    }

    // The script checks this too, but its message talks about an
    // environment variable the user never sees.
    if package == "quartus" {
        check_quartus_root(options.quartus_root.as_deref())?;
    }

    let script = libexec_dir(install).join(format!("install-{package}.sh"));
    let args = vec![forward_slashes(&script)];
    let env = install_environment(install, bash, options);
    // The scripts change into their own folders, so where we start doesn't
    // matter; the temp folder is always writable.
    let working_dir = std::env::temp_dir();

    run_process_with_env(&bash.program, &args, &env, &working_dir, cancel, &mut |_, line| on_line(line))
        .map_err(InstallError::CannotStart)
}

/// Checks the Quartus folder before running nvc's script. The script
/// checks too, but its message talks about an environment variable the
/// user never sees.
fn check_quartus_root(quartus_root: Option<&Path>) -> Result<(), InstallError> {
    match quartus_root {
        Some(dir) if !is_quartus_root(dir) => Err(InstallError::NoQuartus {
            given: Some(dir.to_path_buf()),
        }),
        // The script passes the path to nvc without quotes
        // (`--relative=$QUARTUS_ROOTDIR`), so bash splits it at spaces and
        // every file fails to compile.
        Some(dir) if dir.to_string_lossy().contains(' ') => Err(InstallError::QuartusPathHasSpaces(dir.to_path_buf())),
        Some(_) => Ok(()),
        None => Err(InstallError::NoQuartus { given: None }),
    }
}

/// Environment for the install script: bash's own settings, `NVC` so the
/// scripts use this nvc (they fall back to `nvc` on PATH otherwise), git
/// line-ending settings (see below), and `TCLLIBPATH` / `QUARTUS_ROOTDIR`
/// if given.
fn install_environment(install: &NvcInstall, bash: &Bash, options: &InstallOptions) -> Vec<(String, OsString)> {
    let mut env = bash.env.clone();
    env.push(("NVC".to_string(), OsString::from(forward_slashes(&install.program))));

    // Git for Windows defaults to `core.autocrlf=true`, which clones text
    // files with Windows line endings. The UVVM script reads file names from
    // `compile_order.txt` without removing the trailing carriage return, so
    // nvc is asked to open "types_pkg.vhd<CR>" and fails. Cloning with
    // `core.autocrlf=false` keeps files exactly as in the repository. These
    // variables set a git option for this process only; the user's own git
    // configuration is untouched.
    env.push(("GIT_CONFIG_COUNT".to_string(), OsString::from("1")));
    env.push(("GIT_CONFIG_KEY_0".to_string(), OsString::from("core.autocrlf")));
    env.push(("GIT_CONFIG_VALUE_0".to_string(), OsString::from("false")));

    if let Some(dir) = &options.tcllib_dir {
        // TCLLIBPATH is a Tcl list, so braces keep a path with spaces whole.
        env.push(("TCLLIBPATH".to_string(), OsString::from(format!("{{{}}}", forward_slashes(dir)))));
    }
    if let Some(dir) = &options.quartus_root {
        // The script converts it with `cygpath` under Windows bash, which
        // accepts `C:/...`.
        env.push(("QUARTUS_ROOTDIR".to_string(), OsString::from(forward_slashes(dir))));
    }
    env
}

/// The path with `/` separators. The scripts locate their helper files with
/// `dirname "$BASH_SOURCE"`, which doesn't understand `\`.
fn forward_slashes(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::locate::Version;

    fn fake_install(dir: &Path) -> NvcInstall {
        NvcInstall {
            program: dir.join("bin").join("nvc.exe"),
            version: Version {
                major: 1,
                minor: 23,
                patch: 0,
            },
        }
    }

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join("nvc-studio-tests").join(name);
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn libexec_is_next_to_bin() {
        let install = fake_install(Path::new("C:/Program Files/NVC"));
        let expected = Path::new("C:/Program Files/NVC").join("libexec").join("nvc");
        assert_eq!(libexec_dir(&install), expected);
    }

    #[test]
    fn lists_packages_from_script_names() {
        let dir = scratch("lists_packages_from_script_names");
        let libexec = dir.join("libexec").join("nvc");
        std::fs::create_dir_all(&libexec).unwrap();
        for file in ["install-uvvm.sh", "install-osvvm.sh", "install-osvvm.tcl", "functions.sh"] {
            std::fs::write(libexec.join(file), "").unwrap();
        }
        let install = fake_install(&dir);
        assert_eq!(installable_packages(&install), ["osvvm", "uvvm"]);

        let mut lines = Vec::new();
        let error = install_package(
            &install,
            &Bash {
                program: PathBuf::from("bash"),
                env: vec![],
            },
            "nope",
            &InstallOptions::default(),
            &CancelToken::new(),
            &mut |line| lines.push(line.to_string()),
        )
        .unwrap_err();
        assert_eq!(
            error.to_string(),
            "nvc has no install script for 'nope'. Available: osvvm, uvvm"
        );
        assert!(lines.is_empty());
    }

    #[test]
    fn finds_installed_libraries_by_marker_file() {
        let dir = scratch("finds_installed_libraries_by_marker_file");
        for library in ["OSVVM.08", "OSVVM.19"] {
            std::fs::create_dir_all(dir.join(library)).unwrap();
            std::fs::write(dir.join(library).join("_NVC_LIB"), "").unwrap();
        }
        std::fs::create_dir_all(dir.join("not_a_library")).unwrap();
        assert_eq!(installed_libraries(&dir), ["OSVVM.08", "OSVVM.19"]);
        assert!(installed_libraries(&dir.join("missing")).is_empty());
    }

    // Windows only: the paths contain `C:`, and on Linux `:` separates PATH
    // entries, so they can't be put into a Linux PATH at all. (The code
    // under test only runs on Windows anyway.)
    #[cfg(windows)]
    #[test]
    fn windows_candidates_prefer_git_launcher_then_msys2() {
        let path = std::env::join_paths(["C:/Windows"]).unwrap();
        let candidates = windows_bash_candidates(Some(Path::new("C:/PF")), Path::new("C:/msys64"), &path);
        assert_eq!(candidates[0].program, Path::new("C:/PF").join("Git").join("bin").join("bash.exe"));
        assert!(candidates[0].env.is_empty());

        let msys2 = &candidates[1];
        assert_eq!(msys2.program, Path::new("C:/msys64").join("usr").join("bin").join("bash.exe"));
        assert_eq!(msys2.env[0], ("MSYSTEM".to_string(), OsString::from("UCRT64")));
        let search: Vec<PathBuf> = std::env::split_paths(&msys2.env[1].1).collect();
        assert_eq!(
            search,
            [
                Path::new("C:/msys64").join("ucrt64").join("bin"),
                Path::new("C:/msys64").join("usr").join("bin"),
                PathBuf::from("C:/Windows"),
            ]
        );
    }

    #[test]
    fn environment_names_nvc_and_tcllib() {
        let install = fake_install(Path::new("C:\\Program Files\\NVC"));
        let bash = Bash {
            program: PathBuf::from("bash"),
            env: vec![("MSYSTEM".to_string(), OsString::from("UCRT64"))],
        };
        let options = InstallOptions {
            tcllib_dir: Some(PathBuf::from("C:\\Users\\me\\tcl lib")),
            quartus_root: Some(PathBuf::from("C:\\intelFPGA_lite\\23.1std\\quartus")),
        };
        let env = install_environment(&install, &bash, &options);
        let value = |name: &str| env.iter().find(|(key, _)| key == name).map(|(_, value)| value.clone());
        assert_eq!(env[0].0, "MSYSTEM", "bash's own settings come first");
        let nvc = value("NVC").unwrap();
        assert!(nvc.to_string_lossy().ends_with("/bin/nvc.exe"));
        assert!(!nvc.to_string_lossy().contains('\\'));
        assert_eq!(value("TCLLIBPATH"), Some(OsString::from("{C:/Users/me/tcl lib}")));
        assert_eq!(value("QUARTUS_ROOTDIR"), Some(OsString::from("C:/intelFPGA_lite/23.1std/quartus")));

        // git keeps repository line endings (see install_environment).
        assert_eq!(value("GIT_CONFIG_COUNT"), Some(OsString::from("1")));
        assert_eq!(value("GIT_CONFIG_KEY_0"), Some(OsString::from("core.autocrlf")));
        assert_eq!(value("GIT_CONFIG_VALUE_0"), Some(OsString::from("false")));

        let without = install_environment(&install, &bash, &InstallOptions::default());
        assert!(without.iter().all(|(key, _)| key != "TCLLIBPATH" && key != "QUARTUS_ROOTDIR"));
    }

    /// Makes `<base>/<version>/quartus/<sources>`, like a Quartus install.
    fn fake_quartus(base: &Path, version: &str, sources: &str) -> PathBuf {
        let root = base.join(version).join("quartus");
        std::fs::create_dir_all(root.join(sources)).unwrap();
        root
    }

    #[test]
    fn recognises_both_quartus_source_layouts() {
        let dir = scratch("recognises_both_quartus_source_layouts");
        let old = fake_quartus(&dir, "18.1", "eda/sim_lib");
        let new = fake_quartus(&dir, "23.1std", "questa_fse/intel/vhdl/src/altera");
        assert!(is_quartus_root(&old));
        assert!(is_quartus_root(&new));
        assert!(!is_quartus_root(&dir), "no simulation sources");

        // Picking the version folder (above `quartus`) works too.
        assert_eq!(quartus_root_from(&dir.join("23.1std")), Some(new.clone()));
        assert_eq!(quartus_root_from(&new), Some(new));
        assert_eq!(quartus_root_from(&dir), None);
    }

    #[test]
    fn picks_the_newest_quartus() {
        let dir = scratch("picks_the_newest_quartus");
        let lite = dir.join("intelFPGA_lite");
        let standard = dir.join("intelFPGA");
        fake_quartus(&lite, "9.1", "eda/sim_lib");
        fake_quartus(&lite, "18.1", "eda/sim_lib");
        let newest = fake_quartus(&standard, "23.1std", "questa_fse/intel/vhdl/src/altera");
        // A newer version without simulation sources doesn't count.
        std::fs::create_dir_all(standard.join("24.1std").join("quartus")).unwrap();

        let bases = [lite.clone(), standard, dir.join("missing")];
        assert_eq!(quartus_in(&bases), Some(newest));
        assert_eq!(quartus_in(std::slice::from_ref(&lite)), Some(lite.join("18.1").join("quartus")), "18 is newer than 9");
        assert_eq!(quartus_in(&[dir.join("missing")]), None);
        assert_eq!(version_numbers("23.1std"), [23, 1]);
        assert_eq!(version_numbers("13.0sp1"), [13, 0]);
    }

    #[test]
    fn quartus_install_needs_a_quartus_folder() {
        let dir = scratch("quartus_install_needs_a_quartus_folder");
        let libexec = dir.join("libexec").join("nvc");
        std::fs::create_dir_all(&libexec).unwrap();
        std::fs::write(libexec.join("install-quartus.sh"), "").unwrap();
        let install = fake_install(&dir);
        let bash = Bash {
            program: PathBuf::from("bash"),
            env: vec![],
        };
        let install_with = |quartus_root: Option<PathBuf>| {
            let options = InstallOptions {
                quartus_root,
                ..InstallOptions::default()
            };
            install_package(&install, &bash, "quartus", &options, &CancelToken::new(), &mut |_| {}).unwrap_err()
        };
        assert!(matches!(install_with(None), InstallError::NoQuartus { given: None }));
        let error = install_with(Some(dir.clone()));
        assert!(error.to_string().contains("is not a Quartus install folder"), "{error}");
        let spaced = fake_quartus(&dir.join("Program Files"), "23.1std", "eda/sim_lib");
        assert!(matches!(install_with(Some(spaced)), InstallError::QuartusPathHasSpaces(_)));
    }
}
