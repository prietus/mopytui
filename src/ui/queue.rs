use ratatui::Frame;
use ratatui::layout::{Constraint, Direction, Layout, Margin, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Borders, Cell, Padding, Paragraph, Row, Table, Wrap};
use unicode_width::UnicodeWidthStr;

use crate::app::App;

/// Truncate `s` so it fits in `max_cells` terminal columns, ending with `…`
/// when content is dropped. ratatui's default cell hard-cuts at the byte
/// boundary (no marker), which produces ugly "Move on Up (Single E" tails —
/// this gives us a clean `… ` instead.
fn ellipsize(s: &str, max_cells: u16) -> String {
    let max = max_cells as usize;
    if max == 0 { return String::new(); }
    if s.width() <= max { return s.to_string(); }
    if max == 1 { return "…".to_string(); }
    let budget = max - 1;
    let mut out = String::with_capacity(s.len() + 3);
    let mut w = 0usize;
    for ch in s.chars() {
        let cw = unicode_width::UnicodeWidthChar::width(ch).unwrap_or(0);
        if w + cw > budget { break; }
        out.push(ch);
        w += cw;
    }
    out.push('…');
    out
}

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
    }

    let queue_col = if has_current { cols[1] } else { cols[0] };

    // Spectrum panel below the queue table whenever a track is loaded and
    // there's vertical room (10 rows = bordered frame + 8 rows of bars).
    if app.playback.current.is_some() && queue_col.height >= 22 {
        let rows = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Min(0), Constraint::Length(10)])
            .split(queue_col);
        render_queue_table(f, app, rows[0]);
        crate::ui::spectrum::render(f, app, rows[1]);
    } else {
        render_queue_table(f, app, queue_col);
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

    // The chain box (DAC + format + verdict) shows up when the goodies
    // plugin has answered with at least a DAC label. Without the plugin,
    // the chain info collapses back into the inline meta line under the
    // cover.
    let has_chain_box = app.dac_label.is_some();
    let chain_h: u16 = if has_chain_box { 5 } else { 0 };
    // Pack the bottom block to exactly the lines we'll draw so the cover
    // (square, capped at width/2 in height) gets every spare cell.
    let meta_h = meta_height(app, has_chain_box);
    let bottom_total = meta_h + chain_h;
    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(8), Constraint::Length(bottom_total)])
        .split(inner);

    crate::images::render_cover_widget(f, app, rows[0]);

    if has_chain_box {
        let meta_rows = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Length(meta_h), Constraint::Length(chain_h)])
            .split(rows[1]);
        render_meta_under_cover(f, app, meta_rows[0]);
        render_chain_box(f, app, meta_rows[1]);
    } else {
        render_meta_under_cover(f, app, rows[1]);
    }
}

fn meta_height(app: &App, has_chain_box: bool) -> u16 {
    let Some(t) = &app.playback.current else { return 2; };
    let mut n: u16 = 0;
    if !t.artists_joined().is_empty() { n += 1; }
    n += 1; // album + year line is always pushed
    if t.genre.as_deref().filter(|s| !s.is_empty()).is_some() { n += 1; }
    if !has_chain_box {
        let fmt = fmt_from_uri(&t.uri);
        let bitrate = t.bitrate.or(app.bitrate).filter(|b| *b > 0);
        let audio = app.audio.as_ref().filter(|a| a.rate > 0);
        if fmt.is_some() || bitrate.is_some() || audio.is_some() { n += 1; }
    }
    if goodies_play_count(app, &t.uri).is_some() {
        n += 2; // blank line + "played N×"
    }
    // +1 to absorb the occasional wrap on a long artist/album.
    n.saturating_add(1).max(3)
}

fn render_chain_box(f: &mut Frame, app: &App, area: Rect) {
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(app.theme.accent))
        .padding(Padding::horizontal(1));
    let inner = block.inner(area);
    f.render_widget(block, area);

    let Some(t) = &app.playback.current else { return; };
    let mut lines: Vec<Line> = Vec::new();

    // Line 1 — DAC name in bold, the headline of the box.
    if let Some(dac) = &app.dac_label {
        lines.push(Line::from(Span::styled(
            dac.clone(),
            Style::default()
                .fg(app.theme.fg_strong)
                .add_modifier(Modifier::BOLD),
        )));
    }

    // Line 2 — format · kHz · bit-depth. Format is inferred from the URI
    // (FLAC, MP3, Tidal, …); rate and bits come from `/audio/active`.
    let fmt = fmt_from_uri(&t.uri);
    let audio = app.audio.as_ref().filter(|a| a.rate > 0);
    let muted = Style::default().fg(app.theme.fg_muted);
    let sep = Span::styled("  ·  ", muted);
    let mut chain: Vec<Span> = Vec::new();
    if let Some(f) = fmt {
        chain.push(Span::styled(
            f,
            Style::default()
                .fg(app.theme.accent_alt)
                .add_modifier(Modifier::BOLD),
        ));
    }
    if let Some(a) = audio {
        let khz = a.rate as f32 / 1000.0;
        let khz_s = if (khz - khz.round()).abs() < 0.05 {
            format!("{:.0} kHz", khz)
        } else {
            format!("{:.1} kHz", khz)
        };
        if !chain.is_empty() { chain.push(sep.clone()); }
        chain.push(Span::styled(khz_s, Style::default().fg(app.theme.fg)));
        if a.bits > 0 {
            chain.push(sep.clone());
            chain.push(Span::styled(
                format!("{}-bit", a.bits),
                Style::default().fg(app.theme.fg),
            ));
        }
    }
    if !chain.is_empty() {
        lines.push(Line::from(chain));
    }

    // Line 3 — verdict pill. ✓ + label when bit-perfect, ⚠ otherwise so a
    // degraded chain reads as a soft warning. The glyph carries semantic
    // weight on its own (more readable than the generic ●).
    if let Some(v) = &app.audio_verdict {
        let bit_perfect = v == "bit-perfect";
        let (glyph, color) = if bit_perfect {
            ("✓ ", app.theme.ok)
        } else {
            ("⚠ ", app.theme.warn)
        };
        lines.push(Line::from(vec![
            Span::styled(glyph, Style::default().fg(color).add_modifier(Modifier::BOLD)),
            Span::styled(
                v.clone(),
                Style::default().fg(color).add_modifier(Modifier::BOLD),
            ),
        ]));
    }

    f.render_widget(Paragraph::new(lines), inner);
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

    // Audio chain (codec · bitrate · samplerate/bitdepth). When the
    // `tidal_goodies` plugin is present we render this inside the dedicated
    // chain box below — this inline copy is the fallback for setups without
    // the plugin so users without it still see at least the codec.
    let fmt = fmt_from_uri(&t.uri);
    let bitrate = t.bitrate.or(app.bitrate).filter(|b| *b > 0);
    let audio = app.audio.as_ref().filter(|a| a.rate > 0);
    let has_chain_box = app.dac_label.is_some();
    if !has_chain_box && (fmt.is_some() || bitrate.is_some() || audio.is_some()) {
        let muted = Style::default().fg(app.theme.fg_muted);
        let sep = Span::styled("  ·  ", muted);
        let mut spans: Vec<Span> = Vec::new();
        if let Some(f) = fmt {
            spans.push(Span::styled(
                f,
                Style::default().fg(app.theme.accent_alt).add_modifier(Modifier::BOLD),
            ));
        }
        if let Some(b) = bitrate {
            if !spans.is_empty() { spans.push(sep.clone()); }
            spans.push(Span::styled(format!("{b} kbps"), muted));
        }
        if let Some(a) = audio {
            let khz = a.rate as f32 / 1000.0;
            let khz_s = if (khz - khz.round()).abs() < 0.05 {
                format!("{:.0} kHz", khz)
            } else {
                format!("{:.1} kHz", khz)
            };
            if !spans.is_empty() { spans.push(sep.clone()); }
            spans.push(Span::styled(khz_s, muted));
            if a.bits > 0 {
                spans.push(sep.clone());
                spans.push(Span::styled(format!("{}-bit", a.bits), muted));
            }
        }
        lines.push(Line::from(spans));
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

/// Best-effort codec label from the track URI. Mopidy doesn't expose the
/// actual GStreamer caps via MPD or JSON-RPC, so we infer from the file
/// extension for local tracks and from the URI scheme for streams.
///
/// The leading glyph is universal Unicode (no Nerd Font required): `∿`
/// (U+223F sine wave) for Tidal — visual nod to Tidal's wave logo — `⇄`
/// for network streams, and a musical-note family for local file codecs.
fn fmt_from_uri(uri: &str) -> Option<&'static str> {
    if uri.starts_with("tidal:") {
        return Some("∿ Tidal");
    }
    if uri.starts_with("http://") || uri.starts_with("https://") {
        return Some("⇄ Stream");
    }
    let ext = uri.rsplit('.').next()?.to_ascii_lowercase();
    match ext.as_str() {
        "flac" => Some("♬ FLAC"),
        "mp3"  => Some("♪ MP3"),
        "m4a" | "mp4" => Some("♪ M4A"),
        "alac" => Some("♬ ALAC"),
        "aac"  => Some("♪ AAC"),
        "ogg" | "oga" => Some("♪ OGG"),
        "opus" => Some("♪ OPUS"),
        "wav"  => Some("♬ WAV"),
        "aif" | "aiff" => Some("♬ AIFF"),
        "dsf" | "dff" => Some("♬ DSD"),
        "wv"   => Some("♬ WV"),
        "ape"  => Some("♬ APE"),
        _ => None,
    }
}

fn goodies_play_count(app: &App, uri: &str) -> Option<u32> {
    app.goodies
        .most
        .iter()
        .find(|i| i.uri == uri)
        .and_then(|i| i.count)
}

fn render_queue_table(f: &mut Frame, app: &mut App, area: Rect) {
    let title = format!(" Queue · {} ", app.queue.len());

    // Pre-compute per-column cell budgets so we can ellipsize text instead
    // of letting ratatui hard-cut at the cell boundary. The split mirrors
    // the constraint list below; 1 cell of safety on each dynamic column
    // covers ratatui's internal padding.
    const HASH_W: u16 = 7;
    const LEN_W: u16 = 6;
    let inner_w = area.width.saturating_sub(2); // rounded borders
    let dyn_w = inner_w.saturating_sub(HASH_W + LEN_W + 2); // 2 = column separators
    let artist_w = (dyn_w * 22 / 100).saturating_sub(1);
    let title_w = (dyn_w * 33 / 100).saturating_sub(1);
    let album_w = dyn_w.saturating_sub(artist_w + title_w + 2);

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
                Cell::from(ellipsize(&tl.track.artists_joined(), artist_w)),
                Cell::from(ellipsize(&tl.track.name, title_w)),
                Cell::from(ellipsize(tl.track.album_name(), album_w)),
                Cell::from(tl.track.length.map(|ms| fmt_ms(ms as i64)).unwrap_or_default()),
            ])
            .style(style)
        })
        .collect();

    let widths = [
        Constraint::Length(HASH_W),
        Constraint::Percentage(22),
        Constraint::Percentage(33),
        Constraint::Min(0),
        Constraint::Length(LEN_W),
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
