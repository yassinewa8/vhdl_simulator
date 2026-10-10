//! Drawing the window. All decisions live in `state.rs`; this file only
//! lays out widgets and calls `AppState` methods when they're clicked.
//!
//! egui is "immediate mode": the whole window is redrawn from the current
//! state every frame, a bit like a combinational block re-evaluating its
//! outputs whenever an input changes. There are no widget objects to keep
//! in sync.

use std::path::{Path, PathBuf};
use std::time::Duration;

use eframe::egui::{self, Color32, RichText};
use project::PlannedFile;

use crate::recent::RecentProjects;
use crate::session::{LogStyle, Problem};
use crate::state::{AppState, MainTab, files_by_library, same_file};
use crate::worker::Job;

/// Key under which the recent projects list is saved between runs.
const RECENT_PROJECTS_KEY: &str = "recent_projects";

/// Packages offered in the Libraries menu. nvc can install more (see
/// `cli install`); these are the common verification libraries.
const LIBRARY_PACKAGES: [(&str, &str); 2] = [("osvvm", "OSVVM"), ("uvvm", "UVVM")];

pub struct StudioApp {
    state: AppState,
    /// Last title sent to the window, so it's only updated on change.
    title: String,
}

impl StudioApp {
    /// `project_to_open` comes from the command line; without one, the
    /// most recent project is reopened.
    pub fn new(creation_context: &eframe::CreationContext<'_>, project_to_open: Option<PathBuf>) -> StudioApp {
        let mut saved: Vec<PathBuf> = creation_context
            .storage
            .and_then(|storage| eframe::get_value(storage, RECENT_PROJECTS_KEY))
            .unwrap_or_default();
        // Drop projects that have been deleted or moved since last time.
        saved.retain(|path| path.is_file());
        let mut state = AppState::new(RecentProjects::from_paths(saved));
        // Background work asks egui to redraw when it has news.
        let ctx = creation_context.egui_ctx.clone();
        state.set_waker(move || ctx.request_repaint());

        if let Some(path) = project_to_open {
            // A waveform file opens in the Waves tab; anything else is
            // taken to be a project file or folder.
            if is_waveform_file(&path) {
                state.open_waveform(&path);
            } else {
                state.open_project(&path);
            }
        } else if let Some(last) = state.recent.paths().first().cloned()
            && last.is_file()
        {
            // Reopen the last project, like most IDEs do.
            state.open_project(&last);
        }
        StudioApp {
            state,
            title: String::new(),
        }
    }



    /// Opens a waveform file dropped onto the window.
    fn open_dropped_waveforms(&mut self, ctx: &egui::Context) {
        let dropped: Vec<PathBuf> = ctx.input(|i| i.raw.dropped_files.iter().map(|f| f.path().to_path_buf()).collect());
        if let Some(path) = dropped.iter().find(|p| is_waveform_file(p)) {
            self.state.open_waveform(path);
        }
    }

    fn update_title(&mut self, ctx: &egui::Context) {
        let title = match &self.state.open {
            Some(open) => format!("{} — {}", open.project.config.project.name, studio_core::PRODUCT_NAME),
            None => studio_core::PRODUCT_NAME.to_string(),
        };
        if title != self.title {
            ctx.send_viewport_cmd(egui::ViewportCommand::Title(title.clone()));
            self.title = title;
        }
    }

    fn menu_bar(&mut self, ui: &mut egui::Ui) {
        egui::MenuBar::new().ui(ui, |ui| {
            ui.menu_button("File", |ui| self.file_menu(ui));
            ui.menu_button("Project", |ui| self.project_menu(ui));
            ui.menu_button("Libraries", |ui| self.libraries_menu(ui));
        });
    }

    fn file_menu(&mut self, ui: &mut egui::Ui) {
        if ui.button("New project…").on_hover_text("Choose a folder of VHDL files").clicked()
            && let Some(folder) = rfd::FileDialog::new().set_title("Folder for the new project").pick_folder()
        {
            ui.close();
            self.state.new_project(&folder);
        }
        if ui.button("Open project…").clicked()
            && let Some(file) = rfd::FileDialog::new()
                .set_title("Open project")
                .add_filter("Project file", &["toml"])
                .pick_file()
        {
            ui.close();
            self.state.open_project(&file);
        }
        if ui
            .button("Open waveform…")
            .on_hover_text("An FST, VCD or GHW file, e.g. from OSVVM or another simulator")
            .clicked()
            && let Some(file) = rfd::FileDialog::new()
                .set_title("Open waveform")
                .add_filter("Waveform", wave::SUPPORTED_EXTENSIONS)
                .pick_file()
        {
            ui.close();
            self.state.open_waveform(&file);
        }
        ui.menu_button("Recent projects", |ui| {
            if self.state.recent.paths().is_empty() {
                ui.label("(none yet)");
            }
            for path in self.state.recent.paths().to_vec() {
                if ui.button(path.display().to_string()).clicked() {
                    ui.close();
                    self.state.open_project(&path);
                }
            }
        });
        if ui.add_enabled(self.state.open.is_some(), egui::Button::new("Close project")).clicked() {
            ui.close();
            self.state.close_project();
        }
        ui.separator();
        if ui.button("Quit").clicked() {
            ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
        }
    }

    fn project_menu(&mut self, ui: &mut egui::Ui) {
        let can_build = self.state.open.is_some() && !self.state.is_busy();
        if ui.add_enabled(can_build, egui::Button::new("Build")).clicked() {
            ui.close();
            self.state.start_job(Job::Build { force: false });
        }
        if ui
            .add_enabled(can_build, egui::Button::new("Rebuild all"))
            .on_hover_text("Compile every file, ignoring what was compiled before")
            .clicked()
        {
            ui.close();
            self.state.start_job(Job::Build { force: true });
        }
        if ui.add_enabled(self.state.open.is_some(), egui::Button::new("Reload project file")).clicked() {
            ui.close();
            self.state.refresh_plan();
        }
    }

    fn libraries_menu(&mut self, ui: &mut egui::Ui) {
        for (package, label) in LIBRARY_PACKAGES {
            let button = egui::Button::new(format!("Install {label}"));
            if ui
                .add_enabled(!self.state.is_busy(), button)
                .on_hover_text("Downloads and compiles it into ~/.nvc/lib (can take a minute)")
                .clicked()
            {
                ui.close();
                self.state.start_job(Job::Install {
                    package: package.to_string(),
                    quartus_root: None,
                });
            }
        }
        ui.separator();
        self.quartus_items(ui);
        ui.separator();
        ui.menu_button(format!("Installed ({})", self.state.installed.len()), |ui| {
            if self.state.installed.is_empty() {
                ui.label("Nothing installed yet");
            }
            for library in &self.state.installed {
                ui.label(library);
            }
        });
    }

    /// Intel/Altera Quartus vendor libraries. Nothing is downloaded: nvc's
    /// script compiles the simulation sources of an installed Quartus, so
    /// we need its folder, found automatically or chosen by the user.
    fn quartus_items(&mut self, ui: &mut egui::Ui) {
        let idle = !self.state.is_busy();
        let mut clicked = false;
        let mut chosen = None;
        if ui
            .add_enabled(idle, egui::Button::new("Install Intel Quartus libraries"))
            .on_hover_text(
                "Compiles altera_mf, lpm, cyclonev etc. from your Quartus installation into ~/.nvc/lib. \
                 Quartus is found by itself, or you're asked for its folder. Takes several minutes.",
            )
            .clicked()
        {
            ui.close();
            clicked = true;
            chosen = nvc::find_quartus().or_else(pick_quartus_folder);
        }
        if ui
            .add_enabled(idle, egui::Button::new("Install Quartus libraries from folder…"))
            .on_hover_text("The same, choosing which Quartus installation to use")
            .clicked()
        {
            ui.close();
            clicked = true;
            chosen = pick_quartus_folder();
        }
        match chosen {
            Some(quartus_root) => self.state.start_job(Job::Install {
                package: "quartus".to_string(),
                quartus_root: Some(quartus_root),
            }),
            // No Quartus found and the folder picker was closed: say so,
            // rather than leaving the click looking like it did nothing.
            None if clicked => {
                let message = nvc::InstallError::NoQuartus { given: None }.to_string();
                self.state.session.note(&format!("Quartus libraries not installed: {message}"), LogStyle::Warning);
                self.state.tab = MainTab::Output;
            }
            None => {}
        }
    }

    fn toolbar(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            self.testbench_picker(ui);

            let busy = self.state.is_busy();
            let has_project = self.state.open.is_some();
            if ui
                .add_enabled(has_project && !busy, egui::Button::new("Build"))
                .on_hover_text("Compile what changed, in dependency order")
                .clicked()
            {
                self.state.start_job(Job::Build { force: false });
            }
            let testbench = self.state.selected_testbench().cloned();
            if ui
                .add_enabled(testbench.is_some() && !busy, egui::Button::new("▶ Run"))
                .on_hover_text("Build, then elaborate and run the testbench")
                .clicked()
                && let Some(testbench) = testbench
            {
                self.state.start_job(Job::Run { testbench });
            }
            let stopping = self.state.job.as_ref().is_some_and(|job| job.is_stopping());
            let stop_hint = match &self.state.job {
                Some(job) => format!("Stop: {}", job.description),
                None => "Nothing running".to_string(),
            };
            if ui
                .add_enabled(busy && !stopping, egui::Button::new("■ Stop"))
                .on_hover_text(stop_hint)
                .on_disabled_hover_text("Nothing running")
                .clicked()
            {
                self.state.stop_job();
            }

            ui.separator();
            self.status(ui);
        });
    }

    fn testbench_picker(&mut self, ui: &mut egui::Ui) {
        let names: Vec<String> = match &self.state.open {
            Some(open) => open.project.config.testbenches.iter().map(|tb| tb.display_name().to_string()).collect(),
            None => Vec::new(),
        };
        let selected_text = names
            .get(self.state.selected_testbench)
            .cloned()
            .unwrap_or_else(|| "(no testbenches)".to_string());
        ui.add_enabled_ui(!names.is_empty() && !self.state.is_busy(), |ui| {
            egui::ComboBox::from_id_salt("testbench")
                .selected_text(selected_text)
                .width(180.0)
                .show_ui(ui, |ui| {
                    for (index, name) in names.iter().enumerate() {
                        ui.selectable_value(&mut self.state.selected_testbench, index, name);
                    }
                });
        });
    }

    /// What's happening now, or how the last job ended.
    fn status(&self, ui: &mut egui::Ui) {
        let session = &self.state.session;
        if let Some(progress) = &session.progress {
            ui.spinner();
            ui.label(progress);
        } else if let Some(outcome) = &session.last_outcome {
            let colour = if outcome.succeeded {
                success_colour(ui)
            } else if outcome.cancelled {
                ui.visuals().warn_fg_color
            } else {
                ui.visuals().error_fg_color
            };
            ui.label(RichText::new(&outcome.summary).color(colour).strong());
        }
    }

    fn status_bar(&self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| match &self.state.nvc {
            Ok(install) => {
                ui.weak(format!("nvc {}", install.version))
                    .on_hover_text(install.program.display().to_string());
            }
            Err(message) => {
                ui.colored_label(ui.visuals().error_fg_color, "nvc not found")
                    .on_hover_text(message);
            }
        });
    }

    fn file_panel(&mut self, ui: &mut egui::Ui) {
        let Some(open) = &self.state.open else {
            ui.weak("No project open");
            return;
        };
        ui.heading(&open.project.config.project.name);
        ui.weak(open.project.root.display().to_string());
        ui.separator();

        let plan = match &open.plan {
            Ok(plan) => plan,
            Err(message) => {
                ui.colored_label(ui.visuals().error_fg_color, message);
                return;
            }
        };
        let stale = plan.compile_count();
        if stale > 0 {
            ui.weak(format!("• {stale} file(s) need compiling"));
        }

        let mut clicked: Option<PathBuf> = None;
        egui::ScrollArea::vertical().auto_shrink(false).show(ui, |ui| {
            for (library, files) in files_by_library(&open.project, plan) {
                egui::CollapsingHeader::new(RichText::new(library).strong())
                    .id_salt(library)
                    .default_open(true)
                    .show(ui, |ui| {
                        if files.is_empty() {
                            ui.weak("(no files)");
                        }
                        for file in files {
                            let selected = self
                                .state
                                .selected_file
                                .as_deref()
                                .is_some_and(|path| same_file(path, &file.source.path));
                            if file_row(ui, file, selected).clicked() {
                                clicked = Some(file.source.path.clone());
                            }
                        }
                    });
            }
        });
        if clicked.is_some() {
            self.state.selected_file = clicked;
        }
    }

    fn main_area(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.selectable_value(&mut self.state.tab, MainTab::Output, "Output");
            let problems = &self.state.session.problems;
            let label = if problems.is_empty() {
                "Problems".to_string()
            } else {
                format!("Problems ({})", problems.len())
            };
            ui.selectable_value(&mut self.state.tab, MainTab::Problems, label);
            ui.selectable_value(&mut self.state.tab, MainTab::Waves, "Waves");
            if self.state.tab == MainTab::Output {
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.small_button("Clear").clicked() {
                        self.state.session.clear_log();
                    }
                });
            }
        });
        ui.separator();

        match self.state.tab {
            MainTab::Output => self.output_view(ui),
            MainTab::Problems => self.problems_view(ui),
            MainTab::Waves => crate::wave_ui::waves_tab(ui, &mut self.state),
        }
    }

    fn output_view(&self, ui: &mut egui::Ui) {
        let log = &self.state.session.log;
        if log.is_empty() && self.state.open.is_none() {
            ui.label("Open a project (File → Open project…) or create one from a folder of VHDL files (File → New project…).");
            return;
        }
        let row_height = ui.text_style_height(&egui::TextStyle::Monospace);
        // Only the visible rows are laid out, so a huge log stays fast.
        egui::ScrollArea::both()
            .auto_shrink(false)
            .stick_to_bottom(true)
            .show_rows(ui, row_height, log.len(), |ui, rows| {
                for line in &log[rows] {
                    let text = RichText::new(&line.text).monospace();
                    let text = match line.style {
                        LogStyle::Plain => text,
                        LogStyle::Heading => text.strong(),
                        LogStyle::Note => text.color(ui.visuals().weak_text_color()),
                        LogStyle::Warning => text.color(ui.visuals().warn_fg_color),
                        LogStyle::Error => text.color(ui.visuals().error_fg_color),
                        LogStyle::Success => text.color(success_colour(ui)).strong(),
                    };
                    ui.add(egui::Label::new(text).extend());
                }
            });
    }

    fn problems_view(&mut self, ui: &mut egui::Ui) {
        if self.state.session.problems.is_empty() {
            ui.label("No problems.");
            return;
        }
        let mut clicked = None;
        egui::ScrollArea::vertical()
            .id_salt("problem_list")
            .max_height(ui.available_height() * 0.45)
            .auto_shrink([false, true])
            .show(ui, |ui| {
                for (index, problem) in self.state.session.problems.iter().enumerate() {
                    let selected = self.state.selected_problem == Some(index);
                    if problem_row(ui, problem, selected).clicked() {
                        clicked = Some(index);
                    }
                }
            });
        if let Some(index) = clicked {
            self.state.select_problem(index);
        }

        ui.separator();
        let problem = self.state.selected_problem.and_then(|i| self.state.session.problems.get(i));
        match problem {
            Some(problem) => {
                egui::ScrollArea::both().id_salt("problem_details").auto_shrink(false).show(ui, |ui| {
                    ui.add(egui::Label::new(RichText::new(&problem.details).monospace()).extend());
                });
            }
            None => {
                ui.weak("Click a problem to see nvc's full message.");
            }
        }
    }
}

impl eframe::App for StudioApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        self.state.poll();
        self.open_dropped_waveforms(&ctx);
        self.update_title(&ctx);
        // While a job runs, keep the spinner turning even between messages.
        if self.state.is_busy() {
            ctx.request_repaint_after(Duration::from_millis(100));
        }

        egui::Panel::top("menu").show(ui, |ui| self.menu_bar(ui));
        egui::Panel::top("toolbar").show(ui, |ui| self.toolbar(ui));
        egui::Panel::bottom("status").show(ui, |ui| self.status_bar(ui));
        egui::Panel::left("files")
            .resizable(true)
            .default_size(280.0)
            .show(ui, |ui| self.file_panel(ui));
        egui::CentralPanel::default().show(ui, |ui| self.main_area(ui));
    }

    fn save(&mut self, storage: &mut dyn eframe::Storage) {
        eframe::set_value(storage, RECENT_PROJECTS_KEY, &self.state.recent.paths().to_vec());
    }

    fn on_exit(&mut self) {
        self.state.waves.save_layout_now();
        // Don't leave nvc running after the window closes.
        if let Some(job) = &self.state.job {
            job.stop_and_wait(Duration::from_secs(3));
        }
    }
}

/// One file in the tree: a dot if it needs compiling, the file name, and
/// the units it defines on hover.
fn file_row(ui: &mut egui::Ui, file: &PlannedFile, selected: bool) -> egui::Response {
    let name = file_name(&file.source.path);
    // "•" rather than "●": egui's built-in font has no "●".
    let text = if file.needs_compile { format!("• {name}") } else { format!("   {name}") };
    let units: Vec<String> = file.units.iter().map(|unit| format!("{:?} {}", unit.kind, unit.name)).collect();
    let hover = format!(
        "{}\n{}{}",
        file.source.path.display(),
        if file.needs_compile { "Needs compiling\n" } else { "" },
        units.join("\n")
    );
    ui.selectable_label(selected, text).on_hover_text(hover)
}

fn problem_row(ui: &mut egui::Ui, problem: &Problem, selected: bool) -> egui::Response {
    // Coloured text is hard to read on the selection highlight.
    let colour = if selected {
        ui.visuals().strong_text_color()
    } else if problem.severity.is_error() {
        ui.visuals().error_fg_color
    } else {
        ui.visuals().warn_fg_color
    };
    let location = problem
        .location
        .as_ref()
        .map(|location| format!("{location}  "))
        .unwrap_or_default();
    let text = RichText::new(format!("{}: {location}{}", problem.severity, problem.message)).color(colour);
    ui.selectable_label(selected, text)
}

fn file_name(path: &Path) -> String {
    path.file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.display().to_string())
}

/// True for files with a waveform extension (.fst, .vcd, .ghw).
fn is_waveform_file(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| wave::SUPPORTED_EXTENSIONS.contains(&e.to_lowercase().as_str()))
}

/// Green that's readable on both light and dark backgrounds.
fn success_colour(ui: &egui::Ui) -> Color32 {
    if ui.visuals().dark_mode {
        Color32::from_rgb(0x6c, 0xc6, 0x6f)
    } else {
        Color32::from_rgb(0x1b, 0x7f, 0x2a)
    }
}

/// Asks the user for their Quartus folder. Either the `quartus` folder or
/// the one above it (e.g. `C:\\intelFPGA_lite\\23.1std`) will do; a folder
/// that's neither is passed on as it is, so the install explains the problem.
fn pick_quartus_folder() -> Option<PathBuf> {
    let picked = rfd::FileDialog::new()
        .set_title("Quartus installation folder (e.g. C:\\intelFPGA_lite\\23.1std)")
        .pick_folder()?;
    Some(nvc::quartus_root_from(&picked).unwrap_or(picked))
}
