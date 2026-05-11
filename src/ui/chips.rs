//! Reusable label "chips" — small coloured pills used in lists to convey
//! categorical info at a glance (source backend, etc).

use ratatui::style::{Modifier, Style};
use ratatui::text::Span;

use crate::ui::theme::Theme;

/// Decode the URI scheme to identify the backend that produced this entry.
/// Returns `None` for empty / unknown URIs.
pub fn source_chip(uri: &str, theme: &Theme) -> Span<'static> {
    let (label, fg) = match scheme(uri) {
        Some("tidal") => ("TIDAL", theme.accent_alt),
        Some("local") | Some("file") | Some("m3u") | Some("podcast") => ("LOCAL", theme.ok),
        Some("spotify") => ("SPTFY", theme.warn),
        Some("youtube") | Some("yt") => ("YT", theme.err),
        Some("soundcloud") | Some("sc") => ("SC", theme.warn),
        Some("bandcamp") | Some("bc") => ("BC", theme.ok),
        Some(s) => return Span::styled(
            format!(" {} ", s.to_uppercase()),
            Style::default()
                .fg(theme.fg_muted)
                .bg(theme.bg_chip)
                .add_modifier(Modifier::BOLD),
        ),
        None => return Span::raw(""),
    };
    Span::styled(
        format!(" {label} "),
        Style::default()
            .fg(fg)
            .bg(theme.bg_chip)
            .add_modifier(Modifier::BOLD),
    )
}

fn scheme(uri: &str) -> Option<&str> {
    if uri.is_empty() { return None; }
    uri.split_once(':').map(|(s, _)| s)
}
