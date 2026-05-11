use ratatui::style::Color;

#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct Theme {
    pub name: &'static str,
    pub bg: Color,
    pub fg: Color,
    pub fg_muted: Color,
    pub fg_strong: Color,
    pub border: Color,
    pub accent: Color,
    pub accent_alt: Color,
    pub ok: Color,
    pub warn: Color,
    pub err: Color,
    pub selection_bg: Color,
    pub selection_fg: Color,
    pub progress_filled: Color,
    pub progress_empty: Color,
    pub bg_chip: Color,
}

impl Theme {
    pub fn from_name(name: &str) -> Self {
        match name {
            "soft-dark" => SOFT_DARK,
            "daylight" => DAYLIGHT,
            "solar" => SOLAR,
            _ => MIDNIGHT,
        }
    }
}

// Analogous palette anchored on a soft periwinkle blue. `ok` shares the
// accent hue (no candy green) and `accent_alt` is a blue-leaning violet so
// LOCAL/TIDAL/Lyrics chips sit on the same axis. Amber is reserved for
// star/favourite and warnings — the only warm note.
pub const MIDNIGHT: Theme = Theme {
    name: "midnight",
    bg: Color::Reset,
    fg: Color::Rgb(214, 222, 235),
    fg_muted: Color::Rgb(120, 132, 158),
    fg_strong: Color::Rgb(245, 248, 255),
    border: Color::Rgb(52, 62, 88),
    accent: Color::Rgb(137, 180, 250),       // periwinkle blue
    accent_alt: Color::Rgb(180, 165, 246),   // blue-leaning violet
    ok: Color::Rgb(137, 180, 250),           // same as accent
    warn: Color::Rgb(224, 200, 120),         // soft amber
    err: Color::Rgb(220, 130, 130),          // muted coral
    selection_bg: Color::Rgb(52, 62, 88),
    selection_fg: Color::Rgb(245, 248, 255),
    progress_filled: Color::Rgb(137, 180, 250),
    progress_empty: Color::Rgb(38, 46, 64),
    bg_chip: Color::Rgb(28, 34, 52),
};

pub const SOFT_DARK: Theme = Theme {
    name: "soft-dark",
    bg: Color::Reset,
    fg: Color::Rgb(200, 200, 200),
    fg_muted: Color::Rgb(130, 130, 130),
    fg_strong: Color::Rgb(255, 255, 255),
    border: Color::Rgb(80, 80, 80),
    accent: Color::Rgb(180, 200, 220),
    accent_alt: Color::Rgb(220, 180, 200),
    ok: Color::Rgb(150, 200, 150),
    warn: Color::Rgb(230, 200, 120),
    err: Color::Rgb(220, 130, 130),
    selection_bg: Color::Rgb(60, 60, 60),
    selection_fg: Color::Rgb(255, 255, 255),
    progress_filled: Color::Rgb(200, 200, 200),
    progress_empty: Color::Rgb(50, 50, 50),
    bg_chip: Color::Rgb(30, 30, 30),
};

pub const DAYLIGHT: Theme = Theme {
    name: "daylight",
    bg: Color::Reset,
    fg: Color::Rgb(50, 60, 80),
    fg_muted: Color::Rgb(120, 130, 150),
    fg_strong: Color::Rgb(20, 30, 50),
    border: Color::Rgb(180, 190, 210),
    accent: Color::Rgb(40, 90, 200),
    accent_alt: Color::Rgb(170, 60, 150),
    ok: Color::Rgb(40, 150, 70),
    warn: Color::Rgb(190, 130, 30),
    err: Color::Rgb(190, 50, 60),
    selection_bg: Color::Rgb(220, 230, 245),
    selection_fg: Color::Rgb(20, 30, 50),
    progress_filled: Color::Rgb(40, 90, 200),
    progress_empty: Color::Rgb(220, 225, 235),
    bg_chip: Color::Rgb(245, 247, 252),
};

pub const SOLAR: Theme = Theme {
    name: "solar",
    bg: Color::Reset,
    fg: Color::Rgb(238, 232, 213),
    fg_muted: Color::Rgb(147, 161, 161),
    fg_strong: Color::Rgb(253, 246, 227),
    border: Color::Rgb(88, 110, 117),
    accent: Color::Rgb(38, 139, 210),
    accent_alt: Color::Rgb(211, 54, 130),
    ok: Color::Rgb(133, 153, 0),
    warn: Color::Rgb(181, 137, 0),
    err: Color::Rgb(220, 50, 47),
    selection_bg: Color::Rgb(7, 54, 66),
    selection_fg: Color::Rgb(253, 246, 227),
    progress_filled: Color::Rgb(38, 139, 210),
    progress_empty: Color::Rgb(7, 54, 66),
    bg_chip: Color::Rgb(0, 43, 54),
};
