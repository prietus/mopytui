use ratatui::Frame;
use ratatui::layout::{Constraint, Direction, Layout, Margin, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Borders, Cell, Padding, Paragraph, Row, Table, Wrap};

use crate::app::App;

pub fn render(f: &mut Frame, app: &mut App, area: Rect) {
    // Show the cover panel on the left when there's a current track and the
    // terminal has room. Cover gets ~42% of width (clamped 44..72 cells) so
    // it stays prominent on wide windows.
    let has_current = app.playback.current.is_some() && area.width >= 90;
    let cols = if has_current {
        let cover_w = (area.width as u32 * 42 / 100).clamp(44, 72) as u16;
        Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Length(cover_w), Constraint::Min(40)])
            .split(area)
    } else {
        Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Min(0)])
            .split(area)
    };

    if has_current {
        render_cover_panel(f, app, cols[0]);
        render_queue_table(f, app, cols[1]);
    } else {
        render_queue_table(f, app, cols[0]);
    }
}

fn render_cover_panel(f: &mut Frame, app: &mut App, area: Rect) {
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(app.theme.border))
        .padding(Padding::symmetric(1, 0));
    let inner = block.inner(area);
    f.render_widget(block, area);

    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(8), Constraint::Length(6)])
        .split(inner);

    crate::images::render_cover_widget(f, app, rows[0]);
    render_meta_under_cover(f, app, rows[1]);
}

fn render_meta_under_cover(f: &mut Frame, app: &App, area: Rect) {
    let inner = area.inner(Margin::new(0, 0));
    let Some(t) = &app.playback.current else { return; };
    let artist = t.artists_joined();
    let album = t.album_name().to_string();
    let year = t.date.clone().unwrap_or_default();
    let genre = t.genre.clone().unwrap_or_default();

    let mut lines: Vec<Line> = Vec::new();
    if !artist.is_empty() {
        lines.push(Line::from(vec![
            Span::styled(
                "▸ ",
                Style::default().fg(app.theme.ok).add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                artist,
                Style::default()
                    .fg(app.theme.fg_strong)
                    .add_modifier(Modifier::BOLD),
            ),
        ]));
    }
    let mut album_line: Vec<Span> = Vec::new();
    if !album.is_empty() {
        album_line.push(Span::styled(album, Style::default().fg(app.theme.fg)));
    }
    if !year.is_empty() {
        album_line.push(Span::styled("  ·  ", Style::default().fg(app.theme.fg_muted)));
        album_line.push(Span::styled(year, Style::default().fg(app.theme.fg_muted)));
    }
    lines.push(Line::from(album_line));
    if !genre.is_empty() {
        lines.push(Line::from(Span::styled(
            genre,
            Style::default().fg(app.theme.fg_muted),
        )));
    }

    // Played-count if we recognise the track in goodies stats.
    if let Some(c) = goodies_play_count(app, &t.uri) {
        lines.push(Line::from(""));
        lines.push(Line::from(vec![
            Span::styled(
                "played ",
                Style::default().fg(app.theme.fg_muted),
            ),
            Span::styled(
                format!("{c}×"),
                Style::default()
                    .fg(app.theme.accent)
                    .add_modifier(Modifier::BOLD),
            ),
        ]));
    }

    f.render_widget(Paragraph::new(lines).wrap(Wrap { trim: true }), inner);
}

fn goodies_play_count(app: &App, uri: &str) -> Option<u32> {
    app.goodies
        .most
        .iter()
        .find(|i| i.uri == uri)
        .and_then(|i| i.count)
}

fn render_queue_table(f: &mut Frame, app: &mut App, area: Rect) {
    let title = format!(" Queue — {} ", app.queue.len());

    let header = Row::new(vec![
        Cell::from("  #").style(Style::default().fg(app.theme.fg_muted)),
        Cell::from("Artist").style(Style::default().fg(app.theme.fg_muted)),
        Cell::from("Title").style(Style::default().fg(app.theme.fg_muted)),
        Cell::from("Album").style(Style::default().fg(app.theme.fg_muted)),
        Cell::from("Len").style(Style::default().fg(app.theme.fg_muted)),
    ])
    .style(Style::default().add_modifier(Modifier::BOLD));

    let rows: Vec<Row> = app
        .queue
        .iter()
        .enumerate()
        .map(|(i, tl)| {
            let is_current = Some(tl.tlid) == app.playback.current_tlid;
            let marker = if is_current { " ▶" } else { "  " };
            let style = if is_current {
                Style::default()
                    .fg(app.theme.accent_alt)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(app.theme.fg)
            };
            Row::new(vec![
                Cell::from(format!("{marker}{:>3}", i + 1)),
                Cell::from(tl.track.artists_joined()),
                Cell::from(tl.track.name.clone()),
                Cell::from(tl.track.album_name().to_string()),
                Cell::from(tl.track.length.map(|ms| fmt_ms(ms as i64)).unwrap_or_default()),
            ])
            .style(style)
        })
        .collect();

    let widths = [
        Constraint::Length(7),
        Constraint::Percentage(25),
        Constraint::Percentage(33),
        Constraint::Percentage(33),
        Constraint::Length(6),
    ];

    let table = Table::new(rows, widths)
        .header(header)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_type(BorderType::Rounded)
                .title(Line::from(Span::styled(
                    title,
                    Style::default()
                        .fg(app.theme.fg_strong)
                        .add_modifier(Modifier::BOLD),
                )))
                .border_style(Style::default().fg(app.theme.accent))
                .padding(Padding::horizontal(1)),
        )
        .row_highlight_style(
            Style::default()
                .bg(app.theme.selection_bg)
                .add_modifier(Modifier::BOLD),
        )
        .highlight_symbol("");

    f.render_stateful_widget(table, area, &mut app.queue_state.table);
}

fn fmt_ms(ms: i64) -> String {
    let s = (ms / 1000).max(0);
    format!("{:02}:{:02}", s / 60, s % 60)
}
