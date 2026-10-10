//! Changing `project.toml` from the GUI.
//!
//! Edits go through `toml_edit`, which changes only the values touched and
//! keeps everything else (comments, blank lines, order) as the user wrote
//! it. Saving the whole [`ProjectConfig`](crate::ProjectConfig) instead
//! would throw the comments away.

use std::path::Path;

use toml_edit::{Array, DocumentMut, Item, value};

use crate::config::ProjectError;

/// Takes `file` (a path relative to the project folder, as in a build
/// plan) out of `library` in the project file, and saves it. The file
/// itself stays on disk.
///
/// A file listed by name is taken out of `files`; a file matched by a
/// pattern such as `src/*.vhd` is added to the library's `exclude` list,
/// so the pattern keeps picking up new files.
pub fn remove_file(project_file: &Path, library: &str, file: &Path) -> Result<(), ProjectError> {
    let io_error = |error| ProjectError::Io {
        path: project_file.to_path_buf(),
        error,
    };
    let text = std::fs::read_to_string(project_file).map_err(io_error)?;
    let changed = remove_file_from_text(&text, library, &file.to_string_lossy()).map_err(|problem| {
        ProjectError::Invalid {
            path: project_file.to_path_buf(),
            problems: vec![problem],
        }
    })?;
    std::fs::write(project_file, changed).map_err(io_error)
}

/// [`remove_file`] on the text of a project file: returns the new text.
fn remove_file_from_text(text: &str, library: &str, file: &str) -> Result<String, String> {
    let mut document: DocumentMut = text.parse().map_err(|error| format!("can't read the project file: {error}"))?;
    let entry = document
        .get_mut("library")
        .and_then(Item::as_array_of_tables_mut)
        .and_then(|libraries| {
            libraries
                .iter_mut()
                .find(|table| table.get("name").and_then(Item::as_str) == Some(library))
        })
        .ok_or_else(|| format!("there is no library '{library}'"))?;
    let wanted = normalise(file);
    let is_wanted = |item: &toml_edit::Value| item.as_str().is_some_and(|text| normalise(text) == wanted);

    // Listed by name: take it out of `files`.
    let listed_at = entry
        .get("files")
        .and_then(Item::as_array)
        .and_then(|files| files.iter().position(is_wanted));
    if let Some(position) = listed_at {
        if let Some(files) = entry.get_mut("files").and_then(Item::as_array_mut) {
            remove_keeping_comments(files, position);
        }
    } else {
        // Matched by a pattern: add it to `exclude` (made if missing).
        if entry.get("exclude").is_none() {
            entry.insert("exclude", value(Array::new()));
        }
        let exclude = entry
            .get_mut("exclude")
            .and_then(Item::as_array_mut)
            .ok_or_else(|| format!("library '{library}': 'exclude' should be a list of files"))?;
        if !exclude.iter().any(is_wanted) {
            exclude.push(wanted.as_str());
        }
    }
    Ok(document.to_string())
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
}
