use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;

use crate::app::{App, StatusKind};

pub fn render(f: &mut Frame, app: &App, area: Rect) {
    let theme = &app.theme;

    // If a flash status is set, it takes over the whole row.
    if !app.status.message.is_empty() {
        let color = match app.status.kind {
            StatusKind::Ok => theme.ok,
            StatusKind::Warn => theme.warn,
            StatusKind::Err => theme.err,
            StatusKind::Info => theme.fg_muted,
        };
        f.render_widget(
            Paragraph::new(Span::styled(
                format!(" {} ", app.status.message),
                Style::default().fg(color).add_modifier(Modifier::BOLD),
            )),
            area,
        );
        return;
    }

    let hints = [
        ("space", "play/pause"),
        (">", "next"),
        ("/", "search"),
        ("f", "favorite"),
        ("?", "help"),
    ];
    let mut spans: Vec<Span> = Vec::new();
    spans.push(Span::raw(" "));
    for (i, (k, v)) in hints.iter().enumerate() {
        if i > 0 {
            spans.push(Span::styled("  ·  ", Style::default().fg(theme.fg_muted)));
        }
        spans.push(Span::styled(
            format!("[{k}]"),
            Style::default().fg(theme.accent).add_modifier(Modifier::BOLD),
        ));
        spans.push(Span::styled(
            format!(" {v}"),
            Style::default().fg(theme.fg),
        ));
    }
    f.render_widget(Paragraph::new(Line::from(spans)), area);

    // Right-aligned summary: songs in queue + total duration queued.
    let total_ms: u64 = app.queue.iter().filter_map(|t| t.track.length).sum();
    let total_s = (total_ms / 1000) as i64;
    let summary = format!(
        "  {} songs  ·  {} queued ",
        app.queue.len(),
        fmt_long(total_s)
    );
    let w = summary.chars().count() as u16;
    if area.width > w {
        let r = Rect {
            x: area.x + area.width - w,
            y: area.y,
            width: w,
            height: 1,
        };
        f.render_widget(
            Paragraph::new(Span::styled(
                summary,
                Style::default().fg(theme.fg_muted),
            )),
            r,
        );
    }
}

fn fmt_long(secs: i64) -> String {
    let d = secs / 86400;
    let h = (secs % 86400) / 3600;
    let m = (secs % 3600) / 60;
    let s = secs % 60;
    if d > 0 {
        format!("{d}d {h}h")
    } else if h > 0 {
        format!("{h}h {m:02}m")
    } else {
        format!("{m:02}:{s:02}")
    }
}
