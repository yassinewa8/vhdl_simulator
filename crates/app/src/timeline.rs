//! Which slice of simulation time the wave view shows, and how times map
//! to pixels: like the horizontal timebase of an oscilloscope.
//!
//! No egui here, so the zoom and ruler arithmetic can be tested.

use studio_core::SimTime;

/// Most zoomed in: one femtosecond across this many pixels.
const MAX_PIXELS_PER_FEMTOSECOND: f64 = 50.0;

/// The visible time range, kept as a start time and a scale.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Timeline {
    /// Time at the left edge of the wave area, in femtoseconds.
    start: f64,
    /// Femtoseconds per pixel: bigger is more zoomed out.
    scale: f64,
}

impl Default for Timeline {
    /// 0 to 100 ns over 1000 pixels, until a waveform is fitted.
    fn default() -> Timeline {
        Timeline {
            start: 0.0,
            scale: 100_000.0,
        }
    }
}

/// One mark on the time ruler.
#[derive(Debug, Clone, PartialEq)]
pub struct Tick {
    /// Pixels from the left edge of the wave area.
    pub x: f32,
    pub label: String,
}

impl Timeline {
    /// The time at `x` pixels from the left edge.
    pub fn time_at(&self, x: f32) -> f64 {
        self.start + f64::from(x) * self.scale
    }

    /// Pixels from the left edge for `time` (may be off-screen).
    pub fn x_of(&self, time: SimTime) -> f32 {
        ((time.femtoseconds as f64 - self.start) / self.scale) as f32
    }

    /// The visible range for a wave area `width` pixels wide, clipped to
    /// times that exist (no negative times).
    pub fn visible(&self, width: f32) -> std::ops::Range<SimTime> {
        let start = self.start.max(0.0);
        let end = self.time_at(width).max(start);
        SimTime::from_femtoseconds(start as u64)..SimTime::from_femtoseconds(end.ceil() as u64)
    }

    /// A timeline starting at `start` femtoseconds with `scale`
    /// femtoseconds per pixel (as saved in a layout). Nonsense values
    /// (zero, negative, not a number) fall back to the default view.
    pub fn from_parts(start: f64, scale: f64) -> Timeline {
        if start.is_finite() && scale.is_finite() && scale > 0.0 {
            let mut timeline = Timeline {
                start,
                scale: scale.max(1.0 / MAX_PIXELS_PER_FEMTOSECOND),
            };
            timeline.clamp_start();
            timeline
        } else {
            Timeline::default()
        }
    }

    /// Time at the left edge, in femtoseconds.
    pub fn start(&self) -> f64 {
        self.start
    }

    /// Femtoseconds per pixel.
    pub fn scale(&self) -> f64 {
        self.scale
    }

    /// Shows `0..end` across `width` pixels, with a small margin on the right.
    pub fn fit(&mut self, end: SimTime, width: f32) {
        let end = (end.femtoseconds as f64).max(1.0);
        self.start = 0.0;
        self.scale = (end * 1.02 / f64::from(width.max(1.0))).max(1.0 / MAX_PIXELS_PER_FEMTOSECOND);
    }

    /// Zooms by `factor` (above 1 zooms in), keeping the time under the
    /// mouse at `anchor_x` where it is.
    pub fn zoom(&mut self, factor: f64, anchor_x: f32) {
        if factor <= 0.0 {
            return;
        }
        let anchor_time = self.time_at(anchor_x);
        self.scale = (self.scale / factor).max(1.0 / MAX_PIXELS_PER_FEMTOSECOND);
        self.start = anchor_time - f64::from(anchor_x) * self.scale;
        self.clamp_start();
    }

    /// Moves the view so `time` is in the middle of a `width`-pixel area,
    /// keeping the zoom.
    pub fn centre_on(&mut self, time: SimTime, width: f32) {
        self.start = time.femtoseconds as f64 - f64::from(width) / 2.0 * self.scale;
        self.clamp_start();
    }

    /// Moves the view by `pixels` (positive moves to later times).
    pub fn pan(&mut self, pixels: f32) {
        self.start += f64::from(pixels) * self.scale;
        self.clamp_start();
    }

    /// Never show more than half a screen of negative time: there's
    /// nothing there, but a little room helps see the first edge.
    fn clamp_start(&mut self) {
        self.start = self.start.max(-100.0 * self.scale);
    }

    /// Ruler marks for a wave area `width` pixels wide, at least
    /// `min_spacing` pixels apart, at round 1-2-5 steps.
    pub fn ticks(&self, width: f32, min_spacing: f32) -> Vec<Tick> {
        let step = nice_step(self.scale * f64::from(min_spacing));
        let unit = label_unit(step);
        let first = (self.start.max(0.0) / step).ceil() as u64;
        let last = (self.time_at(width) / step).floor().max(0.0) as u64;
        (first..=last)
            .map(|n| {
                let time = n as f64 * step;
                Tick {
                    x: ((time - self.start) / self.scale) as f32,
                    label: format_tick(time, unit),
                }
            })
            .collect()
    }
}

impl Timeline {
    /// `time` (femtoseconds) as text, as precise as one pixel at the
    /// current zoom, in the unit the ruler uses: e.g. `4.13 ns`.
    pub fn format_time(&self, time: f64, tick_spacing: f32) -> String {
        let (name, size) = label_unit(nice_step(self.scale * f64::from(tick_spacing)));
        // Decimals needed so one pixel changes the last digit.
        let decimals = (size / self.scale).log10().ceil().clamp(0.0, 6.0) as usize;
        format!("{:.*} {name}", decimals, time / size)
    }
}

/// The smallest step of 1, 2 or 5 times a power of ten that is at least
/// `at_least` femtoseconds.
fn nice_step(at_least: f64) -> f64 {
    let at_least = at_least.max(1.0);
    let mut power = 1.0;
    while power * 10.0 <= at_least {
        power *= 10.0;
    }
    [1.0, 2.0, 5.0, 10.0]
        .into_iter()
        .map(|m| m * power)
        .find(|&step| step >= at_least)
        .expect("10 x power is always enough")
}

/// Time units for labels: (name, femtoseconds).
const UNITS: [(&str, f64); 6] = [
    ("s", 1e15),
    ("ms", 1e12),
    ("us", 1e9),
    ("ns", 1e6),
    ("ps", 1e3),
    ("fs", 1.0),
];

/// The unit to label ticks `step` apart in: the largest unit no bigger
/// than the step, so every label is a whole number (500 ps, 100 ns).
fn label_unit(step: f64) -> (&'static str, f64) {
    UNITS
        .into_iter()
        .find(|&(_, size)| step >= size)
        .unwrap_or(("fs", 1.0))
}

fn format_tick(time: f64, (name, size): (&str, f64)) -> String {
    format!("{} {name}", (time / size).round())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ns(value: u64) -> SimTime {
        SimTime::from_femtoseconds(value * 1_000_000)
    }

    #[test]
    fn fit_shows_the_whole_run_with_a_margin() {
        let mut timeline = Timeline::default();
        timeline.fit(ns(1000), 1000.0);
        assert_eq!(timeline.x_of(SimTime::ZERO), 0.0);
        // 1000 ns plus a 2% margin over 1000 px: 1.02 ns per pixel, so
        // 1000 ns is at 1000 / 1.02 = 980.4 px.
        let end_x = timeline.x_of(ns(1000));
        assert!((end_x - 980.4).abs() < 0.1, "{end_x}");
    }

    #[test]
    fn zoom_keeps_the_time_under_the_mouse() {
        let mut timeline = Timeline::default();
        timeline.fit(ns(1000), 1000.0);
        let before = timeline.time_at(300.0);
        timeline.zoom(4.0, 300.0);
        assert!((timeline.time_at(300.0) - before).abs() < 1.0);
        // 1.02 ns (1.02e6 fs) per pixel, zoomed in 4 times.
        assert!((timeline.scale() - 1.02e6 / 4.0).abs() < 1.0);
        timeline.zoom(0.25, 300.0);
        assert!((timeline.time_at(300.0) - before).abs() < 1.0);
    }

    #[test]
    fn zoom_has_a_limit() {
        let mut timeline = Timeline::default();
        timeline.zoom(1e30, 0.0);
        assert_eq!(timeline.scale(), 1.0 / MAX_PIXELS_PER_FEMTOSECOND);
    }

    #[test]
    fn pan_moves_by_pixels_but_not_far_before_zero() {
        let mut timeline = Timeline::default(); // 100 ps per pixel
        timeline.pan(10.0);
        assert_eq!(timeline.time_at(0.0), 1_000_000.0);
        timeline.pan(-1_000_000.0);
        assert_eq!(timeline.time_at(100.0), 0.0, "stops 100 px before zero");
    }

    #[test]
    fn centre_on_keeps_the_zoom() {
        let mut timeline = Timeline::default(); // 100 ps per pixel
        timeline.centre_on(ns(500), 1000.0);
        assert_eq!(timeline.x_of(ns(500)), 500.0);
        assert_eq!(timeline.time_at(1.0) - timeline.time_at(0.0), 100_000.0);
    }

    #[test]
    fn visible_range_never_starts_before_zero() {
        let mut timeline = Timeline::default();
        timeline.pan(-50.0);
        let visible = timeline.visible(1000.0);
        assert_eq!(visible.start, SimTime::ZERO);
        assert_eq!(visible.end, SimTime::from_femtoseconds(95_000_000));
    }

    #[test]
    fn nice_steps_are_1_2_5() {
        assert_eq!(nice_step(1.0), 1.0);
        assert_eq!(nice_step(1.5), 2.0);
        assert_eq!(nice_step(3.0), 5.0);
        assert_eq!(nice_step(7.0), 10.0);
        assert_eq!(nice_step(120.0), 200.0);
        assert_eq!(nice_step(4_000_000.0), 5_000_000.0);
    }

    #[test]
    fn tick_labels_use_a_sensible_unit() {
        assert_eq!(format_tick(500_000.0, label_unit(500_000.0)), "500 ps");
        assert_eq!(format_tick(2_000_000.0, label_unit(500_000.0)), "2000 ps");
        assert_eq!(format_tick(2_000_000.0, label_unit(1_000_000.0)), "2 ns");
        assert_eq!(format_tick(300e6, label_unit(100e6)), "300 ns");
        assert_eq!(format_tick(3e12, label_unit(1e12)), "3 ms");
        assert_eq!(format_tick(0.0, label_unit(1e9)), "0 us");
    }

    #[test]
    fn times_are_shown_as_precisely_as_a_pixel() {
        let mut timeline = Timeline::default();
        timeline.fit(ns(1000), 1000.0); // 1.02 ns per pixel, ruler in ns
        assert_eq!(timeline.format_time(4_128_571.0, 90.0), "4 ns");
        // 10.2 ps per pixel: ruler ticks 1 ns apart, so ns with the two
        // decimals one pixel can show.
        timeline.zoom(100.0, 0.0);
        assert_eq!(timeline.format_time(4_128_571.0, 90.0), "4.13 ns");
    }

    #[test]
    fn ticks_are_evenly_spaced_and_on_screen() {
        let mut timeline = Timeline::default();
        timeline.fit(ns(1000), 1000.0); // ~1 ns per pixel
        let ticks = timeline.ticks(1000.0, 80.0);
        // At least 80 px apart means a 100 ns step. The view runs to
        // 1020 ns (fit leaves a 2% margin), so 0, 100, ..., 1000 ns.
        assert_eq!(ticks.first().unwrap().label, "0 ns");
        assert_eq!(ticks[1].label, "100 ns");
        assert_eq!(ticks.last().unwrap().label, "1000 ns");
        assert_eq!(ticks.len(), 11);
        assert!(ticks.iter().all(|t| (0.0..=1000.0).contains(&t.x)));
    }
}
