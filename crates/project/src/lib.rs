//! Project files and build orchestration.
//!
//! Planned for Phase 2: loading/saving `project.toml`, working out compile
//! order, incremental rebuilds, and installing libraries via `nvc --install`.

/// File name of a project file at the root of a user's VHDL project.
pub const PROJECT_FILE_NAME: &str = "project.toml";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn project_file_name_is_toml() {
        assert!(PROJECT_FILE_NAME.ends_with(".toml"));
    }
}
