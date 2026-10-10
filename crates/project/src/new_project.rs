//! Creating a `project.toml` for a folder of existing VHDL files.

use std::path::Path;

use crate::build::Project;
use crate::config::{LibraryConfig, ProjectConfig, ProjectError, TestbenchConfig};
use crate::files::find_source_files;
use crate::scanner::{UnitKind, scan};
use crate::PROJECT_FILE_NAME;

/// File patterns a new project starts with: every VHDL file in the folder.
pub const DEFAULT_FILE_PATTERNS: [&str; 2] = ["**/*.vhd", "**/*.vhdl"];

/// Settings for a new project in `folder`: one library called `work`
/// holding every VHDL file, and a testbench for every entity whose name
/// looks like one (`*_tb` or `tb_*`). Doesn't write anything.
pub fn default_config_for_folder(folder: &Path) -> Result<ProjectConfig, ProjectError> {
    let name = folder
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| "project".to_string());
    let mut config = ProjectConfig::new(&name);
    config.libraries.push(LibraryConfig {
        name: "work".to_string(),
        files: DEFAULT_FILE_PATTERNS.iter().map(|p| p.to_string()).collect(),
        exclude: Vec::new(),
    });

    for file in find_source_files(&config, folder)?.files {
        // An unreadable file just doesn't contribute testbenches; the
        // first build will report it properly.
        let Ok(contents) = std::fs::read(folder.join(&file.path)) else {
            continue;
        };
        for unit in scan(&String::from_utf8_lossy(&contents)).units {
            let is_testbench_name = unit.name.ends_with("_tb") || unit.name.starts_with("tb_");
            let already_added = config.testbenches.iter().any(|tb| tb.top == unit.name);
            if unit.kind == UnitKind::Entity && is_testbench_name && !already_added {
                config.testbenches.push(TestbenchConfig::new(&unit.name));
            }
        }
    }
    Ok(config)
}

/// Writes a new `project.toml` in `folder` (see
/// [`default_config_for_folder`]) and loads it. Refuses to overwrite an
/// existing project file.
pub fn create_project(folder: &Path) -> Result<Project, ProjectError> {
    let file = folder.join(PROJECT_FILE_NAME);
    if file.exists() {
        return Err(ProjectError::Invalid {
            path: file,
            problems: vec!["a project file already exists in this folder; open it instead".to_string()],
        });
    }
    default_config_for_folder(folder)?.save(&file)?;
    Project::load(&file)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::TempDir;

    #[test]
    fn finds_files_and_testbenches() {
        let dir = TempDir::new("finds_files_and_testbenches");
        dir.write("src/counter.vhd", "entity counter is end; architecture a of counter is begin end;");
        dir.write("tb/counter_tb.vhd", "entity Counter_TB is end;");
        dir.write("tb/tb_uart.vhdl", "entity tb_uart is end; entity helper is end;");

        let config = default_config_for_folder(dir.path()).unwrap();
        assert_eq!(config.project.name, "finds_files_and_testbenches");
        assert_eq!(config.libraries[0].name, "work");
        let tops: Vec<&str> = config.testbenches.iter().map(|tb| tb.top.as_str()).collect();
        assert_eq!(tops, ["counter_tb", "tb_uart"]);
    }

    #[test]
    fn empty_folder_gives_a_project_without_testbenches() {
        let dir = TempDir::new("empty_folder_gives_a_project");
        let config = default_config_for_folder(dir.path()).unwrap();
        assert!(config.testbenches.is_empty());
        assert!(config.problems().is_empty());
    }

    #[test]
    fn creates_and_loads_but_never_overwrites() {
        let dir = TempDir::new("creates_and_loads_but_never_overwrites");
        dir.write("a_tb.vhd", "entity a_tb is end;");

        let project = create_project(dir.path()).unwrap();
        assert!(dir.path().join(PROJECT_FILE_NAME).is_file());
        assert_eq!(project.config.testbenches.len(), 1);

        let error = create_project(dir.path()).unwrap_err();
        assert!(error.to_string().contains("already exists"), "{error}");
    }
}
