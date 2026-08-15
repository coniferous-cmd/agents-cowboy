use std::error::Error as StdError;
use std::fmt;
use std::path::PathBuf;

/// A configured agent-backend instance identifier.
///
/// This intentionally is not an enum: users may configure more than one
/// instance of the same backend.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct AgentId(String);

impl AgentId {
    pub fn new(value: impl Into<String>) -> std::result::Result<Self, &'static str> {
        let value = value.into();
        if value.trim().is_empty() {
            return Err("agent id must not be empty");
        }
        Ok(Self(value))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for AgentId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct SessionKey {
    pub agent_id: AgentId,
    pub native_id: String,
}

impl SessionKey {
    pub fn new(
        agent_id: AgentId,
        native_id: impl Into<String>,
    ) -> std::result::Result<Self, &'static str> {
        let native_id = native_id.into();
        if native_id.trim().is_empty() {
            return Err("native session id must not be empty");
        }
        Ok(Self {
            agent_id,
            native_id,
        })
    }

    pub fn claude(native_id: impl Into<String>) -> Self {
        Self::new(
            AgentId::new("claude").expect("built-in agent id is valid"),
            native_id,
        )
        .expect("Claude session id is valid")
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct BackendCapabilities {
    pub list_sessions: bool,
    pub launch_new: bool,
    pub resume: bool,
    pub rename_session: bool,
    pub delete_session: bool,
    pub delete_project: bool,
    pub profiles: bool,
    pub usage: bool,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct SessionCapabilities {
    pub resume: bool,
    pub rename: bool,
    pub delete: bool,
    pub inspect_history: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionUsage {
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cache_creation_tokens: u64,
    pub cache_read_tokens: u64,
}

impl SessionUsage {
    pub fn total_tokens(&self) -> u64 {
        self.input_tokens + self.output_tokens + self.cache_creation_tokens + self.cache_read_tokens
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct CostEstimate {
    pub input_cost: f64,
    pub output_cost: f64,
    pub cache_creation_cost: f64,
    pub cache_read_cost: f64,
}

impl CostEstimate {
    pub fn total_cost(&self) -> f64 {
        self.input_cost + self.output_cost + self.cache_creation_cost + self.cache_read_cost
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Session {
    pub key: SessionKey,
    pub title: String,
    pub cwd: PathBuf,
    pub source_location: Option<String>,
    pub git_branch: Option<String>,
    pub created_at: Option<String>,
    pub updated_at: Option<String>,
    pub message_count: Option<usize>,
    pub usage: Option<SessionUsage>,
    pub model: Option<String>,
    pub estimated_cost: Option<CostEstimate>,
    pub capabilities: SessionCapabilities,
}

impl Session {
    pub fn native_id(&self) -> &str {
        &self.key.native_id
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Project {
    pub cwd: PathBuf,
    pub sessions: Vec<Session>,
}

impl Project {
    pub fn name(&self) -> String {
        self.cwd
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .filter(|name| !name.is_empty())
            .unwrap_or_else(|| self.cwd.display().to_string())
    }
}

impl Project {
    /// Return the project display name for a single project.
    ///
    /// This is the same as `Project::name()` — the one-segment basename —
    /// and is provided as a convenient alias so callers can uniformly use
    /// `project_display_name(project)` for both single and batch contexts.
    pub fn display_name(&self) -> String {
        self.name()
    }
}

/// Compute display names for a slice of projects, in input order.
///
/// Projects whose directory basename is unique among the slice keep just that
/// name.  Projects that share a basename with one or more others are
/// formatted as `parent/project` (at most two path segments), where `parent` is
/// the direct parent directory name.  Names are never expanded beyond two
/// segments regardless of how many projects share the same `parent/project`.
pub fn project_display_names(projects: &[Project]) -> Vec<String> {
    // Phase 1 — count how many times each basename occurs.
    use std::collections::HashMap;
    let mut counts: HashMap<String, usize> = HashMap::new();
    for p in projects {
        *counts.entry(p.name()).or_insert(0) += 1;
    }

    // Phase 2 — produce one name per project.
    projects
        .iter()
        .map(|p| {
            if counts.get(&p.name()).copied().unwrap_or(1) > 1 {
                // Expand to at most two segments: parent/name.
                let name = p.name();
                let parent = p
                    .cwd
                    .parent()
                    .and_then(|pp| pp.file_name().map(|n| n.to_string_lossy().into_owned()))
                    .unwrap_or_else(|| name.clone());
                format!("{}/{}", parent, name)
            } else {
                p.name()
            }
        })
        .collect()
}

pub fn group_sessions_by_project(mut sessions: Vec<Session>) -> Vec<Project> {
    sessions.sort_by(|left, right| {
        left.cwd
            .cmp(&right.cwd)
            .then_with(|| session_time(right).cmp(&session_time(left)))
            .then_with(|| left.title.cmp(&right.title))
            .then_with(|| left.key.cmp(&right.key))
    });

    let mut projects: Vec<Project> = Vec::new();
    for session in sessions {
        match projects.last_mut() {
            Some(project) if project.cwd == session.cwd => project.sessions.push(session),
            _ => projects.push(Project {
                cwd: session.cwd.clone(),
                sessions: vec![session],
            }),
        }
    }

    projects
}

fn session_time(session: &Session) -> Option<&String> {
    session.updated_at.as_ref().or(session.created_at.as_ref())
}

#[derive(Debug)]
pub enum StetsonError {
    Io(std::io::Error),
    Json(serde_json::Error),
    Sqlite(rusqlite::Error),
    InvalidSessionFile(String),
    InvalidSettingsFile(String),
    InvalidProfileName(String),
    ProfileExists(String),
    ProfileNotFound(String),
    SnapshotNotFound(i64),
    MigrationFailed(String),
    ActivationRecoveryFailed(String),
    UnsupportedSchemaVersion(i64),
    SessionNotFound(String),
    ProjectNotFound(String),
    ProjectSettingsNotFound(String),
    ThemeNotFound(String),
    HomeDirectoryUnavailable,
}

impl fmt::Display for StetsonError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(error) => write!(f, "I/O error: {error}"),
            Self::Json(error) => write!(f, "JSON error: {error}"),
            Self::Sqlite(error) => write!(f, "sqlite error: {error}"),
            Self::InvalidSessionFile(message) => write!(f, "invalid session file: {message}"),
            Self::InvalidSettingsFile(message) => write!(f, "invalid settings file: {message}"),
            Self::InvalidProfileName(message) => write!(f, "invalid profile name: {message}"),
            Self::ProfileExists(name) => write!(f, "profile already exists: {name}"),
            Self::ProfileNotFound(name) => write!(f, "profile not found: {name}"),
            Self::SnapshotNotFound(id) => write!(f, "settings snapshot not found: {id}"),
            Self::MigrationFailed(message) => write!(f, "metadata migration failed: {message}"),
            Self::ActivationRecoveryFailed(message) => {
                write!(f, "profile activation recovery failed: {message}")
            }
            Self::UnsupportedSchemaVersion(version) => {
                write!(f, "unsupported metadata schema version: {version}")
            }
            Self::SessionNotFound(id) => write!(f, "session not found: {id}"),
            Self::ProjectNotFound(path) => write!(f, "project not found: {path}"),
            Self::ProjectSettingsNotFound(path) => {
                write!(f, "project settings not found: {path}")
            }
            Self::ThemeNotFound(name) => write!(f, "theme not found: {name}"),
            Self::HomeDirectoryUnavailable => write!(f, "home directory is unavailable"),
        }
    }
}

impl StdError for StetsonError {
    fn source(&self) -> Option<&(dyn StdError + 'static)> {
        match self {
            Self::Io(error) => Some(error),
            Self::Json(error) => Some(error),
            Self::Sqlite(error) => Some(error),
            Self::InvalidSessionFile(_) => None,
            Self::InvalidSettingsFile(_) => None,
            Self::InvalidProfileName(_) => None,
            Self::ProfileExists(_) => None,
            Self::ProfileNotFound(_) => None,
            Self::SnapshotNotFound(_) => None,
            Self::MigrationFailed(_) => None,
            Self::ActivationRecoveryFailed(_) => None,
            Self::UnsupportedSchemaVersion(_) => None,
            Self::SessionNotFound(_) => None,
            Self::ProjectNotFound(_) => None,
            Self::ProjectSettingsNotFound(_) => None,
            Self::ThemeNotFound(_) => None,
            Self::HomeDirectoryUnavailable => None,
        }
    }
}

impl From<std::io::Error> for StetsonError {
    fn from(value: std::io::Error) -> Self {
        Self::Io(value)
    }
}

impl From<serde_json::Error> for StetsonError {
    fn from(value: serde_json::Error) -> Self {
        Self::Json(value)
    }
}

impl From<rusqlite::Error> for StetsonError {
    fn from(value: rusqlite::Error) -> Self {
        Self::Sqlite(value)
    }
}

pub type Result<T> = std::result::Result<T, StetsonError>;

/// Validate a session rename title input.
pub fn validate_rename_title(input: &str) -> std::result::Result<String, &'static str> {
    let title = input.trim();
    if title.is_empty() {
        return Err("Rename requires a non-empty title");
    }
    Ok(title.to_string())
}

#[cfg(test)]
mod tests {
    use super::{
        group_sessions_by_project, project_display_names, validate_rename_title, AgentId, Project,
        Session, SessionCapabilities, SessionKey, SessionUsage,
    };
    use std::path::PathBuf;

    #[test]
    fn groups_sessions_by_cwd() {
        let sessions = vec![
            Session {
                key: SessionKey::claude("b"),
                title: "Second".into(),
                cwd: PathBuf::from("/work/repo-a"),
                git_branch: None,
                created_at: None,
                updated_at: Some("2025-01-02T00:00:00Z".into()),
                source_location: Some("/tmp/b.jsonl".into()),
                message_count: Some(2),
                usage: None,
                model: None,
                estimated_cost: None,
                capabilities: SessionCapabilities::default(),
            },
            Session {
                key: SessionKey::claude("a"),
                title: "First".into(),
                cwd: PathBuf::from("/work/repo-a"),
                git_branch: None,
                created_at: None,
                updated_at: Some("2025-01-03T00:00:00Z".into()),
                source_location: Some("/tmp/a.jsonl".into()),
                message_count: Some(1),
                usage: None,
                model: None,
                estimated_cost: None,
                capabilities: SessionCapabilities::default(),
            },
            Session {
                key: SessionKey::claude("c"),
                title: "Third".into(),
                cwd: PathBuf::from("/work/repo-b"),
                git_branch: None,
                created_at: None,
                updated_at: None,
                source_location: Some("/tmp/c.jsonl".into()),
                message_count: Some(3),
                usage: None,
                model: None,
                estimated_cost: None,
                capabilities: SessionCapabilities::default(),
            },
        ];

        let projects = group_sessions_by_project(sessions);
        assert_eq!(projects.len(), 2);
        assert_eq!(projects[0].cwd, PathBuf::from("/work/repo-a"));
        assert_eq!(projects[0].sessions.len(), 2);
        assert_eq!(projects[0].sessions[0].native_id(), "a");
        assert_eq!(projects[1].cwd, PathBuf::from("/work/repo-b"));
    }

    #[test]
    fn sessions_with_same_native_id_from_different_agents_coexist() {
        let claude = Session {
            key: SessionKey::new(AgentId::new("claude").unwrap(), "shared-id").unwrap(),
            title: "Claude session".into(),
            cwd: PathBuf::from("/work/repo"),
            source_location: None,
            git_branch: None,
            created_at: None,
            updated_at: None,
            message_count: Some(1),
            usage: None,
            model: None,
            estimated_cost: None,
            capabilities: SessionCapabilities::default(),
        };
        let codex = Session {
            key: SessionKey::new(AgentId::new("codex").unwrap(), "shared-id").unwrap(),
            title: "Codex session".into(),
            ..claude.clone()
        };

        let projects = group_sessions_by_project(vec![claude, codex]);

        assert_eq!(projects.len(), 1);
        assert_eq!(projects[0].sessions.len(), 2);
        assert_ne!(projects[0].sessions[0].key, projects[0].sessions[1].key);
    }

    #[test]
    fn sorts_sessions_by_time_descending_with_created_at_fallback() {
        let sessions = vec![
            Session {
                key: SessionKey::claude("older"),
                title: "Older".into(),
                cwd: PathBuf::from("/work/repo"),
                git_branch: None,
                created_at: Some("2025-01-01T00:00:00Z".into()),
                updated_at: None,
                source_location: Some("/tmp/older.jsonl".into()),
                message_count: Some(1),
                usage: None,
                model: None,
                estimated_cost: None,
                capabilities: SessionCapabilities::default(),
            },
            Session {
                key: SessionKey::claude("newer"),
                title: "Newer".into(),
                cwd: PathBuf::from("/work/repo"),
                git_branch: None,
                created_at: Some("2025-01-03T00:00:00Z".into()),
                updated_at: None,
                source_location: Some("/tmp/newer.jsonl".into()),
                message_count: Some(1),
                usage: None,
                model: None,
                estimated_cost: None,
                capabilities: SessionCapabilities::default(),
            },
            Session {
                key: SessionKey::claude("updated"),
                title: "Updated".into(),
                cwd: PathBuf::from("/work/repo"),
                git_branch: None,
                created_at: Some("2025-01-02T00:00:00Z".into()),
                updated_at: Some("2025-01-04T00:00:00Z".into()),
                source_location: Some("/tmp/updated.jsonl".into()),
                message_count: Some(1),
                usage: None,
                model: None,
                estimated_cost: None,
                capabilities: SessionCapabilities::default(),
            },
        ];

        let projects = group_sessions_by_project(sessions);
        let ids = projects[0]
            .sessions
            .iter()
            .map(Session::native_id)
            .collect::<Vec<_>>();

        assert_eq!(ids, vec!["updated", "newer", "older"]);
    }

    #[test]
    fn session_usage_total_includes_all_four_categories() {
        let usage = SessionUsage {
            input_tokens: 10,
            output_tokens: 20,
            cache_creation_tokens: 30,
            cache_read_tokens: 40,
        };
        // If any field is excluded, the total would be different.
        assert_eq!(usage.total_tokens(), 100);
    }

    #[test]
    fn session_usage_zero_counters_still_known() {
        let usage = SessionUsage {
            input_tokens: 0,
            output_tokens: 0,
            cache_creation_tokens: 0,
            cache_read_tokens: 0,
        };
        assert_eq!(usage.total_tokens(), 0);
    }

    // -----------------------------------------------------------------------
    // project_display_name tests — disambiguating duplicate basenames
    // -----------------------------------------------------------------------

    /// Helper: make a minimal Project with the given cwd.
    fn proj(cwd: &str) -> Project {
        Project {
            cwd: PathBuf::from(cwd),
            sessions: Vec::new(),
        }
    }

    #[test]
    fn single_project_returns_one_segment_name() {
        // A lone project with a unique basename keeps its simple name.
        let projects = vec![proj("/work/api")];
        let names = project_display_names(&projects);
        assert_eq!(names, vec!["api"]);
    }

    #[test]
    fn several_differently_named_projects_all_one_segment() {
        // No duplicates → every project shows only its basename.
        let projects = vec![proj("/work/api"), proj("/work/web"), proj("/work/cli")];
        let names = project_display_names(&projects);
        assert_eq!(names, vec!["api", "web", "cli"]);
    }

    #[test]
    fn two_projects_same_name_expanded_to_parent_project() {
        // Duplicate basenames are disambiguated with their direct parent.
        let projects = vec![proj("/team-a/api"), proj("/team-b/api")];
        let names = project_display_names(&projects);
        assert_eq!(names, vec!["team-a/api", "team-b/api"]);
    }

    #[test]
    fn three_projects_same_name_all_expanded() {
        // Every member of a duplicate group gets expanded.
        let projects = vec![
            proj("/team-a/api"),
            proj("/team-b/api"),
            proj("/team-c/api"),
        ];
        let names = project_display_names(&projects);
        assert_eq!(names, vec!["team-a/api", "team-b/api", "team-c/api"]);
    }

    #[test]
    fn unrelated_unique_project_stays_one_segment() {
        // The colliding group (all sharing "api") is expanded.
        // /work/web is unrelated and stays one-segment.
        let projects = vec![
            proj("/work/api"),
            proj("/team-a/api"),
            proj("/team-b/api"),
            proj("/work/web"),
        ];
        let names = project_display_names(&projects);
        // api collides → all three expanded; web is unique → stays simple
        assert_eq!(names, vec!["work/api", "team-a/api", "team-b/api", "web"]);
    }

    #[test]
    fn repeated_parent_project_never_grows_to_three_segments() {
        // When parent/project also collides we do NOT add a third ancestor.
        // The two-segment cap is hard — identical rows are preferable to a
        // full-path or third-segment escape hatch.
        let projects = vec![
            proj("/a/x/api"),
            proj("/a/y/api"),
            proj("/b/x/api"),
            proj("/b/y/api"),
        ];
        let names = project_display_names(&projects);
        // All still two-segment; the collision is not fully resolved but the
        // rule is respected.
        assert!(names.iter().all(|n| n.matches('/').count() == 1));
    }

    #[test]
    fn root_like_path_uses_fallback_not_panic() {
        // A path with no meaningful parent (e.g. a bare root or bare filename)
        // must not panic and should use the existing safe fallback.
        let projects = vec![proj("api"), proj("/api")];
        let names = project_display_names(&projects);
        // Both names should be non-empty strings (fallback behavior).
        assert_eq!(names.len(), 2);
        assert!(!names[0].is_empty());
        assert!(!names[1].is_empty());
    }

    #[test]
    fn validate_rename_title_rejects_empty() {
        let error = validate_rename_title("   ").unwrap_err();
        assert_eq!(error, "Rename requires a non-empty title");
    }

    #[test]
    fn validate_rename_title_accepts_valid_input() {
        let result = validate_rename_title("  My Session  ").unwrap();
        assert_eq!(result, "My Session");
    }
}
