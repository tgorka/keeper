//! Canonical slash ancestry for saved spaces.
#[must_use]
pub fn split(name: &str) -> (Option<String>, String, u32) {
    let mut segments: Vec<&str> = name
        .split('/')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .collect();
    let Some(leaf) = segments.pop() else {
        return (None, name.to_owned(), 0);
    };
    let depth = u32::try_from(segments.len()).unwrap_or(u32::MAX);
    let parent = (!segments.is_empty()).then(|| segments.join("/"));
    (parent, leaf.to_owned(), depth)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn hierarchy_trims_and_discards_empty_segments() {
        assert_eq!(
            split("Journal/Bali"),
            (Some("Journal".into()), "Bali".into(), 1)
        );
        assert_eq!(
            split(" / Journal // Bali / "),
            (Some("Journal".into()), "Bali".into(), 1)
        );
        assert_eq!(split("///"), (None, "///".into(), 0));
        assert_eq!(split(""), (None, "".into(), 0));
        assert_eq!(split("é/界/日"), (Some("é/界".into()), "日".into(), 2));
    }
}
