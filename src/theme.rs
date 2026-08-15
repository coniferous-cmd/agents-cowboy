use ratatui::style::Color;

use crate::claude_env::Theme;

/// Render-facing theme palette that translates stored theme slot values
/// into `ratatui::style::Color` values for the TUI rendering layer.
#[derive(Debug, Clone, PartialEq)]
pub struct ThemePalette {
    pub active_pane_border: Color,
    pub inactive_pane_border: Color,
    pub project_highlight: Color,
    pub session_highlight: Color,
    pub status_badge_bg: Color,
    pub status_badge_fg: Color,
    pub hint_key_fg: Color,
    pub hint_text_fg: Color,
    pub meta_text_fg: Color,
    pub modal_border: Color,
}

impl Default for ThemePalette {
    /// Dracula fallback defaults, matching the seeded default theme.
    fn default() -> Self {
        Self {
            active_pane_border: Color::Cyan,
            inactive_pane_border: Color::DarkGray,
            project_highlight: Color::Yellow,
            session_highlight: Color::Magenta,
            status_badge_bg: Color::LightMagenta,
            status_badge_fg: Color::Black,
            hint_key_fg: Color::White,
            hint_text_fg: Color::Gray,
            meta_text_fg: Color::DarkGray,
            modal_border: Color::Cyan,
        }
    }
}

impl From<&Theme> for ThemePalette {
    fn from(theme: &Theme) -> Self {
        Self {
            active_pane_border: parse_color(&theme.active_pane_border),
            inactive_pane_border: parse_color(&theme.inactive_pane_border),
            project_highlight: parse_color(&theme.project_highlight),
            session_highlight: parse_color(&theme.session_highlight),
            status_badge_bg: parse_color(&theme.status_badge_bg),
            status_badge_fg: parse_color(&theme.status_badge_fg),
            hint_key_fg: parse_color(&theme.hint_key_fg),
            hint_text_fg: parse_color(&theme.hint_text_fg),
            meta_text_fg: parse_color(&theme.meta_text_fg),
            modal_border: parse_color(&theme.modal_border),
        }
    }
}

/// Parse a color name string into a `ratatui::style::Color`.
///
/// Supports ratatui named colors (case-insensitive) and hex `#RRGGBB` format.
/// Falls back to `Color::Reset` for unrecognised values.
fn parse_color(s: &str) -> Color {
    match s.trim().to_lowercase().as_str() {
        "black" => Color::Black,
        "red" => Color::Red,
        "green" => Color::Green,
        "yellow" => Color::Yellow,
        "blue" => Color::Blue,
        "magenta" => Color::Magenta,
        "cyan" => Color::Cyan,
        "white" => Color::White,
        "gray" => Color::Gray,
        "darkgray" => Color::DarkGray,
        "lightmagenta" => Color::LightMagenta,
        "lightcyan" => Color::LightCyan,
        "lightyellow" => Color::LightYellow,
        "lightgreen" => Color::LightGreen,
        "lightred" => Color::LightRed,
        "lightblue" => Color::LightBlue,
        "reset" => Color::Reset,
        _ => {
            if let Some(hex) = s.trim().strip_prefix('#') {
                if let Ok(rgb) = u32::from_str_radix(hex, 16) {
                    if hex.len() == 6 {
                        let r = ((rgb >> 16) & 0xFF) as u8;
                        let g = ((rgb >> 8) & 0xFF) as u8;
                        let b = (rgb & 0xFF) as u8;
                        return Color::Rgb(r, g, b);
                    }
                }
            }
            Color::Reset
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_palette_is_dracula() {
        let palette = ThemePalette::default();
        assert_eq!(palette.active_pane_border, Color::Cyan);
        assert_eq!(palette.inactive_pane_border, Color::DarkGray);
        assert_eq!(palette.project_highlight, Color::Yellow);
        assert_eq!(palette.session_highlight, Color::Magenta);
        assert_eq!(palette.status_badge_bg, Color::LightMagenta);
        assert_eq!(palette.status_badge_fg, Color::Black);
        assert_eq!(palette.hint_key_fg, Color::White);
        assert_eq!(palette.hint_text_fg, Color::Gray);
        assert_eq!(palette.meta_text_fg, Color::DarkGray);
        assert_eq!(palette.modal_border, Color::Cyan);
    }

    #[test]
    fn from_theme_converts_stored_strings_to_colors() {
        let theme = Theme {
            name: "test".to_string(),
            is_active: true,
            active_pane_border: "Red".to_string(),
            inactive_pane_border: "DarkGray".to_string(),
            project_highlight: "#00FF00".to_string(),
            session_highlight: "magenta".to_string(),
            status_badge_bg: "LightMagenta".to_string(),
            status_badge_fg: "White".to_string(),
            hint_key_fg: "CYAN".to_string(),
            hint_text_fg: "Gray".to_string(),
            meta_text_fg: "DarkGray".to_string(),
            modal_border: "Blue".to_string(),
        };
        let palette = ThemePalette::from(&theme);
        assert_eq!(palette.active_pane_border, Color::Red);
        assert_eq!(palette.inactive_pane_border, Color::DarkGray);
        assert_eq!(palette.project_highlight, Color::Rgb(0, 255, 0));
        assert_eq!(palette.session_highlight, Color::Magenta);
        assert_eq!(palette.status_badge_bg, Color::LightMagenta);
        assert_eq!(palette.status_badge_fg, Color::White);
        assert_eq!(palette.hint_key_fg, Color::Cyan);
        assert_eq!(palette.hint_text_fg, Color::Gray);
        assert_eq!(palette.meta_text_fg, Color::DarkGray);
        assert_eq!(palette.modal_border, Color::Blue);
    }

    #[test]
    fn parse_color_handles_case_insensitivity() {
        assert_eq!(parse_color("CYAN"), Color::Cyan);
        assert_eq!(parse_color("cyan"), Color::Cyan);
        assert_eq!(parse_color("DarkGray"), Color::DarkGray);
        assert_eq!(parse_color("LIGHTMAGENTA"), Color::LightMagenta);
    }

    #[test]
    fn parse_color_handles_hex() {
        assert_eq!(parse_color("#FF0000"), Color::Rgb(255, 0, 0));
        assert_eq!(parse_color("#00FF00"), Color::Rgb(0, 255, 0));
    }

    #[test]
    fn parse_color_falls_back_to_reset_for_unknown() {
        assert_eq!(parse_color("nonexistent"), Color::Reset);
        assert_eq!(parse_color(""), Color::Reset);
    }
}
