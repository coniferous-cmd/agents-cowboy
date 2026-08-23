use ratatui::layout::{Constraint, Direction, Layout, Rect};

/// Computed regions for the main two-column layout.
pub struct MainLayout {
    pub tabs: Rect,
    pub projects: Rect,
    pub sessions: Rect,
    pub profiles: Rect,
    pub status: Rect,
}

/// Split the full terminal area into the main two-column layout
/// (projects | sessions) plus a status bar at the bottom.
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

    // Profiles now occupy the full middle column; the legacy snapshot subpanel
    // was removed when snapshots were dropped.
    let profile_area = chunks[1];

    MainLayout {
        tabs: chunks[0],
        projects: columns[0],
        sessions: columns[1],
        profiles: profile_area,
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
    fn main_panes_are_separated_by_a_single_cell_margin() {
        let layout = compute_main_layout(Rect::new(0, 0, 100, 30));

        assert_eq!(layout.projects.right() + 1, layout.sessions.left());
        // profiles occupy the entire middle chunk, so its bottom edge equals
        // the start of the status bar.
        assert_eq!(layout.profiles.bottom(), layout.status.top());
    }
}
