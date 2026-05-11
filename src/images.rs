use std::collections::HashMap;
use std::sync::Arc;
use std::sync::Mutex;

use anyhow::{Context, Result};
use image::DynamicImage;
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::Span;
use ratatui::widgets::{Block, Borders, Paragraph};
use ratatui_image::{Resize, StatefulImage};
use ratatui_image::picker::Picker;

use crate::app::CoverFitMode;

use crate::app::App;
use crate::mopidy::Client;

/// Shared decoded-image cache. Background tasks write decoded images here;
/// the render loop reads them to materialise per-position `StatefulProtocol`s
/// on demand.
#[derive(Default)]
pub struct ImageCache {
    inner: Mutex<HashMap<String, Arc<DynamicImage>>>,
}

impl ImageCache {
    pub fn new() -> Self { Self::default() }

    pub fn get(&self, uri: &str) -> Option<Arc<DynamicImage>> {
        self.inner.lock().unwrap().get(uri).cloned()
    }

    pub fn put(&self, uri: String, img: Arc<DynamicImage>) {
        self.inner.lock().unwrap().insert(uri, img);
    }

    pub fn contains(&self, uri: &str) -> bool {
        self.inner.lock().unwrap().contains_key(uri)
    }
}

pub fn make_picker(override_protocol: Option<&str>) -> Picker {
    use ratatui_image::picker::ProtocolType;

    // CLI override wins outright.
    let cli_force = match override_protocol {
        Some("kitty") => Some(ProtocolType::Kitty),
        Some("iterm2") => Some(ProtocolType::Iterm2),
        Some("sixel") => Some(ProtocolType::Sixel),
        Some("halfblocks") => Some(ProtocolType::Halfblocks),
        _ => None,
    };

    // Auto-detect protocol via env hint first (more reliable than DA queries
    // — ratatui-image's `from_query_stdio` mis-detects Kitty inside iTerm2,
    // and iTerm2 then silently drops the Kitty escapes). The terminal app
    // sets $TERM_PROGRAM to a known identifier.
    let env_hint = std::env::var("TERM_PROGRAM").ok();
    let env_force = match env_hint.as_deref() {
        Some("iTerm.app") => Some(ProtocolType::Iterm2),
        Some("WezTerm") => Some(ProtocolType::Kitty),  // WezTerm prefers Kitty
        Some("ghostty") => Some(ProtocolType::Kitty),
        Some("Apple_Terminal") => Some(ProtocolType::Halfblocks),
        _ => None,
    };
    // Kitty's own terminal exports KITTY_WINDOW_ID.
    let env_force = env_force.or_else(|| {
        if std::env::var("KITTY_WINDOW_ID").is_ok() {
            Some(ProtocolType::Kitty)
        } else {
            None
        }
    });

    // Build picker. We still want the picker's font_size from the terminal
    // query when possible (so image scaling is correct), but we'll override
    // the protocol_type afterwards if the env tells us better.
    let mut picker = match (cli_force, override_protocol) {
        (_, Some("halfblocks")) => Picker::halfblocks(),
        _ => Picker::from_query_stdio().unwrap_or_else(|_| Picker::halfblocks()),
    };

    if let Some(pt) = cli_force.or(env_force) {
        picker.set_protocol_type(pt);
    }
    picker
}

pub async fn fetch_and_decode(
    client: &Client,
    cache: &ImageCache,
    image_uri: &str,
) -> Result<Arc<DynamicImage>> {
    if let Some(c) = cache.get(image_uri) { return Ok(c); }
    let url = client.image_url(image_uri);
    let bytes = client.fetch_bytes(&url).await.context("fetch cover bytes")?;
    let img = image::load_from_memory(&bytes).context("decode cover")?;
    let arc = Arc::new(img);
    cache.put(image_uri.to_string(), arc.clone());
    Ok(arc)
}

/// Render the cover for the currently-playing track into `area`. The
/// container is expected to already match the desired layout: this fn just
/// scales/crops the image into it. Falls back to a placeholder when no
/// cover is loaded yet.
pub fn render_cover_widget(f: &mut Frame, app: &mut App, area: Rect) {
    let cover_key = app.cover_uri_for_current.clone();
    let area_size = (area.width, area.height);

    // Rebuild the protocol when the cover changes OR — for Crop mode — when
    // the area resizes, because Crop pre-resamples the image to exactly fill
    // `area` in pixels (ratatui-image's own `Resize::Crop` only clips source
    // pixels, it does not upscale, so a small source cover leaves blank
    // space in a tall container).
    let need_rebuild = match (&app.cover_protocol_uri, &cover_key, app.cover_protocol_size) {
        (Some(a), Some(b), Some(sz)) => a != b || (app.cover_fit_mode == CoverFitMode::Crop && sz != area_size),
        (_, Some(_), None) => true,
        (None, Some(_), _) => true,
        _ => false,
    };

    if need_rebuild
        && let Some(uri) = &cover_key
        && let Some(img) = app.images.get(uri)
    {
        let img_for_protocol = match app.cover_fit_mode {
            CoverFitMode::Crop => {
                let fs = app.picker.font_size();
                let tw = (area.width as u32) * (fs.width as u32);
                let th = (area.height as u32) * (fs.height as u32);
                if tw > 0 && th > 0 {
                    img.resize_to_fill(tw, th, image::imageops::FilterType::Lanczos3)
                } else {
                    (*img).clone()
                }
            }
            CoverFitMode::Fit => (*img).clone(),
        };
        let proto = app.picker.new_resize_protocol(img_for_protocol);
        app.cover_protocol = Some(proto);
        app.cover_protocol_uri = Some(uri.clone());
        app.cover_protocol_size = Some(area_size);
    }

    let target = match app.cover_fit_mode {
        CoverFitMode::Crop => area,
        CoverFitMode::Fit => square_area(area),
    };
    let resize = match app.cover_fit_mode {
        // Image was already pre-sized to fill the area exactly — Fit
        // renders it 1:1 without scaling-down inside the protocol.
        CoverFitMode::Crop => Resize::Fit(None),
        CoverFitMode::Fit => Resize::Fit(None),
    };

    if let Some(proto) = app.cover_protocol.as_mut() {
        let widget = StatefulImage::default().resize(resize);
        f.render_stateful_widget(widget, target, proto);
    } else {
        render_placeholder(f, app, target);
    }
}

fn square_area(area: Rect) -> Rect {
    // Terminal cells are ~2:1 (taller than wide), so for an on-screen square
    // we want width-in-cells ≈ 2 × height-in-cells. Pick the largest such
    // rect that fits and centre it in `area`.
    let w_for_full_h = (area.height as u32).saturating_mul(2) as u16;
    let (target_w, target_h) = if w_for_full_h <= area.width {
        (w_for_full_h, area.height)
    } else {
        let h = (area.width / 2).min(area.height);
        ((h as u32 * 2) as u16, h)
    };
    if target_w == 0 || target_h == 0 { return area; }
    let pad_x = area.width.saturating_sub(target_w) / 2;
    let pad_y = area.height.saturating_sub(target_h) / 2;
    Rect {
        x: area.x + pad_x,
        y: area.y + pad_y,
        width: target_w,
        height: target_h,
    }
}

fn render_placeholder(f: &mut Frame, app: &App, area: Rect) {
    use ratatui::layout::Alignment;
    use ratatui::text::Line;
    use ratatui::widgets::BorderType;

    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(app.theme.accent_alt));
    let inner = block.inner(area);
    f.render_widget(block, area);
    if inner.height == 0 || inner.width < 4 {
        return;
    }

    // Diagonal hatching as a "no image here yet" backdrop. Makes it obvious
    // the placeholder is rendering even when text doesn't fit.
    let dim = Style::default().fg(app.theme.progress_empty);
    for row in 0..inner.height {
        let mut s = String::with_capacity(inner.width as usize);
        for col in 0..inner.width {
            if (col + row) % 4 == 0 { s.push('╲'); } else { s.push(' '); }
        }
        let r = Rect { x: inner.x, y: inner.y + row, width: inner.width, height: 1 };
        f.render_widget(Paragraph::new(Line::from(Span::styled(s, dim))), r);
    }

    let state_text = if app.playback.current.is_none() {
        "no track"
    } else if app.cover_uri_for_current.is_none() {
        "looking up cover…"
    } else if let Some(uri) = &app.cover_uri_for_current
        && !app.images.contains(uri)
    {
        "loading cover…"
    } else {
        "rendering…"
    };

    let mid_y = inner.y + inner.height / 2;
    let glyph_r = Rect { x: inner.x, y: mid_y.saturating_sub(1), width: inner.width, height: 1 };
    f.render_widget(
        Paragraph::new(Line::from(Span::styled(
            "♫",
            Style::default()
                .fg(app.theme.accent_alt)
                .add_modifier(Modifier::BOLD),
        )))
        .alignment(Alignment::Center),
        glyph_r,
    );
    let text_r = Rect { x: inner.x, y: mid_y + 1, width: inner.width, height: 1 };
    f.render_widget(
        Paragraph::new(Line::from(Span::styled(
            state_text,
            Style::default()
                .fg(app.theme.fg_strong)
                .add_modifier(Modifier::BOLD),
        )))
        .alignment(Alignment::Center),
        text_r,
    );
}
