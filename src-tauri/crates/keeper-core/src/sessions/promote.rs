//! The README's `## Promote` table, parsed and spliced (FR-243, FR-244, AD-108).
//!
//! The table IS the promotion contract — the zone's own README says so:
//! "promotion = copy under a stable name, listed here". This module reads the
//! documented shape and **preserves everything it does not understand**: an
//! unparseable row is carried verbatim as [`PromoteRow::Unreadable`], surfaced
//! in the panel, and never rewritten (PRD §8). Writes are span-splices over
//! the original bytes under the same discipline as frontmatter (NFR-39): a
//! row update touches its row, an append touches the table's end, and every
//! other byte of the README survives untouched.
//!
//! The documented shape, from `_template/README.md` on both live drives:
//!
//! ```markdown
//! ## Promote
//!
//! | workspace | → artifacts | note |
//! | --------- | ----------- | ---- |
//! | workspace/draft.md | artifacts/report.md | weekly report |
//! ```

/// One row of the promote table.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PromoteRow {
    /// A row in the documented three-column shape.
    Entry {
        /// The `workspace/…` source, verbatim as written.
        source: String,
        /// The `artifacts/…` target, verbatim as written.
        target: String,
        /// The free-text note column, verbatim (may be empty).
        note: String,
    },
    /// A pipe-delimited line the parser could not read as three columns.
    /// Preserved so the panel can show it and a rewrite can never eat it.
    Unreadable {
        /// The raw line, byte-for-byte.
        raw: String,
        /// 0-based line number in the README, for the panel's located note.
        line: usize,
    },
}

/// The parsed table: rows plus the byte spans a splice needs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PromoteTable {
    /// Data rows, in file order. Header and delimiter rows are structure, not
    /// data, and are not carried here.
    pub rows: Vec<PromoteRow>,
    /// Byte offset just past the last row (or past the delimiter when the
    /// table is empty) — where an appended row is spliced in.
    pub append_at: usize,
    /// Byte span `(start, end)` of each data row's line INCLUDING its
    /// terminator, parallel to `rows` — what a row update replaces.
    pub row_spans: Vec<(usize, usize)>,
}

/// Find and parse the `## Promote` table in a README body. `None` when the
/// section or its table is absent — which the panel reports as "no promote
/// table", never invents (files are truth).
pub fn parse(body: &str) -> Option<PromoteTable> {
    let section_start = heading_offset(body, "## Promote")?;
    let after_heading = &body[section_start..];

    // Walk lines after the heading until the next `## ` heading or EOF,
    // looking for the table: a header row, a delimiter row, then data rows.
    let mut offset = section_start;
    let mut lines = after_heading.split_inclusive('\n');
    lines.next().map(|l| offset += l.len())?; // consume the heading line

    let mut rows = Vec::new();
    let mut row_spans = Vec::new();
    let mut append_at = None;
    let mut header_seen = false;
    let mut line_no = body[..offset].matches('\n').count();

    for line in lines {
        let start = offset;
        offset += line.len();
        line_no += 1;
        let trimmed = line.trim_end_matches(['\n', '\r']).trim();
        if trimmed.starts_with("## ") {
            break;
        }
        if !trimmed.starts_with('|') {
            // Prose between the heading and the table (the template carries an
            // HTML comment there) — or the blank line after the table, which
            // ends it once rows have been seen.
            if append_at.is_some() && trimmed.is_empty() {
                break;
            }
            continue;
        }
        if !header_seen {
            header_seen = true; // the `| workspace | → artifacts | note |` row
            continue;
        }
        if append_at.is_none() && is_delimiter_row(trimmed) {
            append_at = Some(offset);
            continue;
        }
        // A data row.
        append_at = Some(offset);
        match split_row(trimmed) {
            Some((source, target, note)) => {
                rows.push(PromoteRow::Entry {
                    source,
                    target,
                    note,
                });
            }
            None => rows.push(PromoteRow::Unreadable {
                raw: trimmed.to_owned(),
                line: line_no,
            }),
        }
        row_spans.push((start, offset));
    }

    append_at.map(|append_at| PromoteTable {
        rows,
        append_at,
        row_spans,
    })
}

/// Render one data row in the canonical spelling the writer uses.
pub fn render_row(source: &str, target: &str, note: &str) -> String {
    format!("| {source} | {target} | {note} |\n")
}

/// The body with one row appended to the table (or updated in place when a
/// row with the same source already exists). Everything outside the touched
/// span is byte-identical (NFR-39). `None` when the body has no table to
/// write into — the caller decides whether to create the section, and that is
/// a different, louder act.
pub fn upsert_row(body: &str, source: &str, target: &str, note: &str) -> Option<String> {
    let table = parse(body)?;
    let rendered = render_row(source, target, note);
    for (row, span) in table.rows.iter().zip(&table.row_spans) {
        if let PromoteRow::Entry { source: s, .. } = row {
            if s == source {
                let mut out = String::with_capacity(body.len() + rendered.len());
                out.push_str(&body[..span.0]);
                out.push_str(&rendered);
                out.push_str(&body[span.1..]);
                return Some(out);
            }
        }
    }
    let mut out = String::with_capacity(body.len() + rendered.len());
    out.push_str(&body[..table.append_at]);
    out.push_str(&rendered);
    out.push_str(&body[table.append_at..]);
    Some(out)
}

/// Byte offset of a `## ` heading line in a body, at a line start.
fn heading_offset(body: &str, heading: &str) -> Option<usize> {
    let mut offset = 0;
    for line in body.split_inclusive('\n') {
        let trimmed = line.trim_end_matches(['\n', '\r']);
        if trimmed.trim() == heading {
            return Some(offset);
        }
        offset += line.len();
    }
    None
}

/// A `| --- | --- | --- |` separator, any dash count, optional colons.
fn is_delimiter_row(line: &str) -> bool {
    let inner = line.trim_matches('|');
    !inner.is_empty()
        && inner
            .split('|')
            .all(|cell| cell.trim().chars().all(|c| matches!(c, '-' | ':')))
        && inner.contains('-')
}

/// Split a `| a | b | c |` row into exactly three trimmed cells.
fn split_row(line: &str) -> Option<(String, String, String)> {
    let inner = line.strip_prefix('|')?.strip_suffix('|')?;
    let cells: Vec<&str> = inner.split('|').map(str::trim).collect();
    if cells.len() != 3 {
        return None;
    }
    Some((
        cells[0].to_owned(),
        cells[1].to_owned(),
        cells[2].to_owned(),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    const TEMPLATE_TAIL: &str = "\
# a session

## Log

### 2026-08-12 — began

## Promote

<!-- promotion notes -->

| workspace | → artifacts | note |
| --------- | ----------- | ---- |
";

    /// The template's own empty table parses to zero rows with a valid append
    /// point — the state every fresh session starts in.
    #[test]
    fn the_template_table_parses_empty() {
        let table = parse(TEMPLATE_TAIL).expect("the template has a table");
        assert!(table.rows.is_empty());
        assert_eq!(table.append_at, TEMPLATE_TAIL.len());
    }

    /// Rows parse in order; a malformed line is carried, located, and NOT
    /// dropped — the panel shows it, the writer never touches it (PRD §8).
    #[test]
    fn rows_parse_and_a_malformed_line_is_preserved_not_dropped() {
        let body = format!(
            "{TEMPLATE_TAIL}| workspace/a.md | artifacts/a.md | first |\n| broken row without cells\n| workspace/b.csv | artifacts/b.csv | |\n"
        );
        let table = parse(&body).expect("a table");
        assert_eq!(table.rows.len(), 3);
        assert_eq!(
            table.rows[0],
            PromoteRow::Entry {
                source: "workspace/a.md".into(),
                target: "artifacts/a.md".into(),
                note: "first".into()
            }
        );
        assert!(matches!(&table.rows[1], PromoteRow::Unreadable { raw, .. }
            if raw == "| broken row without cells"));
        assert!(matches!(&table.rows[2], PromoteRow::Entry { note, .. } if note.is_empty()));
    }

    /// An upsert re-promotes under the same source in place; a new source
    /// appends. Every byte outside the touched span survives — asserted by
    /// reconstruction, not by trust (NFR-39).
    #[test]
    fn upsert_replaces_by_source_or_appends_and_touches_nothing_else() {
        let body = format!(
            "{TEMPLATE_TAIL}| workspace/a.md | artifacts/a.md | v1 |\n\n## After\n\ntext\n"
        );
        let updated = upsert_row(&body, "workspace/a.md", "artifacts/a.md", "v2").expect("upserts");
        assert!(updated.contains("| workspace/a.md | artifacts/a.md | v2 |\n"));
        assert!(!updated.contains("| v1 |"));
        assert!(
            updated.ends_with("## After\n\ntext\n"),
            "the tail is untouched"
        );

        let appended =
            upsert_row(&updated, "workspace/b.md", "artifacts/b.md", "").expect("appends");
        let a_at = appended.find("workspace/a.md").expect("a stays");
        let b_at = appended.find("workspace/b.md").expect("b lands");
        assert!(a_at < b_at, "appends go after existing rows");
    }

    /// A README with no Promote section refuses rather than inventing one —
    /// creating the section is a different, louder act the caller owns.
    #[test]
    fn a_body_without_a_table_is_a_none_not_a_scaffold() {
        assert_eq!(parse("# nothing here\n"), None);
        assert_eq!(
            upsert_row("# nothing\n", "workspace/x", "artifacts/x", ""),
            None
        );
    }
}
