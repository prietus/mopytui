use ratatui::Frame;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;

use crate::app::App;
use crate::mopidy::models::PlayState;

pub fn render(f: &mut Frame, app: &App, area: Rect) {
    if area.height == 0 { return; }
    let total_ms = app.playback.current.as_ref().and_then(|t| t.length).unwrap_or(0) as i64;
    let elapsed = app.playback.elapsed_ms.max(0);
    let pct = if total_ms > 0 {
        (elapsed as f64 / total_ms as f64).clamp(0.0, 1.0)
    } else {
        0.0
    };
    // Single-row progress: state glyph + elapsed + playhead line + total.
    // If the allocated area is taller, anchor to the bottom row.
    let row = Rect {
        x: area.x,
        y: area.y + area.height.saturating_sub(1),
        width: area.width,
        height: 1,
    };
    render_axis(f, app, row, elapsed, total_ms, pct);
}

fn render_axis(f: &mut Frame, app: &App, area: Rect, elapsed: i64, total: i64, pct: f64) {
    let cols = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Length(3),   // play glyph
            Constraint::Length(8),   // elapsed
            Constraint::Length(1),
            Constraint::Min(10),     // playhead line
            Constraint::Length(1),
            Constraint::Length(8),   // total
        ])
        .split(area);

    let state_glyph = match app.playback.state {
        PlayState::Playing => "▶",
        PlayState::Paused => "⏸",
        PlayState::Stopped => "■",
    };
    f.render_widget(
        Paragraph::new(Span::styled(
            format!(" {state_glyph} "),
            Style::default()
                .fg(app.theme.accent)
                .add_modifier(Modifier::BOLD),
        )),
        cols[0],
    );
    f.render_widget(
        Paragraph::new(Span::styled(
            fmt_ms(elapsed),
            Style::default()
                .fg(app.theme.fg_strong)
                .add_modifier(Modifier::BOLD),
        )),
        cols[1],
    );

    let lw = cols[3].width as usize;
    if lw > 0 {
        let head = (pct * lw as f64).round() as usize;
        let head = head.min(lw.saturating_sub(1));
        let mut spans: Vec<Span> = Vec::with_capacity(lw);
        for col in 0..lw {
            let (glyph, color, modifier) = if col == head {
                ("●", app.theme.accent, Modifier::BOLD)
            } else if col < head {
                let t = if head > 0 { col as f32 / head as f32 } else { 0.0 };
                ("━", lerp_color(app.theme.accent_alt, app.theme.accent, t), Modifier::BOLD)
            } else {
                ("─", app.theme.progress_empty, Modifier::empty())
            };
            spans.push(Span::styled(glyph, Style::default().fg(color).add_modifier(modifier)));
        }
        f.render_widget(Paragraph::new(Line::from(spans)), cols[3]);
    }
    f.render_widget(
        Paragraph::new(Span::styled(
            fmt_ms(total),
            Style::default().fg(app.theme.fg_muted),
        )),
        cols[5],
    );
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

fn fmt_ms(ms: i64) -> String {
    let s = (ms / 1000).max(0);
    let h = s / 3600;
    let m = (s % 3600) / 60;
    let sec = s % 60;
    if h > 0 {
        format!("{h}:{m:02}:{sec:02}")
    } else {
        format!("{m}:{sec:02}")
    }
}
