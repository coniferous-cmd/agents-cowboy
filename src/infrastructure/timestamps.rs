use chrono::{DateTime, FixedOffset, Utc};
use serde_json::Value;

/// Collects all string values for the given keys by recursively searching
/// a JSON Value tree. Returns every value found, in arbitrary order.
pub(super) fn collect_timestamps<'a>(value: &'a Value, candidate_keys: &[&str]) -> Vec<&'a str> {
    match value {
        Value::Object(map) => {
            let mut result = Vec::new();
            for key in candidate_keys {
                if let Some(Value::String(s)) = map.get(*key) {
                    let s = s.trim();
                    if !s.is_empty() {
                        result.push(s);
                    }
                }
            }
            for val in map.values() {
                result.extend(collect_timestamps(val, candidate_keys));
            }
            result
        }
        Value::Array(items) => items
            .iter()
            .flat_map(|item| collect_timestamps(item, candidate_keys))
            .collect(),
        _ => Vec::new(),
    }
}

/// Tracks the minimum and maximum valid RFC 3339 timestamps across records.
pub(super) struct TimestampRange {
    earliest: Option<DateTime<FixedOffset>>,
    latest: Option<DateTime<FixedOffset>>,
}

impl TimestampRange {
    pub(super) fn new() -> Self {
        Self {
            earliest: None,
            latest: None,
        }
    }

    /// Parse `ts_str` as RFC 3339 and update the tracked range.
    pub(super) fn consider(&mut self, ts_str: &str) {
        if let Ok(dt) = DateTime::parse_from_rfc3339(ts_str) {
            match self.earliest {
                Some(ref earliest) if dt < *earliest => self.earliest = Some(dt),
                None => self.earliest = Some(dt),
                _ => {}
            }
            match self.latest {
                Some(ref latest) if dt > *latest => self.latest = Some(dt),
                None => self.latest = Some(dt),
                _ => {}
            }
        }
    }

    /// Returns the earliest timestamp as a UTC RFC 3339 string, or `None`.
    pub(super) fn created_at_string(&self) -> Option<String> {
        self.earliest.map(|dt| {
            dt.with_timezone(&Utc)
                .format("%Y-%m-%dT%H:%M:%SZ")
                .to_string()
        })
    }

    /// Returns the latest timestamp as a UTC RFC 3339 string, or `None`.
    pub(super) fn updated_at_string(&self) -> Option<String> {
        self.latest.map(|dt| {
            dt.with_timezone(&Utc)
                .format("%Y-%m-%dT%H:%M:%SZ")
                .to_string()
        })
    }
}
