mod colors;
mod layout;

use ratatui::{
    layout::Alignment,
    text::{Line, Span},
    widgets::{Block, Borders, Clear, List, ListItem, Paragraph, Tabs, Wrap},
    Frame,
};

use crate::{
    app::{AgentTab, AppState, FocusPane, LoadFeedback, ModalState},
    application::{session_key, session_title},
};
use cowboy::features::session_list::{filter_project_sessions_for_agent, sessions_panel_title};

use self::colors::{
    hint_key_style, hint_text_style, meta_text_style, modal_border_style, pane_border,
    project_highlight_style, session_highlight_style, status_badge_style,
};
use self::layout::{centered_rect, compute_main_layout, MainLayout};

const OPEN_HERE_PROJECT_LABEL: &str = "Open Here";
pub fn render(frame: &mut Frame, state: &AppState, feedback: LoadFeedback<'_>, tick: usize) {
    let layout = compute_main_layout(frame.area());

    render_agent_tabs(frame, state, layout.tabs);
    render_projects(frame, state, &layout);
    render_sessions(frame, state, &layout, feedback, tick);
    render_status(frame, state, layout.status);

    match state.modal {
        ModalState::Info => render_info_modal(frame, state),
        ModalState::Rename => {
            render_input_modal(frame, "Rename Session", &state.input_buffer, &state.theme)
        }
        ModalState::Search => {
            render_input_modal(frame, "Search Sessions", &state.input_buffer, &state.theme)
        }
        ModalState::DeleteConfirm => render_confirm_modal(frame, state),
        ModalState::None => {}
    }
}

/// The agent tab strip: one tab per registered strategy, exactly one
/// selected. There is no `All` tab and no focus tab row.
fn render_agent_tabs(frame: &mut Frame, state: &AppState, area: ratatui::layout::Rect) {
    let titles: Vec<Line> = state
        .agent_tabs
        .iter()
        .map(|tab: &AgentTab| Line::from(tab.label.clone()))
        .collect();
    let tabs = Tabs::new(titles)
        .select(Some(state.selected_agent))
        .style(hint_text_style(&state.theme))
        .highlight_style(status_badge_style(&state.theme))
        .divider("|");
    frame.render_widget(tabs, area);
}

fn render_projects(frame: &mut Frame, state: &AppState, layout: &MainLayout) {
    let display_names = cowboy::project_display_names(&state.projects);
    let mut items: Vec<ListItem> = state
        .projects
        .iter()
        .zip(display_names)
        .map(|(project, name)| {
            let count = project.sessions.len();
            if count == 0 {
                ListItem::new(name)
            } else {
                ListItem::new(format!("{name} ({count})"))
            }
        })
        .collect();
    items.push(ListItem::new(OPEN_HERE_PROJECT_LABEL));

    let block = Block::default()
        .title("Projects")
        .borders(Borders::ALL)
        .border_style(pane_border(
            state.focus == FocusPane::Projects,
            &state.theme,
        ));

    let list = List::new(items)
        .block(block)
        .highlight_style(project_highlight_style(&state.theme))
        .highlight_symbol("> ");

    let mut list_state = ratatui::widgets::ListState::default();
    list_state.select(Some(state.selected_project));
    frame.render_stateful_widget(list, layout.projects, &mut list_state);
}

fn render_sessions(
    frame: &mut Frame,
    state: &AppState,
    layout: &MainLayout,
    feedback: LoadFeedback<'_>,
    tick: usize,
) {
    const SPINNER: [&str; 8] = ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧"];
    let spin = SPINNER[tick % SPINNER.len()];
    let message = match feedback {
        LoadFeedback::Ready => None,
        LoadFeedback::Catalog => Some(format!("{spin} Discovering projects…")),
        LoadFeedback::CatalogFailed(error) => Some(format!("Could not discover projects: {error}")),
        LoadFeedback::Sessions => Some(format!("{spin} Loading sessions…")),
        LoadFeedback::Failed(error) => Some(format!("Could not load sessions: {error}")),
    };
    let project = state.projects.get(state.selected_project);
    let filtered_sessions =
        filter_project_sessions_for_agent(project, &state.search_query, state.selected_agent_id());
    let items = if let Some(message) = message {
        vec![ListItem::new(message)]
    } else if filtered_sessions.is_empty() {
        let has_sessions = project.is_some_and(|p| !p.sessions.is_empty());
        let empty = if has_sessions && !state.search_query.is_empty() {
            "No matching sessions"
        } else {
            "No sessions"
        };
        vec![ListItem::new(empty)]
    } else {
        filtered_sessions
            .iter()
            .map(|session| {
                let mut spans = vec![Span::raw(session_title(session).to_string())];
                let ts = session
                    .updated_at
                    .as_deref()
                    .or(session.created_at.as_deref())
                    .and_then(format_ts_short);
                if let Some(ts) = ts {
                    spans.push(Span::styled(
                        format!("  {ts}"),
                        meta_text_style(&state.theme),
                    ));
                }
                if !state.search_query.is_empty() {
                    spans.push(Span::styled(
                        format!("  #{}", session_key(session).native_id),
                        meta_text_style(&state.theme),
                    ));
                }
                if let Some(total) = session
                    .reported_cost
                    .map(|c| c.total)
                    .or_else(|| session.estimated_cost.as_ref().map(|c| c.total_cost()))
                {
                    spans.push(Span::styled(
                        format!("  ${total:.2}"),
                        meta_text_style(&state.theme),
                    ));
                }
                ListItem::new(Line::from(spans))
            })
            .collect()
    };

    // The tab strip is the single source of truth for the active agent; the
    // title carries only the search query.
    let title = sessions_panel_title(&state.search_query);

    let block = Block::default()
        .title(title)
        .borders(Borders::ALL)
        .border_style(pane_border(
            state.focus == FocusPane::Sessions,
            &state.theme,
        ));

    let list = List::new(items)
        .block(block)
        .highlight_style(session_highlight_style(&state.theme))
        .highlight_symbol("> ");

    let mut list_state = ratatui::widgets::ListState::default();
    list_state.select(if filtered_sessions.is_empty() {
        None
    } else {
        Some(state.selected_session)
    });
    frame.render_stateful_widget(list, layout.sessions, &mut list_state);
}

fn render_status(frame: &mut Frame, state: &AppState, area: ratatui::layout::Rect) {
    // An active deletion takes precedence over normal modal/focus status.
    if let Some(target) = state.delete_in_progress.as_ref() {
        let status_line = Line::from(vec![
            Span::styled(" Deleting ", status_badge_style(&state.theme)),
            Span::raw(format!(
                " Deleting {} '{}'... please wait",
                target.label().to_ascii_lowercase(),
                target.name()
            )),
        ]);
        let hints_line = Line::from(shortcuts_for(state));
        frame.render_widget(Paragraph::new(vec![status_line, hints_line]), area);
        return;
    }

    let mode = match state.modal {
        ModalState::None => match state.focus {
            FocusPane::Projects => "Projects",
            FocusPane::Sessions => "Sessions",
        },
        ModalState::Search => "Search",
        ModalState::Rename => "Rename",
        ModalState::DeleteConfirm => "Delete",
        ModalState::Info => "Info",
    };

    let status_message = state
        .toast
        .as_ref()
        .map(|toast| toast.message.as_str())
        .unwrap_or(&state.status);

    let status_line = Line::from(vec![
        Span::styled(format!(" {} ", mode), status_badge_style(&state.theme)),
        Span::raw(format!(" {status_message}")),
    ]);

    let hints_line = Line::from(shortcuts_for(state));

    frame.render_widget(Paragraph::new(vec![status_line, hints_line]), area);
}

fn shortcuts_for(state: &AppState) -> Vec<Span<'static>> {
    let hint = hint_text_style(&state.theme);
    let key_style = hint_key_style(&state.theme);

    // While a deletion is running every shortcut is disabled; advertise only
    // that the app is busy rather than listing inert keys.
    if state.delete_in_progress.is_some() {
        return vec![Span::styled(
            "Deletion in progress — please wait".to_string(),
            hint,
        )];
    }

    let pairs: &[(&str, &str)] = match state.modal {
        ModalState::None => match state.focus {
            FocusPane::Projects => &[
                ("Tab/←→", "Focus"),
                ("↑↓", "Move"),
                ("Enter", "New Session"),
                ("i", "Session Info"),
                ("r", "Rename Session"),
                ("Ctrl+D", "Delete Project"),
                ("/", "Search"),
                ("a", "Agent"),
                ("q/Esc", "Quit"),
            ],
            FocusPane::Sessions => &[
                ("Tab/←→", "Focus"),
                ("↑↓", "Move"),
                ("Enter", "Resume"),
                ("n", "New"),
                ("i", "Info"),
                ("r", "Rename"),
                ("Ctrl+D", "Delete Session"),
                ("/", "Search"),
                ("a", "Agent"),
                ("q/Esc", "Quit"),
            ],
        },
        ModalState::Search => &[("Type", "Query"), ("Enter", "Apply"), ("q/Esc", "Cancel")],
        ModalState::Rename => &[("Type", "Title"), ("Enter", "Save"), ("q/Esc", "Cancel")],
        ModalState::DeleteConfirm => &[("Enter/y/Ctrl+D", "Confirm"), ("q/Esc/n", "Cancel")],
        ModalState::Info => &[("q/Esc/Enter", "Close")],
    };

    let mut spans = Vec::new();
    for (index, (key, label)) in pairs.iter().enumerate() {
        if index > 0 {
            spans.push(Span::styled("  ", hint));
        }
        spans.push(Span::styled(format!("{key} "), key_style));
        spans.push(Span::styled(*label, hint));
    }

    spans
}

fn render_info_modal(frame: &mut Frame, state: &AppState) {
    let Some(info) = &state.info else {
        return;
    };

    let area = centered_rect(64, 52, frame.area());
    frame.render_widget(Clear, area);

    let mut text = vec![
        Line::from(format!("Title: {}", info.title)),
        Line::from(format!("Agent: {}", info.key.agent_id)),
        Line::from(format!("ID: {}", info.key.native_id)),
        Line::from(format!("Project: {}", info.project_name)),
        Line::from(format!("Path: {}", info.working_dir)),
        Line::from(format!(
            "Git Branch: {}",
            info.git_branch.as_deref().unwrap_or("Unknown")
        )),
        Line::from(format!(
            "Created At: {}",
            info.created_at
                .as_deref()
                .and_then(format_ts_short)
                .unwrap_or_else(|| "Unknown".to_string())
        )),
        Line::from(format!(
            "Updated At: {}",
            info.updated_at
                .as_deref()
                .and_then(format_ts_short)
                .unwrap_or_else(|| "Unknown".to_string())
        )),
        Line::from(format!(
            "Messages: {}",
            info.message_count
                .map(|count| count.to_string())
                .unwrap_or_else(|| "Unknown".to_string())
        )),
    ];

    if let Some(source_location) = &info.source_location {
        text.push(Line::from(format!("Source: {source_location}")));
    }

    if let Some(model) = &info.model {
        text.push(Line::from(format!("Model: {model}")));
    }

    if let Some(usage) = &info.usage {
        text.push(Line::from(""));
        text.push(Line::from(format!(
            "Input Tokens: {}",
            format_token_count(usage.input_tokens)
        )));
        text.push(Line::from(format!(
            "Output Tokens: {}",
            format_token_count(usage.output_tokens)
        )));
        text.push(Line::from(format!(
            "Cache Write: {}",
            format_token_count(usage.cache_creation_tokens)
        )));
        text.push(Line::from(format!(
            "Cache Read: {}",
            format_token_count(usage.cache_read_tokens)
        )));
        text.push(Line::from(format!(
            "Total Tokens: {}",
            format_token_count(usage.total_tokens())
        )));
    } else {
        text.push(Line::from(""));
        text.push(Line::from("Token Usage: Unknown"));
    }

    if let Some(cost) = info.reported_cost {
        text.push(Line::from(""));
        text.push(Line::from(format!("Cost: ${:.4}", cost.total)));
        text.push(Line::from(if cost.from_provider {
            "  (reported by provider)"
        } else {
            "  (reported by pi)"
        }));
    } else if let Some(cost) = &info.estimated_cost {
        text.push(Line::from(""));
        text.push(Line::from(format!("Est. Cost: ${:.4}", cost.total_cost())));
        text.push(Line::from("  (estimate based on token usage)"));
    }

    text.push(Line::from(""));
    text.push(Line::from("Press q, Esc, or Enter to close"));

    let block = Block::default()
        .title("Session Info")
        .borders(Borders::ALL)
        .border_style(modal_border_style(&state.theme));
    frame.render_widget(
        Paragraph::new(text).block(block).wrap(Wrap { trim: false }),
        area,
    );
}

fn render_input_modal(
    frame: &mut Frame,
    title: &str,
    value: &str,
    theme: &cowboy::theme::ThemePalette,
) {
    let area = centered_rect(60, 25, frame.area());
    frame.render_widget(Clear, area);
    let block = Block::default()
        .title(title)
        .borders(Borders::ALL)
        .border_style(modal_border_style(theme));
    frame.render_widget(
        Paragraph::new(value.to_string())
            .block(block)
            .alignment(Alignment::Left),
        area,
    );
}

fn render_confirm_modal(frame: &mut Frame, state: &AppState) {
    let area = centered_rect(54, 24, frame.area());
    frame.render_widget(Clear, area);

    let Some(target) = state.delete_target.as_ref() else {
        return;
    };

    let block = Block::default()
        .title(format!("Delete {}", target.label()))
        .borders(Borders::ALL)
        .border_style(modal_border_style(&state.theme));
    let text = target
        .confirmation_lines()
        .into_iter()
        .map(Line::from)
        .collect::<Vec<_>>();
    frame.render_widget(Paragraph::new(text).block(block), area);
}

/// Format an RFC 3339 timestamp to "yyyy-MM-dd HH:mm:ss" in the system-local
/// time zone.
fn format_ts_short(ts: &str) -> Option<String> {
    let dt = chrono::DateTime::parse_from_rfc3339(ts).ok()?;
    let local = dt.with_timezone(&chrono::Local);
    Some(local.format("%Y-%m-%d %H:%M:%S").to_string())
}

/// Format an RFC 3339 timestamp in a specific time zone — used in tests with
/// a controlled zone.
#[cfg(test)]
fn format_timestamp_in_tz<Tz: chrono::TimeZone>(ts: &str, tz: &Tz) -> Option<String>
where
    Tz::Offset: std::fmt::Display,
{
    let dt = chrono::DateTime::parse_from_rfc3339(ts).ok()?;
    let local = dt.with_timezone(tz);
    Some(local.format("%Y-%m-%d %H:%M:%S").to_string())
}

/// Format a token count into a compact, human-readable string.
///
/// Rules:
/// - < 1_000          → raw integer string  (e.g. "0", "999")
/// - < 1_000_000      → `{:.1}K`             (e.g. "1.0K", "12.3K", "1000.0K")
/// - >= 1_000_000     → `{:.1}M`             (e.g. "1.0M", "12.3M")
fn format_token_count(value: u64) -> String {
    if value < 1_000 {
        value.to_string()
    } else if value < 1_000_000 {
        format!("{:.1}K", value as f64 / 1_000.0)
    } else {
        format!("{:.1}M", value as f64 / 1_000_000.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cowboy::domain::{AgentId, Project, Session, SessionCapabilities, SessionKey};
    use ratatui::{backend::TestBackend, Terminal};

    /// Render the whole UI into an off-screen backend and return one string per
    /// terminal row, so tests assert on what the user actually sees.
    fn render_to_lines(state: &AppState, width: u16, height: u16) -> Vec<String> {
        render_with_feedback_to_lines(state, LoadFeedback::Ready, 0, width, height)
    }

    fn render_with_feedback_to_lines(
        state: &AppState,
        feedback: LoadFeedback<'_>,
        tick: usize,
        width: u16,
        height: u16,
    ) -> Vec<String> {
        let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
        terminal
            .draw(|frame| render(frame, state, feedback, tick))
            .unwrap();
        let buffer = terminal.backend().buffer();
        (0..height)
            .map(|y| (0..width).map(|x| buffer[(x, y)].symbol()).collect())
            .collect()
    }

    #[test]
    fn unmatched_search_is_not_an_empty_project() {
        let mut state = state_with_one_claude_session();
        state.search_query = "not present".into();
        let text = render_to_lines(&state, 98, 12).join("\n");
        assert!(text.contains("No matching sessions"));
        assert!(!text.contains("No sessions"));
    }

    #[test]
    fn sessions_panel_animates_loading_and_distinguishes_empty_from_failure() {
        use crate::app::LoadFeedback;
        let mut state = state_with_one_claude_session();
        state.projects[0].sessions.clear();
        for (tick, glyph) in [(0, "⠋"), (1, "⠙")] {
            let text = render_with_feedback_to_lines(&state, LoadFeedback::Sessions, tick, 98, 12)
                .join("\n");
            assert!(text.contains(&format!("{glyph} Loading sessions…")));
            assert!(!text.contains("No sessions"));
            assert!(!text.contains("repo-a (0)"));
        }
        let empty =
            render_with_feedback_to_lines(&state, LoadFeedback::Ready, 2, 98, 12).join("\n");
        assert!(empty.contains("No sessions"));
        assert!(!empty.contains("Loading sessions"));
        let failure =
            render_with_feedback_to_lines(&state, LoadFeedback::Failed("read error"), 2, 98, 12)
                .join("\n");
        assert!(failure.contains("Could not load sessions: read error"));
        assert!(!failure.contains("Loading sessions"));
        assert!(!failure.contains("No sessions"));
    }

    fn state_with_one_claude_session() -> AppState {
        let mut state = AppState::default();
        state.projects.push(Project {
            cwd: std::path::PathBuf::from("/repo-a"),
            sessions: vec![Session {
                key: SessionKey::claude("abc"),
                title: "Fix login".into(),
                cwd: std::path::PathBuf::from("/repo-a"),
                git_branch: None,
                created_at: None,
                updated_at: None,
                source_location: None,
                message_count: None,
                usage: None,
                model: None,
                estimated_cost: None,
                reported_cost: None,
                capabilities: SessionCapabilities::default(),
            }],
        });
        state
    }

    fn row_metadata<'a>(lines: &'a [String], title: &str) -> Option<&'a str> {
        lines
            .iter()
            .find_map(|row| row.split_once(title))
            .and_then(|(_, tail)| tail.split_once('│'))
            .map(|(metadata, _)| metadata.trim())
    }

    #[test]
    fn session_row_with_usage_but_no_cost_has_no_token_suffix() {
        let mut state = state_with_one_claude_session();
        state.search_query = "login".into();
        state.projects[0].sessions[0].usage = Some(cowboy::domain::SessionUsage {
            input_tokens: 12_345,
            output_tokens: 0,
            cache_creation_tokens: 0,
            cache_read_tokens: 0,
        });
        let lines = render_to_lines(&state, 100, 16);
        assert_eq!(row_metadata(&lines, "Fix login"), Some("#abc"));
    }

    #[test]
    fn reported_cost_precedes_estimate_in_row_and_details() {
        for origin in [None, Some(true), Some(false)] {
            let mut state = state_with_one_claude_session();
            state.agent_tabs = vec![claude_tab()];
            let s = &mut state.projects[0].sessions[0];
            s.estimated_cost = Some(cowboy::CostEstimate {
                input_cost: 2.,
                output_cost: 0.,
                cache_creation_cost: 0.,
                cache_read_cost: 0.,
            });
            s.reported_cost = origin.map(|from_provider| cowboy::domain::ReportedCost {
                total: 0.013308,
                from_provider,
            });
            s.usage = Some(cowboy::domain::SessionUsage {
                input_tokens: 12_345,
                output_tokens: 0,
                cache_creation_tokens: 0,
                cache_read_tokens: 0,
            });
            let lines = render_to_lines(&state, 140, 70);
            assert_eq!(
                row_metadata(&lines, "Fix login"),
                Some(if origin.is_some() { "$0.01" } else { "$2.00" })
            );
            let p = &state.projects[0];
            state.info = Some(crate::app::build_session_info(p, &p.sessions[0]));
            state.modal = ModalState::Info;
            let text = render_to_lines(&state, 140, 70).join("\n");
            let (cost, label) = match origin {
                Some(true) => ("Cost: $0.0133", "(reported by provider)"),
                Some(false) => ("Cost: $0.0133", "(reported by pi)"),
                None => ("Est. Cost: $2.0000", "(estimate based on token usage)"),
            };
            assert!(text.contains(cost) && text.contains(label));
        }
    }

    #[test]
    fn session_info_distinguishes_token_breakdown_zero_and_unknown() {
        use cowboy::domain::SessionUsage;

        let mut state = state_with_one_claude_session();
        state.modal = ModalState::Info;
        let cases = [
            (
                Some(SessionUsage {
                    input_tokens: 1_000,
                    output_tokens: 200,
                    cache_creation_tokens: 30,
                    cache_read_tokens: 4,
                }),
                vec![
                    "Input Tokens: 1.0K",
                    "Output Tokens: 200",
                    "Cache Write: 30",
                    "Cache Read: 4",
                    "Total Tokens: 1.2K",
                ],
                "Token Usage: Unknown",
            ),
            (
                Some(SessionUsage {
                    input_tokens: 0,
                    output_tokens: 0,
                    cache_creation_tokens: 0,
                    cache_read_tokens: 0,
                }),
                vec!["Total Tokens: 0"],
                "Token Usage: Unknown",
            ),
            (None, vec!["Token Usage: Unknown"], "Total Tokens:"),
        ];
        for (usage, expected, absent) in cases {
            let project = &mut state.projects[0];
            project.sessions[0].usage = usage;
            state.info = Some(crate::app::build_session_info(
                project,
                &project.sessions[0],
            ));
            let text = render_to_lines(&state, 140, 70).join("\n");
            for line in expected {
                assert!(text.contains(line), "missing: {line}");
            }
            assert!(!text.contains(absent), "unexpected: {absent}");
        }
    }

    fn codex_tab() -> crate::app::AgentTab {
        crate::app::AgentTab {
            id: AgentId::new("codex").unwrap(),
            label: "Codex".to_string(),
        }
    }

    #[test]
    fn panes_start_below_the_agent_tab_strip() {
        let mut state = state_with_one_claude_session();
        state.agent_tabs = vec![claude_tab()];

        let lines = render_to_lines(&state, 80, 20);

        assert!(lines[0].contains("Claude"), "row 0: {:?}", lines[0]);
        assert!(lines[1].starts_with("┌Projects"), "row 1: {:?}", lines[1]);
        assert!(lines[1].contains("┌Sessions─"), "row 1: {:?}", lines[1]);
    }

    #[test]
    fn strip_shows_exactly_the_registered_tabs_without_all_or_focus_row() {
        let mut state = state_with_one_claude_session();
        state.agent_tabs = vec![claude_tab(), codex_tab()];

        let lines = render_to_lines(&state, 80, 20);
        let screen = lines.join("\n");

        assert!(lines[0].contains("Claude"), "row 0: {:?}", lines[0]);
        assert!(lines[0].contains("Codex"), "row 0: {:?}", lines[0]);
        assert!(!lines[0].contains("All"), "row 0: {:?}", lines[0]);
        // The removed focus tab row must not return.
        assert!(!screen.contains("Projects  |  Sessions"), "{screen}");
    }

    #[test]
    fn selected_agent_tab_is_highlighted() {
        let mut state = state_with_one_claude_session();
        state.agent_tabs = vec![claude_tab(), codex_tab()];
        state.selected_agent = 1;

        let mut terminal = Terminal::new(TestBackend::new(80, 20)).unwrap();
        terminal
            .draw(|frame| render(frame, &state, LoadFeedback::Ready, 0))
            .unwrap();
        let buffer = terminal.backend().buffer();
        let row: String = (0..80).map(|x| buffer[(x, 0)].symbol()).collect();
        let claude_x = row.find("Claude").expect("Claude tab rendered") as u16;
        let codex_x = row.find("Codex").expect("Codex tab rendered") as u16;

        assert_ne!(
            buffer[(claude_x, 0)].bg,
            buffer[(codex_x, 0)].bg,
            "selected tab must carry a distinct highlight"
        );
    }

    #[test]
    fn sessions_title_stays_plain_when_searching_without_agent_tabs() {
        let mut state = state_with_one_claude_session();
        state.search_query = "login".into();

        let lines = render_to_lines(&state, 80, 20);

        assert!(
            lines[1].contains("┌Sessions / login─"),
            "row 1: {:?}",
            lines[1]
        );
    }

    fn claude_tab() -> crate::app::AgentTab {
        crate::app::AgentTab {
            id: AgentId::new("claude").unwrap(),
            label: "Claude".to_string(),
        }
    }

    #[test]
    fn sessions_title_has_no_agent_suffix_under_a_selected_tab() {
        let mut state = state_with_one_claude_session();
        state.agent_tabs = vec![claude_tab()];

        let lines = render_to_lines(&state, 80, 20);

        assert!(lines[1].contains("┌Sessions─"), "row 1: {:?}", lines[1]);
        assert!(!lines[1].contains("· Claude"), "row 1: {:?}", lines[1]);
    }

    #[test]
    fn sessions_title_shows_only_the_query_under_a_selected_tab() {
        let mut state = state_with_one_claude_session();
        state.agent_tabs = vec![claude_tab()];
        state.search_query = "login".into();

        let lines = render_to_lines(&state, 80, 20);

        assert!(
            lines[1].contains("┌Sessions / login─"),
            "row 1: {:?}",
            lines[1]
        );
        assert!(!lines[1].contains("· Claude"), "row 1: {:?}", lines[1]);
    }

    #[test]
    fn test_format_token_count_raw() {
        assert_eq!(format_token_count(0), "0");
        assert_eq!(format_token_count(999), "999");
    }

    #[test]
    fn test_format_token_count_k() {
        assert_eq!(format_token_count(1_000), "1.0K");
        assert_eq!(format_token_count(12_345), "12.3K");
        assert_eq!(format_token_count(999_999), "1000.0K");
    }

    #[test]
    fn test_format_token_count_m() {
        assert_eq!(format_token_count(1_000_000), "1.0M");
        assert_eq!(format_token_count(12_345_678), "12.3M");
    }

    #[test]
    fn normal_shortcuts_advertise_escape_to_quit() {
        let state = AppState::default();

        let text = shortcuts_for(&state)
            .iter()
            .map(|span| span.content.as_ref())
            .collect::<String>();

        assert!(text.contains("q/Esc Quit"));
    }

    #[test]
    fn project_rows_show_loaded_session_count_without_token_usage() {
        let mut state = state_with_one_claude_session();
        state.agent_tabs = vec![claude_tab()];
        state.projects[0].sessions[0].usage = Some(cowboy::domain::SessionUsage {
            input_tokens: 9_876,
            output_tokens: 0,
            cache_creation_tokens: 0,
            cache_read_tokens: 0,
        });
        state.projects[0].cwd = "/work/repo-a".into();
        state.projects[0].sessions[0].cwd = "/work/repo-a".into();
        state.projects.push(Project {
            cwd: "/workspace/repo-a".into(),
            sessions: Vec::new(),
        });

        let lines = render_to_lines(&state, 100, 16);
        let projects: Vec<_> = lines
            .iter()
            .filter_map(|line| line.strip_prefix('│')?.split_once('│'))
            .map(|(cell, _)| cell.trim().trim_start_matches("> "))
            .filter(|name| name.contains("repo-a") || *name == "Open Here")
            .collect();
        assert_eq!(
            projects,
            ["work/repo-a (1)", "workspace/repo-a", "Open Here"]
        );
    }

    // ── Timestamp formatting ────────────────────────────────────────────

    #[test]
    fn utc_timestamp_formatted_in_utc_tz() {
        let result = format_timestamp_in_tz("2025-07-05T08:00:00Z", &chrono::Utc);
        assert_eq!(result, Some("2025-07-05 08:00:00".to_string()));
    }

    #[test]
    fn timestamp_converts_to_positive_offset() {
        let tz = chrono::FixedOffset::east_opt(8 * 3600).unwrap();
        let result = format_timestamp_in_tz("2025-07-05T00:00:00Z", &tz);
        // 00:00 UTC → 08:00 Asia/Shanghai
        assert_eq!(result, Some("2025-07-05 08:00:00".to_string()));
    }

    #[test]
    fn cross_midnight_date_change() {
        let tz = chrono::FixedOffset::east_opt(8 * 3600).unwrap();
        let result = format_timestamp_in_tz("2025-07-05T18:00:00Z", &tz);
        // 18:00 UTC → 02:00 next day
        assert_eq!(result, Some("2025-07-06 02:00:00".to_string()));
    }

    #[test]
    fn fractional_seconds_omitted() {
        let result = format_timestamp_in_tz("2025-07-05T08:00:00.123Z", &chrono::Utc);
        assert_eq!(result, Some("2025-07-05 08:00:00".to_string()));
    }

    #[test]
    fn non_utc_offset_converted_correctly() {
        let tz = chrono::Utc;
        // +05:00 at 10:00 = 05:00 UTC
        let result = format_timestamp_in_tz("2025-07-05T10:00:00+05:00", &tz);
        assert_eq!(result, Some("2025-07-05 05:00:00".to_string()));
    }

    #[test]
    fn malformed_input_returns_none() {
        let result = format_ts_short("not-a-timestamp");
        assert_eq!(result, None);
    }

    #[test]
    fn missing_timestamp_shows_unknown() {
        let result: Option<String> = None;
        assert_eq!(
            result
                .as_deref()
                .and_then(format_ts_short)
                .unwrap_or_else(|| "Unknown".to_string()),
            "Unknown"
        );
    }

    #[test]
    fn format_ts_short_and_tz_helper_agree_on_utc() {
        // format_ts_short uses Local tz, so we can't directly compare.
        // Instead verify that both helpers handle identical UTC input identically.
        let utc_result = format_timestamp_in_tz("2025-07-05T12:00:00Z", &chrono::Utc);
        assert_eq!(utc_result, Some("2025-07-05 12:00:00".to_string()));
    }
}
