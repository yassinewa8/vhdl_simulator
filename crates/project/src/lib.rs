//! Project files and build orchestration.
//!
//! Modules:
//! - `config`: the `project.toml` file (load, save, check)
//! - `files`: expanding each library's file list and glob patterns
//! - `scanner`: finding what each VHDL file defines and uses
//! - `order`: turning that into "who needs whom" and a compile order
//! - `state`: fingerprints of compiled files, for incremental builds
//! - `build`: planning and running a build, and running testbenches
//! - `new_project`: a starting `project.toml` for a folder of VHDL files
//! - `hierarchy`: the design tree, testbench on top (from the sources)
//! - `highlight`: splitting VHDL into coloured pieces for the editor
//! - `edit`: changing `project.toml` in place, keeping the user's comments
//!
//! Installing OSVVM/UVVM lives in the `nvc` crate (`nvc::install_package`),
//! since it runs scripts from the nvc installation.

mod build;
mod config;
mod edit;
mod files;
mod hierarchy;
mod highlight;
mod new_project;
mod order;
mod scanner;
mod state;

pub use config::{
    ExternalLibraries, LibraryConfig, ProjectConfig, ProjectError, ProjectSettings,
    TestbenchConfig, is_vhdl_identifier,
};
pub use build::{
    BuildEvent, BuildPlan, BuildSummary, PlannedFile, Project, TestbenchOutcome, plan_build,
    run_build, run_testbench,
};
pub use edit::{add_files, add_testbenches, remove_file, remove_testbenches};
pub use hierarchy::{HierarchyNode, design_hierarchy};
pub use highlight::{Span, TokenKind, highlight};
pub use files::{FileList, SourceFile, find_source_files};
pub use new_project::{DEFAULT_FILE_PATTERNS, create_project, default_config_for_folder, looks_like_testbench};
pub use order::{CycleError, Dependency, DependencyGraph, build_graph, compile_order};
pub use scanner::{DesignUnit, Instance, Reference, ScannedFile, UnitKind, scan};
pub use state::{BUILD_STATE_FILE_NAME, BuildSettings, BuildState, fingerprint};

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
