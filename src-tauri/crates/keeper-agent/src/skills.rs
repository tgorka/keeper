//! `skills_list` and `skill_view` (story 94.2, AD-362): the zone's
//! `_skills/` as 89.3's index offers it to the agent, and one offered
//! skill's files, loaded on demand (progressive loading, research §9.1).
//! A skill a BMAD step invokes is followed inline this way (R109).
//!
//! Every file is reached through `browse::resolve` with the skill's own
//! folder as the root, so a path out of it — `..`, or a link — is refused,
//! never read; a read is capped at [`MAX_READ_BYTES`] and the cut is said.
//! A file that is not UTF-8 is refused as not text, wherever its bad byte
//! is: only a character the cap itself cut in two is left out.

use std::io::Read as _;
use std::path::Path;

use keeper_core::agents::skills::{self, SkillsIndex};
use keeper_core::agents::workflow::ViewCall;
use keeper_core::bots::tools::{ToolOutcome, MAX_READ_BYTES};
use keeper_sync::browse;

use crate::bmad::FileRead;

fn refused(reason: impl Into<String>) -> ToolOutcome {
    ToolOutcome::Refused {
        reason: reason.into(),
    }
}

/// Why `name` is not one of the skills `index` offers: the validator's
/// reasons for a refused one, that a proposed one waits for a person, else
/// that it is not offered.
pub fn not_offered(index: &SkillsIndex, name: &str) -> String {
    if index.waiting.iter().any(|dir| dir == name) {
        return format!("{name} is not offered: it {}", skills::WAITING);
    }
    match index.refused.iter().find(|(dir, _)| dir == name) {
        Some((_, reasons)) => format!("{name} is not offered: {}", reasons.join(" ")),
        None => format!(
            "{name} is not a skill offered to this agent; skills_list names the ones that are."
        ),
    }
}

/// `skills_list`: the offered skills by name and purpose, then each
/// refused folder of `_skills/` with its reasons, then the skills that wait
/// for a person.
pub fn list(index: &SkillsIndex) -> ToolOutcome {
    let mut body = String::new();
    if index.offered.is_empty() {
        body.push_str("No skill is offered to you.\n");
    } else {
        body.push_str("Offered to you (load one with skill_view):\n");
        for skill in &index.offered {
            body.push_str(&format!("- {}: {}\n", skill.name, skill.description));
        }
    }
    if !index.refused.is_empty() {
        body.push_str("\nIn _skills/ but not offered:\n");
        for (dir, reasons) in &index.refused {
            body.push_str(&format!("- {dir}: {}\n", reasons.join(" ")));
        }
    }
    if !index.waiting.is_empty() {
        body.push_str("\nWaiting for a person:\n");
        for dir in &index.waiting {
            body.push_str(&format!("- {dir}: {}\n", skills::WAITING));
        }
    }
    ToolOutcome::Text {
        body,
        truncated_at: None,
        of_bytes: None,
        okf: None,
    }
}

/// `skill_view`: the offered skill's `SKILL.md`, or the file `call.path`
/// inside its folder `<zone>/_skills/<name>/` of the drive at `root`. The
/// file read is pushed onto `files`.
pub fn view(
    index: &SkillsIndex,
    root: &Path,
    zone: &str,
    call: &ViewCall,
    files: &mut Vec<FileRead>,
) -> ToolOutcome {
    let name = call.name.as_str();
    if !index.offered.iter().any(|skill| skill.name == name) {
        return refused(not_offered(index, name));
    }
    let folder = format!("{zone}/_skills/{name}");
    let rel = call.path.as_deref().unwrap_or("SKILL.md");
    let dir = match browse::resolve(root, &folder) {
        Ok(Some(dir)) if dir.is_dir() => dir,
        Ok(_) => return refused(format!("{folder}/ is not in this drive.")),
        Err(refusal) => return refused(format!("{folder} is refused: {refusal}")),
    };
    let path = match browse::resolve(&dir, rel) {
        Ok(Some(path)) if path.is_file() => path,
        Ok(Some(_)) => return refused(format!("{rel} is not a file of the skill {name}.")),
        Ok(None) => return refused(format!("The skill {name} has no {rel}.")),
        Err(refusal) => return refused(format!("{rel} is refused: {refusal}")),
    };
    let read = std::fs::File::open(&path).and_then(|file| {
        let total = file.metadata()?.len();
        let mut bytes = Vec::new();
        file.take(MAX_READ_BYTES).read_to_end(&mut bytes)?;
        Ok((bytes, total))
    });
    let (mut bytes, total) = match read {
        Ok(read) => read,
        Err(error) => return refused(format!("{rel} could not be read: {error}")),
    };
    let cut = total > bytes.len() as u64;
    match std::str::from_utf8(&bytes) {
        Ok(_) => {}
        // The cap fell inside the last character: it is left out.
        Err(error) if cut && error.error_len().is_none() => bytes.truncate(error.valid_up_to()),
        Err(_) => return refused(format!("{rel} is not text.")),
    }
    let Ok(body) = String::from_utf8(bytes) else {
        return refused(format!("{rel} is not text."));
    };
    files.push(FileRead::of(
        root,
        format!("{folder}/{rel}"),
        &path,
        body.as_bytes(),
    ));
    ToolOutcome::Text {
        truncated_at: cut.then_some(body.len() as u64),
        of_bytes: cut.then_some(total),
        body,
        okf: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use keeper_core::agents::skills::{index, SkillFilter};
    use keeper_core::agents::workflow::parse_view;

    fn write(root: &Path, rel: &str, text: &str) {
        let path = root.join(rel);
        std::fs::create_dir_all(path.parent().expect("parent")).expect("mkdir");
        std::fs::write(path, text).expect("write");
    }

    fn skill(name: &str) -> String {
        format!("---\nname: {name}\ndescription: Does {name}.\n---\n\nThe {name} steps.\n")
    }

    /// A drive whose zone holds `x` (with a reference), `y`, and a folder
    /// whose `SKILL.md` names another skill; the agent is offered `x` alone.
    fn drive() -> (tempfile::TempDir, SkillsIndex) {
        let root = tempfile::tempdir().expect("tempdir");
        write(root.path(), "80-agents/_skills/x/SKILL.md", &skill("x"));
        write(
            root.path(),
            "80-agents/_skills/x/references/a.md",
            "reference a\n",
        );
        write(root.path(), "80-agents/_skills/y/SKILL.md", &skill("y"));
        write(
            root.path(),
            "80-agents/_skills/mismatch/SKILL.md",
            &skill("other"),
        );
        let found: Vec<(String, String)> = ["x", "y", "mismatch"]
            .iter()
            .map(|dir| {
                let text = std::fs::read_to_string(
                    root.path()
                        .join(format!("80-agents/_skills/{dir}/SKILL.md")),
                )
                .expect("read");
                ((*dir).to_owned(), text)
            })
            .collect();
        let offered = index(&found, &SkillFilter::from_list(&["x".to_owned()]));
        (root, offered)
    }

    /// The outcome of `skill_view` with `args`, and where each file it read
    /// came from.
    fn view_of(
        index: &SkillsIndex,
        root: &Path,
        args: serde_json::Value,
    ) -> (ToolOutcome, Vec<String>) {
        let mut files = Vec::new();
        let call = parse_view(&args).expect("arguments");
        let outcome = view(index, root, "80-agents", &call, &mut files);
        (
            outcome,
            files.iter().map(|read| read.path().to_owned()).collect(),
        )
    }

    /// 94.2 acceptance 5: `skills_list` is exactly 89.3's offer — each
    /// offered skill by name and purpose, each refused folder with the
    /// validator's reasons — and a skill not offered is not listed as one.
    #[test]
    fn skills_list_is_the_offered_index() {
        let (_root, index) = drive();
        let ToolOutcome::Text { body, .. } = list(&index) else {
            panic!("a list");
        };
        let lines: Vec<&str> = body.lines().collect();
        assert!(lines.contains(&"- x: Does x."), "{body}");
        let (_, reasons) = index
            .refused
            .iter()
            .find(|(dir, _)| dir == "mismatch")
            .expect("mismatch is refused");
        assert!(
            lines.contains(&format!("- mismatch: {}", reasons.join(" ")).as_str()),
            "{body}"
        );
        assert!(!body.contains("- y:"), "y is valid but not offered: {body}");
    }

    /// 94.2 acceptance 5: `skill_view` reads an offered skill's files and
    /// nothing outside its folder.
    #[cfg(unix)]
    #[test]
    fn skill_view_stays_inside_the_skill() {
        let (root, index) = drive();
        let root = root.path();
        let text = |outcome: ToolOutcome| match outcome {
            ToolOutcome::Text { body, .. } => body,
            other => panic!("not text: {other:?}"),
        };
        let refusal = |outcome: ToolOutcome| match outcome {
            ToolOutcome::Refused { reason } => reason,
            other => panic!("not refused: {other:?}"),
        };

        let (outcome, files) = view_of(&index, root, serde_json::json!({"name": "x"}));
        assert_eq!(text(outcome), skill("x"));
        assert_eq!(files, ["80-agents/_skills/x/SKILL.md"]);
        let (outcome, files) = view_of(
            &index,
            root,
            serde_json::json!({"name": "x", "path": "references/a.md"}),
        );
        assert_eq!(text(outcome), "reference a\n");
        assert_eq!(files, ["80-agents/_skills/x/references/a.md"]);

        let (outcome, files) = view_of(
            &index,
            root,
            serde_json::json!({"name": "x", "path": "../y/SKILL.md"}),
        );
        refusal(outcome);
        assert!(files.is_empty());
        let outside = tempfile::tempdir().expect("outside");
        write(outside.path(), "secret.md", "not the skill's\n");
        std::os::unix::fs::symlink(
            outside.path().join("secret.md"),
            root.join("80-agents/_skills/x/leak.md"),
        )
        .expect("link");
        let (outcome, files) = view_of(
            &index,
            root,
            serde_json::json!({"name": "x", "path": "leak.md"}),
        );
        assert!(!refusal(outcome).contains("not the skill's"));
        assert!(files.is_empty());

        // Valid but not offered, and refused by the validator.
        let (outcome, files) = view_of(&index, root, serde_json::json!({"name": "y"}));
        refusal(outcome);
        assert!(files.is_empty());
        let (outcome, files) = view_of(&index, root, serde_json::json!({"name": "mismatch"}));
        refusal(outcome);
        assert!(files.is_empty());

        // A file past the cap is cut, and the cut is said.
        let long = "é".repeat(MAX_READ_BYTES as usize);
        write(root, "80-agents/_skills/x/references/long.md", &long);
        let (outcome, _) = view_of(
            &index,
            root,
            serde_json::json!({"name": "x", "path": "references/long.md"}),
        );
        let ToolOutcome::Text {
            body,
            truncated_at,
            of_bytes,
            ..
        } = outcome
        else {
            panic!("text");
        };
        assert_eq!(body.len() as u64, MAX_READ_BYTES);
        assert_eq!(truncated_at, Some(MAX_READ_BYTES));
        assert_eq!(of_bytes, Some(long.len() as u64));
    }

    /// R195: past the cap, only the character the cap cut in two is left
    /// out; a byte that is not UTF-8 anywhere before it refuses the file as
    /// not text, as it does below the cap.
    #[test]
    fn skill_view_cuts_at_a_character_and_refuses_bad_bytes() {
        let (root, index) = drive();
        let root = root.path();
        let at = |rel: &str, bytes: &[u8]| {
            let path = root.join("80-agents/_skills/x").join(rel);
            std::fs::write(path, bytes).expect("write");
            view_of(&index, root, serde_json::json!({"name": "x", "path": rel}))
        };
        let cap = MAX_READ_BYTES as usize;

        // One byte, then two-byte characters: the cap splits the last one.
        let split = format!("a{}", "é".repeat(cap));
        let (outcome, files) = at("split.md", split.as_bytes());
        let ToolOutcome::Text {
            body, truncated_at, ..
        } = outcome
        else {
            panic!("text: {outcome:?}");
        };
        assert_eq!(body.len(), cap - 1);
        assert!(split.starts_with(&body));
        assert_eq!(truncated_at, Some(cap as u64 - 1));
        assert_eq!(files, ["80-agents/_skills/x/split.md"]);

        let mut early = vec![0xff];
        early.extend(std::iter::repeat_n(b'a', cap + 10));
        let mut middle = vec![b'a'; cap / 2];
        middle.push(0xff);
        middle.extend(std::iter::repeat_n(b'a', cap));
        let mut short = b"ok ".to_vec();
        short.push(0xff);
        for (rel, bytes) in [
            ("early.md", early),
            ("middle.md", middle),
            ("short.md", short),
        ] {
            let (outcome, files) = at(rel, &bytes);
            assert!(
                matches!(outcome, ToolOutcome::Refused { .. }),
                "{rel}: {outcome:?}"
            );
            assert!(files.is_empty(), "{rel}");
        }
    }
}
