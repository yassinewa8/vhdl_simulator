//! Turning each library's `files` list (paths and glob patterns) into the
//! actual source files on disk.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use crate::config::{ProjectConfig, ProjectError};

/// One VHDL source file and the library it's compiled into.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceFile {
    /// Library name exactly as written in the project file.
    pub library: String,
    /// Path relative to the project folder where possible (absolute if the
    /// file is outside it), so nvc messages show short paths.
    pub path: PathBuf,
}

/// The result of expanding every library's file list.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FileList {
    /// In the order the project file lists them: library by library, and
    /// within a library in pattern order (alphabetical within one pattern).
    pub files: Vec<SourceFile>,
    /// Things worth telling the user that don't stop a build, such as a
    /// pattern that matched no files.
    pub warnings: Vec<String>,
}

impl FileList {
    /// The files belonging to one library.
    pub fn in_library<'a>(&'a self, library: &'a str) -> impl Iterator<Item = &'a SourceFile> {
        self.files.iter().filter(move |file| file.library == library)
    }
}

/// Expands the `files` lists of every library in `config`. `root` is the
/// folder containing `project.toml`.
///
/// It's an error if a plain path (not a pattern) doesn't exist, or if the
/// same file is listed in two libraries.
pub fn find_source_files(config: &ProjectConfig, root: &Path) -> Result<FileList, ProjectError> {
    let mut list = FileList::default();
    let mut problems = Vec::new();
    // Which library each file was first found in, to catch duplicates.
    let mut owner: HashMap<PathBuf, String> = HashMap::new();

    for library in &config.libraries {
        for entry in &library.files {
            let matches = if is_glob_pattern(entry) {
                let matches = expand_pattern(root, entry, &mut problems);
                if matches.is_empty() {
                    list.warnings.push(format!(
                        "library '{}': pattern '{entry}' matched no files",
                        library.name
                    ));
                }
                matches
            } else if root.join(entry).is_file() {
                vec![PathBuf::from(entry)]
            } else {
                problems.push(format!("library '{}': file '{entry}' not found", library.name));
                Vec::new()
            };

            for path in matches {
                let path = relative_to(root, &path);
                match owner.get(&path) {
                    // Matched twice in one library (e.g. by two patterns): fine.
                    Some(existing) if *existing == library.name => {}
                    Some(existing) => problems.push(format!(
                        "'{}' is in both library '{existing}' and library '{}'",
                        path.display(),
                        library.name
                    )),
                    None => {
                        owner.insert(path.clone(), library.name.clone());
                        list.files.push(SourceFile {
                            library: library.name.clone(),
                            path,
                        });
                    }
                }
            }
        }
    }

    if problems.is_empty() {
        Ok(list)
    } else {
        Err(ProjectError::Invalid {
            path: root.join(crate::PROJECT_FILE_NAME),
            problems,
        })
    }
}

/// True if `entry` contains glob wildcards (`*`, `?` or `[`).
fn is_glob_pattern(entry: &str) -> bool {
    entry.contains(['*', '?', '['])
}

/// All files matching `pattern`, which is relative to `root` unless absolute.
fn expand_pattern(root: &Path, pattern: &str, problems: &mut Vec<String>) -> Vec<PathBuf> {
    // The root folder's own name might contain characters such as `[` that
    // glob would treat as wildcards, so escape that part.
    let full_pattern = if Path::new(pattern).is_absolute() {
        pattern.to_string()
    } else {
        let root = glob::Pattern::escape(&root.to_string_lossy());
        format!("{root}/{pattern}")
    };

    match glob::glob(&full_pattern) {
        Ok(paths) => paths
            .filter_map(Result::ok) // skip folders we aren't allowed to read
            .filter(|path| path.is_file())
            .collect(),
        Err(error) => {
            problems.push(format!("'{pattern}' is not a valid pattern: {error}"));
            Vec::new()
        }
    }
}

/// `path` relative to `root` if it's inside it, with `/` separators so
/// paths look the same on every platform.
fn relative_to(root: &Path, path: &Path) -> PathBuf {
    let relative = path.strip_prefix(root).unwrap_or(path);
    let text = relative.to_string_lossy().replace('\\', "/");
    PathBuf::from(text.trim_start_matches("./"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::LibraryConfig;
    use crate::test_support::TempDir;

    fn config_with(libraries: &[(&str, &[&str])]) -> ProjectConfig {
        let mut config = ProjectConfig::new("test");
        for (name, files) in libraries {
            config.libraries.push(LibraryConfig {
                name: name.to_string(),
                files: files.iter().map(|f| f.to_string()).collect(),
            });
        }
        config
    }

    fn paths(list: &FileList) -> Vec<(String, String)> {
        list.files
            .iter()
            .map(|f| (f.library.clone(), f.path.to_string_lossy().into_owned()))
            .collect()
    }

    #[test]
    fn expands_patterns_and_plain_paths() {
        let dir = TempDir::new("expands_patterns_and_plain_paths");
        dir.write("src/b.vhd", "");
        dir.write("src/a.vhd", "");
        dir.write("src/sub/c.vhd", "");
        dir.write("src/notes.txt", "");
        dir.write("tb/top_tb.vhd", "");

        let config = config_with(&[
            ("design", &["src/**/*.vhd"]),
            ("tb", &["tb/top_tb.vhd"]),
        ]);
        let list = find_source_files(&config, dir.path()).unwrap();
        assert_eq!(
            paths(&list),
            [
                ("design".into(), "src/a.vhd".into()),
                ("design".into(), "src/b.vhd".into()),
                ("design".into(), "src/sub/c.vhd".into()),
                ("tb".into(), "tb/top_tb.vhd".into()),
            ]
        );
        assert!(list.warnings.is_empty());
        assert_eq!(list.in_library("tb").count(), 1);
    }

    #[test]
    fn root_folder_with_brackets_is_not_a_pattern() {
        let dir = TempDir::new("root[with]brackets");
        dir.write("a.vhd", "");
        let config = config_with(&[("work", &["*.vhd"])]);
        let list = find_source_files(&config, dir.path()).unwrap();
        assert_eq!(paths(&list), [("work".into(), "a.vhd".into())]);
    }

    #[test]
    fn file_matched_twice_in_one_library_is_listed_once() {
        let dir = TempDir::new("file_matched_twice_in_one_library");
        dir.write("a.vhd", "");
        let config = config_with(&[("work", &["*.vhd", "a.vhd"])]);
        let list = find_source_files(&config, dir.path()).unwrap();
        assert_eq!(list.files.len(), 1);
    }

    #[test]
    fn empty_pattern_is_only_a_warning() {
        let dir = TempDir::new("empty_pattern_is_only_a_warning");
        let config = config_with(&[("work", &["src/*.vhd"])]);
        let list = find_source_files(&config, dir.path()).unwrap();
        assert!(list.files.is_empty());
        assert_eq!(list.warnings, ["library 'work': pattern 'src/*.vhd' matched no files"]);
    }

    #[test]
    fn missing_file_and_shared_file_are_errors() {
        let dir = TempDir::new("missing_file_and_shared_file_are_errors");
        dir.write("shared.vhd", "");
        let config = config_with(&[
            ("one", &["shared.vhd", "missing.vhd"]),
            ("two", &["*.vhd"]),
        ]);
        let Err(ProjectError::Invalid { problems, .. }) = find_source_files(&config, dir.path())
        else {
            panic!("expected errors");
        };
        assert_eq!(
            problems,
            [
                "library 'one': file 'missing.vhd' not found",
                "'shared.vhd' is in both library 'one' and library 'two'",
            ]
        );
    }
}
