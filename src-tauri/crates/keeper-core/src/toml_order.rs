//! keeper's TOML files, read in key order.
//!
//! `toml::Table` iterates in document order when any crate in the build turns
//! on `toml`'s `preserve_order` (keeper-ported does, for BMAD's files) and
//! alphabetically otherwise. Every keeper file was written against the
//! alphabetical reading — which of two spellings of one setting wins, which
//! unknown key a refusal names, which error of a file with two is reported, the
//! bytes a rewritten file comes out as — so keeper reads its own files here, in
//! key order, whatever the build (R170). Only `keeper_ported::bmad` reads in
//! document order. `keeper_sync::toml_order` is the same reader for the crates
//! below keeper-core.

use serde::Deserialize;
use toml::de::{DeTable, DeValue, Deserializer};

/// `toml::from_str`, with every table of the document visited in key order:
/// a `toml::Table` comes out with its keys inserted in order, and a struct
/// sees its fields in order, so its first error is the one an alphabetical
/// reading meets first. Errors keep their spans and their snippet of `text`.
pub fn from_str<'a, T: Deserialize<'a>>(text: &'a str) -> Result<T, toml::de::Error> {
    let mut root = DeTable::parse(text)?;
    sort_table(root.get_mut());
    T::deserialize(Deserializer::from(root)).map_err(|mut error| {
        error.set_input(Some(text));
        error
    })
}

fn sort_table(table: &mut DeTable<'_>) {
    let mut entries: Vec<_> = std::mem::take(table).into_iter().collect();
    entries.sort_by(|a, b| a.0.cmp(&b.0));
    for (_, value) in &mut entries {
        sort_value(value.get_mut());
    }
    *table = entries.into_iter().collect();
}

fn sort_value(value: &mut DeValue<'_>) {
    match value {
        DeValue::Table(table) => sort_table(table),
        DeValue::Array(items) => {
            for item in items.iter_mut() {
                sort_value(item.get_mut());
            }
        }
        _ => {}
    }
}

/// `table` with every table inside it in key order, for a table keeper built
/// itself before it is written.
pub fn sorted_table(table: &toml::Table) -> toml::Table {
    let mut entries: Vec<(&String, &toml::Value)> = table.iter().collect();
    entries.sort_by(|a, b| a.0.cmp(b.0));
    entries
        .into_iter()
        .map(|(key, value)| (key.clone(), sorted_value(value)))
        .collect()
}

fn sorted_value(value: &toml::Value) -> toml::Value {
    match value {
        toml::Value::Table(table) => toml::Value::Table(sorted_table(table)),
        toml::Value::Array(items) => toml::Value::Array(items.iter().map(sorted_value).collect()),
        other => other.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Debug, Deserialize)]
    #[serde(deny_unknown_fields)]
    #[allow(dead_code)]
    struct Known {
        known: Option<i64>,
    }

    /// keeper-core always links keeper-ported, so `toml` keeps document order
    /// here: the key order below is this reader's doing, not the map's.
    #[test]
    fn a_document_is_read_in_key_order_whatever_order_it_is_written_in() {
        let text = "zeta = 1\nalpha = { z = 1, a = [{ y = 1, b = 2 }] }\n";
        let table: toml::Table = from_str(text).expect("parses");
        assert_eq!(table.keys().collect::<Vec<_>>(), ["alpha", "zeta"]);
        let inner = table["alpha"].as_table().expect("a table");
        assert_eq!(inner.keys().collect::<Vec<_>>(), ["a", "z"]);
        let item = inner["a"][0].as_table().expect("a table");
        assert_eq!(item.keys().collect::<Vec<_>>(), ["b", "y"]);

        let error = from_str::<Known>("zeta = 1\nalpha = 2\n").expect_err("refused");
        assert!(error.message().contains("`alpha`"), "{error}");
        assert!(
            error.span().is_some_and(|span| span.start >= 9),
            "{error:?}"
        );
        assert!(
            error.to_string().contains("alpha = 2"),
            "the snippet: {error}"
        );
    }

    /// Each keeper file with two unknown keys names the first in key order,
    /// as it did before `toml` kept document order (R170).
    #[test]
    fn every_keeper_file_names_its_first_unknown_key_in_key_order() {
        let two = "zeta = 1\nalpha = 2\n";
        assert_eq!(
            crate::agents::drive::parse(two)
                .map(|_| ())
                .map_err(|e| e.to_string()),
            Err("_drive.toml has `alpha`, which is not one of its keys.".to_owned())
        );
        let session = crate::agents::session::parse_session_agent_toml(two)
            .map(|_| ())
            .expect_err("refused");
        assert!(
            matches!(&session, crate::agents::session::SessionRefusal::UnknownKey { key } if key == "alpha"),
            "{session:?}"
        );
        let block = crate::notes::media_block::parse(two)
            .map(|_| ())
            .expect_err("refused");
        assert!(
            matches!(&block, crate::notes::media_block::BlockRefusal::UnknownKey { key } if key == "alpha"),
            "{block:?}"
        );
        let agentd = crate::agents::agentd::AgentdConfig::parse(&format!("version = 1\n{two}"))
            .map(|_| ())
            .expect_err("refused")
            .to_string();
        assert!(agentd.contains("`alpha`"), "{agentd}");
        let descriptor = crate::org_account::descriptor::parse_toml(two).expect_err("refused");
        assert!(
            descriptor.message.contains("`alpha`"),
            "{}",
            descriptor.message
        );
        assert_eq!(descriptor.line, Some(2), "the line of the key named");
        let models = crate::transcription::models::ModelSet::from_toml(two)
            .map(|_| ())
            .expect_err("refused");
        assert!(format!("{models:?}").contains("`alpha`"), "{models:?}");
    }
}
