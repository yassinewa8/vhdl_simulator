//! Drawing the Waves tab: the signal tree, the names column, the time
//! ruler and the waves themselves.
//!
//! The wave area is painted by hand (lines and shapes), one row per
//! signal, and only for the rows and times on screen. For each row it asks
//! the wave crate for a per-pixel summary, so drawing costs the same
//! whether a signal has ten changes or ten million.

use std::time::Instant;

use eframe::egui::{self, Align2, Color32, FontId, Pos2, Rect, RichText, Sense, Stroke, Vec2};
use studio_core::{Radix, SimTime, Value, format_value};
use wave::{Bucket, ScopeId, ScopeKind, SignalId, SignalKind, Waveform};

use crate::cursors::{CursorLine, Cursors, frequency_text, line_to_grab, snap};
use crate::state::AppState;
use crate::timeline::Timeline;
use crate::wave_list::{Drop, RowKind, RowRef};
use crate::wave_state::{DisplayedSignal, MAX_FILTER_RESULTS, WaveState};
use crate::wave_style::{Level, Look, Palette, Style, WaveColour, level_of, style_of};

/// Width of the signal names column.
const NAMES_WIDTH: f32 = 180.0;
/// Width of the column showing each signal's value at the cursor.
const VALUES_WIDTH: f32 = 110.0;
/// A click this close (in pixels) to an edge puts the cursor on the edge.
const SNAP_PIXELS: f32 = 6.0;
/// Pressing this close (in pixels) to a cursor or marker grabs it.
const GRAB_PIXELS: f32 = 5.0;
/// Cursor colours: the main cursor bright yellow, the second one cyan.
const PRIMARY_CURSOR: Color32 = Color32::from_rgb(0xff, 0xd7, 0x40);
const SECONDARY_CURSOR: Color32 = Color32::from_rgb(0x40, 0xc8, 0xe0);
const RULER_HEIGHT: f32 = 24.0;
const ROW_HEIGHT: f32 = 24.0;
/// How far signals inside a group are indented in the names column.
const GROUP_INDENT: f32 = 14.0;
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
            ui.add(
                egui::TextEdit::singleline(&mut state.waves.tree_filter)
                    .hint_text("Search signals")
                    .desired_width(f32::INFINITY),
            );
            if state.waves.tree_filter.trim().is_empty() {
                if let Some(wave) = &state.waves.wave {
                    signal_tree(ui, wave, &mut to_add);
                }
            } else {
                let matches = state.waves.filtered_signals().to_vec();
                if let Some(wave) = &state.waves.wave {
                    search_results(ui, wave, &matches, &mut to_add);
                }
            }
        });
    egui::Panel::bottom("wave_cursor_bar").show(ui, |ui| cursor_bar(ui, &mut state.waves));
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

fn waker(ctx: &egui::Context) -> impl Fn() + Send + Sync + 'static {
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
        if ui
            .add_enabled(has_wave, egui::Button::new("Reload"))
            .on_hover_text("Read the file again, keeping signals, zoom and cursors (happens by itself when it changes)")
            .clicked()
        {
            waves.reload(waker(ui.ctx()));
        }
        if waves.is_opening() {
            ui.spinner();
            ui.label("Opening…");
        }
        if !waves.missing_signals.is_empty() {
            let count = waves.missing_signals.len();
            ui.colored_label(ui.visuals().warn_fg_color, format!("{count} saved signal(s) not in this waveform"))
                .on_hover_text(waves.missing_signals.join("\n"));
        }
        if let Some((done, total)) = waves.loading_progress() {
            ui.spinner();
            ui.label(format!("Loading signals {done}/{total}"));
        }
        ui.separator();
        navigation_controls(ui, waves);
        if let Some(time) = &waves.hover_time {
            ui.separator();
            ui.monospace(time);
        }
    });
}

/// Next/previous edge and find-value controls, for the selected signal.
fn navigation_controls(ui: &mut egui::Ui, waves: &mut WaveState) {
    let selected = waves.wave.is_some() && waves.selected.is_some();
    let hint = "Select a signal first (click its row)";
    if ui
        .add_enabled(selected, egui::Button::new("Prev edge"))
        .on_hover_text("Cursor to the selected signal's previous change (Left arrow)")
        .on_disabled_hover_text(hint)
        .clicked()
    {
        let _ = waves.step_edge(false);
    }
    if ui
        .add_enabled(selected, egui::Button::new("Next edge"))
        .on_hover_text("Cursor to the selected signal's next change (Right arrow)")
        .on_disabled_hover_text(hint)
        .clicked()
    {
        let _ = waves.step_edge(true);
    }
    ui.separator();
    let field = ui.add_enabled(
        selected,
        egui::TextEdit::singleline(&mut waves.find_text)
            .hint_text("find value")
            .desired_width(90.0),
    );
    let field = field.on_hover_text(
        "A value of the selected signal, in its radix (A5 on a hex bus), or with a prefix \
         (0x1F, 0b0101), a letter for every bit (X, U, Z), or an enum name (idle). Enter finds the next.",
    );
    let enter = field.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
    if ui.add_enabled(selected, egui::Button::new("Prev")).on_disabled_hover_text(hint).clicked() {
        let _ = waves.find_value(false);
    }
    if ui.add_enabled(selected, egui::Button::new("Next")).on_disabled_hover_text(hint).clicked() || (enter && selected) {
        let _ = waves.find_value(true);
    }
    if let Some(message) = &waves.find_message {
        ui.colored_label(ui.visuals().warn_fg_color, message);
    }
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

/// Signals matching the search box, each with its path for context.
fn search_results(ui: &mut egui::Ui, wave: &Waveform, matches: &[SignalId], to_add: &mut Vec<SignalId>) {
    if matches.is_empty() {
        ui.weak("No matching signals");
        return;
    }
    if matches.len() >= MAX_FILTER_RESULTS {
        ui.weak(format!("Showing the first {MAX_FILTER_RESULTS} matches"));
    }
    egui::ScrollArea::both().auto_shrink(false).show(ui, |ui| {
        for &id in matches {
            signal_entry(ui, wave, id, to_add);
            ui.weak(wave.signal(id).path);
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

/// The parts of the wave canvas, left to right.
struct Columns {
    names: Rect,
    values: Rect,
    waves: Rect,
}

impl Columns {
    fn of(rect: Rect) -> Columns {
        let names_right = rect.left() + NAMES_WIDTH;
        let values_right = names_right + VALUES_WIDTH;
        Columns {
            names: Rect::from_min_max(rect.min, Pos2::new(names_right, rect.bottom())),
            values: Rect::from_min_max(Pos2::new(names_right, rect.top()), Pos2::new(values_right, rect.bottom())),
            waves: Rect::from_min_max(Pos2::new(values_right, rect.top()), rect.max),
        }
    }
}

/// The names column, values column, ruler and waves. Returns signals
/// dropped onto it.
fn wave_canvas(ui: &mut egui::Ui, waves: &mut WaveState) -> Vec<SignalId> {
    let (rect, response) = ui.allocate_exact_size(ui.available_size(), Sense::click_and_drag());
    let painter = ui.painter_at(rect);
    let visuals = ui.visuals().clone();
    let palette = Palette::for_theme(visuals.dark_mode);
    painter.rect_filled(rect, 0.0, visuals.extreme_bg_color);

    let columns = Columns::of(rect);
    let width = columns.waves.width().max(1.0);
    if waves.needs_fit {
        if let Some(wave) = &waves.wave {
            waves.timeline.fit(wave.end_time(), width);
        }
        waves.needs_fit = false;
    }
    if let Some(factor) = waves.zoom_request.take() {
        waves.timeline.zoom(factor, width / 2.0);
    }
    if let Some(time) = waves.centre_request.take() {
        waves.timeline.centre_on(time, width);
    }
    if let Some(time) = waves.reveal_request.take() {
        let x = waves.timeline.x_of(time);
        if !(0.0..=width).contains(&x) {
            waves.timeline.centre_on(time, width);
        }
    }
    handle_mouse_and_keys(ui, &response, waves, &columns);

    draw_ruler(&painter, &visuals, &waves.timeline, columns.waves);
    let rows_top = rect.top() + RULER_HEIGHT;
    let divider = Stroke::new(1.0, visuals.weak_text_color());
    painter.line_segment([Pos2::new(rect.left(), rows_top), Pos2::new(rect.right(), rows_top)], divider);
    for x in [columns.names.right(), columns.values.right()] {
        painter.line_segment([Pos2::new(x, rect.top()), Pos2::new(x, rect.bottom())], divider);
    }
    painter.text(
        Pos2::new(columns.values.left() + 6.0, rect.top() + 4.0),
        Align2::LEFT_TOP,
        "at cursor",
        FontId::proportional(11.0),
        visuals.weak_text_color(),
    );

    if waves.list.is_empty() {
        painter.text(
            Pos2::new(columns.waves.center().x, rows_top + 30.0),
            Align2::CENTER_TOP,
            "Double-click a signal in the tree, or drag it here",
            FontId::proportional(14.0),
            visuals.weak_text_color(),
        );
    }

    let wave = waves.wave.as_ref().expect("checked by the caller");
    let rows = waves.list.rows();
    let first_row = (waves.row_scroll / ROW_HEIGHT).floor().max(0.0) as usize;
    for (index, &row) in rows.iter().enumerate().skip(first_row) {
        let top = rows_top + index as f32 * ROW_HEIGHT - waves.row_scroll;
        if top > rect.bottom() {
            break; // the rest are below the window
        }
        let row_rect = Rect::from_min_max(Pos2::new(rect.left(), top), Pos2::new(rect.right(), top + ROW_HEIGHT));
        if waves.is_selected(row) {
            let strength = if waves.selected == Some(row) { 0.4 } else { 0.25 };
            painter.rect_filled(row_rect, 0.0, visuals.selection.bg_fill.gamma_multiply(strength));
        }
        match waves.list.kind(row) {
            Some(RowKind::Signal { signal, in_group }) => {
                let indent = if in_group { GROUP_INDENT } else { 0.0 };
                let name_colour = signal.colour.colour(visuals.dark_mode).unwrap_or(visuals.text_color());
                painter.with_clip_rect(columns.names).text(
                    Pos2::new(columns.names.left() + 6.0 + indent, row_rect.center().y),
                    Align2::LEFT_CENTER,
                    &signal.label,
                    FontId::proportional(14.0),
                    name_colour,
                );
                if let Some(cursor) = waves.cursors.primary
                    && let Some(value) = wave.value_at(signal.id, cursor)
                {
                    draw_cursor_value(&painter, &palette, &value, signal, columns.values, row_rect.center().y);
                }
                let wave_row = Rect::from_min_max(Pos2::new(columns.waves.left(), top), Pos2::new(columns.waves.right(), top + ROW_HEIGHT));
                draw_signal(&painter, &visuals, wave, signal, &waves.timeline, wave_row);
            }
            Some(RowKind::Divider(label)) => draw_divider(&painter, &visuals, row_rect, columns.names, label),
            Some(RowKind::GroupHeader { name, collapsed, count }) => {
                draw_group_header(&painter, &visuals, columns.names, row_rect, name, collapsed, count);
            }
            None => {}
        }
    }
    if let Some(drop) = waves.row_drop {
        draw_drop_guide(&painter, &visuals, &rows, drop, rect, rows_top - waves.row_scroll);
    }
    draw_cursors(&painter, &visuals, &waves.cursors, &waves.timeline, columns.waves);
    if let Some((time, row)) = waves.hover_snap {
        let top = rows_top + row as f32 * ROW_HEIGHT - waves.row_scroll;
        draw_snap_hint(&painter, columns.waves.left() + waves.timeline.x_of(time), top);
    }

    // Signals dragged from the tree and dropped here.
    let mut dropped = Vec::new();
    if let Some(id) = response.dnd_release_payload::<SignalId>() {
        dropped.push(*id);
    }
    dropped
}

/// A signal's value at the cursor, in the values column, clipped to fit.
fn draw_cursor_value(painter: &egui::Painter, palette: &Palette, value: &Value, signal: &DisplayedSignal, column: Rect, y: f32) {
    let colour = palette.text_colour(style_of(value).look);
    let text = texts_to_try(value, signal.radix).remove(0);
    let area = Rect::from_min_max(Pos2::new(column.left() + 6.0, y - ROW_HEIGHT / 2.0), Pos2::new(column.right() - 4.0, y + ROW_HEIGHT / 2.0));
    painter
        .with_clip_rect(area)
        .text(Pos2::new(area.left(), y), Align2::LEFT_CENTER, text, FontId::monospace(12.0), colour);
}

/// Where a click would snap to: a short line through the row with a dot
/// on the edge, in the cursor's colour but fainter.
fn draw_snap_hint(painter: &egui::Painter, x: f32, row_top: f32) {
    let colour = PRIMARY_CURSOR.gamma_multiply(0.7);
    painter.line_segment([Pos2::new(x, row_top), Pos2::new(x, row_top + ROW_HEIGHT)], Stroke::new(1.0, colour));
    painter.circle_filled(Pos2::new(x, row_top + ROW_HEIGHT / 2.0), 3.0, colour);
}

/// The two cursors and the markers, as vertical lines over the waves.
fn draw_cursors(painter: &egui::Painter, visuals: &egui::Visuals, cursors: &Cursors, timeline: &Timeline, area: Rect) {
    let line = |time: SimTime, stroke: Stroke, dashed: bool| {
        let x = area.left() + timeline.x_of(time);
        if !(area.left()..=area.right()).contains(&x) {
            return None;
        }
        let points = [Pos2::new(x, area.top() + RULER_HEIGHT), Pos2::new(x, area.bottom())];
        if dashed {
            painter.extend(egui::Shape::dashed_line(&points, stroke, 6.0, 4.0));
        } else {
            painter.line_segment(points, stroke);
        }
        Some(x)
    };
    for marker in &cursors.markers {
        let colour = visuals.weak_text_color();
        if let Some(x) = line(marker.time, Stroke::new(1.0, colour), true) {
            // The name sits in the ruler, on a small tag.
            let galley = painter.layout_no_wrap(marker.name.clone(), FontId::proportional(11.0), visuals.strong_text_color());
            let tag = Rect::from_min_size(Pos2::new(x - 1.0, area.top() + RULER_HEIGHT - 14.0), galley.size() + Vec2::new(6.0, 0.0));
            painter.rect_filled(tag, 2.0, visuals.widgets.inactive.bg_fill);
            painter.galley(tag.min + Vec2::new(3.0, 0.0), galley, visuals.strong_text_color());
        }
    }
    if let Some(time) = cursors.secondary {
        line(time, Stroke::new(1.0, SECONDARY_CURSOR), true);
    }
    if let Some(time) = cursors.primary {
        line(time, Stroke::new(1.5, PRIMARY_CURSOR), false);
    }
}

/// The bar under the waves: cursor times, the gap between them, markers.
fn cursor_bar(ui: &mut egui::Ui, waves: &mut WaveState) {
    // A copy of the (small) timeline, so the bar can still change `waves`.
    let timeline = waves.timeline;
    let time_text = move |time: SimTime| timeline.format_time(time.femtoseconds as f64, TICK_SPACING);
    ui.horizontal(|ui| {
        match waves.cursors.primary {
            Some(time) => ui.label(RichText::new(format!("Cursor {}", time_text(time))).color(PRIMARY_CURSOR)),
            None => ui.weak("Click the waves to place a cursor (Shift+click for a second one)"),
        };
        if let Some(time) = waves.cursors.secondary {
            ui.separator();
            ui.label(RichText::new(format!("Cursor 2 {}", time_text(time))).color(SECONDARY_CURSOR));
        }
        if let Some(delta) = waves.cursors.delta() {
            ui.separator();
            let frequency = frequency_text(delta).map(|f| format!("  (1/Δ = {f})")).unwrap_or_default();
            ui.label(RichText::new(format!("Δ {}{frequency}", time_text(delta))).strong());
        }
        ui.separator();
        if ui
            .add_enabled(waves.cursors.primary.is_some(), egui::Button::new("Add marker"))
            .on_hover_text("Mark the cursor's time (M)")
            .clicked()
            && let Some(time) = waves.cursors.primary
        {
            waves.cursors.add_marker(time);
        }
        let count = waves.cursors.markers.len();
        ui.add_enabled_ui(count > 0, |ui| {
            ui.menu_button(format!("Markers ({count})"), |ui| markers_menu(ui, waves));
        });
    });
}

/// Each marker with its name (editable), time, and Go / delete buttons.
fn markers_menu(ui: &mut egui::Ui, waves: &mut WaveState) {
    let mut go_to = None;
    let mut delete = None;
    let timeline = waves.timeline;
    for (index, marker) in waves.cursors.markers.iter_mut().enumerate() {
        ui.horizontal(|ui| {
            ui.add(egui::TextEdit::singleline(&mut marker.name).desired_width(90.0));
            ui.monospace(timeline.format_time(marker.time.femtoseconds as f64, TICK_SPACING));
            if ui.button("Go").on_hover_text("Move the cursor here and centre the view").clicked() {
                go_to = Some(marker.time);
            }
            // A word rather than a symbol: egui's built-in font has no "✕".
            if ui.button("Delete").clicked() {
                delete = Some(index);
            }
        });
    }
    if let Some(time) = go_to {
        waves.cursors.primary = Some(time);
        waves.centre_request = Some(time);
        ui.close();
    }
    if let Some(index) = delete {
        waves.cursors.remove_marker(index);
    }
}

/// A divider row: a line across, with its label (if any) at the left.
fn draw_divider(painter: &egui::Painter, visuals: &egui::Visuals, row: Rect, names: Rect, label: &str) {
    let y = row.center().y;
    let stroke = Stroke::new(1.0, visuals.weak_text_color());
    let line_start = if label.is_empty() {
        row.left() + 4.0
    } else {
        let galley = painter.layout_no_wrap(label.to_string(), FontId::proportional(12.0), visuals.text_color());
        let width = galley.size().x;
        painter.with_clip_rect(names).galley(Pos2::new(row.left() + 6.0, y - galley.size().y / 2.0), galley, visuals.text_color());
        row.left() + 12.0 + width
    };
    painter.line_segment([Pos2::new(line_start, y), Pos2::new(row.right() - 4.0, y)], stroke);
}

/// A group's header: a triangle (pointing right when collapsed, down when
/// open), the name, and how many signals it holds. The triangle is drawn
/// rather than typed, as egui's built-in font lacks some arrow symbols.
fn draw_group_header(painter: &egui::Painter, visuals: &egui::Visuals, names: Rect, row: Rect, name: &str, collapsed: bool, count: usize) {
    let centre = Pos2::new(row.left() + 12.0, row.center().y);
    let s = 4.0;
    let points = if collapsed {
        vec![centre + Vec2::new(-s / 2.0, -s), centre + Vec2::new(s, 0.0), centre + Vec2::new(-s / 2.0, s)]
    } else {
        vec![centre + Vec2::new(-s, -s / 2.0), centre + Vec2::new(s, -s / 2.0), centre + Vec2::new(0.0, s)]
    };
    painter.add(egui::Shape::convex_polygon(points, visuals.text_color(), Stroke::NONE));
    painter.with_clip_rect(names).text(
        Pos2::new(row.left() + 22.0, row.center().y),
        Align2::LEFT_CENTER,
        format!("{name}  ({count})"),
        FontId::proportional(14.0),
        visuals.strong_text_color(),
    );
    // A faint band across the waves, so the group's start is easy to see.
    let band = Rect::from_min_max(Pos2::new(names.right(), row.top() + 2.0), Pos2::new(row.right(), row.bottom() - 2.0));
    painter.rect_filled(band, 0.0, visuals.widgets.inactive.bg_fill.gamma_multiply(0.4));
}

/// Where a dragged row will land: a line in the gap, or an outline around
/// the group it will join.
fn draw_drop_guide(painter: &egui::Painter, visuals: &egui::Visuals, rows: &[RowRef], drop: Drop, rect: Rect, rows_origin: f32) {
    let colour = visuals.selection.stroke.color;
    match drop {
        Drop::Before(before) => {
            let gap = before.and_then(|b| rows.iter().position(|&r| r == b)).unwrap_or(rows.len());
            let y = rows_origin + gap as f32 * ROW_HEIGHT;
            painter.line_segment([Pos2::new(rect.left(), y), Pos2::new(rect.right(), y)], Stroke::new(2.0, colour));
        }
        Drop::IntoGroup(item) => {
            if let Some(index) = rows.iter().position(|&r| r == RowRef::top(item)) {
                let top = rows_origin + index as f32 * ROW_HEIGHT;
                let outline = Rect::from_min_max(Pos2::new(rect.left() + 1.0, top), Pos2::new(rect.right() - 1.0, top + ROW_HEIGHT));
                painter.rect_stroke(outline, 2.0, Stroke::new(2.0, colour), egui::StrokeKind::Inside);
            }
        }
    }
}

/// The on-screen row (counting from the top of the list) at height `y`.
fn row_index_at(waves: &WaveState, columns: &Columns, y: f32) -> Option<usize> {
    let row = ((y - columns.names.top() - RULER_HEIGHT + waves.row_scroll) / ROW_HEIGHT).floor();
    (row >= 0.0 && (row as usize) < waves.list.rows().len()).then_some(row as usize)
}

/// The row at height `y`.
fn row_at(waves: &WaveState, columns: &Columns, y: f32) -> Option<RowRef> {
    row_index_at(waves, columns, y).map(|index| waves.list.rows()[index])
}

/// The right-click menu for the selected row.
fn row_menu(ui: &mut egui::Ui, waves: &mut WaveState) {
    if waves.selection.len() > 1 {
        return several_rows_menu(ui, waves);
    }
    let selected = waves.selected;
    let on_a_row = selected.and_then(|row| waves.list.kind(row)).is_some();
    match selected.and_then(|row| waves.list.kind(row)) {
        Some(RowKind::Signal { .. }) => signal_menu(ui, waves, selected.expect("checked")),
        Some(RowKind::Divider(_)) => {
            let row = selected.expect("checked");
            ui.label("Label:");
            if let Some(label) = waves.list.title_mut(row) {
                ui.text_edit_singleline(label);
            }
            if ui.button("Remove divider").clicked() {
                waves.remove(row);
                ui.close();
            }
        }
        Some(RowKind::GroupHeader { collapsed, .. }) => {
            let row = selected.expect("checked");
            ui.label("Group name:");
            if let Some(name) = waves.list.title_mut(row) {
                ui.text_edit_singleline(name);
            }
            if ui.button(if collapsed { "Expand" } else { "Collapse" }).clicked() {
                waves.list.toggle_collapsed(row.item);
                ui.close();
            }
            if ui.button("Ungroup").on_hover_text("Keep the signals, remove the group").clicked() {
                waves.list.ungroup(row.item);
                waves.selected = None;
                ui.close();
            }
            if ui.button("Remove group and its signals").clicked() {
                waves.remove(row);
                ui.close();
            }
        }
        None => {}
    }
    // The row's own items above, the general ones below.
    if on_a_row {
        ui.separator();
    }
    if ui.button("Add divider").on_hover_text("A labelled line between rows").clicked() {
        waves.selected = Some(waves.list.add_divider_after(selected));
        ui.close();
    }
    if ui.button("New group").on_hover_text("Then drag signals onto its header").clicked() {
        waves.selected = Some(waves.list.add_group_before(selected));
        ui.close();
    }
    if ui.add_enabled(waves.cursors.secondary.is_some(), egui::Button::new("Clear second cursor")).clicked() {
        waves.cursors.secondary = None;
        ui.close();
    }
}

/// The menu when several rows are selected.
fn several_rows_menu(ui: &mut egui::Ui, waves: &mut WaveState) {
    let count = waves.selection.len();
    ui.weak(format!("{count} rows selected"));
    if ui.button(format!("Group {count} selected")).on_hover_text("Ctrl+G").clicked() {
        waves.group_selection();
        ui.close();
    }
    if ui.button(format!("Remove {count} selected")).on_hover_text("Delete").clicked() {
        waves.remove_selection();
        ui.close();
    }
}

/// Radix, colour, name and removal for one signal row.
fn signal_menu(ui: &mut egui::Ui, waves: &mut WaveState, row: RowRef) {
    let Some(signal) = waves.list.signal_mut(row) else { return };
    ui.label("Name:");
    ui.text_edit_singleline(&mut signal.label);
    if signal.label != signal.original_name() && ui.button("Reset name").clicked() {
        signal.label = signal.original_name().to_string();
    }
    ui.weak(&signal.path);
    ui.separator();
    // Radix only matters for buses; enums, reals and single bits show
    // their values as they are.
    if !signal.is_bit {
        ui.menu_button(format!("Radix: {}", signal.radix.name()), |ui| {
            for radix in Radix::ALL {
                if ui.radio_value(&mut signal.radix, radix, radix.name()).clicked() {
                    ui.close();
                }
            }
        });
    }
    ui.menu_button(format!("Colour: {}", signal.colour.name()), |ui| {
        for colour in WaveColour::ALL {
            if ui.radio_value(&mut signal.colour, colour, colour.name()).clicked() {
                ui.close();
            }
        }
    });
    if ui.button("Remove signal").clicked() {
        waves.remove(row);
        ui.close();
    }
}

fn handle_mouse_and_keys(ui: &egui::Ui, response: &egui::Response, waves: &mut WaveState, columns: &Columns) {
    let wave_area = columns.waves;
    let pointer = response.hover_pos();
    waves.hover_time = pointer
        .filter(|p| wave_area.contains(*p))
        .map(|p| waves.timeline.time_at(p.x - wave_area.left()))
        .filter(|&t| t >= 0.0)
        .map(|t| waves.timeline.format_time(t, TICK_SPACING));

    // Near an edge of the signal under the mouse: remember where a click
    // would snap to, so it can be shown before clicking.
    waves.hover_snap = None;
    if let Some(p) = pointer.filter(|p| wave_area.contains(*p))
        && waves.dragging.is_none()
        && let Some(index) = row_index_at(waves, columns, p.y)
    {
        let raw = SimTime::from_femtoseconds(waves.timeline.time_at(p.x - wave_area.left()).max(0.0) as u64);
        let snapped = snapped_time(waves, Some(waves.list.rows()[index]), raw);
        if snapped != raw {
            waves.hover_snap = Some((snapped, index));
            waves.hover_time = Some(waves.timeline.format_time(snapped.femtoseconds as f64, TICK_SPACING));
        }
    }

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
        } else if scroll.y != 0.0 {
            // Over the names and values, scrolling moves up and down the list.
            let content = waves.list.rows().len() as f32 * ROW_HEIGHT;
            let visible = columns.names.height() - RULER_HEIGHT;
            waves.row_scroll = (waves.row_scroll - scroll.y).clamp(0.0, (content - visible).max(0.0));
        }
    }

    drag_rows(ui, response, waves, columns);
    drag_lines_or_pan(ui, response, waves, columns);

    // A click selects the row; in the waves it also places a cursor
    // (Shift: the second cursor), snapped to a nearby edge of that row.
    if response.clicked()
        && let Some(p) = response.interact_pointer_pos()
    {
        let row = row_at(waves, columns, p.y);
        let (ctrl, shift) = ui.input(|i| (i.modifiers.command, i.modifiers.shift));
        let in_waves = wave_area.contains(p);
        // Ctrl+click adds to the selection; in the names, Shift+click
        // selects a range (in the waves, Shift+click places cursor 2).
        match row {
            Some(row) if ctrl => waves.toggle_selected(row),
            Some(row) if shift && !in_waves => waves.select_range_to(row),
            _ if shift && in_waves => {}
            _ => waves.select_only(row),
        }
        // Clicking a group's name collapses or expands it.
        if let Some(row) = row
            && !in_waves
            && !ctrl
            && !shift
            && matches!(waves.list.kind(row), Some(RowKind::GroupHeader { .. }))
        {
            waves.list.toggle_collapsed(row.item);
        }
        let clicked = waves.timeline.time_at(p.x - wave_area.left());
        if in_waves && clicked >= 0.0 && !ctrl {
            let time = snapped_time(waves, row, SimTime::from_femtoseconds(clicked as u64));
            if shift {
                waves.cursors.secondary = Some(time);
            } else {
                waves.cursors.primary = Some(time);
            }
        }
    }
    // Right-clicking a row outside the selection selects just that row;
    // inside it, the selection stays (so the menu can act on all of it).
    if response.secondary_clicked()
        && let Some(p) = response.interact_pointer_pos()
    {
        let row = row_at(waves, columns, p.y);
        if row.is_none_or(|row| !waves.is_selected(row)) {
            waves.select_only(row);
        }
    }
    response.context_menu(|ui| row_menu(ui, waves));

    if response.hovered() || response.has_focus() {
        let (fit, delete, marker, left, right, group) = ui.input(|i| {
            (
                i.key_pressed(egui::Key::F),
                i.key_pressed(egui::Key::Delete),
                i.key_pressed(egui::Key::M) && !i.modifiers.command,
                i.key_pressed(egui::Key::ArrowLeft),
                i.key_pressed(egui::Key::ArrowRight),
                i.modifiers.command && i.key_pressed(egui::Key::G),
            )
        });
        if group {
            waves.group_selection();
        }
        if left {
            let _ = waves.step_edge(false);
        }
        if right {
            let _ = waves.step_edge(true);
        }
        if fit {
            waves.needs_fit = true;
        }
        if delete {
            waves.remove_selection();
        }
        if marker && let Some(time) = waves.cursors.primary {
            waves.cursors.add_marker(time);
        }
    }
}

/// Dragging in the names or values column reorders rows: a guide shows
/// where the row will land, and it moves when let go.
fn drag_rows(ui: &egui::Ui, response: &egui::Response, waves: &mut WaveState, columns: &Columns) {
    let pressed_in_names = ui
        .input(|i| i.pointer.press_origin())
        .filter(|p| p.x < columns.waves.left() && !egui::DragAndDrop::has_any_payload(ui.ctx()));
    if response.drag_started_by(egui::PointerButton::Primary)
        && let Some(origin) = pressed_in_names
    {
        waves.row_drag = row_at(waves, columns, origin.y);
        if let Some(row) = waves.row_drag
            && !waves.is_selected(row)
        {
            waves.select_only(Some(row));
        }
    }
    let Some(moving) = waves.row_drag else { return };
    ui.ctx().set_cursor_icon(egui::CursorIcon::Grabbing);
    if let Some(p) = response.interact_pointer_pos() {
        let position = (p.y - columns.names.top() - RULER_HEIGHT + waves.row_scroll) / ROW_HEIGHT;
        waves.row_drop = Some(waves.list.drop_target(moving, position));
    }
    if response.drag_stopped() {
        if let Some(drop) = waves.row_drop.take() {
            waves.move_dragged(moving, drop);
        }
        waves.row_drag = None;
    }
}

/// Dragging in the waves: a press on (or near) a cursor or marker drags
/// that line, which snaps to a nearby edge when let go; a press anywhere
/// else pans, like grabbing the trace.
fn drag_lines_or_pan(ui: &egui::Ui, response: &egui::Response, waves: &mut WaveState, columns: &Columns) {
    let wave_area = columns.waves;
    // The lines' screen positions, for grabbing.
    let lines: Vec<(CursorLine, f32)> = waves
        .cursors
        .lines()
        .into_iter()
        .map(|(line, time)| (line, wave_area.left() + waves.timeline.x_of(time)))
        .collect();
    let grab_at = |x: f32| line_to_grab(&lines, x, GRAB_PIXELS);

    // Show a left-right arrow when a line can be grabbed (or is held).
    let hovering_line = response.hover_pos().filter(|p| wave_area.contains(*p)).and_then(|p| grab_at(p.x));
    if hovering_line.is_some() || waves.dragging.is_some() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeHorizontal);
    }

    let pressed_in_waves = ui
        .input(|i| i.pointer.press_origin())
        .filter(|p| wave_area.contains(*p) && !egui::DragAndDrop::has_any_payload(ui.ctx()));
    if response.drag_started_by(egui::PointerButton::Primary)
        && let Some(origin) = pressed_in_waves
    {
        waves.dragging = grab_at(origin.x);
    }
    let pointer = response.interact_pointer_pos();
    let time_at = |waves: &WaveState, x: f32| SimTime::from_femtoseconds(waves.timeline.time_at(x - wave_area.left()).max(0.0) as u64);

    if response.dragged_by(egui::PointerButton::Primary) && pressed_in_waves.is_some() {
        match (waves.dragging, pointer) {
            (Some(line), Some(p)) => {
                // Snap live: the line jumps onto an edge of the signal
                // under the mouse as soon as it comes close to one.
                let row = row_at(waves, columns, p.y);
                let time = snapped_time(waves, row, time_at(waves, p.x));
                waves.cursors.set_time(line, time);
            }
            (None, _) => waves.timeline.pan(-response.drag_delta().x),
            (Some(_), None) => {}
        }
    }
    if response.drag_stopped()
        && let Some(line) = waves.dragging.take()
        && let Some(p) = pointer
    {
        // Let go near an edge of the signal under the mouse: snap onto it.
        let row = row_at(waves, columns, p.y);
        let time = snapped_time(waves, row, time_at(waves, p.x));
        waves.cursors.set_time(line, time);
    }
}

/// `clicked`, moved onto the nearest edge of the signal in `row` if one is
/// within a few pixels.
fn snapped_time(waves: &WaveState, row: Option<RowRef>, clicked: SimTime) -> SimTime {
    let (Some(wave), Some(signal)) = (&waves.wave, row.and_then(|row| waves.list.signal(row))) else {
        return clicked;
    };
    let id = signal.id;
    // A change exactly at the click counts as "before" it.
    let before = wave.prev_change(id, SimTime::from_femtoseconds(clicked.femtoseconds + 1));
    let after = wave.next_change(id, clicked);
    let tolerance = SimTime::from_femtoseconds((waves.timeline.time_at(SNAP_PIXELS) - waves.timeline.time_at(0.0)) as u64);
    snap(clicked, before, after, tolerance)
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
        palette: Palette::for_theme(visuals.dark_mode).with_signal_colour(signal.colour, visuals.dark_mode),
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
                shape.busy_column(column_x, &last_value);
                current = Some(last_value);
                segment_start = column_x + 1.0;
            }
        }
    }
    shape.segment(current.as_ref(), segment_start, x_end.min(row.right()));
}

/// Line width of the waves.
const WAVE_WIDTH: f32 = 1.5;
/// How strongly unknown values fill their area (0 = not at all, 1 = solid).
const UNKNOWN_FILL: f32 = 0.3;

/// Draws the pieces of one signal's wave.
struct Shape<'a> {
    painter: &'a egui::Painter,
    palette: Palette,
    top: f32,
    bottom: f32,
    signal: &'a DisplayedSignal,
}

impl Shape<'_> {
    fn middle(&self) -> f32 {
        (self.top + self.bottom) / 2.0
    }

    fn stroke(&self, look: Look) -> Stroke {
        Stroke::new(WAVE_WIDTH, self.palette.colour(look))
    }

    /// The lowest and highest y a one-bit value covers.
    fn bit_extent(&self, value: &Value) -> (f32, f32) {
        match level_of(value) {
            Level::High => (self.top, self.top),
            Level::Low => (self.bottom, self.bottom),
            Level::Middle => (self.middle(), self.middle()),
            Level::Block => (self.top, self.bottom),
        }
    }

    /// A steady value from `x0` to `x1`.
    fn segment(&self, value: Option<&Value>, x0: f32, x1: f32) {
        let Some(value) = value else { return }; // no value yet
        if x1 <= x0 {
            return;
        }
        let style = style_of(value);
        if self.signal.is_bit {
            self.bit_segment(value, style.look, x0, x1);
        } else {
            self.bus_segment(value, style, x0, x1);
        }
    }

    fn bit_segment(&self, value: &Value, look: Look, x0: f32, x1: f32) {
        let stroke = self.stroke(look);
        match level_of(value) {
            Level::Block => self.block(look, x0, x1),
            level => {
                let y = self.bit_extent(value).0;
                let line = [Pos2::new(x0, y), Pos2::new(x1, y)];
                if look == Look::Weak && level != Level::Middle {
                    // L and H: dashed, so a pull-up looks different from a drive.
                    self.painter.extend(egui::Shape::dashed_line(&line, stroke, 4.0, 3.0));
                } else {
                    self.painter.line_segment(line, stroke);
                }
            }
        }
    }

    /// A filled box between the top and bottom: how X, U, W and '-' look.
    fn block(&self, look: Look, x0: f32, x1: f32) {
        let colour = self.palette.colour(look);
        let area = Rect::from_min_max(Pos2::new(x0, self.top), Pos2::new(x1, self.bottom));
        self.painter.rect_filled(area, 0.0, colour.gamma_multiply(UNKNOWN_FILL));
        let stroke = self.stroke(look);
        self.painter.line_segment([area.left_top(), area.right_top()], stroke);
        self.painter.line_segment([area.left_bottom(), area.right_bottom()], stroke);
    }

    fn bus_segment(&self, value: &Value, style: Style, x0: f32, x1: f32) {
        if style.all_high_z {
            // A released bus: a mid-level line, like a single Z.
            let y = self.middle();
            self.painter.line_segment([Pos2::new(x0, y), Pos2::new(x1, y)], self.stroke(Look::HighZ));
        } else {
            // Two lines, inset so transitions show as a cross; tinted when
            // the value has unknown bits.
            let (a, b) = (x0 + 2.0, x1 - 2.0);
            if b > a {
                if style.look.is_block() {
                    let area = Rect::from_min_max(Pos2::new(a, self.top), Pos2::new(b, self.bottom));
                    self.painter.rect_filled(area, 0.0, self.palette.colour(style.look).gamma_multiply(UNKNOWN_FILL));
                }
                let stroke = self.stroke(style.look);
                self.painter.line_segment([Pos2::new(a, self.top), Pos2::new(b, self.top)], stroke);
                self.painter.line_segment([Pos2::new(a, self.bottom), Pos2::new(b, self.bottom)], stroke);
            }
        }
        self.value_text(value, style.look, x0, x1);
    }

    /// The value printed in a bus segment, if there's room for all of it.
    fn value_text(&self, value: &Value, look: Look, x0: f32, x1: f32) {
        let room = x1 - x0 - 8.0;
        if room < 8.0 {
            return;
        }
        let colour = self.palette.text_colour(look);
        for text in texts_to_try(value, self.signal.radix) {
            let galley = self.painter.layout_no_wrap(text, FontId::monospace(12.0), colour);
            if galley.size().x <= room {
                let pos = Pos2::new((x0 + x1 - galley.size().x) / 2.0, self.middle() - galley.size().y / 2.0);
                self.painter.galley(pos, galley, colour);
                return;
            }
        }
    }

    /// A change at `x`, in the colour of the new value.
    fn transition(&self, old: Option<&Value>, new: &Value, x: f32) {
        let stroke = self.stroke(style_of(new).look);
        if self.signal.is_bit {
            // A vertical line covering both the old and the new level.
            let (new_top, new_bottom) = self.bit_extent(new);
            let (old_top, old_bottom) = old.map_or((new_top, new_bottom), |old| self.bit_extent(old));
            let (from, to) = (new_top.min(old_top), new_bottom.max(old_bottom));
            self.painter.line_segment([Pos2::new(x, from), Pos2::new(x, to)], stroke);
        } else {
            self.painter.line_segment([Pos2::new(x - 2.0, self.top), Pos2::new(x + 2.0, self.bottom)], stroke);
            self.painter.line_segment([Pos2::new(x - 2.0, self.bottom), Pos2::new(x + 2.0, self.top)], stroke);
        }
    }

    /// A pixel column with many changes: drawn solid ("lots going on"),
    /// in the colour of the value it ends on.
    fn busy_column(&self, x: f32, last_value: &Value) {
        let stroke = self.stroke(style_of(last_value).look);
        self.painter.line_segment([Pos2::new(x + 0.5, self.top), Pos2::new(x + 0.5, self.bottom)], stroke);
    }
}

/// The texts to try printing for a bus value, best first. Hex or decimal
/// can only show `?` for a digit that mixes unknown and known bits, which
/// hides which bits are bad, so then the bits themselves are tried first
/// (`01XZWLH-` rather than `??`), falling back to the chosen radix.
fn texts_to_try(value: &Value, radix: Radix) -> Vec<String> {
    let chosen = format_value(value, radix);
    if chosen.contains('?') && radix != Radix::Binary {
        vec![format_value(value, Radix::Binary), chosen]
    } else {
        vec![chosen]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use studio_core::parse_logic_vector;

    fn logic(text: &str) -> Value {
        Value::Logic(parse_logic_vector(text).unwrap())
    }

    #[test]
    fn unclear_digits_show_the_bits_instead() {
        assert_eq!(texts_to_try(&logic("01XZWLH-"), Radix::Hex), ["01XZWLH-", "??"]);
        assert_eq!(texts_to_try(&logic("XXXX0101"), Radix::Hex), ["X5"], "a whole-digit X is clear enough");
        assert_eq!(texts_to_try(&logic("10100101"), Radix::Hex), ["A5"]);
        assert_eq!(texts_to_try(&logic("01X1"), Radix::Unsigned), ["01X1", "?"]);
    }
}
