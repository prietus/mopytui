use ratatui::Frame;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Borders, Paragraph, Wrap};

use crate::app::App;

pub fn render(f: &mut Frame, app: &mut App, area: Rect) {
    let cols = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
        .split(area);
    render_album(f, app, cols[0]);
    render_artist(f, app, cols[1]);
}

fn render_album(f: &mut Frame, app: &mut App, area: Rect) {
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .title(" Album info ")
        .border_style(Style::default().fg(app.theme.border));
    let inner = block.inner(area);
    f.render_widget(block, area);

    if app.playback.current.is_none() {
        f.render_widget(server_info(app), inner);
        return;
    }
    let track = app.playback.current.clone().unwrap();

    // Reserve room at the top for the cover (only when one is loaded).
    let cover_key = app.cover_uri_for_current.clone();
    let has_cover = cover_key
        .as_deref()
        .map(|k| app.images.contains(k))
        .unwrap_or(false);
    let img_rows: u16 = if has_cover { 12 } else { 0 };
    let layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(img_rows), Constraint::Min(0)])
        .split(inner);
    if has_cover
        && let Some(key) = &cover_key
    {
        crate::images::render_info_image(f, app, layout[0], key);
    }
    let text_area = layout[1];

    let mut lines: Vec<Line> = Vec::new();
    lines.push(Line::from(Span::styled(
        track.album_name().to_string(),
        Style::default()
            .fg(app.theme.fg_strong)
            .add_modifier(Modifier::BOLD),
    )));
    lines.push(Line::from(Span::styled(
        track.artists_joined(),
        Style::default().fg(app.theme.accent),
    )));

    if let Some(meta) = &app.current_album_meta {
        lines.push(Line::from(""));
        if let Some(rel) = &meta.release {
            field(&mut lines, app, "Released", &rel.date);
            field(&mut lines, app, "Country", &rel.country);
            field(&mut lines, app, "Label", &rel.label);
            field(&mut lines, app, "Catalog", &rel.catalog_number);
            field(&mut lines, app, "Barcode", &rel.barcode);
            field(&mut lines, app, "Status", &rel.status);
            if !rel.genres.is_empty() {
                field(&mut lines, app, "Genres", &rel.genres.join(", "));
            }
            if !rel.credits.is_empty() {
                lines.push(Line::from(""));
                lines.push(Line::from(Span::styled(
                    "Credits",
                    Style::default().fg(app.theme.accent).add_modifier(Modifier::BOLD),
                )));
                let mut seen = 0;
                for c in &rel.credits {
                    if seen >= 20 { lines.push(Line::from(Span::styled("  …", Style::default().fg(app.theme.fg_muted)))); break; }
                    lines.push(Line::from(vec![
                        Span::styled(format!("  {}", c.name), Style::default().fg(app.theme.fg)),
                        Span::styled(format!("  — {}", c.role), Style::default().fg(app.theme.fg_muted)),
                    ]));
                    seen += 1;
                }
            }
        }
        if let Some(w) = &meta.wiki {
            lines.push(Line::from(""));
            lines.push(Line::from(Span::styled(
                w.title.clone(),
                Style::default().fg(app.theme.accent_alt).add_modifier(Modifier::BOLD),
            )));
            for line in w.extract.lines().take(8) {
                lines.push(Line::from(Span::styled(
                    line.to_string(),
                    Style::default().fg(app.theme.fg),
                )));
            }
        }
        if meta.release.is_none() && meta.wiki.is_none() {
            lines.push(Line::from(Span::styled(
                "No metadata available for this album.",
                Style::default().fg(app.theme.fg_muted),
            )));
        }
    } else if app.meta_key.is_some() {
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            "Looking up MusicBrainz + Wikipedia…",
            Style::default().fg(app.theme.fg_muted),
        )));
    }

    f.render_widget(
        Paragraph::new(lines).wrap(Wrap { trim: false }),
        text_area,
    );
}

fn render_artist(f: &mut Frame, app: &mut App, area: Rect) {
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .title(" Artist info ")
        .border_style(Style::default().fg(app.theme.border));
    let inner = block.inner(area);
    f.render_widget(block, area);

    if app.playback.current.is_none() {
        return;
    }
    let track = app.playback.current.clone().unwrap();

    // Reserve room at the top for the artist avatar (fanart.tv).
    let avatar_key = app.current_artist_avatar_key.clone();
    let has_avatar = avatar_key
        .as_deref()
        .map(|k| app.images.contains(k))
        .unwrap_or(false);
    let img_rows: u16 = if has_avatar { 12 } else { 0 };
    let layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(img_rows), Constraint::Min(0)])
        .split(inner);
    if has_avatar
        && let Some(key) = &avatar_key
    {
        crate::images::render_info_image(f, app, layout[0], key);
    }
    let text_area = layout[1];

    let mut lines: Vec<Line> = Vec::new();
    lines.push(Line::from(Span::styled(
        track.artists_joined(),
        Style::default()
            .fg(app.theme.fg_strong)
            .add_modifier(Modifier::BOLD),
    )));

    if let Some(meta) = &app.current_artist_meta {
        lines.push(Line::from(""));
        if let Some(info) = &meta.info {
            field(&mut lines, app, "Type", &info.kind);
            field(&mut lines, app, "Area", &info.area);
            field(&mut lines, app, "Began", &info.begin_date);
            field(&mut lines, app, "Ended", &info.end_date);
            if !info.members.is_empty() {
                lines.push(Line::from(""));
                lines.push(Line::from(Span::styled(
                    "Members",
                    Style::default().fg(app.theme.accent).add_modifier(Modifier::BOLD),
                )));
                let mut seen = 0;
                for m in &info.members {
                    if seen >= 12 { lines.push(Line::from(Span::styled("  …", Style::default().fg(app.theme.fg_muted)))); break; }
                    let role = if m.role.is_empty() { String::new() } else { format!(" — {}", m.role) };
                    let period = if m.period.is_empty() { String::new() } else { format!("  ({})", m.period) };
                    lines.push(Line::from(vec![
                        Span::styled(format!("  {}", m.name), Style::default().fg(app.theme.fg)),
                        Span::styled(role, Style::default().fg(app.theme.fg_muted)),
                        Span::styled(period, Style::default().fg(app.theme.fg_muted)),
                    ]));
                    seen += 1;
                }
            }
        }
        if let Some(w) = &meta.wiki {
            lines.push(Line::from(""));
            lines.push(Line::from(Span::styled(
                w.title.clone(),
                Style::default().fg(app.theme.accent_alt).add_modifier(Modifier::BOLD),
            )));
            for line in w.extract.lines().take(12) {
                lines.push(Line::from(Span::styled(
                    line.to_string(),
                    Style::default().fg(app.theme.fg),
                )));
            }
        }
        if meta.info.is_none() && meta.wiki.is_none() {
            lines.push(Line::from(Span::styled(
                "No metadata available for this artist.",
                Style::default().fg(app.theme.fg_muted),
            )));
        }
    } else if app.meta_key.is_some() {
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            "Looking up MusicBrainz + Wikipedia…",
            Style::default().fg(app.theme.fg_muted),
        )));
    }

    f.render_widget(
        Paragraph::new(lines).wrap(Wrap { trim: false }),
        text_area,
    );
}

fn field(lines: &mut Vec<Line>, app: &App, label: &str, value: &str) {
    if value.trim().is_empty() {
        return;
    }
    lines.push(Line::from(vec![
        Span::styled(
            format!("{label:<10}"),
            Style::default().fg(app.theme.fg_muted),
        ),
        Span::styled(value.to_string(), Style::default().fg(app.theme.fg)),
    ]));
}

fn server_info(app: &App) -> Paragraph<'_> {
    let cfg_path = crate::config::config_path()
        .map(|p| p.display().to_string())
        .unwrap_or_else(|| "(no config dir)".into());
    Paragraph::new(vec![
        Line::from(Span::styled(
            "Nothing playing",
            Style::default()
                .fg(app.theme.fg_muted)
                .add_modifier(Modifier::ITALIC),
        )),
        Line::from(""),
        Line::from(Span::styled(
            "Server",
            Style::default().fg(app.theme.accent).add_modifier(Modifier::BOLD),
        )),
        Line::from(format!("  http://{}:{}", app.cfg.host, app.cfg.http_port)),
        Line::from(format!("  mpd  {}:{}", app.cfg.host, app.cfg.mpd_port)),
        Line::from(format!(
            "  goodies: {}",
            if app.goodies.available { "available" } else { "not installed" }
        )),
        Line::from(""),
        Line::from(Span::styled(
            "Config file (TOML)",
            Style::default().fg(app.theme.accent).add_modifier(Modifier::BOLD),
        )),
        Line::from(format!("  {cfg_path}")),
        Line::from(""),
        Line::from(Span::styled(
            "Override at launch",
            Style::default().fg(app.theme.accent).add_modifier(Modifier::BOLD),
        )),
        Line::from("  mopytui --host 192.168.1.10 --port 6680"),
        Line::from("  mopytui 192.168.1.10:6680           (shorthand)"),
        Line::from("  mopytui --theme solar"),
    ])
}
