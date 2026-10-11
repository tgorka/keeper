// Ported from BMAD-METHOD `src/scripts/memlog.py` at tag v6.12.0
// (05bfbd46d00766ec88eb9b42e76be2c575d64d7b). MIT, Copyright (c) 2025 BMad
// Code, LLC; see `UPSTREAM.md`. The commands became functions over the file's
// text; reading, the temp file, the fsync and the rename are the caller's.

//! BMAD's memory log: a `.memlog.md` file of `key: value` frontmatter and a
//! flat, append-only list of one-line entries.
//!
//! Every write is a whole new text the caller replaces the file with; the
//! clock is a parameter (`now`, upstream's local `%Y-%m-%dT%H:%M`).

use std::fmt;

use super::py;

/// The memlog's file name inside a run folder.
pub const MEMLOG: &str = ".memlog.md";

/// Why a memlog command refused. `Display` is upstream's sentence: the
/// `ValueError` text for an unreadable file, the CLI's `error:` line otherwise.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MemlogError {
    NoFrontmatter,
    Unterminated,
    Exists { path: String },
    Field { pair: String },
}

impl fmt::Display for MemlogError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoFrontmatter => f.write_str(".memlog.md has no frontmatter"),
            Self::Unterminated => f.write_str(".memlog.md frontmatter is not terminated"),
            Self::Exists { path } => write!(
                f,
                "error: {path} already exists; use append/set to update it"
            ),
            Self::Field { pair } => {
                write!(
                    f,
                    "error: --field expects key=value, got {}",
                    py::repr(pair)
                )
            }
        }
    }
}

impl std::error::Error for MemlogError {}

/// Where a command writes: a run folder (the memlog is
/// `<workspace>/.memlog.md`) or the memlog file itself.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Target<'a> {
    Workspace(&'a str),
    Path(&'a str),
}

impl Target<'_> {
    /// The memlog's path as `memlog.py`'s `resolve` prints it: `str(Path(p))`
    /// or `str(Path(w) / ".memlog.md")`, with `pathlib`'s POSIX spelling —
    /// separators collapsed (two leading ones kept), `.` parts dropped, no
    /// trailing slash, `..` kept as written. Purely lexical: containment is
    /// the caller's.
    pub fn file(self) -> String {
        match self {
            Self::Path(path) => posix_path(path),
            Self::Workspace(dir) => match posix_path(dir).as_str() {
                "." => MEMLOG.to_owned(),
                root if root.ends_with('/') => format!("{root}{MEMLOG}"),
                dir => format!("{dir}/{MEMLOG}"),
            },
        }
    }
}

/// `str(PurePosixPath(path))`.
fn posix_path(path: &str) -> String {
    let slashes = path.len() - path.trim_start_matches('/').len();
    let root = match slashes {
        0 => "",
        2 => "//",
        _ => "/",
    };
    let parts: Vec<&str> = path
        .split('/')
        .filter(|part| !part.is_empty() && *part != ".")
        .collect();
    let joined = format!("{root}{}", parts.join("/"));
    if joined.is_empty() {
        ".".to_owned()
    } else {
        joined
    }
}

/// The frontmatter in source order, with a Python dict's semantics: setting a
/// key it has keeps that key's place.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Meta(Vec<(String, String)>);

impl Meta {
    pub fn get(&self, key: &str) -> Option<&str> {
        self.0
            .iter()
            .find(|(k, _)| k == key)
            .map(|(_, value)| value.as_str())
    }

    pub fn set(&mut self, key: &str, value: &str) {
        match self.0.iter_mut().find(|(k, _)| k == key) {
            Some((_, existing)) => value.clone_into(existing),
            None => self.0.push((key.to_owned(), value.to_owned())),
        }
    }

    fn remove(&mut self, key: &str) {
        self.0.retain(|(k, _)| k != key);
    }

    /// The keys in order.
    pub fn keys(&self) -> impl Iterator<Item = &str> {
        self.0.iter().map(|(key, _)| key.as_str())
    }
}

/// The frontmatter and the body. The closing fence is the first line that is
/// exactly `---`, so a `---` inside a value never ends the frontmatter.
pub fn split(text: &str) -> Result<(Meta, String), MemlogError> {
    let lines = py::splitlines(text);
    if lines.first() != Some(&"---") {
        return Err(MemlogError::NoFrontmatter);
    }
    let end = lines
        .iter()
        .skip(1)
        .position(|line| *line == "---")
        .map(|at| at + 1)
        .ok_or(MemlogError::Unterminated)?;
    let mut meta = Meta::default();
    for line in &lines[1..end] {
        if let Some((key, value)) = line.split_once(':') {
            meta.set(py::strip(key), py::strip(value));
        }
    }
    let body = lines[end + 1..].join("\n");
    Ok((meta, body.trim_start_matches('\n').to_owned()))
}

/// The file's text. A value's line breaks become spaces, so a multi-line
/// field cannot break the fence on the next read.
pub fn render(meta: &Meta, body: &str) -> String {
    let fields: Vec<String> = meta
        .0
        .iter()
        .map(|(key, value)| format!("{key}: {}", py::splitlines(value).join(" ")))
        .collect();
    format!(
        "---\n{}\n---\n\n{}\n",
        fields.join("\n"),
        body.trim_end_matches('\n')
    )
}

/// Stamp `updated` and keep it last.
pub fn touch(meta: &mut Meta, now: &str) {
    meta.remove("updated");
    meta.set("updated", now);
}

/// The number of entries: body lines that start `- `.
pub fn entry_count(body: &str) -> usize {
    py::splitlines(body)
        .into_iter()
        .filter(|line| line.starts_with("- "))
        .count()
}

/// The one line of JSON every command echoes, so the caller never re-reads
/// the file to know where it stands.
pub fn ack(path: &str, body: &str) -> String {
    format!(
        "{{\"ok\": true, \"memlog\": {}, \"entries\": {}}}",
        py::json_string(path, true),
        entry_count(body)
    )
}

/// A command's result: the file's new text and its body, for [`ack`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Written {
    pub text: String,
    pub body: String,
}

/// `init`: a new memlog holding `fields` (each `key=value`) and `updated`.
/// Refused when `exists`, before any field is read.
pub fn init<'f>(
    path: &str,
    exists: bool,
    fields: impl IntoIterator<Item = &'f str>,
    now: &str,
) -> Result<Written, MemlogError> {
    if exists {
        return Err(MemlogError::Exists {
            path: path.to_owned(),
        });
    }
    let mut meta = Meta::default();
    for pair in fields {
        let Some((key, value)) = pair.split_once('=') else {
            return Err(MemlogError::Field {
                pair: pair.to_owned(),
            });
        };
        meta.set(py::strip(key), py::strip(value));
    }
    touch(&mut meta, now);
    Ok(Written {
        text: render(&meta, ""),
        body: String::new(),
    })
}

/// `append`: one entry at the end of `text`'s body. The text's whitespace runs
/// collapse to single spaces; `entry_type` and `by` render as one tag:
/// `(idea)`, `(idea by user)`, `(by coach)`.
pub fn append(
    text: &str,
    entry: &str,
    entry_type: Option<&str>,
    by: Option<&str>,
    now: &str,
) -> Result<Written, MemlogError> {
    let (mut meta, body) = split(text)?;
    let words: Vec<&str> = py::split(entry).collect();
    let mut label = entry_type.unwrap_or_default().to_owned();
    if let Some(by) = by.filter(|by| !by.is_empty()) {
        label = py::strip(&format!("{label} by {by}")).to_owned();
    }
    let tag = if label.is_empty() {
        String::new()
    } else {
        format!("({label}) ")
    };
    let line = format!("- {tag}{}", words.join(" "));
    let body = if py::strip(&body).is_empty() {
        line
    } else {
        format!("{}\n{line}", body.trim_end_matches('\n'))
    };
    touch(&mut meta, now);
    Ok(Written {
        text: render(&meta, &body),
        body,
    })
}

/// `set`: one frontmatter field, set or replaced; the body is kept.
pub fn set(text: &str, key: &str, value: &str, now: &str) -> Result<Written, MemlogError> {
    let (mut meta, body) = split(text)?;
    meta.set(key, value);
    touch(&mut meta, now);
    Ok(Written {
        text: render(&meta, &body),
        body,
    })
}

#[cfg(test)]
mod tests {
    //! Upstream's cases (`src/scripts/tests/test_memlog.py` at v6.12.0), over
    //! an in-memory disk standing in for the files the CLI touches, then the
    //! architecture memlog's round trip.
    use std::collections::HashMap;
    use std::path::Path;

    use super::*;

    const NOW: &str = "2026-10-05T09:30";

    /// The CLI's three commands over a map of path → text, answering the
    /// script's exit codes and printing its ack.
    #[derive(Default)]
    struct Disk {
        files: HashMap<String, String>,
        acks: Vec<String>,
    }

    impl Disk {
        fn run(
            &mut self,
            target: Target<'_>,
            result: impl FnOnce(&str, Option<&str>) -> Result<Written, MemlogError>,
        ) -> i32 {
            let path = target.file();
            let existing = self.files.get(&path).cloned();
            match result(&path, existing.as_deref()) {
                Ok(written) => {
                    self.acks.push(ack(&path, &written.body));
                    self.files.insert(path, written.text);
                    0
                }
                Err(MemlogError::Exists { .. } | MemlogError::Field { .. }) => 2,
                Err(error) => panic!("{error}"),
            }
        }

        fn init(&mut self, target: Target<'_>, fields: &[&str]) -> i32 {
            self.run(target, |path, existing| {
                init(path, existing.is_some(), fields.iter().copied(), NOW)
            })
        }

        fn append(
            &mut self,
            target: Target<'_>,
            text: &str,
            kind: Option<&str>,
            by: Option<&str>,
        ) -> i32 {
            self.run(target, |_, existing| {
                append(existing.expect("the memlog exists"), text, kind, by, NOW)
            })
        }

        fn set(&mut self, target: Target<'_>, key: &str, value: &str) -> i32 {
            self.run(target, |_, existing| {
                set(existing.expect("the memlog exists"), key, value, NOW)
            })
        }

        fn read(&self, ws: &str) -> &str {
            &self.files[&Target::Workspace(ws).file()]
        }

        fn split(&self, ws: &str) -> (Meta, String) {
            split(self.read(ws)).expect("splits")
        }

        fn entries(&self, ws: &str) -> Vec<String> {
            py::splitlines(&self.split(ws).1)
                .into_iter()
                .filter(|line| line.starts_with("- "))
                .map(str::to_owned)
                .collect()
        }
    }

    const WS: &str = "/tmp/ws";

    fn ws() -> Target<'static> {
        Target::Workspace(WS)
    }

    fn init_default(disk: &mut Disk) {
        assert_eq!(
            disk.init(
                ws(),
                &["topic=Reinvent the lunchbox", "goal=ideas for a pitch"]
            ),
            0
        );
    }

    fn add(disk: &mut Disk, text: &str, kind: Option<&str>) {
        assert_eq!(disk.append(ws(), text, kind, None), 0);
    }

    #[test]
    fn init_writes_frontmatter_fields() {
        let mut disk = Disk::default();
        init_default(&mut disk);
        let (meta, body) = disk.split(WS);
        assert_eq!(meta.get("topic"), Some("Reinvent the lunchbox"));
        assert_eq!(meta.get("goal"), Some("ideas for a pitch"));
        assert_eq!(meta.get("updated"), Some(NOW));
        assert_eq!(py::strip(&body), "");
    }

    #[test]
    fn init_has_no_lifecycle_status() {
        let mut disk = Disk::default();
        init_default(&mut disk);
        assert_eq!(disk.split(WS).0.get("status"), None);
    }

    #[test]
    fn init_arbitrary_fields() {
        let mut disk = Disk::default();
        assert_eq!(disk.init(ws(), &["topic=T", "audience=board"]), 0);
        assert_eq!(disk.split(WS).0.get("audience"), Some("board"));
    }

    #[test]
    fn init_refuses_overwrite() {
        let mut disk = Disk::default();
        init_default(&mut disk);
        assert_eq!(disk.init(ws(), &["topic=other"]), 2);
        assert_eq!(
            init("/tmp/ws/.memlog.md", true, [], NOW)
                .expect_err("refused")
                .to_string(),
            "error: /tmp/ws/.memlog.md already exists; use append/set to update it"
        );
    }

    #[test]
    fn init_rejects_malformed_field() {
        let mut disk = Disk::default();
        assert_eq!(disk.init(ws(), &["noequals"]), 2);
        assert_eq!(
            init("x", false, ["noequals"], NOW)
                .expect_err("refused")
                .to_string(),
            "error: --field expects key=value, got 'noequals'"
        );
    }

    #[test]
    fn path_addressing_targets_the_file_directly() {
        let mut disk = Disk::default();
        let target = Target::Path("/tmp/run/.memlog.md");
        assert_eq!(disk.init(target, &["topic=T"]), 0);
        assert!(disk.files.contains_key("/tmp/run/.memlog.md"));
        assert_eq!(disk.append(target, "an idea", Some("idea"), None), 0);
        let body = split(&disk.files["/tmp/run/.memlog.md"]).expect("splits").1;
        assert!(body.contains("- (idea) an idea"), "{body}");
    }

    #[test]
    fn workspace_and_path_resolve_to_same_file() {
        let mut disk = Disk::default();
        init_default(&mut disk);
        let via_path = format!("{WS}/{MEMLOG}");
        assert_eq!(
            disk.append(Target::Path(&via_path), "from path", None, None),
            0
        );
        assert_eq!(disk.append(ws(), "from workspace", None, None), 0);
        assert_eq!(disk.entries(WS), ["- from path", "- from workspace"]);
        assert_eq!(Target::Workspace("/tmp/ws/").file(), via_path);
    }

    /// What `memlog.py`'s `resolve` printed (`str(Path(w) / ".memlog.md")`,
    /// `str(Path(p))`) for the same spellings.
    #[test]
    fn a_target_is_spelled_as_pathlib_spells_it() {
        for (workspace, file) in [
            ("runs//today/.", "runs/today/.memlog.md"),
            ("", ".memlog.md"),
            (".", ".memlog.md"),
            ("/", "/.memlog.md"),
            ("//", "//.memlog.md"),
            ("///x//", "/x/.memlog.md"),
            ("//srv/./a/", "//srv/a/.memlog.md"),
            ("../a/./b", "../a/b/.memlog.md"),
            ("a/..", "a/../.memlog.md"),
        ] {
            assert_eq!(Target::Workspace(workspace).file(), file, "{workspace:?}");
        }
        for (path, file) in [
            ("runs//today/./.memlog.md", "runs/today/.memlog.md"),
            ("./.memlog.md", ".memlog.md"),
            ("//x/.memlog.md", "//x/.memlog.md"),
        ] {
            assert_eq!(Target::Path(path).file(), file, "{path:?}");
        }
    }

    #[test]
    fn append_lands_at_end_in_order() {
        let mut disk = Disk::default();
        init_default(&mut disk);
        for text in ["first", "second", "third"] {
            add(&mut disk, text, None);
        }
        assert_eq!(disk.entries(WS), ["- first", "- second", "- third"]);
    }

    #[test]
    fn no_sections_or_headings_ever() {
        let mut disk = Disk::default();
        init_default(&mut disk);
        add(&mut disk, "started foo", Some("technique"));
        add(&mut disk, "an idea", Some("idea"));
        add(&mut disk, "started bar", Some("technique"));
        assert!(!disk.split(WS).1.contains("## "));
    }

    #[test]
    fn type_renders_as_inline_tag() {
        let mut disk = Disk::default();
        init_default(&mut disk);
        add(&mut disk, "the earth revolves around the sun", Some("idea"));
        add(&mut disk, "how do we handle stampede?", Some("question"));
        let body = disk.split(WS).1;
        assert!(body.contains("- (idea) the earth revolves around the sun"));
        assert!(body.contains("- (question) how do we handle stampede?"));
    }

    #[test]
    fn append_without_type_is_plain_note() {
        let mut disk = Disk::default();
        init_default(&mut disk);
        add(&mut disk, "bare entry", None);
        assert_eq!(disk.entries(WS), ["- bare entry"]);
    }

    #[test]
    fn completion_is_an_entry_not_a_status() {
        let mut disk = Disk::default();
        init_default(&mut disk);
        add(&mut disk, "session complete", Some("event"));
        assert_eq!(disk.split(WS).0.get("status"), None);
        assert_eq!(
            disk.entries(WS).last().map(String::as_str),
            Some("- (event) session complete")
        );
    }

    #[test]
    fn append_collapses_newlines_into_one_line() {
        let mut disk = Disk::default();
        init_default(&mut disk);
        add(&mut disk, "line one\nline two\n  spaced   out", None);
        assert_eq!(disk.entries(WS), ["- line one line two spaced out"]);
    }

    #[test]
    fn revisited_technique_is_just_a_later_entry() {
        let mut disk = Disk::default();
        init_default(&mut disk);
        for (text, kind) in [
            ("started SCAMPER", "technique"),
            ("magnetic latch", "idea"),
            ("started Six Hats", "technique"),
            ("stale data risk", "idea"),
            ("started SCAMPER", "technique"),
            ("stackable tiers", "idea"),
        ] {
            add(&mut disk, text, Some(kind));
        }
        assert_eq!(
            disk.entries(WS),
            [
                "- (technique) started SCAMPER",
                "- (idea) magnetic latch",
                "- (technique) started Six Hats",
                "- (idea) stale data risk",
                "- (technique) started SCAMPER",
                "- (idea) stackable tiers",
            ]
        );
    }

    #[test]
    fn by_renders_attribution_in_tag() {
        let mut disk = Disk::default();
        init_default(&mut disk);
        assert_eq!(
            disk.append(ws(), "magnetic latch lid", Some("idea"), Some("user")),
            0
        );
        assert_eq!(
            disk.append(ws(), "lid doubles as a plate", Some("idea"), Some("coach")),
            0
        );
        let body = disk.split(WS).1;
        assert!(body.contains("- (idea by user) magnetic latch lid"));
        assert!(body.contains("- (idea by coach) lid doubles as a plate"));
    }

    #[test]
    fn by_without_type_renders_alone() {
        let mut disk = Disk::default();
        init_default(&mut disk);
        assert_eq!(
            disk.append(ws(), "off-the-cuff thought", None, Some("coach")),
            0
        );
        assert_eq!(disk.entries(WS), ["- (by coach) off-the-cuff thought"]);
    }

    #[test]
    fn heterogeneous_entry_types_coexist() {
        let mut disk = Disk::default();
        init_default(&mut disk);
        add(&mut disk, "an idea", Some("idea"));
        add(&mut disk, "an open question", Some("question"));
        add(&mut disk, "a decision we made", Some("decision"));
        add(&mut disk, "user wants mobile-first", Some("direction"));
        let body = disk.split(WS).1;
        for tag in ["(idea)", "(question)", "(decision)", "(direction)"] {
            assert!(body.contains(tag), "{tag}");
        }
    }

    #[test]
    fn free_vocabulary_is_not_enforced() {
        let mut disk = Disk::default();
        init_default(&mut disk);
        add(&mut disk, "a custom kind", Some("crack"));
        add(&mut disk, "another", Some("lock"));
        let body = disk.split(WS).1;
        assert!(body.contains("- (crack) a custom kind"));
        assert!(body.contains("- (lock) another"));
    }

    #[test]
    fn set_adds_field() {
        let mut disk = Disk::default();
        init_default(&mut disk);
        disk.set(ws(), "mode", "partner");
        assert_eq!(disk.split(WS).0.get("mode"), Some("partner"));
    }

    #[test]
    fn set_replaces_field() {
        let mut disk = Disk::default();
        assert_eq!(disk.init(ws(), &["topic=T", "mode=facilitator"]), 0);
        disk.set(ws(), "mode", "partner");
        assert_eq!(disk.split(WS).0.get("mode"), Some("partner"));
    }

    #[test]
    fn set_preserves_body() {
        let mut disk = Disk::default();
        init_default(&mut disk);
        add(&mut disk, "keep me", Some("idea"));
        disk.set(ws(), "mode", "partner");
        let (meta, body) = disk.split(WS);
        assert_eq!(meta.get("mode"), Some("partner"));
        assert!(body.contains("- (idea) keep me"));
    }

    #[test]
    fn updated_stays_last() {
        let mut disk = Disk::default();
        init_default(&mut disk);
        disk.set(ws(), "owner", "BMad");
        assert_eq!(disk.split(WS).0.keys().last(), Some("updated"));
    }

    #[test]
    fn roundtrip_render_is_stable() {
        let mut disk = Disk::default();
        init_default(&mut disk);
        add(&mut disk, "one", Some("idea"));
        let first = disk.read(WS);
        let (meta, body) = split(first).expect("splits");
        assert_eq!(render(&meta, &body), first);
    }

    #[test]
    fn commas_in_field_survive() {
        let mut disk = Disk::default();
        assert_eq!(disk.init(ws(), &["topic=cars, trains, and planes"]), 0);
        add(&mut disk, "z", Some("idea"));
        assert_eq!(
            disk.split(WS).0.get("topic"),
            Some("cars, trains, and planes")
        );
    }

    #[test]
    fn triple_dash_in_field_does_not_corrupt_frontmatter() {
        let mut disk = Disk::default();
        assert_eq!(
            disk.init(ws(), &["topic=Pricing --- tiers --- and add-ons"]),
            0
        );
        add(&mut disk, "an idea", Some("idea"));
        let (meta, body) = disk.split(WS);
        assert_eq!(meta.get("topic"), Some("Pricing --- tiers --- and add-ons"));
        assert_eq!(disk.entries(WS), ["- (idea) an idea"]);
        assert!(!body.contains("topic:"));
    }

    #[test]
    fn newline_in_field_is_neutralized() {
        let mut disk = Disk::default();
        assert_eq!(disk.init(ws(), &["topic=line one\nline two"]), 0);
        add(&mut disk, "x", Some("idea"));
        let topic = disk.split(WS).0.get("topic").map(str::to_owned);
        assert!(!topic.expect("topic").contains('\n'));
    }

    #[test]
    fn append_emits_json_ack() {
        let mut disk = Disk::default();
        init_default(&mut disk);
        add(&mut disk, "x", Some("idea"));
        let out: serde_json::Value =
            serde_json::from_str(disk.acks.last().expect("an ack")).expect("JSON");
        assert_eq!(out["ok"], serde_json::Value::Bool(true));
        assert_eq!(out["entries"], 1);
        assert!(out["memlog"].as_str().expect("a path").ends_with(MEMLOG));
        assert!(out.get("status").is_none());
        assert!(out.get("section").is_none());
    }

    #[test]
    fn ack_entry_count_climbs() {
        let mut disk = Disk::default();
        init_default(&mut disk);
        add(&mut disk, "a", None);
        add(&mut disk, "b", None);
        let out: serde_json::Value =
            serde_json::from_str(disk.acks.last().expect("an ack")).expect("JSON");
        assert_eq!(out["entries"], 2);
    }

    /// The architecture's real memlog: `memlog.py`'s `render(*split(x))`
    /// printed it back unchanged (fixture README), so the port must too; then
    /// the frontmatter refusals and the ack's ASCII escaping.
    #[test]
    fn memlog_round_trips_the_architecture_memlog() {
        let path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/bmad/architecture.memlog.md");
        let text = std::fs::read_to_string(path).expect("fixture");
        let (meta, body) = split(&text).expect("splits");
        assert_eq!(render(&meta, &body), text);

        assert_eq!(split("no fence\n"), Err(MemlogError::NoFrontmatter));
        assert_eq!(split("---\ntopic: x\n"), Err(MemlogError::Unterminated));
        let (meta, body) = split("---\ntopic: x\n----\n---\n- e\n").expect("splits");
        assert_eq!(
            (meta.get("topic"), body.as_str()),
            (Some("x"), "- e"),
            "only a line that is exactly `---` closes the frontmatter"
        );
        assert_eq!(
            ack("runs/żółw/.memlog.md", "- a\n- b\nnot an entry"),
            r#"{"ok": true, "memlog": "runs/\u017c\u00f3\u0142w/.memlog.md", "entries": 2}"#
        );
    }

    /// Append's tag grammar and whitespace collapse beside `set`, over one
    /// file, with the clock passed in.
    #[test]
    fn memlog_append_and_set() {
        let start = init("m", false, ["topic=T"], "2026-01-01T00:00").expect("init");
        let one = append(
            &start.text,
            " a\t b\u{1f}\n c ",
            Some("idea"),
            Some(""),
            "2026-01-01T00:01",
        )
        .expect("append");
        let two = set(&one.text, "updated", "ignored", "2026-01-01T00:02").expect("set");
        assert_eq!(
            two.text,
            "---\ntopic: T\nupdated: 2026-01-01T00:02\n---\n\n- (idea) a b c\n"
        );
    }
}
