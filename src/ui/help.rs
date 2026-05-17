use ratatui::Frame;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Borders, Paragraph};

use crate::app::App;

const ROWS: &[(&str, &str)] = &[
    ("Global", ""),
    ("q  Q  Esc", "quit"),
    ("?", "toggle help"),
    ("1..8", "Queue · Albums · Library · Playlists · Search · Now Playing · Stats · Info"),
    ("Tab", "next view"),
    ("", ""),
    ("Library", ""),
    ("↑/↓  j/k", "move selection"),
    ("Enter", "open / play"),
    ("Backspace  h", "go up"),
    ("a", "add selection to queue"),
    ("A", "add selection to queue & play"),
    ("r", "refresh library (`core.library.refresh`)"),
    ("", ""),
    ("Albums", ""),
    ("h/j/k/l  arrows", "navigate grid"),
    ("Enter", "open album detail"),
    ("p", "play this album"),
    ("a", "add album to queue"),
    ("f", "toggle Tidal favorite"),
    ("Esc / Backspace", "back to grid (from detail)"),
    ("r", "reload album collection"),
    ("", ""),
    ("Playback", ""),
    ("Space", "play/pause"),
    ("s", "stop"),
    (">", "next track"),
    ("<", "previous track"),
    ("[  ]", "seek -/+ 10s"),
    ("←/→", "seek -/+ 5s"),
    ("-/+  =", "volume -/+ 5"),
    ("m", "mute"),
    ("R", "toggle random"),
    ("T", "toggle repeat"),
    ("S", "toggle single"),
    ("C", "toggle consume"),
    ("L", "toggle synced lyrics panel"),
    ("c", "toggle cover fit ↔ crop"),
    ("v", "cycle visualizer style (bars · mirror · dots · wave)"),
    ("f", "toggle Tidal favorite (album / current track)"),
    ("", ""),
    ("Queue", ""),
    ("Enter", "play this entry"),
    ("d  Del", "remove from queue"),
    ("J / K", "move down / up"),
    ("X", "clear queue"),
    ("Z", "shuffle queue"),
    ("", ""),
    ("Search", ""),
    ("/", "open search (focus first field)"),
    ("↑/↓ Tab", "navigate fields · sources · buttons · results"),
    ("type", "edit focused field"),
    ("Space", "toggle Local/Tidal checkbox"),
    ("Enter", "run search (form) · play/open (result)"),
    ("Esc", "jump to results (or Search button if empty)"),
    ("p / a / f", "play · add · favorite (on result)"),
];

pub fn render(f: &mut Frame, app: &App, area: Rect) {
    let v = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(0)])
        .split(area);
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .title(" Help ")
        .border_style(Style::default().fg(app.theme.border));
    let inner = block.inner(v[0]);
    f.render_widget(block, v[0]);

    let mut lines: Vec<Line> = Vec::with_capacity(ROWS.len());
    for (k, d) in ROWS {
        if d.is_empty() {
            lines.push(Line::from(Span::styled(
                *k,
                Style::default()
                    .fg(app.theme.accent)
                    .add_modifier(Modifier::BOLD),
            )));
        } else {
            lines.push(Line::from(vec![
                Span::styled(format!("  {k:<14}"), Style::default().fg(app.theme.fg_strong)),
                Span::styled(*d, Style::default().fg(app.theme.fg_muted)),
            ]));
        }
    }
    f.render_widget(Paragraph::new(lines), inner);
}
