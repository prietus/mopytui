use ratatui::Frame;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Bar, BarChart, BarGroup, Block, BorderType, Borders, List, ListItem, Paragraph};

use crate::app::{App, GoodiesTab};

pub fn render(f: &mut Frame, app: &mut App, area: Rect) {
    if !app.goodies.available {
        let p = Paragraph::new(vec![
            Line::from(Span::styled(
                "tidal_goodies plugin not installed",
                Style::default()
                    .fg(app.theme.warn)
                    .add_modifier(Modifier::BOLD),
            )),
            Line::from(""),
            Line::from(Span::styled(
                "Stats and the favorites toggle require mopidy-tidal-goodies",
                Style::default().fg(app.theme.fg_muted),
            )),
            Line::from(Span::styled(
                "on the server. Once installed, this tab unlocks automatically.",
                Style::default().fg(app.theme.fg_muted),
            )),
        ])
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_type(BorderType::Rounded)
                .title(" Stats ")
                .border_style(Style::default().fg(app.theme.border)),
        );
        f.render_widget(p, area);
        return;
    }

    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(1), Constraint::Min(0)])
        .split(area);

    render_tabs(f, app, rows[0]);

    match app.goodies.tab {
        GoodiesTab::Recent | GoodiesTab::MostPlayed | GoodiesTab::TopArtists | GoodiesTab::TopAlbums => {
            render_list(f, app, rows[1]);
        }
        GoodiesTab::Heatmap => render_heatmap(f, app, rows[1]),
        GoodiesTab::Genres => render_genres(f, app, rows[1]),
        GoodiesTab::Totals => render_totals(f, app, rows[1]),
    }
}

fn render_tabs(f: &mut Frame, app: &App, area: Rect) {
    let tabs = [
        GoodiesTab::Recent,
        GoodiesTab::MostPlayed,
        GoodiesTab::TopArtists,
        GoodiesTab::TopAlbums,
        GoodiesTab::Heatmap,
        GoodiesTab::Genres,
        GoodiesTab::Totals,
    ];
    let mut spans = Vec::new();
    for t in tabs {
        let active = app.goodies.tab == t;
        let style = if active {
            Style::default()
                .fg(app.theme.bg_chip)
                .bg(app.theme.accent_alt)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(app.theme.fg_muted)
        };
        spans.push(Span::raw(" "));
        spans.push(Span::styled(format!(" {} ", t.label()), style));
    }
    f.render_widget(Paragraph::new(Line::from(spans)), area);
}

fn render_list(f: &mut Frame, app: &mut App, area: Rect) {
    let items = match app.goodies.tab {
        GoodiesTab::Recent => &app.goodies.recent,
        _ => &app.goodies.most,
    };
    let favs = &app.goodies.favorites;
    let list_items: Vec<ListItem> = items
        .iter()
        .map(|i| {
            let count_text = i.count.map(|c| format!("  ×{c}")).unwrap_or_default();
            let star = match crate::app::tidal_album_id(&i.uri) {
                Some(id) if favs.contains(id) => "★ ",
                _ => "  ",
            };
            ListItem::new(Line::from(vec![
                Span::styled(star, Style::default().fg(app.theme.warn)),
                Span::styled(
                    i.title.clone(),
                    Style::default().fg(app.theme.fg_strong).add_modifier(Modifier::BOLD),
                ),
                Span::styled(
                    format!("  ·  {}", i.subtitle),
                    Style::default().fg(app.theme.fg_muted),
                ),
                Span::styled(count_text, Style::default().fg(app.theme.accent)),
            ]))
        })
        .collect();
    let list = List::new(list_items)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_type(BorderType::Rounded)
                .title(Line::from(Span::styled(
                    format!(" {} ", app.goodies.tab.label()),
                    Style::default()
                        .fg(app.theme.fg_strong)
                        .add_modifier(Modifier::BOLD),
                )))
                .border_style(Style::default().fg(app.theme.border)),
        )
        .highlight_style(
            Style::default()
                .bg(app.theme.selection_bg)
                .add_modifier(Modifier::BOLD),
        )
        .highlight_symbol(" ▌");
    f.render_stateful_widget(list, area, &mut app.goodies.state);
}

fn render_heatmap(f: &mut Frame, app: &App, area: Rect) {
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .title(" When you listen ")
        .border_style(Style::default().fg(app.theme.border));
    let inner = block.inner(area);
    f.render_widget(block, area);

    if app.goodies.heatmap_hours.is_empty() && app.goodies.heatmap_dow.is_empty() {
        f.render_widget(
            Paragraph::new(Span::styled(
                "Loading…",
                Style::default().fg(app.theme.fg_muted),
            )),
            inner,
        );
        return;
    }

    let split = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(2),         // by-hour title
            Constraint::Percentage(50),    // hours bar chart
            Constraint::Length(2),         // by-dow title
            Constraint::Min(0),            // dow bar chart
        ])
        .split(inner);

    // ── hours ──
    f.render_widget(
        Paragraph::new(Span::styled(
            "  by hour of day",
            Style::default()
                .fg(app.theme.accent_alt)
                .add_modifier(Modifier::BOLD),
        )),
        split[0],
    );
    let labels: Vec<String> = (0..24).map(|h| format!("{h:02}")).collect();
    let bars: Vec<Bar> = labels
        .iter()
        .zip(app.goodies.heatmap_hours.iter())
        .map(|(label, &v)| {
            Bar::default()
                .value(v)
                .label(Line::from(label.clone()))
                .style(Style::default().fg(app.theme.accent))
                .value_style(Style::default().fg(app.theme.fg_strong))
        })
        .collect();
    let bw = ((split[1].width.saturating_sub(2) / 24).saturating_sub(1)).max(1);
    let chart = BarChart::default()
        .data(BarGroup::default().bars(&bars))
        .bar_width(bw)
        .bar_gap(0)
        .bar_style(Style::default().fg(app.theme.accent));
    f.render_widget(chart, split[1]);

    // ── days ──
    f.render_widget(
        Paragraph::new(Span::styled(
            "  by day of week",
            Style::default()
                .fg(app.theme.accent_alt)
                .add_modifier(Modifier::BOLD),
        )),
        split[2],
    );
    let dow_labels = ["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"];
    let bars: Vec<Bar> = dow_labels
        .iter()
        .zip(app.goodies.heatmap_dow.iter())
        .map(|(label, &v)| {
            Bar::default()
                .value(v)
                .label(Line::from((*label).to_string()))
                .style(Style::default().fg(app.theme.accent_alt))
                .value_style(Style::default().fg(app.theme.fg_strong))
        })
        .collect();
    let bw = ((split[3].width.saturating_sub(2) / 7).saturating_sub(1)).max(3);
    let chart = BarChart::default()
        .data(BarGroup::default().bars(&bars))
        .bar_width(bw)
        .bar_gap(1)
        .bar_style(Style::default().fg(app.theme.accent_alt));
    f.render_widget(chart, split[3]);
}

fn render_genres(f: &mut Frame, app: &App, area: Rect) {
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .title(format!(" Top genres ({}) ", app.goodies.genres.len()))
        .border_style(Style::default().fg(app.theme.border));
    let inner = block.inner(area);
    f.render_widget(block, area);

    if app.goodies.genres.is_empty() {
        f.render_widget(
            Paragraph::new(Span::styled(
                "Loading…",
                Style::default().fg(app.theme.fg_muted),
            )),
            inner,
        );
        return;
    }

    let take = (inner.height as usize).min(app.goodies.genres.len());
    let max = app.goodies.genres.iter().map(|(_, c)| *c).max().unwrap_or(1);
    let label_w = app
        .goodies
        .genres
        .iter()
        .take(take)
        .map(|(g, _)| g.chars().count())
        .max()
        .unwrap_or(8)
        .min(20) as u16;
    let count_w = 6u16;
    let bar_w = inner.width.saturating_sub(label_w + count_w + 4);

    let mut lines: Vec<Line> = Vec::with_capacity(take);
    for (genre, count) in app.goodies.genres.iter().take(take) {
        let g = truncate(genre, label_w as usize);
        let filled = ((*count as f64 / max as f64) * bar_w as f64).round() as usize;
        let filled = filled.min(bar_w as usize);
        let bar = "█".repeat(filled);
        let rest = "░".repeat((bar_w as usize).saturating_sub(filled));
        lines.push(Line::from(vec![
            Span::styled(
                format!("  {g:<lw$}  ", lw = label_w as usize),
                Style::default().fg(app.theme.fg),
            ),
            Span::styled(bar, Style::default().fg(app.theme.accent)),
            Span::styled(rest, Style::default().fg(app.theme.progress_empty)),
            Span::styled(
                format!("  {count:>cw$}", cw = (count_w - 2) as usize),
                Style::default()
                    .fg(app.theme.fg_strong)
                    .add_modifier(Modifier::BOLD),
            ),
        ]));
    }
    f.render_widget(Paragraph::new(lines), inner);
}

fn render_totals(f: &mut Frame, app: &App, area: Rect) {
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .title(" Listening totals ")
        .border_style(Style::default().fg(app.theme.border));
    let inner = block.inner(area);
    f.render_widget(block, area);

    let Some(t) = &app.goodies.totals else {
        f.render_widget(
            Paragraph::new(Span::styled(
                "Loading…",
                Style::default().fg(app.theme.fg_muted),
            )),
            inner,
        );
        return;
    };

    // Best-effort field extraction. Different goodies versions may name these
    // differently, so we look at a handful of common spellings.
    let pairs: Vec<(&str, String)> = [
        ("Plays", pick(t, &["plays", "total_plays", "play_count"])),
        ("Tracks", pick(t, &["tracks", "unique_tracks", "distinct_tracks"])),
        ("Albums", pick(t, &["albums", "unique_albums", "distinct_albums"])),
        ("Artists", pick(t, &["artists", "unique_artists", "distinct_artists"])),
        ("Hours", pick(t, &["hours", "total_hours", "listen_hours"])),
        ("Days active", pick(t, &["days_active", "active_days"])),
        ("First play", pick(t, &["first_play", "since"])),
        ("Last play", pick(t, &["last_play", "latest"])),
    ]
    .into_iter()
    .filter(|(_, v)| !v.is_empty())
    .collect();

    if pairs.is_empty() {
        // Fall back to a raw JSON dump so users still see something useful.
        let body = serde_json::to_string_pretty(t).unwrap_or_default();
        f.render_widget(
            Paragraph::new(body).style(Style::default().fg(app.theme.fg_muted)),
            inner,
        );
        return;
    }

    let cols = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
        .split(inner);

    let half = pairs.len().div_ceil(2);
    let left: Vec<Line> = pairs[..half.min(pairs.len())]
        .iter()
        .flat_map(|(k, v)| big_pair(app, k, v))
        .collect();
    let right: Vec<Line> = pairs[half.min(pairs.len())..]
        .iter()
        .flat_map(|(k, v)| big_pair(app, k, v))
        .collect();

    f.render_widget(Paragraph::new(left), cols[0]);
    f.render_widget(Paragraph::new(right), cols[1]);
}

fn big_pair(app: &App, k: &str, v: &str) -> Vec<Line<'static>> {
    vec![
        Line::from(""),
        Line::from(Span::styled(
            format!("  {k}"),
            Style::default()
                .fg(app.theme.accent)
                .add_modifier(Modifier::BOLD),
        )),
        Line::from(Span::styled(
            format!("  {v}"),
            Style::default()
                .fg(app.theme.fg_strong)
                .add_modifier(Modifier::BOLD),
        )),
    ]
}

fn pick(v: &serde_json::Value, keys: &[&str]) -> String {
    for k in keys {
        if let Some(val) = v.get(k) {
            if let Some(n) = val.as_u64() {
                return fmt_int(n);
            }
            if let Some(f) = val.as_f64() {
                return format!("{f:.1}");
            }
            if let Some(s) = val.as_str() {
                return s.to_string();
            }
        }
    }
    String::new()
}

fn fmt_int(n: u64) -> String {
    let s = n.to_string();
    let bytes = s.as_bytes();
    let mut out = String::with_capacity(s.len() + s.len() / 3);
    for (i, b) in bytes.iter().enumerate() {
        if i > 0 && (bytes.len() - i) % 3 == 0 {
            out.push(',');
        }
        out.push(*b as char);
    }
    out
}

fn truncate(s: &str, n: usize) -> String {
    let chars: Vec<char> = s.chars().collect();
    if chars.len() <= n { return s.to_string(); }
    let mut out: String = chars.iter().take(n.saturating_sub(1)).collect();
    out.push('…');
    out
}
