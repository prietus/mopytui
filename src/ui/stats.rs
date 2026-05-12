use ratatui::Frame;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{
    Block, BorderType, Borders, HighlightSpacing, List, ListItem, Paragraph, Sparkline,
};

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
        GoodiesTab::Recent | GoodiesTab::MostPlayed => render_list(f, app, rows[1]),
        GoodiesTab::TopArtists | GoodiesTab::TopAlbums => render_bars_list(f, app, rows[1]),
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

fn render_bars_list(f: &mut Frame, app: &mut App, area: Rect) {
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .title(Line::from(Span::styled(
            format!(" {} ", app.goodies.tab.label()),
            Style::default()
                .fg(app.theme.fg_strong)
                .add_modifier(Modifier::BOLD),
        )))
        .border_style(Style::default().fg(app.theme.border));
    let inner = block.inner(area);

    if app.goodies.most.is_empty() {
        let p = Paragraph::new(Span::styled(
            "Loading…",
            Style::default().fg(app.theme.fg_muted),
        ))
        .block(block);
        f.render_widget(p, area);
        return;
    }

    let favs = &app.goodies.favorites;
    let max = app
        .goodies
        .most
        .iter()
        .filter_map(|i| i.count)
        .max()
        .unwrap_or(1)
        .max(1);

    // Reserve 2 cols for the highlight symbol so bars never dance on select.
    let usable = inner.width.saturating_sub(2);
    let star_w: u16 = 2;
    let count_w: u16 = 6;
    let pad: u16 = 2;
    let title_w: u16 = ((usable as f32 * 0.4) as u16).clamp(12, 32);
    let bar_w: u16 = usable
        .saturating_sub(star_w + title_w + count_w + pad)
        .max(4);

    let list_items: Vec<ListItem> = app
        .goodies
        .most
        .iter()
        .map(|i| {
            let star = match crate::app::tidal_album_id(&i.uri) {
                Some(id) if favs.contains(id) => "★ ",
                _ => "  ",
            };
            let label = if i.subtitle.is_empty() {
                i.title.clone()
            } else {
                format!("{} · {}", i.title, i.subtitle)
            };
            let label = pad_or_truncate(&label, title_w as usize);
            let count = i.count.unwrap_or(0);
            let frac = (count as f64 / max as f64).clamp(0.0, 1.0);
            let filled = ((frac * bar_w as f64).round() as usize).min(bar_w as usize);
            let bar_filled = "█".repeat(filled);
            let bar_empty = "░".repeat((bar_w as usize).saturating_sub(filled));
            ListItem::new(Line::from(vec![
                Span::styled(star, Style::default().fg(app.theme.warn)),
                Span::styled(
                    label,
                    Style::default()
                        .fg(app.theme.fg_strong)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::raw("  "),
                Span::styled(bar_filled, Style::default().fg(app.theme.accent)),
                Span::styled(bar_empty, Style::default().fg(app.theme.progress_empty)),
                Span::styled(
                    format!(" {count:>cw$}", cw = (count_w - 1) as usize),
                    Style::default()
                        .fg(app.theme.fg_strong)
                        .add_modifier(Modifier::BOLD),
                ),
            ]))
        })
        .collect();

    let list = List::new(list_items)
        .block(block)
        .highlight_style(
            Style::default()
                .bg(app.theme.selection_bg)
                .add_modifier(Modifier::BOLD),
        )
        .highlight_symbol(" ▌")
        .highlight_spacing(HighlightSpacing::Always);
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
            Constraint::Length(2), // hours title
            Constraint::Length(2), // hours grid
            Constraint::Length(1), // hours labels
            Constraint::Length(2), // spacing
            Constraint::Length(2), // dow title
            Constraint::Length(2), // dow grid
            Constraint::Length(1), // dow labels
            Constraint::Length(1), // spacing
            Constraint::Min(0),    // peaks summary
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
    render_intensity_strip(f, app, split[1], &app.goodies.heatmap_hours, app.theme.accent);
    render_hour_labels(f, app, split[2]);

    // ── days ──
    f.render_widget(
        Paragraph::new(Span::styled(
            "  by day of week",
            Style::default()
                .fg(app.theme.accent_alt)
                .add_modifier(Modifier::BOLD),
        )),
        split[4],
    );
    render_intensity_strip(f, app, split[5], &app.goodies.heatmap_dow, app.theme.accent_alt);
    render_dow_labels(f, app, split[6]);

    render_heatmap_peaks(f, app, split[8]);
}

fn render_intensity_strip(f: &mut Frame, app: &App, area: Rect, data: &[u64], base: Color) {
    if data.is_empty() || area.width == 0 {
        return;
    }
    let n = data.len() as u16;
    // Two columns per cell, no gap, leaving a 2-col left pad.
    let left_pad: u16 = 2;
    let total = area.width.saturating_sub(left_pad);
    let cell_w = (total / n).max(1).min(6);
    let max = *data.iter().max().unwrap_or(&1);
    let max = max.max(1);

    let empty = app.theme.progress_empty;
    let mut spans: Vec<Span> = Vec::with_capacity(data.len() + 1);
    spans.push(Span::raw(" ".repeat(left_pad as usize)));
    for &v in data {
        let frac = v as f64 / max as f64;
        let color = blend_intensity(empty, base, frac);
        let cell: String = "█".repeat(cell_w as usize);
        spans.push(Span::styled(cell, Style::default().fg(color)));
    }
    // Render top half + bottom half (same content) for a thicker block.
    let line = Line::from(spans);
    f.render_widget(Paragraph::new(line.clone()), Rect { height: 1, ..area });
    if area.height >= 2 {
        let row2 = Rect {
            y: area.y + 1,
            height: 1,
            ..area
        };
        f.render_widget(Paragraph::new(line), row2);
    }
}

fn render_hour_labels(f: &mut Frame, app: &App, area: Rect) {
    if app.goodies.heatmap_hours.is_empty() {
        return;
    }
    let n = app.goodies.heatmap_hours.len() as u16;
    let left_pad: u16 = 2;
    let total = area.width.saturating_sub(left_pad);
    let cell_w = (total / n).max(1).min(6) as usize;

    let mut spans = vec![Span::raw(" ".repeat(left_pad as usize))];
    let stride = if cell_w >= 2 { 3 } else { 6 };
    for h in 0..24u32 {
        let chunk_w = cell_w;
        let txt = if h % stride == 0 {
            format!("{h:02}")
        } else {
            String::new()
        };
        let padded = format!("{txt:<chunk_w$}");
        spans.push(Span::styled(padded, Style::default().fg(app.theme.fg_muted)));
    }
    f.render_widget(Paragraph::new(Line::from(spans)), area);
}

fn render_dow_labels(f: &mut Frame, app: &App, area: Rect) {
    if app.goodies.heatmap_dow.is_empty() {
        return;
    }
    let n = app.goodies.heatmap_dow.len() as u16;
    let left_pad: u16 = 2;
    let total = area.width.saturating_sub(left_pad);
    let cell_w = (total / n).max(1).min(6) as usize;

    let labels = ["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"];
    let mut spans = vec![Span::raw(" ".repeat(left_pad as usize))];
    for label in labels {
        let txt: String = label.chars().take(cell_w).collect();
        let padded = format!("{txt:<cell_w$}");
        spans.push(Span::styled(padded, Style::default().fg(app.theme.fg_muted)));
    }
    f.render_widget(Paragraph::new(Line::from(spans)), area);
}

fn render_heatmap_peaks(f: &mut Frame, app: &App, area: Rect) {
    let peak_hour = app
        .goodies
        .heatmap_hours
        .iter()
        .enumerate()
        .max_by_key(|(_, v)| *v)
        .filter(|(_, v)| **v > 0)
        .map(|(i, _)| i);
    let dow_labels = ["Monday", "Tuesday", "Wednesday", "Thursday", "Friday", "Saturday", "Sunday"];
    let peak_dow = app
        .goodies
        .heatmap_dow
        .iter()
        .enumerate()
        .max_by_key(|(_, v)| *v)
        .filter(|(_, v)| **v > 0)
        .and_then(|(i, _)| dow_labels.get(i).copied());

    let mut parts: Vec<Span> = vec![Span::styled(
        "  peak:  ",
        Style::default().fg(app.theme.fg_muted),
    )];
    if let Some(h) = peak_hour {
        parts.push(Span::styled(
            format!("{h:02}:00"),
            Style::default()
                .fg(app.theme.accent)
                .add_modifier(Modifier::BOLD),
        ));
    }
    if peak_hour.is_some() && peak_dow.is_some() {
        parts.push(Span::raw("  ·  "));
    }
    if let Some(d) = peak_dow {
        parts.push(Span::styled(
            d.to_string(),
            Style::default()
                .fg(app.theme.accent_alt)
                .add_modifier(Modifier::BOLD),
        ));
    }
    f.render_widget(Paragraph::new(Line::from(parts)), area);
}

/// Linear blend from `lo` toward `hi` by `t∈[0,1]`. Works for any Color::Rgb;
/// falls back to `hi` for non-RGB colors (themes are 24-bit so this is fine).
fn blend_intensity(lo: Color, hi: Color, t: f64) -> Color {
    let t = t.clamp(0.0, 1.0);
    match (lo, hi) {
        (Color::Rgb(lr, lg, lb), Color::Rgb(hr, hg, hb)) => {
            let lerp = |a: u8, b: u8| -> u8 {
                let a = a as f64;
                let b = b as f64;
                (a + (b - a) * t).round().clamp(0.0, 255.0) as u8
            };
            Color::Rgb(lerp(lr, hr), lerp(lg, hg), lerp(lb, hb))
        }
        _ => hi,
    }
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

    // Split the inner area into a "big numbers" grid and an optional
    // sparkline at the bottom showing activity by hour of day.
    let has_spark = !app.goodies.heatmap_hours.is_empty() && inner.height >= 8;
    let layout = if has_spark {
        Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Min(0),    // grid
                Constraint::Length(1), // sparkline caption
                Constraint::Length(3), // sparkline
            ])
            .split(inner)
    } else {
        Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Min(0)])
            .split(inner)
    };

    let cols = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
        .split(layout[0]);

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

    if has_spark {
        f.render_widget(
            Paragraph::new(Span::styled(
                "  your day",
                Style::default()
                    .fg(app.theme.accent_alt)
                    .add_modifier(Modifier::BOLD),
            )),
            layout[1],
        );
        let spark = Sparkline::default()
            .data(&app.goodies.heatmap_hours)
            .style(Style::default().fg(app.theme.accent));
        let pad: u16 = 2;
        let spark_area = Rect {
            x: layout[2].x + pad,
            y: layout[2].y,
            width: layout[2].width.saturating_sub(pad),
            height: layout[2].height,
        };
        f.render_widget(spark, spark_area);
    }
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

/// Truncate to `n` chars (adding an ellipsis when shortened) and pad with
/// spaces so the result is always exactly `n` chars wide.
fn pad_or_truncate(s: &str, n: usize) -> String {
    let t = truncate(s, n);
    let len = t.chars().count();
    if len >= n {
        t
    } else {
        format!("{t}{}", " ".repeat(n - len))
    }
}
