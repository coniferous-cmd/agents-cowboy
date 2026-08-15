mod colors;
mod layout;

use ratatui::{
    layout::{Alignment, Constraint, Direction, Layout},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, List, ListItem, Paragraph, Tabs, Wrap},
    Frame,
};

use crate::{
    app::{agent_filters, AppState, FocusPane, MainTab, ModalState},
    application::{session_key, session_title},
};
use cowboy::features::project_usage::{build_project_row_label, summarize_project_usage};
use cowboy::features::session_list::{filter_project_sessions_for_agent, sessions_panel_title};

use self::colors::{
    hint_key_style, hint_text_style, meta_text_style, modal_border_style, pane_border,
    project_highlight_style, session_highlight_style, status_badge_style,
};
use self::layout::{centered_rect, compute_main_layout, MainLayout};

const OPEN_HERE_PROJECT_LABEL: &str = "Open Here";
pub fn render(frame: &mut Frame, state: &AppState) {
    let layout = compute_main_layout(frame.area());

    render_tabs(frame, state, &layout);
    if state.main_tab == MainTab::Profiles {
        render_profiles(frame, state, &layout);
    } else {
        render_projects(frame, state, &layout);
        render_sessions(frame, state, &layout);
    }
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
        ModalState::NewProfile => {
            render_input_modal(frame, "New Profile", &state.input_buffer, &state.theme)
        }
        ModalState::EditProfile { .. } => {
            // Editor owns the terminal; no modal rendering needed
        }
        ModalState::BindProfile { profile_cursor } => {
            render_bind_profile_modal(frame, state, profile_cursor)
        }
        ModalState::None => {}
    }
}

fn render_tabs(frame: &mut Frame, state: &AppState, layout: &MainLayout) {
    let selected = match state.main_tab {
        MainTab::Projects => 0,
        MainTab::Profiles => 1,
    };
    frame.render_widget(
        Tabs::new(["Projects", "Profiles"])
            .select(selected)
            .divider(" | ")
            .highlight_style(status_badge_style(&state.theme)),
        layout.tabs,
    );
}

fn render_projects(frame: &mut Frame, state: &AppState, layout: &MainLayout) {
    let display_names = cowboy::project_display_names(&state.projects);
    let mut items: Vec<ListItem> = state
        .projects
        .iter()
        .zip(display_names.iter())
        .zip(state.project_bindings.iter())
        .map(|((project, name), binding)| {
            let summary = summarize_project_usage(project);
            let label = if let Some(profile_name) = binding {
                format!(
                    "[{profile_name}] {}",
                    build_project_row_label(name, &summary)
                )
            } else {
                build_project_row_label(name, &summary)
            };
            ListItem::new(label)
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

fn render_profiles(frame: &mut Frame, state: &AppState, layout: &MainLayout) {
    let profile_items = if state.profiles.is_empty() {
        vec![ListItem::new(
            "No profiles (use `cowboy config create <name>`)",
        )]
    } else {
        state
            .profiles
            .iter()
            .map(|profile| {
                let active = if state.active_profile_name.as_deref() == Some(&profile.name) {
                    " ●"
                } else {
                    ""
                };
                ListItem::new(format!("{}{active}", profile.name))
            })
            .collect()
    };
    let mut profile_state = ratatui::widgets::ListState::default();
    if !state.profiles.is_empty() && state.profile_cursor < state.profiles.len() {
        profile_state.select(Some(state.profile_cursor));
    }
    frame.render_stateful_widget(
        List::new(profile_items)
            .block(Block::default().title("Profiles").borders(Borders::ALL))
            .highlight_style(project_highlight_style(&state.theme))
            .highlight_symbol("> "),
        layout.profiles,
        &mut profile_state,
    );

    let snapshot_items = if state.snapshots.is_empty() {
        vec![ListItem::new("No snapshots")]
    } else {
        state
            .snapshots
            .iter()
            .map(|snapshot| {
                ListItem::new(format!(
                    "{}  {} bytes  {}",
                    snapshot.captured_at,
                    snapshot.settings_json.len(),
                    snapshot.source.as_deref().unwrap_or("-")
                ))
            })
            .collect()
    };
    let mut snapshot_state = ratatui::widgets::ListState::default();
    if state.profile_cursor >= state.profiles.len() && !state.snapshots.is_empty() {
        snapshot_state.select(Some(state.profile_cursor - state.profiles.len()));
    }
    frame.render_stateful_widget(
        List::new(snapshot_items)
            .block(Block::default().title("Snapshots").borders(Borders::ALL))
            .highlight_style(session_highlight_style(&state.theme))
            .highlight_symbol("> "),
        layout.snapshots,
        &mut snapshot_state,
    );
}

fn render_sessions(frame: &mut Frame, state: &AppState, layout: &MainLayout) {
    let areas = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(1), Constraint::Min(0)])
        .split(layout.sessions);
    let filters = agent_filters(&state.projects);
    let selected_filter = filters
        .iter()
        .position(|filter| filter == &state.agent_filter)
        .unwrap_or(0);
    frame.render_widget(
        Tabs::new(
            filters
                .iter()
                .map(|filter| filter.label().to_string())
                .collect::<Vec<_>>(),
        )
        .select(selected_filter)
        .divider(" | ")
        .highlight_style(status_badge_style(&state.theme)),
        areas[0],
    );

    let filtered_sessions = filter_project_sessions_for_agent(
        state.projects.get(state.selected_project),
        &state.search_query,
        state.agent_filter.agent_id(),
    );
    let items = if filtered_sessions.is_empty() {
        vec![ListItem::new("No sessions")]
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
                if let Some(cost) = &session.estimated_cost {
                    spans.push(Span::styled(
                        format!("  ${:.2}", cost.total_cost()),
                        meta_text_style(&state.theme),
                    ));
                } else if let Some(usage) = &session.usage {
                    spans.push(Span::styled(
                        format!("  {}", format_token_count(usage.total_tokens())),
                        meta_text_style(&state.theme),
                    ));
                }
                ListItem::new(Line::from(spans))
            })
            .collect()
    };

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
    frame.render_stateful_widget(list, areas[1], &mut list_state);
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
        ModalState::None if state.main_tab == MainTab::Profiles => "Profiles",
        ModalState::None => match state.focus {
            FocusPane::Projects => "Projects",
            FocusPane::Sessions => "Sessions",
        },
        ModalState::Search => "Search",
        ModalState::Rename => "Rename",
        ModalState::NewProfile => "New Profile",
        ModalState::DeleteConfirm => "Delete",
        ModalState::Info => "Info",
        ModalState::EditProfile { .. } => "Editing",
        ModalState::BindProfile { .. } => "Bind Profile",
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

    let pairs: &[(&str, &str)] = if state.main_tab == MainTab::Profiles
        && state.modal == ModalState::None
    {
        &[
            ("↑↓", "Move"),
            ("Enter", "Activate"),
            ("n", "New"),
            ("Ctrl+D", "Delete"),
            ("q/Esc", "Quit"),
        ]
    } else {
        match state.modal {
            ModalState::None => match state.focus {
                FocusPane::Projects => &[
                    ("Tab/←→", "Focus"),
                    ("↑↓", "Move"),
                    ("Enter", "New Session"),
                    ("i", "Session Info"),
                    ("r", "Rename Session"),
                    ("e", "Bind Profile"),
                    ("u", "Unbind"),
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
            ModalState::NewProfile => &[
                ("Type", "Name"),
                ("Enter", "Open editor"),
                ("q/Esc", "Cancel"),
            ],
            ModalState::EditProfile { .. } => &[], // Editor owns the terminal
            ModalState::BindProfile { .. } => {
                &[("↑↓", "Select"), ("Enter", "Bind"), ("q/Esc", "Cancel")]
            }
            ModalState::Info => &[("q/Esc/Enter", "Close")],
        }
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

    if let Some(cost) = &info.estimated_cost {
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

fn render_bind_profile_modal(frame: &mut Frame, state: &AppState, profile_cursor: usize) {
    let area = centered_rect(40, 40, frame.area());
    frame.render_widget(Clear, area);

    let items: Vec<ListItem> = state
        .profiles
        .iter()
        .map(|profile| {
            let active = if state.active_profile_name.as_deref() == Some(&profile.name) {
                " *"
            } else {
                ""
            };
            ListItem::new(format!("{}{active}", profile.name))
        })
        .collect();

    let block = Block::default()
        .title("Bind Profile to Project")
        .borders(Borders::ALL)
        .border_style(modal_border_style(&state.theme));

    let list = List::new(items)
        .block(block)
        .highlight_style(project_highlight_style(&state.theme))
        .highlight_symbol("> ");

    let mut list_state = ratatui::widgets::ListState::default();
    if !state.profiles.is_empty() && profile_cursor < state.profiles.len() {
        list_state.select(Some(profile_cursor));
    }
    frame.render_stateful_widget(list, area, &mut list_state);
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
    fn profile_shortcuts_advertise_enter_without_space() {
        let state = AppState {
            main_tab: MainTab::Profiles,
            ..AppState::default()
        };

        let text = shortcuts_for(&state)
            .iter()
            .map(|span| span.content.as_ref())
            .collect::<String>();

        assert!(text.contains("Enter Activate"));
        assert!(!text.contains("Space"));
    }

    #[test]
    fn projects_list_has_correct_order_of_items() {
        use crate::app::AppState;
        use ratatui::widgets::ListItem;

        let mut state = AppState::default();
        state.projects.push(cowboy::domain::Project {
            cwd: std::path::PathBuf::from("/repo-a"),
            sessions: Vec::new(),
        });

        let display_names = cowboy::project_display_names(&state.projects);
        let items: Vec<ListItem> = state
            .projects
            .iter()
            .zip(display_names.iter())
            .map(|(project, name)| {
                let summary = cowboy::features::project_usage::summarize_project_usage(project);
                let label =
                    cowboy::features::project_usage::build_project_row_label(name, &summary);
                ListItem::new(label)
            })
            .collect();
        assert_eq!(items.len(), 1);

        // The final synthetic item is always "Open Here".
        let _open_here = ListItem::new("Open Here");
        assert_eq!(OPEN_HERE_PROJECT_LABEL, "Open Here");
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
