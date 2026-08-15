use crate::domain::Project;

/// Summarizes token usage across all sessions in a project.
///
/// # States
///
/// | Scenario | `known_total` | `session_count` | `sessions_with_usage` |
/// |---|---|---|---|
/// | Empty project | `0` | `0` | `0` |
/// | All sessions have usage | sum of totals | N | N |
/// | Mixed known + unknown | sum of known | N | M (< N) |
/// | Sessions exist, none have usage | `0` | N | `0` |
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectUsageSummary {
    /// Sum of `SessionUsage::total_tokens()` across every session that has usage data.
    pub known_total: u64,
    /// Total number of sessions in the project.
    pub session_count: usize,
    /// Number of sessions that have `Some(usage)`.
    pub sessions_with_usage: usize,
}

impl ProjectUsageSummary {
    /// Whether every session in the project has usage data.
    /// An empty project is considered complete (no sessions = nothing missing).
    pub fn is_complete(&self) -> bool {
        self.sessions_with_usage == self.session_count
    }

    /// Whether any session has usage data (partial or complete).
    pub fn has_any_usage(&self) -> bool {
        self.sessions_with_usage > 0
    }
}

/// Compute the usage summary for a project from its sessions.
///
/// Each session's `usage` field is inspected:
/// - `Some(usage)` contributes its `total_tokens()` to `known_total`.
/// - `None` is treated as missing data and does not contribute.
pub fn summarize_project_usage(project: &Project) -> ProjectUsageSummary {
    let session_count = project.sessions.len();
    let mut known_total: u64 = 0;
    let mut sessions_with_usage: usize = 0;

    for session in &project.sessions {
        if let Some(usage) = &session.usage {
            known_total = known_total.saturating_add(usage.total_tokens());
            sessions_with_usage += 1;
        }
    }

    ProjectUsageSummary {
        known_total,
        session_count,
        sessions_with_usage,
    }
}

/// Build the display label for a project row in the TUI.
///
/// Formats the project name, session count, and token usage summary according to
/// the rules:
/// - Empty project: `name (0) 0`
/// - Complete usage: `name (count) 12.3K`
/// - Partial usage: `name (count) 12.3K+`
/// - All unknown: `name (count) Unknown`
pub fn build_project_row_label(name: &str, summary: &ProjectUsageSummary) -> String {
    let base = format!("{} ({})", name, summary.session_count);

    let usage_suffix = if summary.session_count == 0 {
        "0".to_string()
    } else if !summary.has_any_usage() {
        "Unknown".to_string()
    } else {
        let formatted = format_token_count(summary.known_total);
        if summary.is_complete() {
            formatted
        } else {
            format!("{}+", formatted)
        }
    };

    format!("{} {}", base, usage_suffix)
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
    use crate::domain::{Project, Session, SessionCapabilities, SessionKey, SessionUsage};
    use std::path::PathBuf;

    // -----------------------------------------------------------------------
    // summarize_project_usage tests
    // -----------------------------------------------------------------------

    fn make_session(id: &str, usage: Option<SessionUsage>) -> Session {
        Session {
            key: SessionKey::claude(id),
            title: format!("Session {id}"),
            cwd: PathBuf::from("/work/repo"),
            git_branch: None,
            created_at: None,
            updated_at: None,
            source_location: Some(format!("/tmp/{id}.jsonl")),
            message_count: Some(0),
            usage,
            model: None,
            estimated_cost: None,
            capabilities: SessionCapabilities::default(),
        }
    }

    fn make_usage(input: u64, output: u64, cache_create: u64, cache_read: u64) -> SessionUsage {
        SessionUsage {
            input_tokens: input,
            output_tokens: output,
            cache_creation_tokens: cache_create,
            cache_read_tokens: cache_read,
        }
    }

    #[test]
    fn empty_project_returns_complete_zero() {
        let project = Project {
            cwd: PathBuf::from("/work/empty"),
            sessions: vec![],
        };

        let summary = summarize_project_usage(&project);

        assert_eq!(summary.known_total, 0);
        assert_eq!(summary.session_count, 0);
        assert_eq!(summary.sessions_with_usage, 0);
        assert!(summary.is_complete());
        assert!(!summary.has_any_usage());
    }

    #[test]
    fn single_session_with_usage_returns_its_total() {
        let usage = make_usage(1000, 500, 200, 100); // total = 1800
        let project = Project {
            cwd: PathBuf::from("/work/repo"),
            sessions: vec![make_session("s1", Some(usage))],
        };

        let summary = summarize_project_usage(&project);

        assert_eq!(summary.known_total, 1800);
        assert_eq!(summary.session_count, 1);
        assert_eq!(summary.sessions_with_usage, 1);
        assert!(summary.is_complete());
        assert!(summary.has_any_usage());
    }

    #[test]
    fn multiple_complete_sessions_are_summed() {
        let usage1 = make_usage(100, 50, 25, 25); // total = 200
        let usage2 = make_usage(300, 150, 75, 75); // total = 600
        let project = Project {
            cwd: PathBuf::from("/work/repo"),
            sessions: vec![
                make_session("s1", Some(usage1)),
                make_session("s2", Some(usage2)),
            ],
        };

        let summary = summarize_project_usage(&project);

        assert_eq!(summary.known_total, 800); // 200 + 600
        assert_eq!(summary.session_count, 2);
        assert_eq!(summary.sessions_with_usage, 2);
        assert!(summary.is_complete());
    }

    #[test]
    fn mixed_known_and_missing_usage_returns_partial() {
        let usage = make_usage(1000, 500, 200, 100); // total = 1800
        let project = Project {
            cwd: PathBuf::from("/work/repo"),
            sessions: vec![
                make_session("s1", Some(usage)),
                make_session("s2", None), // missing usage
            ],
        };

        let summary = summarize_project_usage(&project);

        assert_eq!(summary.known_total, 1800);
        assert_eq!(summary.session_count, 2);
        assert_eq!(summary.sessions_with_usage, 1);
        assert!(!summary.is_complete());
        assert!(summary.has_any_usage());
    }

    #[test]
    fn all_sessions_missing_usage_returns_unknown() {
        let project = Project {
            cwd: PathBuf::from("/work/repo"),
            sessions: vec![make_session("s1", None), make_session("s2", None)],
        };

        let summary = summarize_project_usage(&project);

        assert_eq!(summary.known_total, 0);
        assert_eq!(summary.session_count, 2);
        assert_eq!(summary.sessions_with_usage, 0);
        assert!(!summary.is_complete());
        assert!(!summary.has_any_usage());
    }

    #[test]
    fn cache_creation_and_cache_read_are_included_in_total() {
        // Use deliberately different values so omitting any field fails the test.
        let usage = make_usage(10, 20, 30, 40); // total = 100
        let project = Project {
            cwd: PathBuf::from("/work/repo"),
            sessions: vec![make_session("s1", Some(usage))],
        };

        let summary = summarize_project_usage(&project);

        // If cache_creation or cache_read were excluded, total would be 30 instead of 100.
        assert_eq!(summary.known_total, 100);
    }

    #[test]
    fn explicit_zero_counters_is_known_not_unknown() {
        let usage = make_usage(0, 0, 0, 0); // total = 0, but known
        let project = Project {
            cwd: PathBuf::from("/work/repo"),
            sessions: vec![make_session("s1", Some(usage))],
        };

        let summary = summarize_project_usage(&project);

        assert_eq!(summary.known_total, 0);
        assert_eq!(summary.sessions_with_usage, 1);
        assert!(summary.is_complete());
        assert!(summary.has_any_usage());
    }

    // -----------------------------------------------------------------------
    // build_project_row_label tests
    // -----------------------------------------------------------------------

    #[test]
    fn label_complete_usage_formats_correctly() {
        let summary = ProjectUsageSummary {
            known_total: 12_345,
            session_count: 8,
            sessions_with_usage: 8,
        };

        let label = build_project_row_label("cowboy", &summary);

        assert_eq!(label, "cowboy (8) 12.3K");
    }

    #[test]
    fn label_partial_usage_appends_plus() {
        let summary = ProjectUsageSummary {
            known_total: 12_345,
            session_count: 10,
            sessions_with_usage: 8,
        };

        let label = build_project_row_label("cowboy", &summary);

        assert_eq!(label, "cowboy (10) 12.3K+");
    }

    #[test]
    fn label_all_unknown_shows_unknown() {
        let summary = ProjectUsageSummary {
            known_total: 0,
            session_count: 5,
            sessions_with_usage: 0,
        };

        let label = build_project_row_label("cowboy", &summary);

        assert_eq!(label, "cowboy (5) Unknown");
    }

    #[test]
    fn label_empty_project_shows_zero() {
        let summary = ProjectUsageSummary {
            known_total: 0,
            session_count: 0,
            sessions_with_usage: 0,
        };

        let label = build_project_row_label("cowboy", &summary);

        assert_eq!(label, "cowboy (0) 0");
    }

    #[test]
    fn label_raw_small_value_not_formatted() {
        let summary = ProjectUsageSummary {
            known_total: 42,
            session_count: 1,
            sessions_with_usage: 1,
        };

        let label = build_project_row_label("tiny", &summary);

        assert_eq!(label, "tiny (1) 42");
    }

    #[test]
    fn label_million_value_uses_m_suffix() {
        let summary = ProjectUsageSummary {
            known_total: 2_500_000,
            session_count: 3,
            sessions_with_usage: 3,
        };

        let label = build_project_row_label("big", &summary);

        assert_eq!(label, "big (3) 2.5M");
    }

    #[test]
    fn label_session_count_unchanged() {
        let summary = ProjectUsageSummary {
            known_total: 1000,
            session_count: 7,
            sessions_with_usage: 7,
        };

        let label = build_project_row_label("test", &summary);

        // The session count " (7)" must still appear
        assert!(label.contains("(7)"));
    }

    #[test]
    fn label_formatter_rounds_correctly() {
        let summary = ProjectUsageSummary {
            known_total: 12_349, // should round to 12.3K
            session_count: 1,
            sessions_with_usage: 1,
        };

        let label = build_project_row_label("proj", &summary);

        assert_eq!(label, "proj (1) 12.3K");
    }
}
