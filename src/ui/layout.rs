use ratatui::layout::{Constraint, Direction, Layout, Rect};

/// Computed regions for the main two-column layout.
pub struct MainLayout {
    pub projects: Rect,
    pub sessions: Rect,
    pub status: Rect,
}

/// Split the full terminal area into the main two-column layout
/// (projects | sessions) plus a status bar at the bottom. The panes start at
/// the top row; focus is shown by the pane borders, not a tab strip.
pub fn compute_main_layout(area: Rect) -> MainLayout {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(0), Constraint::Length(2)])
        .split(area);

    let columns = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(32), Constraint::Percentage(68)])
        .spacing(1)
        .split(chunks[0]);

    MainLayout {
        projects: columns[0],
        sessions: columns[1],
        status: chunks[1],
    }
}

/// Compute a centered modal rectangle as a percentage of the full area.
pub fn centered_rect(width_pct: u16, height_pct: u16, area: Rect) -> Rect {
    let vertical = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage((100 - height_pct) / 2),
            Constraint::Percentage(height_pct),
            Constraint::Percentage((100 - height_pct) / 2),
        ])
        .split(area);

    Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage((100 - width_pct) / 2),
            Constraint::Percentage(width_pct),
            Constraint::Percentage((100 - width_pct) / 2),
        ])
        .split(vertical[1])[1]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn panes_start_at_the_top_row_without_a_tab_strip() {
        let layout = compute_main_layout(Rect::new(0, 0, 100, 30));

        assert_eq!(layout.projects.top(), 0);
        assert_eq!(layout.sessions.top(), 0);
        assert_eq!(layout.projects.height, 28);
        assert_eq!(layout.sessions.height, 28);
    }

    #[test]
    fn main_panes_are_separated_by_a_single_cell_margin() {
        let layout = compute_main_layout(Rect::new(0, 0, 100, 30));

        assert_eq!(layout.projects.right() + 1, layout.sessions.left());
        assert_eq!(layout.projects.bottom(), layout.status.top());
        assert_eq!(layout.sessions.bottom(), layout.status.top());
    }
}
