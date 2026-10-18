//! The design hierarchy: a testbench on top, the entities it instantiates
//! below it, and so on down, worked out from the source files (no
//! simulation needed). Think of it as the block diagram's tree.

use std::path::PathBuf;

use crate::build::{BuildPlan, PlannedFile};
use crate::scanner::{Reference, UnitKind};

/// Deeper than this is surely a loop the checks below missed; stop.
const MAX_DEPTH: usize = 64;

/// One block in the tree.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HierarchyNode {
    /// The instance label, e.g. `u_add` (empty for the top).
    pub label: String,
    /// The entity (or component) name, lower case.
    pub entity: String,
    /// Where the entity was found in the project, if it was: its library
    /// and the file defining it. `None` for things outside the project,
    /// such as vendor primitives (`altsyncram`) or OSVVM components.
    pub found: Option<(String, PathBuf)>,
    /// For something outside the project instantiated as `entity lib.x`,
    /// the library it names.
    pub other_library: Option<String>,
    /// The entity also appears above this point (it instantiates itself,
    /// directly or not), so its children aren't shown again.
    pub recursive: bool,
    pub children: Vec<HierarchyNode>,
}

/// The tree below the entity `top` (usually a testbench), or `None` if no
/// project file defines `top`.
pub fn design_hierarchy(plan: &BuildPlan, top: &str) -> Option<HierarchyNode> {
    let top = top.to_lowercase();
    let (library, file) = find_entity(plan, &top, None)?;
    let mut node = HierarchyNode {
        label: String::new(),
        entity: top,
        found: Some((library, plan.files[file].source.path.clone())),
        other_library: None,
        recursive: false,
        children: Vec::new(),
    };
    add_children(plan, &mut node, &mut Vec::new());
    Some(node)
}

/// Fills in `node.children` from the instances in its entity's
/// architectures, then theirs. `above` holds the entities on the way down.
fn add_children(plan: &BuildPlan, node: &mut HierarchyNode, above: &mut Vec<(String, String)>) {
    let Some((library, _)) = &node.found else { return };
    let key = (library.to_lowercase(), node.entity.clone());
    if above.contains(&key) || above.len() >= MAX_DEPTH {
        node.recursive = true;
        return;
    }
    above.push(key);

    // Architectures of the entity are in its own library (in any file).
    let library = library.clone();
    for file in plan.files.iter().filter(|file| file.source.library.eq_ignore_ascii_case(&library)) {
        for instance in file.instances.iter().filter(|instance| instance.parent == node.entity) {
            let mut child = child_node(plan, &instance.label, &instance.target, &library);
            add_children(plan, &mut child, above);
            node.children.push(child);
        }
    }
    above.pop();
}

/// A node for one instance, before its own children are added.
fn child_node(plan: &BuildPlan, label: &str, target: &Reference, parent_library: &str) -> HierarchyNode {
    let (entity, wanted_library) = match target {
        // `work` means the library of the architecture doing the instantiating.
        Reference::Selected { library, unit } if library == "work" => (unit, Some(parent_library.to_string())),
        Reference::Selected { library, unit } => (unit, Some(library.clone())),
        Reference::Unqualified(name) => (name, None),
    };
    let found = find_entity(plan, entity, wanted_library.as_deref().or(Some(parent_library)))
        .or_else(|| match wanted_library {
            // A component: any library will do if its own doesn't have it.
            None => find_entity(plan, entity, None),
            Some(_) => None,
        })
        .map(|(library, file)| (library, plan.files[file].source.path.clone()));
    let other_library = match (&found, target) {
        (None, Reference::Selected { library, .. }) if library != "work" => Some(library.clone()),
        _ => None,
    };
    HierarchyNode {
        label: label.to_string(),
        entity: entity.clone(),
        found,
        other_library,
        recursive: false,
        children: Vec::new(),
    }
}

/// The library and file (index in the plan) defining entity `name`, in
/// `library` if given, otherwise in any library.
fn find_entity(plan: &BuildPlan, name: &str, library: Option<&str>) -> Option<(String, usize)> {
    let defines = |file: &PlannedFile| {
        file.units
            .iter()
            .any(|unit| unit.kind == UnitKind::Entity && unit.name == name)
    };
    plan.files
        .iter()
        .position(|file| library.is_none_or(|library| file.source.library.eq_ignore_ascii_case(library)) && defines(file))
        .map(|index| (plan.files[index].source.library.clone(), index))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::build::{Project, plan_build};
    use crate::test_support::TempDir;

    fn plan_for(test: &str, files: &[(&str, &str)]) -> (TempDir, BuildPlan) {
        let dir = TempDir::new(test);
        dir.write(
            "project.toml",
            r#"
            [project]
            name = "demo"
            [[library]]
            name = "work"
            files = ["src/*.vhd"]
            [[library]]
            name = "lib2"
            files = ["lib2/*.vhd"]
            "#,
        );
        for (path, source) in files {
            dir.write(path, source);
        }
        let project = Project::load(dir.path()).unwrap();
        let plan = plan_build(&project, "1.23.0", false).unwrap();
        (dir, plan)
    }

    /// The tree as indented text lines, easy to compare.
    fn lines(node: &HierarchyNode, depth: usize, out: &mut Vec<String>) {
        let place = match (&node.found, &node.other_library) {
            (Some((library, _)), _) => library.clone(),
            (None, Some(library)) => format!("{library}, not in project"),
            (None, None) => "not in project".to_string(),
        };
        let recursive = if node.recursive { " (recursive)" } else { "" };
        out.push(format!("{}{} : {} ({place}){recursive}", "  ".repeat(depth), node.label, node.entity));
        for child in &node.children {
            lines(child, depth + 1, out);
        }
    }

    fn tree(plan: &BuildPlan, top: &str) -> Vec<String> {
        let mut out = Vec::new();
        lines(&design_hierarchy(plan, top).unwrap(), 0, &mut out);
        out
    }

    #[test]
    fn testbench_on_top_and_what_it_instantiates_below() {
        let (_dir, plan) = plan_for(
            "hierarchy_basic",
            &[
                (
                    "src/top_tb.vhd",
                    "library lib2; entity top_tb is end;
                     architecture tb of top_tb is begin
                       dut : entity work.top port map (x);
                       u_check : checker port map (x);
                     end;",
                ),
                (
                    "src/top.vhd",
                    "entity top is end;
                     architecture rtl of top is begin
                       u_add : adder port map (a);
                       u_ram : altsyncram generic map (w => 8) port map (a);
                       u_fifo : entity vendor_lib.fifo port map (a);
                     end;",
                ),
                // An architecture in its own file still counts.
                ("src/adder.vhd", "entity adder is end;"),
                ("src/adder_rtl.vhd", "architecture rtl of adder is begin u_ha : half_adder port map (a); end;"),
                ("src/half_adder.vhd", "entity half_adder is end;"),
                // A component found in another library.
                ("lib2/checker.vhd", "entity checker is end;"),
            ],
        );
        assert_eq!(
            tree(&plan, "TOP_TB"),
            [
                " : top_tb (work)",
                "  dut : top (work)",
                "    u_add : adder (work)",
                "      u_ha : half_adder (work)",
                "    u_ram : altsyncram (not in project)",
                "    u_fifo : fifo (vendor_lib, not in project)",
                "  u_check : checker (lib2)",
            ]
        );
        assert!(design_hierarchy(&plan, "no_such_tb").is_none());
    }

    #[test]
    fn an_entity_instantiating_itself_stops_the_tree() {
        let (_dir, plan) = plan_for(
            "hierarchy_recursive",
            &[(
                "src/tree.vhd",
                "entity node is end;
                 architecture rtl of node is begin
                   g : if DEPTH > 0 generate
                     u_left : entity work.node port map (a);
                   end generate;
                 end;",
            )],
        );
        assert_eq!(tree(&plan, "node"), [" : node (work)", "  u_left : node (work) (recursive)"]);
    }
}
