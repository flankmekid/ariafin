use ratatui::style::{Color, Modifier, Style};

pub struct Theme;

impl Theme {
    // Accent color used for highlights, borders, and active elements.
    pub const ACCENT: Color = Color::Rgb(99, 179, 237);   // sky blue
    pub const ACCENT_DIM: Color = Color::Rgb(49, 130, 206);

    // Text hierarchy
    pub const TEXT_PRIMARY: Color = Color::Rgb(237, 242, 247);
    pub const TEXT_SECONDARY: Color = Color::Rgb(160, 174, 192);
    pub const TEXT_MUTED: Color = Color::Rgb(112, 128, 150);

    // Backgrounds
    pub const BG: Color = Color::Rgb(13, 17, 23);
    pub const BG_ELEVATED: Color = Color::Rgb(22, 27, 34);
    pub const BG_SELECTED: Color = Color::Rgb(30, 41, 59);

    // Status
    pub const SUCCESS: Color = Color::Rgb(72, 187, 120);
    pub const WARNING: Color = Color::Rgb(237, 137, 54);
    pub const ERROR: Color = Color::Rgb(252, 129, 129);

    pub fn normal() -> Style {
        Style::default().fg(Self::TEXT_PRIMARY)
    }

    pub fn secondary() -> Style {
        Style::default().fg(Self::TEXT_SECONDARY)
    }

    pub fn muted() -> Style {
        Style::default().fg(Self::TEXT_MUTED)
    }

    pub fn accent() -> Style {
        Style::default().fg(Self::ACCENT)
    }

    pub fn accent_bold() -> Style {
        Style::default()
            .fg(Self::ACCENT)
            .add_modifier(Modifier::BOLD)
    }

    pub fn selected() -> Style {
        Style::default()
            .fg(Self::TEXT_PRIMARY)
            .bg(Self::BG_SELECTED)
            .add_modifier(Modifier::BOLD)
    }

    pub fn tab_active() -> Style {
        Style::default()
            .fg(Self::ACCENT)
            .add_modifier(Modifier::BOLD)
    }

    pub fn tab_inactive() -> Style {
        Style::default().fg(Self::TEXT_MUTED)
    }

    pub fn border() -> Style {
        Style::default().fg(Color::Rgb(55, 65, 85))
    }

    pub fn border_focused() -> Style {
        Style::default().fg(Self::ACCENT_DIM)
    }
}
