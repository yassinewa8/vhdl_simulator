//! Remembering what was compiled last time, for incremental builds.
//!
//! After a build, `<output_dir>/build-state.toml` records a fingerprint of
//! each successfully compiled file's contents. Next time, a file is
//! recompiled if its fingerprint changed, and so is everything downstream
//! of it, like re-running synthesis only for the modules whose inputs moved.
//! Contents are compared rather than dates, so touching a file or switching
//! git branches back and forth doesn't cause needless rebuilds.

use std::collections::{BTreeMap, HashSet};
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::files::SourceFile;
use crate::order::DependencyGraph;

/// File name of the build state, inside the output folder.
pub const BUILD_STATE_FILE_NAME: &str = "build-state.toml";

/// Settings that affect every compiled file. If any of them changes,
/// everything is rebuilt.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct BuildSettings {
    /// A new nvc may store libraries differently.
    pub nvc_version: String,
    pub standard: String,
    pub analyse_args: Vec<String>,
}

/// What a file looked like when it was last compiled successfully.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileState {
    pub library: String,
    pub fingerprint: String,
}

/// Everything in `build-state.toml`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct BuildState {
    pub settings: BuildSettings,
    /// Keyed by the file's path as shown in the project (`src/a.vhd`).
    #[serde(default)]
    pub files: BTreeMap<String, FileState>,
}

impl BuildState {
    /// Reads the state file. A missing or unreadable file gives an empty
    /// state, which just means everything gets compiled.
    pub fn load(path: &Path) -> BuildState {
        std::fs::read_to_string(path)
            .ok()
            .and_then(|text| toml::from_str(&text).ok())
            .unwrap_or_default()
    }

    pub fn save(&self, path: &Path) -> std::io::Result<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let text = toml::to_string_pretty(self).expect("build state always converts to TOML");
        std::fs::write(path, text)
    }
}

/// The key a file is stored under in the build state.
pub fn state_key(file: &SourceFile) -> String {
    file.path.to_string_lossy().into_owned()
}

/// A short fingerprint of a file's contents (64-bit FNV-1a, as hex).
///
/// Written out here rather than using Rust's built-in hasher, because that
/// one may change between Rust versions, which would trigger a full rebuild.
/// This isn't for security, only for noticing that a file changed.
pub fn fingerprint(contents: &[u8]) -> String {
    const OFFSET_BASIS: u64 = 0xcbf2_9ce4_8422_2325;
    const PRIME: u64 = 0x0000_0100_0000_01b3;
    let mut hash = OFFSET_BASIS;
    for &byte in contents {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(PRIME);
    }
    format!("{hash:016x}")
}

/// Decides which files need compiling.
///
/// - `order`: compile order (indexes into `files`)
/// - `fingerprints`: each file's current fingerprint, same indexes as `files`
/// - `missing_libraries`: libraries (lower case) whose output folder is
///   gone, e.g. because the user deleted `build/`
///
/// A file needs compiling if it's new, changed, moved to another library,
/// its library folder is missing, the build settings changed, or it needs
/// (directly or indirectly) a file that is being compiled.
pub fn files_to_compile(
    files: &[SourceFile],
    graph: &DependencyGraph,
    order: &[usize],
    fingerprints: &[String],
    previous: &BuildState,
    settings: &BuildSettings,
    missing_libraries: &HashSet<String>,
) -> Vec<bool> {
    let settings_changed = previous.settings != *settings;
    let mut needs_compile = vec![false; files.len()];

    // Going in compile order means a file's dependencies have already been
    // decided by the time we reach it, so one pass is enough.
    for &index in order {
        let file = &files[index];
        let changed = match previous.files.get(&state_key(file)) {
            None => true,
            Some(old) => old.library != file.library || old.fingerprint != fingerprints[index],
        };
        let library_missing = missing_libraries.contains(&file.library.to_lowercase());
        let dependency_recompiled = graph.dependencies[index]
            .iter()
            .any(|dependency| needs_compile[dependency.file]);

        needs_compile[index] = settings_changed || changed || library_missing || dependency_recompiled;
    }
    needs_compile
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::order::{build_graph, compile_order};
    use crate::scanner::scan;
    use crate::test_support::TempDir;
    use std::path::PathBuf;

    /// A little design: pkg <- adder <- top, and an unrelated `other`.
    struct Design {
        files: Vec<SourceFile>,
        graph: DependencyGraph,
        order: Vec<usize>,
        fingerprints: Vec<String>,
    }

    fn design() -> Design {
        let sources = [
            ("pkg.vhd", "package pkg is end;"),
            ("adder.vhd", "use work.pkg.all; entity adder is end;"),
            ("top.vhd", "architecture a of top is begin u : adder port map (x); end;"),
            ("other.vhd", "entity other is end;"),
        ];
        let files: Vec<SourceFile> = sources
            .iter()
            .map(|(path, _)| SourceFile {
                library: "work".to_string(),
                path: PathBuf::from(path),
            })
            .collect();
        let scans: Vec<_> = sources.iter().map(|(_, source)| scan(source)).collect();
        let graph = build_graph(&files, &scans);
        let order = compile_order(&graph, &files).unwrap();
        let fingerprints = sources.iter().map(|(_, s)| fingerprint(s.as_bytes())).collect();
        Design {
            files,
            graph,
            order,
            fingerprints,
        }
    }

    fn settings() -> BuildSettings {
        BuildSettings {
            nvc_version: "1.23.0".to_string(),
            standard: "2008".to_string(),
            analyse_args: vec![],
        }
    }

    /// The state as if everything in `design` had just been compiled.
    fn all_compiled(design: &Design) -> BuildState {
        let files = design
            .files
            .iter()
            .zip(&design.fingerprints)
            .map(|(file, fingerprint)| {
                let state = FileState {
                    library: file.library.clone(),
                    fingerprint: fingerprint.clone(),
                };
                (state_key(file), state)
            })
            .collect();
        BuildState {
            settings: settings(),
            files,
        }
    }

    fn decide(design: &Design, previous: &BuildState, settings: &BuildSettings) -> Vec<bool> {
        files_to_compile(
            &design.files,
            &design.graph,
            &design.order,
            &design.fingerprints,
            previous,
            settings,
            &HashSet::new(),
        )
    }

    #[test]
    fn first_build_compiles_everything() {
        let design = design();
        assert_eq!(decide(&design, &BuildState::default(), &settings()), [true; 4]);
    }

    #[test]
    fn nothing_changed_compiles_nothing() {
        let design = design();
        let state = all_compiled(&design);
        assert_eq!(decide(&design, &state, &settings()), [false; 4]);
    }

    #[test]
    fn changed_file_recompiles_everything_downstream() {
        let mut design = design();
        let state = all_compiled(&design);
        design.fingerprints[0] = fingerprint(b"package pkg is constant c : bit := '1'; end;");
        // pkg changed, so adder (uses pkg) and top (uses adder) follow.
        assert_eq!(decide(&design, &state, &settings()), [true, true, true, false]);
    }

    #[test]
    fn changed_leaf_file_recompiles_only_itself() {
        let mut design = design();
        let state = all_compiled(&design);
        design.fingerprints[2] = fingerprint(b"changed top");
        assert_eq!(decide(&design, &state, &settings()), [false, false, true, false]);
    }

    #[test]
    fn new_settings_rebuild_everything() {
        let design = design();
        let state = all_compiled(&design);
        let mut new_settings = settings();
        new_settings.analyse_args = vec!["--relaxed".to_string()];
        assert_eq!(decide(&design, &state, &new_settings), [true; 4]);
    }

    #[test]
    fn file_moved_to_another_library_is_recompiled() {
        let mut design = design();
        let state = all_compiled(&design);
        design.files[3].library = "other_lib".to_string();
        assert_eq!(decide(&design, &state, &settings()), [false, false, false, true]);
    }

    #[test]
    fn missing_library_folder_recompiles_its_files() {
        let design = design();
        let state = all_compiled(&design);
        let missing = HashSet::from(["work".to_string()]);
        let decision = files_to_compile(
            &design.files,
            &design.graph,
            &design.order,
            &design.fingerprints,
            &state,
            &settings(),
            &missing,
        );
        assert_eq!(decision, [true; 4]);
    }

    #[test]
    fn fingerprint_is_stable_and_sensitive() {
        // Known FNV-1a 64 values, so the format can't drift unnoticed.
        assert_eq!(fingerprint(b""), "cbf29ce484222325");
        assert_eq!(fingerprint(b"a"), "af63dc4c8601ec8c");
        assert_ne!(fingerprint(b"entity a"), fingerprint(b"entity b"));
    }

    #[test]
    fn state_file_round_trip_and_missing_file() {
        let dir = TempDir::new("state_file_round_trip");
        let path = dir.path().join("build").join(BUILD_STATE_FILE_NAME);
        assert_eq!(BuildState::load(&path), BuildState::default());

        let state = all_compiled(&design());
        state.save(&path).unwrap();
        assert_eq!(BuildState::load(&path), state);
    }

    #[test]
    fn corrupt_state_file_means_rebuild() {
        let dir = TempDir::new("corrupt_state_file_means_rebuild");
        let path = dir.write(BUILD_STATE_FILE_NAME, "this is not [valid toml");
        assert_eq!(BuildState::load(&path), BuildState::default());
    }
}
