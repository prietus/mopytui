use ratatui::Frame;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Borders, Padding, Paragraph, Wrap};

use crate::app::App;
use crate::mopidy::models::PlayState;

pub fn render(f: &mut Frame, app: &App, area: Rect) {
    let show_vol = app.playback.volume >= 0;
    let cols = if show_vol {
        Layout::default()
            .direction(Direction::Horizontal)
            .constraints([
                Constraint::Length(34),
                Constraint::Min(20),
                Constraint::Length(28),
            ])
            .split(area)
    } else {
        // Bit-perfect: no software mixer reported → drop the volume box and
        // give the title section the reclaimed real estate.
        Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Length(34), Constraint::Min(0)])
            .split(area)
    };

    render_state_box(f, app, cols[0]);
    render_title_box(f, app, cols[1]);
    if show_vol {
        render_vol_box(f, app, cols[2]);
    }
}

fn render_state_box(f: &mut Frame, app: &App, area: Rect) {
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(app.theme.border))
        .padding(Padding::horizontal(1));
    let inner = block.inner(area);
    f.render_widget(block, area);

    let (label, color) = match app.playback.state {
        PlayState::Playing => ("[Playing]", app.theme.ok),
        PlayState::Paused => ("[Paused]", app.theme.warn),
        PlayState::Stopped => ("[Stopped]", app.theme.fg_muted),
    };
    let total = app.playback.current.as_ref().and_then(|t| t.length).unwrap_or(0) as i64;
    let elapsed = app.playback.elapsed_ms.max(0);
    let timer = format!("{}  /  {}", fmt_ms(elapsed), fmt_ms(total));
    let bitrate = app
        .playback
        .current
        .as_ref()
        .and_then(|t| t.bitrate)
        .or(app.bitrate)
        .filter(|b| *b > 0)
        .map(|b| format!("  ·  {b} kbps"))
        .unwrap_or_default();
    let mut chip_extra = String::new();
    if let Some(a) = &app.audio {
        chip_extra = format!("{}-bit · {} kHz", a.bits, a.rate / 1000);
    }

    let lines = vec![
        Line::from(Span::styled(
            label,
            Style::default().fg(color).add_modifier(Modifier::BOLD),
        )),
        Line::from(vec![
            Span::styled(
                timer,
                Style::default()
                    .fg(app.theme.fg_strong)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(bitrate, Style::default().fg(app.theme.fg_muted)),
        ]),
        Line::from(Span::styled(
            chip_extra,
            Style::default().fg(app.theme.accent_alt),
        )),
    ];
    f.render_widget(Paragraph::new(lines), inner);
}

fn render_title_box(f: &mut Frame, app: &App, area: Rect) {
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(app.theme.border))
        .padding(Padding::horizontal(2));
    let inner = block.inner(area);
    f.render_widget(block, area);

    let (title, artists, album) = match &app.playback.current {
        Some(t) => (t.name.clone(), t.artists_joined(), t.album_name().to_string()),
        None => ("—".into(), "Nothing playing".into(), String::new()),
    };

    let favorited = app
        .playback
        .current
        .as_ref()
        .and_then(|t| t.album.as_ref())
        .and_then(|a| a.uri.as_deref())
        .and_then(crate::app::tidal_album_id)
        .map(|id| app.goodies.favorites.contains(id))
        .unwrap_or(false);

    let mut title_spans: Vec<Span> = Vec::new();
    if favorited {
        title_spans.push(Span::styled(
            "★ ",
            Style::default().fg(app.theme.warn).add_modifier(Modifier::BOLD),
        ));
    }
    title_spans.push(Span::styled(
        title,
        Style::default()
            .fg(app.theme.fg_strong)
            .add_modifier(Modifier::BOLD),
    ));

    let mut sub_spans: Vec<Span> = Vec::new();
    if !artists.is_empty() {
        sub_spans.push(Span::styled(
            artists,
            Style::default().fg(app.theme.accent).add_modifier(Modifier::BOLD),
        ));
    }
    if !album.is_empty() {
        sub_spans.push(Span::styled("  ·  ", Style::default().fg(app.theme.fg_muted)));
        sub_spans.push(Span::styled(album, Style::default().fg(app.theme.fg_muted)));
    }

    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),
            Constraint::Min(1),
            Constraint::Length(1),
        ])
        .split(inner);

    f.render_widget(
        Paragraph::new(Line::from(title_spans)).wrap(Wrap { trim: true }),
        rows[1],
    );
    f.render_widget(
        Paragraph::new(Line::from(sub_spans)).wrap(Wrap { trim: true }),
        rows[2],
    );
}

fn render_vol_box(f: &mut Frame, app: &App, area: Rect) {
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(app.theme.border))
        .padding(Padding::horizontal(1));
    let inner = block.inner(area);
    f.render_widget(block, area);

    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(1), Constraint::Length(1), Constraint::Min(0)])
        .split(inner);

    // Volume bar
    let vol = app.playback.volume.max(0) as u16;
    let bar_width = rows[0].width.saturating_sub(10) as usize; // " Vol " + " 100% "
    let filled = ((vol as f64 / 100.0) * bar_width as f64).round() as usize;
    let rest = bar_width.saturating_sub(filled);
    f.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled(
                "Vol ",
                Style::default()
                    .fg(app.theme.fg_muted)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                "▰".repeat(filled),
                Style::default()
                    .fg(app.theme.accent_alt)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                "▱".repeat(rest),
                Style::default().fg(app.theme.progress_empty),
            ),
            Span::styled(
                format!(" {vol:>3}%"),
                Style::default()
                    .fg(app.theme.fg_strong)
                    .add_modifier(Modifier::BOLD),
            ),
        ])),
        rows[0],
    );

    // Modes row + connection dot.
    let conn = if app.connected { "●" } else { "○" };
    let conn_style = if app.connected {
        Style::default().fg(app.theme.ok)
    } else {
        Style::default().fg(app.theme.err)
    };
    let modes = Line::from(vec![
        Span::raw(" "),
        mode_glyph(app, "↻", app.modes.repeat),
        Span::raw("  "),
        mode_glyph(app, "⇄", app.modes.random),
        Span::raw("  "),
        mode_glyph(app, "∞", app.modes.single),
        Span::raw("  "),
        mode_glyph(app, "✕", app.modes.consume),
        Span::raw("   "),
        Span::styled(conn, conn_style),
    ]);
    f.render_widget(Paragraph::new(modes), rows[1]);
}

fn mode_glyph(app: &App, ch: &'static str, on: bool) -> Span<'static> {
    if on {
        Span::styled(
            ch,
            Style::default()
                .fg(app.theme.accent)
                .add_modifier(Modifier::BOLD),
        )
    } else {
        Span::styled(ch, Style::default().fg(app.theme.fg_muted))
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
