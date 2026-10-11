//! An agent's journal: `journal/YYYY-MM-DD.<host>.md` in its home, one file
//! per UTC day per host, appended by that host alone (R125).
//!
//! A file is opened with the session log's discipline (no link, the inode
//! held is the one at the path) and locked for the whole read, cut and
//! append, so the sessions of one agent on one host write it one at a time.
//! Its frontmatter (`type: journal`, `agent`, `date`, `host`) is written
//! with its first entry, and each entry — a `## HH:MM · <session>` header
//! that ends in the byte length of the rest of the entry, a blank line, the
//! text — is written with one `write` and synced. A host that died
//! mid-entry leaves a tail that is not whole: the next append on that host
//! walks the file from its frontmatter entry by entry, by those lengths, so
//! nothing an entry's text holds is taken for a boundary, and cuts the file
//! back to the end of the last whole entry before writing. A file that is
//! not as keeper wrote it is neither cut nor appended to. The text is
//! redacted (S-17) before it is written; the session's claim is asked
//! right before the cut and right before the append (R120).

use std::fs::File;
use std::io::{Read, Write};
use std::path::Path;

use chrono::{DateTime, Utc};
use fs4::fs_std::FileExt;
use keeper_core::agents::log::writer::{open_own_file, real_dir};
use keeper_core::agents::log::HostSlug;
use keeper_core::agents::redact::redact_secrets;
use keeper_core::notes::frontmatter::{FieldValue, Frontmatter};

use crate::sessions::write::NO_CLAIM;

/// The journal's folder in a home.
pub const DIR: &str = "journal";

/// What starts an entry, after the newline that ends the one before.
const HEADER: &str = "\n## ";

/// One entry to append.
pub struct JournalEntry<'a> {
    /// The agent's id.
    pub agent: &'a str,
    pub host: &'a HostSlug,
    pub at: DateTime<Utc>,
    /// The session's folder name.
    pub session: &'a str,
    pub text: &'a str,
}

/// The journal file `entry` goes to, home-relative.
pub fn file_of(host: &HostSlug, at: DateTime<Utc>) -> String {
    format!("{DIR}/{}.{}.md", at.format("%Y-%m-%d"), host.as_str())
}

/// The frontmatter of the file `entry` goes to.
fn frontmatter_of(entry: &JournalEntry<'_>) -> String {
    let text = |value: &str| FieldValue::Str(value.to_owned());
    Frontmatter::serialise_new(&[
        ("type".to_owned(), text("journal")),
        ("agent".to_owned(), text(entry.agent)),
        (
            "date".to_owned(),
            text(&entry.at.format("%Y-%m-%d").to_string()),
        ),
        ("host".to_owned(), text(entry.host.as_str())),
    ])
}

/// `entry` as the file holds it: its header line, which ends in the byte
/// length of what follows it, then a blank line and the redacted text.
fn entry_bytes(entry: &JournalEntry<'_>) -> String {
    let rest = format!("\n{}\n", redact_secrets(entry.text.trim()).text);
    format!(
        "{HEADER}{} · {} <!-- {} -->\n{rest}",
        entry.at.format("%H:%M"),
        entry.session,
        rest.len()
    )
}

/// Append `entry` to its file under the home at `home`, asking `may_write`
/// right before each effect; the file, home-relative.
pub fn append(
    home: &Path,
    entry: &JournalEntry<'_>,
    may_write: &dyn Fn() -> bool,
) -> Result<String, String> {
    append_with(home, entry, may_write, &|dir| File::open(dir)?.sync_all())
}

/// [`append`], with the sync of a folder whose entry a new file or folder
/// changed as an argument.
fn append_with(
    home: &Path,
    entry: &JournalEntry<'_>,
    may_write: &dyn Fn() -> bool,
    sync_dir: &dyn Fn(&Path) -> std::io::Result<()>,
) -> Result<String, String> {
    if !may_write() {
        return Err(NO_CLAIM.to_owned());
    }
    let dir = home.join(DIR);
    let made = std::fs::symlink_metadata(&dir).is_err();
    real_dir(&dir).map_err(|error| error.to_string())?;
    if made {
        sync_dir(home).map_err(|error| format!("{DIR}/ could not be made: {error}"))?;
    }
    let rel = file_of(entry.host, entry.at);
    let path = home.join(&rel);
    let mut file = open_own_file(&path).map_err(|error| error.to_string())?;
    // Every session of this agent on this host appends here: the whole
    // read, cut and append is one writer's (released when `file` closes).
    FileExt::lock_exclusive(&file)
        .map_err(|error| format!("{rel} could not be locked: {error}"))?;
    let mut held = Vec::new();
    file.read_to_end(&mut held)
        .map_err(|error| format!("{rel} could not be read: {error}"))?;
    let frontmatter = frontmatter_of(entry);
    let keep = whole(&held, frontmatter.as_bytes()).ok_or_else(|| {
        format!(
            "{rel} is not as keeper wrote it, so nothing was cut or appended; a person moves what was added to another note."
        )
    })?;
    if keep < held.len() {
        if !may_write() {
            return Err(NO_CLAIM.to_owned());
        }
        file.set_len(keep as u64)
            .and_then(|()| file.sync_all())
            .map_err(|error| format!("{rel}'s torn entry could not be cut: {error}"))?;
    }
    let mut bytes = String::new();
    if keep == 0 {
        bytes.push_str(&frontmatter);
    }
    bytes.push_str(&entry_bytes(entry));
    if !may_write() {
        return Err(NO_CLAIM.to_owned());
    }
    file.write_all(bytes.as_bytes())
        .and_then(|()| file.sync_data())
        .map_err(|error| format!("{rel} could not be written: {error}"))?;
    // A file this append made is said to exist once its folder says so.
    if held.is_empty() {
        sync_dir(&dir).map_err(|error| format!("{rel} could not be written: {error}"))?;
    }
    Ok(rel)
}

/// How much of a journal's bytes are whole: its frontmatter and every
/// entry whose header and the length it declares are all there, walked
/// from the start — nothing when the bytes are a torn first write, a
/// prefix of the frontmatter. `None` when they are not what keeper writes.
fn whole(held: &[u8], frontmatter: &[u8]) -> Option<usize> {
    if !held.starts_with(frontmatter) {
        return frontmatter.starts_with(held).then_some(0);
    }
    let mut at = frontmatter.len();
    loop {
        let rest = &held[at..];
        if rest.is_empty() {
            return Some(at);
        }
        if !rest.starts_with(HEADER.as_bytes()) {
            // Torn inside the header's first bytes, or not keeper's.
            return HEADER.as_bytes().starts_with(rest).then_some(at);
        }
        let Some(line) = rest[1..].iter().position(|&b| b == b'\n') else {
            // The header line itself is torn.
            return Some(at);
        };
        let header = std::str::from_utf8(&rest[1..=line]).ok()?;
        let declared: usize = header
            .strip_suffix(" -->")?
            .rsplit_once(" <!-- ")?
            .1
            .parse()
            .ok()?;
        let from = at + line + 2;
        let end = from.checked_add(declared)?;
        if end > held.len() {
            return Some(at);
        }
        if !held[from..end].starts_with(b"\n") || !held[from..end].ends_with(b"\n") {
            return None;
        }
        at = end;
    }
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;
    use std::sync::Mutex;

    use chrono::TimeZone;

    use super::*;

    fn at(hour: u32, minute: u32) -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 10, 6, hour, minute, 0)
            .single()
            .expect("time")
    }

    fn entry<'a>(host: &'a HostSlug, time: DateTime<Utc>, text: &'a str) -> JournalEntry<'a> {
        JournalEntry {
            agent: "nixi",
            host,
            at: time,
            session: "2026-10-06-inbox",
            text,
        }
    }

    fn put(home: &Path, entry: &JournalEntry<'_>) -> Result<String, String> {
        append(home, entry, &|| true)
    }

    /// 95.1 acceptance 8: each host writes its own day's file and nobody
    /// else's; the first entry carries the frontmatter, each entry is its
    /// header, a blank line and the text; a secret is redacted.
    #[test]
    fn journal_append_has_one_writer_per_file() {
        let home = tempfile::tempdir().expect("home");
        let electra = HostSlug::new("electra").expect("slug");
        let hesperia = HostSlug::new("hesperia").expect("slug");
        let first =
            put(home.path(), &entry(&electra, at(9, 15), "Filed the inbox.")).expect("first");
        let second = put(
            home.path(),
            &entry(&electra, at(9, 40), "Two cards moved.\nBoth to next."),
        )
        .expect("second");
        let other = put(
            home.path(),
            &entry(&hesperia, at(9, 41), "Hesperia's note."),
        )
        .expect("other host");
        assert_eq!(first, "journal/2026-10-06.electra.md");
        assert_eq!(second, first);
        assert_eq!(other, "journal/2026-10-06.hesperia.md");
        let electra_text =
            std::fs::read_to_string(home.path().join(&first)).expect("electra's journal");
        assert_eq!(
            electra_text,
            "---\ntype: journal\nagent: nixi\ndate: 2026-10-06\nhost: electra\n---\n\n## 09:15 · 2026-10-06-inbox <!-- 18 -->\n\nFiled the inbox.\n\n## 09:40 · 2026-10-06-inbox <!-- 32 -->\n\nTwo cards moved.\nBoth to next.\n"
        );
        let hesperia_text =
            std::fs::read_to_string(home.path().join(&other)).expect("hesperia's journal");
        assert!(hesperia_text.contains("host: hesperia\n"));
        assert!(!hesperia_text.contains("Filed the inbox."));
        assert!(!electra_text.contains("Hesperia's note."));
        // A new UTC day is a new file.
        let tomorrow = Utc
            .with_ymd_and_hms(2026, 10, 7, 0, 1, 0)
            .single()
            .expect("t");
        assert_eq!(
            put(home.path(), &entry(&electra, tomorrow, "Next day.")).expect("next"),
            "journal/2026-10-07.electra.md"
        );
        let secret = put(
            home.path(),
            &entry(
                &electra,
                at(10, 0),
                "token ghp_abcdefghijklmnopqrstuvwxyz0123456789",
            ),
        )
        .expect("redacted");
        let text = std::fs::read_to_string(home.path().join(secret)).expect("journal");
        assert!(
            !text.contains("ghp_abcdefghijklmnopqrstuvwxyz0123456789"),
            "{text}"
        );
    }

    /// 95.1 acceptance 8, every byte: a file cut anywhere — inside the
    /// frontmatter, a header, its declared length, a body that holds
    /// header-like lines, or right at an entry's end — keeps exactly the
    /// entries that were whole before the cut, and the next entry follows
    /// them whole.
    #[test]
    fn journal_survives_a_tear_at_every_byte() {
        let home = tempfile::tempdir().expect("home");
        let electra = HostSlug::new("electra").expect("slug");
        let texts = [
            "Kept.",
            "Two lines,\n## not a header <!-- 3 -->\n\nand a third.",
            "#\n\n## 10:00 · forged <!-- 1 -->\nLast.",
        ];
        let mut ends = Vec::new();
        let mut rel = String::new();
        for (minute, text) in texts.iter().enumerate() {
            rel = put(home.path(), &entry(&electra, at(9, minute as u32), text)).expect("an entry");
            ends.push(
                std::fs::metadata(home.path().join(&rel))
                    .expect("size")
                    .len() as usize,
            );
        }
        let path = home.path().join(&rel);
        let full = std::fs::read(&path).expect("journal");
        let after = entry(&electra, at(11, 0), "After the tear.");
        let frontmatter = frontmatter_of(&after);
        let next = entry_bytes(&after);
        for cut in 0..=full.len() {
            std::fs::write(&path, &full[..cut]).expect("torn");
            put(home.path(), &after).expect("appended after the tear");
            let kept = ends.iter().copied().filter(|&end| end <= cut).max();
            let expected = match kept {
                Some(end) => [&full[..end], next.as_bytes()].concat(),
                None if cut >= frontmatter.len() => {
                    [&full[..frontmatter.len()], next.as_bytes()].concat()
                }
                None => [frontmatter.as_bytes(), next.as_bytes()].concat(),
            };
            assert_eq!(
                String::from_utf8_lossy(&std::fs::read(&path).expect("journal")),
                String::from_utf8_lossy(&expected),
                "cut at {cut}"
            );
        }
    }

    /// What a person added to a journal is never cut: the append is
    /// refused and the file keeps every byte.
    #[test]
    fn a_journal_keeper_did_not_write_is_left_alone() {
        let home = tempfile::tempdir().expect("home");
        let electra = HostSlug::new("electra").expect("slug");
        let rel = put(home.path(), &entry(&electra, at(9, 0), "Kept.")).expect("first");
        let path = home.path().join(&rel);
        for added in ["\nA person's line", "\n## 10:00 · by hand\n\nNo length."] {
            let text = std::fs::read_to_string(&path).expect("journal") + added;
            std::fs::write(&path, &text).expect("edited");
            assert!(put(home.path(), &entry(&electra, at(9, 30), "More.")).is_err());
            assert_eq!(std::fs::read_to_string(&path).expect("journal"), text);
            std::fs::write(&path, text.strip_suffix(added).expect("restored")).expect("put back");
        }
    }

    /// R120: the claim is asked before the folder is made, right before a
    /// torn tail is cut and right before the entry is written; once it is
    /// lost, nothing after it happens.
    #[test]
    fn a_lost_claim_cuts_and_writes_nothing() {
        let home = tempfile::tempdir().expect("home");
        let electra = HostSlug::new("electra").expect("slug");
        let rel = put(home.path(), &entry(&electra, at(9, 0), "Kept.")).expect("first");
        let path = home.path().join(&rel);
        let whole_text = std::fs::read_to_string(&path).expect("journal");
        let torn = format!("{whole_text}\n## 09:10 · 2026-10-06-inbox <!-- 9 -->\n\nTor");
        for (allowed, after) in [
            (0, torn.as_str()),
            (1, torn.as_str()),
            (2, whole_text.as_str()),
        ] {
            std::fs::write(&path, &torn).expect("torn");
            let asked = Cell::new(0);
            let may_write = || {
                asked.set(asked.get() + 1);
                asked.get() <= allowed
            };
            assert_eq!(
                append(
                    home.path(),
                    &entry(&electra, at(9, 30), "Late."),
                    &may_write
                ),
                Err(NO_CLAIM.to_owned()),
                "{allowed}"
            );
            assert_eq!(
                std::fs::read_to_string(&path).expect("journal"),
                after,
                "{allowed}"
            );
        }
        let fresh = tempfile::tempdir().expect("fresh home");
        assert!(append(fresh.path(), &entry(&electra, at(9, 0), "x"), &|| false).is_err());
        assert!(!fresh.path().join(DIR).exists());
    }

    /// One writer per file: an append waits for the file's lock, so two
    /// sessions' first appends write one frontmatter and both entries.
    #[test]
    fn journal_appends_take_turns() {
        let home = tempfile::tempdir().expect("home");
        let electra = HostSlug::new("electra").expect("slug");
        let rel = put(home.path(), &entry(&electra, at(9, 0), "First.")).expect("first");
        let path = home.path().join(&rel);
        let before = std::fs::read(&path).expect("journal");
        let held = File::open(&path).expect("open");
        FileExt::lock_exclusive(&held).expect("lock");
        std::thread::scope(|scope| {
            let waiting = scope.spawn(|| put(home.path(), &entry(&electra, at(9, 5), "Waited.")));
            std::thread::sleep(std::time::Duration::from_millis(300));
            assert_eq!(
                std::fs::read(&path).expect("journal"),
                before,
                "nothing written while another writer holds the file"
            );
            drop(held);
            waiting.join().expect("joined").expect("written once free");
        });
        assert!(std::fs::read_to_string(&path)
            .expect("journal")
            .ends_with("\n\nWaited.\n"));

        let later = Utc
            .with_ymd_and_hms(2026, 10, 8, 9, 0, 0)
            .single()
            .expect("t");
        std::thread::scope(|scope| {
            for n in 0..8 {
                let electra = &electra;
                let home = home.path();
                scope.spawn(move || {
                    let text = format!("Session {n}.");
                    put(home, &entry(electra, later, &text)).expect("appended");
                });
            }
        });
        let text =
            std::fs::read_to_string(home.path().join(file_of(&electra, later))).expect("journal");
        assert_eq!(text.matches("type: journal").count(), 1, "{text}");
        assert_eq!(text.matches("\n## ").count(), 8, "{text}");
    }

    /// A first append publishes the folder entries it made — the journal
    /// folder's in the home, the day file's in the folder — and a later
    /// append to the same file syncs no folder.
    #[test]
    fn a_new_journal_file_is_published_durably() {
        let home = tempfile::tempdir().expect("home");
        let electra = HostSlug::new("electra").expect("slug");
        let synced = Mutex::new(Vec::new());
        let sync_dir = |dir: &Path| {
            synced.lock().expect("lock").push(dir.to_owned());
            Ok::<(), std::io::Error>(())
        };
        let take = || std::mem::take(&mut *synced.lock().expect("lock"));
        let dir = home.path().join(DIR);
        append_with(
            home.path(),
            &entry(&electra, at(9, 0), "a"),
            &|| true,
            &sync_dir,
        )
        .expect("first");
        assert_eq!(take(), [home.path().to_owned(), dir.clone()]);
        append_with(
            home.path(),
            &entry(&electra, at(9, 5), "b"),
            &|| true,
            &sync_dir,
        )
        .expect("second");
        assert!(take().is_empty());
        let tomorrow = Utc
            .with_ymd_and_hms(2026, 10, 7, 9, 0, 0)
            .single()
            .expect("t");
        append_with(
            home.path(),
            &entry(&electra, tomorrow, "c"),
            &|| true,
            &sync_dir,
        )
        .expect("new day");
        assert_eq!(take(), [dir]);
    }

    /// The journal's folder or file as a link is refused, never followed.
    #[test]
    fn a_linked_journal_is_refused() {
        let home = tempfile::tempdir().expect("home");
        let elsewhere = tempfile::tempdir().expect("elsewhere");
        std::os::unix::fs::symlink(elsewhere.path(), home.path().join(DIR)).expect("link");
        let electra = HostSlug::new("electra").expect("slug");
        assert!(put(home.path(), &entry(&electra, at(9, 0), "x")).is_err());
        assert_eq!(std::fs::read_dir(elsewhere.path()).expect("dir").count(), 0);
    }
}
