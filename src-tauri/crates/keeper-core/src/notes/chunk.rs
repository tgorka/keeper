//! Heading-first retrieval units, not a second Markdown parser (AD-262).
//!
//! Research §3.1 supplies paragraph buffering and no overlap. Fences refine that
//! shape: a blank line in code is not a paragraph boundary. Every stored text is
//! a slice of the editor body; the contextual prefix belongs only to embeddings.

use std::collections::BTreeMap;

use super::index::RESERVED_FIELD_PREFIX;

pub const CHUNK_TARGET_CHARS: usize = 1_200;
pub const CHUNK_MAX_CHARS: usize = 4_000;
pub const CHUNK_MIN_CHARS: usize = 50;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Chunk {
    pub ordinal: u32,
    pub heading: String,
    pub byte_start: usize,
    pub byte_end: usize,
    pub text: String,
}

pub fn chunk_body(body: &str) -> Vec<Chunk> {
    let mut chunks = Vec::new();
    let mut headings: Vec<(usize, String)> = Vec::new();
    let mut heading = String::new();
    let mut fence: Option<(u8, usize)> = None;
    let mut paragraph = 0;
    let mut buffer: Option<usize> = None;
    let mut at = 0;
    while let Some((start, end, next)) = super::line_bounds(body, at) {
        let line = &body[start..end];
        let trimmed = line.trim_start_matches(' ');
        let indent = line.len() - trimmed.len();
        let marker = trimmed.as_bytes().first().copied();
        let run = marker.map_or(0, |m| trimmed.bytes().take_while(|b| *b == m).count());
        let is_fence = indent <= 3 && matches!(marker, Some(b'`' | b'~')) && run >= 3;
        if let Some((m, n)) = fence {
            if is_fence && marker == Some(m) && run >= n && trimmed[run..].trim().is_empty() {
                fence = None;
            }
        } else {
            let level = line.bytes().take_while(|b| *b == b'#').count();
            if (1..=6).contains(&level)
                && (line.len() == level || line.as_bytes()[level].is_ascii_whitespace())
            {
                feed(body, paragraph, start, &heading, &mut buffer, &mut chunks);
                flush(body, &mut buffer, start, &heading, &mut chunks);
                headings.retain(|(depth, _)| *depth < level);
                headings.push((
                    level,
                    line[level..].trim().trim_end_matches('#').trim().to_owned(),
                ));
                heading = headings
                    .iter()
                    .map(|(_, h)| h.as_str())
                    .collect::<Vec<_>>()
                    .join(" › ");
                paragraph = start;
            } else if line.trim().is_empty() {
                feed(body, paragraph, next, &heading, &mut buffer, &mut chunks);
                paragraph = next;
            }
            if is_fence {
                fence = marker.map(|m| (m, run));
            }
        }
        at = next;
    }
    feed(
        body,
        paragraph,
        body.len(),
        &heading,
        &mut buffer,
        &mut chunks,
    );
    flush(body, &mut buffer, body.len(), &heading, &mut chunks);
    chunks
}

fn feed(
    body: &str,
    mut start: usize,
    end: usize,
    heading: &str,
    buffer: &mut Option<usize>,
    chunks: &mut Vec<Chunk>,
) {
    if start == end {
        return;
    }
    if let Some(first) = *buffer {
        if body[first..end].chars().nth(CHUNK_TARGET_CHARS).is_some() {
            flush(body, buffer, start, heading, chunks);
        }
    }
    while let Some((offset, _)) = body[start..end].char_indices().nth(CHUNK_MAX_CHARS) {
        flush(body, buffer, start, heading, chunks);
        let hard_end = start + offset;
        let cut = body[start..hard_end]
            .rfind('\n')
            .map_or(hard_end, |i| start + i + 1);
        push(body, start, cut, heading, chunks);
        start = cut;
    }
    if start < end {
        buffer.get_or_insert(start);
    }
}

fn flush(
    body: &str,
    buffer: &mut Option<usize>,
    end: usize,
    heading: &str,
    chunks: &mut Vec<Chunk>,
) {
    if let Some(start) = buffer.take() {
        push(body, start, end, heading, chunks);
    }
}

fn push(body: &str, start: usize, end: usize, heading: &str, chunks: &mut Vec<Chunk>) {
    if start == end {
        return;
    }
    let text = &body[start..end];
    if text.trim().is_empty() {
        return;
    }
    if text.chars().take(CHUNK_MIN_CHARS).count() < CHUNK_MIN_CHARS {
        if let Some(previous) = chunks.last_mut() {
            if previous.heading == heading
                && body[previous.byte_start..end].chars().count() <= CHUNK_MAX_CHARS
            {
                previous.text.push_str(&body[previous.byte_end..end]);
                previous.byte_end = end;
                return;
            }
        }
    }
    chunks.push(Chunk {
        ordinal: chunks.len() as u32 + 1,
        heading: heading.to_owned(),
        byte_start: start,
        byte_end: end,
        text: text.to_owned(),
    });
}

pub fn head_text(
    title: &str,
    path: &str,
    tags: &[String],
    fields: &BTreeMap<String, String>,
) -> String {
    std::iter::once(title)
        .chain(std::iter::once(path))
        .chain(tags.iter().map(String::as_str))
        .chain(
            fields
                .iter()
                .filter(|(key, _)| !key.starts_with(RESERVED_FIELD_PREFIX))
                .map(|(_, value)| value.as_str()),
        )
        .collect::<Vec<_>>()
        .join("\n")
}

pub fn embedding_text(title: &str, chunk: &Chunk) -> String {
    if chunk.heading.is_empty() {
        format!("{title}\n\n{}", chunk.text)
    } else {
        format!("{title} › {}\n\n{}", chunk.heading, chunk.text)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn short_sections_keep_their_heading() {
        let chunks = chunk_body("# A\n\n# B\nshort");
        assert_eq!(
            chunks
                .iter()
                .map(|c| c.heading.as_str())
                .collect::<Vec<_>>(),
            ["A", "B"]
        );
        assert_eq!(
            embedding_text("Title", &chunks[1]),
            "Title › B\n\n# B\nshort"
        );
    }

    #[test]
    fn whitespace_paragraphs_are_not_embeddable() {
        assert!(chunk_body(" \n\n\t\r\n").is_empty());
    }

    #[test]
    fn short_note() {
        let body = "a".repeat(30);
        assert_eq!(chunk_body(&body)[0].text, body);
        assert_eq!(chunk_body(&body).len(), 1);
    }

    #[test]
    fn fence_keeps_blank_lines() {
        let body = format!(
            "```rust\n{}\n\n# not a heading\n{}\n```",
            "x".repeat(700),
            "y".repeat(700)
        );
        let chunks = chunk_body(&body);
        assert_eq!(chunks.len(), 1);
        assert_eq!(chunks[0].text, body);
        assert_eq!(chunks[0].heading, "");
    }

    #[test]
    fn heading_only() {
        let chunks = chunk_body("# Heading");
        assert_eq!(chunks.len(), 1);
        assert_eq!(chunks[0].heading, "Heading");
        assert_eq!(chunks[0].text, "# Heading");
    }

    #[test]
    fn oversized_paragraph() {
        let body = "ł".repeat(10_000);
        let chunks = chunk_body(&body);
        assert_eq!(chunks.len(), 3);
        assert!(chunks
            .iter()
            .all(|c| c.text.chars().count() <= CHUNK_MAX_CHARS));
        assert_eq!(
            chunks.iter().map(|c| c.text.as_str()).collect::<String>(),
            body
        );
    }

    #[test]
    fn byte_ranges_are_exact() {
        let body = format!(
            "# A\n{}\n\n## B\n{}\n\nlast 🙂",
            "Łódź ".repeat(400),
            "notatkę ".repeat(600)
        );
        let chunks = chunk_body(&body);
        let mut previous = 0;
        for (i, chunk) in chunks.iter().enumerate() {
            assert_eq!(chunk.ordinal, i as u32 + 1);
            assert_eq!(chunk.byte_start, previous);
            assert_eq!(chunk.text, body[chunk.byte_start..chunk.byte_end]);
            previous = chunk.byte_end;
        }
        assert_eq!(previous, body.len());
        assert!(chunks.iter().any(|c| c.heading == "A › B"));
    }

    #[test]
    fn contextual_embedding_and_reserved_head() {
        let fields = BTreeMap::from([
            ("project".into(), "public".into()),
            ("keeper.origin".into(), "private".into()),
        ]);
        assert_eq!(
            head_text("title", "path", &["tag".into()], &fields),
            "title\npath\ntag\npublic"
        );
        assert_eq!(
            embedding_text("title", &chunk_body("# A")[0]),
            "title › A\n\n# A"
        );
        assert_eq!(
            embedding_text("title", &chunk_body("body")[0]),
            "title\n\nbody"
        );
    }
}
