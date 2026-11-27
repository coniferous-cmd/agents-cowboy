use serde_json::Value;
use std::collections::HashMap;

#[derive(Eq, Hash, PartialEq)]
pub(crate) enum UsageKey {
    Request(String),
    Message(String),
    Entry(String),
}

impl UsageKey {
    pub(crate) fn from_record(record: &Value) -> Option<Self> {
        let message = &record["message"];
        let request_keys = ["requestId", "request_id"];
        let message_keys = ["messageId", "message_id"];
        find_id(record, &request_keys)
            .or_else(|| find_id(message, &request_keys))
            .map(|id| Self::Request(id.to_owned()))
            .or_else(|| {
                find_id(message, &["id", "messageId", "message_id"])
                    .or_else(|| find_id(record, &message_keys))
                    .map(|id| Self::Message(id.to_owned()))
            })
    }

    pub(crate) fn from_pi_record(record: &Value) -> Option<Self> {
        Self::from_record(record)
            .or_else(|| find_id(record, &["id"]).map(|id| Self::Entry(id.to_owned())))
    }
}

fn find_id<'a>(value: &'a Value, keys: &[&str]) -> Option<&'a str> {
    keys.iter()
        .filter_map(|key| value[*key].as_str())
        .find(|id| !id.trim().is_empty())
}

pub(crate) struct UsageRecords<T> {
    slots: HashMap<UsageKey, usize>,
    records: Vec<T>,
}

impl<T> UsageRecords<T> {
    pub(crate) fn new() -> Self {
        Self {
            slots: HashMap::new(),
            records: Vec::new(),
        }
    }

    pub(crate) fn insert(&mut self, key: Option<UsageKey>, usage: T) {
        if let Some(key) = key {
            let slot = *self.slots.entry(key).or_insert(self.records.len());
            if slot < self.records.len() {
                self.records[slot] = usage;
                return;
            }
        }
        self.records.push(usage);
    }

    pub(crate) fn into_records(self) -> Vec<T> {
        self.records
    }
}
