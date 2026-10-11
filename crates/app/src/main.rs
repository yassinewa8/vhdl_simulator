//! Desktop GUI entry point.
//!
//! Modules:
//! - `state`: what the app knows and the actions buttons trigger (no egui)
//! - `worker`: runs builds, simulations and installs on a background thread
//! - `session`: the output log, Problems list and progress line
//! - `recent`: the recent projects list
//! - `ui`: drawing the window with egui

mod cursors;
mod editor;
mod editor_ui;
mod layout;
mod recent;
mod session;
mod state;
mod timeline;
mod wave_list;
mod wave_state;
mod wave_style;
mod wave_ui;
mod ui;
mod worker;

use std::path::PathBuf;

use eframe::egui;

fn main() -> eframe::Result {
    // `app path/to/project.toml` (or a folder) opens that project;
    // `app some.fst` opens a waveform.
    let project_to_open = std::env::args_os().nth(1).map(PathBuf::from);
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title(window_title())
            .with_inner_size([1200.0, 800.0]),
        ..Default::default()
    };

    eframe::run_native(
        &window_title(),
        options,
        Box::new(|creation_context| Ok(Box::new(ui::StudioApp::new(creation_context, project_to_open)))),
    )
}

/// Text shown in the window's title bar before a project is opened.
fn window_title() -> String {
    studio_core::PRODUCT_NAME.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn window_title_is_nvc_studio() {
        assert_eq!(window_title(), "NVC Studio");
    }
}
