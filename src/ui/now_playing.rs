use ratatui::Frame;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Borders, Padding, Paragraph, Wrap};

use crate::app::{App, CoverFitMode};

pub fn render(f: &mut Frame, app: &mut App, area: Rect) {
    let outer = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(app.theme.border))
        .padding(Padding::symmetric(1, 0));
    let inner = outer.inner(area);
    f.render_widget(outer, area);

    // The lyrics column shows whenever the user has the toggle on. If the
    // fetch is still pending or returned nothing, we still draw a status
    // panel so the user gets visible feedback (otherwise toggling `L` looks
    // like a no-op).
    let show_lyrics = app.show_lyrics;

    match app.cover_fit_mode {
        CoverFitMode::Fit => layout_fit(f, app, inner, show_lyrics),
        CoverFitMode::Crop => layout_crop(f, app, inner, show_lyrics),
    }
}

/// Fit mode: the cover panel is sized to match the image aspect (square at
/// the available height). Lyrics get whatever's left to the right; without
/// lyrics, the cover sits on the left with the right side empty (or could
/// hold album info in future).
fn layout_fit(f: &mut Frame, app: &mut App, inner: Rect, show_lyrics: bool) {
    // Square cover: width-in-cells ≈ 2 × height-in-cells (cells are 2:1).
    let max_cover_w = (inner.height as u32).saturating_mul(2) as u16;
    let cover_w = max_cover_w.min(inner.width);

    if show_lyrics {
        // Cover left at its natural size, lyrics fills the rest.
        let cover_w = cover_w.min(inner.width.saturating_sub(28));
        let cols = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Length(cover_w), Constraint::Min(20)])
            .split(inner);
        crate::images::render_cover_widget(f, app, cols[0]);
        render_lyrics(f, app, cols[1]);
    } else {
        // Centre the cover horizontally.
        let pad = inner.width.saturating_sub(cover_w) / 2;
        let cols = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([
                Constraint::Length(pad),
                Constraint::Length(cover_w),
                Constraint::Min(0),
            ])
            .split(inner);
        crate::images::render_cover_widget(f, app, cols[1]);
    }
}

/// Crop mode: the cover fills its sub-area, clipping image edges as
/// needed. Without lyrics we still pin it to a centred square so the
/// artwork doesn't get stretched across the full body width.
fn layout_crop(f: &mut Frame, app: &mut App, inner: Rect, show_lyrics: bool) {
    let max_cover_w = (inner.height as u32).saturating_mul(2) as u16;
    let cover_w = max_cover_w.min(inner.width);

    if show_lyrics {
        let cover_w = cover_w.min((inner.width as u32 * 62 / 100) as u16);
        let cols = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Length(cover_w), Constraint::Min(20)])
            .split(inner);
        crate::images::render_cover_widget(f, app, cols[0]);
        render_lyrics(f, app, cols[1]);
    } else {
        let pad = inner.width.saturating_sub(cover_w) / 2;
        let cols = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([
                Constraint::Length(pad),
                Constraint::Length(cover_w),
                Constraint::Min(0),
            ])
            .split(inner);
        crate::images::render_cover_widget(f, app, cols[1]);
    }
}

fn render_lyrics(f: &mut Frame, app: &App, area: Rect) {
    // Check the cache to distinguish "still fetching" from "fetch done, none".
    let cache_state = app
        .lyrics_key
        .as_ref()
        .and_then(|k| app.lyrics_cache.get(k));
    let fetch_done = cache_state.is_some();
    let title_text = match app.lyrics.as_ref() {
        Some(l) if l.has_synced() => "♬ Lyrics",
        Some(l) if l.instrumental => "♬ Instrumental",
        Some(_) => "♬ Lyrics",
        None if app.playback.current.is_none() => "♬ Lyrics",
        None if fetch_done => "♬ Lyrics  ·  not found",
        None => "♬ Lyrics  ·  fetching…",
    };
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .title(Line::from(vec![
            Span::raw(" "),
            Span::styled(
                title_text,
                Style::default()
                    .fg(app.theme.accent_alt)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw(" "),
        ]))
        .border_style(Style::default().fg(app.theme.border))
        .padding(Padding::horizontal(1));
    let inner = block.inner(area);
    f.render_widget(block, area);

    let Some(lyrics) = app.lyrics.as_ref() else {
        let msg = if app.playback.current.is_none() {
            "no track playing"
        } else if fetch_done {
            "no lyrics found on lrclib.net"
        } else if app.lyrics_key.is_some() {
            "looking up lyrics on lrclib.net…"
        } else {
            "lyrics unavailable for this track"
        };
        f.render_widget(
            Paragraph::new(Span::styled(
                msg,
                Style::default().fg(app.theme.fg_muted),
            )),
            inner,
        );
        return;
    };

    if lyrics.instrumental && !lyrics.has_text() {
        f.render_widget(
            Paragraph::new(Span::styled(
                "♪ instrumental ♪",
                Style::default().fg(app.theme.fg_muted),
            )),
            inner,
        );
        return;
    }

    if !lyrics.has_text() {
        f.render_widget(
            Paragraph::new(Span::styled(
                "no lyrics found on lrclib.net",
                Style::default().fg(app.theme.fg_muted),
            )),
            inner,
        );
        return;
    }

    if !lyrics.has_synced() {
        let txt = lyrics.plain.clone().unwrap_or_default();
        f.render_widget(
            Paragraph::new(txt)
                .wrap(Wrap { trim: false })
                .style(Style::default().fg(app.theme.fg_muted)),
            inner,
        );
        return;
    }

    let elapsed = app.playback.elapsed_ms.max(0);
    let active = lyrics.current_line(elapsed).unwrap_or(0);
    let height = inner.height as usize;
    let half = height / 2;
    let start = active.saturating_sub(half);
    let end = (start + height).min(lyrics.synced.len());

    let mut lines: Vec<Line> = Vec::with_capacity(end - start);
    for i in start..end {
        let (_ts, text) = &lyrics.synced[i];
        let style = if i == active {
            Style::default()
                .fg(app.theme.fg_strong)
                .add_modifier(Modifier::BOLD)
        } else if i + 1 == active || i == active + 1 {
            Style::default().fg(app.theme.fg)
        } else {
            Style::default().fg(app.theme.fg_muted)
        };
        let prefix = if i == active { "▸ " } else { "  " };
        lines.push(Line::from(vec![
            Span::styled(prefix, Style::default().fg(app.theme.accent)),
            Span::styled(text.clone(), style),
        ]));
    }
    f.render_widget(
        Paragraph::new(lines).wrap(Wrap { trim: false }),
        inner,
    );
}
