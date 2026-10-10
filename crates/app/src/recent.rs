//! The "Recent projects" list in the File menu.
//!
//! Plain data with no GUI code, so it can be tested. The app saves it
//! between runs using eframe's storage.

use std::path::{Path, PathBuf};

/// How many projects the list remembers.
pub const MAX_RECENT_PROJECTS: usize = 8;

/// Project files, most recently opened first, no duplicates.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RecentProjects {
    paths: Vec<PathBuf>,
}

impl RecentProjects {
    /// Builds the list from saved paths, tidying it up in case the saved
    /// data was edited or came from an older version.
    pub fn from_paths(paths: Vec<PathBuf>) -> RecentProjects {
        let mut recent = RecentProjects::default();
        // Adding in reverse keeps the saved order, since `add` puts
        // each path at the front.
        for path in paths.into_iter().rev() {
            recent.add(&path);
        }
        recent
    }

    pub fn paths(&self) -> &[PathBuf] {
        &self.paths
    }

    /// Moves `path` to the top of the list, adding it if it's new.
    pub fn add(&mut self, path: &Path) {
        self.remove(path);
        self.paths.insert(0, path.to_path_buf());
        self.paths.truncate(MAX_RECENT_PROJECTS);
    }

    /// Forgets `path`, e.g. because it no longer exists.
    pub fn remove(&mut self, path: &Path) {
        self.paths.retain(|existing| !same_path(existing, path));
    }
}

/// Windows paths are case-insensitive, so `C:\Proj` and `c:\proj` are the
/// same project there.
fn same_path(a: &Path, b: &Path) -> bool {
    if cfg!(windows) {
        a.to_string_lossy().to_lowercase() == b.to_string_lossy().to_lowercase()
    } else {
        a == b
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names(recent: &RecentProjects) -> Vec<String> {
        recent.paths().iter().map(|p| p.to_string_lossy().into_owned()).collect()
    }

    #[test]
    fn newest_first_without_duplicates() {
        let mut recent = RecentProjects::default();
        recent.add(Path::new("a/project.toml"));
        recent.add(Path::new("b/project.toml"));
        recent.add(Path::new("a/project.toml"));
        assert_eq!(names(&recent), ["a/project.toml", "b/project.toml"]);
    }

    #[test]
    fn keeps_only_the_most_recent() {
        let mut recent = RecentProjects::default();
        for i in 0..MAX_RECENT_PROJECTS + 3 {
            recent.add(Path::new(&format!("p{i}")));
        }
        assert_eq!(recent.paths().len(), MAX_RECENT_PROJECTS);
        assert_eq!(recent.paths()[0], Path::new(&format!("p{}", MAX_RECENT_PROJECTS + 2)));
    }

    #[test]
    fn remove_forgets_a_project() {
        let mut recent = RecentProjects::from_paths(vec!["a".into(), "b".into()]);
        recent.remove(Path::new("a"));
        assert_eq!(names(&recent), ["b"]);
    }

    #[test]
    fn loading_keeps_saved_order_and_tidies_up() {
        let saved = vec!["x".into(), "y".into(), "x".into()];
        let recent = RecentProjects::from_paths(saved);
        assert_eq!(names(&recent), ["x", "y"]);
    }

    #[cfg(windows)]
    #[test]
    fn windows_paths_ignore_case() {
        let mut recent = RecentProjects::default();
        recent.add(Path::new("C:\\Proj\\project.toml"));
        recent.add(Path::new("c:\\proj\\project.toml"));
        assert_eq!(recent.paths().len(), 1);
    }
}
