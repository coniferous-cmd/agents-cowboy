use cowboy::pi::PiProjectsStore;
use serde_json::{json, Value};
use std::{
    fs,
    path::{Path, PathBuf},
};
fn header(id: &str, cwd: &Path) -> Value {
    json!({"type":"session","version":3,"id":id,"cwd":cwd,"timestamp":"2025-01-01T03:00:00+03:00"})
}
fn put(root: &Path, dir: &str, name: &str, records: &[Value]) -> PathBuf {
    let dir = root.join(dir);
    fs::create_dir_all(&dir).unwrap();
    let path = dir.join(format!("{name}.jsonl"));
    fs::write(
        &path,
        records
            .iter()
            .map(Value::to_string)
            .collect::<Vec<_>>()
            .join("\n\n"),
    )
    .unwrap();
    path
}
fn msg(role: &str, fields: Value) -> Value {
    let mut m = fields;
    m["role"] = json!(role);
    json!({"type":"message","timestamp":"2025-01-01T03:00:00Z","message":m})
}
#[test]
fn parse_v3_stream_and_group_by_header() {
    let t = tempfile::tempdir().unwrap();
    let root = t.path();
    let cwd = Path::new("/some-work");
    let h = header("native", cwd);
    let model = "claude-sonnet-4-20250514";
    let a = msg(
        "assistant",
        json!({"model":model,"timestamp":5,"usage":{"input":10,"output":2,"cacheRead":4,"cacheWrite":3,"reasoning":800,"cost":{"total":0.25,"totalFromProvider":true}}}),
    );
    let b = msg(
        "assistant",
        json!({"model":"second","usage":{"input":2,"output":1,"cost":{"total":0.5,"totalFromProvider":false}}}),
    );
    put(
        root,
        "aa",
        "stem",
        &[
            h.clone(),
            msg(
                "user",
                json!({"content":[{"type":"text","text":" first\nline "}]}),
            ),
            a.clone(),
            b,
        ],
    );
    put(root, "bb", "other", &[header("b", cwd), a]);
    let store = PiProjectsStore::new(root);
    let projects = store.load_projects().unwrap();
    assert_eq!(projects.len(), 1);
    let s = projects[0]
        .sessions
        .iter()
        .find(|s| s.native_id() == "native")
        .unwrap();
    assert_eq!(s.cwd, cwd);
    assert_eq!(s.title, "first line");
    assert_eq!(s.created_at.as_deref(), Some("2025-01-01T00:00:00Z"));
    assert_eq!(s.updated_at.as_deref(), Some("2025-01-01T03:00:00Z"));
    assert_eq!(s.message_count, Some(3));
    assert_eq!(s.model.as_deref(), Some(model));
    let u = s.usage.as_ref().unwrap();
    assert_eq!(
        (
            u.input_tokens,
            u.output_tokens,
            u.cache_read_tokens,
            u.cache_creation_tokens,
            u.total_tokens()
        ),
        (12, 3, 4, 3, 22)
    );
    let c = s.reported_cost.unwrap();
    assert_eq!((c.total, c.from_provider), (0.75, false));
    assert!(
        projects[0]
            .sessions
            .iter()
            .find(|s| s.native_id() == "b")
            .unwrap()
            .reported_cost
            .unwrap()
            .from_provider
    );
}
#[test]
fn title_usage_and_estimation_fallbacks() {
    let t = tempfile::tempdir().unwrap();
    let h = header("a", t.path());
    let store = PiProjectsStore::new(t.path());
    put(t.path(), "p", "stem", std::slice::from_ref(&h));
    let s = store.discover_sessions().unwrap().pop().unwrap();
    assert_eq!(s.title, "stem");
    assert!(s.usage.is_none() && s.reported_cost.is_none());
    put(
        t.path(),
        "p",
        "stem",
        &[
            h,
            json!({"type":"session_info","name":"old"}),
            json!({"type":"session_info","name":"new"}),
            msg(
                "assistant",
                json!({"model":"claude-sonnet-4-20250514","usage":{"input":0,"output":0,"cacheWrite":0,"cacheRead":0}}),
            ),
        ],
    );
    let s = store.discover_sessions().unwrap().pop().unwrap();
    assert_eq!(s.title, "new");
    assert_eq!(s.usage.unwrap().total_tokens(), 0);
    assert!(s.reported_cost.is_none() && s.estimated_cost.is_some());
}
#[test]
fn missing_root_ignored_files_and_invalid_headers() {
    let t = tempfile::tempdir().unwrap();
    let root = t.path().join("sessions");
    let store = PiProjectsStore::new(&root);
    assert!(store.load_projects().unwrap().is_empty());
    let h = header("a", t.path());
    let mut old = h.clone();
    old["version"] = json!(2);
    put(&root, "p", "old", &[old]);
    let p = put(&root, "p", "stem", std::slice::from_ref(&h));
    fs::write(root.join("p/note"), "junk").unwrap();
    fs::create_dir_all(root.join("p/sub.jsonl")).unwrap();
    assert_eq!(store.discover_sessions().unwrap().len(), 1);
    for (record, reason) in [
        (json!({"type":"message"}), "header"),
        (json!({"type":"session","version":3,"cwd":"x"}), "id"),
        (
            json!({"type":"session","version":3,"cwd":"x","id":" "}),
            "id",
        ),
        (json!({"type":"session","version":3,"id":"a"}), "cwd"),
        (
            json!({"type":"session","version":3,"id":"a","cwd":""}),
            "cwd",
        ),
    ] {
        fs::write(&p, record.to_string()).unwrap();
        let e = store.load_projects().unwrap_err().to_string();
        assert!(e.contains(reason) && e.contains("stem.jsonl"), "{e}");
    }
    fs::write(&p, format!("{h}\n\nBAD")).unwrap();
    assert!(store
        .load_projects()
        .unwrap_err()
        .to_string()
        .contains("stem.jsonl:3"));
}
#[test]
fn rename_appends_after_last_record_id_even_with_only_a_header() {
    let t = tempfile::tempdir().unwrap();
    let p = put(t.path(), "p", "stem", &[header("header-id", t.path())]);
    let store = PiProjectsStore::new(t.path());
    let key = store.discover_sessions().unwrap().remove(0).key;
    store.rename_session(&key, "new").unwrap();
    let raw = fs::read_to_string(p).unwrap();
    let v: Value = serde_json::from_str(raw.lines().last().unwrap()).unwrap();
    assert_eq!(v["parentId"], "header-id");
}
#[test]
fn rename_append_update_and_session_delete() {
    let t = tempfile::tempdir().unwrap();
    let p = put(
        t.path(),
        "p",
        "stem",
        &[
            header("s", t.path()),
            json!({"type":"message","id":"abcdef12"}),
        ],
    );
    let store = PiProjectsStore::new(t.path());
    let key = store.discover_sessions().unwrap().remove(0).key;
    assert!(store.rename_session(&key, "  ").is_err());
    store.rename_session(&key, "First").unwrap();
    let raw = fs::read_to_string(&p).unwrap();
    let name: Value = serde_json::from_str(raw.lines().last().unwrap()).unwrap();
    assert_eq!(name["parentId"], "abcdef12");
    assert_eq!(name["type"], "session_info");
    let id = name["id"].as_str().unwrap();
    assert!(id.len() == 8 && u32::from_str_radix(id, 16).is_ok());
    chrono::DateTime::parse_from_rfc3339(name["timestamp"].as_str().unwrap()).unwrap();
    store.rename_session(&key, "Final").unwrap();
    assert!(!fs::read_to_string(&p).unwrap().contains("First"));
    assert_eq!(store.discover_sessions().unwrap()[0].title, "Final");
    store.delete_session(&key).unwrap();
    assert!(!p.exists());
    assert!(store.rename_session(&key, "x").is_err() && store.delete_session(&key).is_err());
}
#[test]
fn delete_matches_group_identity_only_after_complete_scan() {
    let t = tempfile::tempdir().unwrap();
    let cwd = t.path().join("real-project");
    fs::create_dir_all(cwd.join("child")).unwrap();
    let alias = cwd.join("child/..");
    assert_eq!(
        fs::canonicalize(&cwd).unwrap(),
        fs::canonicalize(&alias).unwrap()
    );
    let root = t.path().join("sessions");
    let s = PiProjectsStore::new(&root);
    let a = put(&root, "mixed", "a", &[header("a", &cwd)]);
    let b = put(&root, "clean", "b", &[header("b", &cwd)]);
    let other = put(&root, "mixed", "alias", &[header("c", &alias)]);
    let old = put(
        &root,
        "mixed",
        "old",
        &[json!({"type":"session","version":2})],
    );
    let note = root.join("mixed/note");
    fs::write(&note, "x").unwrap();
    assert_eq!(s.load_projects().unwrap().len(), 2);
    let bad = put(&root, "mixed", "bad", &[json!({})]);
    assert!(s.delete_project(&cwd).is_err());
    assert!(a.exists() && b.exists());
    fs::remove_file(bad).unwrap();
    s.delete_project(&cwd).unwrap();
    assert!(!a.exists() && !b.parent().unwrap().exists());
    assert!(other.exists() && old.exists() && note.exists());
    assert!(s.delete_project(&cwd).is_err());
}
