use crate::{
    domain::{AgentId, ReportedCost, SessionCapabilities, SessionKey},
    infrastructure::timestamps::TimestampRange,
    pricing, Result, Session, SessionUsage, StetsonError,
};
use serde_json::Value;
use std::{
    fs::File,
    io::{BufRead, BufReader},
    path::Path,
};

pub(super) fn invalid(path: &Path, reason: impl std::fmt::Display) -> StetsonError {
    StetsonError::InvalidSessionFile(format!("{}: {reason}", path.display()))
}
pub(super) fn parse(path: &Path) -> Result<Option<Session>> {
    let mut ts = TimestampRange::new();
    let mut key_cwd = None;
    let mut count = 0;
    let mut title = None;
    let mut user = None;
    let mut seen_user = false;
    let mut model = None;
    let mut usage = None;
    let mut reported = None;
    for (i, line) in BufReader::new(File::open(path)?).lines().enumerate() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        let v: Value = serde_json::from_str(&line).map_err(|e| {
            StetsonError::InvalidSessionFile(format!("{}:{}: {e}", path.display(), i + 1))
        })?;
        let kind = v["type"].as_str();
        if key_cwd.is_none() {
            if kind != Some("session") {
                return Err(invalid(path, "missing session header"));
            }
            let version = v["version"]
                .as_u64()
                .ok_or_else(|| invalid(path, "invalid session version"))?;
            if version != 3 {
                return Ok(None);
            }
            let field = |k: &str| {
                v[k].as_str()
                    .filter(|s| !s.trim().is_empty())
                    .ok_or_else(|| invalid(path, format!("missing {k}")))
            };
            let key = SessionKey::new(AgentId::new("pi").unwrap(), field("id")?)
                .map_err(|e| invalid(path, e))?;
            key_cwd = Some((key, field("cwd")?.into()));
        }
        if let Some(s) = v["timestamp"].as_str() {
            ts.consider(s);
        }
        if kind == Some("session_info") {
            if let Some(n) = v["name"].as_str() {
                title = Some(n.to_owned());
            }
        }
        if kind != Some("message") {
            continue;
        }
        count += 1;
        let m = &v["message"];
        match m["role"].as_str() {
            Some("user") if !seen_user => {
                seen_user = true;
                let c = &m["content"];
                user = c
                    .as_str()
                    .or_else(|| {
                        c.as_array()?.iter().find(|x| x["type"] == "text")?["text"].as_str()
                    })
                    .map(|s| {
                        s.lines()
                            .map(str::trim)
                            .collect::<Vec<_>>()
                            .join(" ")
                            .trim()
                            .to_owned()
                    })
                    .filter(|s| !s.is_empty());
            }
            Some("assistant") => {
                if model.is_none() {
                    model = m["model"].as_str().map(str::to_owned);
                }
                let u = &m["usage"];
                if !u.is_object() {
                    continue;
                }
                let sum = usage.get_or_insert(SessionUsage {
                    input_tokens: 0,
                    output_tokens: 0,
                    cache_read_tokens: 0,
                    cache_creation_tokens: 0,
                });
                let n = |k: &str| u[k].as_u64().unwrap_or(0);
                sum.input_tokens += n("input");
                sum.output_tokens += n("output");
                sum.cache_read_tokens += n("cacheRead");
                sum.cache_creation_tokens += n("cacheWrite");
                let c = &u["cost"];
                if let Some(total) = c["total"].as_f64() {
                    let cost = reported.get_or_insert(ReportedCost {
                        total: 0.,
                        from_provider: true,
                    });
                    cost.total += total;
                    cost.from_provider &= c["totalFromProvider"].as_bool() == Some(true);
                }
            }
            _ => {}
        }
    }
    let (key, cwd) = key_cwd.ok_or_else(|| invalid(path, "missing session header"))?;
    let estimated_cost = usage
        .as_ref()
        .zip(model.as_deref())
        .and_then(|(u, m)| pricing::estimate_cost(u, m));
    Ok(Some(Session {
        key,
        cwd,
        title: title.or(user).unwrap_or_else(|| {
            path.file_stem()
                .unwrap_or_default()
                .to_string_lossy()
                .into_owned()
        }),
        source_location: Some(path.display().to_string()),
        git_branch: None,
        created_at: ts.created_at_string(),
        updated_at: ts.updated_at_string(),
        message_count: Some(count),
        model,
        usage,
        reported_cost: reported,
        estimated_cost,
        capabilities: SessionCapabilities {
            resume: true,
            rename: true,
            delete: true,
            inspect_history: true,
        },
    }))
}
