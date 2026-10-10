//! Building a project and running its testbenches.
//!
//! A build has two halves, kept apart so the first can be tested and shown
//! without running nvc:
//! 1. [`plan_build`]: find the files, scan them, work out the compile
//!    order and which files changed.
//! 2. [`run_build`]: analyse each file that needs it, one nvc process per
//!    file, then save the build state.
//!
//! Both report progress through a callback taking a [`BuildEvent`], so the
//! CLI can print it and the GUI can forward it to its window.

use std::collections::{BTreeMap, HashSet};
use std::path::{Path, PathBuf};

use nvc::{CancelToken, Library, Nvc, NvcInstall, NvcOptions, OutputItem, ProcessResult, RunOptions, Step, StepReport};

use crate::config::{ProjectConfig, ProjectError, TestbenchConfig};
use crate::files::{SourceFile, find_source_files};
use crate::order::{build_graph, compile_order};
use crate::scanner::{DesignUnit, UnitKind, scan};
use crate::state::{BUILD_STATE_FILE_NAME, BuildSettings, BuildState, FileState, files_to_compile, fingerprint, state_key};

/// A loaded project: its settings and where it lives.
#[derive(Debug, Clone, PartialEq)]
pub struct Project {
    /// The folder containing the project file. All relative paths in the
    /// project are relative to this, and nvc runs here.
    pub root: PathBuf,
    /// The project file itself.
    pub file: PathBuf,
    pub config: ProjectConfig,
}

impl Project {
    /// Loads a project from its `project.toml`, or from the folder holding it.
    pub fn load(path: &Path) -> Result<Project, ProjectError> {
        let file = if path.is_dir() {
            path.join(crate::PROJECT_FILE_NAME)
        } else {
            path.to_path_buf()
        };
        let file = std::path::absolute(&file).map_err(|error| ProjectError::Io {
            path: file.clone(),
            error,
        })?;
        let config = ProjectConfig::load(&file)?;
        let root = file.parent().unwrap_or(Path::new(".")).to_path_buf();
        Ok(Project { root, file, config })
    }

    /// Folder a library is compiled into, as nvc sees it (relative to
    /// `root` unless the output folder is absolute). Lower case, so it
    /// doesn't matter how the name is capitalised in the project file.
    pub fn library_dir(&self, library: &str) -> PathBuf {
        self.config.project.output_dir.join(library.to_lowercase())
    }

    /// Where the build state is saved.
    pub fn state_file(&self) -> PathBuf {
        self.root.join(&self.config.project.output_dir).join(BUILD_STATE_FILE_NAME)
    }

    /// An `Nvc` set up to compile into `library`, able to see every other
    /// project library that has been compiled so far, plus the external ones.
    pub fn nvc_for_library(&self, library: &str, install: &NvcInstall) -> Nvc {
        let work = Library {
            name: library.to_lowercase(),
            dir: self.library_dir(library),
        };
        let mut options = NvcOptions::new(self.config.project.standard, work);
        options.search_paths = self.config.external.search_paths.clone();
        options.extra_analyse_args = self.config.project.analyse_args.clone();

        // Only map folders that exist: nvc warns about mapped libraries
        // that aren't there yet (e.g. before the first build).
        for other in &self.config.libraries {
            let dir = self.library_dir(&other.name);
            if !other.name.eq_ignore_ascii_case(library) && self.root.join(&dir).is_dir() {
                options.library_map.push(Library {
                    name: other.name.to_lowercase(),
                    dir,
                });
            }
        }
        for (name, dir) in &self.config.external.map {
            options.library_map.push(Library {
                name: name.to_lowercase(),
                dir: dir.clone(),
            });
        }
        Nvc::new(install, options, &self.root)
    }
}

/// One file in a build plan.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlannedFile {
    pub source: SourceFile,
    /// Design units the file defines, for showing in the GUI.
    pub units: Vec<DesignUnit>,
    /// Positions in [`BuildPlan::files`] of the files this one needs.
    /// Always earlier positions.
    pub needs: Vec<usize>,
    pub fingerprint: String,
    /// False if the file is unchanged since it was last compiled.
    pub needs_compile: bool,
}

/// What a build will do, worked out before running anything.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BuildPlan {
    /// Every project file, in compile order.
    pub files: Vec<PlannedFile>,
    /// Problems that don't stop the build, e.g. a pattern matching nothing.
    pub warnings: Vec<String>,
    settings: BuildSettings,
    previous: BuildState,
}

impl BuildPlan {
    /// How many files will be compiled.
    pub fn compile_count(&self) -> usize {
        self.files.iter().filter(|file| file.needs_compile).count()
    }

    /// The library holding the entity (or configuration) called `name`.
    pub fn library_of(&self, name: &str) -> Option<&str> {
        let name = name.to_lowercase();
        self.files
            .iter()
            .find(|file| {
                file.units.iter().any(|unit| {
                    unit.name == name
                        && matches!(unit.kind, UnitKind::Entity | UnitKind::Configuration { .. })
                })
            })
            .map(|file| file.source.library.as_str())
    }
}

/// Works out what a build needs to do. Reads every source file but
/// doesn't run nvc.
///
/// `nvc_version` is part of the build settings: a different nvc rebuilds
/// everything. `force` ignores the saved state and compiles every file.
pub fn plan_build(project: &Project, nvc_version: &str, force: bool) -> Result<BuildPlan, ProjectError> {
    let file_list = find_source_files(&project.config, &project.root)?;
    let files = file_list.files;

    let mut scans = Vec::with_capacity(files.len());
    let mut fingerprints = Vec::with_capacity(files.len());
    for file in &files {
        let full_path = project.root.join(&file.path);
        let contents = std::fs::read(&full_path).map_err(|error| ProjectError::Io {
            path: full_path,
            error,
        })?;
        // Lossy, so a stray Latin-1 character in a comment can't stop us.
        scans.push(scan(&String::from_utf8_lossy(&contents)));
        fingerprints.push(fingerprint(&contents));
    }

    let graph = build_graph(&files, &scans);
    let order = compile_order(&graph, &files).map_err(ProjectError::Cycle)?;

    let settings = BuildSettings {
        nvc_version: nvc_version.to_string(),
        standard: project.config.project.standard.year().to_string(),
        analyse_args: project.config.project.analyse_args.clone(),
    };
    let previous = if force {
        BuildState::default()
    } else {
        BuildState::load(&project.state_file())
    };
    let missing_libraries: HashSet<String> = project
        .config
        .libraries
        .iter()
        .filter(|library| !project.root.join(project.library_dir(&library.name)).is_dir())
        .map(|library| library.name.to_lowercase())
        .collect();
    let needs_compile = files_to_compile(
        &files,
        &graph,
        &order,
        &fingerprints,
        &previous,
        &settings,
        &missing_libraries,
    );

    // Rearrange everything into compile order. `position[i]` is where the
    // file that was at index `i` ends up.
    let mut position = vec![0; files.len()];
    for (new_position, &index) in order.iter().enumerate() {
        position[index] = new_position;
    }
    let planned = order
        .iter()
        .map(|&index| PlannedFile {
            source: files[index].clone(),
            units: scans[index].units.clone(),
            needs: graph.dependencies[index].iter().map(|d| position[d.file]).collect(),
            fingerprint: fingerprints[index].clone(),
            needs_compile: needs_compile[index],
        })
        .collect();

    let mut warnings = file_list.warnings;
    warnings.extend(graph.warnings);
    Ok(BuildPlan {
        files: planned,
        warnings,
        settings,
        previous,
    })
}

/// Progress of a build or testbench run, as it happens.
#[derive(Debug, Clone)]
pub enum BuildEvent {
    /// About to analyse a file. `number` counts from 1 up to `total`, the
    /// number of files being compiled in this build.
    FileStarted {
        path: PathBuf,
        library: String,
        number: usize,
        total: usize,
    },
    FileFinished {
        path: PathBuf,
        process: ProcessResult,
        errors: usize,
    },
    /// Not compiled because a file it needs failed.
    FileSkipped { path: PathBuf, because: PathBuf },
    /// Elaborating or running a testbench.
    StepStarted { step: Step, top: String },
    StepFinished {
        step: Step,
        process: ProcessResult,
        errors: usize,
    },
    /// A message from nvc.
    Output(OutputItem),
}

/// How a build went.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct BuildSummary {
    pub compiled: usize,
    pub up_to_date: usize,
    pub failed: Vec<PathBuf>,
    pub skipped: usize,
    pub cancelled: bool,
}

impl BuildSummary {
    pub fn succeeded(&self) -> bool {
        self.failed.is_empty() && self.skipped == 0 && !self.cancelled
    }
}

/// What happened to one file during a build.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FileOutcome {
    UpToDate,
    Compiled,
    Failed,
    Skipped,
    Cancelled,
}

/// Compiles the files in `plan` that need it, in order.
///
/// When a file fails, files that need it are skipped, but independent
/// files are still compiled, so all real errors are reported in one go.
/// The build state is saved at the end, even if cancelled, so finished
/// work isn't repeated.
///
/// Returns an error only if nvc couldn't be started or the state couldn't
/// be saved.
pub fn run_build(
    project: &Project,
    plan: &BuildPlan,
    install: &NvcInstall,
    cancel: &CancelToken,
    on_event: &mut dyn FnMut(BuildEvent),
) -> Result<BuildSummary, ProjectError> {
    let mut new_state = BuildState {
        settings: plan.settings.clone(),
        files: BTreeMap::new(),
    };
    let mut outcomes: Vec<FileOutcome> = Vec::with_capacity(plan.files.len());
    let total = plan.compile_count();
    let mut number = 0;

    for file in &plan.files {
        let key = state_key(&file.source);
        let outcome = if !file.needs_compile {
            if let Some(old) = plan.previous.files.get(&key) {
                new_state.files.insert(key, old.clone());
            }
            FileOutcome::UpToDate
        } else if cancel.is_cancelled() {
            FileOutcome::Cancelled
        } else if let Some(&blocker) = file
            .needs
            .iter()
            .find(|&&needed| matches!(outcomes[needed], FileOutcome::Failed | FileOutcome::Skipped))
        {
            on_event(BuildEvent::FileSkipped {
                path: file.source.path.clone(),
                because: plan.files[blocker].source.path.clone(),
            });
            FileOutcome::Skipped
        } else {
            number += 1;
            on_event(BuildEvent::FileStarted {
                path: file.source.path.clone(),
                library: file.source.library.clone(),
                number,
                total,
            });
            let nvc = project.nvc_for_library(&file.source.library, install);
            let files = [file.source.path.clone()];
            let report = match nvc.analyse(&files, cancel, &mut |item| on_event(BuildEvent::Output(item.clone()))) {
                Ok(report) => report,
                Err(error) => {
                    // Keep what was done so far before giving up.
                    let _ = new_state.save(&project.state_file());
                    return Err(ProjectError::Io {
                        path: install.program.clone(),
                        error,
                    });
                }
            };
            on_event(BuildEvent::FileFinished {
                path: file.source.path.clone(),
                process: report.process,
                errors: report.error_count(),
            });

            if report.succeeded() {
                let state = FileState {
                    library: file.source.library.clone(),
                    fingerprint: file.fingerprint.clone(),
                };
                new_state.files.insert(key, state);
                FileOutcome::Compiled
            } else if report.process.cancelled {
                FileOutcome::Cancelled
            } else {
                FileOutcome::Failed
            }
        };
        outcomes.push(outcome);
    }

    let state_file = project.state_file();
    new_state
        .save(&state_file)
        .map_err(|error| ProjectError::Io { path: state_file, error })?;

    let count = |wanted: FileOutcome| outcomes.iter().filter(|&&outcome| outcome == wanted).count();
    Ok(BuildSummary {
        compiled: count(FileOutcome::Compiled),
        up_to_date: count(FileOutcome::UpToDate),
        failed: plan
            .files
            .iter()
            .zip(&outcomes)
            .filter(|(_, outcome)| **outcome == FileOutcome::Failed)
            .map(|(file, _)| file.source.path.clone())
            .collect(),
        skipped: count(FileOutcome::Skipped),
        cancelled: count(FileOutcome::Cancelled) > 0,
    })
}

/// How a testbench run went: the elaborate report, then the run report
/// if elaboration succeeded.
#[derive(Debug, Clone)]
pub struct TestbenchOutcome {
    pub reports: Vec<StepReport>,
}

impl TestbenchOutcome {
    /// True if both steps ran and succeeded.
    pub fn passed(&self) -> bool {
        self.reports.len() == 2 && self.reports.iter().all(StepReport::succeeded)
    }
}

/// Elaborates and runs a testbench. The project should have been built
/// first; `plan` is used to find which library the testbench is in.
pub fn run_testbench(
    project: &Project,
    plan: &BuildPlan,
    testbench: &TestbenchConfig,
    install: &NvcInstall,
    cancel: &CancelToken,
    on_event: &mut dyn FnMut(BuildEvent),
) -> Result<TestbenchOutcome, ProjectError> {
    let Some(library) = plan.library_of(&testbench.top) else {
        return Err(ProjectError::Invalid {
            path: project.file.clone(),
            problems: vec![format!(
                "testbench '{}': no entity called '{}' in any project library",
                testbench.display_name(),
                testbench.top
            )],
        });
    };
    let mut nvc = project.nvc_for_library(library, install);
    nvc.options.extra_elaborate_args = testbench.elaborate_args.clone();
    nvc.options.extra_run_args = testbench.run_args.clone();

    let top = testbench.top.to_lowercase();
    let generics: Vec<(String, String)> = testbench
        .generics
        .iter()
        .map(|(name, value)| (name.clone(), value.clone()))
        .collect();
    let run_options = RunOptions {
        stop_time: testbench.stop_time,
        wave_file: None, // waveforms arrive in Phase 3
    };
    let io_error = |error| ProjectError::Io {
        path: install.program.clone(),
        error,
    };

    let mut reports = Vec::new();
    for step in [Step::Elaborate, Step::Run] {
        on_event(BuildEvent::StepStarted { step, top: top.clone() });
        let mut forward = |item: &OutputItem| on_event(BuildEvent::Output(item.clone()));
        let report = match step {
            Step::Elaborate => nvc.elaborate(&top, &generics, cancel, &mut forward),
            _ => nvc.run(&top, &run_options, cancel, &mut forward),
        }
        .map_err(io_error)?;
        on_event(BuildEvent::StepFinished {
            step,
            process: report.process,
            errors: report.error_count(),
        });
        let succeeded = report.succeeded();
        reports.push(report);
        if !succeeded {
            break;
        }
    }
    Ok(TestbenchOutcome { reports })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::TempDir;

    /// A project on disk: files are (path, source) pairs.
    fn project_with(test_name: &str, toml: &str, files: &[(&str, &str)]) -> (TempDir, Project) {
        let dir = TempDir::new(test_name);
        dir.write("project.toml", toml);
        for (path, source) in files {
            dir.write(path, source);
        }
        let project = Project::load(dir.path()).unwrap();
        (dir, project)
    }

    const TWO_LIBRARIES: &str = r#"
        [project]
        name = "demo"
        [[library]]
        name = "Util"
        files = ["util/*.vhd"]
        [[library]]
        name = "design"
        files = ["src/*.vhd"]
    "#;

    fn two_library_project(test_name: &str) -> (TempDir, Project) {
        project_with(
            test_name,
            TWO_LIBRARIES,
            &[
                ("util/a_body.vhd", "package body pkg is end;"),
                ("util/b_pkg.vhd", "package pkg is end;"),
                ("src/top.vhd", "library util; use util.pkg.all; entity top is end;"),
            ],
        )
    }

    fn planned_paths(plan: &BuildPlan) -> Vec<String> {
        plan.files
            .iter()
            .map(|f| f.source.path.to_string_lossy().into_owned())
            .collect()
    }

    #[test]
    fn plan_orders_files_across_libraries() {
        let (_dir, project) = two_library_project("plan_orders_files_across_libraries");
        let plan = plan_build(&project, "1.23.0", false).unwrap();
        assert_eq!(planned_paths(&plan), ["util/b_pkg.vhd", "util/a_body.vhd", "src/top.vhd"]);
        assert_eq!(plan.files[1].needs, [0]);
        assert_eq!(plan.files[2].needs, [0]);
        assert_eq!(plan.compile_count(), 3);
        assert_eq!(plan.library_of("TOP"), Some("design"));
        assert_eq!(plan.library_of("pkg"), None, "packages aren't testbenches");
    }

    #[test]
    fn plan_uses_saved_state() {
        let (_dir, project) = two_library_project("plan_uses_saved_state");
        let plan = plan_build(&project, "1.23.0", false).unwrap();

        // Pretend a build succeeded: library folders exist, state saved.
        for library in ["util", "design"] {
            std::fs::create_dir_all(project.root.join(project.library_dir(library))).unwrap();
        }
        let mut state = BuildState {
            settings: plan.settings.clone(),
            files: BTreeMap::new(),
        };
        for file in &plan.files {
            let file_state = FileState {
                library: file.source.library.clone(),
                fingerprint: file.fingerprint.clone(),
            };
            state.files.insert(state_key(&file.source), file_state);
        }
        state.save(&project.state_file()).unwrap();

        assert_eq!(plan_build(&project, "1.23.0", false).unwrap().compile_count(), 0);
        assert_eq!(plan_build(&project, "1.23.0", true).unwrap().compile_count(), 3, "force");
        assert_eq!(plan_build(&project, "1.24.0", false).unwrap().compile_count(), 3, "new nvc");

        std::fs::write(project.root.join("util/a_body.vhd"), "package body pkg is -- edited\nend;").unwrap();
        let plan = plan_build(&project, "1.23.0", false).unwrap();
        let compiled: Vec<bool> = plan.files.iter().map(|f| f.needs_compile).collect();
        assert_eq!(compiled, [false, true, false], "only the edited body");
    }

    #[test]
    fn plan_reports_a_cycle() {
        let (_dir, project) = project_with(
            "plan_reports_a_cycle",
            "[project]\nname = \"x\"\n[[library]]\nname = \"work\"\nfiles = [\"*.vhd\"]\n",
            &[
                ("a.vhd", "use work.b.all; package a is end;"),
                ("b.vhd", "use work.a.all; package b is end;"),
            ],
        );
        let error = plan_build(&project, "1.23.0", false).unwrap_err();
        assert_eq!(
            error.to_string(),
            "circular dependency: a.vhd (uses b) → b.vhd (uses a) → a.vhd"
        );
    }

    #[test]
    fn nvc_options_for_a_library() {
        let (_dir, mut project) = two_library_project("nvc_options_for_a_library");
        project.config.project.analyse_args = vec!["--relaxed".to_string()];
        project.config.external.search_paths = vec![PathBuf::from("C:/libs")];
        project.config.external.map.insert("unisim".to_string(), PathBuf::from("C:/unisim"));
        let install = NvcInstall {
            program: PathBuf::from("nvc"),
            version: nvc::MINIMUM_VERSION,
        };

        // Before anything is compiled, the other library isn't mapped.
        let nvc = project.nvc_for_library("design", &install);
        assert_eq!(nvc.options.work.name, "design");
        assert_eq!(nvc.options.work.dir, Path::new("build").join("design"));
        assert_eq!(nvc.options.extra_analyse_args, ["--relaxed"]);
        assert_eq!(nvc.options.search_paths, [PathBuf::from("C:/libs")]);
        let mapped: Vec<&str> = nvc.options.library_map.iter().map(|l| l.name.as_str()).collect();
        assert_eq!(mapped, ["unisim"]);
        assert_eq!(nvc.working_dir, project.root);

        // Once util's folder exists, it is mapped (in lower case).
        std::fs::create_dir_all(project.root.join("build/util")).unwrap();
        let nvc = project.nvc_for_library("design", &install);
        let mapped: Vec<&str> = nvc.options.library_map.iter().map(|l| l.name.as_str()).collect();
        assert_eq!(mapped, ["util", "unisim"]);
    }

    #[test]
    fn missing_testbench_entity_is_explained() {
        let (_dir, project) = two_library_project("missing_testbench_entity_is_explained");
        let plan = plan_build(&project, "1.23.0", false).unwrap();
        let install = NvcInstall {
            program: PathBuf::from("nvc"),
            version: nvc::MINIMUM_VERSION,
        };
        let error = run_testbench(
            &project,
            &plan,
            &TestbenchConfig::new("no_such_tb"),
            &install,
            &CancelToken::new(),
            &mut |_| {},
        )
        .unwrap_err();
        assert!(error.to_string().contains("no entity called 'no_such_tb'"), "{error}");
    }

    #[test]
    fn summary_success() {
        assert!(BuildSummary::default().succeeded());
        let failed = BuildSummary {
            failed: vec![PathBuf::from("a.vhd")],
            ..BuildSummary::default()
        };
        assert!(!failed.succeeded());
        let cancelled = BuildSummary {
            cancelled: true,
            ..BuildSummary::default()
        };
        assert!(!cancelled.succeeded());
    }
}
