use std::path::PathBuf;

mod mutation;
mod parser;
mod project_paths;
mod store;
pub(super) mod timestamps;

#[derive(Debug, Clone)]
pub struct ClaudeProjectsStore {
    root: PathBuf,
}

// Keep tests in this file to avoid spreading test modules across submodules
// during the initial extraction. They can be split later if desired.
#[cfg(test)]
mod tests {
    use super::ClaudeProjectsStore;
    use crate::domain::{group_sessions_by_project, Session, SessionCapabilities, SessionKey};
    use std::fs;
    use std::path::{Path, PathBuf};
    use tempfile::tempdir;

    #[test]
    fn parses_session_fields_and_ignores_nested_subagents() {
        let temp = tempdir().unwrap();
        let projects_root = temp.path().join(".claude/projects");
        let project_dir = projects_root.join("-tmp-demo-repo");
        let subagents_dir = project_dir.join("subagents");
        fs::create_dir_all(&subagents_dir).unwrap();

        let session_path = project_dir.join("session-1.jsonl");
        fs::write(
            &session_path,
            [
                r#"{"type":"meta","sessionId":"session-1","cwd":"/tmp/demo/repo","gitBranch":"main","timestamp":"2025-07-05T08:00:00Z"}"#,
                r#"{"type":"agent-name","agentName":"Fallback Title"}"#,
                r#"{"type":"custom-title","customTitle":"Preferred Title"}"#,
                r#"{"role":"user","message":{"content":"hello"}}"#,
                r#"{"role":"assistant","message":{"content":"world"}}"#,
            ]
            .join("\n"),
        )
        .unwrap();
        fs::write(
            subagents_dir.join("nested.jsonl"),
            r#"{"sessionId":"nested","cwd":"/tmp/ignored"}"#,
        )
        .unwrap();

        let store = ClaudeProjectsStore::new(&projects_root);
        let sessions = store.discover_sessions().unwrap();

        assert_eq!(sessions.len(), 1);
        let session = &sessions[0];
        assert_eq!(session.native_id(), "session-1");
        assert_eq!(session.title, "Preferred Title");
        assert_eq!(session.cwd, Path::new("/tmp/demo/repo"));
        assert_eq!(session.git_branch.as_deref(), Some("main"));
        assert_eq!(session.created_at.as_deref(), Some("2025-07-05T08:00:00Z"));
        assert_eq!(session.updated_at.as_deref(), Some("2025-07-05T08:00:00Z"));
        assert_eq!(session.message_count, Some(2));
        assert_eq!(
            session.source_location.as_deref(),
            Some(session_path.to_str().unwrap())
        );
        assert!(session.usage.is_none());
        assert!(session.model.is_none());
        assert!(session.estimated_cost.is_none());
    }

    #[test]
    fn falls_back_to_project_dir_when_cwd_missing() {
        let temp = tempdir().unwrap();
        let projects_root = temp.path().join(".claude/projects");
        let project_dir = projects_root.join("-Users-test-demo");
        fs::create_dir_all(&project_dir).unwrap();
        let session_path = project_dir.join("session-2.jsonl");

        fs::write(
            &session_path,
            r#"{"type":"meta","sessionId":"session-2","agentName":"Agent Title"}"#,
        )
        .unwrap();

        let store = ClaudeProjectsStore::new(&projects_root);
        let session = store.discover_sessions().unwrap().pop().unwrap();
        assert_eq!(session.cwd, Path::new("/Users/test/demo"));
        assert_eq!(session.title, "Agent Title");
    }

    #[test]
    fn from_home_uses_default_claude_projects_directory() {
        let store = ClaudeProjectsStore::from_home().unwrap();
        assert!(store
            .root()
            .ends_with(Path::new(".claude").join("projects")));
    }

    #[test]
    fn rename_updates_existing_title_record() {
        let temp = tempdir().unwrap();
        let projects_root = temp.path().join(".claude/projects");
        let project_dir = projects_root.join("repo-a");
        fs::create_dir_all(&project_dir).unwrap();
        let session_path = project_dir.join("session-3.jsonl");

        fs::write(
            &session_path,
            [
                r#"{"type":"meta","sessionId":"session-3","cwd":"/repo-a"}"#,
                r#"{"type":"custom-title","customTitle":"Old Title"}"#,
            ]
            .join("\n"),
        )
        .unwrap();

        let store = ClaudeProjectsStore::new(&projects_root);
        let session = store.rename_session(&session_path, "New Title").unwrap();
        let updated_contents = fs::read_to_string(&session_path).unwrap();

        assert_eq!(session.title, "New Title");
        assert!(updated_contents.contains(r#""customTitle":"New Title""#));
        assert!(!updated_contents.contains("Old Title"));
    }

    #[test]
    fn delete_removes_session_and_companion_directories() {
        let temp = tempdir().unwrap();
        let claude_root = temp.path().join(".claude");
        let projects_root = claude_root.join("projects");
        let project_dir = projects_root.join("repo-b");
        fs::create_dir_all(&project_dir).unwrap();

        let session_id = "session-4";
        let session_path = project_dir.join(format!("{session_id}.jsonl"));
        let sidecar_dir = project_dir.join(session_id);
        let session_env = claude_root.join("session-env").join(session_id);
        let file_history = claude_root.join("file-history").join(session_id);
        let tasks = claude_root.join("tasks").join(session_id);

        fs::write(
            &session_path,
            r#"{"type":"meta","sessionId":"session-4","cwd":"/repo-b"}"#,
        )
        .unwrap();
        fs::create_dir_all(&sidecar_dir).unwrap();
        fs::create_dir_all(&session_env).unwrap();
        fs::create_dir_all(&file_history).unwrap();
        fs::create_dir_all(&tasks).unwrap();

        let store = ClaudeProjectsStore::new(&projects_root);
        store.delete_session(session_id, &session_path).unwrap();

        assert!(!session_path.exists());
        assert!(!sidecar_dir.exists());
        assert!(!session_env.exists());
        assert!(!file_history.exists());
        assert!(!tasks.exists());
    }

    #[test]
    fn delete_project_removes_project_directory_and_all_sessions() {
        let temp = tempdir().unwrap();
        let claude_root = temp.path().join(".claude");
        let projects_root = claude_root.join("projects");
        let project_dir = projects_root.join("-repo-c");
        fs::create_dir_all(&project_dir).unwrap();

        for session_id in ["session-5", "session-6"] {
            let session_path = project_dir.join(format!("{session_id}.jsonl"));
            fs::write(
                &session_path,
                format!(r#"{{"type":"meta","sessionId":"{session_id}","cwd":"/repo/c"}}"#),
            )
            .unwrap();
            fs::create_dir_all(project_dir.join(session_id)).unwrap();
            fs::create_dir_all(claude_root.join("session-env").join(session_id)).unwrap();
            fs::create_dir_all(claude_root.join("file-history").join(session_id)).unwrap();
            fs::create_dir_all(claude_root.join("tasks").join(session_id)).unwrap();
        }

        let store = ClaudeProjectsStore::new(&projects_root);
        store.delete_project(Path::new("/repo/c")).unwrap();

        assert!(!project_dir.exists());
        assert!(!claude_root.join("session-env/session-5").exists());
        assert!(!claude_root.join("session-env/session-6").exists());
        assert!(!claude_root.join("file-history/session-5").exists());
        assert!(!claude_root.join("file-history/session-6").exists());
        assert!(!claude_root.join("tasks/session-5").exists());
        assert!(!claude_root.join("tasks/session-6").exists());
    }

    #[test]
    fn groups_projects_from_discovered_sessions() {
        let temp = tempdir().unwrap();
        let projects_root = temp.path().join(".claude/projects");
        let repo_a = projects_root.join("repo-a");
        let repo_b = projects_root.join("repo-b");
        fs::create_dir_all(&repo_a).unwrap();
        fs::create_dir_all(&repo_b).unwrap();

        fs::write(
            repo_a.join("one.jsonl"),
            [
                r#"{"type":"meta","sessionId":"one","cwd":"/repo-a","timestamp":"2025-01-01T00:00:00Z","updatedAt":"2025-01-01T01:00:00Z"}"#,
                r#"{"role":"user","message":{"content":"hi"}}"#,
            ]
            .join("\n"),
        )
        .unwrap();
        fs::write(
            repo_a.join("two.jsonl"),
            [
                r#"{"type":"meta","sessionId":"two","cwd":"/repo-a","timestamp":"2025-01-02T00:00:00Z","updatedAt":"2025-01-02T01:00:00Z"}"#,
                r#"{"role":"user","message":{"content":"hi"}}"#,
            ]
            .join("\n"),
        )
        .unwrap();
        fs::write(
            repo_b.join("three.jsonl"),
            r#"{"type":"meta","sessionId":"three","cwd":"/repo-b"}"#,
        )
        .unwrap();

        let store = ClaudeProjectsStore::new(&projects_root);
        let projects = group_sessions_by_project(store.discover_sessions().unwrap());

        assert_eq!(projects.len(), 2);
        assert_eq!(projects[0].sessions[0].native_id(), "two");
        assert_eq!(projects[0].sessions[1].native_id(), "one");
        assert_eq!(projects[1].sessions[0].native_id(), "three");
    }

    #[test]
    fn resolves_project_path_from_project_or_child_directory() {
        let temp = tempdir().unwrap();
        let projects_root = temp.path().join(".claude/projects");
        let project_dir = projects_root.join("-Users-test-demo");
        fs::create_dir_all(&project_dir).unwrap();

        let store = ClaudeProjectsStore::new(&projects_root);
        let project_path = PathBuf::from("/Users/test/demo");

        assert_eq!(
            store.resolve_project_path(&project_path).unwrap(),
            Some(project_path.clone())
        );
        assert_eq!(
            store
                .resolve_project_path(&project_path.join("subdir"))
                .unwrap(),
            Some(project_path)
        );
    }

    #[test]
    fn resolves_project_path_from_current_windows_project_directory_name_and_session_cwd() {
        let temp = tempdir().unwrap();
        let projects_root = temp.path().join(".claude/projects");
        let project_dir = projects_root
            .join("D--kingdom-workstation-xlkh-xlkh-tsdb-research-source-industrial-control-1");
        fs::create_dir_all(&project_dir).unwrap();

        let session_path = project_dir.join("session.jsonl");
        let project_path = PathBuf::from(
            r"D:\kingdom\workstation\xlkh\xlkh-tsdb-research\source\industrial-control_1",
        );
        fs::write(
            &session_path,
            serde_json::json!({
                "type": "meta",
                "sessionId": "session-7",
                "cwd": project_path.display().to_string(),
            })
            .to_string(),
        )
        .unwrap();

        let store = ClaudeProjectsStore::new(&projects_root);

        assert_eq!(
            store.resolve_project_path(&project_path).unwrap(),
            Some(project_path)
        );
    }

    #[test]
    fn delete_project_with_hyphen_in_directory_name() {
        let temp = tempdir().unwrap();
        let claude_root = temp.path().join(".claude");
        let projects_root = claude_root.join("projects");
        let project_dir = projects_root.join("-tmp-my-repo");
        fs::create_dir_all(&project_dir).unwrap();

        let session_path = project_dir.join("session.jsonl");
        fs::write(
            &session_path,
            r#"{"type":"meta","sessionId":"s1","cwd":"/tmp/my-repo"}"#,
        )
        .unwrap();

        let store = ClaudeProjectsStore::new(&projects_root);
        store.delete_project(Path::new("/tmp/my-repo")).unwrap();

        assert!(!project_dir.exists());
    }

    #[test]
    fn delete_project_nested_no_hyphen() {
        let temp = tempdir().unwrap();
        let claude_root = temp.path().join(".claude");
        let projects_root = claude_root.join("projects");
        let project_dir = projects_root.join("-tmp-notes-harness");
        fs::create_dir_all(&project_dir).unwrap();

        let session_path = project_dir.join("session.jsonl");
        fs::write(
            &session_path,
            r#"{"type":"meta","sessionId":"s2","cwd":"/tmp/notes/harness"}"#,
        )
        .unwrap();

        let store = ClaudeProjectsStore::new(&projects_root);
        store
            .delete_project(Path::new("/tmp/notes/harness"))
            .unwrap();

        assert!(!project_dir.exists());
    }

    #[test]
    fn parses_usage_from_assistant_message_records() {
        let temp = tempdir().unwrap();
        let projects_root = temp.path().join(".claude/projects");
        let project_dir = projects_root.join("-tmp-demo-repo");
        fs::create_dir_all(&project_dir).unwrap();

        let session_path = project_dir.join("session-usage.jsonl");
        fs::write(
            &session_path,
            [
                r#"{"type":"meta","sessionId":"session-usage","cwd":"/tmp/demo/repo","timestamp":"2025-07-05T08:00:00Z"}"#,
                r#"{"role":"user","message":{"content":"hello"}}"#,
                r#"{"role":"assistant","message":{"model":"claude-sonnet-4-20250514","content":[{"type":"text","text":"hi"}],"usage":{"input_tokens":1000,"output_tokens":500,"cache_creation_input_tokens":200,"cache_read_input_tokens":100}}}"#,
                r#"{"role":"assistant","message":{"model":"claude-sonnet-4-20250514","content":[{"type":"text","text":"there"}],"usage":{"input_tokens":1500,"output_tokens":800,"cache_creation_input_tokens":0,"cache_read_input_tokens":300}}}"#,
            ]
            .join("\n"),
        )
        .unwrap();

        let store = ClaudeProjectsStore::new(&projects_root);
        let sessions = store.discover_sessions().unwrap();
        assert_eq!(sessions.len(), 1);

        let session = &sessions[0];
        let usage = session.usage.as_ref().unwrap();
        assert_eq!(usage.input_tokens, 2500);
        assert_eq!(usage.output_tokens, 1300);
        assert_eq!(usage.cache_creation_tokens, 200);
        assert_eq!(usage.cache_read_tokens, 400);
        assert_eq!(session.model.as_deref(), Some("claude-sonnet-4-20250514"));
        assert!(session.estimated_cost.is_some());
    }

    #[test]
    fn session_without_usage_has_none_fields() {
        let temp = tempdir().unwrap();
        let projects_root = temp.path().join(".claude/projects");
        let project_dir = projects_root.join("-tmp-demo-repo");
        fs::create_dir_all(&project_dir).unwrap();

        let session_path = project_dir.join("session-no-usage.jsonl");
        fs::write(
            &session_path,
            [
                r#"{"type":"meta","sessionId":"session-no-usage","cwd":"/tmp/demo/repo"}"#,
                r#"{"role":"user","message":{"content":"hello"}}"#,
            ]
            .join("\n"),
        )
        .unwrap();

        let store = ClaudeProjectsStore::new(&projects_root);
        let sessions = store.discover_sessions().unwrap();
        let session = &sessions[0];
        assert!(session.usage.is_none());
        assert!(session.model.is_none());
        assert!(session.estimated_cost.is_none());
    }

    // ── Timestamp behaviour ──────────────────────────────────────────────

    #[test]
    fn earliest_ordinary_timestamp_is_created_at() {
        let temp = tempdir().unwrap();
        let projects_root = temp.path().join(".claude/projects");
        let project_dir = projects_root.join("-tmp-demo-repo");
        fs::create_dir_all(&project_dir).unwrap();
        let session_path = project_dir.join("ts-earliest.jsonl");
        fs::write(
            &session_path,
            [
                r#"{"type":"record","timestamp":"2025-07-05T10:00:00Z"}"#,
                r#"{"type":"record","timestamp":"2025-07-05T08:00:00Z"}"#,
            ]
            .join("\n"),
        )
        .unwrap();
        let store = ClaudeProjectsStore::new(&projects_root);
        let session = store.discover_sessions().unwrap().pop().unwrap();
        assert_eq!(session.created_at.as_deref(), Some("2025-07-05T08:00:00Z"));
    }

    #[test]
    fn latest_ordinary_timestamp_is_updated_at() {
        let temp = tempdir().unwrap();
        let projects_root = temp.path().join(".claude/projects");
        let project_dir = projects_root.join("-tmp-demo-repo");
        fs::create_dir_all(&project_dir).unwrap();
        let session_path = project_dir.join("ts-latest.jsonl");
        fs::write(
            &session_path,
            [
                r#"{"type":"record","timestamp":"2025-07-05T08:00:00Z"}"#,
                r#"{"type":"record","timestamp":"2025-07-05T10:00:00Z"}"#,
            ]
            .join("\n"),
        )
        .unwrap();
        let store = ClaudeProjectsStore::new(&projects_root);
        let session = store.discover_sessions().unwrap().pop().unwrap();
        assert_eq!(session.updated_at.as_deref(), Some("2025-07-05T10:00:00Z"));
    }

    #[test]
    fn single_valid_timestamp_populates_both_fields() {
        let temp = tempdir().unwrap();
        let projects_root = temp.path().join(".claude/projects");
        let project_dir = projects_root.join("-tmp-demo-repo");
        fs::create_dir_all(&project_dir).unwrap();
        let session_path = project_dir.join("ts-single.jsonl");
        fs::write(
            &session_path,
            r#"{"type":"meta","sessionId":"s1","cwd":"/tmp/demo","timestamp":"2025-07-05T12:00:00Z"}"#,
        )
        .unwrap();
        let store = ClaudeProjectsStore::new(&projects_root);
        let session = store.discover_sessions().unwrap().pop().unwrap();
        assert_eq!(session.created_at.as_deref(), Some("2025-07-05T12:00:00Z"));
        assert_eq!(session.updated_at.as_deref(), Some("2025-07-05T12:00:00Z"));
    }

    #[test]
    fn explicit_created_at_and_updated_at_aliases_work() {
        let temp = tempdir().unwrap();
        let projects_root = temp.path().join(".claude/projects");
        let project_dir = projects_root.join("-tmp-demo-repo");
        fs::create_dir_all(&project_dir).unwrap();
        let session_path = project_dir.join("ts-aliases.jsonl");
        fs::write(
            &session_path,
            [
                r#"{"type":"meta","sessionId":"s2","cwd":"/tmp/demo","createdAt":"2025-07-05T08:00:00Z","updatedAt":"2025-07-05T10:00:00Z"}"#,
            ]
            .join("\n"),
        )
        .unwrap();
        let store = ClaudeProjectsStore::new(&projects_root);
        let session = store.discover_sessions().unwrap().pop().unwrap();
        assert_eq!(session.created_at.as_deref(), Some("2025-07-05T08:00:00Z"));
        assert_eq!(session.updated_at.as_deref(), Some("2025-07-05T10:00:00Z"));
    }

    #[test]
    fn reordered_records_produce_chronological_min_max() {
        let temp = tempdir().unwrap();
        let projects_root = temp.path().join(".claude/projects");
        let project_dir = projects_root.join("-tmp-demo-repo");
        fs::create_dir_all(&project_dir).unwrap();
        let session_path = project_dir.join("ts-reordered.jsonl");
        fs::write(
            &session_path,
            [
                r#"{"type":"record","timestamp":"2025-07-06T00:30:00Z"}"#,
                r#"{"type":"record","timestamp":"2025-07-05T23:30:00Z"}"#,
            ]
            .join("\n"),
        )
        .unwrap();
        let store = ClaudeProjectsStore::new(&projects_root);
        let session = store.discover_sessions().unwrap().pop().unwrap();
        assert_eq!(session.created_at.as_deref(), Some("2025-07-05T23:30:00Z"));
        assert_eq!(session.updated_at.as_deref(), Some("2025-07-06T00:30:00Z"));
    }

    #[test]
    fn different_utc_offsets_compare_as_absolute_instants() {
        let temp = tempdir().unwrap();
        let projects_root = temp.path().join(".claude/projects");
        let project_dir = projects_root.join("-tmp-demo-repo");
        fs::create_dir_all(&project_dir).unwrap();
        let session_path = project_dir.join("ts-offsets.jsonl");
        fs::write(
            &session_path,
            [
                r#"{"type":"record","timestamp":"2025-07-05T12:00:00+02:00"}"#,
                r#"{"type":"record","timestamp":"2025-07-05T10:00:00+05:00"}"#,
            ]
            .join("\n"),
        )
        .unwrap();
        let store = ClaudeProjectsStore::new(&projects_root);
        let session = store.discover_sessions().unwrap().pop().unwrap();
        assert_eq!(session.created_at.as_deref(), Some("2025-07-05T05:00:00Z"));
        assert_eq!(session.updated_at.as_deref(), Some("2025-07-05T10:00:00Z"));
    }

    #[test]
    fn malformed_timestamp_ignored_when_valid_timestamps_exist() {
        let temp = tempdir().unwrap();
        let projects_root = temp.path().join(".claude/projects");
        let project_dir = projects_root.join("-tmp-demo-repo");
        fs::create_dir_all(&project_dir).unwrap();
        let session_path = project_dir.join("ts-malformed.jsonl");
        fs::write(
            &session_path,
            [
                r#"{"type":"record","timestamp":"not-a-timestamp"}"#,
                r#"{"type":"record","timestamp":"2025-07-05T08:00:00Z"}"#,
            ]
            .join("\n"),
        )
        .unwrap();
        let store = ClaudeProjectsStore::new(&projects_root);
        let session = store.discover_sessions().unwrap().pop().unwrap();
        assert_eq!(session.created_at.as_deref(), Some("2025-07-05T08:00:00Z"));
        assert_eq!(session.updated_at.as_deref(), Some("2025-07-05T08:00:00Z"));
    }

    #[test]
    fn no_valid_timestamps_leaves_both_fields_absent() {
        let temp = tempdir().unwrap();
        let projects_root = temp.path().join(".claude/projects");
        let project_dir = projects_root.join("-tmp-demo-repo");
        fs::create_dir_all(&project_dir).unwrap();
        let session_path = project_dir.join("ts-none.jsonl");
        fs::write(
            &session_path,
            [
                r#"{"type":"meta","sessionId":"s3","cwd":"/tmp/demo"}"#,
                r#"{"role":"user","message":{"content":"hello"}}"#,
            ]
            .join("\n"),
        )
        .unwrap();
        let store = ClaudeProjectsStore::new(&projects_root);
        let session = store.discover_sessions().unwrap().pop().unwrap();
        assert!(session.created_at.is_none());
        assert!(session.updated_at.is_none());
    }

    #[test]
    fn timestamps_normalised_to_utc_rfc3339() {
        let temp = tempdir().unwrap();
        let projects_root = temp.path().join(".claude/projects");
        let project_dir = projects_root.join("-tmp-demo-repo");
        fs::create_dir_all(&project_dir).unwrap();
        let session_path = project_dir.join("ts-normalised.jsonl");
        fs::write(
            &session_path,
            r#"{"type":"record","timestamp":"2025-07-05T10:00:00+02:00"}"#,
        )
        .unwrap();
        let store = ClaudeProjectsStore::new(&projects_root);
        let session = store.discover_sessions().unwrap().pop().unwrap();
        assert_eq!(session.created_at.as_deref(), Some("2025-07-05T08:00:00Z"));
    }

    #[test]
    fn utc_normalised_timestamps_sort_lexicographically() {
        let temp = tempdir().unwrap();
        let projects_root = temp.path().join(".claude/projects");
        let project_dir = projects_root.join("-tmp-demo-repo");
        fs::create_dir_all(&project_dir).unwrap();

        let earlier_path = project_dir.join("earlier.jsonl");
        fs::write(
            &earlier_path,
            [
                r#"{"type":"meta","sessionId":"e1","cwd":"/tmp/demo","timestamp":"2025-07-05T08:00:00Z"}"#,
                r#"{"role":"user","message":{"content":"a"}}"#,
            ]
            .join("\n"),
        )
        .unwrap();

        let later_path = project_dir.join("later.jsonl");
        fs::write(
            &later_path,
            [
                r#"{"type":"meta","sessionId":"e2","cwd":"/tmp/demo","timestamp":"2025-07-05T10:00:00Z"}"#,
                r#"{"role":"user","message":{"content":"b"}}"#,
            ]
            .join("\n"),
        )
        .unwrap();

        let store = ClaudeProjectsStore::new(&projects_root);
        let projects = group_sessions_by_project(store.discover_sessions().unwrap());
        assert_eq!(projects.len(), 1);
        assert_eq!(projects[0].sessions[0].native_id(), "e2");
        assert_eq!(projects[0].sessions[1].native_id(), "e1");
    }

    // ── Edge-case characterisation tests ──────────────────────────────────

    #[test]
    fn empty_projects_directory_returns_empty_sessions() {
        let temp = tempdir().unwrap();
        let projects_root = temp.path().join(".claude/projects");

        let store = ClaudeProjectsStore::new(&projects_root);
        let sessions = store.discover_sessions().unwrap();
        assert!(sessions.is_empty());
    }

    #[test]
    fn empty_projects_subdirectory_returns_empty_sessions() {
        let temp = tempdir().unwrap();
        let projects_root = temp.path().join(".claude/projects");
        let project_dir = projects_root.join("-empty-repo");
        fs::create_dir_all(&project_dir).unwrap();

        let store = ClaudeProjectsStore::new(&projects_root);
        let sessions = store.discover_sessions().unwrap();
        assert!(sessions.is_empty());
    }

    #[test]
    fn non_jsonl_files_are_ignored_in_session_discovery() {
        let temp = tempdir().unwrap();
        let projects_root = temp.path().join(".claude/projects");
        let project_dir = projects_root.join("-tmp-repo");
        fs::create_dir_all(&project_dir).unwrap();

        fs::write(project_dir.join("readme.md"), "# Notes").unwrap();
        fs::write(project_dir.join("data.txt"), "not a session").unwrap();
        fs::write(project_dir.join(".hidden"), "skip me").unwrap();

        let store = ClaudeProjectsStore::new(&projects_root);
        let sessions = store.discover_sessions().unwrap();
        assert!(sessions.is_empty());
    }

    #[test]
    fn session_with_only_meta_record_has_message_count_zero() {
        let temp = tempdir().unwrap();
        let projects_root = temp.path().join(".claude/projects");
        let project_dir = projects_root.join("-tmp-repo");
        fs::create_dir_all(&project_dir).unwrap();

        let session_path = project_dir.join("meta-only.jsonl");
        fs::write(
            &session_path,
            r#"{"type":"meta","sessionId":"m1","cwd":"/tmp/repo"}"#,
        )
        .unwrap();

        let store = ClaudeProjectsStore::new(&projects_root);
        let sessions = store.discover_sessions().unwrap();
        assert_eq!(sessions.len(), 1);
        assert_eq!(sessions[0].native_id(), "m1");
        assert_eq!(sessions[0].message_count, Some(0));
    }

    #[test]
    fn malformed_json_line_causes_discover_sessions_error() {
        let temp = tempdir().unwrap();
        let projects_root = temp.path().join(".claude/projects");
        let project_dir = projects_root.join("-tmp-repo");
        fs::create_dir_all(&project_dir).unwrap();

        let session_path = project_dir.join("bad.jsonl");
        fs::write(
            &session_path,
            [
                r#"{"type":"meta","sessionId":"s1","cwd":"/tmp/repo"}"#,
                r#"this is not valid json"#,
            ]
            .join("\n"),
        )
        .unwrap();

        let store = ClaudeProjectsStore::new(&projects_root);
        let result = store.discover_sessions();
        assert!(result.is_err());

        let error = result.unwrap_err();
        let error_msg = error.to_string();
        assert!(
            error_msg.contains("invalid session file"),
            "expected InvalidSessionFile error, got: {error_msg}"
        );
    }

    #[test]
    fn rename_missing_session_returns_error() {
        let temp = tempdir().unwrap();
        let projects_root = temp.path().join(".claude/projects");
        let missing_path = projects_root.join("no-such-project/session.jsonl");

        let store = ClaudeProjectsStore::new(&projects_root);
        let result = store.rename_session(&missing_path, "New Title");
        assert!(result.is_err());
    }

    #[test]
    fn delete_missing_session_returns_session_not_found() {
        let temp = tempdir().unwrap();
        let projects_root = temp.path().join(".claude/projects");

        let store = ClaudeProjectsStore::new(&projects_root);
        let result = store.delete_session("ghost", Path::new("/nonexistent/session.jsonl"));
        assert!(result.is_err());

        let error = result.unwrap_err();
        assert!(
            error.to_string().contains("session not found"),
            "expected SessionNotFound error"
        );
    }

    #[test]
    fn delete_project_inexistent_cwd_returns_project_not_found() {
        let temp = tempdir().unwrap();
        let projects_root = temp.path().join(".claude/projects");

        let store = ClaudeProjectsStore::new(&projects_root);
        let result = store.delete_project(Path::new("/no/such/project"));
        assert!(result.is_err());

        let error = result.unwrap_err();
        assert!(
            error.to_string().contains("project not found"),
            "expected ProjectNotFound error"
        );
    }

    #[test]
    fn empty_session_file_creates_session_without_fields() {
        let temp = tempdir().unwrap();
        let projects_root = temp.path().join(".claude/projects");
        let project_dir = projects_root.join("-tmp-repo");
        fs::create_dir_all(&project_dir).unwrap();

        let session_path = project_dir.join("empty-file.jsonl");
        fs::write(&session_path, "").unwrap();

        let store = ClaudeProjectsStore::new(&projects_root);
        let sessions = store.discover_sessions().unwrap();
        assert_eq!(sessions.len(), 1);
        assert_eq!(sessions[0].native_id(), "empty-file");
        assert_eq!(sessions[0].message_count, Some(0));
    }

    #[test]
    fn whitespace_only_file_skips_blank_lines() {
        let temp = tempdir().unwrap();
        let projects_root = temp.path().join(".claude/projects");
        let project_dir = projects_root.join("-tmp-repo");
        fs::create_dir_all(&project_dir).unwrap();

        let session_path = project_dir.join("ws-only.jsonl");
        fs::write(
            &session_path,
            [
                "",
                "   ",
                r#"{"type":"meta","sessionId":"ws1","cwd":"/tmp/repo"}"#,
                "",
            ]
            .join("\n"),
        )
        .unwrap();

        let store = ClaudeProjectsStore::new(&projects_root);
        let sessions = store.discover_sessions().unwrap();
        assert_eq!(sessions.len(), 1);
        assert_eq!(sessions[0].native_id(), "ws1");
    }

    #[test]
    fn domain_group_sessions_by_cwd() {
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
}
