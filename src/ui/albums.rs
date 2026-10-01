use ratatui::Frame;
use ratatui::layout::{Alignment, Constraint, Direction, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Borders, List, ListItem, Padding, Paragraph, Wrap};
use ratatui_image::{Resize, StatefulImage};

use crate::app::{AlbumCard, AlbumDetail, AlbumsMode, AlbumSource, App};

pub fn render(f: &mut Frame, app: &mut App, area: Rect) {
    match app.albums.mode {
        AlbumsMode::Grid => render_grid(f, app, area),
        AlbumsMode::Detail => render_detail(f, app, area),
    }
}

// ─── grid ───────────────────────────────────────────────────────────────────

fn render_grid(f: &mut Frame, app: &mut App, area: Rect) {
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .title(Line::from(vec![
            Span::raw(" "),
            Span::styled(
                "Albums",
                Style::default()
                    .fg(app.theme.accent)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                format!("  ·  {} ", app.albums.items.len()),
                Style::default().fg(app.theme.fg_muted),
            ),
        ]))
        .border_style(Style::default().fg(app.theme.border))
        .padding(Padding::uniform(1));
    let inner = block.inner(area);
    f.render_widget(block, area);

    if app.albums.items.is_empty() {
        let msg = if app.albums.loading {
            "Loading albums…"
        } else if !app.albums.loaded {
            "Press 'r' to load your collection (browse local + tidal)."
        } else {
            "No albums found. Check that mopidy backends expose albums via browse."
        };
        f.render_widget(
            Paragraph::new(Span::styled(msg, Style::default().fg(app.theme.fg_muted))),
            inner,
        );
        return;
    }

    // Cell dimensions. We want the cover to render SQUARE in pixels, so
    // the height-in-cells must match width-in-cells × (cell_w_px / cell_h_px).
    // Then add 4 rows of text + 2 rows of border below.
    let fs = app.picker.font_size();
    let GridLayout { cols, cell_w, cell_h, rows: rows_visible } =
        grid_layout(inner.width, inner.height, fs.width, fs.height);
    app.albums.grid_cols = cols;
    app.albums.grid_rows = rows_visible;

    // Scroll selection into view.
    let sel = app.albums.grid_index.min(app.albums.items.len() - 1);
    let sel_row = sel / cols;
    if sel_row < app.albums.grid_offset_row {
        app.albums.grid_offset_row = sel_row;
    } else if sel_row >= app.albums.grid_offset_row + rows_visible {
        app.albums.grid_offset_row = sel_row + 1 - rows_visible;
    }

    let start = app.albums.grid_offset_row * cols;
    let end = (start + cols * rows_visible).min(app.albums.items.len());

    // Render cards into a grid. We iterate by global index so selection
    // highlighting works straight from app.albums.grid_index.
    let visible_uris: Vec<String> = app.albums.items[start..end]
        .iter()
        .map(|c| c.uri.clone())
        .collect();
    for (rel_idx, uri) in visible_uris.iter().enumerate() {
        let global_idx = start + rel_idx;
        let row = rel_idx / cols;
        let col = rel_idx % cols;
        let x = inner.x + col as u16 * cell_w;
        let y = inner.y + row as u16 * cell_h;
        // Last column gets the leftover width to avoid a one-cell gap.
        let w = if col + 1 == cols {
            inner.width - col as u16 * cell_w
        } else {
            cell_w.saturating_sub(1)
        };
        let cell_rect = Rect {
            x,
            y,
            width: w,
            height: cell_h.saturating_sub(1),
        };
        let selected = global_idx == app.albums.grid_index;
        // Re-fetch card by URI so we don't borrow app.albums.items immutably
        // while also mutating app (e.g. inserting into cover_protocols).
        let card = match app.albums.items.iter().find(|c| &c.uri == uri).cloned() {
            Some(c) => c,
            None => continue,
        };
        render_card(f, app, cell_rect, &card, selected);
    }
}

/// Shorten `text` so it wraps to at most `rows` lines of `width` columns,
/// cutting at a word boundary and ending with an ellipsis. Wrapping loses a
/// little at word edges, so only ~85% of the nominal capacity is used.
fn fit_text(text: &str, rows: usize, width: usize) -> String {
    let cap = (rows * width.max(1)) * 85 / 100;
    if text.chars().count() <= cap {
        return text.to_string();
    }
    let cut: String = text.chars().take(cap.saturating_sub(1)).collect();
    let cut = match cut.rfind(char::is_whitespace) {
        Some(i) => &cut[..i],
        None => &cut,
    };
    format!("{}…", cut.trim_end())
}

/// Rows of text under a cover: name, artist, source chip + year.
const CARD_TEXT_ROWS: u16 = 3;

#[derive(Debug, PartialEq, Eq)]
struct GridLayout {
    cols: usize,
    cell_w: u16,
    cell_h: u16,
    rows: usize,
}

/// Card size for a grid area of `w`×`h` cells and a font of `fw`×`fh` pixels.
///
/// Covers are kept square. Cards are as big as possible (28 cells wide) unless
/// that leaves room for only one row, in which case they shrink (down to 16
/// cells) until at least two rows of albums fit — a single row would leave
/// most of a tall panel empty.
fn grid_layout(w: u16, h: u16, fw: u16, fh: u16) -> GridLayout {
    let layout_for = |target_cell_w: u16| {
        let cell_w = target_cell_w.min(w).max(1);
        let cols = ((w / cell_w) as usize).max(1);
        let cell_w = (w / cols as u16).max(16);
        // Cover height (in cells) needed to be square at `cell_w` cells wide.
        // Strip 4 for the border + horizontal-padding columns the card adds.
        let cover_inner_w = cell_w.saturating_sub(4);
        let cover_h = ((cover_inner_w as u32 * fw as u32) / fh.max(1) as u32).max(5) as u16;
        let cell_h = cover_h + CARD_TEXT_ROWS + 2; // borders top + bottom
        let rows = ((h / cell_h) as usize).max(1);
        GridLayout { cols, cell_w, cell_h, rows }
    };
    (16..=28u16)
        .rev()
        .step_by(2)
        .map(layout_for)
        .find(|l| l.rows >= 2)
        .unwrap_or_else(|| layout_for(28))
}

fn render_card(f: &mut Frame, app: &mut App, area: Rect, card: &AlbumCard, selected: bool) {
    let border_style = if selected {
        Style::default()
            .fg(app.theme.accent)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(app.theme.border)
    };
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(border_style)
        .padding(Padding::horizontal(1));
    let inner = block.inner(area);
    f.render_widget(block, area);

    // Cover top, metadata below.
    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(3), Constraint::Length(CARD_TEXT_ROWS)])
        .split(inner);

    render_thumbnail(f, app, rows[0], card);
    render_card_text(f, app, rows[1], card, selected);
}

fn render_thumbnail(f: &mut Frame, app: &mut App, area: Rect, card: &AlbumCard) {
    if area.height == 0 || area.width == 0 {
        return;
    }

    // `schedule_album_cover` aliases the decoded cover under `card.uri` in
    // the shared image cache (it cannot write to `cover_url_by_uri` from
    // the spawned task), so we look it up directly here.
    let image_in_cache = app.images.contains(&card.uri);
    if !image_in_cache && !app.albums.cover_requested.contains(&card.uri) {
        crate::cmd::schedule_album_cover(app, card);
    }

    // Build the protocol once, pre-resizing the image to exactly fill the
    // cell area in pixels so `Resize::Crop`'s pure pixel-clip behaviour
    // doesn't leave blank stripes on small source covers.
    let proto_size_changed = app
        .albums
        .cover_protocol_sizes
        .get(&card.uri)
        .map(|sz| sz != &(area.width, area.height))
        .unwrap_or(true);
    if (proto_size_changed || !app.albums.cover_protocols.contains_key(&card.uri))
        && let Some(img) = app.images.get(&card.uri)
    {
        let fs = app.picker.font_size();
        let tw = (area.width as u32) * (fs.width as u32);
        let th = (area.height as u32) * (fs.height as u32);
        let resized = if tw > 0 && th > 0 {
            img.resize_to_fill(tw, th, image::imageops::FilterType::Lanczos3)
        } else {
            (*img).clone()
        };
        let proto = app.picker.new_resize_protocol(resized);
        app.albums.cover_protocols.insert(card.uri.clone(), proto);
        app.albums
            .cover_protocol_sizes
            .insert(card.uri.clone(), (area.width, area.height));
    }

    if let Some(proto) = app.albums.cover_protocols.get_mut(&card.uri) {
        let widget = StatefulImage::default().resize(Resize::Fit(None));
        f.render_stateful_widget(widget, area, proto);
    } else {
        // Hatched placeholder so the cell still reads as "cover here, soon".
        for row in 0..area.height {
            let mut s = String::with_capacity(area.width as usize);
            for col in 0..area.width {
                if (col + row) % 4 == 0 { s.push('╲'); } else { s.push(' '); }
            }
            let r = Rect { x: area.x, y: area.y + row, width: area.width, height: 1 };
            f.render_widget(
                Paragraph::new(Line::from(Span::styled(
                    s,
                    Style::default().fg(app.theme.progress_empty),
                ))),
                r,
            );
        }
    }
}

fn render_card_text(f: &mut Frame, app: &App, area: Rect, card: &AlbumCard, selected: bool) {
    let name_style = if selected {
        Style::default()
            .fg(app.theme.fg_strong)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(app.theme.fg)
    };
    let starred = crate::app::tidal_album_id(&card.uri)
        .map(|id| app.goodies.favorites.contains(id))
        .unwrap_or(false);
    let mut name_spans: Vec<Span> = Vec::new();
    if starred {
        name_spans.push(Span::styled(
            "★ ",
            Style::default().fg(app.theme.warn).add_modifier(Modifier::BOLD),
        ));
    }
    name_spans.push(Span::styled(card.name.clone(), name_style));

    let source_chip = match card.source {
        AlbumSource::Local => Span::styled(
            " LOCAL ",
            Style::default()
                .fg(app.theme.ok)
                .bg(app.theme.bg_chip)
                .add_modifier(Modifier::BOLD),
        ),
        AlbumSource::Tidal => Span::styled(
            " TIDAL ",
            Style::default()
                .fg(app.theme.accent_alt)
                .bg(app.theme.bg_chip)
                .add_modifier(Modifier::BOLD),
        ),
        AlbumSource::Other => Span::raw(""),
    };

    let lines = vec![
        Line::from(name_spans),
        Line::from(Span::styled(
            card.artist.clone(),
            Style::default().fg(app.theme.accent),
        )),
        Line::from(vec![
            source_chip,
            Span::raw("  "),
            Span::styled(
                card.year.clone().unwrap_or_default(),
                Style::default().fg(app.theme.fg_muted),
            ),
        ]),
    ];
    f.render_widget(
        Paragraph::new(lines).wrap(Wrap { trim: true }),
        area,
    );
}

// ─── detail ─────────────────────────────────────────────────────────────────

fn render_detail(f: &mut Frame, app: &mut App, area: Rect) {
    let Some(detail) = app.albums.detail.clone() else {
        render_grid(f, app, area);
        return;
    };

    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .title(Line::from(vec![
            Span::raw(" "),
            Span::styled(
                "Album",
                Style::default()
                    .fg(app.theme.accent)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                "   Esc / Backspace to return to grid ",
                Style::default().fg(app.theme.fg_muted),
            ),
        ]))
        .border_style(Style::default().fg(app.theme.border))
        .padding(Padding::uniform(1));
    let inner = block.inner(area);
    f.render_widget(block, area);

    let cols = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(40), Constraint::Percentage(60)])
        .split(inner);

    render_detail_left(f, app, cols[0], &detail);
    render_detail_right(f, app, cols[1], &detail);
}

fn render_detail_left(f: &mut Frame, app: &mut App, area: Rect, detail: &AlbumDetail) {
    // Big cover on top, action buttons below.
    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(8), Constraint::Length(7)])
        .split(area);

    // Constrain the cover to a visual square (width = height × cell aspect)
    // centred horizontally in the column so it doesn't stretch to fill the
    // whole 40% pane.
    let cover_area = square_cover_area(rows[0], app);
    render_thumbnail(f, app, cover_area, &detail.card);
    render_detail_actions(f, app, rows[1], detail);
}

fn square_cover_area(area: Rect, app: &App) -> Rect {
    if area.width == 0 || area.height == 0 {
        return area;
    }
    let fs = app.picker.font_size();
    // For a visual square: w_cells × fs.width ≈ h_cells × fs.height.
    let target_w =
        ((area.height as u32 * fs.height as u32) / fs.width.max(1) as u32).min(area.width as u32)
            as u16;
    let pad_x = area.width.saturating_sub(target_w) / 2;
    Rect {
        x: area.x + pad_x,
        y: area.y,
        width: target_w,
        height: area.height,
    }
}

fn render_detail_actions(f: &mut Frame, app: &App, area: Rect, detail: &AlbumDetail) {
    let starred = crate::app::tidal_album_id(&detail.card.uri)
        .map(|id| app.goodies.favorites.contains(id))
        .unwrap_or(false);
    let fav_label = if starred { "★ unfav" } else { "☆ fav" };

    let total_secs: i64 = detail
        .tracks
        .iter()
        .filter_map(|t| t.length.map(|ms| (ms / 1000) as i64))
        .sum();
    let mins = total_secs / 60;
    let secs = total_secs % 60;

    let lines = vec![
        Line::from(""),
        Line::from(vec![
            chip(app, " p ", "play all", app.theme.accent),
            Span::raw("   "),
            chip(app, " a ", "queue", app.theme.fg_strong),
            Span::raw("   "),
            chip(app, " f ", fav_label, app.theme.warn),
        ]),
        Line::from(""),
        Line::from(vec![
            Span::styled(
                format!("{} tracks", detail.tracks.len()),
                Style::default().fg(app.theme.fg),
            ),
            Span::styled("  ·  ", Style::default().fg(app.theme.fg_muted)),
            Span::styled(
                format!("{mins:02}:{secs:02}"),
                Style::default().fg(app.theme.fg_muted),
            ),
        ]),
    ];
    f.render_widget(Paragraph::new(lines), area);
}

fn chip(_app: &App, key: &str, label: &str, fg: ratatui::style::Color) -> Span<'static> {
    Span::styled(
        format!("[{key}] {label}"),
        Style::default().fg(fg).add_modifier(Modifier::BOLD),
    )
}

fn render_detail_right(f: &mut Frame, app: &mut App, area: Rect, detail: &AlbumDetail) {
    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(2), // title
            Constraint::Length(1), // artist
            Constraint::Length(1), // meta line (year · tracks · time)
            Constraint::Length(1), // genres
            Constraint::Min(6),    // wiki + credits
            Constraint::Min(0),    // tracks
        ])
        .split(area);

    let title_spans: Vec<Span> = vec![
        Span::styled(
            detail.card.name.clone(),
            Style::default()
                .fg(app.theme.fg_strong)
                .add_modifier(Modifier::BOLD),
        ),
    ];
    f.render_widget(
        Paragraph::new(Line::from(title_spans)).wrap(Wrap { trim: true }),
        rows[0],
    );

    f.render_widget(
        Paragraph::new(Span::styled(
            detail.card.artist.clone(),
            Style::default()
                .fg(app.theme.accent)
                .add_modifier(Modifier::BOLD),
        )),
        rows[1],
    );

    // Meta chips: source · year · tracks · time
    let total_secs: i64 = detail
        .tracks
        .iter()
        .filter_map(|t| t.length.map(|ms| (ms / 1000) as i64))
        .sum();
    let mins = total_secs / 60;
    let mut meta_spans = vec![match detail.card.source {
        AlbumSource::Local => Span::styled(
            " LOCAL ",
            Style::default()
                .fg(app.theme.ok)
                .bg(app.theme.bg_chip)
                .add_modifier(Modifier::BOLD),
        ),
        AlbumSource::Tidal => Span::styled(
            " TIDAL ",
            Style::default()
                .fg(app.theme.accent_alt)
                .bg(app.theme.bg_chip)
                .add_modifier(Modifier::BOLD),
        ),
        AlbumSource::Other => Span::raw(""),
    }];
    if let Some(y) = &detail.card.year {
        meta_spans.push(Span::raw("  "));
        meta_spans.push(Span::styled(
            y.clone(),
            Style::default().fg(app.theme.fg_muted),
        ));
    }
    meta_spans.push(Span::styled(
        format!("  ·  {} tracks  ·  {} min", detail.tracks.len(), mins),
        Style::default().fg(app.theme.fg_muted),
    ));
    f.render_widget(Paragraph::new(Line::from(meta_spans)), rows[2]);

    // Genres / MusicBrainz info (if present in current_album_meta).
    let mut genre_spans: Vec<Span> = Vec::new();
    if let Some(meta) = &app.current_album_meta
        && let Some(rel) = &meta.release
    {
        for g in rel.genres.iter().take(6) {
            genre_spans.push(Span::styled(
                format!(" {} ", g),
                Style::default()
                    .fg(app.theme.fg)
                    .bg(app.theme.bg_chip)
                    .add_modifier(Modifier::BOLD),
            ));
            genre_spans.push(Span::raw(" "));
        }
        if !rel.label.is_empty() {
            genre_spans.push(Span::raw("  "));
            genre_spans.push(Span::styled(
                rel.label.clone(),
                Style::default().fg(app.theme.fg_muted),
            ));
            if !rel.catalog_number.is_empty() {
                genre_spans.push(Span::styled(
                    format!("  {}", rel.catalog_number),
                    Style::default().fg(app.theme.fg_muted),
                ));
            }
        }
    }
    f.render_widget(Paragraph::new(Line::from(genre_spans)), rows[3]);

    // Wiki / credits block
    let mut wiki_lines: Vec<Line> = Vec::new();
    // Tidal credits of the selected track (goodies: tags or Tidal), when known.
    let track_credits = detail
        .tracks
        .get(detail.track_index)
        .zip(app.detail_credits.as_ref())
        .map(|(t, json)| crate::mopidy::client::track_credits(json, &t.uri, &t.name))
        .unwrap_or_default();
    const MAX_CREDIT_ROWS: usize = 6;
    let shown_credits = track_credits.len().min(MAX_CREDIT_ROWS) + usize::from(track_credits.len() > MAX_CREDIT_ROWS);
    // blank + heading + credit rows are reserved; the review gets the rest.
    let review_rows = (rows[4].height as usize)
        .saturating_sub(if track_credits.is_empty() { 0 } else { 2 + shown_credits } + 1)
        .max(2);
    if let Some(meta) = &app.current_album_meta {
        if let Some(t) = &meta.tidal {
            // Tidal's editorial review (with its source) beats the Wikipedia
            // article, which is often a loose title match.
            let heading = match t.source.as_deref().filter(|s| !s.is_empty()) {
                Some(src) => format!("Review · Tidal ({src})"),
                None => "Review · Tidal".to_string(),
            };
            wiki_lines.push(Line::from(Span::styled(
                heading,
                Style::default()
                    .fg(app.theme.accent_alt)
                    .add_modifier(Modifier::BOLD),
            )));
            let text = if track_credits.is_empty() {
                t.text.clone()
            } else {
                fit_text(&t.text, review_rows, rows[4].width as usize)
            };
            for line in text.lines().take(8) {
                wiki_lines.push(Line::from(Span::styled(
                    line.to_string(),
                    Style::default().fg(app.theme.fg),
                )));
            }
        } else if let Some(w) = &meta.wiki {
            wiki_lines.push(Line::from(Span::styled(
                w.title.clone(),
                Style::default()
                    .fg(app.theme.accent_alt)
                    .add_modifier(Modifier::BOLD),
            )));
            for line in w.extract.lines().take(4) {
                wiki_lines.push(Line::from(Span::styled(
                    line.to_string(),
                    Style::default().fg(app.theme.fg),
                )));
            }
        }
        if let Some(rel) = &meta.release
            && !rel.credits.is_empty()
            && track_credits.is_empty()
        {
            wiki_lines.push(Line::from(""));
            wiki_lines.push(Line::from(Span::styled(
                "Credits",
                Style::default()
                    .fg(app.theme.accent)
                    .add_modifier(Modifier::BOLD),
            )));
            for c in rel.credits.iter().take(5) {
                wiki_lines.push(Line::from(vec![
                    Span::styled(
                        format!("  {}", c.name),
                        Style::default().fg(app.theme.fg),
                    ),
                    Span::styled(
                        format!("  — {}", c.role),
                        Style::default().fg(app.theme.fg_muted),
                    ),
                ]));
            }
        }
    } else if app.meta_key.is_some() {
        wiki_lines.push(Line::from(Span::styled(
            "Loading MusicBrainz + Wikipedia…",
            Style::default().fg(app.theme.fg_muted),
        )));
    }
    if !track_credits.is_empty() {
        let track_name = detail
            .tracks
            .get(detail.track_index)
            .map(|t| t.name.clone())
            .unwrap_or_default();
        wiki_lines.push(Line::from(""));
        wiki_lines.push(Line::from(vec![
            Span::styled(
                "Credits",
                Style::default().fg(app.theme.accent).add_modifier(Modifier::BOLD),
            ),
            Span::styled(format!(" · {track_name}"), Style::default().fg(app.theme.fg_muted)),
        ]));
        for c in track_credits.iter().take(MAX_CREDIT_ROWS) {
            wiki_lines.push(Line::from(vec![
                Span::styled(format!("  {}", c.role), Style::default().fg(app.theme.fg_muted)),
                Span::styled(format!("  {}", c.names.join(", ")), Style::default().fg(app.theme.fg)),
            ]));
        }
        if track_credits.len() > MAX_CREDIT_ROWS {
            wiki_lines.push(Line::from(Span::styled("  …", Style::default().fg(app.theme.fg_muted))));
        }
    }
    f.render_widget(
        Paragraph::new(wiki_lines).wrap(Wrap { trim: false }),
        rows[4],
    );

    // Tracks
    let items: Vec<ListItem> = detail
        .tracks
        .iter()
        .enumerate()
        .map(|(i, t)| {
            let len = t.length.map(|ms| {
                let s = (ms / 1000) as i64;
                format!("{:02}:{:02}", s / 60, s % 60)
            }).unwrap_or_default();
            ListItem::new(Line::from(vec![
                Span::styled(
                    format!("{:>3}  ", i + 1),
                    Style::default().fg(app.theme.fg_muted),
                ),
                Span::styled(
                    t.name.clone(),
                    Style::default().fg(app.theme.fg_strong),
                ),
                Span::styled(
                    format!("        {len}"),
                    Style::default().fg(app.theme.fg_muted),
                ),
            ]))
        })
        .collect();

    let mut track_state = ratatui::widgets::ListState::default();
    track_state.select(Some(detail.track_index));
    let list = List::new(items)
        .block(
            Block::default()
                .borders(Borders::TOP)
                .border_type(BorderType::Rounded)
                .border_style(Style::default().fg(app.theme.border))
                .title(Line::from(Span::styled(
                    " Tracks ",
                    Style::default()
                        .fg(app.theme.fg_strong)
                        .add_modifier(Modifier::BOLD),
                ))),
        )
        .highlight_style(
            Style::default()
                .bg(app.theme.selection_bg)
                .add_modifier(Modifier::BOLD),
        )
        .highlight_symbol("▎ ");
    f.render_stateful_widget(list, rows[5], &mut track_state);
    // Silence unused-write warnings on Alignment when not used here.
    let _ = Alignment::Center;
}

#[cfg(test)]
mod tests {
    use super::{fit_text, grid_layout};

    // iTerm2 on a retina Mac: 14x30 px cells.
    const FW: u16 = 14;
    const FH: u16 = 30;

    #[test]
    fn shrinks_covers_to_fit_two_rows_in_a_typical_window() {
        // The panel from a ~128x45 window: 124 columns by 25 rows inside the block.
        let l = grid_layout(124, 25, FW, FH);
        assert!(l.rows >= 2, "{l:?}");
        assert!(l.cols >= 5, "{l:?}");
        assert!(l.cell_h * l.rows as u16 <= 25, "{l:?}");
    }

    #[test]
    fn keeps_big_covers_when_the_panel_is_tall() {
        let l = grid_layout(124, 60, FW, FH);
        assert!(l.rows >= 2 && l.cell_w >= 24, "{l:?}");
    }

    #[test]
    fn falls_back_to_one_big_row_when_even_small_covers_do_not_fit_twice() {
        let l = grid_layout(100, 12, FW, FH);
        assert_eq!(l.rows, 1);
        assert!(l.cell_w >= 24, "{l:?}");
    }

    #[test]
    fn never_panics_on_tiny_areas() {
        for (w, h) in [(0, 0), (1, 1), (10, 3), (200, 1)] {
            let _ = grid_layout(w, h, FW, FH);
        }
    }

    #[test]
    fn fit_text_keeps_short_text_and_cuts_long_text_at_a_word() {
        assert_eq!(fit_text("short review", 4, 80), "short review");
        let long = "word ".repeat(200);
        let out = fit_text(&long, 3, 40);
        assert!(out.ends_with('…') && out.chars().count() <= 3 * 40);
        assert!(!out.trim_end_matches('…').ends_with(' '));
        // Multi-byte characters must not be cut mid-codepoint.
        let uni = fit_text("héllo wörld ünïcode ñandú", 1, 12);
        assert!(uni.ends_with('…') && uni.chars().count() <= 12);
    }
}
