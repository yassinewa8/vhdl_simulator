//! Finding the nvc program and checking its version.

use std::fmt;
use std::path::{Path, PathBuf};
use std::process::Command;

/// The oldest nvc we support. Its output format is the one recorded in
/// `docs/nvc-cli-notes.md` and `tests/fixtures/nvc/`.
pub const MINIMUM_VERSION: Version = Version {
    major: 1,
    minor: 23,
    patch: 0,
};

/// A version number such as 1.23.0.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Version {
    pub major: u32,
    pub minor: u32,
    pub patch: u32,
}

impl Version {
    /// Reads the first line of `nvc --version`, e.g.
    /// `nvc 1.23.0 (1.23.0.r0.gcba10a2e0) (Using LLVM 22.1.8)`.
    /// Also copes with development builds such as `nvc 1.24-devel`.
    pub fn parse_nvc_output(output: &str) -> Option<Version> {
        let first_line = output.lines().next()?;
        let mut words = first_line.split_whitespace();
        if words.next()? != "nvc" {
            return None;
        }
        let mut parts = words.next()?.split('.');
        Some(Version {
            major: leading_number(parts.next()?)?,
            minor: leading_number(parts.next()?)?,
            patch: parts.next().and_then(leading_number).unwrap_or(0),
        })
    }
}

impl fmt::Display for Version {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}.{}.{}", self.major, self.minor, self.patch)
    }
}

/// The number at the start of `text`: `"24-devel"` gives 24.
fn leading_number(text: &str) -> Option<u32> {
    let end = text
        .find(|c: char| !c.is_ascii_digit())
        .unwrap_or(text.len());
    text[..end].parse().ok()
}

/// An nvc we found and checked.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NvcInstall {
    pub program: PathBuf,
    pub version: Version,
}

/// Why nvc couldn't be used.
#[derive(Debug)]
pub enum LocateError {
    /// No nvc in any of the places we looked.
    NotFound { searched: Vec<PathBuf> },
    /// Found a file, but running it failed.
    CannotRun {
        program: PathBuf,
        error: std::io::Error,
    },
    /// It ran, but `--version` didn't look like nvc.
    UnrecognisedVersion { program: PathBuf, output: String },
    /// nvc is older than [`MINIMUM_VERSION`].
    TooOld { program: PathBuf, found: Version },
}

const INSTALL_HINT: &str = "Install NVC from https://github.com/nickg/nvc/releases \
     (Windows: the .msi; Linux: the .deb for your Ubuntu version, or build from source), \
     then open a new terminal so the PATH is updated.";

impl fmt::Display for LocateError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            LocateError::NotFound { searched } => {
                writeln!(f, "could not find nvc. Looked in:")?;
                for path in searched {
                    writeln!(f, "  {}", path.display())?;
                }
                write!(f, "{INSTALL_HINT}")
            }
            LocateError::CannotRun { program, error } => {
                write!(f, "found {} but could not run it: {error}", program.display())
            }
            LocateError::UnrecognisedVersion { program, output } => write!(
                f,
                "{} doesn't look like nvc; `--version` printed: {}",
                program.display(),
                output.lines().next().unwrap_or("(nothing)")
            ),
            LocateError::TooOld { program, found } => write!(
                f,
                "{} is nvc {found}, but version {MINIMUM_VERSION} or newer is needed. {INSTALL_HINT}",
                program.display()
            ),
        }
    }
}

impl std::error::Error for LocateError {}

/// Finds nvc and checks its version.
///
/// Looks at `configured` first if given (e.g. a path from the user's
/// settings), otherwise searches PATH and then the standard install folder.
pub fn locate_nvc(configured: Option<&Path>) -> Result<NvcInstall, LocateError> {
    let candidates = match configured {
        Some(path) => vec![path.to_path_buf()],
        None => default_candidates(),
    };
    let Some(program) = candidates.iter().find(|path| path.is_file()) else {
        return Err(LocateError::NotFound {
            searched: candidates,
        });
    };
    check_version(program)
}

/// Runs `nvc --version` and checks it's new enough.
pub fn check_version(program: &Path) -> Result<NvcInstall, LocateError> {
    let output = Command::new(program)
        .arg("--version")
        .output()
        .map_err(|error| LocateError::CannotRun {
            program: program.to_path_buf(),
            error,
        })?;
    let text = String::from_utf8_lossy(&output.stdout).into_owned();

    let Some(version) = Version::parse_nvc_output(&text) else {
        return Err(LocateError::UnrecognisedVersion {
            program: program.to_path_buf(),
            output: text,
        });
    };
    if version < MINIMUM_VERSION {
        return Err(LocateError::TooOld {
            program: program.to_path_buf(),
            found: version,
        });
    }
    Ok(NvcInstall {
        program: program.to_path_buf(),
        version,
    })
}

/// Where to look when no path is configured: every PATH folder, then the
/// folder the Windows installer uses (in case PATH hasn't been refreshed).
fn default_candidates() -> Vec<PathBuf> {
    let file_name = format!("nvc{}", std::env::consts::EXE_SUFFIX);
    let path_var = std::env::var_os("PATH").unwrap_or_default();
    let mut candidates = candidates_on_path(&path_var, &file_name);

    if cfg!(windows)
        && let Some(program_files) = std::env::var_os("ProgramFiles")
    {
        candidates.push(PathBuf::from(program_files).join("NVC").join("bin").join(&file_name));
    }
    candidates
}

/// `file_name` inside each folder listed in a PATH-style variable.
fn candidates_on_path(path_var: &std::ffi::OsStr, file_name: &str) -> Vec<PathBuf> {
    std::env::split_paths(path_var)
        .filter(|dir| !dir.as_os_str().is_empty())
        .map(|dir| dir.join(file_name))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_release_version() {
        let output = "nvc 1.23.0 (1.23.0.r0.gcba10a2e0) (Using LLVM 22.1.8)\nCopyright (C) 2011-2026  Nick Gasson\n";
        assert_eq!(
            Version::parse_nvc_output(output),
            Some(Version {
                major: 1,
                minor: 23,
                patch: 0
            })
        );
    }

    #[test]
    fn parses_development_version() {
        assert_eq!(
            Version::parse_nvc_output("nvc 1.24-devel (Using LLVM 22)"),
            Some(Version {
                major: 1,
                minor: 24,
                patch: 0
            })
        );
    }

    #[test]
    fn rejects_other_programs() {
        assert_eq!(Version::parse_nvc_output("ghdl 4.0.0"), None);
        assert_eq!(Version::parse_nvc_output(""), None);
        assert_eq!(Version::parse_nvc_output("nvc"), None);
    }

    #[test]
    fn versions_compare_numerically() {
        let v = |major, minor, patch| Version {
            major,
            minor,
            patch,
        };
        assert!(v(1, 9, 0) < v(1, 23, 0));
        assert!(v(1, 23, 1) > MINIMUM_VERSION);
        assert!(v(2, 0, 0) > MINIMUM_VERSION);
        assert!(v(1, 22, 9) < MINIMUM_VERSION);
    }

    #[test]
    fn missing_configured_path_is_not_found() {
        let missing = Path::new("definitely/not/here/nvc.exe");
        let error = locate_nvc(Some(missing)).unwrap_err();
        assert!(matches!(error, LocateError::NotFound { .. }));
        let message = error.to_string();
        assert!(message.contains("definitely"));
        assert!(message.contains("github.com/nickg/nvc"));
    }

    #[test]
    fn builds_one_candidate_per_path_folder() {
        let path_var = std::env::join_paths(["dir_a", "dir_b"]).unwrap();
        let candidates = candidates_on_path(&path_var, "nvc");
        assert_eq!(
            candidates,
            [Path::new("dir_a").join("nvc"), Path::new("dir_b").join("nvc")]
        );
    }
}
