//! Project files and build orchestration.
//!
//! Modules:
//! - `config`: the `project.toml` file (load, save, check)
//! - `files`: expanding each library's file list and glob patterns
//! - `scanner`: finding what each VHDL file defines and uses, for compile order
//!
//! Still to come in Phase 2: compile order, incremental builds and
//! installing libraries with `nvc --install`.

mod config;
mod files;
mod scanner;

pub use config::{
    ExternalLibraries, LibraryConfig, ProjectConfig, ProjectError, ProjectSettings,
    TestbenchConfig, is_vhdl_identifier,
};
pub use files::{FileList, SourceFile, find_source_files};
pub use scanner::{DesignUnit, Reference, ScannedFile, UnitKind, scan};

/// File name of a project file at the root of a user's VHDL project.
pub const PROJECT_FILE_NAME: &str = "project.toml";

/// Helpers shared by the unit tests in this crate.
#[cfg(test)]
mod test_support {
    use std::path::{Path, PathBuf};

    /// A fresh, empty folder under the system temp folder for one test.
    /// Not deleted afterwards, so it can be inspected when a test fails;
    /// it's emptied again the next time that test runs.
    pub struct TempDir {
        path: PathBuf,
    }

    impl TempDir {
        pub fn new(test_name: &str) -> TempDir {
            let path = std::env::temp_dir().join("nvc-studio-tests").join(test_name);
            let _ = std::fs::remove_dir_all(&path);
            std::fs::create_dir_all(&path).unwrap();
            TempDir { path }
        }

        pub fn path(&self) -> &Path {
            &self.path
        }

        /// Writes `contents` to `relative`, creating folders as needed.
        pub fn write(&self, relative: &str, contents: &str) -> PathBuf {
            let file = self.path.join(relative);
            std::fs::create_dir_all(file.parent().unwrap()).unwrap();
            std::fs::write(&file, contents).unwrap();
            file
        }
    }
}
