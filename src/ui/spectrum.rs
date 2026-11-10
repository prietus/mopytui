#![allow(dead_code)]

//! Spectrum / waveform / VU visualizer with five interchangeable styles, cycled at
//! runtime with `v` (see [`crate::app::VisStyle`]). Mopidy doesn't expose
//! audio samples to JSON-RPC clients, so when the audio FIFO/UDP/TCP reader
//! isn't live we synthesise plausible band heights from `elapsed_ms` — the
//! point is "alive while playing", not signal accuracy.

use ratatui::Frame;
use ratatui::layout::{Alignment, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Borders, Padding, Paragraph};

use crate::app::{App, VisStyle};
use crate::mopidy::models::PlayState;

const BANDS: usize = 32;

pub fn render(f: &mut Frame, app: &App, area: Rect) {
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(app.theme.border))
        .padding(Padding::symmetric(2, 0))
        .title(Line::from(vec![
            Span::raw(" "),
            Span::styled(
                if app.visualizer == VisStyle::Vu { "VU METER" } else { "SPECTRUM" },
                Style::default()
                    .fg(app.theme.accent_alt)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled("  ·  ", Style::default().fg(app.theme.fg_muted)),
            Span::styled(
                spectrum_source_label(app),
                Style::default()
                    .fg(spectrum_source_color(app))
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled("  ·  ", Style::default().fg(app.theme.fg_muted)),
            Span::styled(
                app.visualizer.label(),
                Style::default()
                    .fg(app.theme.accent)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw(" "),
        ]).alignment(Alignment::Center));
    let inner = block.inner(area);
    f.render_widget(block, area);

    if inner.height == 0 || inner.width < 8 {
        return;
    }

    match app.visualizer {
        VisStyle::Bars => render_bars(f, app, inner),
        VisStyle::Mirror => render_mirror(f, app, inner),
        VisStyle::Dots => render_dots(f, app, inner),
        VisStyle::Wave => render_wave(f, app, inner),
        VisStyle::Vu => render_vu(f, app, inner),
    }
}

// ─── shared ────────────────────────────────────────────────────────────────

/// Per-band magnitudes in `[0.0, 1.0]`. Returns real FFT magnitudes when the
/// audio reader is live; falls back to a synthesised "music-shaped" curve
/// driven by `elapsed_ms` otherwise.
fn band_heights(app: &App) -> [f64; BANDS] {
    if let Some(reader) = app.audio_reader.as_ref()
        && let Some(real) = crate::audio::compute_fft_bands(reader, BANDS)
    {
        let mut out = [0.0_f64; BANDS];
        for (i, v) in real.iter().take(BANDS).enumerate() {
            out[i] = *v as f64;
        }
        return out;
    }
    let now_ms = app.playback.elapsed_ms.max(0) as f64;
    let playing = app.playback.state == PlayState::Playing;
    compute_heights(now_ms, playing)
}

/// Distribute `total_w` cells across `BANDS` so the bars + gaps fill the
/// width exactly. Returns `(base_bar_w, extra, gap)`: the first `extra`
/// bands get `base_bar_w + 1` cells; the rest get `base_bar_w`; a `gap`-cell
/// space sits between adjacent bars (no trailing gap after the last).
fn band_layout(total_w: usize) -> (usize, usize, usize) {
    let gap = if total_w / BANDS >= 3 { 1 } else { 0 };
    let between = (BANDS - 1) * gap;
    let bar_budget = total_w.saturating_sub(between);
    let base_bar_w = (bar_budget / BANDS).max(1);
    let extra = bar_budget.saturating_sub(base_bar_w * BANDS);
    (base_bar_w, extra, gap)
}

// ─── bars ──────────────────────────────────────────────────────────────────

fn render_bars(f: &mut Frame, app: &App, inner: Rect) {
    if inner.width < BANDS as u16 {
        return;
    }
    let heights = band_heights(app);
    let max_h = inner.height as f64;
    let scaled: [f64; BANDS] = std::array::from_fn(|i| (heights[i] * max_h).max(0.3));
    let (base_bar_w, extra, gap) = band_layout(inner.width as usize);

    for row in 0..inner.height {
        let mut spans: Vec<Span> = Vec::with_capacity(BANDS * 2);
        let row_top = (inner.height - row) as f64;
        let row_bottom = row_top - 1.0;
        for (i, &h) in scaled.iter().enumerate() {
            let bar_w = if i < extra { base_bar_w + 1 } else { base_bar_w };
            let col = lerp_color(app.theme.accent_alt, app.theme.accent, i as f32 / BANDS as f32);
            let glyph = sub_cell_glyph(h, row_top, row_bottom);
            spans.push(Span::styled(
                glyph.repeat(bar_w),
                Style::default().fg(col).add_modifier(Modifier::BOLD),
            ));
            if i + 1 < BANDS {
                spans.push(Span::raw(" ".repeat(gap)));
            }
        }
        let r = Rect { x: inner.x, y: inner.y + row, width: inner.width, height: 1 };
        f.render_widget(Paragraph::new(Line::from(spans)), r);
    }
}

/// Pick a lower-block glyph for a bar slice between `row_bottom` and
/// `row_top` (cell units from the panel base). `h` is the bar's total height
/// in the same units.
fn sub_cell_glyph(h: f64, row_top: f64, row_bottom: f64) -> &'static str {
    if h <= row_bottom { return " "; }
    if h >= row_top { return "█"; }
    let frac = (h - row_bottom).clamp(0.0, 1.0);
    match (frac * 8.0).round() as u8 {
        0 => " ",
        1 => "▁",
        2 => "▂",
        3 => "▃",
        4 => "▄",
        5 => "▅",
        6 => "▆",
        7 => "▇",
        _ => "█",
    }
}

// ─── mirror ────────────────────────────────────────────────────────────────

fn render_mirror(f: &mut Frame, app: &App, inner: Rect) {
    if inner.width < BANDS as u16 {
        return;
    }
    let heights = band_heights(app);
    let center = inner.height as f64 / 2.0;
    let (base_bar_w, extra, gap) = band_layout(inner.width as usize);

    for row in 0..inner.height {
        let r = row as f64;
        // Half-cell mid-points; distance from the panel centre line (cells).
        let upper_dist = (r + 0.25 - center).abs();
        let lower_dist = (r + 0.75 - center).abs();
        let mut spans: Vec<Span> = Vec::with_capacity(BANDS * 2);
        for (i, &band_h) in heights.iter().enumerate() {
            // Bar half-height in cells. `band_h ∈ [0, 1]`, max reach = `center`.
            let bh = band_h * center;
            let upper = bh > upper_dist;
            let lower = bh > lower_dist;
            let glyph = match (upper, lower) {
                (true, true) => "█",
                (true, false) => "▀",
                (false, true) => "▄",
                (false, false) => " ",
            };
            let bar_w = if i < extra { base_bar_w + 1 } else { base_bar_w };
            let col = lerp_color(app.theme.accent_alt, app.theme.accent, i as f32 / BANDS as f32);
            spans.push(Span::styled(
                glyph.repeat(bar_w),
                Style::default().fg(col).add_modifier(Modifier::BOLD),
            ));
            if i + 1 < BANDS {
                spans.push(Span::raw(" ".repeat(gap)));
            }
        }
        let area = Rect { x: inner.x, y: inner.y + row, width: inner.width, height: 1 };
        f.render_widget(Paragraph::new(Line::from(spans)), area);
    }
}

// ─── dots (braille FFT) ────────────────────────────────────────────────────

fn render_dots(f: &mut Frame, app: &App, inner: Rect) {
    let heights = band_heights(app);
    let dot_w = inner.width as usize * 2;
    let dot_h = inner.height as usize * 4;
    let mut grid = vec![false; dot_w * dot_h];

    for b in 0..BANDS {
        let l = (b * dot_w) / BANDS;
        let r = (((b + 1) * dot_w) / BANDS).min(dot_w);
        if r <= l { continue; }
        let bar_dots = (heights[b] * dot_h as f64).round() as usize;
        if bar_dots == 0 { continue; }
        let top = dot_h.saturating_sub(bar_dots);
        for y in top..dot_h {
            for x in l..r {
                grid[y * dot_w + x] = true;
            }
        }
    }

    render_braille_grid(f, app, inner, &grid, dot_w);
}

// ─── wave (braille PCM line) ───────────────────────────────────────────────

fn render_wave(f: &mut Frame, app: &App, inner: Rect) {
    let dot_w = inner.width as usize * 2;
    let dot_h = inner.height as usize * 4;

    let mut samples = vec![0.0_f32; dot_w];
    let live = match app.audio_reader.as_ref() {
        Some(reader) => reader.copy_recent(&mut samples, dot_w),
        None => false,
    };
    if !live {
        // Pseudo-wave from `elapsed_ms` — a soft two-tone sine while playing,
        // flat when paused/stopped.
        let now_s = (app.playback.elapsed_ms.max(0) as f64) * 0.001;
        let amp = if app.playback.state == PlayState::Playing { 0.55 } else { 0.0 };
        for (x, s) in samples.iter_mut().enumerate() {
            let t = now_s + x as f64 / dot_w as f64 * 1.2;
            *s = (amp * ((t * 4.0).sin() * 0.7 + (t * 9.5).sin() * 0.3)) as f32;
        }
    }

    let center_y = dot_h as f64 / 2.0;
    let amp_dots = (dot_h as f64 / 2.0) - 0.5;

    let mut grid = vec![false; dot_w * dot_h];
    let mut prev_y: Option<usize> = None;
    for x in 0..dot_w {
        let s = samples[x].clamp(-1.0, 1.0) as f64;
        let y = (center_y - s * amp_dots)
            .round()
            .clamp(0.0, (dot_h - 1) as f64) as usize;
        grid[y * dot_w + x] = true;
        // Vertical fill between this sample and the previous so the trace
        // reads as a continuous line instead of disconnected dots.
        if let Some(py) = prev_y {
            let (lo, hi) = if py < y { (py, y) } else { (y, py) };
            for fy in lo..=hi {
                grid[fy * dot_w + x] = true;
            }
        }
        prev_y = Some(y);
    }

    render_braille_grid(f, app, inner, &grid, dot_w);
}

// ─── braille helpers ───────────────────────────────────────────────────────

/// Pack a `dot_w × (inner.height × 4)` boolean grid into braille glyphs and
/// render it one row at a time. Colour is interpolated per cell column so the
/// left edge fades from `accent_alt` toward `accent` on the right.
fn render_braille_grid(f: &mut Frame, app: &App, inner: Rect, grid: &[bool], dot_w: usize) {
    for cy in 0..inner.height {
        let mut spans: Vec<Span> = Vec::with_capacity(inner.width as usize);
        for cx in 0..inner.width {
            let base_x = cx as usize * 2;
            let base_y = cy as usize * 4;
            let mut bits: u8 = 0;
            for dy in 0..4 {
                for dx in 0..2 {
                    if grid[(base_y + dy) * dot_w + (base_x + dx)] {
                        bits |= dot_bit(dx, dy);
                    }
                }
            }
            let glyph = char::from_u32(0x2800 + bits as u32).unwrap_or(' ');
            let col = lerp_color(
                app.theme.accent_alt,
                app.theme.accent,
                cx as f32 / inner.width.max(1) as f32,
            );
            spans.push(Span::styled(
                glyph.to_string(),
                Style::default().fg(col).add_modifier(Modifier::BOLD),
            ));
        }
        let area = Rect { x: inner.x, y: inner.y + cy, width: inner.width, height: 1 };
        f.render_widget(Paragraph::new(Line::from(spans)), area);
    }
}

/// Bit position for a dot at column `x ∈ {0,1}`, row `y ∈ {0..4}` in the
/// Unicode braille pattern at `U+2800 + bits`. Layout:
///
/// ```text
///   x=0  x=1
///   bit0 bit3   ← dy=0
///   bit1 bit4   ← dy=1
///   bit2 bit5   ← dy=2
///   bit6 bit7   ← dy=3
/// ```
fn dot_bit(x: usize, y: usize) -> u8 {
    match (x, y) {
        (0, 0) => 0x01, (1, 0) => 0x08,
        (0, 1) => 0x02, (1, 1) => 0x10,
        (0, 2) => 0x04, (1, 2) => 0x20,
        (0, 3) => 0x40, (1, 3) => 0x80,
        _ => 0,
    }
}

// ─── source label / pseudo / colour ────────────────────────────────────────

// ─── vu meter ──────────────────────────────────────────────────────────────

use crate::audio::{VU_MIN_DB, Vu};

/// Position of `db` on the meter, `0.0` (scale minimum) to `1.0` (0 dBFS).
fn vu_frac(db: f32) -> f64 {
    ((db - VU_MIN_DB) / -VU_MIN_DB).clamp(0.0, 1.0) as f64
}

/// The glyphs of one meter bar `w` cells wide: full blocks up to the level
/// (with a partial block for the fraction of a cell), a dim track after it,
/// and a marker on the cell where the held peak sits.
fn vu_cells(level: f64, peak: f64, w: usize) -> Vec<&'static str> {
    const PARTIAL: [&str; 8] = ["░", "▏", "▎", "▍", "▌", "▋", "▊", "▉"];
    let filled = level.clamp(0.0, 1.0) * w as f64;
    let peak_cell = ((peak.clamp(0.0, 1.0) * w as f64).ceil() as usize).clamp(1, w.max(1)) - 1;
    (0..w)
        .map(|i| {
            if (i + 1) as f64 <= filled {
                "█"
            } else if i == peak_cell && peak > 0.0 {
                "┃"
            } else if (i as f64) < filled {
                PARTIAL[(((filled - i as f64) * 8.0) as usize).min(7)]
            } else {
                "░"
            }
        })
        .collect()
}

/// Scale line under a bar of `w` cells: dB labels at their positions.
fn vu_scale(w: usize) -> String {
    let mut row = vec![' '; w];
    for db in [-48.0_f32, -36.0, -24.0, -12.0, -6.0, -3.0, 0.0] {
        let label = format!("{db:.0}");
        let at = (vu_frac(db) * w as f64).round() as usize;
        // Centre the label on its position, kept inside the row.
        let start = at.saturating_sub(label.len() / 2).min(w.saturating_sub(label.len()));
        if row[start..(start + label.len()).min(w)].iter().all(|c| *c == ' ')
            && (start == 0 || row[start - 1] == ' ')
        {
            for (k, ch) in label.chars().enumerate() {
                if start + k < w { row[start + k] = ch; }
            }
        }
    }
    row.into_iter().collect()
}

/// Levels to show: the real meter when audio is live, otherwise a gentle
/// synthesised swing while playing (like the pseudo spectrum) or silence.
fn vu_reading(app: &App) -> Vu {
    if let Some(vu) = app.audio_reader.as_ref().and_then(|r| r.vu()) {
        return vu;
    }
    if app.playback.state != PlayState::Playing {
        return Vu { level: [VU_MIN_DB; 2], peak: [VU_MIN_DB; 2] };
    }
    let t = app.playback.elapsed_ms.max(0) as f32 * 0.001;
    let ch = |phase: f32| -18.0 + 7.0 * (t * 3.1 + phase).sin() + 3.0 * (t * 7.7 + phase * 2.0).sin();
    let level = [ch(0.0), ch(0.9)];
    Vu { level, peak: [level[0] + 5.0, level[1] + 5.0] }
}

fn render_vu(f: &mut Frame, app: &App, inner: Rect) {
    const LABEL_W: u16 = 2;
    const READOUT_W: u16 = 10;
    if inner.height < 3 || inner.width < LABEL_W + READOUT_W + 12 {
        return;
    }
    let vu = vu_reading(app);
    let bar_w = (inner.width - LABEL_W - READOUT_W) as usize;
    let bar_h: u16 = if inner.height >= 7 { 2 } else { 1 };
    let top = inner.y + (inner.height - (2 * bar_h + 1)) / 2;
    // Green up to -12 dBFS, amber up to -3, red above.
    let zone = |i: usize| {
        let db = VU_MIN_DB + (i as f32 + 0.5) / bar_w as f32 * -VU_MIN_DB;
        if db >= -3.0 { app.theme.err } else if db >= -12.0 { app.theme.warn } else { app.theme.ok }
    };

    for (ch, name) in ["L", "R"].into_iter().enumerate() {
        let cells = vu_cells(vu_frac(vu.level[ch]), vu_frac(vu.peak[ch]), bar_w);
        let readout = if vu.level[ch] <= VU_MIN_DB {
            format!("{:>w$}", "-∞ dB", w = READOUT_W as usize)
        } else {
            format!("{:>w$}", format!("{:.1} dB", vu.level[ch]), w = READOUT_W as usize)
        };
        let y0 = top + ch as u16 * (bar_h + 1);
        for dy in 0..bar_h {
            let mut spans: Vec<Span> = Vec::with_capacity(bar_w + 2);
            spans.push(Span::styled(
                if dy == 0 { format!("{name} ") } else { "  ".to_string() },
                Style::default().fg(app.theme.fg_strong).add_modifier(Modifier::BOLD),
            ));
            for (i, glyph) in cells.iter().enumerate() {
                let style = match *glyph {
                    "░" => Style::default().fg(app.theme.border),
                    "┃" => Style::default().fg(app.theme.fg_strong).add_modifier(Modifier::BOLD),
                    _ => Style::default().fg(zone(i)),
                };
                spans.push(Span::styled(*glyph, style));
            }
            if dy == 0 {
                spans.push(Span::styled(readout.clone(), Style::default().fg(app.theme.fg_muted)));
            }
            let r = Rect { x: inner.x, y: y0 + dy, width: inner.width, height: 1 };
            f.render_widget(Paragraph::new(Line::from(spans)), r);
        }
    }
    // dB scale between the two bars.
    let r = Rect { x: inner.x + LABEL_W, y: top + bar_h, width: bar_w as u16, height: 1 };
    f.render_widget(
        Paragraph::new(Span::styled(vu_scale(bar_w), Style::default().fg(app.theme.fg_muted))),
        r,
    );
}

#[cfg(test)]
mod vu_tests {
    use super::*;

    #[test]
    fn bar_fills_to_the_level_with_a_partial_cell_and_marks_the_peak() {
        let cells = vu_cells(0.45, 0.8, 10).concat();
        assert_eq!(cells, "████▌░░┃░░");
        // Silence: empty track, no peak marker. Full scale: all blocks.
        assert_eq!(vu_cells(0.0, 0.0, 6).concat(), "░░░░░░");
        assert_eq!(vu_cells(1.0, 1.0, 6).concat(), "██████");
        // A peak inside the filled part is hidden by the bar.
        assert_eq!(vu_cells(0.5, 0.5, 4).concat(), "██░░");
    }

    #[test]
    fn scale_puts_labels_at_their_positions_without_overlap() {
        let s = vu_scale(96);
        assert_eq!(s.chars().count(), 96);
        assert!(s.starts_with("-48") && s.ends_with('0'));
        for label in ["-36", "-24", "-12", "-6", "-3"] {
            assert!(s.contains(label), "{label} missing in {s:?}");
        }
        // -24 dBFS is the middle of a -48..0 scale.
        let mid = s.find("-24").unwrap() + 1;
        assert!((47..=49).contains(&mid), "{mid}");
        // Narrow bars drop labels instead of overlapping them.
        let narrow = vu_scale(14);
        assert_eq!(narrow.chars().count(), 14);
        assert!(!narrow.contains("--") && !narrow.contains("-3-"));
    }

    #[test]
    fn panel_shows_both_channels_the_scale_and_the_readouts() {
        use ratatui::Terminal;
        use ratatui::backend::TestBackend;
        let mut app = crate::app::App::new(
            crate::config::AppConfig::default(),
            crate::mopidy::Client::new("127.0.0.1", 6680),
            std::sync::Arc::new(crate::images::ImageCache::new()),
            ratatui_image::picker::Picker::halfblocks(),
            None,
        );
        app.visualizer = VisStyle::Vu;
        app.playback.state = PlayState::Playing;
        app.playback.elapsed_ms = 12_345;
        let mut term = Terminal::new(TestBackend::new(100, 10)).unwrap();
        term.draw(|f| render(f, &app, f.area())).unwrap();
        let buf = term.backend().buffer().clone();
        let screen: Vec<String> = (0..10)
            .map(|y| (0..100).map(|x| buf[(x, y)].symbol()).collect::<String>().trim_end().to_string())
            .collect();
        println!("{}", screen.join("\n"));
        let all = screen.join("\n");
        assert!(all.contains("VU METER") && all.contains("vu"));
        assert!(screen.iter().any(|l| l.contains("L █")) && screen.iter().any(|l| l.contains("R █")));
        assert!(all.contains("-48") && all.contains("-12") && all.contains(" dB"));
    }

    #[test]
    fn db_maps_linearly_onto_the_meter() {
        assert_eq!(vu_frac(VU_MIN_DB), 0.0);
        assert_eq!(vu_frac(0.0), 1.0);
        assert_eq!(vu_frac(-24.0), 0.5);
        assert_eq!(vu_frac(6.0), 1.0);
    }
}

fn spectrum_source_label(app: &App) -> &'static str {
    // The VU meter reads levels, not an FFT.
    let vu = app.visualizer == VisStyle::Vu;
    match app.audio_reader.as_ref() {
        Some(r) if r.is_live() => if vu { "live" } else { "FFT (live)" },
        Some(_) => if vu { "no signal" } else { "FFT (no signal)" },
        None => "pseudo",
    }
}

fn spectrum_source_color(app: &App) -> ratatui::style::Color {
    match app.audio_reader.as_ref() {
        Some(r) if r.is_live() => app.theme.ok,
        Some(_) => app.theme.warn,
        None => app.theme.fg_muted,
    }
}

/// Synthesised band heights for the pseudo-spectrum. Returns values in
/// `[0.0, 1.0]` shaped like music: bell envelope so mid-bands tend taller,
/// and higher bands oscillate faster than lower ones.
fn compute_heights(now_ms: f64, playing: bool) -> [f64; BANDS] {
    let mut out = [0.0; BANDS];
    if !playing {
        return out;
    }
    let t = now_ms * 0.001;
    for i in 0..BANDS {
        let fi = i as f64;
        let n = BANDS as f64;
        let norm = fi / (n - 1.0);
        let envelope = 0.40 + 0.60 * (1.0 - (2.0 * norm - 1.0).powi(2));
        let speed = 1.0 + norm * 1.4;
        let f1 = 9.0 + (fi * 0.31).sin().abs() * 12.0;
        let f2 = 14.0 + (fi * 0.17).cos().abs() * 16.0;
        let f3 = 6.0 + (fi * 0.43).sin().abs() * 8.0;
        let phase = fi * 0.6;
        let osc = 0.42 * (t * f1 * speed + phase).sin()
            + 0.34 * (t * f2 * speed + phase * 0.7).sin()
            + 0.24 * (t * f3 * speed + phase * 1.3).cos();
        let val = (0.5 + 0.5 * osc).clamp(0.05, 1.0) * envelope;
        out[i] = val.max(0.01);
    }
    out
}

fn lerp_color(a: Color, b: Color, t: f32) -> Color {
    match (a, b) {
        (Color::Rgb(r1, g1, b1), Color::Rgb(r2, g2, b2)) => {
            let t = t.clamp(0.0, 1.0);
            Color::Rgb(
                (r1 as f32 + (r2 as f32 - r1 as f32) * t).round() as u8,
                (g1 as f32 + (g2 as f32 - g1 as f32) * t).round() as u8,
                (b1 as f32 + (b2 as f32 - b1 as f32) * t).round() as u8,
            )
        }
        _ => a,
    }
}
