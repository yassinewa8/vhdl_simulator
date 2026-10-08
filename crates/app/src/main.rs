//! Desktop GUI entry point. Opens an empty window for now.

use eframe::egui;

fn main() -> eframe::Result {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title(window_title())
            .with_inner_size([1200.0, 800.0]),
        ..Default::default()
    };

    eframe::run_native(
        &window_title(),
        options,
        Box::new(|_creation_context| Ok(Box::new(StudioApp))),
    )
}

/// Text shown in the window's title bar.
fn window_title() -> String {
    studio_core::PRODUCT_NAME.to_string()
}

/// The whole application's state. Empty until Phase 2 adds projects.
struct StudioApp;

impl eframe::App for StudioApp {
    /// Called by eframe every frame to draw the window contents.
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        egui::CentralPanel::default().show(ui, |_ui| {});
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn window_title_is_nvc_studio() {
        assert_eq!(window_title(), "NVC Studio");
    }
}
