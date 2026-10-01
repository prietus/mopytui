pub mod albums;
pub mod chips;
pub mod header;
pub mod help;
pub mod info;
pub mod library;
pub mod now_playing;
pub mod playlists;
pub mod progress;
pub mod queue;
pub mod search;
pub mod spectrum;
pub mod stats;
pub mod status;
pub mod theme;

use ratatui::Frame;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;

use crate::app::{App, View};

/// Top-level render: persistent chrome (header → tabs → body → progress →
/// status). The Queue view embeds the spectrum panel below its track table
/// when a track is loaded and there's enough vertical room.
pub fn render(f: &mut Frame, app: &mut App) {
    let area = f.area();
    let h = area.height;

    let header_h: u16 = if h < 22 { 3 } else { 4 };

    let v = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(header_h),    // header (3 boxes)
            Constraint::Length(1),           // tabs
            Constraint::Min(0),              // body
            Constraint::Length(1),           // progress bar (single row)
            Constraint::Length(1),           // hints + mpd info
        ])
        .split(area);

    header::render(f, app, v[0]);
    render_tabs(f, app, v[1]);
    render_body(f, app, v[2]);
    progress::render(f, app, v[3]);
    status::render(f, app, v[4]);
    render_tidal_login(f, app, area);
}

/// Centered popup shown while the Mopidy server has no Tidal session.
fn render_tidal_login(f: &mut Frame, app: &App, area: Rect) {
    use ratatui::widgets::{Block, BorderType, Borders, Clear, Wrap};
    let Some(url) = app.tidal_login.as_deref() else { return };
    if app.tidal_login_hidden {
        return;
    }
    // Device code at the end of link.tidal.com/XXXXX, handy on another device.
    let code = url
        .split("://")
        .nth(1)
        .filter(|rest| rest.starts_with("link.tidal.com") || rest.contains("tidal.com/"))
        .and_then(|rest| rest.rsplit('/').next())
        .filter(|c| !c.is_empty());
    let t = &app.theme;
    let mut lines = vec![
        Line::from(Span::styled(
            "Connect your Tidal account",
            Style::default().fg(t.fg_strong).add_modifier(Modifier::BOLD),
        )),
        Line::from(""),
        Line::from(Span::styled(
            "The Mopidy server isn't signed in to Tidal. Open the link, sign in and approve it:",
            Style::default().fg(t.fg),
        )),
        Line::from(""),
        Line::from(Span::styled(url.to_string(), Style::default().fg(t.accent).add_modifier(Modifier::BOLD))),
    ];
    if let Some(c) = code {
        lines.push(Line::from(""));
        lines.push(Line::from(vec![
            Span::styled("Code  ", Style::default().fg(t.fg_muted)),
            Span::styled(c.to_string(), Style::default().fg(t.fg_strong).add_modifier(Modifier::BOLD)),
        ]));
    }
    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled("Waiting for authorization…", Style::default().fg(t.fg_muted))));
    lines.push(Line::from(""));
    lines.push(Line::from(vec![
        Span::styled("[Enter]", Style::default().fg(t.accent).add_modifier(Modifier::BOLD)),
        Span::styled(" open in browser   ", Style::default().fg(t.fg)),
        Span::styled("[Esc]", Style::default().fg(t.accent).add_modifier(Modifier::BOLD)),
        Span::styled(" hide ([!] to show again)", Style::default().fg(t.fg)),
    ]));

    let w = area.width.saturating_sub(4).clamp(20, 72);
    let h = (lines.len() as u16 + 4 + (url.len() as u16 / w.saturating_sub(4).max(1))).min(area.height);
    let r = Rect {
        x: area.x + (area.width.saturating_sub(w)) / 2,
        y: area.y + (area.height.saturating_sub(h)) / 2,
        width: w.min(area.width),
        height: h,
    };
    f.render_widget(Clear, r);
    f.render_widget(
        Paragraph::new(lines).wrap(Wrap { trim: false }).block(
            Block::default()
                .borders(Borders::ALL)
                .border_type(BorderType::Rounded)
                .border_style(Style::default().fg(t.accent))
                .padding(ratatui::widgets::Padding::horizontal(1)),
        ),
        r,
    );
}

fn render_tabs(f: &mut Frame, app: &App, area: Rect) {
    let order = [
        (View::Queue, "1", "Queue"),
        (View::Albums, "2", "Albums"),
        (View::Library, "3", "Library"),
        (View::Playlists, "4", "Playlists"),
        (View::Search, "5", "Search"),
        (View::NowPlaying, "6", "Playing"),
        (View::Goodies, "7", "Stats"),
        (View::Info, "8", "Info"),
    ];

    let mut spans: Vec<Span> = Vec::new();
    spans.push(Span::raw(" "));
    for (v, key, label) in order {
        let active = v == app.view;
        let chip = format!(" {key} {label} ");
        let style = if active {
            Style::default()
                .fg(app.theme.bg_chip)
                .bg(app.theme.accent)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(app.theme.fg_muted)
        };
        spans.push(Span::styled(chip, style));
        spans.push(Span::raw(" "));
    }
    f.render_widget(Paragraph::new(Line::from(spans)), area);

    // Right-aligned lyrics toggle — only shown on Now Playing, since that's
    // the only view that renders the lyrics panel.
    if app.view == View::NowPlaying {
        let lyrics_chip = " L Lyrics ";
        let lyrics_style = if app.show_lyrics {
            Style::default()
                .fg(app.theme.bg_chip)
                .bg(app.theme.accent_alt)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(app.theme.fg_muted)
        };
        let lyrics_w = lyrics_chip.chars().count() as u16 + 1;
        if area.width > lyrics_w {
            let r = Rect {
                x: area.x + area.width - lyrics_w,
                y: area.y,
                width: lyrics_w,
                height: 1,
            };
            f.render_widget(
                Paragraph::new(Span::styled(lyrics_chip, lyrics_style)),
                r,
            );
        }
    }
}

fn render_body(f: &mut Frame, app: &mut App, area: Rect) {
    match app.view {
        View::Library => library::render(f, app, area),
        View::Albums => albums::render(f, app, area),
        View::Queue => queue::render(f, app, area),
        View::NowPlaying => now_playing::render(f, app, area),
        View::Search => search::render(f, app, area),
        View::Playlists => playlists::render(f, app, area),
        View::Goodies => stats::render(f, app, area),
        View::Info => info::render(f, app, area),
        View::Help => help::render(f, app, area),
    }
}
