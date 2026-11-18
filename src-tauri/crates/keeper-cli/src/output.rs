//! The only writer of stdout (keeper-syncd's `Printer` convention): a verb's
//! answer, as one JSON document under `--json` or as its human sentence.

use std::io::Write as _;

use serde_json::Value;

pub struct Printer {
    json: bool,
}

impl Printer {
    pub fn new(json: bool) -> Self {
        Self { json }
    }

    /// Print `answer`: under `--json` the document itself, on one line (the
    /// graph's answers are the same line the headless graph consumer has
    /// always printed, byte for byte, which keeper-hq's harness parses);
    /// otherwise the sentence `human` renders from it. A closed stdout is not
    /// a panic: the exit code still says how the command went.
    pub fn answer(&self, answer: &Value, human: impl FnOnce(&Value) -> String) {
        let text = if self.json {
            answer.to_string()
        } else {
            human(answer)
        };
        let mut out = std::io::stdout().lock();
        let _ = writeln!(out, "{text}").and_then(|()| out.flush());
    }
}
