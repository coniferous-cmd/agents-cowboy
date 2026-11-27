use crate::domain::{AgentId, Project, Session};

pub fn filter_project_sessions<'a>(project: Option<&'a Project>, query: &str) -> Vec<&'a Session> {
    filter_project_sessions_for_agent(project, query, None)
}

pub fn filter_project_sessions_for_agent<'a>(
    project: Option<&'a Project>,
    query: &str,
    agent_id: Option<&AgentId>,
) -> Vec<&'a Session> {
    let Some(project) = project else {
        return Vec::new();
    };

    let query = query.trim().to_ascii_lowercase();
    project
        .sessions
        .iter()
        .filter(|session| {
            agent_id.is_none_or(|agent_id| session.key.agent_id == *agent_id)
                && (query.is_empty()
                    || session.title.to_ascii_lowercase().contains(&query)
                    || session.native_id().to_ascii_lowercase().contains(&query))
        })
        .collect()
}

/// Title of the sessions pane. `agent_label` is the active agent filter, or
/// `None` when every agent is shown; the search query, if any, follows it.
pub fn sessions_panel_title(agent_label: Option<&str>, query: &str) -> String {
    let mut title = match agent_label {
        Some(label) => format!("Sessions · {label}"),
        None => "Sessions".to_string(),
    };
    if !query.is_empty() {
        title.push_str(&format!(" / {query}"));
    }
    title
}

#[cfg(test)]
mod tests {
    use super::{filter_project_sessions, sessions_panel_title};
    use crate::domain::{Project, Session, SessionCapabilities, SessionKey};
    use std::path::PathBuf;

    #[test]
    fn panel_title_is_plain_sessions_without_filter_or_query() {
        assert_eq!(sessions_panel_title(None, ""), "Sessions");
    }

    #[test]
    fn panel_title_shows_the_agent_filter_when_not_searching() {
        assert_eq!(
            sessions_panel_title(Some("Claude"), ""),
            "Sessions · Claude"
        );
    }

    #[test]
    fn panel_title_shows_the_query_without_an_agent_filter() {
        assert_eq!(sessions_panel_title(None, "fix"), "Sessions / fix");
    }

    #[test]
    fn panel_title_shows_the_agent_filter_and_query_when_searching() {
        assert_eq!(
            sessions_panel_title(Some("Claude"), "fix"),
            "Sessions · Claude / fix"
        );
    }

    #[test]
    fn filters_sessions_by_title_and_id() {
        let project = Project {
            cwd: PathBuf::from("/work/repo"),
            sessions: vec![
                Session {
                    key: SessionKey::claude("abc"),
                    title: "First Session".into(),
                    cwd: PathBuf::from("/work/repo"),
                    git_branch: None,
                    created_at: None,
                    updated_at: None,
                    source_location: Some("/tmp/abc.jsonl".into()),
                    message_count: Some(1),
                    usage: None,
                    model: None,
                    estimated_cost: None,
                    capabilities: SessionCapabilities::default(),
                },
                Session {
                    key: SessionKey::claude("xyz"),
                    title: "Second".into(),
                    cwd: PathBuf::from("/work/repo"),
                    git_branch: None,
                    created_at: None,
                    updated_at: None,
                    source_location: Some("/tmp/xyz.jsonl".into()),
                    message_count: Some(2),
                    usage: None,
                    model: None,
                    estimated_cost: None,
                    capabilities: SessionCapabilities::default(),
                },
            ],
        };

        let by_title = filter_project_sessions(Some(&project), "first");
        assert_eq!(by_title.len(), 1);
        assert_eq!(by_title[0].native_id(), "abc");

        let by_id = filter_project_sessions(Some(&project), "xyz");
        assert_eq!(by_id.len(), 1);
        assert_eq!(by_id[0].title, "Second");
    }
}
