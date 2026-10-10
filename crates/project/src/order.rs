//! Working out which files depend on which, and the order to compile them.
//!
//! Think of each file as a block in a pipeline: it can't start until every
//! block feeding it has finished. Units defined by the scanner are matched
//! to the names other files refer to, giving "file A needs file B" edges.
//! A topological sort then gives an order where every file comes after
//! everything it needs.

use std::collections::{BTreeSet, HashMap};
use std::fmt;
use std::path::PathBuf;

use crate::files::SourceFile;
use crate::scanner::{Reference, ScannedFile};

/// One "this file needs that file" edge.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Dependency {
    /// Index of the file that must be compiled first.
    pub file: usize,
    /// The unit that file defines which caused the edge, for messages.
    pub unit: String,
}

/// Who needs whom, for a list of files.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DependencyGraph {
    /// For each file (same index as the input list), the other files it
    /// needs, at most one entry per needed file.
    pub dependencies: Vec<Vec<Dependency>>,
    /// e.g. the same unit defined in two files of one library.
    pub warnings: Vec<String>,
}

/// Matches each file's references to the units the other files define.
///
/// `files` and `scans` must be the same length, with matching indexes.
/// References to units outside the project (ieee, OSVVM, vendor libraries)
/// don't match anything and are ignored.
pub fn build_graph(files: &[SourceFile], scans: &[ScannedFile]) -> DependencyGraph {
    assert_eq!(files.len(), scans.len(), "one scan per file");
    let mut graph = DependencyGraph::default();

    // (library, unit) -> index of the file that defines it. Both lower case.
    let mut defined_in: HashMap<(String, String), usize> = HashMap::new();
    for (index, (file, scan)) in files.iter().zip(scans).enumerate() {
        let library = file.library.to_lowercase();
        for unit in scan.units.iter().filter(|unit| unit.kind.is_primary()) {
            let key = (library.clone(), unit.name.clone());
            match defined_in.get(&key) {
                Some(&first) if first != index => graph.warnings.push(format!(
                    "'{}' is defined in both {} and {} (library '{}'); using the first",
                    unit.name,
                    files[first].path.display(),
                    file.path.display(),
                    file.library
                )),
                Some(_) => {}
                None => {
                    defined_in.insert(key, index);
                }
            }
        }
    }

    let libraries: BTreeSet<String> = files.iter().map(|f| f.library.to_lowercase()).collect();

    for (index, (file, scan)) in files.iter().zip(scans).enumerate() {
        let own_library = file.library.to_lowercase();
        let mut needed: Vec<Dependency> = Vec::new();

        for reference in &scan.references {
            let (target, unit) = match reference {
                Reference::Selected { library, unit } => {
                    // `work` means "the library this file is compiled into".
                    let library = if library == "work" { &own_library } else { library };
                    let key = (library.clone(), unit.clone());
                    (defined_in.get(&key).copied(), unit)
                }
                Reference::Unqualified(unit) => (
                    find_unqualified(unit, &own_library, &libraries, &defined_in),
                    unit,
                ),
            };
            if let Some(target) = target
                && target != index
                && !needed.iter().any(|d| d.file == target)
            {
                needed.push(Dependency {
                    file: target,
                    unit: unit.clone(),
                });
            }
        }
        graph.dependencies.push(needed);
    }
    graph
}

/// Finds a bare name such as a component: in the file's own library if it
/// is there, otherwise in another library if exactly one has it. (If
/// several do, guessing could invent a false circular dependency.)
fn find_unqualified(
    unit: &str,
    own_library: &str,
    libraries: &BTreeSet<String>,
    defined_in: &HashMap<(String, String), usize>,
) -> Option<usize> {
    let in_library = |library: &str| defined_in.get(&(library.to_string(), unit.to_string())).copied();
    if let Some(index) = in_library(own_library) {
        return Some(index);
    }
    let mut others = libraries.iter().filter_map(|library| in_library(library));
    match (others.next(), others.next()) {
        (Some(index), None) => Some(index),
        _ => None,
    }
}

/// Files that need each other in a loop, so no order works.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CycleError {
    /// Each file in the loop with the unit it needs from the next one. The
    /// last file needs something from the first.
    pub steps: Vec<(PathBuf, String)>,
}

impl fmt::Display for CycleError {
    /// e.g. `circular dependency: a.vhd (uses pkg_b) → b.vhd (uses pkg_a) → a.vhd`
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "circular dependency: ")?;
        for (path, unit) in &self.steps {
            write!(f, "{} (uses {unit}) → ", path.display())?;
        }
        match self.steps.first() {
            Some((first, _)) => write!(f, "{}", first.display()),
            None => Ok(()),
        }
    }
}

impl std::error::Error for CycleError {}

/// The order to compile files in, as indexes into `files`.
///
/// Every file comes after the files it needs. When several files are ready
/// at once, the one listed first in the project goes first, so the result
/// is predictable and follows the user's own order where it can.
pub fn compile_order(graph: &DependencyGraph, files: &[SourceFile]) -> Result<Vec<usize>, CycleError> {
    let count = graph.dependencies.len();
    // How many unfinished files each file is still waiting for, and the
    // reverse edges: who is waiting for each file.
    let mut waiting_for: Vec<usize> = graph.dependencies.iter().map(Vec::len).collect();
    let mut needed_by: Vec<Vec<usize>> = vec![Vec::new(); count];
    for (index, dependencies) in graph.dependencies.iter().enumerate() {
        for dependency in dependencies {
            needed_by[dependency.file].push(index);
        }
    }

    // A sorted set, so the lowest-numbered ready file is always taken first.
    let mut ready: BTreeSet<usize> = (0..count).filter(|&i| waiting_for[i] == 0).collect();
    let mut order = Vec::with_capacity(count);
    while let Some(index) = ready.pop_first() {
        order.push(index);
        for &waiter in &needed_by[index] {
            waiting_for[waiter] -= 1;
            if waiting_for[waiter] == 0 {
                ready.insert(waiter);
            }
        }
    }

    if order.len() == count {
        Ok(order)
    } else {
        Err(find_cycle(graph, files, &waiting_for))
    }
}

/// Describes one loop among the files the sort couldn't place.
///
/// Every such file still waits for another unplaced file, so following
/// those edges from any of them must eventually come back round.
fn find_cycle(graph: &DependencyGraph, files: &[SourceFile], waiting_for: &[usize]) -> CycleError {
    let stuck = |index: usize| waiting_for[index] > 0;
    let start = (0..waiting_for.len()).find(|&i| stuck(i)).expect("some file is stuck");

    let mut path: Vec<(usize, &Dependency)> = Vec::new();
    let mut current = start;
    loop {
        if let Some(position) = path.iter().position(|(file, _)| *file == current) {
            // Back at a file we've seen: the loop is from there onwards.
            let steps = path[position..]
                .iter()
                .map(|(file, dependency)| (files[*file].path.clone(), dependency.unit.clone()))
                .collect();
            return CycleError { steps };
        }
        let next = graph.dependencies[current]
            .iter()
            .find(|dependency| stuck(dependency.file))
            .expect("a stuck file waits for another stuck file");
        path.push((current, next));
        current = next.file;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scanner::scan;

    /// Scans in-memory files given as (library, path, source).
    fn graph_of(sources: &[(&str, &str, &str)]) -> (Vec<SourceFile>, DependencyGraph) {
        let files: Vec<SourceFile> = sources
            .iter()
            .map(|(library, path, _)| SourceFile {
                library: library.to_string(),
                path: PathBuf::from(path),
            })
            .collect();
        let scans: Vec<ScannedFile> = sources.iter().map(|(_, _, source)| scan(source)).collect();
        let graph = build_graph(&files, &scans);
        (files, graph)
    }

    fn order_of(sources: &[(&str, &str, &str)]) -> Vec<String> {
        let (files, graph) = graph_of(sources);
        compile_order(&graph, &files)
            .unwrap()
            .into_iter()
            .map(|i| files[i].path.to_string_lossy().into_owned())
            .collect()
    }

    #[test]
    fn package_before_its_users_and_body() {
        let order = order_of(&[
            ("work", "body.vhd", "package body p is end;"),
            ("work", "user.vhd", "use work.p.all; entity e is end;"),
            ("work", "pkg.vhd", "package p is end;"),
        ]);
        assert_eq!(order, ["pkg.vhd", "body.vhd", "user.vhd"]);
    }

    #[test]
    fn independent_files_keep_their_listed_order() {
        let order = order_of(&[
            ("work", "c.vhd", "entity c is end;"),
            ("work", "a.vhd", "entity a is end;"),
            ("work", "b.vhd", "entity b is end;"),
        ]);
        assert_eq!(order, ["c.vhd", "a.vhd", "b.vhd"]);
    }

    #[test]
    fn entity_before_architecture_in_another_file() {
        let order = order_of(&[
            ("work", "rtl.vhd", "architecture rtl of counter is begin end;"),
            ("work", "ent.vhd", "entity counter is end;"),
        ]);
        assert_eq!(order, ["ent.vhd", "rtl.vhd"]);
    }

    #[test]
    fn instantiations_across_libraries() {
        let order = order_of(&[
            (
                "tb_lib",
                "tb.vhd",
                "library design; architecture tb of top_tb is begin
                   dut : entity design.top port map (clk);
                 end;",
            ),
            (
                "design",
                "top.vhd",
                "entity top is end; architecture a of top is begin
                   u : adder port map (x);
                 end;",
            ),
            ("design", "adder.vhd", "entity adder is end;"),
        ]);
        assert_eq!(order, ["adder.vhd", "top.vhd", "tb.vhd"]);
    }

    #[test]
    fn work_means_the_files_own_library() {
        // Both libraries have a `p`; work.p must pick the file's own one.
        let (_, graph) = graph_of(&[
            ("a", "a_p.vhd", "package p is end;"),
            ("b", "b_p.vhd", "package p is end;"),
            ("b", "b_user.vhd", "use work.p.all; entity e is end;"),
        ]);
        assert_eq!(
            graph.dependencies[2],
            [Dependency {
                file: 1,
                unit: "p".to_string()
            }]
        );
    }

    #[test]
    fn component_found_in_two_other_libraries_is_not_guessed() {
        let (_, graph) = graph_of(&[
            ("a", "a.vhd", "entity buf is end;"),
            ("b", "b.vhd", "entity buf is end;"),
            ("c", "c.vhd", "architecture x of top is begin u : buf port map (i); end;"),
        ]);
        assert!(graph.dependencies[2].is_empty());
    }

    #[test]
    fn outside_libraries_and_self_references_are_ignored() {
        let (_, graph) = graph_of(&[(
            "work",
            "all.vhd",
            "library ieee; use ieee.std_logic_1164.all;
             package p is end; package body p is end;
             entity e is end; architecture a of e is begin end;",
        )]);
        assert!(graph.dependencies[0].is_empty());
        assert!(graph.warnings.is_empty());
    }

    #[test]
    fn duplicate_unit_is_a_warning() {
        let (_, graph) = graph_of(&[
            ("work", "one.vhd", "package p is end;"),
            ("work", "two.vhd", "package p is end;"),
        ]);
        assert_eq!(
            graph.warnings,
            ["'p' is defined in both one.vhd and two.vhd (library 'work'); using the first"]
        );
    }

    #[test]
    fn circular_dependency_is_reported_as_a_chain() {
        let (files, graph) = graph_of(&[
            ("work", "start.vhd", "use work.pkg_a.all; entity e is end;"),
            ("work", "a.vhd", "use work.pkg_b.all; package pkg_a is end;"),
            ("work", "b.vhd", "use work.pkg_a.all; package pkg_b is end;"),
        ]);
        let error = compile_order(&graph, &files).unwrap_err();
        assert_eq!(
            error.to_string(),
            "circular dependency: a.vhd (uses pkg_b) → b.vhd (uses pkg_a) → a.vhd"
        );
    }

    #[test]
    fn empty_project_has_an_empty_order() {
        let (files, graph) = graph_of(&[]);
        assert_eq!(compile_order(&graph, &files).unwrap(), Vec::<usize>::new());
    }
}
