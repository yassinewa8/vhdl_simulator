//! Drawing the Waves tab: the signal tree, the names column, the time
//! ruler and the waves themselves.
//!
//! The wave area is painted by hand (lines and shapes), one row per
//! signal, and only for the rows and times on screen. For each row it asks
//! the wave crate for a per-pixel summary, so drawing costs the same
//! whether a signal has ten changes or ten million.

use std::time::Instant;

use eframe::egui::{self, Align2, Color32, FontId, Pos2, Rect, RichText, Sense, Stroke, Vec2};
use studio_core::{Logic, Value, format_value};
use wave::{Bucket, ScopeId, ScopeKind, SignalId, SignalKind, Waveform};

use crate::state::AppState;
use crate::timeline::Timeline;
use crate::wave_state::{DisplayedSignal, WaveState};

/// Width of the signal names column.
const NAMES_WIDTH: f32 = 180.0;
const RULER_HEIGHT: f32 = 24.0;
const ROW_HEIGHT: f32 = 24.0;
/// Space above and below a wave inside its row.
const ROW_PADDING: f32 = 4.0;
/// Ruler labels are at least this many pixels apart.
const TICK_SPACING: f32 = 90.0;
/// How many pixels the waves move per pixel of mouse-wheel scrolling.
const PAN_PER_SCROLL_PIXEL: f32 = 1.0;

/// Draws the Waves tab.
pub fn waves_tab(ui: &mut egui::Ui, state: &mut AppState) {
    let started = Instant::now();
    let wake = waker(ui.ctx());

    toolbar(ui, &mut state.waves);
    ui.separator();

    if state.waves.wave.is_none() {
        empty_message(ui, &state.waves);
        return;
    }

    let mut to_add: Vec<SignalId> = Vec::new();
    egui::Panel::left("wave_signal_tree")
        .resizable(true)
        .default_size(220.0)
        .show(ui, |ui| {
            if let Some(wave) = &state.waves.wave {
                signal_tree(ui, wave, &mut to_add);
            }
        });
    let dropped = wave_canvas(ui, &mut state.waves);
    to_add.extend(dropped);
    if !to_add.is_empty() {
        state.waves.add(&to_add, wake);
    }

    // Debug overlay: how long this tab took to build, for the plan's
    // "stays smooth" rule. Drawn last so it sits on top.
    let elapsed = started.elapsed();
    let corner = ui.max_rect().right_bottom() - Vec2::new(8.0, 6.0);
    ui.painter().text(
        corner,
        Align2::RIGHT_BOTTOM,
        format!("draw {:.1} ms", elapsed.as_secs_f64() * 1000.0),
        FontId::monospace(11.0),
        ui.visuals().weak_text_color(),
    );
}

fn waker(ctx: &egui::Context) -> impl Fn() + Send + 'static {
    let ctx = ctx.clone();
    move || ctx.request_repaint()
}

fn toolbar(ui: &mut egui::Ui, waves: &mut WaveState) {
    ui.horizontal(|ui| {
        match waves.path() {
            Some(path) => {
                let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
                ui.label(RichText::new(name).strong()).on_hover_text(path.display().to_string());
            }
            None => {
                ui.weak("No waveform");
            }
        }
        let has_wave = waves.wave.is_some();
        ui.separator();
        if ui
            .add_enabled(has_wave, egui::Button::new("Fit"))
            .on_hover_text("Show the whole run (F)")
            .clicked()
        {
            waves.needs_fit = true;
        }
        if ui.add_enabled(has_wave, egui::Button::new("+")).on_hover_text("Zoom in (Ctrl+scroll)").clicked() {
            waves.zoom_request = Some(2.0);
        }
        if ui.add_enabled(has_wave, egui::Button::new("−")).on_hover_text("Zoom out (Ctrl+scroll)").clicked() {
            waves.zoom_request = Some(0.5);
        }
        if waves.is_opening() {
            ui.spinner();
            ui.label("Opening…");
        }
        if let Some((done, total)) = waves.loading_progress() {
            ui.spinner();
            ui.label(format!("Loading signals {done}/{total}"));
        }
        if let Some(time) = &waves.hover_time {
            ui.separator();
            ui.monospace(time);
        }
    });
}

fn empty_message(ui: &mut egui::Ui, waves: &WaveState) {
    ui.add_space(20.0);
    if let Some(error) = &waves.error {
        ui.colored_label(ui.visuals().error_fg_color, error);
    } else if !waves.is_opening() {
        ui.label("Run a testbench to see its waveform here, or use File → Open waveform… (or drop a .fst/.vcd file on the window).");
    }
}

/// The tree of scopes and signals. Double-click a signal (or drag it onto
/// the waves) to add it; right-click a scope to add all its signals.
fn signal_tree(ui: &mut egui::Ui, wave: &Waveform, to_add: &mut Vec<SignalId>) {
    egui::ScrollArea::both().auto_shrink(false).show(ui, |ui| {
        // Instances (the testbench) start expanded; packages, which
        // libraries like UVVM add many of, start collapsed.
        for scope in wave.top_scopes() {
            let open = wave.scope(scope).kind == ScopeKind::Instance;
            scope_node(ui, wave, scope, open, to_add);
        }
    });
}

fn scope_node(ui: &mut egui::Ui, wave: &Waveform, id: ScopeId, open: bool, to_add: &mut Vec<SignalId>) {
    let scope = wave.scope(id);
    // The body is only built while the scope is expanded, so huge designs
    // cost nothing until opened.
    let response = egui::CollapsingHeader::new(&scope.name)
        .id_salt(&scope.path)
        .default_open(open)
        .show(ui, |ui| {
            for &child in &scope.scopes {
                scope_node(ui, wave, child, false, to_add);
            }
            for &signal in &scope.signals {
                signal_entry(ui, wave, signal, to_add);
            }
        });
    response.header_response.context_menu(|ui| {
        if ui.button(format!("Add all {} signals", scope.signals.len())).clicked() {
            to_add.extend(&scope.signals);
            ui.close();
        }
    });
}

fn signal_entry(ui: &mut egui::Ui, wave: &Waveform, id: SignalId, to_add: &mut Vec<SignalId>) {
    let info = wave.signal(id);
    let width = match info.kind {
        SignalKind::Logic { width } if width > 1 => format!(" [{}]", width),
        _ => String::new(),
    };
    let hover = format!("{}\n{}{}", info.path, info.vhdl_type.as_deref().unwrap_or("(type unknown)"), width);
    let label = format!("{}{width}", info.name);
    // One widget that is both clickable (double-click adds) and draggable
    // (drop onto the waves adds). egui's `dnd_drag_source` puts a separate
    // drag-only widget on top, which swallows the double-click.
    let response = ui
        .selectable_label(false, label)
        .interact(Sense::click_and_drag())
        .on_hover_text(hover);
    response.dnd_set_drag_payload(id);
    if response.dragged() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::Grabbing);
    }
    if response.double_clicked() {
        to_add.push(id);
    }
}

/// The names column, ruler and waves. Returns signals dropped onto it.
fn wave_canvas(ui: &mut egui::Ui, waves: &mut WaveState) -> Vec<SignalId> {
    let (rect, response) = ui.allocate_exact_size(ui.available_size(), Sense::click_and_drag());
    let painter = ui.painter_at(rect);
    let visuals = ui.visuals().clone();
    painter.rect_filled(rect, 0.0, visuals.extreme_bg_color);

    let names = Rect::from_min_max(rect.min, Pos2::new(rect.left() + NAMES_WIDTH, rect.bottom()));
    let wave_area = Rect::from_min_max(Pos2::new(names.right(), rect.top()), rect.max);
    let width = wave_area.width().max(1.0);

    if waves.needs_fit {
        if let Some(wave) = &waves.wave {
            waves.timeline.fit(wave.end_time(), width);
        }
        waves.needs_fit = false;
    }
    if let Some(factor) = waves.zoom_request.take() {
        waves.timeline.zoom(factor, width / 2.0);
    }
    handle_mouse_and_keys(ui, &response, waves, names, wave_area);

    draw_ruler(&painter, &visuals, &waves.timeline, wave_area);
    let rows_top = rect.top() + RULER_HEIGHT;
    painter.line_segment(
        [Pos2::new(rect.left(), rows_top), Pos2::new(rect.right(), rows_top)],
        Stroke::new(1.0, visuals.weak_text_color()),
    );
    painter.line_segment(
        [Pos2::new(names.right(), rect.top()), Pos2::new(names.right(), rect.bottom())],
        Stroke::new(1.0, visuals.weak_text_color()),
    );

    if waves.signals.is_empty() {
        painter.text(
            Pos2::new(wave_area.center().x, rows_top + 30.0),
            Align2::CENTER_TOP,
            "Double-click a signal in the tree, or drag it here",
            FontId::proportional(14.0),
            visuals.weak_text_color(),
        );
    }

    let wave = waves.wave.as_ref().expect("checked by the caller");
    let first_row = (waves.row_scroll / ROW_HEIGHT).floor().max(0.0) as usize;
    for (index, signal) in waves.signals.iter().enumerate().skip(first_row) {
        let top = rows_top + index as f32 * ROW_HEIGHT - waves.row_scroll;
        if top > rect.bottom() {
            break; // the rest are below the window
        }
        let row = Rect::from_min_max(Pos2::new(rect.left(), top), Pos2::new(rect.right(), top + ROW_HEIGHT));
        if waves.selected == Some(index) {
            painter.rect_filled(row, 0.0, visuals.selection.bg_fill.gamma_multiply(0.35));
        }
        painter.text(
            Pos2::new(names.left() + 6.0, row.center().y),
            Align2::LEFT_CENTER,
            &signal.label,
            FontId::proportional(14.0),
            visuals.text_color(),
        );
        let wave_row = Rect::from_min_max(Pos2::new(wave_area.left(), top), Pos2::new(wave_area.right(), top + ROW_HEIGHT));
        draw_signal(&painter, &visuals, wave, signal, &waves.timeline, wave_row);
    }

    // Signals dragged from the tree and dropped here.
    let mut dropped = Vec::new();
    if let Some(id) = response.dnd_release_payload::<SignalId>() {
        dropped.push(*id);
    }
    dropped
}

fn handle_mouse_and_keys(ui: &egui::Ui, response: &egui::Response, waves: &mut WaveState, names: Rect, wave_area: Rect) {
    let pointer = response.hover_pos();
    waves.hover_time = pointer
        .filter(|p| wave_area.contains(*p))
        .map(|p| waves.timeline.time_at(p.x - wave_area.left()))
        .filter(|&t| t >= 0.0)
        .map(|t| waves.timeline.format_time(t, TICK_SPACING));

    let (zoom, scroll) = ui.input(|i| (i.zoom_delta(), i.smooth_scroll_delta));
    if let Some(p) = pointer {
        if wave_area.contains(p) {
            // Ctrl+scroll (or a pinch) zooms around the mouse; plain
            // scrolling moves along in time.
            if zoom != 1.0 {
                waves.timeline.zoom(f64::from(zoom), p.x - wave_area.left());
            } else if scroll != Vec2::ZERO {
                waves.timeline.pan(-(scroll.x + scroll.y) * PAN_PER_SCROLL_PIXEL);
            }
        } else if names.contains(p) && scroll.y != 0.0 {
            // Over the names, scrolling moves up and down the signal list.
            let content = waves.signals.len() as f32 * ROW_HEIGHT;
            let visible = names.height() - RULER_HEIGHT;
            waves.row_scroll = (waves.row_scroll - scroll.y).clamp(0.0, (content - visible).max(0.0));
        }
    }

    // Dragging in the wave area pans, like grabbing the trace.
    if response.dragged_by(egui::PointerButton::Primary)
        && response.interact_pointer_pos().is_some_and(|p| wave_area.contains(p))
        && !egui::DragAndDrop::has_any_payload(ui.ctx())
    {
        waves.timeline.pan(-response.drag_delta().x);
    }

    // Clicking a row selects it.
    if response.clicked()
        && let Some(p) = response.interact_pointer_pos()
    {
        let row = ((p.y - names.top() - RULER_HEIGHT + waves.row_scroll) / ROW_HEIGHT).floor();
        waves.selected = (row >= 0.0 && (row as usize) < waves.signals.len()).then_some(row as usize);
    }
    response.context_menu(|ui| {
        let can_remove = waves.selected.is_some();
        if ui.add_enabled(can_remove, egui::Button::new("Remove signal")).clicked()
            && let Some(index) = waves.selected
        {
            waves.remove(index);
            ui.close();
        }
    });

    if response.hovered() || response.has_focus() {
        let (fit, delete) = ui.input(|i| (i.key_pressed(egui::Key::F), i.key_pressed(egui::Key::Delete)));
        if fit {
            waves.needs_fit = true;
        }
        if delete && let Some(index) = waves.selected {
            waves.remove(index);
        }
    }
}

fn draw_ruler(painter: &egui::Painter, visuals: &egui::Visuals, timeline: &Timeline, wave_area: Rect) {
    let grid = Stroke::new(1.0, visuals.weak_text_color().gamma_multiply(0.25));
    for tick in timeline.ticks(wave_area.width(), TICK_SPACING) {
        let x = wave_area.left() + tick.x;
        painter.line_segment([Pos2::new(x, wave_area.top() + RULER_HEIGHT - 6.0), Pos2::new(x, wave_area.bottom())], grid);
        painter.text(
            Pos2::new(x + 3.0, wave_area.top() + 4.0),
            Align2::LEFT_TOP,
            tick.label,
            FontId::monospace(11.0),
            visuals.weak_text_color(),
        );
    }
}

/// Wave colour: green, as on most simulators' wave windows.
fn wave_colour(visuals: &egui::Visuals) -> Color32 {
    if visuals.dark_mode {
        Color32::from_rgb(0x4c, 0xd9, 0x64)
    } else {
        Color32::from_rgb(0x1a, 0x8a, 0x2e)
    }
}

/// Draws one signal across `row` (the wave part of its row).
fn draw_signal(
    painter: &egui::Painter,
    visuals: &egui::Visuals,
    wave: &Waveform,
    signal: &DisplayedSignal,
    timeline: &Timeline,
    row: Rect,
) {
    let visible = timeline.visible(row.width());
    // Where the visible times start and end on screen (the view may
    // start before time zero).
    let x_start = row.left() + timeline.x_of(visible.start);
    let x_end = row.left() + timeline.x_of(visible.end);
    let pixels = (x_end - x_start).ceil().max(0.0) as usize;
    let Some(summary) = wave.summary(signal.id, visible, pixels) else {
        painter.text(row.left_center() + Vec2::new(6.0, 0.0), Align2::LEFT_CENTER, "loading…", FontId::proportional(12.0), visuals.weak_text_color());
        return;
    };

    let shape = Shape {
        painter,
        stroke: Stroke::new(1.5, wave_colour(visuals)),
        text_colour: visuals.strong_text_color(),
        top: row.top() + ROW_PADDING,
        bottom: row.bottom() - ROW_PADDING,
        signal,
    };
    let mut current = summary.start;
    let mut segment_start = x_start;
    for (column, bucket) in summary.buckets.into_iter().enumerate() {
        let column_x = x_start + column as f32;
        match bucket {
            Bucket::Quiet => {}
            Bucket::One(change) => {
                // The exact time, kept inside this pixel column.
                let x = (row.left() + timeline.x_of(change.time)).clamp(column_x, column_x + 1.0);
                shape.segment(current.as_ref(), segment_start, x);
                shape.transition(current.as_ref(), &change.value, x);
                current = Some(change.value);
                segment_start = x;
            }
            Bucket::Many { last_value, .. } => {
                shape.segment(current.as_ref(), segment_start, column_x);
                shape.busy_column(column_x);
                current = Some(last_value);
                segment_start = column_x + 1.0;
            }
        }
    }
    shape.segment(current.as_ref(), segment_start, x_end.min(row.right()));
}

/// Draws the pieces of one signal's wave.
struct Shape<'a> {
    painter: &'a egui::Painter,
    stroke: Stroke,
    text_colour: Color32,
    top: f32,
    bottom: f32,
    signal: &'a DisplayedSignal,
}

impl Shape<'_> {
    fn middle(&self) -> f32 {
        (self.top + self.bottom) / 2.0
    }

    /// The y coordinate a one-bit value is drawn at.
    fn bit_level(&self, value: &Value) -> f32 {
        match level_of(value) {
            Level::High => self.top,
            Level::Low => self.bottom,
            Level::Middle => self.middle(),
        }
    }

    /// A steady value from `x0` to `x1`.
    fn segment(&self, value: Option<&Value>, x0: f32, x1: f32) {
        let Some(value) = value else { return }; // no value yet
        if x1 <= x0 {
            return;
        }
        if self.signal.is_bit {
            let y = self.bit_level(value);
            self.painter.line_segment([Pos2::new(x0, y), Pos2::new(x1, y)], self.stroke);
            return;
        }
        // A bus: two lines, slightly inset so transitions show as an X.
        let (a, b) = (x0 + 2.0, x1 - 2.0);
        if b > a {
            self.painter.line_segment([Pos2::new(a, self.top), Pos2::new(b, self.top)], self.stroke);
            self.painter.line_segment([Pos2::new(a, self.bottom), Pos2::new(b, self.bottom)], self.stroke);
        }
        self.value_text(value, x0, x1);
    }

    /// The value printed in a bus segment, if there's room for it.
    fn value_text(&self, value: &Value, x0: f32, x1: f32) {
        let room = x1 - x0 - 8.0;
        if room < 12.0 {
            return;
        }
        let galley = self.painter.layout_no_wrap(format_value(value, self.signal.radix), FontId::monospace(12.0), self.text_colour);
        if galley.size().x <= room {
            let pos = Pos2::new((x0 + x1 - galley.size().x) / 2.0, self.middle() - galley.size().y / 2.0);
            self.painter.galley(pos, galley, self.text_colour);
        }
    }

    /// A change at `x`.
    fn transition(&self, old: Option<&Value>, new: &Value, x: f32) {
        if self.signal.is_bit {
            let from = old.map_or(self.bit_level(new), |old| self.bit_level(old));
            self.painter.line_segment([Pos2::new(x, from), Pos2::new(x, self.bit_level(new))], self.stroke);
        } else {
            self.painter.line_segment([Pos2::new(x - 2.0, self.top), Pos2::new(x + 2.0, self.bottom)], self.stroke);
            self.painter.line_segment([Pos2::new(x - 2.0, self.bottom), Pos2::new(x + 2.0, self.top)], self.stroke);
        }
    }

    /// A pixel column with many changes: drawn solid ("lots going on").
    fn busy_column(&self, x: f32) {
        self.painter.line_segment([Pos2::new(x + 0.5, self.top), Pos2::new(x + 0.5, self.bottom)], self.stroke);
    }
}

/// Where a one-bit wave is drawn.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Level {
    High,
    Low,
    /// Half-way: Z, X, U, W and '-' (colours per value come in step 2).
    Middle,
}

/// 1 and H are drawn high, 0 and L low (weak values like strong ones, as
/// numeric_std treats them), anything else in the middle.
fn level_of(value: &Value) -> Level {
    match value {
        Value::Logic(bits) => match bits.first().and_then(|bit: &Logic| bit.to_bit()) {
            Some(true) => Level::High,
            Some(false) => Level::Low,
            None => Level::Middle,
        },
        _ => Level::Middle,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use studio_core::parse_logic_vector;

    #[test]
    fn bit_levels() {
        let value = |text: &str| Value::Logic(parse_logic_vector(text).unwrap());
        assert_eq!(level_of(&value("1")), Level::High);
        assert_eq!(level_of(&value("H")), Level::High);
        assert_eq!(level_of(&value("0")), Level::Low);
        assert_eq!(level_of(&value("L")), Level::Low);
        for unknown in ["U", "X", "Z", "W", "-"] {
            assert_eq!(level_of(&value(unknown)), Level::Middle, "{unknown}");
        }
        assert_eq!(level_of(&Value::Text("idle".to_string())), Level::Middle);
    }
}
