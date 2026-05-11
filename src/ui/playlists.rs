use ratatui::Frame;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Borders, List, ListItem};

use crate::app::{App, PlaylistsFocus};

pub fn render(f: &mut Frame, app: &mut App, area: Rect) {
    let cols = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(40), Constraint::Percentage(60)])
        .split(area);

    let list_focused = app.playlists.focus == PlaylistsFocus::List;
    let tracks_focused = app.playlists.focus == PlaylistsFocus::Tracks;

    let items: Vec<ListItem> = app
        .playlists
        .items
        .iter()
        .map(|p| {
            ListItem::new(Line::from(vec![
                Span::styled(" ≡ ", Style::default().fg(app.theme.accent_alt)),
                Span::styled(p.name.clone(), Style::default().fg(app.theme.fg)),
            ]))
        })
        .collect();
    let list = List::new(items)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_type(BorderType::Rounded)
                .title(format!(" Playlists ({}) ", app.playlists.items.len()))
                .border_style(Style::default().fg(if list_focused {
                    app.theme.accent
                } else {
                    app.theme.border
                })),
        )
        .highlight_style(
            Style::default()
                .bg(app.theme.selection_bg)
                .add_modifier(Modifier::BOLD),
        )
        .highlight_symbol(" ▌");
    f.render_stateful_widget(list, cols[0], &mut app.playlists.state);

    let track_items: Vec<ListItem> = app
        .playlists
        .current
        .as_ref()
        .map(|p| {
            p.tracks
                .iter()
                .map(|t| {
                    ListItem::new(Line::from(vec![
                        Span::styled(" · ", Style::default().fg(app.theme.fg_muted)),
                        Span::styled(t.name.clone(), Style::default().fg(app.theme.fg)),
                        Span::styled(
                            format!("  · {}", t.artists_joined()),
                            Style::default().fg(app.theme.fg_muted),
                        ),
                    ]))
                })
                .collect()
        })
        .unwrap_or_default();
    let tlist = List::new(track_items)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_type(BorderType::Rounded)
                .title(
                    app.playlists
                        .current
                        .as_ref()
                        .map(|p| format!(" {} ", p.name))
                        .unwrap_or_else(|| " Tracks ".into()),
                )
                .border_style(Style::default().fg(if tracks_focused {
                    app.theme.accent
                } else {
                    app.theme.border
                })),
        )
        .highlight_style(
            Style::default()
                .bg(app.theme.selection_bg)
                .add_modifier(Modifier::BOLD),
        )
        .highlight_symbol(" ▌");
    f.render_stateful_widget(tlist, cols[1], &mut app.playlists.tracks_state);
}
