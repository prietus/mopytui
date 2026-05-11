use ratatui::Frame;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Borders, List, ListItem, Paragraph};

use crate::app::{App, LibraryFocus};

pub fn render(f: &mut Frame, app: &mut App, area: Rect) {
    let cols = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(45), Constraint::Percentage(55)])
        .split(area);

    render_entries(f, app, cols[0]);
    render_right(f, app, cols[1]);
}

fn render_entries(f: &mut Frame, app: &mut App, area: Rect) {
    let title = if app.library.crumbs.is_empty() {
        " Library ".to_string()
    } else {
        let path: Vec<&str> = app
            .library
            .crumbs
            .iter()
            .map(|(_, n)| n.as_str())
            .collect();
        format!(" Library › {} ", path.join(" › "))
    };

    let favs = &app.goodies.favorites;
    let items: Vec<ListItem> = app
        .library
        .entries
        .iter()
        .map(|e| {
            let icon = match e.kind.as_str() {
                "directory" => "▸",
                "album" => "□",
                "artist" => "♪",
                "playlist" => "≡",
                "track" => "·",
                _ => " ",
            };
            let star = match crate::app::tidal_album_id(&e.uri) {
                Some(id) if favs.contains(id) => "★ ",
                _ => "  ",
            };
            ListItem::new(Line::from(vec![
                crate::ui::chips::source_chip(&e.uri, &app.theme),
                Span::styled(format!(" {icon} "), Style::default().fg(app.theme.accent_alt)),
                Span::styled(star, Style::default().fg(app.theme.warn)),
                Span::styled(e.name.clone(), Style::default().fg(app.theme.fg)),
            ]))
        })
        .collect();

    let focused = app.library.focus == LibraryFocus::Entries;
    let border_color = if focused { app.theme.accent } else { app.theme.border };
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .title(title)
        .border_style(Style::default().fg(border_color));

    let list = List::new(items)
        .block(block)
        .highlight_style(
            Style::default()
                .bg(app.theme.selection_bg)
                .fg(app.theme.selection_fg)
                .add_modifier(Modifier::BOLD),
        )
        .highlight_symbol(" ▌");
    f.render_stateful_widget(list, area, &mut app.library.entries_state);
}

fn render_right(f: &mut Frame, app: &mut App, area: Rect) {
    let focused = app.library.focus == LibraryFocus::Tracks;
    let border_color = if focused { app.theme.accent } else { app.theme.border };

    let Some(tracks) = app.library.album_tracks.clone() else {
        let p = Paragraph::new(vec![
            Line::from(Span::styled(
                "Select an album",
                Style::default().fg(app.theme.fg_muted),
            )),
            Line::from(""),
            Line::from(Span::styled(
                "Enter on a directory to descend, on an album to load its tracks.",
                Style::default().fg(app.theme.fg_muted),
            )),
        ])
        .block(
            Block::default()
                .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
                .title(" Tracks ")
                .border_style(Style::default().fg(border_color)),
        );
        f.render_widget(p, area);
        return;
    };

    let items: Vec<ListItem> = tracks
        .iter()
        .map(|t| {
            let no = t.track_no.map(|n| format!("{n:>2} ")).unwrap_or_default();
            let len = t
                .length
                .map(|ms| format!("  {}", fmt_ms(ms as i64)))
                .unwrap_or_default();
            ListItem::new(Line::from(vec![
                Span::styled(no, Style::default().fg(app.theme.fg_muted)),
                Span::styled(t.name.clone(), Style::default().fg(app.theme.fg)),
                Span::styled(len, Style::default().fg(app.theme.fg_muted)),
            ]))
        })
        .collect();

    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .title(format!(" Tracks ({}) ", tracks.len()))
        .border_style(Style::default().fg(border_color));
    let list = List::new(items)
        .block(block)
        .highlight_style(
            Style::default()
                .bg(app.theme.selection_bg)
                .fg(app.theme.selection_fg)
                .add_modifier(Modifier::BOLD),
        )
        .highlight_symbol(" ▌");
    f.render_stateful_widget(list, area, &mut app.library.album_tracks_state);
}

fn fmt_ms(ms: i64) -> String {
    let s = (ms / 1000).max(0);
    format!("{:02}:{:02}", s / 60, s % 60)
}
