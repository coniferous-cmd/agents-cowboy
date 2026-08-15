use ratatui::style::{Modifier, Style};

use cowboy::theme::ThemePalette;

/// Style for the currently focused pane border.
pub fn pane_border(is_active: bool, theme: &ThemePalette) -> Style {
    if is_active {
        Style::default().fg(theme.active_pane_border)
    } else {
        Style::default().fg(theme.inactive_pane_border)
    }
}

/// Highlight style for the selected project item.
pub fn project_highlight_style(theme: &ThemePalette) -> Style {
    Style::default()
        .fg(theme.project_highlight)
        .add_modifier(Modifier::BOLD)
}

/// Highlight style for the selected session item.
pub fn session_highlight_style(theme: &ThemePalette) -> Style {
    Style::default()
        .fg(theme.session_highlight)
        .add_modifier(Modifier::BOLD)
}

/// Style for secondary metadata text (timestamps, session IDs).
pub fn meta_text_style(theme: &ThemePalette) -> Style {
    Style::default().fg(theme.meta_text_fg)
}

/// Style for the mode badge in the status bar.
pub fn status_badge_style(theme: &ThemePalette) -> Style {
    Style::default()
        .bg(theme.status_badge_bg)
        .fg(theme.status_badge_fg)
}

/// Style for shortcut-key labels in the hint bar.
pub fn hint_key_style(theme: &ThemePalette) -> Style {
    Style::default()
        .fg(theme.hint_key_fg)
        .add_modifier(Modifier::BOLD)
}

/// Style for shortcut description text in the hint bar.
pub fn hint_text_style(theme: &ThemePalette) -> Style {
    Style::default().fg(theme.hint_text_fg)
}

/// Style for modal window borders.
pub fn modal_border_style(theme: &ThemePalette) -> Style {
    Style::default().fg(theme.modal_border)
}
