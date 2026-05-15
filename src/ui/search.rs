use ratatui::Frame;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Borders, List, ListItem, Padding, Paragraph};

use crate::app::{App, SearchField, SearchFocus, SearchHit};

/// Height of the form panel: 8 fields + 2 separators + 1 sources + 1 buttons
/// + 2 borders + 1 top/bottom padding.
const FORM_HEIGHT: u16 = 14;

pub fn render(f: &mut Frame, app: &mut App, area: Rect) {
    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(FORM_HEIGHT), Constraint::Min(0)])
        .split(area);

    render_form(f, app, rows[0]);
    render_results(f, app, rows[1]);
}

fn render_form(f: &mut Frame, app: &App, area: Rect) {
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(app.theme.accent))
        .padding(Padding::horizontal(2))
        .title(Line::from(vec![
            Span::raw(" "),
            Span::styled(
                "Search engine",
                Style::default()
                    .fg(app.theme.accent)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                "  ·  ↑/↓ Tab nav · type to edit · Enter run · r reset ",
                Style::default().fg(app.theme.fg_muted),
            ),
        ]));
    let inner = block.inner(area);
    f.render_widget(block, area);

    // Vertical layout: 8 field rows, sep, sources, sep, buttons.
    let constraints = vec![
        Constraint::Length(1), // Any
        Constraint::Length(1), // Artist
        Constraint::Length(1), // Album Artist
        Constraint::Length(1), // Album
        Constraint::Length(1), // Title
        Constraint::Length(1), // Genre
        Constraint::Length(1), // Date
        Constraint::Length(1), // Comment
        Constraint::Length(1), // Sources row
        Constraint::Length(1), // Buttons row
        Constraint::Min(0),
    ];
    let lines = Layout::default()
        .direction(Direction::Vertical)
        .constraints(constraints)
        .split(inner);

    // Fields
    let label_width = 14;
    for (i, field) in SearchField::ALL.iter().enumerate() {
        let focused = app.search.focus == SearchFocus::Field(i);
        let value = app.search.form.get(*field);
        let label_style = if focused {
            Style::default()
                .fg(app.theme.accent)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default()
                .fg(app.theme.fg_strong)
                .add_modifier(Modifier::BOLD)
        };
        let value_span = if value.is_empty() {
            Span::styled(
                "<empty>".to_string(),
                Style::default()
                    .fg(app.theme.fg_muted)
                    .add_modifier(Modifier::ITALIC),
            )
        } else {
            Span::styled(
                value.to_string(),
                Style::default()
                    .fg(app.theme.fg_strong)
                    .add_modifier(Modifier::BOLD),
            )
        };
        let mut spans = vec![
            Span::styled(
                if focused { "▶ " } else { "  " },
                Style::default().fg(app.theme.accent),
            ),
            Span::styled(format!("{:<w$}", field.label(), w = label_width), label_style),
            Span::styled(": ", Style::default().fg(app.theme.fg_muted)),
            value_span,
        ];
        if focused {
            spans.push(Span::styled(
                "▏",
                Style::default()
                    .fg(app.theme.accent)
                    .add_modifier(Modifier::SLOW_BLINK | Modifier::BOLD),
            ));
        }
        f.render_widget(Paragraph::new(Line::from(spans)), lines[i]);
    }

    // Sources row.
    let local_focused = app.search.focus == SearchFocus::Source(0);
    let tidal_focused = app.search.focus == SearchFocus::Source(1);
    let chk = |on: bool| if on { "[✓]" } else { "[ ]" };
    let sources_spans = vec![
        Span::raw("  "),
        Span::styled(
            format!("{:<w$}", "Sources", w = label_width),
            Style::default()
                .fg(app.theme.fg_strong)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(": ", Style::default().fg(app.theme.fg_muted)),
        Span::styled(
            chk(app.search.form.local),
            if local_focused {
                Style::default()
                    .fg(app.theme.accent)
                    .add_modifier(Modifier::BOLD | Modifier::REVERSED)
            } else if app.search.form.local {
                Style::default().fg(app.theme.ok).add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(app.theme.fg_muted)
            },
        ),
        Span::styled(
            " Local",
            if local_focused {
                Style::default()
                    .fg(app.theme.accent)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(app.theme.fg)
            },
        ),
        Span::raw("   "),
        Span::styled(
            chk(app.search.form.tidal),
            if tidal_focused {
                Style::default()
                    .fg(app.theme.accent)
                    .add_modifier(Modifier::BOLD | Modifier::REVERSED)
            } else if app.search.form.tidal {
                Style::default().fg(app.theme.ok).add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(app.theme.fg_muted)
            },
        ),
        Span::styled(
            " Tidal",
            if tidal_focused {
                Style::default()
                    .fg(app.theme.accent)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(app.theme.fg)
            },
        ),
    ];
    f.render_widget(Paragraph::new(Line::from(sources_spans)), lines[8]);

    // Buttons row.
    let search_focused = app.search.focus == SearchFocus::SearchBtn;
    let reset_focused = app.search.focus == SearchFocus::ResetBtn;
    let btn = |text: &str, focused: bool, accent: ratatui::style::Color| {
        if focused {
            Span::styled(
                format!("[ {text} ]"),
                Style::default()
                    .fg(app.theme.bg)
                    .bg(accent)
                    .add_modifier(Modifier::BOLD),
            )
        } else {
            Span::styled(
                format!("[ {text} ]"),
                Style::default().fg(accent).add_modifier(Modifier::BOLD),
            )
        }
    };
    let buttons = vec![
        Span::raw("  "),
        btn("Search", search_focused, app.theme.accent),
        Span::raw("   "),
        btn("Reset", reset_focused, app.theme.warn),
    ];
    f.render_widget(Paragraph::new(Line::from(buttons)), lines[9]);
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
                    Span::styled("◉ ", Style::default().fg(app.theme.accent_alt)),
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
        Some(q) if !items.is_empty() => format!(" Results · {q} — {} ", items.len()),
        Some(_) => " No results — adjust filters ".to_string(),
        None => " Fill fields and press Enter (or Search) ".to_string(),
    };

    let focused = app.search.focus == SearchFocus::Results;
    let border_color = if focused { app.theme.accent } else { app.theme.border };

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
                .border_style(Style::default().fg(border_color)),
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
