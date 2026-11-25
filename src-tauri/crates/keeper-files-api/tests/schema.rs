use std::collections::{BTreeMap, BTreeSet};

use serde::Deserialize;
use serde_json::Value;

const SYNCULAR_JSON: &str = include_str!("../../../../syncular/syncular.json");
const SYNCULAR_IR_JSON: &str = include_str!("../../../../syncular/syncular.ir.json");

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Ir {
    tables: Vec<IrTable>,
    subscriptions: Vec<IrSubscription>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct IrTable {
    name: String,
    primary_key: String,
    columns: Vec<IrColumn>,
    scopes: Vec<IrScope>,
}

#[derive(Deserialize)]
struct IrColumn {
    name: String,
    #[serde(rename = "type")]
    col_type: String,
    #[allow(dead_code)]
    nullable: bool,
}

#[derive(Deserialize)]
struct IrScope {
    pattern: String,
    variable: String,
    column: String,
}

#[derive(Deserialize)]
struct IrSubscription {
    name: String,
    scopes: Vec<IrSubscriptionScope>,
}

#[derive(Deserialize)]
struct IrSubscriptionScope {
    variable: String,
}

fn parse_ir() -> Ir {
    serde_json::from_str(SYNCULAR_IR_JSON).expect("valid syncular.ir.json")
}

fn parse_manifest() -> Value {
    serde_json::from_str(SYNCULAR_JSON).expect("valid syncular.json")
}

fn table_by_name<'a>(ir: &'a Ir, name: &str) -> &'a IrTable {
    ir.tables
        .iter()
        .find(|table| table.name == name)
        .unwrap_or_else(|| panic!("missing table '{name}' in syncular.ir.json"))
}

fn column_names(table: &IrTable) -> BTreeSet<&str> {
    table.columns.iter().map(|col| col.name.as_str()).collect()
}

fn is_snake_ident(name: &str) -> bool {
    let mut bytes = name.bytes();
    bytes.next().is_some_and(|b| b.is_ascii_lowercase())
        && bytes.all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_')
}

#[test]
fn six_files_tables_and_exact_scopes() {
    let ir = parse_ir();
    let expected: BTreeMap<&str, (&str, &str)> = BTreeMap::from([
        ("files_entries", ("files_drive", "drive_key")),
        ("files_entry_sources", ("files_sources", "domain")),
        ("files_events", ("files_page", "page_key")),
        ("files_event_ops", ("files_page", "page_key")),
        ("files_head", ("files_domain", "domain")),
        ("files_receipts", ("files_client", "client_scope")),
    ]);

    let actual_files_tables: BTreeSet<&str> = ir
        .tables
        .iter()
        .filter(|table| table.name.starts_with("files_"))
        .map(|table| table.name.as_str())
        .collect();
    let expected_names: BTreeSet<&str> = expected.keys().copied().collect();
    assert_eq!(actual_files_tables, expected_names);

    for (table_name, (exp_var, exp_col)) in expected {
        let table = table_by_name(&ir, table_name);
        assert_eq!(
            table.scopes.len(),
            1,
            "{table_name} must have exactly one scope"
        );
        let scope = &table.scopes[0];
        assert_eq!(scope.variable, exp_var, "{table_name} scope variable");
        assert_eq!(scope.column, exp_col, "{table_name} scope column");
        assert_eq!(
            scope.pattern,
            format!("{exp_var}:{{{exp_var}}}"),
            "{table_name} scope pattern"
        );
    }
}

#[test]
fn no_wildcards_in_syncular_manifest() {
    let manifest = parse_manifest();

    for table in manifest["tables"].as_array().expect("manifest.tables") {
        let table_name = table["name"].as_str().expect("table.name");
        for scope in table["scopes"].as_array().expect("table.scopes") {
            let pattern = scope
                .as_str()
                .or_else(|| scope.get("pattern").and_then(Value::as_str))
                .unwrap_or_else(|| panic!("table '{table_name}' scope missing pattern"));
            assert!(
                !pattern.contains('*'),
                "table '{table_name}' scope pattern contains '*': {pattern}"
            );
        }
    }

    for sub in manifest["subscriptions"]
        .as_array()
        .expect("manifest.subscriptions")
    {
        let sub_name = sub["name"].as_str().expect("subscription.name");
        for (var, val) in sub["scopes"].as_object().expect("subscription.scopes") {
            assert!(
                !var.contains('*'),
                "subscription '{sub_name}' scope variable contains '*': {var}"
            );
            let val_str = val.as_str().expect("subscription scope value string");
            assert!(
                !val_str.contains('*'),
                "subscription '{sub_name}' scope '{var}' value contains '*': {val_str}"
            );
        }
    }
}

#[test]
fn no_blob_ref_columns_on_files_tables() {
    let ir = parse_ir();
    for table in ir.tables.iter().filter(|t| t.name.starts_with("files_")) {
        for col in &table.columns {
            assert_ne!(
                col.col_type, "blob_ref",
                "{}.{} must not use blob_ref",
                table.name, col.name
            );
        }
    }
}

#[test]
fn files_identifiers_and_primary_keys() {
    let ir = parse_ir();
    for table in ir.tables.iter().filter(|t| t.name.starts_with("files_")) {
        assert!(
            is_snake_ident(&table.name),
            "table name '{}' must match ^[a-z][a-z0-9_]*$",
            table.name
        );
        assert_eq!(
            table.primary_key, "id",
            "table '{}' primaryKey must be 'id'",
            table.name
        );
        for col in &table.columns {
            assert!(
                is_snake_ident(&col.name),
                "{}.{} must match ^[a-z][a-z0-9_]*$",
                table.name,
                col.name
            );
        }
        for scope in &table.scopes {
            assert!(
                is_snake_ident(&scope.variable),
                "{} scope variable '{}' must match ^[a-z][a-z0-9_]*$",
                table.name,
                scope.variable
            );
        }
    }
}

#[test]
fn kg_twin_columns_preserved() {
    let ir = parse_ir();

    let kg_events = column_names(table_by_name(&ir, "kg_events"));
    let files_events = column_names(table_by_name(&ir, "files_events"));
    let missing_events: Vec<&str> = kg_events
        .into_iter()
        .filter(|col| !matches!(*col, "graph" | "graph_seq"))
        .filter(|col| !files_events.contains(col))
        .collect();
    assert!(
        missing_events.is_empty(),
        "files_events missing kg_events columns: {missing_events:?}"
    );

    let kg_ops = column_names(table_by_name(&ir, "kg_event_ops"));
    let files_ops = column_names(table_by_name(&ir, "files_event_ops"));
    let missing_ops: Vec<&str> = kg_ops
        .into_iter()
        .filter(|col| !matches!(*col, "graph" | "graph_seq"))
        .filter(|col| !files_ops.contains(col))
        .collect();
    assert!(
        missing_ops.is_empty(),
        "files_event_ops missing kg_event_ops columns: {missing_ops:?}"
    );

    let kg_receipts = column_names(table_by_name(&ir, "kg_receipts"));
    let files_receipts = column_names(table_by_name(&ir, "files_receipts"));
    let missing_receipts: Vec<&str> = kg_receipts
        .into_iter()
        .filter(|col| *col != "graph_seq")
        .filter(|col| !files_receipts.contains(col))
        .collect();
    assert!(
        missing_receipts.is_empty(),
        "files_receipts missing kg_receipts columns: {missing_receipts:?}"
    );
}

#[test]
fn reserved_governance_and_op_columns_present() {
    let ir = parse_ir();

    let files_events = column_names(table_by_name(&ir, "files_events"));
    for required in [
        "origin_actor",
        "origin_chain",
        "approval_basis",
        "proposal_id",
        "change_set_id",
        "job_id",
        "job_ordinal",
        "job_attempt_id",
        "context_id",
        "approval_id",
    ] {
        assert!(
            files_events.contains(required),
            "files_events missing required column '{required}'"
        );
    }

    let files_ops = column_names(table_by_name(&ir, "files_event_ops"));
    for required in ["operation", "entity_type"] {
        assert!(
            files_ops.contains(required),
            "files_event_ops missing required column '{required}'"
        );
    }
}

#[test]
fn deferred_tables_and_facts_scopes_absent() {
    let ir = parse_ir();
    let table_names: BTreeSet<&str> = ir.tables.iter().map(|t| t.name.as_str()).collect();
    for forbidden in [
        "files_facts",
        "files_proposals",
        "files_reviews",
        "files_proposal_reviewers",
    ] {
        assert!(
            !table_names.contains(forbidden),
            "table '{forbidden}' must not exist yet"
        );
    }

    for table in &ir.tables {
        for scope in &table.scopes {
            assert_ne!(
                scope.variable, "files_facts",
                "table '{}' must not declare files_facts scope",
                table.name
            );
        }
    }

    for sub in &ir.subscriptions {
        assert!(
            !sub.name.to_ascii_lowercase().contains("files_facts")
                && !sub.name.to_ascii_lowercase().contains("filesfacts"),
            "subscription '{}' must not name files_facts",
            sub.name
        );
        for scope in &sub.scopes {
            assert_ne!(
                scope.variable, "files_facts",
                "subscription '{}' must not use files_facts scope variable",
                sub.name
            );
        }
    }
}

#[test]
fn no_path_columns_and_entry_sources_scope() {
    let ir = parse_ir();
    for table in ir.tables.iter().filter(|t| t.name.starts_with("files_")) {
        let cols = column_names(table);
        assert!(
            !cols.contains("path"),
            "table '{}' must not carry a 'path' column",
            table.name
        );
    }

    let entries = column_names(table_by_name(&ir, "files_entries"));
    for required in ["parent_id", "name", "name_fold"] {
        assert!(
            entries.contains(required),
            "files_entries missing column '{required}'"
        );
    }

    let entry_sources = table_by_name(&ir, "files_entry_sources");
    assert_eq!(entry_sources.scopes.len(), 1);
    assert_eq!(entry_sources.scopes[0].variable, "files_sources");
}
