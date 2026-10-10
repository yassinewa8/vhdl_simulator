//! The application's state and the actions the window can trigger (open
//! a project, build, run, stop, install).
//!
//! No egui here: the window (`ui.rs`) reads this state to draw, and calls
//! these methods when buttons are clicked. That keeps the logic testable.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use nvc::{NvcInstall, default_library_dir, installed_libraries, locate_nvc};
use project::{BuildPlan, PlannedFile, Project, ProjectError, TestbenchConfig, create_project, plan_build};

use crate::recent::RecentProjects;
use crate::session::{LogStyle, Session};
use crate::wave_state::WaveState;
use crate::worker::{Job, JobContext, RunningJob, WorkerMessage};

/// Asks the window to redraw. Background work (builds, waveform loads)
/// calls it from other threads whenever it has news.
type Waker = Arc<dyn Fn() + Send + Sync>;

/// An open project and its latest build plan (file order, what's stale).
pub struct OpenProject {
    pub project: Project,
    /// `Err` holds a message, e.g. a circular dependency or a missing file.
    pub plan: Result<BuildPlan, String>,
}

/// Which view fills the main area.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MainTab {
    Output,
    Problems,
    Waves,
}

/// Everything the window shows, and what it's doing.
pub struct AppState {
    /// The nvc we found at start-up, or why we couldn't use one.
    pub nvc: Result<NvcInstall, String>,
    pub open: Option<OpenProject>,
    pub recent: RecentProjects,
    pub session: Session,
    pub job: Option<RunningJob>,
    /// Index into the open project's testbenches.
    pub selected_testbench: usize,
    pub selected_file: Option<PathBuf>,
    pub selected_problem: Option<usize>,
    pub tab: MainTab,
    /// Library folders in `~/.nvc/lib`, for the Libraries menu.
    pub installed: Vec<String>,
    /// The Waves tab.
    pub waves: WaveState,
    waker: Waker,
}

impl AppState {
    pub fn new(recent: RecentProjects) -> AppState {
        let nvc = locate_nvc(None).map_err(|error| error.to_string());
        let mut state = AppState {
            nvc,
            open: None,
            recent,
            session: Session::default(),
            job: None,
            selected_testbench: 0,
            selected_file: None,
            selected_problem: None,
            tab: MainTab::Output,
            installed: Vec::new(),
            waves: WaveState::default(),
            waker: Arc::new(|| {}),
        };
        state.refresh_installed();
        if let Err(message) = &state.nvc {
            let message = format!("nvc not found: {message}");
            state.session.note(&message, LogStyle::Error);
        }
        state
    }

    /// Sets how background work asks the window to redraw (the window
    /// passes egui's `request_repaint`).
    pub fn set_waker(&mut self, wake: impl Fn() + Send + Sync + 'static) {
        self.waker = Arc::new(wake);
    }

    fn wake_fn(&self) -> impl Fn() + Send + Sync + 'static {
        let waker = self.waker.clone();
        move || waker()
    }

    /// Opens a waveform file in the Waves tab.
    /// Opens a waveform file in the Waves tab. Its layout is kept next to
    /// it, as `<file>.layout.toml`.
    pub fn open_waveform(&mut self, path: &Path) {
        let layout_file = PathBuf::from(format!("{}.layout.toml", path.display()));
        let wake = self.wake_fn();
        self.waves.open_with_layout(path, Some(layout_file), None, wake);
        self.tab = MainTab::Waves;
    }

    /// Shows the waveform a run just wrote. If it's the one already shown,
    /// it's reloaded keeping the whole setup; otherwise it opens with the
    /// testbench's layout from `<project>/layouts/`.
    fn open_run_waveform(&mut self, path: &Path) {
        if self.waves.wave.is_some() && self.waves.path().as_deref() == Some(path) {
            let wake = self.wake_fn();
            self.waves.reload(wake);
        } else {
            let layout_file = self.open.as_ref().map(|open| {
                let name = path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
                open.project.root.join("layouts").join(format!("{name}.toml"))
            });
            let wake = self.wake_fn();
            self.waves.open_with_layout(path, layout_file, None, wake);
        }
        self.tab = MainTab::Waves;
    }

    /// Takes in everything that has arrived from background work. Call
    /// once per frame.
    pub fn poll(&mut self) {
        self.poll_job();
        self.waves.poll();
        let now = std::time::Instant::now();
        self.waves.autosave(now);
        // While nvc runs it may be rewriting the file; the finished run
        // reloads it anyway.
        if !self.is_busy() {
            let wake = self.wake_fn();
            self.waves.check_file(now, wake);
        }
    }

    /// Opens a project from its `project.toml` (or the folder holding it).
    /// Problems are written to the log.
    pub fn open_project(&mut self, path: &Path) {
        match Project::load(path) {
            Ok(project) => {
                self.recent.add(&project.file);
                let message = format!("Opened {}", project.file.display());
                self.open = Some(OpenProject {
                    plan: Err("not planned yet".to_string()),
                    project,
                });
                self.selected_testbench = 0;
                self.selected_file = None;
                self.selected_problem = None;
                self.session.problems.clear();
                self.refresh_plan();
                self.session.note(&message, LogStyle::Plain);
            }
            Err(error) => {
                // Forget projects that have gone, so the list stays useful.
                if !path.exists() {
                    self.recent.remove(path);
                }
                self.report_error(&error);
            }
        }
    }

    /// Creates a `project.toml` in `folder` and opens it. If there's one
    /// already, opens that instead.
    pub fn new_project(&mut self, folder: &Path) {
        let existing = folder.join(project::PROJECT_FILE_NAME);
        if existing.is_file() {
            self.session.note("This folder already has a project file; opening it.", LogStyle::Plain);
            self.open_project(&existing);
            return;
        }
        match create_project(folder) {
            Ok(project) => {
                let count = project.config.testbenches.len();
                self.session.note(
                    &format!("Created {} with {count} testbench(es) found automatically", project.file.display()),
                    LogStyle::Plain,
                );
                self.open_project(&project.file);
            }
            Err(error) => self.report_error(&error),
        }
    }

    pub fn close_project(&mut self) {
        self.open = None;
        self.selected_file = None;
        self.selected_problem = None;
        self.session.problems.clear();
    }

    /// Works out the build plan again, e.g. after a build or an edit.
    pub fn refresh_plan(&mut self) {
        let version = match &self.nvc {
            Ok(install) => install.version.to_string(),
            // Without nvc we can still show files and compile order.
            Err(_) => String::new(),
        };
        if let Some(open) = &mut self.open {
            // Pick up edits to project.toml too.
            if let Ok(reloaded) = Project::load(&open.project.file) {
                open.project = reloaded;
            }
            open.plan = plan_build(&open.project, &version, false).map_err(|error| error.to_string());
            let testbench_count = open.project.config.testbenches.len();
            if self.selected_testbench >= testbench_count {
                self.selected_testbench = 0;
            }
        }
    }

    pub fn refresh_installed(&mut self) {
        self.installed = default_library_dir()
            .map(|dir| installed_libraries(&dir))
            .unwrap_or_default();
    }

    /// The testbench chosen in the toolbar, if the project has any.
    pub fn selected_testbench(&self) -> Option<&TestbenchConfig> {
        let open = self.open.as_ref()?;
        open.project.config.testbenches.get(self.selected_testbench)
    }

    pub fn is_busy(&self) -> bool {
        self.job.is_some()
    }

    /// Starts a job in the background. `wake` asks the window to redraw;
    /// it's called from the worker thread whenever there's news.
    pub fn start_job(&mut self, job: Job) {
        if self.is_busy() {
            return; // the buttons are disabled; this is just a safeguard
        }
        let nvc = match &self.nvc {
            Ok(nvc) => nvc.clone(),
            Err(message) => {
                self.session.note(&format!("Can't start: nvc not found. {message}"), LogStyle::Error);
                return;
            }
        };
        let context = JobContext {
            project: self.open.as_ref().map(|open| open.project.clone()),
            nvc,
        };
        self.session.begin_job(&job.description());
        self.selected_problem = None;
        self.tab = MainTab::Output;
        self.job = Some(RunningJob::start(job, context, self.wake_fn()));
    }

    pub fn stop_job(&self) {
        if let Some(job) = &self.job {
            job.stop();
        }
    }

    /// Takes in whatever the worker has sent. Call once per frame.
    fn poll_job(&mut self) {
        let Some(job) = &self.job else { return };
        let mut finished = None;
        for message in job.take_messages() {
            if let WorkerMessage::Finished(outcome) = &message {
                finished = Some(outcome.clone());
            }
            self.session.handle(message);
        }
        let Some(outcome) = finished else { return };
        self.job = None;
        self.refresh_plan();
        self.refresh_installed();
        if self.session.error_count() > 0 {
            // After a failed build or run, show the errors straight away.
            self.tab = MainTab::Problems;
        } else if let (true, Some(wave_file)) = (outcome.succeeded, &outcome.wave_file) {
            // After a successful run, show its waveform.
            self.open_run_waveform(wave_file);
        }
    }

    /// Selects a problem, and the file it's in if that's a project file.
    pub fn select_problem(&mut self, index: usize) {
        self.selected_problem = Some(index);
        let file = self.session.problems.get(index).and_then(|p| p.location.as_ref()).map(|l| l.file.clone());
        if let Some(file) = file
            && self.planned_file(&file).is_some()
        {
            self.selected_file = Some(file);
        }
    }

    /// The project file at `path` (as nvc printed it), if there is one.
    pub fn planned_file(&self, path: &Path) -> Option<&PlannedFile> {
        let plan = self.open.as_ref()?.plan.as_ref().ok()?;
        plan.files.iter().find(|file| same_file(&file.source.path, path))
    }

    fn report_error(&mut self, error: &ProjectError) {
        for line in error.to_string().lines() {
            self.session.note(line, LogStyle::Error);
        }
        self.tab = MainTab::Output;
    }
}

/// Project files grouped by library, in the order the project file lists
/// the libraries. Each group's files are in compile order.
pub fn files_by_library<'a>(project: &'a Project, plan: &'a BuildPlan) -> Vec<(&'a str, Vec<&'a PlannedFile>)> {
    project
        .config
        .libraries
        .iter()
        .map(|library| {
            let files = plan.files.iter().filter(|f| f.source.library == library.name).collect();
            (library.name.as_str(), files)
        })
        .collect()
}

/// True if two paths name the same project file, ignoring `/` vs `\`
/// (nvc prints `/`; Windows paths may use either).
pub fn same_file(a: &Path, b: &Path) -> bool {
    let normalise = |path: &Path| path.to_string_lossy().replace('\\', "/");
    normalise(a) == normalise(b)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc;
    use std::time::Duration;

    /// A copy of a test project from tests/vhdl/projects in a scratch folder.
    fn copy_of_project(name: &str, test_name: &str) -> PathBuf {
        let source = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/vhdl/projects").join(name);
        let target = std::env::temp_dir().join("nvc-studio-tests").join(test_name);
        let _ = std::fs::remove_dir_all(&target);
        copy_folder(&source, &target);
        target
    }

    fn copy_folder(from: &Path, to: &Path) {
        std::fs::create_dir_all(to).unwrap();
        for entry in std::fs::read_dir(from).unwrap() {
            let entry = entry.unwrap();
            if entry.file_type().unwrap().is_dir() {
                copy_folder(&entry.path(), &to.join(entry.file_name()));
            } else {
                std::fs::copy(entry.path(), to.join(entry.file_name())).unwrap();
            }
        }
    }

    /// Runs a job the way the window does: start it, then poll whenever
    /// woken until it's done.
    fn run_to_completion(state: &mut AppState, job: Job) {
        let (wake_sender, woken) = mpsc::channel();
        state.set_waker(move || {
            let _ = wake_sender.send(());
        });
        state.start_job(job);
        while state.is_busy() || state.waves.is_opening() {
            woken.recv_timeout(Duration::from_secs(60)).expect("job took too long");
            state.poll();
        }
    }

    fn nvc_available(state: &AppState) -> bool {
        match &state.nvc {
            Ok(_) => true,
            Err(_) if std::env::var_os("NVC_REQUIRED").is_some() => panic!("NVC_REQUIRED but nvc not found"),
            Err(message) => {
                eprintln!("skipping: {message}");
                false
            }
        }
    }

    #[test]
    fn same_file_ignores_slash_direction() {
        assert!(same_file(Path::new("src/a.vhd"), Path::new("src\\a.vhd")));
        assert!(!same_file(Path::new("src/a.vhd"), Path::new("src/b.vhd")));
    }

    #[test]
    fn opening_a_project_plans_it_and_remembers_it() {
        let folder = copy_of_project("three_libs", "app_opening_a_project");
        let mut state = AppState::new(RecentProjects::default());
        state.open_project(&folder);

        let open = state.open.as_ref().expect("project should open");
        let plan = open.plan.as_ref().expect("plan should work");
        assert_eq!(plan.files.len(), 6);
        assert_eq!(state.recent.paths(), std::slice::from_ref(&open.project.file));
        assert_eq!(state.selected_testbench().unwrap().top, "top_tb");

        let groups = files_by_library(&open.project, plan);
        let names: Vec<(&str, usize)> = groups.iter().map(|(name, files)| (*name, files.len())).collect();
        assert_eq!(names, [("tb_lib", 1), ("design_lib", 3), ("util_lib", 2)]);
    }

    #[test]
    fn opening_a_missing_project_logs_an_error_and_forgets_it() {
        let mut state = AppState::new(RecentProjects::from_paths(vec!["no/such/project.toml".into()]));
        state.open_project(Path::new("no/such/project.toml"));
        assert!(state.open.is_none());
        assert!(state.recent.paths().is_empty());
        assert_eq!(state.session.log.last().unwrap().style, LogStyle::Error);
    }

    #[test]
    fn new_project_in_a_folder_finds_the_testbench() {
        let folder = copy_of_project("three_libs", "app_new_project");
        std::fs::remove_file(folder.join("project.toml")).unwrap();
        let mut state = AppState::new(RecentProjects::default());
        state.new_project(&folder);
        let open = state.open.as_ref().expect("project should be created and opened");
        assert_eq!(open.project.config.testbenches[0].top, "top_tb");
    }

    #[test]
    fn run_builds_then_simulates_in_the_background() {
        let mut state = AppState::new(RecentProjects::default());
        if !nvc_available(&state) {
            return;
        }
        let folder = copy_of_project("three_libs", "app_run_builds_then_simulates");
        state.open_project(&folder);

        let testbench = state.selected_testbench().unwrap().clone();
        run_to_completion(&mut state, Job::Run { testbench });

        let outcome = state.session.last_outcome.clone().unwrap();
        assert!(outcome.succeeded, "{:#?}", state.session.log);
        assert_eq!(outcome.summary, "top_tb PASSED");
        assert!(state.session.log.iter().any(|line| line.text.contains("y = 25")));
        let wave_line = state.session.log.iter().find(|line| line.text.starts_with("Waveform: "));
        assert!(wave_line.unwrap().text.ends_with("top_tb.fst"), "{:#?}", state.session.log);
        // The waveform opened in the Waves tab.
        assert_eq!(state.tab, MainTab::Waves);
        assert!(state.waves.wave.is_some(), "{:?}", state.waves.error);

        // Set up the view, then run again: nvc rewrites the waveform the
        // app has open (which must not be locked), and the setup stays.
        let x = state.waves.wave.as_ref().unwrap().find_signal("top_tb.x").unwrap();
        state.waves.add(&[x], || {});
        state.waves.cursors.primary = Some(studio_core::SimTime::parse("5 ns").unwrap());
        let testbench = state.selected_testbench().unwrap().clone();
        run_to_completion(&mut state, Job::Run { testbench });
        assert!(state.session.last_outcome.as_ref().unwrap().succeeded, "{:#?}", state.session.log);
        assert!(state.waves.wave.is_some(), "{:?}", state.waves.error);
        assert_eq!(state.waves.list.signals().map(|s| s.label.as_str()).collect::<Vec<_>>(), ["x"]);
        assert_eq!(state.waves.cursors.primary, studio_core::SimTime::parse("5 ns"));
        state.waves.save_layout_now();
        assert!(folder.join("layouts").join("top_tb.toml").is_file(), "layout saved in the project");
        // The plan was refreshed after the build: nothing left to compile.
        let plan = state.open.as_ref().unwrap().plan.as_ref().unwrap();
        assert_eq!(plan.compile_count(), 0);
    }

    #[test]
    fn failed_build_shows_problems_and_selects_the_file() {
        let mut state = AppState::new(RecentProjects::default());
        if !nvc_available(&state) {
            return;
        }
        let folder = copy_of_project("three_libs", "app_failed_build_shows_problems");
        let adder = folder.join("src/b_adder.vhd");
        let text = std::fs::read_to_string(&adder).unwrap().replace("sum <= a + b;", "sum <= a + c;");
        std::fs::write(&adder, text).unwrap();
        state.open_project(&folder);

        run_to_completion(&mut state, Job::Build { force: false });

        assert!(!state.session.last_outcome.as_ref().unwrap().succeeded);
        assert_eq!(state.tab, MainTab::Problems);
        assert!(state.session.error_count() >= 1);
        state.select_problem(0);
        assert_eq!(state.selected_file.as_deref(), Some(Path::new("src/b_adder.vhd")));
    }
}
