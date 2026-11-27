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

/// Scope loaded projects to one agent: keep only that agent's sessions and
/// drop directories left without any. The synthetic `Open Here` row is added
/// by the UI and is never part of this list, so it survives every agent.
pub fn projects_for_agent(mut projects: Vec<Project>, agent_id: &AgentId) -> Vec<Project> {
    for project in &mut projects {
        project
            .sessions
            .retain(|session| session.key.agent_id == *agent_id);
    }
    projects.retain(|project| !project.sessions.is_empty());
    projects
}

/// Title of the sessions pane. The agent tab strip is the single source of
/// truth for the active agent, so the title carries only the search query.
pub fn sessions_panel_title(query: &str) -> String {
    if query.is_empty() {
        "Sessions".to_string()
    } else {
        format!("Sessions / {query}")
    }
}

#[cfg(test)]
mod tests {
    use super::{filter_project_sessions, sessions_panel_title};
    use crate::domain::{Project, Session, SessionCapabilities, SessionKey};
    use std::path::PathBuf;

    #[test]
    fn panel_title_is_plain_sessions_without_a_query() {
        assert_eq!(sessions_panel_title(""), "Sessions");
    }

    #[test]
    fn panel_title_shows_the_query() {
        assert_eq!(sessions_panel_title("fix"), "Sessions / fix");
    }

    #[test]
    fn projects_for_agent_scopes_sessions_and_drops_empty_projects() {
        use super::projects_for_agent;
        use crate::domain::AgentId;

        let claude = AgentId::new("claude").unwrap();
        let codex = AgentId::new("codex").unwrap();
        let make_session = |key: SessionKey, cwd: &str| Session {
            key,
            title: "t".into(),
            cwd: PathBuf::from(cwd),
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
        };
        let projects = vec![
            Project {
                cwd: PathBuf::from("/work/mixed"),
                sessions: vec![
                    make_session(SessionKey::claude("c1"), "/work/mixed"),
                    make_session(SessionKey::new(codex.clone(), "x1").unwrap(), "/work/mixed"),
                ],
            },
            Project {
                cwd: PathBuf::from("/work/codex-only"),
                sessions: vec![make_session(
                    SessionKey::new(codex, "x2").unwrap(),
                    "/work/codex-only",
                )],
            },
            Project {
                cwd: PathBuf::from("/work/empty"),
                sessions: vec![],
            },
        ];

        let scoped = projects_for_agent(projects, &claude);

        assert_eq!(scoped.len(), 1);
        assert_eq!(scoped[0].cwd, PathBuf::from("/work/mixed"));
        assert_eq!(scoped[0].sessions.len(), 1);
        assert_eq!(scoped[0].sessions[0].native_id(), "c1");
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
                    reported_cost: None,
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
                    reported_cost: None,
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
