use crate::domain::{Result, Session, SessionCapabilities, SessionKey, SessionUsage, StetsonError};
use crate::pricing;
use serde_json::Value;
use std::fs;
use std::path::{Path, PathBuf};

use super::timestamps::{collect_timestamps, TimestampRange};

pub(super) fn parse_session_file(
    session_path: &Path,
    project_hint: Option<&Path>,
) -> Result<Session> {
    let contents = fs::read_to_string(session_path)?;
    let mut session_id = None;
    let mut custom_title = None;
    let mut agent_title = None;
    let mut cwd = None;
    let mut git_branch = None;
    let mut ts_range = TimestampRange::new();
    let mut message_count = 0usize;
    let mut usage = SessionUsage {
        input_tokens: 0,
        output_tokens: 0,
        cache_creation_tokens: 0,
        cache_read_tokens: 0,
    };
    let mut has_usage = false;
    let mut model: Option<String> = None;

    for (line_number, line) in contents.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }

        let value = serde_json::from_str::<Value>(line).map_err(|error| {
            StetsonError::InvalidSessionFile(format!(
                "{}:{}: {error}",
                session_path.display(),
                line_number + 1
            ))
        })?;

        if session_id.is_none() {
            session_id = find_first_string(&value, &["sessionId", "session_id", "id"]);
        }
        if custom_title.is_none() {
            custom_title = find_title(&value, TitleKind::Custom);
        }
        if agent_title.is_none() {
            agent_title = find_title(&value, TitleKind::Agent);
        }
        if cwd.is_none() {
            cwd = find_first_string(
                &value,
                &["cwd", "currentWorkingDirectory", "workingDirectory"],
            )
            .map(PathBuf::from);
        }
        if git_branch.is_none() {
            git_branch = find_first_string(&value, &["gitBranch", "branch"]);
            if git_branch.is_none() {
                git_branch = find_nested_string(&value, &["git", "branch"]);
            }
        }
        for ts_str in collect_timestamps(
            &value,
            &[
                "timestamp",
                "createdAt",
                "created_at",
                "updatedAt",
                "updated_at",
            ],
        ) {
            ts_range.consider(ts_str);
        }
        if is_message_record(&value) {
            message_count += 1;
        }

        if let Some(record_usage) = extract_usage(&value) {
            usage.input_tokens += record_usage.input_tokens;
            usage.output_tokens += record_usage.output_tokens;
            usage.cache_creation_tokens += record_usage.cache_creation_tokens;
            usage.cache_read_tokens += record_usage.cache_read_tokens;
            has_usage = true;
        }
        if model.is_none() {
            model = extract_model(&value);
        }
    }

    let session_id = session_id.unwrap_or_else(|| session_path_stem(session_path));
    let title = custom_title
        .or(agent_title)
        .unwrap_or_else(|| session_path_stem(session_path));
    let cwd = match cwd {
        Some(ref c) if c.is_absolute() => c.clone(),
        _ => project_hint
            .map(Path::to_path_buf)
            .or(cwd)
            .unwrap_or_else(|| PathBuf::from(".")),
    };

    let created_at = ts_range.created_at_string();
    let updated_at = ts_range.updated_at_string();

    let usage = if has_usage { Some(usage) } else { None };
    let estimated_cost = match (&usage, &model) {
        (Some(u), Some(m)) => pricing::estimate_cost(u, m),
        _ => None,
    };

    Ok(Session {
        key: SessionKey::claude(session_id),
        title,
        cwd,
        git_branch,
        created_at,
        updated_at,
        source_location: Some(session_path.display().to_string()),
        message_count: Some(message_count),
        usage,
        model,
        estimated_cost,
        capabilities: SessionCapabilities {
            resume: true,
            rename: true,
            delete: true,
            inspect_history: true,
        },
    })
}

pub(super) fn session_path_stem(path: &Path) -> String {
    path.file_stem()
        .map(|stem| stem.to_string_lossy().into_owned())
        .filter(|stem| !stem.is_empty())
        .unwrap_or_else(|| path.display().to_string())
}

fn is_message_record(value: &Value) -> bool {
    if let Some(role) = find_first_string(value, &["role"]) {
        return matches!(
            role.as_str(),
            "user" | "assistant" | "system" | "tool" | "developer"
        );
    }

    if let Some(kind) = find_first_string(value, &["type"]) {
        return matches!(
            kind.as_str(),
            "user" | "assistant" | "system" | "tool" | "message"
        );
    }

    value.get("message").is_some() || value.get("content").and_then(Value::as_array).is_some()
}

enum TitleKind {
    Custom,
    Agent,
}

fn find_title(value: &Value, kind: TitleKind) -> Option<String> {
    match kind {
        TitleKind::Custom => find_nested_string(value, &["custom-title", "customTitle"])
            .or_else(|| find_nested_string(value, &["custom_title", "customTitle"]))
            .or_else(|| find_first_string(value, &["customTitle"])),
        TitleKind::Agent => find_nested_string(value, &["agent-name", "agentName"])
            .or_else(|| find_nested_string(value, &["agent_name", "agentName"]))
            .or_else(|| find_first_string(value, &["agentName"])),
    }
}

pub(super) fn find_nested_string(value: &Value, path: &[&str]) -> Option<String> {
    if path.is_empty() {
        return value.as_str().map(str::to_owned);
    }

    match value {
        Value::Object(map) => {
            if let Some(candidate) = map.get(path[0]) {
                if let Some(found) = find_nested_string(candidate, &path[1..]) {
                    return Some(found);
                }
            }

            for nested in map.values() {
                if let Some(found) = find_nested_string(nested, path) {
                    return Some(found);
                }
            }

            None
        }
        Value::Array(items) => items.iter().find_map(|item| find_nested_string(item, path)),
        _ => None,
    }
}

pub(super) fn find_first_string(value: &Value, candidate_keys: &[&str]) -> Option<String> {
    match value {
        Value::Object(map) => {
            for key in candidate_keys {
                if let Some(found) = map.get(*key).and_then(value_to_string) {
                    return Some(found);
                }
            }

            for nested in map.values() {
                if let Some(found) = find_first_string(nested, candidate_keys) {
                    return Some(found);
                }
            }

            None
        }
        Value::Array(items) => items
            .iter()
            .find_map(|item| find_first_string(item, candidate_keys)),
        _ => None,
    }
}

fn value_to_string(value: &Value) -> Option<String> {
    match value {
        Value::String(value) if !value.trim().is_empty() => Some(value.clone()),
        Value::Number(value) => Some(value.to_string()),
        _ => None,
    }
}

fn extract_usage(value: &Value) -> Option<SessionUsage> {
    let usage_value = find_nested_usage(value)?;

    let input_tokens = find_u64_field(usage_value, &["input_tokens"]).unwrap_or(0);
    let output_tokens = find_u64_field(usage_value, &["output_tokens"]).unwrap_or(0);
    let cache_creation_tokens =
        find_u64_field(usage_value, &["cache_creation_input_tokens"]).unwrap_or(0);
    let cache_read_tokens = find_u64_field(usage_value, &["cache_read_input_tokens"]).unwrap_or(0);

    if input_tokens == 0
        && output_tokens == 0
        && cache_creation_tokens == 0
        && cache_read_tokens == 0
    {
        return None;
    }

    Some(SessionUsage {
        input_tokens,
        output_tokens,
        cache_creation_tokens,
        cache_read_tokens,
    })
}

fn find_nested_usage(value: &Value) -> Option<&Value> {
    // Path 1: message.usage (assistant records)
    if let Some(usage) = value.get("message").and_then(|m| m.get("usage")) {
        return Some(usage);
    }
    // Path 2: top-level usage
    if let Some(usage) = value.get("usage") {
        return Some(usage);
    }
    None
}

fn find_u64_field(value: &Value, keys: &[&str]) -> Option<u64> {
    for key in keys {
        if let Some(num) = value.get(*key).and_then(|v| v.as_u64()) {
            return Some(num);
        }
    }
    None
}

fn extract_model(value: &Value) -> Option<String> {
    // Path 1: message.model (assistant records)
    if let Some(model) = value
        .get("message")
        .and_then(|m| m.get("model"))
        .and_then(|v| v.as_str())
    {
        return Some(model.to_owned());
    }
    // Path 2: top-level model
    if let Some(model) = value.get("model").and_then(|v| v.as_str()) {
        return Some(model.to_owned());
    }
    None
}

pub(super) fn set_title_in_value(value: &mut Value, new_title: &str) -> bool {
    if set_nested_title(value, &["custom-title", "customTitle"], new_title) {
        return true;
    }
    if set_nested_title(value, &["custom_title", "customTitle"], new_title) {
        return true;
    }
    if set_direct_string_key(value, "customTitle", new_title) {
        return true;
    }
    if set_nested_title(value, &["agent-name", "agentName"], new_title) {
        return true;
    }
    if set_nested_title(value, &["agent_name", "agentName"], new_title) {
        return true;
    }
    set_direct_string_key(value, "agentName", new_title)
}

fn set_direct_string_key(value: &mut Value, key: &str, new_title: &str) -> bool {
    match value {
        Value::Object(map) => {
            if map.contains_key(key) {
                map.insert(key.to_string(), Value::String(new_title.to_string()));
                return true;
            }
            for nested in map.values_mut() {
                if set_direct_string_key(nested, key, new_title) {
                    return true;
                }
            }
            false
        }
        Value::Array(items) => items
            .iter_mut()
            .any(|item| set_direct_string_key(item, key, new_title)),
        _ => false,
    }
}

fn set_nested_title(value: &mut Value, path: &[&str], new_title: &str) -> bool {
    if path.is_empty() {
        return false;
    }

    match value {
        Value::Object(map) => {
            if path.len() == 1 && map.contains_key(path[0]) {
                map.insert(path[0].to_string(), Value::String(new_title.to_string()));
                return true;
            }

            if let Some(next) = map.get_mut(path[0]) {
                if set_nested_title(next, &path[1..], new_title) {
                    return true;
                }
            }

            for nested in map.values_mut() {
                if set_nested_title(nested, path, new_title) {
                    return true;
                }
            }

            false
        }
        Value::Array(items) => items
            .iter_mut()
            .any(|item| set_nested_title(item, path, new_title)),
        _ => false,
    }
}
