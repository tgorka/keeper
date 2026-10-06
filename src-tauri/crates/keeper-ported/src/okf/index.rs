//! An `index.md` listing as the drive's `okf index` writes it: an optional
//! bundle frontmatter (`okf_bundle_name`, `okf_bundle_title`,
//! `okf_bundle_entry_default`), a `# ` title, and `## ` sections —
//! `Bundles`, `Staging`, `Directories`, `Documents`, a staging zone's
//! `Files` — of lines `* [title](link)` or `* [title](link) - description`,
//! a space in a link written `%20`.
//!
//! The drive has a renderer and no parser; this reads what the renderer
//! wrote, proved over listings it wrote (`tests/fixtures/okf/index.jsonl`).

use super::doc::split_frontmatter;
use super::yaml::{self, Value};

/// The bundle a root listing declares.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BundleHead {
    pub name: String,
    pub title: String,
    pub entry: String,
}

/// One listed entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    /// The `## ` section it is under.
    pub section: String,
    pub title: String,
    /// As written: relative to the listing's folder, `%20` for a space.
    pub link: String,
    pub description: String,
}

/// A parsed listing.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Listing {
    pub bundle: Option<BundleHead>,
    pub entries: Vec<Entry>,
}

impl Listing {
    /// The entries under section `name`.
    pub fn section<'l>(&'l self, name: &'l str) -> impl Iterator<Item = &'l Entry> {
        self.entries
            .iter()
            .filter(move |entry| entry.section == name)
    }
}

/// `[title](link)` then nothing or ` - description`, after a line's `* `.
/// The title may hold brackets; the link holds no space and no `)`: the
/// first `](` whose link ends at the next `)` and is followed by nothing or
/// ` - `. Every `](` before the same `)` shares that `)`, what follows it
/// and the last space before it, each asked once, so a line costs its
/// length whatever delimiters it holds.
fn entry_line(rest: &str) -> Option<(String, String, String)> {
    let rest = rest.strip_prefix('[')?;
    // The `)` the current `](` run ends at, whether what follows it may end
    // an entry, and the last space between the run's first link and it.
    let mut run: Option<(usize, Option<&str>, Option<usize>)> = None;
    let mut from = 0;
    while let Some(at) = rest[from..].find("](").map(|at| from + at) {
        let link = at + 2;
        let (close, description, space) = match run {
            Some(run) if link <= run.0 => run,
            // No `)` after this `](` is none after any later one either.
            _ => {
                let close = link + rest[link..].find(')')?;
                let tail = &rest[close + 1..];
                let description = if tail.is_empty() {
                    Some("")
                } else {
                    tail.strip_prefix(" - ")
                };
                let space = rest[link..close].rfind(' ').map(|at| link + at);
                *run.insert((close, description, space))
            }
        };
        if let (true, Some(description)) = (space.is_none_or(|space| space < link), description) {
            return Some((
                rest[..at].to_owned(),
                rest[link..close].to_owned(),
                description.to_owned(),
            ));
        }
        from = link;
    }
    None
}

/// One entry as [`Lines`] reads it: the section it is under is the
/// listing's own heading, borrowed, so a long heading costs its length
/// once and never again per entry under it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LineEntry<'t> {
    pub section: &'t str,
    pub title: String,
    pub link: String,
    pub description: String,
}

impl From<LineEntry<'_>> for Entry {
    fn from(entry: LineEntry<'_>) -> Entry {
        Entry {
            section: entry.section.to_owned(),
            title: entry.title,
            link: entry.link,
            description: entry.description,
        }
    }
}

/// A listing's lines after its frontmatter, read one at a time: each the
/// entry it lists, or `None` — a heading, a line before the first `## `,
/// any other line. A reader may stop between any two lines; a line costs
/// its length ([`entry_line`]), whatever heading it is under.
pub struct Lines<'t> {
    lines: std::str::Lines<'t>,
    section: &'t str,
}

impl<'t> Iterator for Lines<'t> {
    type Item = Option<LineEntry<'t>>;

    fn next(&mut self) -> Option<Option<LineEntry<'t>>> {
        let line = self.lines.next()?;
        let line = line.strip_suffix('\r').unwrap_or(line);
        if let Some(heading) = line.strip_prefix("## ") {
            self.section = heading.trim();
            return Some(None);
        }
        if self.section.is_empty() {
            return Some(None);
        }
        Some(
            line.strip_prefix("* ")
                .and_then(entry_line)
                .map(|(title, link, description)| LineEntry {
                    section: self.section,
                    title,
                    link,
                    description,
                }),
        )
    }
}

/// The text after `text`'s frontmatter, the whole text without one (or
/// with one never closed).
fn body(text: &str) -> &str {
    match split_frontmatter(text) {
        Ok(Some((_, body))) => body,
        _ => text,
    }
}

/// `text`'s listed entries, a line at a time ([`Lines`]); its frontmatter
/// is passed over, not read.
pub fn lines(text: &str) -> Lines<'_> {
    Lines {
        lines: body(text).lines(),
        section: "",
    }
}

/// Read a listing's text.
pub fn parse(text: &str) -> Listing {
    let bundle = match split_frontmatter(text) {
        Ok(Some((meta, _))) => {
            let meta = yaml::parse(meta).unwrap_or(Value::Null);
            let field = |key: &str| meta.get(key).and_then(Value::scalar_text);
            field("okf_bundle_name").map(|name| BundleHead {
                name,
                title: field("okf_bundle_title").unwrap_or_default(),
                entry: field("okf_bundle_entry_default").unwrap_or_default(),
            })
        }
        _ => None,
    };
    Listing {
        bundle,
        entries: lines(text).flatten().map(Entry::from).collect(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The entry grammar as it was first written and proved against the
    /// drive's listings: each `](` tried in turn, its link up to the next
    /// `)` — quadratic in the delimiters, the reading [`entry_line`] must
    /// give in linear time.
    fn entry_line_by_suffix(rest: &str) -> Option<(String, String, String)> {
        let rest = rest.strip_prefix('[')?;
        let mut from = 0;
        while let Some(at) = rest[from..].find("](").map(|at| from + at) {
            let after = &rest[at + 2..];
            if let Some(close) = after.find(')') {
                let link = &after[..close];
                let tail = &after[close + 1..];
                let description = if tail.is_empty() {
                    Some("")
                } else {
                    tail.strip_prefix(" - ")
                };
                if let (false, Some(description)) = (link.contains(' '), description) {
                    return Some((
                        rest[..at].to_owned(),
                        link.to_owned(),
                        description.to_owned(),
                    ));
                }
            }
            from = at + 2;
        }
        None
    }

    /// R95S4-03: every line of up to seven of `[`, `]`, `(`, `)`, ` `,
    /// `-` and `a` after a `[` reads as the grammar proved against the
    /// drive's listings reads it — titles holding brackets, links with a
    /// space or a wrong tail passed over for a later `](`.
    #[test]
    fn every_short_line_reads_as_the_proved_grammar() {
        const ALPHABET: [char; 7] = ['[', ']', '(', ')', ' ', '-', 'a'];
        let mut lines = vec![String::from("[")];
        let mut checked = 0;
        for _ in 0..7 {
            let mut longer = Vec::with_capacity(lines.len() * ALPHABET.len());
            for line in &lines {
                for c in ALPHABET {
                    let mut next = line.clone();
                    next.push(c);
                    assert_eq!(entry_line(&next), entry_line_by_suffix(&next), "{next:?}");
                    checked += 1;
                    longer.push(next);
                }
            }
            lines = longer;
        }
        assert_eq!(checked, (1..=7).map(|n| 7usize.pow(n)).sum::<usize>());
    }

    /// R95S4-03: a line costs its length whatever delimiters it holds — a
    /// line of half a million `](` and no `)`, or with a `)` that ends no
    /// entry, is no entry, read in a small part of the time that trying
    /// each `](` against the rest of the line takes (seconds per line).
    #[test]
    fn a_line_of_delimiters_reads_in_its_length() {
        let runs = "](".repeat(500_000);
        let listing =
            format!("## Documents\n* [{runs}\n* [{runs})x\n* [{runs} ) - d\n* [N](n.md) - d\n");
        let started = std::time::Instant::now();
        let read: Vec<Option<LineEntry>> = lines(&listing).collect();
        let took = started.elapsed();
        assert_eq!(read.len(), 5);
        assert!(read[..4].iter().all(Option::is_none), "{:?}", &read[..4]);
        assert_eq!(read[4].as_ref().map(|e| e.link.as_str()), Some("n.md"));
        assert!(took < std::time::Duration::from_secs(2), "{took:?}");
    }

    /// R95S5-03: a short entry under a long heading costs its own length —
    /// the heading is read once, never copied per entry: under a heading of
    /// half a MiB, each of 20 000 one-line entries holds that heading's
    /// own bytes in the listing, not a copy of them (10 GB copied, else).
    #[test]
    fn a_long_heading_is_never_copied_per_entry() {
        let heading = "h".repeat(512 * 1024);
        let listing = format!("## {heading}\n{}", "* [t](l)\n".repeat(20_000));
        let held = listing.as_bytes().as_ptr_range();
        let mut entries = 0;
        for entry in lines(&listing).flatten() {
            assert_eq!(entry.section.len(), heading.len());
            assert!(
                held.contains(&entry.section.as_ptr()),
                "entry {entries}'s section is a copy of the heading"
            );
            entries += 1;
        }
        assert_eq!(entries, 20_000);
    }
}
