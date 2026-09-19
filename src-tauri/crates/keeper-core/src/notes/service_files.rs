//! List-only service-file classification (AD-267). A name is not a path or glob.

pub const DEFAULT_SERVICE_FILE_NAMES: [&str; 4] = ["index.md", "agents.md", "claude.md", "log.md"];

/// Compare the final vault-relative segment against names normalized by the registry.
pub fn is_service_file(path: &str, names: &[String]) -> bool {
    let basename = path.rsplit('/').next().unwrap_or_default();
    if basename.is_ascii() {
        names
            .iter()
            .any(|name| !name.is_empty() && basename.eq_ignore_ascii_case(name))
    } else {
        names.contains(&basename.to_lowercase())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matches_basename_in_any_folder_without_case() {
        let names = DEFAULT_SERVICE_FILE_NAMES.map(str::to_owned);
        assert!(is_service_file("agents.md", &names));
        assert!(is_service_file("docs/Log.MD", &names));
        assert!(is_service_file("a/b/CLAUDE.md", &names));
        assert!(!is_service_file("docs/my-log.md", &names));
        assert!(!is_service_file("log.md/note.md", &names));
    }

    #[test]
    fn ignores_paths_and_blank_names() {
        let names = ["docs/log.md", "docs\\log.md", "", "   "].map(str::to_owned);
        assert!(!is_service_file("docs/log.md", &names));
        assert!(!is_service_file("", &names));
        assert!(!is_service_file("log.md", &[]));
    }
}
