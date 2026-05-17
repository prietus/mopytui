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

    let header_h: u16 = if h < 22 { 3 } else { 5 };

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
