//! Last-answer occupancy, independent of cumulative billing usage (Q109-19).
//!
//! Always present on an open session (owner ruling 2026-10-27, R-HQ-85): the composer's bar
//! draws it in every view mode, reading unknown until an answer has measured it.
use std::collections::HashSet;
use std::sync::{LazyLock, Mutex};

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
/// One row for parts a newer runtime emits that this build does not name (R10921-05).
const OTHER: (&str, &str) = ("Other", "Parts this version of keeper does not name yet.");
const MAX_SAFE: u64 = 9_007_199_254_740_991;
/// Answer ids whose malformed context was already logged. The view is recomputed on every
/// replica change, so without this one bad answer would warn for as long as it is the latest.
static REFUSED: LazyLock<Mutex<HashSet<String>>> = LazyLock::new(Mutex::default);

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
    /// `None` until an answer has measured the context (or the latest one did not).
    #[ts(type = "number | null")]
    pub used: Option<u64>,
    #[ts(type = "number | null")]
    pub window: Option<u64>,
    /// The bar's compact reading: `5.3k / 1M`, `5.3k` when the window is unknown, `—` when
    /// nothing was measured.
    pub label: String,
    /// Rust-formatted rounded percentage, absent when the window or the count is unknown.
    pub percent: Option<String>,
    /// Bounded for the accessible meter; the label preserves over-window values. Absent when
    /// unknown, so no meter claims a zero nobody measured.
    pub meter_percent: Option<u32>,
    /// Always drawn: an empty ring while unknown.
    pub ring_background: String,
    pub summary: String,
    pub parts: Vec<AgentContextPartVm>,
}

/// Before any answer: what the ring's tooltip and the Details panel say.
pub const NOT_MEASURED: &str = "Context unknown: no answer has measured it yet.";
/// The latest answer carries no (readable) measurement; never an older answer's.
pub const LATEST_UNMEASURED: &str = "Context unknown: the latest answer did not measure it.";

/// The latest answer's occupancy, shown to any member in every view mode (owner ruling
/// 2026-10-27, superseding R10921-02's Verbose-only rule). Never falls back to an older answer
/// (or billing usage) when the latest lacks context: that reads unknown.
pub fn latest(rows: &[Map<String, Value>]) -> AgentContextVm {
    let Some((row, metadata)) = rows
        .iter()
        .rev()
        .filter(|row| row.get("role").and_then(Value::as_str) == Some("agent"))
        .find_map(|row| {
            let metadata = json(row, "metadata");
            metadata
                .get("kind")
                .is_none_or(Value::is_null)
                .then_some((row, metadata))
        })
    else {
        return unknown(NOT_MEASURED);
    };
    let Some(context) = metadata.get("context") else {
        return unknown(LATEST_UNMEASURED);
    };
    parse(context).unwrap_or_else(|| {
        if first_refusal(row.get("id").and_then(Value::as_str)) {
            tracing::warn!("hq: refusing malformed answer context metadata");
        }
        unknown(LATEST_UNMEASURED)
    })
}

fn unknown(summary: &str) -> AgentContextVm {
    AgentContextVm {
        used: None,
        window: None,
        label: "—".to_owned(),
        percent: None,
        meter_percent: None,
        ring_background: ring(0),
        summary: summary.to_owned(),
        parts: Vec::new(),
    }
}

fn ring(percent: u32) -> String {
    format!(
        "conic-gradient(currentColor {}deg, var(--muted) 0deg)",
        percent * 360 / 100
    )
}

/// A token count as the bar reads it: `950`, `5.3k`, `48k`, `125k`, `1M`, `1.5M`.
fn compact(n: u64) -> String {
    let wide = u128::from(n);
    let (unit, suffix) = if wide < 1_000 {
        return n.to_string();
    } else if (wide + 500) / 1_000 < 1_000 {
        // 999 950 rounds to 1000k: the next unit says it.
        (1_000u128, "k")
    } else {
        (1_000_000u128, "M")
    };
    let tenths = (wide * 10 + unit / 2) / unit;
    if tenths >= 1_000 {
        format!("{}{suffix}", (wide + unit / 2) / unit)
    } else if tenths % 10 == 0 {
        format!("{}{suffix}", tenths / 10)
    } else {
        format!("{}.{}{suffix}", tenths / 10, tenths % 10)
    }
}

/// True the first time an answer id is refused; an answer without an id always counts.
fn first_refusal(id: Option<&str>) -> bool {
    let Some(id) = id else { return true };
    REFUSED
        .lock()
        .map(|mut seen| seen.insert(id.to_owned()))
        .unwrap_or(true)
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

fn part(label: &str, description: &str, tokens: u64, used: u64) -> AgentContextPartVm {
    AgentContextPartVm {
        label: label.into(),
        description: description.into(),
        tokens: format!("≈ {tokens} tokens"),
        share: format!("≈ {}% of used context", rounded(tokens, used)),
    }
}

fn parse(context: &Value) -> Option<AgentContextVm> {
    let used = number(context.get("used")?)?;
    let window = match context.get("window")? {
        Value::Null => None,
        value => Some(number(value).filter(|n| *n > 0)?),
    };
    let mut known = [None; PARTS.len()];
    let mut other = None;
    let mut sum = 0u64;
    for raw in context.get("parts")?.as_array()? {
        let key = raw.get("part")?.as_str()?;
        if raw.get("estimated")?.as_bool() != Some(true) {
            return None;
        }
        let tokens = number(raw.get("tokens")?)?;
        sum = sum.checked_add(tokens)?;
        match PARTS.iter().position(|(name, _, _)| *name == key) {
            // A known part named twice cannot be apportioned.
            Some(i) => {
                if known[i].replace(tokens).is_some() {
                    return None;
                }
            }
            None => other = Some(other.unwrap_or(0u64).checked_add(tokens)?),
        }
    }
    if sum > used {
        return None;
    }
    let mut parts = PARTS
        .iter()
        .zip(known)
        .map(|((_, label, description), tokens)| Some(part(label, description, tokens?, used)))
        .collect::<Option<Vec<_>>>()?;
    if let Some(tokens) = other {
        parts.push(part(OTHER.0, OTHER.1, tokens, used));
    }
    let percent = window.map(|window| rounded(used, window));
    let meter_percent = percent.map(|n| n.min(100) as u32);
    let summary = match (window, percent) {
        (Some(window), Some(percent)) => format!("{percent}% · used ≈ {used} / {window} tokens"),
        _ => format!("used ≈ {used} tokens"),
    };
    Some(AgentContextVm {
        used: Some(used),
        window,
        label: match window {
            Some(window) => format!("{} / {}", compact(used), compact(window)),
            None => compact(used),
        },
        percent: percent.map(|n| format!("{n}%")),
        meter_percent,
        ring_background: ring(meter_percent.unwrap_or(0)),
        summary,
        parts,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;
    use tracing_subscriber::layer::SubscriberExt;

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
        let vm = latest(&rows(metadata(json!(300))));
        assert_eq!(vm.used, Some(101));
        assert_eq!(vm.label, "101 / 300");
        assert_eq!(vm.percent.as_deref(), Some("34%"));
        assert_eq!(vm.parts[0].share, "≈ 34% of used context");
        assert_eq!(vm.parts[4].share, "≈ 66% of used context");
        assert_eq!(vm.summary, "34% · used ≈ 101 / 300 tokens");
    }
    #[test]
    fn unknown_window_has_no_percentage_or_ring() {
        let vm = latest(&rows(metadata(Value::Null)));
        assert_eq!(vm.percent, None);
        assert_eq!(vm.meter_percent, None);
        assert_eq!(
            vm.ring_background,
            "conic-gradient(currentColor 0deg, var(--muted) 0deg)"
        );
        assert_eq!(vm.label, "101");
        assert_eq!(vm.summary, "used ≈ 101 tokens");
    }
    #[test]
    fn before_any_answer_and_after_an_unmeasured_one_the_ring_reads_unknown_never_an_older_count() {
        let before = latest(&[]);
        assert_eq!(before.used, None);
        assert_eq!(before.label, "—");
        assert_eq!(
            before.meter_percent, None,
            "no meter claims a measured zero"
        );
        assert_eq!(before.summary, NOT_MEASURED);
        assert!(before.parts.is_empty());
        let mut rows = rows(metadata(json!(300)));
        rows.push(
            json!({"role": "agent", "metadata": "{\"usage\":{\"prompt_tokens\":9999}}"})
                .as_object()
                .expect("row")
                .clone(),
        );
        let unmeasured = latest(&rows);
        assert_eq!(unmeasured.used, None, "never the older answer's count");
        assert_eq!(unmeasured.summary, LATEST_UNMEASURED);
    }
    #[test]
    fn the_bar_reads_a_compact_count_over_the_window() {
        for (n, shown) in [
            (0, "0"),
            (950, "950"),
            (5347, "5.3k"),
            (48_000, "48k"),
            (99_960, "100k"),
            (125_000, "125k"),
            (999_950, "1M"),
            (1_048_576, "1M"),
            (1_500_000, "1.5M"),
            (200_000_000, "200M"),
        ] {
            assert_eq!(compact(n), shown, "{n}");
        }
        // `.11`'s answer (owner report 2026-10-27): 5347 of a 1,048,576 window.
        let mut value = metadata(json!(1_048_576));
        value["used"] = json!(5347);
        value["parts"][4]["tokens"] = json!(5347 - 34);
        let vm = latest(&rows(value));
        assert_eq!(vm.label, "5.3k / 1M");
        assert_eq!(vm.percent.as_deref(), Some("1%"));
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
            assert!(latest(&rows(value)).used.is_none(), "used {bad}");
            let mut value = metadata(json!(300));
            value["parts"][0]["tokens"] = bad;
            assert!(latest(&rows(value)).used.is_none());
        }
        let extra = |part: &str, tokens: u64, estimated: bool| json!({"part": part, "tokens": tokens, "estimated": estimated});
        for field in [
            "overflow",
            "duplicate",
            "estimated",
            "missing",
            "window",
            "nameless",
            "unknown estimated",
            "unknown overflow",
            "unknown malformed",
        ] {
            let mut value = metadata(json!(300));
            let parts = value["parts"].as_array_mut().expect("parts");
            match field {
                "overflow" => parts[0]["tokens"] = json!(102),
                // Every known part present, one of them twice: only the duplicate guard refuses.
                "duplicate" => parts.push(extra("persona", 0, true)),
                "estimated" => parts[0]["estimated"] = json!(false),
                "missing" => {
                    parts.pop();
                }
                "nameless" => parts.push(json!({"tokens": 0, "estimated": true})),
                "unknown estimated" => parts.push(extra("skills", 0, false)),
                "unknown overflow" => parts.push(extra("skills", 1, true)),
                // A malformed unknown part refuses the whole context, never a silent skip.
                "unknown malformed" => {
                    parts.push(json!({"part": "skills", "tokens": -1, "estimated": true}))
                }
                _ => value["window"] = json!(0),
            }
            assert!(latest(&rows(value)).used.is_none(), "{field}");
        }
    }
    #[test]
    fn unknown_new_parts_become_one_other_row_and_keep_the_meter() {
        let mut value = metadata(json!(300));
        value["used"] = json!(126);
        for (part, tokens) in [("skills", 20), ("cache", 5)] {
            value["parts"]
                .as_array_mut()
                .expect("parts")
                .push(json!({"part": part, "tokens": tokens, "estimated": true}));
        }
        let vm = latest(&rows(value));
        assert_eq!(vm.summary, "42% · used ≈ 126 / 300 tokens");
        assert_eq!(vm.parts.len(), PARTS.len() + 1);
        assert_eq!(vm.parts[0].label, "System and persona");
        let other = vm.parts.last().expect("other");
        assert_eq!(other.label, "Other");
        assert_eq!(other.tokens, "≈ 25 tokens");
        assert_eq!(other.share, "≈ 20% of used context");
        assert!(vm.parts.iter().all(|part| part.tokens.starts_with("≈ ")));
        let known = latest(&rows(metadata(json!(300))));
        assert!(known.parts.iter().all(|part| part.label != "Other"));
    }
    struct Count(Arc<AtomicUsize>);
    impl<S: tracing::Subscriber> tracing_subscriber::Layer<S> for Count {
        fn on_event(&self, _: &tracing::Event<'_>, _: tracing_subscriber::layer::Context<'_, S>) {
            self.0.fetch_add(1, Ordering::SeqCst);
        }
    }
    #[test]
    fn a_malformed_answer_is_logged_once_per_answer_id() {
        let mut bad = metadata(json!(300));
        bad["used"] = json!(-1);
        let answer = |id: &str| {
            let mut rows = rows(bad.clone());
            rows[0].insert("id".into(), json!(id));
            rows
        };
        let (first, second) = (answer("refused-once-a"), answer("refused-once-b"));
        let warnings = Arc::new(AtomicUsize::new(0));
        let subscriber = tracing_subscriber::registry().with(Count(warnings.clone()));
        tracing::subscriber::with_default(subscriber, || {
            for _ in 0..3 {
                assert!(latest(&first).used.is_none());
            }
            assert!(latest(&second).used.is_none());
            assert!(latest(&first).used.is_none());
        });
        assert_eq!(warnings.load(Ordering::SeqCst), 2);
    }
    #[test]
    fn zero_and_over_window_counts_are_honest_and_bounded() {
        let mut zero = metadata(json!(300));
        zero["used"] = json!(0);
        for part in zero["parts"].as_array_mut().expect("parts") {
            part["tokens"] = json!(0);
        }
        assert_eq!(latest(&rows(zero)).percent.as_deref(), Some("0%"));
        let over = latest(&rows(metadata(json!(50))));
        assert_eq!(over.percent.as_deref(), Some("202%"));
        assert_eq!(over.meter_percent, Some(100));
        let mut extreme = metadata(json!(1));
        extreme["used"] = json!(MAX_SAFE);
        let extreme = latest(&rows(extreme));
        assert_eq!(extreme.percent.as_deref(), Some("900719925474099100%"));
        assert_eq!(extreme.meter_percent, Some(100));
    }
}
