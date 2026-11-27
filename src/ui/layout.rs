use ratatui::layout::{Constraint, Direction, Layout, Rect};

/// Computed regions for the agent tab strip plus the main two-column layout.
pub struct MainLayout {
    pub tabs: Rect,
    pub projects: Rect,
    pub sessions: Rect,
    pub status: Rect,
}

/// Split the full terminal area into a one-row agent tab strip, the main
/// two-column layout (projects | sessions), and a status bar at the bottom.
/// Pane focus is shown by the pane borders; there is no focus tab row.
pub fn compute_main_layout(area: Rect) -> MainLayout {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),
            Constraint::Min(0),
            Constraint::Length(2),
        ])
        .split(area);

    let columns = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(32), Constraint::Percentage(68)])
        .spacing(1)
        .split(chunks[1]);

    MainLayout {
        tabs: chunks[0],
        projects: columns[0],
        sessions: columns[1],
        status: chunks[2],
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
    fn panes_start_below_the_agent_tab_strip() {
        let layout = compute_main_layout(Rect::new(0, 0, 100, 30));

        assert_eq!(layout.tabs.top(), 0);
        assert_eq!(layout.tabs.height, 1);
        assert_eq!(layout.projects.top(), 1);
        assert_eq!(layout.sessions.top(), 1);
        assert_eq!(layout.projects.height, 27);
        assert_eq!(layout.sessions.height, 27);
    }

    #[test]
    fn main_panes_are_separated_by_a_single_cell_margin() {
        let layout = compute_main_layout(Rect::new(0, 0, 100, 30));

        assert_eq!(layout.projects.right() + 1, layout.sessions.left());
        assert_eq!(layout.projects.bottom(), layout.status.top());
        assert_eq!(layout.sessions.bottom(), layout.status.top());
    }
}
