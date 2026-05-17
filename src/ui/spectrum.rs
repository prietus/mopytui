#![allow(dead_code)]

//! Pseudo-spectrum visualizer. Mopidy doesn't expose audio samples to clients,
//! so we synthesise plausible band heights from `elapsed_ms`. The point is
//! "alive while playing" — not signal accuracy.

use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Borders, Padding, Paragraph};

use crate::app::App;
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
                "SPECTRUM",
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
            Span::styled(
                "  ·  32 bands  ·  log scale",
                Style::default().fg(app.theme.fg_muted),
            ),
            Span::raw(" "),
        ]));
    let inner = block.inner(area);
    f.render_widget(block, area);

    if inner.height == 0 || inner.width < BANDS as u16 {
        return;
    }

    let playing = app.playback.state == PlayState::Playing;
    let now_ms = app.playback.elapsed_ms.max(0) as f64;

    // Real FFT spectrum if the audio FIFO is live; pseudo otherwise.
    let (heights, _source): ([f64; BANDS], &'static str) =
        if let Some(reader) = app.audio_reader.as_ref()
            && let Some(real) = crate::audio::compute_fft_bands(reader, BANDS)
        {
            let mut out = [0.0_f64; BANDS];
            for (i, v) in real.iter().take(BANDS).enumerate() {
                out[i] = (*v as f64) * inner.height as f64;
            }
            (out, "FFT")
        } else {
            (
                compute_heights(now_ms, playing, inner.height as f64),
                "pseudo",
            )
        };
    // Fill `inner.width` exactly: each band gets `base_bar_w` cells, the
    // first `extra` bands get +1 so leftover cells aren't wasted on the
    // right. Gap (1 cell) sits between bars; no trailing gap after the last.
    let total_w = inner.width as usize;
    let gap = if total_w / BANDS >= 3 { 1 } else { 0 };
    let between = (BANDS - 1) * gap;
    let bar_budget = total_w.saturating_sub(between);
    let base_bar_w = (bar_budget / BANDS).max(1);
    let extra = bar_budget.saturating_sub(base_bar_w * BANDS);

    // Render row by row, from top of the panel to its base. For each row we
    // pick a sub-cell glyph based on how much of the bar reaches into it.
    for row in 0..inner.height {
        let y = inner.y + row;
        let mut spans: Vec<Span> = Vec::with_capacity(BANDS * 2);
        // Distance from base in cells (1.0 = full cell).
        let row_top = (inner.height - row) as f64;
        let row_bottom = row_top - 1.0;
        for (i, &h) in heights.iter().enumerate() {
            let bar_w = if i < extra { base_bar_w + 1 } else { base_bar_w };
            let col = lerp_color(app.theme.accent_alt, app.theme.accent, i as f32 / BANDS as f32);
            let glyph = sub_cell_glyph(h, row_top, row_bottom);
            spans.push(Span::styled(
                glyph.repeat(bar_w),
                Style::default()
                    .fg(col)
                    .add_modifier(Modifier::BOLD),
            ));
            if i + 1 < BANDS {
                spans.push(Span::raw(" ".repeat(gap)));
            }
        }
        let r = Rect { x: inner.x, y, width: inner.width, height: 1 };
        f.render_widget(Paragraph::new(Line::from(spans)), r);
    }
}

/// Choose which of the eight lower-block glyphs (or space) renders the slice
/// of bar between `row_bottom` and `row_top` (in cell units from the panel
/// base). `h` is the bar's total height in the same units.
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

fn spectrum_source_label(app: &App) -> &'static str {
    match app.audio_reader.as_ref() {
        Some(r) if r.is_live() => "FFT (live)",
        Some(_) => "FFT (no signal)",
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

fn compute_heights(now_ms: f64, playing: bool, max_h: f64) -> [f64; BANDS] {
    let mut out = [0.0; BANDS];
    if !playing {
        return out;
    }
    // Convert to seconds and scale up: at 10 fps we want oscillators in the
    // 1–4 Hz range so the bars bounce noticeably between frames without
    // looking like aliased garbage. Inner argument advances ~3–9 rad/sec.
    let t = now_ms * 0.001;
    for i in 0..BANDS {
        let fi = i as f64;
        let n = BANDS as f64;
        let norm = fi / (n - 1.0);
        // Bell envelope so the middle bands tend taller. Music-like.
        let envelope = 0.40 + 0.60 * (1.0 - (2.0 * norm - 1.0).powi(2));
        // Higher bands oscillate faster (treble feels snappier than bass).
        let speed = 1.0 + norm * 1.4;
        let f1 = 9.0 + (fi * 0.31).sin().abs() * 12.0;
        let f2 = 14.0 + (fi * 0.17).cos().abs() * 16.0;
        let f3 = 6.0 + (fi * 0.43).sin().abs() * 8.0;
        let phase = fi * 0.6;
        let osc = 0.42 * (t * f1 * speed + phase).sin()
            + 0.34 * (t * f2 * speed + phase * 0.7).sin()
            + 0.24 * (t * f3 * speed + phase * 1.3).cos();
        let val = (0.5 + 0.5 * osc).clamp(0.05, 1.0) * envelope;
        out[i] = (val * max_h).max(0.3);
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
