use ratatui::Frame;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Borders, List, ListItem, Padding, Paragraph};

use crate::app::{App, SearchField, SearchFocus, SearchHit};

/// Height of the form panel: 8 fields + 2 separators + 1 sources + 1 buttons
/// + 2 borders + 1 top/bottom padding.
const FORM_HEIGHT: u16 = 14;

/// Height of the collapsed bar: one line of content + 2 borders.
const BAR_HEIGHT: u16 = 3;

pub fn render(f: &mut Frame, app: &mut App, area: Rect) {
    let bar_h = if app.search.show_filters { FORM_HEIGHT } else { BAR_HEIGHT };
    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(bar_h), Constraint::Min(0)])
        .split(area);

    if app.search.show_filters {
        render_form(f, app, rows[0]);
    } else {
        render_bar(f, app, rows[0]);
    }
    // Results on the left, detail of the highlighted one on the right. Too
    // narrow a terminal gets the results alone.
    if rows[1].width >= 90 {
        let cols = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(45), Constraint::Percentage(55)])
            .split(rows[1]);
        render_results(f, app, cols[0]);
        render_detail(f, app, cols[1]);
    } else {
        render_results(f, app, rows[1]);
    }
}

/// The collapsed search bar: the query, the sources, and a hint about the
/// advanced filters (with how many are filled in).
fn render_bar(f: &mut Frame, app: &App, area: Rect) {
    let focused_bar = matches!(
        app.search.focus,
        SearchFocus::Field(_) | SearchFocus::Source(_)
    );
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(if focused_bar { app.theme.accent } else { app.theme.border }))
        .padding(Padding::horizontal(1))
        .title(Line::from(vec![
            Span::raw(" "),
            Span::styled(
                "Search",
                Style::default().fg(app.theme.accent).add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                "  ·  type · Enter run · Tab sources/results · Ctrl+F filters ",
                Style::default().fg(app.theme.fg_muted),
            ),
        ]));
    let inner = block.inner(area);
    f.render_widget(block, area);

    let editing = app.search.focus == SearchFocus::Field(0);
    let query = app.search.form.get(SearchField::ALL[0]);
    let mut spans = vec![
        Span::styled(
            if editing { "▶ " } else { "  " },
            Style::default().fg(app.theme.accent),
        ),
        if query.is_empty() {
            Span::styled(
                "search artists, albums, tracks…".to_string(),
                Style::default().fg(app.theme.fg_muted).add_modifier(Modifier::ITALIC),
            )
        } else {
            Span::styled(
                query.to_string(),
                Style::default().fg(app.theme.fg_strong).add_modifier(Modifier::BOLD),
            )
        },
    ];
    if editing {
        spans.push(Span::styled(
            "▏",
            Style::default().fg(app.theme.accent).add_modifier(Modifier::SLOW_BLINK | Modifier::BOLD),
        ));
    }
    spans.extend(source_toggles(app));
    let extra = SearchField::ALL[1..].iter().filter(|fl| !app.search.form.get(**fl).trim().is_empty()).count();
    if extra > 0 {
        spans.push(Span::styled(
            format!("   +{extra} filter{}", if extra == 1 { "" } else { "s" }),
            Style::default().fg(app.theme.warn).add_modifier(Modifier::BOLD),
        ));
    }
    f.render_widget(Paragraph::new(Line::from(spans)), inner);
}

/// `   [✓] Local   [✓] Tidal` with focus highlighting.
fn source_toggles(app: &App) -> Vec<Span<'static>> {
    let chk = |on: bool| if on { "[✓]" } else { "[ ]" };
    let mut out = vec![Span::raw("     ")];
    for (idx, (label, on)) in [("Local", app.search.form.local), ("Tidal", app.search.form.tidal)].into_iter().enumerate() {
        let focused = app.search.focus == SearchFocus::Source(idx);
        out.push(Span::styled(
            chk(on),
            if focused {
                Style::default().fg(app.theme.accent).add_modifier(Modifier::BOLD | Modifier::REVERSED)
            } else if on {
                Style::default().fg(app.theme.ok).add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(app.theme.fg_muted)
            },
        ));
        out.push(Span::styled(
            format!(" {label}"),
            if focused {
                Style::default().fg(app.theme.accent).add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(app.theme.fg)
            },
        ));
        out.push(Span::raw("   "));
    }
    out
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
                "  ·  ↑/↓ Tab nav · type to edit · Enter run · Ctrl+F hide filters ",
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
    use crate::search::SearchRow;
    let favs = &app.goodies.favorites;
    let items: Vec<ListItem> = app
        .search
        .rows
        .iter()
        .map(|row| match row {
            SearchRow::Source { label, count } => ListItem::new(Line::from(vec![
                Span::styled("── ", Style::default().fg(app.theme.border)),
                Span::styled(
                    *label,
                    Style::default().fg(app.theme.accent).add_modifier(Modifier::BOLD),
                ),
                Span::styled(format!(" · {count} ──"), Style::default().fg(app.theme.border)),
            ])),
            SearchRow::Kind { label, count } => ListItem::new(Line::from(Span::styled(
                format!("  {label} ({count})"),
                Style::default().fg(app.theme.fg_muted).add_modifier(Modifier::BOLD),
            ))),
            SearchRow::Hit(i) => match &app.search.flat[*i] {
                SearchHit::Track(t) => ListItem::new(Line::from(vec![
                    Span::styled("    ♪ ", Style::default().fg(app.theme.accent)),
                    Span::styled(
                        t.name.clone(),
                        Style::default().fg(app.theme.fg_strong).add_modifier(Modifier::BOLD),
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
                    ListItem::new(Line::from(vec![
                        Span::styled(
                            if starred { "  ★ " } else { "    " },
                            Style::default().fg(app.theme.warn).add_modifier(Modifier::BOLD),
                        ),
                        Span::styled("◉ ", Style::default().fg(app.theme.accent_alt)),
                        Span::styled(
                            a.name.clone(),
                            Style::default().fg(app.theme.fg_strong).add_modifier(Modifier::BOLD),
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
                SearchHit::Artist(a) => ListItem::new(Line::from(vec![
                    Span::styled("    ▲ ", Style::default().fg(app.theme.warn)),
                    Span::styled(
                        a.name.clone(),
                        Style::default().fg(app.theme.fg_strong).add_modifier(Modifier::BOLD),
                    ),
                ])),
            },
        })
        .collect();

    let hits = app.search.flat.len();
    let title_label = match &app.search.last_query {
        Some(q) if hits > 0 => format!(" Results · {q} — {hits} "),
        Some(_) => " No results — adjust the query or filters ".to_string(),
        None => " Type a query and press Enter ".to_string(),
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
                    Style::default().fg(app.theme.fg_strong).add_modifier(Modifier::BOLD),
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

// ─── detail panel ───────────────────────────────────────────────────────────

/// Detail of the highlighted result: cover, what it is, its track list and the
/// keys that act on it.
fn render_detail(f: &mut Frame, app: &mut App, area: Rect) {
    let focused = app.search.focus == SearchFocus::Detail;
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(if focused { app.theme.accent } else { app.theme.border }))
        .padding(Padding::horizontal(1))
        .title(Line::from(Span::styled(
            " Detail ",
            Style::default().fg(app.theme.fg_strong).add_modifier(Modifier::BOLD),
        )));
    let inner = block.inner(area);
    f.render_widget(block, area);

    let Some(hit) = app.search.selected_hit().cloned() else {
        f.render_widget(
            Paragraph::new(Span::styled(
                "Highlight a result to see its cover and details.",
                Style::default().fg(app.theme.fg_muted).add_modifier(Modifier::ITALIC),
            )),
            inner,
        );
        return;
    };

    let cover_h = 12u16.min(inner.height.saturating_sub(4)).max(4);
    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(cover_h),
            Constraint::Length(1),
            Constraint::Min(0),
            Constraint::Length(1),
        ])
        .split(inner);
    let top = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Length(cover_h * 2 + 2), Constraint::Min(10)])
        .split(rows[0]);

    // Cover (or a placeholder while it loads / when there is none).
    match hit.cover_key().filter(|k| app.images.contains(k)) {
        Some(key) => crate::images::render_info_image(f, app, top[0], &key),
        None => {
            let ph = Rect { width: top[0].width.saturating_sub(1), ..top[0] };
            f.render_widget(
                Paragraph::new(Span::styled("♪", Style::default().fg(app.theme.fg_muted)))
                    .alignment(ratatui::layout::Alignment::Center)
                    .block(
                        Block::default()
                            .borders(Borders::ALL)
                            .border_type(BorderType::Rounded)
                            .border_style(Style::default().fg(app.theme.border)),
                    ),
                ph,
            );
        }
    }

    f.render_widget(
        Paragraph::new(detail_lines(app, &hit)).wrap(ratatui::widgets::Wrap { trim: true }),
        top[1],
    );

    // Track list: the album's tracks (the matching one highlighted for a track hit).
    let playing_uri = match &hit {
        SearchHit::Track(t) => Some(t.uri.clone()),
        _ => None,
    };
    match &hit {
        SearchHit::Artist(_) => {
            f.render_widget(
                Paragraph::new(Span::styled(
                    "Enter browses this artist's albums · o starts a radio from them.",
                    Style::default().fg(app.theme.fg_muted),
                ))
                .wrap(ratatui::widgets::Wrap { trim: true }),
                rows[2],
            );
        }
        _ => {
            let tracks = hit.detail_key().and_then(|k| app.search.detail_cache.get(&k)).cloned();
            match tracks.as_ref() {
                None => f.render_widget(
                    Paragraph::new(Span::styled(
                        if hit.detail_key().is_some() { "Loading tracks…" } else { "" },
                        Style::default().fg(app.theme.fg_muted),
                    )),
                    rows[2],
                ),
                Some(tracks) if tracks.is_empty() => f.render_widget(
                    Paragraph::new(Span::styled("No track list.", Style::default().fg(app.theme.fg_muted))),
                    rows[2],
                ),
                Some(tracks) => {
                    let items: Vec<ListItem> = tracks
                        .iter()
                        .enumerate()
                        .map(|(i, t)| {
                            let len = t.length.map(|ms| {
                                let s = (ms / 1000) as i64;
                                format!("  {:02}:{:02}", s / 60, s % 60)
                            }).unwrap_or_default();
                            let is_hit = playing_uri.as_deref() == Some(t.uri.as_str());
                            let name_style = if is_hit {
                                Style::default().fg(app.theme.accent).add_modifier(Modifier::BOLD)
                            } else {
                                Style::default().fg(app.theme.fg)
                            };
                            ListItem::new(Line::from(vec![
                                Span::styled(
                                    format!("{:>3}  ", t.track_no.map(|n| n as usize).unwrap_or(i + 1)),
                                    Style::default().fg(app.theme.fg_muted),
                                ),
                                Span::styled(t.name.clone(), name_style),
                                Span::styled(len, Style::default().fg(app.theme.fg_muted)),
                            ]))
                        })
                        .collect();
                    f.render_widget(
                        Paragraph::new(Line::from(Span::styled(
                            format!("Tracks · {}", tracks.len()),
                            Style::default().fg(app.theme.accent).add_modifier(Modifier::BOLD),
                        ))),
                        rows[1],
                    );
                    // Only the focused list shows (and scrolls to) a selection.
                    let mut idle = ratatui::widgets::ListState::default();
                    let state = if focused { &mut app.search.detail_state } else { &mut idle };
                    let list = List::new(items)
                        .highlight_style(
                            Style::default()
                                .bg(app.theme.selection_bg)
                                .fg(app.theme.selection_fg)
                                .add_modifier(Modifier::BOLD),
                        )
                        .highlight_symbol("▌");
                    f.render_stateful_widget(list, rows[2], state);
                }
            }
        }
    }

    f.render_widget(Paragraph::new(Line::from(action_spans(app, &hit, focused))), rows[3]);
}

/// Title, artist, chips and one-line facts for the highlighted result.
fn detail_lines(app: &App, hit: &SearchHit) -> Vec<Line<'static>> {
    let t = &app.theme;
    let strong = Style::default().fg(t.fg_strong).add_modifier(Modifier::BOLD);
    let muted = Style::default().fg(t.fg_muted);
    let uri = hit.uri().to_string();
    let in_library = crate::app::tidal_album_id(match hit {
        SearchHit::Track(tr) => tr.album.as_ref().and_then(|a| a.uri.as_deref()).unwrap_or(""),
        _ => &uri,
    })
    .is_some_and(|id| app.goodies.favorites.contains(id));

    let (kind, title, artist, facts): (&str, String, String, Vec<String>) = match hit {
        SearchHit::Album(a) => (
            "Album",
            a.name.clone(),
            a.artists.iter().map(|x| x.name.clone()).collect::<Vec<_>>().join(", "),
            [
                a.date.clone().filter(|d| !d.is_empty()),
                a.num_tracks.map(|n| format!("{n} tracks")),
            ]
            .into_iter()
            .flatten()
            .collect(),
        ),
        SearchHit::Track(tr) => (
            "Track",
            tr.name.clone(),
            tr.artists_joined(),
            [
                Some(tr.album_name().to_string()).filter(|s| !s.is_empty()),
                tr.date.clone().filter(|d| !d.is_empty()),
                tr.length.map(|ms| {
                    let s = ms / 1000;
                    format!("{:02}:{:02}", s / 60, s % 60)
                }),
            ]
            .into_iter()
            .flatten()
            .collect(),
        ),
        SearchHit::Artist(a) => ("Artist", a.name.clone(), String::new(), Vec::new()),
    };

    let mut chips = vec![
        crate::ui::chips::source_chip(&uri, t),
        Span::styled(format!("  {kind}"), muted),
    ];
    if in_library {
        chips.push(Span::styled(
            "  ★ in your library",
            Style::default().fg(t.warn).add_modifier(Modifier::BOLD),
        ));
    }
    let mut lines = vec![Line::from(Span::styled(title, strong))];
    if !artist.is_empty() {
        lines.push(Line::from(Span::styled(artist, Style::default().fg(t.accent))));
    }
    lines.push(Line::from(chips));
    if !facts.is_empty() {
        lines.push(Line::from(Span::styled(facts.join("  ·  "), muted)));
    }
    lines
}

/// The keys that act on the highlighted result, as `[k] label` pills.
fn action_spans(app: &App, hit: &SearchHit, in_tracks: bool) -> Vec<Span<'static>> {
    let key = |k: &str, label: &str| -> Vec<Span<'static>> {
        vec![
            Span::styled(
                format!("[{k}] "),
                Style::default().fg(app.theme.accent).add_modifier(Modifier::BOLD),
            ),
            Span::styled(format!("{label}   "), Style::default().fg(app.theme.fg)),
        ]
    };
    let mut out = Vec::new();
    if in_tracks {
        out.extend(key("Enter", "add track"));
        out.extend(key("o", "radio"));
        out.extend(key("f", "library"));
        out.extend(key("h", "back"));
        return out;
    }
    match hit {
        SearchHit::Album(_) => {
            out.extend(key("p", "play"));
            out.extend(key("a", "queue"));
            out.extend(key("f", "library"));
            out.extend(key("→", "tracks"));
            out.extend(key("Enter", "open"));
        }
        SearchHit::Track(_) => {
            out.extend(key("Enter", "add"));
            out.extend(key("o", "radio"));
            out.extend(key("f", "library"));
            out.extend(key("→", "album tracks"));
        }
        SearchHit::Artist(_) => {
            out.extend(key("Enter", "browse"));
            out.extend(key("o", "radio"));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use ratatui::Terminal;
    use ratatui::backend::TestBackend;
    use ratatui_image::picker::Picker;

    use crate::app::{App, SearchHit, SearchFocus, View};
    use crate::config::AppConfig;
    use crate::images::ImageCache;
    use crate::mopidy::Client;
    use crate::mopidy::models::{Album, Artist, Track};

    fn app_with_results() -> App {
        let mut app = App::new(
            AppConfig::default(),
            Client::new("127.0.0.1", 6680),
            Arc::new(ImageCache::new()),
            Picker::halfblocks(),
            None,
        );
        app.view = View::Search;
        let artist = Artist { uri: Some("tidal:artist:9".into()), name: "Santana".into() };
        let album = |uri: &str, name: &str| Album {
            uri: Some(uri.into()),
            name: name.into(),
            artists: vec![artist.clone()],
            date: Some("1972".into()),
            num_tracks: Some(10),
        };
        let hits = vec![
            SearchHit::Track(Track {
                uri: "local:track:a".into(),
                name: "Stone Flower".into(),
                artists: vec![artist.clone()],
                album: Some(album("local:album:md5:x", "Caravanserai")),
                ..Default::default()
            }),
            SearchHit::Album(album("tidal:album:1", "Caravanserai")),
            SearchHit::Artist(artist.clone()),
        ];
        let (flat, rows) = crate::search::group_hits(hits);
        app.search.state.select(crate::search::first_hit_row(&rows));
        app.search.flat = flat;
        app.search.rows = rows;
        app.search.last_query = Some("any:santana".into());
        app.search.focus = SearchFocus::Results;
        app
    }

    fn screen(app: &mut App, w: u16, h: u16) -> String {
        let mut term = Terminal::new(TestBackend::new(w, h)).unwrap();
        term.draw(|f| super::render(f, app, f.area())).unwrap();
        let buf = term.backend().buffer().clone();
        (0..h)
            .map(|y| (0..w).map(|x| buf[(x, y)].symbol()).collect::<String>().trim_end().to_string())
            .collect::<Vec<_>>()
            .join("\n")
    }

    #[test]
    fn groups_results_by_source_and_shows_the_detail_of_the_highlighted_one() {
        let mut app = app_with_results();
        let s = screen(&mut app, 140, 40);
        println!("{s}");
        // Grouped list, Tidal before Local, each with kind sections.
        let (tidal, local) = (s.find("TIDAL").unwrap(), s.find("LOCAL").unwrap());
        assert!(tidal < local, "TIDAL section comes first");
        assert!(s.contains("ÁLBUMES (1)") && s.contains("ARTISTAS (1)") && s.contains("PISTAS (1)"));
        // Detail of the first hit (the Tidal album) on the right.
        assert!(s.contains("Detail"));
        assert!(s.contains("1972") && s.contains("10 tracks"));
        assert!(s.contains("[p] play") && s.contains("[f] library"));
        // Collapsed bar by default.
        assert!(s.contains("Ctrl+F filters") && !s.contains("Album Artist"));
    }

    #[test]
    fn filters_expand_with_ctrl_f_state_and_narrow_terminals_drop_the_detail() {
        let mut app = app_with_results();
        app.search.show_filters = true;
        let s = screen(&mut app, 140, 40);
        assert!(s.contains("Album Artist") && s.contains("Genre"));
        app.search.show_filters = false;
        let narrow = screen(&mut app, 80, 30);
        assert!(!narrow.contains("Detail") && narrow.contains("ÁLBUMES"));
    }

    fn key(code: crossterm::event::KeyCode) -> crossterm::event::KeyEvent {
        crossterm::event::KeyEvent::new(code, crossterm::event::KeyModifiers::NONE)
    }

    #[test]
    fn right_enters_the_detail_track_list_and_enter_adds_the_highlighted_track() {
        use crate::app::Cmd;
        use crossterm::event::KeyCode;
        let mut app = app_with_results();
        let tracks: Vec<Track> = (1..=3)
            .map(|n| Track { uri: format!("tidal:track:1:2:{n}"), name: format!("T{n}"), ..Default::default() })
            .collect();
        // Highlighted result: the Tidal album (first hit).
        assert_eq!(app.search.selected_hit().unwrap().uri(), "tidal:album:1");

        // Nothing loaded yet: → stays in the results.
        crate::input::handle_key(&mut app, key(KeyCode::Right));
        assert_eq!(app.search.focus, SearchFocus::Results);

        app.search.detail_cache.insert("tidal:album:1".into(), tracks);
        crate::input::handle_key(&mut app, key(KeyCode::Right));
        assert_eq!(app.search.focus, SearchFocus::Detail);
        assert_eq!(app.search.detail_state.selected(), Some(0));

        crate::input::handle_key(&mut app, key(KeyCode::Down));
        crate::input::handle_key(&mut app, key(KeyCode::Char('j')));
        crate::input::handle_key(&mut app, key(KeyCode::Char('j'))); // clamps at the last track
        assert_eq!(app.search.detail_state.selected(), Some(2));

        let cmd = crate::input::handle_key(&mut app, key(KeyCode::Enter));
        assert!(matches!(cmd, Cmd::Add(v) if v == vec!["tidal:track:1:2:3".to_string()]));

        // Esc goes back; moving to another result resets the track selection.
        crate::input::handle_key(&mut app, key(KeyCode::Esc));
        assert_eq!(app.search.focus, SearchFocus::Results);
        app.search.select_hit_delta(1);
        assert_eq!(app.search.detail_state.selected(), None);
    }

    #[test]
    fn focused_detail_shows_its_own_hints() {
        let mut app = app_with_results();
        app.search.detail_cache.insert(
            "tidal:album:1".into(),
            vec![Track { uri: "tidal:track:1:2:1".into(), name: "Song One".into(), ..Default::default() }],
        );
        app.search.focus = SearchFocus::Detail;
        app.search.detail_state.select(Some(0));
        let s = screen(&mut app, 140, 40);
        assert!(s.contains("Song One") && s.contains("[Enter] add track") && s.contains("[h] back"));
    }
}
