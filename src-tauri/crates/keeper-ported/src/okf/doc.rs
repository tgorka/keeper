//! One OKF document as the drive's tools read it: its frontmatter, its
//! body, and the title and description a listing shows for it.
//!
//! The OKF digest names the parts — a YAML block between `---` lines,
//! `title` and `description` recommended, `index.md` and `log.md` reserved;
//! the fallbacks are the drive's answers over `tests/fixtures/okf/docs.jsonl`.

use super::yaml::{self, Value};

/// The file names OKF reserves: a listing and an update history.
pub const RESERVED: [&str; 2] = ["index.md", "log.md"];

/// A document read from its text.
#[derive(Debug, Clone, PartialEq)]
pub struct Doc {
    /// Drive-relative, `/`-separated.
    pub path: String,
    /// The frontmatter, a mapping; empty without one or when it does not
    /// parse.
    pub meta: Value,
    /// The text after the frontmatter, the whole text without one.
    pub body: String,
    /// Why the frontmatter could not be read.
    pub error: Option<String>,
}

/// The frontmatter's text and the body, when `text` opens with a `---` line;
/// `Err` when that block is never closed.
pub fn split_frontmatter(text: &str) -> Result<Option<(&str, &str)>, &'static str> {
    let Some(rest) = text
        .strip_prefix("---\n")
        .or_else(|| text.strip_prefix("---\r\n"))
    else {
        return Ok(None);
    };
    let mut at = 0;
    loop {
        let line_end = rest[at..].find('\n').map(|end| at + end);
        let line = &rest[at..line_end.unwrap_or(rest.len())];
        if line.strip_suffix('\r').unwrap_or(line) == "---" {
            let meta = rest[..at].strip_suffix('\n').unwrap_or(&rest[..at]);
            let body = line_end.map_or("", |end| &rest[end + 1..]);
            return Ok(Some((meta, body)));
        }
        match line_end {
            Some(end) => at = end + 1,
            None => return Err("opening --- has no closing ---"),
        }
    }
}

impl Doc {
    /// Read `text` as the document at `path`.
    pub fn read(path: &str, text: &str) -> Doc {
        let empty = Value::Map(Vec::new());
        match split_frontmatter(text) {
            Ok(None) => Doc {
                path: path.to_owned(),
                meta: empty,
                body: text.to_owned(),
                error: None,
            },
            Ok(Some((meta, body))) => {
                let (meta, error) = match yaml::parse(meta) {
                    Ok(value @ Value::Map(_)) => (value, None),
                    Ok(Value::Null) => (empty, None),
                    Ok(_) => (empty, Some("the frontmatter is not a mapping".to_owned())),
                    Err(error) => (empty, Some(error.to_string())),
                };
                Doc {
                    path: path.to_owned(),
                    meta,
                    body: body.to_owned(),
                    error,
                }
            }
            Err(error) => Doc {
                path: path.to_owned(),
                meta: empty,
                body: text.to_owned(),
                error: Some(error.to_owned()),
            },
        }
    }

    /// The file's own name.
    pub fn basename(&self) -> &str {
        self.path.rsplit('/').next().unwrap_or(&self.path)
    }

    /// Whether OKF reserves the file's name.
    pub fn reserved(&self) -> bool {
        RESERVED.contains(&self.basename())
    }

    /// The OKF `type`, when the frontmatter states one as text.
    pub fn kind(&self) -> Option<&str> {
        self.meta.get("type").and_then(Value::as_str)
    }

    /// The title: the frontmatter's `title` when it is non-blank text, else
    /// the first line of the body that starts `# `, else the file's stem
    /// with `-` and `_` read as spaces.
    pub fn title(&self) -> String {
        if let Some(title) = self
            .meta
            .get("title")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|title| !title.is_empty())
        {
            return title.to_owned();
        }
        if let Some(heading) = self.body.lines().find_map(|line| line.strip_prefix("# ")) {
            return heading.trim().to_owned();
        }
        let name = self.basename();
        name.strip_suffix(".md")
            .unwrap_or(name)
            .replace(['-', '_'], " ")
    }

    /// The description: the frontmatter's `description` when it is text,
    /// its whitespace collapsed; else empty.
    pub fn description(&self) -> String {
        self.meta
            .get("description")
            .and_then(Value::as_str)
            .map(|text| text.split_whitespace().collect::<Vec<_>>().join(" "))
            .unwrap_or_default()
    }
}
