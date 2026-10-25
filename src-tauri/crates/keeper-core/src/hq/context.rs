//! Last-answer occupancy, independent of cumulative billing usage (Q109-19).
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use ts_rs::TS;

use super::sessions::json;

const PARTS: [(&str, &str, &str); 7] = [
    (
        "persona",
        "System and persona",
        "System instructions, SOUL.md and USER.md.",
    ),
    ("memory", "Memory", "Memory included in this prompt."),
    (
        "tools",
        "Built-in tools",
        "Built-in tool definitions offered to the model.",
    ),
    (
        "mcp",
        "MCP tools",
        "MCP tool definitions offered to the model.",
    ),
    (
        "history",
        "History",
        "Conversation history and the current question.",
    ),
    (
        "attachments",
        "Attachments",
        "Attachments included in this turn.",
    ),
    (
        "tool_results",
        "This turn's tool results",
        "Tool calls and results included in this turn.",
    ),
];
const MAX_SAFE: u64 = 9_007_199_254_740_991;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct AgentContextPartVm {
    pub label: String,
    pub description: String,
    pub tokens: String,
    pub share: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct AgentContextVm {
    #[ts(type = "number")]
    pub used: u64,
    #[ts(type = "number | null")]
    pub window: Option<u64>,
    /// Rust-formatted rounded percentage, absent when the window is unknown.
    pub percent: Option<String>,
    /// Bounded for the accessible meter; the label preserves over-window values.
    pub meter_percent: Option<u32>,
    pub ring_background: Option<String>,
    pub summary: String,
    pub parts: Vec<AgentContextPartVm>,
}

/// Never fall back to an older answer (or billing usage) when the latest lacks context.
pub fn latest(rows: &[Map<String, Value>], reader: bool) -> Option<AgentContextVm> {
    if !reader {
        return None;
    }
    let metadata = rows
        .iter()
        .rev()
        .filter(|row| row.get("role").and_then(Value::as_str) == Some("agent"))
        .find_map(|row| {
            let metadata = json(row, "metadata");
            metadata
                .get("kind")
                .is_none_or(Value::is_null)
                .then_some(metadata)
        })?;
    let context = metadata.get("context")?;
    let result = parse(context);
    if result.is_none() {
        tracing::warn!("hq: refusing malformed answer context metadata");
    }
    result
}

fn number(value: &Value) -> Option<u64> {
    value.as_u64().filter(|n| *n <= MAX_SAFE)
}

fn rounded(n: u64, total: u64) -> u128 {
    if total == 0 {
        return 0;
    }
    (u128::from(n) * 100 + u128::from(total) / 2) / u128::from(total)
}

fn parse(context: &Value) -> Option<AgentContextVm> {
    let used = number(context.get("used")?)?;
    let window = match context.get("window")? {
        Value::Null => None,
        value => Some(number(value).filter(|n| *n > 0)?),
    };
    let raw = context.get("parts")?.as_array()?;
    if raw.len() != PARTS.len() {
        return None;
    }
    let mut sum = 0u64;
    let mut parts = Vec::with_capacity(PARTS.len());
    for (key, label, description) in PARTS {
        let mut matches = raw
            .iter()
            .filter(|part| part.get("part").and_then(Value::as_str) == Some(key));
        let part = matches.next()?;
        if matches.next().is_some() || part.get("estimated")?.as_bool() != Some(true) {
            return None;
        }
        let tokens = number(part.get("tokens")?)?;
        sum = sum.checked_add(tokens)?;
        if sum > used {
            return None;
        }
        parts.push(AgentContextPartVm {
            label: label.into(),
            description: description.into(),
            tokens: format!("≈ {tokens} tokens"),
            share: format!("≈ {}% of used context", rounded(tokens, used)),
        });
    }
    let percent = window.map(|window| rounded(used, window));
    let meter_percent = percent.map(|n| n.min(100) as u32);
    let ring_background = meter_percent.map(|n| {
        format!(
            "conic-gradient(currentColor {}deg, var(--muted) 0deg)",
            n * 360 / 100
        )
    });
    let summary = match (window, percent) {
        (Some(window), Some(percent)) => format!("{percent}% · used ≈ {used} / {window} tokens"),
        _ => format!("used ≈ {used} tokens"),
    };
    Some(AgentContextVm {
        used,
        window,
        percent: percent.map(|n| format!("{n}%")),
        meter_percent,
        ring_background,
        summary,
        parts,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn metadata(window: Value) -> Value {
        json!({"used": 101, "window": window, "parts": PARTS.iter().enumerate().map(|(i, (part, _, _))|
            json!({"part": part, "tokens": if i == 0 { 34 } else if i == 4 { 67 } else { 0 }, "estimated": true})
        ).collect::<Vec<_>>()})
    }
    fn rows(context: Value) -> Vec<Map<String, Value>> {
        vec![json!({"role": "agent", "metadata": json!({"context": context, "usage": {"prompt_tokens": 9999}}).to_string()})
            .as_object().expect("row").clone()]
    }
    #[test]
    fn occupancy_is_not_cumulative_usage_and_rounds_in_rust() {
        let vm = latest(&rows(metadata(json!(300))), true).expect("context");
        assert_eq!(vm.used, 101);
        assert_eq!(vm.percent.as_deref(), Some("34%"));
        assert_eq!(vm.parts[0].share, "≈ 34% of used context");
        assert_eq!(vm.parts[4].share, "≈ 66% of used context");
        assert_eq!(vm.summary, "34% · used ≈ 101 / 300 tokens");
    }
    #[test]
    fn unknown_window_has_no_percentage_or_ring() {
        let vm = latest(&rows(metadata(Value::Null)), true).expect("context");
        assert_eq!(vm.percent, None);
        assert_eq!(vm.meter_percent, None);
        assert_eq!(vm.ring_background, None);
        assert_eq!(vm.summary, "used ≈ 101 tokens");
    }
    #[test]
    fn missing_old_or_nonreader_metadata_never_shows_a_zero_meter() {
        assert!(latest(&rows(metadata(json!(300))), false).is_none());
        let mut rows = rows(metadata(json!(300)));
        rows.push(
            json!({"role": "agent", "metadata": "{\"usage\":{\"prompt_tokens\":9999}}"})
                .as_object()
                .expect("row")
                .clone(),
        );
        assert!(latest(&rows, true).is_none());
        assert!(latest(&[], true).is_none());
    }
    #[test]
    fn malformed_parts_and_numbers_are_refused() {
        for bad in [
            json!(-1),
            json!("20"),
            json!(1.5),
            Value::Null,
            json!(MAX_SAFE + 1),
        ] {
            let mut value = metadata(json!(300));
            value["used"] = bad.clone();
            assert!(latest(&rows(value), true).is_none(), "used {bad}");
            let mut value = metadata(json!(300));
            value["parts"][0]["tokens"] = bad;
            assert!(latest(&rows(value), true).is_none());
        }
        for field in ["overflow", "duplicate", "estimated", "missing", "window"] {
            let mut value = metadata(json!(300));
            match field {
                "overflow" => value["parts"][0]["tokens"] = json!(102),
                "duplicate" => value["parts"][1]["part"] = json!("persona"),
                "estimated" => value["parts"][0]["estimated"] = json!(false),
                "missing" => {
                    value["parts"].as_array_mut().expect("parts").pop();
                }
                _ => value["window"] = json!(0),
            }
            assert!(latest(&rows(value), true).is_none(), "{field}");
        }
    }
    #[test]
    fn zero_and_over_window_counts_are_honest_and_bounded() {
        let mut zero = metadata(json!(300));
        zero["used"] = json!(0);
        for part in zero["parts"].as_array_mut().expect("parts") {
            part["tokens"] = json!(0);
        }
        assert_eq!(
            latest(&rows(zero), true).expect("zero").percent.as_deref(),
            Some("0%")
        );
        let over = latest(&rows(metadata(json!(50))), true).expect("over");
        assert_eq!(over.percent.as_deref(), Some("202%"));
        assert_eq!(over.meter_percent, Some(100));
        let mut extreme = metadata(json!(1));
        extreme["used"] = json!(MAX_SAFE);
        let extreme = latest(&rows(extreme), true).expect("extreme");
        assert_eq!(extreme.percent.as_deref(), Some("900719925474099100%"));
        assert_eq!(extreme.meter_percent, Some(100));
    }
}
