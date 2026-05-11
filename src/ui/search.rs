use ratatui::Frame;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Borders, List, ListItem, Padding, Paragraph};

use crate::app::{App, SearchHit};

pub fn render(f: &mut Frame, app: &mut App, area: Rect) {
    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(3), Constraint::Min(0)])
        .split(area);

    render_input(f, app, rows[0]);
    render_results(f, app, rows[1]);
}

fn render_input(f: &mut Frame, app: &App, area: Rect) {
    let editing = app.search.editing;
    let border_color = if editing { app.theme.accent } else { app.theme.border };
    let title = if editing {
        Line::from(vec![
            Span::raw(" "),
            Span::styled(
                "Search",
                Style::default()
                    .fg(app.theme.accent)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                "  ·  Enter to run · Esc to leave ",
                Style::default().fg(app.theme.fg_muted),
            ),
        ])
    } else {
        Line::from(vec![
            Span::raw(" "),
            Span::styled(
                "Search",
                Style::default()
                    .fg(app.theme.fg_strong)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                "  ·  press / to type ",
                Style::default().fg(app.theme.fg_muted),
            ),
        ])
    };

    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .title(title)
        .border_style(Style::default().fg(border_color))
        .padding(Padding::horizontal(1));
    let inner = block.inner(area);
    f.render_widget(block, area);

    let prompt = if editing {
        Span::styled(
            "▶ ",
            Style::default()
                .fg(app.theme.accent)
                .add_modifier(Modifier::BOLD),
        )
    } else {
        Span::styled("◎ ", Style::default().fg(app.theme.fg_muted))
    };
    let text: Span = if app.search.input.is_empty() && !editing {
        Span::styled(
            "type to find tracks, albums, artists (Local + Tidal)",
            Style::default()
                .fg(app.theme.fg_muted)
                .add_modifier(Modifier::ITALIC),
        )
    } else if app.search.input.is_empty() {
        Span::styled(
            "type something…",
            Style::default()
                .fg(app.theme.fg_muted)
                .add_modifier(Modifier::ITALIC),
        )
    } else {
        Span::styled(
            app.search.input.clone(),
            Style::default()
                .fg(app.theme.fg_strong)
                .add_modifier(Modifier::BOLD),
        )
    };
    let cursor = if editing {
        Span::styled(
            "▏",
            Style::default()
                .fg(app.theme.accent)
                .add_modifier(Modifier::SLOW_BLINK | Modifier::BOLD),
        )
    } else {
        Span::raw("")
    };
    f.render_widget(
        Paragraph::new(Line::from(vec![prompt, text, cursor])),
        inner,
    );
}

fn render_results(f: &mut Frame, app: &mut App, area: Rect) {
    let favs = &app.goodies.favorites;
    let items: Vec<ListItem> = app
        .search
        .flat
        .iter()
        .map(|h| match h {
            SearchHit::Track(t) => ListItem::new(Line::from(vec![
                crate::ui::chips::source_chip(&t.uri, &app.theme),
                Span::styled("  ♪ ", Style::default().fg(app.theme.accent)),
                Span::styled(
                    t.name.clone(),
                    Style::default()
                        .fg(app.theme.fg_strong)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled(
                    format!("  ·  {}", t.artists_joined()),
                    Style::default().fg(app.theme.fg),
                ),
                Span::styled(
                    format!("  ·  {}", t.album_name()),
                    Style::default().fg(app.theme.fg_muted),
                ),
            ])),
            SearchHit::Album(a) => {
                let uri = a.uri.clone().unwrap_or_default();
                let starred = crate::app::tidal_album_id(&uri)
                    .map(|id| favs.contains(id))
                    .unwrap_or(false);
                let star = if starred { "★ " } else { "  " };
                ListItem::new(Line::from(vec![
                    crate::ui::chips::source_chip(&uri, &app.theme),
                    Span::styled(
                        format!(" {star}"),
                        Style::default()
                            .fg(app.theme.warn)
                            .add_modifier(Modifier::BOLD),
                    ),
                    Span::styled("□ ", Style::default().fg(app.theme.accent_alt)),
                    Span::styled(
                        a.name.clone(),
                        Style::default()
                            .fg(app.theme.fg_strong)
                            .add_modifier(Modifier::BOLD),
                    ),
                    Span::styled(
                        format!(
                            "  ·  {}",
                            a.artists.iter().map(|x| x.name.clone()).collect::<Vec<_>>().join(", ")
                        ),
                        Style::default().fg(app.theme.fg_muted),
                    ),
                ]))
            }
            SearchHit::Artist(a) => {
                let uri = a.uri.clone().unwrap_or_default();
                ListItem::new(Line::from(vec![
                    crate::ui::chips::source_chip(&uri, &app.theme),
                    Span::styled("  ▲ ", Style::default().fg(app.theme.warn)),
                    Span::styled(
                        a.name.clone(),
                        Style::default()
                            .fg(app.theme.fg_strong)
                            .add_modifier(Modifier::BOLD),
                    ),
                ]))
            }
        })
        .collect();

    let title_label = match &app.search.last_query {
        Some(q) if !items.is_empty() => format!(" Results for \"{q}\" — {} ", items.len()),
        Some(_) => " No results — try a different query ".to_string(),
        None => " Tip: search by track, artist, or album name ".to_string(),
    };

    let list = List::new(items)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_type(BorderType::Rounded)
                .title(Line::from(Span::styled(
                    title_label,
                    Style::default()
                        .fg(app.theme.fg_strong)
                        .add_modifier(Modifier::BOLD),
                )))
                .border_style(Style::default().fg(app.theme.border)),
        )
        .highlight_style(
            Style::default()
                .bg(app.theme.selection_bg)
                .fg(app.theme.selection_fg)
                .add_modifier(Modifier::BOLD),
        )
        .highlight_symbol(" ▌");
    f.render_stateful_widget(list, area, &mut app.search.state);
}
