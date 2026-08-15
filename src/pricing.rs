use crate::domain::{CostEstimate, SessionUsage};
use serde_json::Value;
use std::sync::OnceLock;

const PRICING_JSON: &str = include_str!("../data/llm_pricing.json");

/// Pricing rates per million tokens for a given model and effective date.
struct PricingEntry {
    #[allow(dead_code)]
    provider: String,
    model_prefixes: Vec<String>,
    #[allow(dead_code)]
    effective_date: String,
    input_per_million: f64,
    output_per_million: f64,
    cache_creation_per_million: f64,
    cache_read_per_million: f64,
}

fn pricing_table() -> &'static [PricingEntry] {
    static PRICING_TABLE: OnceLock<Vec<PricingEntry>> = OnceLock::new();
    PRICING_TABLE.get_or_init(load_pricing_table).as_slice()
}

fn load_pricing_table() -> Vec<PricingEntry> {
    let root: Value =
        serde_json::from_str(PRICING_JSON).expect("embedded LLM pricing JSON must be valid");
    let prices = root
        .get("prices")
        .and_then(Value::as_array)
        .expect("embedded LLM pricing JSON must contain a prices array");

    prices
        .iter()
        .map(|entry| PricingEntry {
            provider: required_string(entry, "provider"),
            model_prefixes: required_string_array(entry, "model_prefixes"),
            effective_date: required_string(entry, "effective_date"),
            input_per_million: required_f64(entry, "input_per_million"),
            output_per_million: required_f64(entry, "output_per_million"),
            cache_creation_per_million: required_f64(entry, "cache_creation_per_million"),
            cache_read_per_million: required_f64(entry, "cache_read_per_million"),
        })
        .collect()
}

fn required_string(entry: &Value, field: &str) -> String {
    entry
        .get(field)
        .and_then(Value::as_str)
        .unwrap_or_else(|| panic!("pricing entry must contain string field {field}"))
        .to_owned()
}

fn required_string_array(entry: &Value, field: &str) -> Vec<String> {
    entry
        .get(field)
        .and_then(Value::as_array)
        .unwrap_or_else(|| panic!("pricing entry must contain string array field {field}"))
        .iter()
        .map(|value| {
            value
                .as_str()
                .unwrap_or_else(|| panic!("pricing entry field {field} must contain strings"))
                .to_owned()
        })
        .collect()
}

fn required_f64(entry: &Value, field: &str) -> f64 {
    entry
        .get(field)
        .and_then(Value::as_f64)
        .unwrap_or_else(|| panic!("pricing entry must contain number field {field}"))
}

fn find_pricing(model: &str) -> Option<&'static PricingEntry> {
    pricing_table().iter().rev().find(|entry| {
        entry
            .model_prefixes
            .iter()
            .any(|prefix| model.contains(prefix))
    })
}

pub fn estimate_cost(usage: &SessionUsage, model: &str) -> Option<CostEstimate> {
    let pricing = find_pricing(model)?;

    Some(CostEstimate {
        input_cost: usage.input_tokens as f64 * pricing.input_per_million / 1_000_000.0,
        output_cost: usage.output_tokens as f64 * pricing.output_per_million / 1_000_000.0,
        cache_creation_cost: usage.cache_creation_tokens as f64
            * pricing.cache_creation_per_million
            / 1_000_000.0,
        cache_read_cost: usage.cache_read_tokens as f64 * pricing.cache_read_per_million
            / 1_000_000.0,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matches_sonnet_model() {
        let usage = SessionUsage {
            input_tokens: 1_000_000,
            output_tokens: 500_000,
            cache_creation_tokens: 0,
            cache_read_tokens: 0,
        };
        let cost = estimate_cost(&usage, "claude-sonnet-4-20250514").unwrap();
        assert!((cost.input_cost - 3.0).abs() < 0.01);
        assert!((cost.output_cost - 7.5).abs() < 0.01);
    }

    #[test]
    fn matches_opus_model() {
        let usage = SessionUsage {
            input_tokens: 1_000_000,
            output_tokens: 100_000,
            cache_creation_tokens: 0,
            cache_read_tokens: 0,
        };
        let cost = estimate_cost(&usage, "claude-opus-4-20250514").unwrap();
        assert!((cost.input_cost - 15.0).abs() < 0.01);
        assert!((cost.output_cost - 7.5).abs() < 0.01);
    }

    #[test]
    fn handles_cache_tokens() {
        let usage = SessionUsage {
            input_tokens: 0,
            output_tokens: 0,
            cache_creation_tokens: 2_000_000,
            cache_read_tokens: 10_000_000,
        };
        let cost = estimate_cost(&usage, "claude-sonnet-4-20250514").unwrap();
        assert!((cost.cache_creation_cost - 7.5).abs() < 0.01);
        assert!((cost.cache_read_cost - 3.0).abs() < 0.01);
    }

    #[test]
    fn unknown_model_returns_none() {
        let usage = SessionUsage {
            input_tokens: 1000,
            output_tokens: 1000,
            cache_creation_tokens: 0,
            cache_read_tokens: 0,
        };
        assert!(estimate_cost(&usage, "unknown-model").is_none());
    }

    #[test]
    fn loads_embedded_pricing_json() {
        let prices = pricing_table();
        assert!(prices.len() >= 6);
        assert!(prices.iter().any(|entry| entry.provider == "anthropic"
            && entry
                .model_prefixes
                .iter()
                .any(|prefix| prefix == "claude-sonnet-4")));
    }

    #[test]
    fn total_cost_sums_all_components() {
        let cost = CostEstimate {
            input_cost: 1.0,
            output_cost: 2.0,
            cache_creation_cost: 3.0,
            cache_read_cost: 4.0,
        };
        assert!((cost.total_cost() - 10.0).abs() < 0.01);
    }

    #[test]
    fn total_tokens_sums_all_components() {
        let usage = SessionUsage {
            input_tokens: 100,
            output_tokens: 200,
            cache_creation_tokens: 300,
            cache_read_tokens: 400,
        };
        assert_eq!(usage.total_tokens(), 1000);
    }
}
