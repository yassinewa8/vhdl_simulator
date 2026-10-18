//! Changing `project.toml` from the GUI.
//!
//! Edits go through `toml_edit`, which changes only the values touched and
//! keeps everything else (comments, blank lines, order) as the user wrote
//! it. Saving the whole [`ProjectConfig`](crate::ProjectConfig) instead
//! would throw the comments away.

use std::path::Path;

use toml_edit::{Array, ArrayOfTables, DocumentMut, Item, Table, Value, value};

use crate::config::ProjectError;

/// Takes `file` (a path relative to the project folder, as in a build
/// plan) out of `library` in the project file, and saves it. The file
/// itself stays on disk.
///
/// A file listed by name is taken out of `files`; a file matched by a
/// pattern such as `src/*.vhd` is added to the library's `exclude` list,
/// so the pattern keeps picking up new files.
pub fn remove_file(project_file: &Path, library: &str, file: &Path) -> Result<(), ProjectError> {
    edit_file(project_file, |document| remove_file_from(document, library, &file.to_string_lossy()))
}

/// Adds `files` (relative to the project folder where possible) to
/// `library` in the project file, and saves it. A file that was excluded
/// is taken off the `exclude` list; one that a `files` pattern doesn't
/// already match is added to `files` by name.
pub fn add_files(project_file: &Path, library: &str, files: &[String]) -> Result<(), ProjectError> {
    edit_file(project_file, |document| {
        files.iter().try_for_each(|file| add_file_to(document, library, file))
    })
}

/// Adds a `[[testbench]]` entry for each entity in `tops` that doesn't
/// have one yet.
pub fn add_testbenches(project_file: &Path, tops: &[String]) -> Result<(), ProjectError> {
    edit_file(project_file, |document| {
        add_testbenches_to(document, tops);
        Ok(())
    })
}

/// Removes the `[[testbench]]` entries whose `top` is one of `tops`.
pub fn remove_testbenches(project_file: &Path, tops: &[String]) -> Result<(), ProjectError> {
    edit_file(project_file, |document| {
        remove_testbenches_from(document, tops);
        Ok(())
    })
}

/// Reads the project file, lets `change` edit it, and writes it back.
fn edit_file(
    project_file: &Path,
    change: impl FnOnce(&mut DocumentMut) -> Result<(), String>,
) -> Result<(), ProjectError> {
    let io_error = |error| ProjectError::Io {
        path: project_file.to_path_buf(),
        error,
    };
    let invalid = |problem| ProjectError::Invalid {
        path: project_file.to_path_buf(),
        problems: vec![problem],
    };
    let text = std::fs::read_to_string(project_file).map_err(io_error)?;
    let mut document: DocumentMut = text
        .parse()
        .map_err(|error| invalid(format!("can't read the project file: {error}")))?;
    change(&mut document).map_err(invalid)?;
    std::fs::write(project_file, document.to_string()).map_err(io_error)
}

/// The `[[library]]` table called `library`.
fn library_entry<'a>(document: &'a mut DocumentMut, library: &str) -> Result<&'a mut Table, String> {
    document
        .get_mut("library")
        .and_then(Item::as_array_of_tables_mut)
        .and_then(|libraries| {
            libraries
                .iter_mut()
                .find(|table| table.get("name").and_then(Item::as_str) == Some(library))
        })
        .ok_or_else(|| format!("there is no library '{library}'"))
}

/// A list of strings in a library table, made empty if missing.
fn string_list<'a>(entry: &'a mut Table, key: &str, library: &str) -> Result<&'a mut Array, String> {
    if entry.get(key).is_none() {
        entry.insert(key, value(Array::new()));
    }
    entry
        .get_mut(key)
        .and_then(Item::as_array_mut)
        .ok_or_else(|| format!("library '{library}': '{key}' should be a list of files"))
}

/// Position of `file` in a list of paths, comparing paths, not text.
fn position_of(list: &Array, file: &str) -> Option<usize> {
    let wanted = normalise(file);
    list.iter().position(|item| item.as_str().is_some_and(|text| normalise(text) == wanted))
}

fn remove_file_from(document: &mut DocumentMut, library: &str, file: &str) -> Result<(), String> {
    let entry = library_entry(document, library)?;
    // Listed by name: take it out of `files`.
    let listed_at = entry.get("files").and_then(Item::as_array).and_then(|files| position_of(files, file));
    if let Some(position) = listed_at {
        if let Some(files) = entry.get_mut("files").and_then(Item::as_array_mut) {
            remove_keeping_comments(files, position);
        }
    } else {
        // Matched by a pattern: add it to `exclude`.
        let exclude = string_list(entry, "exclude", library)?;
        if position_of(exclude, file).is_none() {
            exclude.push(normalise(file));
        }
    }
    Ok(())
}

fn add_file_to(document: &mut DocumentMut, library: &str, file: &str) -> Result<(), String> {
    let entry = library_entry(document, library)?;
    // Excluded before (e.g. removed earlier): just stop excluding it.
    let excluded_at = entry.get("exclude").and_then(Item::as_array).and_then(|list| position_of(list, file));
    if let Some(position) = excluded_at
        && let Some(exclude) = entry.get_mut("exclude").and_then(Item::as_array_mut)
    {
        remove_keeping_comments(exclude, position);
        if exclude.is_empty() {
            entry.remove("exclude");
        }
    }
    // Then make sure `files` covers it.
    let path = normalise(file);
    let files = string_list(entry, "files", library)?;
    let covered = files
        .iter()
        .any(|item| item.as_str().is_some_and(|text| crate::files::entry_matches(text, &path)));
    if !covered {
        push_like_the_others(files, &path);
    }
    Ok(())
}

/// Appends to a list, laid out like the items already in it: in a list
/// written one item per line, the new item gets its own line too.
fn push_like_the_others(list: &mut Array, text: &str) {
    let leading = list
        .iter()
        .last()
        .and_then(|item| item.decor().prefix())
        .and_then(|prefix| prefix.as_str())
        .map(str::to_string);
    let mut item = Value::from(text);
    match leading {
        // One item per line: start a new line, indented the same.
        Some(leading) if leading.contains('\n') => item.decor_mut().set_prefix(leading),
        // All on one line: just a space after the comma.
        Some(_) => item.decor_mut().set_prefix(" "),
        None => {}
    }
    list.push_formatted(item);
}

fn add_testbenches_to(document: &mut DocumentMut, tops: &[String]) {
    if document.get("testbench").is_none() {
        document.insert("testbench", Item::ArrayOfTables(ArrayOfTables::new()));
    }
    let Some(testbenches) = document.get_mut("testbench").and_then(Item::as_array_of_tables_mut) else {
        return;
    };
    for top in tops {
        let exists = testbenches
            .iter()
            .any(|table| table.get("top").and_then(Item::as_str).is_some_and(|t| t.eq_ignore_ascii_case(top)));
        if !exists {
            let mut table = Table::new();
            table.insert("top", value(top.as_str()));
            // A blank line before it, as people write them.
            table.decor_mut().set_prefix("\n");
            testbenches.push(table);
        }
    }
}

fn remove_testbenches_from(document: &mut DocumentMut, tops: &[String]) {
    let Some(testbenches) = document.get_mut("testbench").and_then(Item::as_array_of_tables_mut) else {
        return;
    };
    testbenches.retain(|table| {
        let top = table.get("top").and_then(Item::as_str).unwrap_or("");
        !tops.iter().any(|gone| gone.eq_ignore_ascii_case(top))
    });
    if testbenches.is_empty() {
        document.remove("testbench");
    }
}

/// Removes item `position` from a list without losing comments.
///
/// toml_edit keeps the spaces and comments *before* an item with that
/// item. In
///
/// ```toml
/// files = [
///     "src/*.vhd",   # everything in src
///     "tb/top_tb.vhd",
/// ]
/// ```
///
/// the comment belongs to `"tb/top_tb.vhd"`, so removing that item would
/// also delete the comment about `src`. Handing the removed item's leading
/// text on to the next item (or to the end of the list) keeps it.
fn remove_keeping_comments(list: &mut Array, position: usize) {
    let leading = list
        .get(position)
        .and_then(|item| item.decor().prefix())
        .and_then(|prefix| prefix.as_str())
        .map(str::to_string);
    list.remove(position);
    let Some(leading) = leading else { return };
    if let Some(next) = list.get_mut(position) {
        next.decor_mut().set_prefix(leading);
    } else {
        // It was the last item: the text before it goes before the `]`
        // instead, without the line break and indentation that led to the
        // item (the end of the list has its own).
        let trailing = list.trailing().as_str().unwrap_or("").to_string();
        list.set_trailing(format!("{}{trailing}", leading.trim_end()));
    }
}

/// A path as text with `/` separators and no leading `./`, so the same
/// file written two ways compares equal.
pub(crate) fn normalise(path: &str) -> String {
    let path = path.replace('\\', "/");
    path.strip_prefix("./").unwrap_or(&path).to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::ProjectConfig;

    /// Applies one change to the text of a project file.
    fn apply(text: &str, change: impl FnOnce(&mut DocumentMut) -> Result<(), String>) -> Result<String, String> {
        let mut document: DocumentMut = text.parse().unwrap();
        change(&mut document)?;
        Ok(document.to_string())
    }

    fn remove_file_from_text(text: &str, library: &str, file: &str) -> Result<String, String> {
        apply(text, |document| remove_file_from(document, library, file))
    }

    fn add_file_to_text(text: &str, library: &str, file: &str) -> Result<String, String> {
        apply(text, |document| add_file_to(document, library, file))
    }

    const PROJECT: &str = "\
# My design. This comment must survive.

[project]
name = \"demo\"

[[library]]
name = \"work\"
files = [
    \"src/*.vhd\",   # everything in src
    \"tb/top_tb.vhd\",
    \"tb/other_tb.vhd\",
]

[[library]]
name = \"util\"
files = [\"util/*.vhd\"]
";

    #[test]
    fn a_file_listed_by_name_is_taken_out_of_files() {
        let changed = remove_file_from_text(PROJECT, "work", "tb/top_tb.vhd").unwrap();
        let config = ProjectConfig::from_toml(&changed, Path::new("project.toml")).unwrap();
        assert_eq!(config.libraries[0].files, ["src/*.vhd", "tb/other_tb.vhd"]);
        assert!(config.libraries[0].exclude.is_empty());
        assert!(changed.contains("# My design. This comment must survive."));
        assert!(changed.contains("# everything in src"));
    }

    #[test]
    fn removing_the_last_item_keeps_the_comment_before_it() {
        let text = "[[library]]
name = \"work\"
files = [
    \"a.vhd\",  # keep me
    \"b.vhd\",
]
";
        let changed = remove_file_from_text(text, "work", "b.vhd").unwrap();
        assert_eq!(changed, "[[library]]
name = \"work\"
files = [
    \"a.vhd\",  # keep me
]
");
    }

    #[test]
    fn a_file_matched_by_a_pattern_is_excluded() {
        let changed = remove_file_from_text(PROJECT, "util", "util/old.vhd").unwrap();
        // Removing it again doesn't list it twice.
        let changed = remove_file_from_text(&changed, "util", "./util\\old.vhd").unwrap();
        let config = ProjectConfig::from_toml(&changed, Path::new("project.toml")).unwrap();
        assert_eq!(config.libraries[1].files, ["util/*.vhd"]);
        assert_eq!(config.libraries[1].exclude, ["util/old.vhd"]);
        assert!(config.libraries[0].exclude.is_empty(), "other libraries untouched");
        assert!(changed.contains("# My design. This comment must survive."));
    }

    #[test]
    fn an_unknown_library_is_reported() {
        let error = remove_file_from_text(PROJECT, "nope", "a.vhd").unwrap_err();
        assert_eq!(error, "there is no library 'nope'");
    }

    #[test]
    fn adding_back_a_removed_file_restores_the_project_file() {
        let original = ProjectConfig::from_toml(PROJECT, Path::new("project.toml")).unwrap();
        for (library, file) in [("util", "util/old.vhd"), ("work", "tb/top_tb.vhd")] {
            let removed = remove_file_from_text(PROJECT, library, file).unwrap();
            let added = add_file_to_text(&removed, library, file).unwrap();
            let mut config = ProjectConfig::from_toml(&added, Path::new("project.toml")).unwrap();
            // A file listed by name comes back at the end of the list.
            for library in &mut config.libraries {
                library.files.sort();
            }
            let mut expected = original.clone();
            for library in &mut expected.libraries {
                library.files.sort();
            }
            assert_eq!(config, expected, "{file}");
        }
    }

    #[test]
    fn a_new_file_is_added_by_name_unless_a_pattern_covers_it() {
        let covered = add_file_to_text(PROJECT, "work", "src/new.vhd").unwrap();
        assert_eq!(covered, PROJECT, "src/*.vhd already matches it");

        let added = add_file_to_text(PROJECT, "work", "tb/new_tb.vhd").unwrap();
        let config = ProjectConfig::from_toml(&added, Path::new("project.toml")).unwrap();
        assert_eq!(config.libraries[0].files, ["src/*.vhd", "tb/top_tb.vhd", "tb/other_tb.vhd", "tb/new_tb.vhd"]);
        assert!(added.contains("    \"tb/other_tb.vhd\",\n    \"tb/new_tb.vhd\""), "on its own line:\n{added}");
        assert!(added.contains("# everything in src"));

        let one_line = add_file_to_text(PROJECT, "util", "extra/x.vhd").unwrap();
        assert!(one_line.contains("files = [\"util/*.vhd\", \"extra/x.vhd\"]"), "{one_line}");
    }

    #[test]
    fn testbench_entries_are_added_once_and_removed() {
        let added = apply(PROJECT, |document| {
            add_testbenches_to(document, &["top_tb".to_string(), "other_tb".to_string()]);
            add_testbenches_to(document, &["TOP_TB".to_string()]);
            Ok(())
        })
        .unwrap();
        let config = ProjectConfig::from_toml(&added, Path::new("project.toml")).unwrap();
        let tops: Vec<&str> = config.testbenches.iter().map(|tb| tb.top.as_str()).collect();
        assert_eq!(tops, ["top_tb", "other_tb"]);

        let removed = apply(&added, |document| {
            remove_testbenches_from(document, &["Top_TB".to_string()]);
            Ok(())
        })
        .unwrap();
        let config = ProjectConfig::from_toml(&removed, Path::new("project.toml")).unwrap();
        assert_eq!(config.testbenches.len(), 1);

        let none_left = apply(&removed, |document| {
            remove_testbenches_from(document, &["other_tb".to_string()]);
            Ok(())
        })
        .unwrap();
        assert!(!none_left.contains("[[testbench]]"));
        assert!(none_left.contains("# My design. This comment must survive."));
    }
}
