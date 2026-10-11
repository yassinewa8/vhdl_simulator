//! Drawing the source editor: a tab button per open file, the editor with
//! line numbers, highlighting and a find bar, and the "save changes?"
//! questions. The logic (saving, line ends, changes on disk, finding)
//! lives in `editor.rs`.

use eframe::egui::{
    self, Align, Button, Color32, FontId, Key, KeyboardShortcut, Modifiers, RichText, TextBuffer,
    text::{CCursor, CCursorRange, LayoutJob, TextFormat},
    text_edit::TextEditState,
};
use project::{TokenKind, highlight};

use crate::editor::find_next;
use crate::state::{AppState, MainTab};

/// Something waiting for the user to answer "save your changes?".
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CloseRequest {
    /// Closing one tab with unsaved edits.
    Tab(usize),
    /// Closing the window with unsaved edits in some tabs.
    Window,
}

/// The find bar above the editor (Ctrl+F).
#[derive(Debug, Default)]
pub struct FindBar {
    open: bool,
    text: String,
    /// Put the keyboard focus in the search field on the next frame.
    focus_field: bool,
    not_found: bool,
    /// The frame in which the last search ran (see `editor_view`).
    searched_in_frame: Option<u64>,
}

/// A tab button for each open file, after the fixed tabs. A `•` marks
/// unsaved edits; `×` (or a middle click) closes the tab.
pub fn tab_buttons(ui: &mut egui::Ui, state: &mut AppState, close_request: &mut Option<CloseRequest>) {
    let mut show = None;
    let mut close = None;
    for (index, file) in state.editors.files.iter().enumerate() {
        let selected = state.tab == MainTab::Source && state.editors.active == index;
        let title = if file.is_modified() { format!("{} •", file.title()) } else { file.title() };
        let button = ui.selectable_label(selected, title).on_hover_text(file.path.display().to_string());
        if button.clicked() {
            show = Some(index);
        }
        if button.middle_clicked() || ui.small_button("×").on_hover_text("Close").clicked() {
            close = Some(index);
        }
    }
    if let Some(index) = show {
        state.editors.active = index;
        state.tab = MainTab::Source;
    }
    if let Some(index) = close {
        if state.editors.files[index].is_modified() {
            *close_request = Some(CloseRequest::Tab(index));
        } else {
            close_tab(state, index);
        }
    }
}

fn close_tab(state: &mut AppState, index: usize) {
    state.editors.close(index);
    if state.editors.files.is_empty() && state.tab == MainTab::Source {
        state.tab = MainTab::Output;
    }
}

/// What the user asked for above the text, done once nothing is borrowed.
#[derive(Default)]
struct Actions {
    save: bool,
    reload: bool,
    keep_mine: bool,
    find_next: bool,
}

/// The file in the active tab: a line with its path and buttons, any
/// warnings, the find bar, then the text.
pub fn editor_view(ui: &mut egui::Ui, state: &mut AppState, find: &mut FindBar) {
    let index = state.editors.active;
    let Some(file) = state.editors.files.get(index) else {
        ui.weak("No file open. Double-click a file in the list to open it.");
        return;
    };
    let editable = file.read_only.is_none();
    let editor_id = egui::Id::new(("source editor", &file.path));
    let mut actions = Actions::default();

    let save_keys = KeyboardShortcut::new(Modifiers::COMMAND, Key::S);
    let find_keys = KeyboardShortcut::new(Modifiers::COMMAND, Key::F);
    let find_again_keys = KeyboardShortcut::new(Modifiers::NONE, Key::F3);
    actions.save = ui.input_mut(|i| i.consume_shortcut(&save_keys));
    if ui.input_mut(|i| i.consume_shortcut(&find_keys)) {
        find.open = true;
        find.focus_field = true;
    }
    actions.find_next = find.open && ui.input_mut(|i| i.consume_shortcut(&find_again_keys));

    // Buttons first, from the right, so a long path is shortened to fit
    // the space left instead of running under them.
    ui.horizontal(|ui| {
        ui.with_layout(egui::Layout::right_to_left(Align::Center), |ui| {
            let save = Button::new("Save");
            if ui.add_enabled(editable && file.is_modified(), save).on_hover_text("Ctrl+S").clicked() {
                actions.save = true;
            }
            if ui.button("Find").on_hover_text("Ctrl+F, then Enter or F3 for the next one").clicked() {
                find.open = true;
                find.focus_field = true;
            }
            let path = RichText::new(file.path.display().to_string()).weak();
            ui.with_layout(egui::Layout::left_to_right(Align::Center), |ui| {
                ui.add(egui::Label::new(path).truncate());
            });
        });
    });
    if let Some(reason) = &file.read_only {
        ui.colored_label(ui.visuals().warn_fg_color, reason);
    }
    if let Some(error) = &file.save_error {
        ui.colored_label(ui.visuals().error_fg_color, format!("Not saved: {error}"));
    }
    if file.changed_on_disk {
        ui.horizontal(|ui| {
            ui.colored_label(ui.visuals().warn_fg_color, "The file changed on disk, and you have unsaved edits here.");
            actions.reload = ui.button("Reload (drop my edits)").clicked();
            actions.keep_mine = ui.button("Keep mine").on_hover_text("Saving will overwrite the file").clicked();
        });
    }
    if find.open {
        find_bar(ui, find, &mut actions);
    }
    ui.separator();

    // Act on the buttons, now that `file` isn't borrowed any more.
    if actions.save && editable {
        state.save_source(index);
    }
    let file = &mut state.editors.files[index];
    if actions.reload
        && let Err(error) = file.reload()
    {
        file.save_error = Some(format!("couldn't reload: {error}"));
    }
    if actions.keep_mine {
        file.keep_mine();
    }

    // Find: select the next match after the cursor and scroll to it.
    let mut scroll_to = None;
    // egui sometimes draws a frame twice to settle its layout, and both
    // passes see the same key press. Without this, one Enter or F3 would
    // jump two matches ahead.
    let frame = ui.ctx().cumulative_frame_nr();
    if actions.find_next && find.searched_in_frame != Some(frame) {
        find.searched_in_frame = Some(frame);
        let after = TextEditState::load(ui.ctx(), editor_id)
            .and_then(|state| state.cursor.char_range())
            .map_or(0, |range| range.primary.index.0.max(range.secondary.index.0));
        match find_next(&file.text, &find.text, after) {
            Some(found) => {
                let mut edit_state = TextEditState::load(ui.ctx(), editor_id).unwrap_or_default();
                let range = CCursorRange::two(CCursor::new(found.start), CCursor::new(found.end));
                edit_state.cursor.set_char_range(Some(range));
                edit_state.store(ui.ctx(), editor_id);
                ui.ctx().memory_mut(|memory| memory.request_focus(editor_id));
                scroll_to = Some(found.start);
                find.not_found = false;
            }
            None => find.not_found = true,
        }
    }

    text_area(ui, file, editable, editor_id, scroll_to);
}

/// The search field and its buttons.
fn find_bar(ui: &mut egui::Ui, find: &mut FindBar, actions: &mut Actions) {
    ui.horizontal(|ui| {
        ui.label("Find:");
        let field = ui.add(egui::TextEdit::singleline(&mut find.text).desired_width(220.0));
        if find.focus_field {
            field.request_focus();
            find.focus_field = false;
        }
        if field.changed() {
            find.not_found = false;
        }
        // Take the Enter key out of the input: the found text is about to
        // be selected in the editor, and the editor would otherwise get
        // this same Enter and replace the selection with a line break.
        let entered = field.lost_focus() && ui.input_mut(|i| i.consume_key(Modifiers::NONE, Key::Enter));
        if entered || ui.button("Next").on_hover_text("F3").clicked() {
            actions.find_next = true;
        }
        if find.not_found {
            ui.colored_label(ui.visuals().warn_fg_color, "Not found");
        }
        if ui.small_button("×").on_hover_text("Close (Esc)").clicked()
            || (field.has_focus() && ui.input(|i| i.key_pressed(Key::Escape)))
        {
            find.open = false;
        }
    });
}

/// Line numbers and the text itself, scrolling together.
fn text_area(
    ui: &mut egui::Ui,
    file: &mut crate::editor::OpenFile,
    editable: bool,
    editor_id: egui::Id,
    scroll_to: Option<usize>,
) {
    let font = FontId::monospace(egui::TextStyle::Monospace.resolve(ui.style()).size);
    let dark = ui.visuals().dark_mode;
    let lines = file.text.split('\n').count();

    egui::ScrollArea::both().auto_shrink(false).show(ui, |ui| {
        ui.horizontal_top(|ui| {
            let numbers: Vec<String> = (1..=lines).map(|n| n.to_string()).collect();
            ui.add(egui::Label::new(RichText::new(numbers.join("\n")).font(font.clone()).weak()).selectable(false));
            ui.add_space(8.0);

            let mut layouter = |ui: &egui::Ui, text: &dyn TextBuffer, _wrap_width: f32| {
                let job = ui
                    .ctx()
                    .memory_mut(|memory| memory.caches.cache::<HighlightCache>().get((&font, dark, text.as_str())).clone());
                ui.ctx().fonts_mut(|fonts| fonts.layout_job(job))
            };
            // A read-only file is shown as a `&str`, which can be selected
            // and copied but not changed.
            let copy = if editable { String::new() } else { file.text.clone() };
            let mut read_only_text: &str = &copy;
            let buffer: &mut dyn TextBuffer = if editable { &mut file.text } else { &mut read_only_text };
            let output = egui::TextEdit::multiline(buffer)
                .id(editor_id)
                .code_editor()
                .frame(egui::Frame::NONE)
                .margin(egui::Margin::ZERO)
                .desired_width(f32::INFINITY)
                .desired_rows(lines)
                .layouter(&mut layouter)
                .show(ui);

            // A match chosen by Find: bring it into view.
            if let Some(start) = scroll_to {
                let rect = output.galley.pos_from_cursor(CCursor::new(start)).translate(output.galley_pos.to_vec2());
                ui.scroll_to_rect(rect.expand(40.0), Some(Align::Center));
            }
        });
    });
}

/// Colours for each kind of text, close to common editor themes.
fn colour(kind: TokenKind, dark: bool) -> Color32 {
    match (kind, dark) {
        (TokenKind::Plain, true) => Color32::from_gray(220),
        (TokenKind::Plain, false) => Color32::from_gray(30),
        (TokenKind::Keyword, true) => Color32::from_rgb(86, 156, 214),
        (TokenKind::Keyword, false) => Color32::from_rgb(0, 0, 220),
        (TokenKind::Type, true) => Color32::from_rgb(78, 201, 176),
        (TokenKind::Type, false) => Color32::from_rgb(38, 127, 153),
        (TokenKind::Comment, true) => Color32::from_rgb(106, 153, 85),
        (TokenKind::Comment, false) => Color32::from_rgb(0, 128, 0),
        (TokenKind::Literal, true) => Color32::from_rgb(206, 145, 120),
        (TokenKind::Literal, false) => Color32::from_rgb(163, 21, 21),
        (TokenKind::Number, true) => Color32::from_rgb(181, 206, 168),
        (TokenKind::Number, false) => Color32::from_rgb(9, 134, 88),
    }
}

/// Turns VHDL text into coloured text for egui. Kept in egui's frame
/// cache, so it's only worked out again when the text changes.
#[derive(Default)]
struct Highlighter;

type HighlightCache = egui::cache::FrameCache<LayoutJob, Highlighter>;

impl egui::cache::ComputerMut<(&FontId, bool, &str), LayoutJob> for Highlighter {
    fn compute(&mut self, (font, dark, text): (&FontId, bool, &str)) -> LayoutJob {
        let mut job = LayoutJob::default();
        // Code isn't wrapped; long lines scroll sideways.
        job.wrap.max_width = f32::INFINITY;
        for span in highlight(text) {
            let format = TextFormat::simple(font.clone(), colour(span.kind, dark));
            job.append(&text[span.range], 0.0, format);
        }
        job
    }
}

/// Asks about unsaved edits when a tab or the window is being closed.
/// `allow_close` lets the window close next time it's asked to.
pub fn unsaved_changes_dialog(
    ctx: &egui::Context,
    state: &mut AppState,
    request: &mut Option<CloseRequest>,
    allow_close: &mut bool,
) {
    let Some(what) = *request else { return };
    let question = match what {
        CloseRequest::Tab(index) => match state.editors.files.get(index) {
            Some(file) => format!("Save your changes to {}?", file.title()),
            None => {
                *request = None;
                return;
            }
        },
        CloseRequest::Window => format!("{} file(s) have unsaved changes. Save them?", state.editors.modified_count()),
    };

    let mut answer = None;
    egui::Modal::new(egui::Id::new("unsaved changes")).show(ctx, |ui| {
        ui.label(question);
        ui.add_space(8.0);
        ui.horizontal(|ui| {
            if ui.button("Save").clicked() {
                answer = Some(Answer::Save);
            }
            if ui.button("Don't save").clicked() {
                answer = Some(Answer::Discard);
            }
            if ui.button("Cancel").clicked() || ui.input(|i| i.key_pressed(Key::Escape)) {
                answer = Some(Answer::Cancel);
            }
        });
    });

    let Some(answer) = answer else { return };
    *request = None;
    match (what, answer) {
        (_, Answer::Cancel) => {}
        (CloseRequest::Tab(index), Answer::Save) => {
            // If saving fails, the tab stays open showing why.
            if state.save_source(index) {
                close_tab(state, index);
            }
        }
        (CloseRequest::Tab(index), Answer::Discard) => close_tab(state, index),
        (CloseRequest::Window, Answer::Save) => {
            if state.save_all_sources() {
                *allow_close = true;
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }
        }
        (CloseRequest::Window, Answer::Discard) => {
            *allow_close = true;
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
    }
}

#[derive(Debug, Clone, Copy)]
enum Answer {
    Save,
    Discard,
    Cancel,
}
