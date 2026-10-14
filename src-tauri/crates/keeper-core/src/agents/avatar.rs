//! An agent's picture: a file in its home (story 100.4, R384).
//!
//! A home may hold `avatar.svg` (preferred), else `avatar.png`, `avatar.webp`
//! or `avatar.jpg`, each at most [`MAX_BYTES`]. [`scan`] decides which file is
//! the picture from what the caller read — it touches no disk: the agents
//! zone's reader (`keeper-agent`'s `zone::read_avatar`) reaches every file
//! through `browse::resolve_known` and hands the bytes here — a git-LFS
//! pointer's object's bytes when the object is on the device, its size read
//! from the pointer first. A candidate that is too large, cannot be read or
//! is not the format its name says ([`sniff`], the reading every receiving
//! device applies), and any other `avatar.*` beside them, is an
//! [`AvatarProblem`]: a sentence for the agent's settings row and a WARN line
//! (R381), never a picture. A pointer whose object is not on the device is
//! no picture there: the agent keeps its letter and nothing is published.
//!
//! Where the zone is on the device the picture is drawn from the drive
//! (`keeper-file://`, [`crate::file_asset::file_url_in`]). Elsewhere — the
//! phone, another Mac, another client — it is the agent's Matrix profile
//! avatar, which a host serving the agent sets from the same file when its
//! content hash changes ([`publish`]) — only while it holds the agent's
//! picture claim in the control room (`keeper-agent`'s `avatar`, R399) —
//! recording what it published in the agent's own account data
//! ([`AvatarRecordContent`]) so neither a tick, a restart nor another host
//! uploads an unchanged file again. A device draws that profile avatar from
//! the original media, never a server thumbnail, and never more than
//! [`MAX_BYTES`] of it ([`crate::media::fetch_agent_avatar`]).
//!
//! An SVG is only ever an `<img>` source served as `image/svg+xml` with
//! `nosniff` (`note_protocol::mime_for`, the shell's `keeper-media` avatar
//! answer): it is never inlined into the DOM, so its script never runs.

use std::future::Future;
use std::time::{Duration, Instant};

use matrix_sdk::ruma::events::macros::EventContent;
use matrix_sdk::ruma::{MxcUri, OwnedMxcUri};
use matrix_sdk::Client;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::agents::events::CONTENT_VERSION;

/// The picture's file names in a home, the preferred first.
pub const FILE_NAMES: [&str; 4] = ["avatar.svg", "avatar.png", "avatar.webp", "avatar.jpg"];

/// A picture's largest size, in bytes: of the file on the drive, of the
/// object its git-LFS pointer names, and of every profile avatar a device
/// downloads.
pub const MAX_BYTES: u64 = 256 * 1024;

/// How far into a picture's bytes an SVG's root element may start, past a
/// byte-order mark, an XML declaration, comments, processing instructions
/// and a doctype.
pub const SVG_PROLOGUE_MAX: usize = 4 * 1024;

/// How long after a failed publication the host tries the same file again.
pub const RETRY_AFTER: Duration = Duration::from_secs(10 * 60);

/// How long a host waits to look again when another host held the agent's
/// picture claim: that host is publishing, and its record is read then.
pub const HELD_ELSEWHERE_RETRY: Duration = Duration::from_secs(60);

/// The `Content-Type` of one of [`FILE_NAMES`]: from the extension, never
/// sniffed.
pub fn mime_of(file: &str) -> Option<&'static str> {
    Some(match file {
        "avatar.svg" => "image/svg+xml",
        "avatar.png" => "image/png",
        "avatar.webp" => "image/webp",
        "avatar.jpg" => "image/jpeg",
        _ => return None,
    })
}

/// What a home's reader found at one of [`FILE_NAMES`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Candidate {
    /// Nothing is there.
    Absent,
    /// The picture's bytes, read whole: the file's, or the object's its
    /// git-LFS pointer names (the reader refuses past [`MAX_BYTES`]).
    Bytes(Vec<u8>),
    /// A git-LFS pointer whose object is not on this device: no picture
    /// here — the agent keeps its letter — and nothing is published.
    NotHere,
    /// The file, or the object its pointer names, is larger than
    /// [`MAX_BYTES`]: its size.
    TooLarge(u64),
    /// The file is there and could not be read: why.
    Unreadable(String),
}

/// The picture a home holds.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AvatarFile {
    /// One of [`FILE_NAMES`].
    pub file: &'static str,
    pub mime: &'static str,
    /// SHA-256 of the picture's bytes, lowercase hex: the content's identity.
    pub hash: String,
    /// The picture's bytes.
    pub bytes: Vec<u8>,
}

/// Why a file in a home is not the agent's picture.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum AvatarProblem {
    TooLarge {
        file: String,
        bytes: u64,
    },
    Unreadable {
        file: String,
        why: String,
    },
    /// Its bytes are not the format its name says, as every device reads
    /// them ([`sniff`]).
    NotItsFormat {
        file: String,
    },
    /// An `avatar.*` keeper does not read: another format, or another
    /// spelling of one it does.
    Ignored {
        file: String,
    },
}

impl AvatarProblem {
    /// The file it is about, home-relative.
    pub fn file(&self) -> &str {
        match self {
            AvatarProblem::TooLarge { file, .. }
            | AvatarProblem::Unreadable { file, .. }
            | AvatarProblem::NotItsFormat { file }
            | AvatarProblem::Ignored { file } => file,
        }
    }

    /// The sentence the agent's settings row says, naming the agent.
    pub fn sentence(&self, agent: &str) -> String {
        match self {
            AvatarProblem::TooLarge { file, bytes } => format!(
                "{agent}'s {file} is {} KiB; keeper draws a picture of at most {} KiB, so it is not used.",
                bytes.div_ceil(1024),
                MAX_BYTES / 1024
            ),
            AvatarProblem::Unreadable { file, why } => {
                format!("{agent}'s {file} could not be read ({why}), so it is not used.")
            }
            AvatarProblem::NotItsFormat { file } => format!(
                "{agent}'s {file} is not {}, so it is not used{}.",
                format_name(file),
                if file.as_str() == "avatar.svg" {
                    format!(
                        " (its <svg> element must begin within its first {} KiB)",
                        SVG_PROLOGUE_MAX / 1024
                    )
                } else {
                    String::new()
                }
            ),
            AvatarProblem::Ignored { file } => format!(
                "{agent}'s {file} is not a picture keeper reads; name it {}.",
                names_sentence()
            ),
        }
    }
}

/// What one of [`FILE_NAMES`] must hold, for a sentence.
fn format_name(file: &str) -> &'static str {
    match file {
        "avatar.svg" => "an SVG image",
        "avatar.png" => "a PNG image",
        "avatar.webp" => "a WebP image",
        _ => "a JPEG image",
    }
}

fn names_sentence() -> String {
    let (last, rest) = FILE_NAMES.split_last().unwrap_or((&"", &[]));
    format!("{}, or {last}", rest.join(", "))
}

/// A home's picture and what keeper did not use.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AvatarScan {
    pub avatar: Option<AvatarFile>,
    pub problems: Vec<AvatarProblem>,
    /// The home's picture is a git-LFS pointer whose object is not on this
    /// device (R399 #6, R414): this device draws the letter — never the
    /// agent's Matrix profile avatar, which may be an older picture.
    pub not_here: bool,
}

impl AvatarScan {
    /// The settings row's sentence: every problem, in order, naming `agent`;
    /// `None` when there is none.
    pub fn sentence(&self, agent: &str) -> Option<String> {
        if self.problems.is_empty() {
            return None;
        }
        Some(
            self.problems
                .iter()
                .map(|problem| problem.sentence(agent))
                .collect::<Vec<_>>()
                .join(" "),
        )
    }
}

/// Decide a home's picture. `names` are the home's entries; `read` reads
/// one of [`FILE_NAMES`] and is asked in preference order until one is the
/// picture: a candidate too large, unreadable or not its name's format is a
/// problem and the next is asked; a pointer whose object is not here ends
/// the asking with no picture. Every other entry spelled `avatar.*` (any
/// case) is ignored with a problem naming it.
pub fn scan(names: &[String], mut read: impl FnMut(&'static str) -> Candidate) -> AvatarScan {
    let mut found = AvatarScan::default();
    for file in FILE_NAMES {
        let Some(mime) = mime_of(file) else { continue };
        let bytes = match read(file) {
            Candidate::Absent => continue,
            Candidate::Bytes(bytes) => bytes,
            Candidate::NotHere => {
                found.not_here = true;
                break;
            }
            Candidate::TooLarge(size) => {
                found.problems.push(AvatarProblem::TooLarge {
                    file: file.to_owned(),
                    bytes: size,
                });
                continue;
            }
            Candidate::Unreadable(why) => {
                found.problems.push(AvatarProblem::Unreadable {
                    file: file.to_owned(),
                    why,
                });
                continue;
            }
        };
        if bytes.len() as u64 > MAX_BYTES {
            found.problems.push(AvatarProblem::TooLarge {
                file: file.to_owned(),
                bytes: bytes.len() as u64,
            });
            continue;
        }
        if sniff(&bytes) != Some(mime) {
            found.problems.push(AvatarProblem::NotItsFormat {
                file: file.to_owned(),
            });
            continue;
        }
        found.avatar = Some(AvatarFile {
            file,
            mime,
            hash: hex::encode(Sha256::digest(&bytes)),
            bytes,
        });
        break;
    }
    let mut ignored: Vec<AvatarProblem> = names
        .iter()
        .filter(|name| is_avatar_spelling(name) && !FILE_NAMES.contains(&name.as_str()))
        .map(|name| AvatarProblem::Ignored { file: name.clone() })
        .collect();
    ignored.sort();
    found.problems.extend(ignored);
    found
}

/// Whether `name` is spelled like a picture's file: `avatar.` and anything,
/// in any case.
fn is_avatar_spelling(name: &str) -> bool {
    name.get(..7)
        .is_some_and(|stem| stem.eq_ignore_ascii_case("avatar."))
}

/// A short version of `hash` for a URL's cache key: a changed picture is a
/// new URL, so a webview never draws the old one from its cache.
pub fn version(hash: &str) -> &str {
    hash.get(..12).unwrap_or(hash)
}

/// The `Content-Type` of a picture's bytes, from the four formats a picture
/// may be, or `None` for anything else — which is then not a picture: not
/// served on a device, not used from a home. Matrix media carries no type
/// the SDK hands back, and a profile avatar may have been set by any
/// client, so its bytes are recognised, never trusted.
pub fn sniff(bytes: &[u8]) -> Option<&'static str> {
    if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        return Some("image/png");
    }
    if bytes.starts_with(&[0xFF, 0xD8, 0xFF]) {
        return Some("image/jpeg");
    }
    if bytes.len() >= 12 && &bytes[..4] == b"RIFF" && &bytes[8..12] == b"WEBP" {
        return Some("image/webp");
    }
    svg_root(&bytes[..bytes.len().min(SVG_PROLOGUE_MAX)]).then_some("image/svg+xml")
}

/// Whether `head` is an XML document whose root element is `svg` (with or
/// without a namespace prefix): past a byte-order mark, whitespace, an XML
/// declaration and other processing instructions, comments and a doctype —
/// its internal subset included, never resolved — the first element is
/// `<svg`. A prologue that does not end inside `head` is not recognised.
fn svg_root(head: &[u8]) -> bool {
    let mut rest = head.strip_prefix(b"\xEF\xBB\xBF").unwrap_or(head);
    loop {
        rest = rest.trim_ascii_start();
        let skipped = if rest.starts_with(b"<?") {
            after(&rest[2..], b"?>")
        } else if rest.starts_with(b"<!--") {
            after(&rest[4..], b"-->")
        } else if rest.len() >= 9 && rest[..9].eq_ignore_ascii_case(b"<!DOCTYPE") {
            after_doctype(&rest[9..])
        } else {
            break;
        };
        match skipped {
            Some(next) => rest = next,
            None => return false,
        }
    }
    let Some(element) = rest.strip_prefix(b"<") else {
        return false;
    };
    let name_end = element
        .iter()
        .position(|byte| byte.is_ascii_whitespace() || matches!(byte, b'>' | b'/'))
        .unwrap_or(element.len());
    if name_end == element.len() {
        return false;
    }
    let name = &element[..name_end];
    let local = name
        .iter()
        .rposition(|byte| *byte == b':')
        .map_or(name, |colon| &name[colon + 1..]);
    local == b"svg"
}

/// What follows the first `end` in `text`, or `None` without one.
fn after<'a>(text: &'a [u8], end: &[u8]) -> Option<&'a [u8]> {
    text.windows(end.len())
        .position(|window| window == end)
        .map(|at| &text[at + end.len()..])
}

/// What follows a doctype's closing `>`, `text` starting after
/// `<!DOCTYPE`: a `>` inside its internal subset (`[…]`) or a quoted
/// literal does not close it.
fn after_doctype(text: &[u8]) -> Option<&[u8]> {
    let mut depth = 0usize;
    let mut quote: Option<u8> = None;
    for (at, byte) in text.iter().enumerate() {
        match (quote, *byte) {
            (Some(open), byte) if byte == open => quote = None,
            (Some(_), _) => {}
            (None, b'"' | b'\'') => quote = Some(*byte),
            (None, b'[') => depth += 1,
            (None, b']') => depth = depth.saturating_sub(1),
            (None, b'>') if depth == 0 => return Some(&text[at + 1..]),
            (None, _) => {}
        }
    }
    None
}

/// `dev.keeper.agent.avatar` (global account data of the agent's own
/// account): the picture its host last published — the file's hash and the
/// profile avatar it became. A host whose file hashes the same, and whose
/// profile still holds that `mxc`, uploads nothing.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, EventContent)]
#[ruma_event(type = "dev.keeper.agent.avatar", kind = GlobalAccountData)]
pub struct AvatarRecordContent {
    pub v: u32,
    pub hash: String,
    pub mxc: OwnedMxcUri,
}

/// Whether the profile already shows the file of `hash`: the record names
/// that hash and the `mxc` the profile holds now. A profile someone changed
/// since, or a record of another version, is published again.
pub fn is_current(
    record: Option<&AvatarRecordContent>,
    profile: Option<&MxcUri>,
    hash: &str,
) -> bool {
    record.is_some_and(|record| {
        record.v == CONTENT_VERSION && record.hash == hash && Some(&*record.mxc) == profile
    })
}

/// What [`publish`] did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Published {
    /// The profile already showed this file: nothing was uploaded.
    Unchanged,
    /// The file was uploaded and is the profile avatar now.
    Uploaded(OwnedMxcUri),
}

/// The agent's own account on its homeserver, as a publication reads and
/// writes it. Each error is a sentence's clause: no URL, no token.
pub trait AvatarPort: Sync {
    /// The agent's [`AvatarRecordContent`], when it has one this build reads.
    fn record(&self) -> impl Future<Output = Result<Option<AvatarRecordContent>, String>> + Send;
    /// The profile avatar the agent's profile holds now.
    fn profile(&self) -> impl Future<Output = Result<Option<OwnedMxcUri>, String>> + Send;
    /// Upload `bytes`, typed `mime`.
    fn upload(
        &self,
        mime: &'static str,
        bytes: Vec<u8>,
    ) -> impl Future<Output = Result<OwnedMxcUri, String>> + Send;
    /// Make `mxc` the profile avatar.
    fn set_profile(&self, mxc: &MxcUri) -> impl Future<Output = Result<(), String>> + Send;
    /// Write `record` as the agent's [`AvatarRecordContent`].
    fn write_record(
        &self,
        record: AvatarRecordContent,
    ) -> impl Future<Output = Result<(), String>> + Send;
}

impl AvatarPort for Client {
    async fn record(&self) -> Result<Option<AvatarRecordContent>, String> {
        Ok(self
            .account()
            .fetch_account_data_static::<AvatarRecordContent>()
            .await
            .map_err(|error| format!("its record could not be read: {error}"))?
            .and_then(|raw| raw.deserialize().ok()))
    }

    async fn profile(&self) -> Result<Option<OwnedMxcUri>, String> {
        self.account()
            .get_avatar_url()
            .await
            .map_err(|error| format!("its profile could not be read: {error}"))
    }

    async fn upload(&self, mime: &'static str, bytes: Vec<u8>) -> Result<OwnedMxcUri, String> {
        let mime: mime::Mime = mime
            .parse()
            .map_err(|_| format!("{mime} is not a media type"))?;
        Ok(self
            .media()
            .upload(&mime, bytes, None)
            .await
            .map_err(|error| format!("the upload failed: {error}"))?
            .content_uri)
    }

    async fn set_profile(&self, mxc: &MxcUri) -> Result<(), String> {
        self.account()
            .set_avatar_url(Some(mxc))
            .await
            .map_err(|error| format!("the profile could not be set: {error}"))
    }

    async fn write_record(&self, record: AvatarRecordContent) -> Result<(), String> {
        self.account()
            .set_account_data(record)
            .await
            .map(|_| ())
            .map_err(|error| format!("its record could not be written: {error}"))
    }
}

/// Whether the agent's profile shows the file of `hash` now, by its record
/// and its profile as the server holds them: what a host asks before it
/// takes the agent's picture claim, so an unchanged file — a file put back
/// to what was published included — settles without a claim or an upload.
pub async fn is_published(port: &impl AvatarPort, hash: &str) -> Result<bool, String> {
    let record = port.record().await?;
    let profile = port.profile().await?;
    Ok(is_current(record.as_ref(), profile.as_deref(), hash))
}

/// Make `file` the profile avatar of `port`'s agent, unless the record and
/// the profile say it already is: read them from the server again, and only
/// when they do not match upload the bytes (typed by the file's extension),
/// set the avatar and write the record. Called by the holder of the agent's
/// picture claim. The error is a sentence's clause: no URL, no token.
pub async fn publish(port: &impl AvatarPort, file: &AvatarFile) -> Result<Published, String> {
    if is_published(port, &file.hash).await? {
        return Ok(Published::Unchanged);
    }
    let mxc = port.upload(file.mime, file.bytes.clone()).await?;
    port.set_profile(&mxc).await?;
    port.write_record(AvatarRecordContent {
        v: CONTENT_VERSION,
        hash: file.hash.clone(),
        mxc: mxc.clone(),
    })
    .await?;
    Ok(Published::Uploaded(mxc))
}

/// A host's memory of one agent's publication: the hash its profile is
/// known to show — set only by a publication that succeeded — and when the
/// file of a hash not settled is to be tried again, so a scan asks the
/// server only when the file changed, a failure is tried again after
/// [`RETRY_AFTER`] and a claim held elsewhere after
/// [`HELD_ELSEWHERE_RETRY`].
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct PublishMemo {
    settled: Option<String>,
    again: Option<(String, Instant)>,
}

/// One host's failed publication of an agent's picture, as a settings row
/// says it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PublishFailure {
    /// The host whose publication failed, when it is not this device.
    pub host: Option<String>,
    /// What failed: a [`publish`] clause.
    pub error: String,
}

/// The settings row's sentence for `failure`, naming the agent.
pub fn publish_failed(agent: &str, failure: &PublishFailure) -> String {
    let minutes = RETRY_AFTER.as_secs() / 60;
    let error = &failure.error;
    match &failure.host {
        None => format!(
            "{agent}'s picture could not be set as its Matrix profile avatar ({error}); devices without the drive keep showing its letter. keeper tries again in {minutes} minutes."
        ),
        Some(host) => format!(
            "{agent}'s picture could not be set as its Matrix profile avatar on {host} ({error}); devices without the drive keep showing its letter. {host} tries again in {minutes} minutes."
        ),
    }
}

impl PublishMemo {
    /// Whether the file of `hash` is to be published at `now`.
    pub fn due(&self, hash: &str, now: Instant) -> bool {
        if self.settled.as_deref() == Some(hash) {
            return false;
        }
        match &self.again {
            Some((waiting, at)) if waiting == hash => now >= *at,
            _ => true,
        }
    }

    /// The profile shows the file of `hash`.
    pub fn settle(&mut self, hash: &str) {
        self.settled = Some(hash.to_owned());
        self.again = None;
    }

    /// Publishing the file of `hash` failed at `now`. Whatever was settled
    /// is not known any more — the attempt may have changed the profile —
    /// so the next file, the settled one put back included, is asked of
    /// the server.
    pub fn fail(&mut self, hash: &str, now: Instant) {
        self.settled = None;
        self.again = Some((hash.to_owned(), now + RETRY_AFTER));
    }

    /// Another host held the agent's picture claim at `now`: it publishes,
    /// and the file of `hash` is looked at again after
    /// [`HELD_ELSEWHERE_RETRY`].
    pub fn held_elsewhere(&mut self, hash: &str, now: Instant) {
        self.settled = None;
        self.again = Some((hash.to_owned(), now + HELD_ELSEWHERE_RETRY));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names(list: &[&str]) -> Vec<String> {
        list.iter().map(|name| (*name).to_owned()).collect()
    }

    const PNG: &[u8] = b"\x89PNG\r\n\x1a\nrest";
    const JPG: &[u8] = &[0xFF, 0xD8, 0xFF, 0xE0, 1];

    /// `len` bytes of an SVG document.
    fn svg_of(len: usize) -> Vec<u8> {
        let mut bytes = b"<svg xmlns=\"http://www.w3.org/2000/svg\"><!--".to_vec();
        let close = b"--></svg>";
        bytes.resize(len - close.len(), b'x');
        bytes.extend_from_slice(close);
        bytes
    }

    #[test]
    fn the_svg_is_preferred_and_the_others_are_not_read() {
        let mut asked = Vec::new();
        let scan = scan(&names(&["avatar.svg", "avatar.png"]), |file| {
            asked.push(file);
            Candidate::Bytes(format!("<svg>{file}</svg>").into_bytes())
        });
        let avatar = scan.avatar.expect("a picture");
        assert_eq!((avatar.file, avatar.mime), ("avatar.svg", "image/svg+xml"));
        assert_eq!(asked, ["avatar.svg"]);
        assert!(scan.problems.is_empty());
    }

    #[test]
    fn a_too_large_or_unreadable_file_is_a_problem_and_the_next_is_the_picture() {
        let scan = scan(&[], |file| match file {
            "avatar.svg" => Candidate::TooLarge(MAX_BYTES + 1),
            "avatar.png" => Candidate::Unreadable("permission denied".to_owned()),
            "avatar.webp" => Candidate::Absent,
            _ => Candidate::Bytes(JPG.to_vec()),
        });
        assert_eq!(
            scan.avatar.map(|a| (a.file, a.mime)),
            Some(("avatar.jpg", "image/jpeg"))
        );
        assert_eq!(
            scan.problems,
            [
                AvatarProblem::TooLarge {
                    file: "avatar.svg".into(),
                    bytes: MAX_BYTES + 1
                },
                AvatarProblem::Unreadable {
                    file: "avatar.png".into(),
                    why: "permission denied".into()
                },
            ]
        );
    }

    #[test]
    fn the_size_bound_is_256_kib_inclusive() {
        let at = scan(&[], |_| Candidate::Bytes(svg_of(MAX_BYTES as usize)));
        assert!(at.avatar.is_some(), "{:?}", at.problems);
        let over = scan(&[], |file| {
            if file == "avatar.svg" {
                Candidate::Bytes(svg_of(MAX_BYTES as usize + 1))
            } else {
                Candidate::Absent
            }
        });
        assert_eq!(over.avatar, None);
        assert_eq!(
            over.problems,
            [AvatarProblem::TooLarge {
                file: "avatar.svg".into(),
                bytes: MAX_BYTES + 1
            }]
        );
    }

    /// R399 #8: which files are ignored, never how the sentence reads.
    #[test]
    fn any_other_avatar_spelling_is_ignored_and_named() {
        let scan = scan(
            &names(&[
                "avatar.gif",
                "Avatar.PNG",
                "avatar.jpeg",
                "avatars",
                "SOUL.md",
                "avatar.svg",
            ]),
            |_| Candidate::Absent,
        );
        assert_eq!(scan.avatar, None);
        let ignored: Vec<&str> = scan.problems.iter().map(AvatarProblem::file).collect();
        assert_eq!(ignored, ["Avatar.PNG", "avatar.gif", "avatar.jpeg"]);
    }

    /// R399 #6: a pointer whose object is not on this device ends the
    /// asking — no picture here, nothing to publish — even with a usable
    /// file after it.
    #[test]
    fn a_pointer_whose_object_is_not_here_leaves_the_letter() {
        let scan = scan(&[], |file| match file {
            "avatar.svg" => Candidate::NotHere,
            _ => Candidate::Bytes(PNG.to_vec()),
        });
        assert_eq!(scan.avatar, None);
        assert!(scan.problems.is_empty());
        assert!(scan.not_here);
    }

    /// R399 #7: what the home's reading uses is what every device serves —
    /// a file not of its name's format is a problem and the next is asked.
    #[test]
    fn a_file_not_of_its_names_format_is_a_problem_and_the_next_is_the_picture() {
        let scan = scan(&[], |file| match file {
            "avatar.svg" => Candidate::Bytes(b"<html><svg/></html>".to_vec()),
            "avatar.png" => Candidate::Bytes(JPG.to_vec()),
            "avatar.webp" => Candidate::Absent,
            _ => Candidate::Bytes(JPG.to_vec()),
        });
        assert_eq!(scan.avatar.map(|a| a.file), Some("avatar.jpg"));
        assert_eq!(
            scan.problems,
            [
                AvatarProblem::NotItsFormat {
                    file: "avatar.svg".into()
                },
                AvatarProblem::NotItsFormat {
                    file: "avatar.png".into()
                },
            ]
        );
    }

    #[test]
    fn the_hash_is_the_contents_and_moves_with_it() {
        let one = scan(&[], |_| Candidate::Bytes(b"<svg/>".to_vec()))
            .avatar
            .expect("one");
        let same = scan(&[], |_| Candidate::Bytes(b"<svg/>".to_vec()))
            .avatar
            .expect("same");
        let other = scan(&[], |_| Candidate::Bytes(b"<svg />".to_vec()))
            .avatar
            .expect("other");
        assert_eq!(one.hash, same.hash);
        assert_ne!(one.hash, other.hash);
        assert_eq!(one.hash.len(), 64);
    }

    #[test]
    fn sniff_recognises_the_four_formats_and_nothing_else() {
        assert_eq!(sniff(PNG), Some("image/png"));
        assert_eq!(sniff(JPG), Some("image/jpeg"));
        assert_eq!(sniff(b"RIFF\0\0\0\0WEBPVP8 "), Some("image/webp"));
        assert_eq!(sniff(b"<html><body>no</body></html>"), None);
        assert_eq!(sniff(b"GIF89a"), None);
        assert_eq!(sniff(b"not <svg"), None);
    }

    /// R399 #7: a valid SVG keeps its prologue — a byte-order mark, an XML
    /// declaration, comments, a doctype with an internal subset — longer
    /// than a kilobyte, as long as its root begins within
    /// [`SVG_PROLOGUE_MAX`]; a document whose root is another element is
    /// no SVG, whatever it holds.
    #[test]
    fn an_svg_is_recognised_by_its_root_past_a_long_prologue() {
        let comment = format!("<!-- {} -->\n", "a > b, <svg> in words; ".repeat(100));
        assert!(comment.len() > 2048);
        let valid = format!(
            "\u{feff}<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n{comment}<!DOCTYPE svg PUBLIC \"-//W3C//DTD SVG 1.1//EN\" \"x>y.dtd\" [\n  <!ENTITY e \"<x>\">\n]>\n<?xml-stylesheet href=\"a.css\"?>\n<svg:svg xmlns:svg=\"http://www.w3.org/2000/svg\"/>"
        );
        assert_eq!(sniff(valid.as_bytes()), Some("image/svg+xml"));
        assert_eq!(
            sniff(b"<svg\nviewBox=\"0 0 1 1\"></svg>"),
            Some("image/svg+xml")
        );
        for refused in [
            "<html><svg></svg></html>",
            "<svgx/>",
            "<!-- never closed <svg/>",
            "<?xml version=\"1.0\"?><foreignObject><svg/></foreignObject>",
        ] {
            assert_eq!(sniff(refused.as_bytes()), None, "{refused}");
        }
        let past = format!("<!--{}--><svg/>", "x".repeat(SVG_PROLOGUE_MAX));
        assert_eq!(sniff(past.as_bytes()), None);
    }

    fn record(hash: &str, mxc: &str) -> AvatarRecordContent {
        AvatarRecordContent {
            v: CONTENT_VERSION,
            hash: hash.to_owned(),
            mxc: mxc.into(),
        }
    }

    #[test]
    fn the_profile_is_current_only_for_the_recorded_hash_and_mxc() {
        let mxc: &MxcUri = "mxc://example.org/a".into();
        let other: &MxcUri = "mxc://example.org/b".into();
        assert!(is_current(
            Some(&record("h", "mxc://example.org/a")),
            Some(mxc),
            "h"
        ));
        assert!(!is_current(
            Some(&record("h", "mxc://example.org/a")),
            Some(mxc),
            "changed"
        ));
        assert!(!is_current(
            Some(&record("h", "mxc://example.org/a")),
            Some(other),
            "h"
        ));
        assert!(!is_current(
            Some(&record("h", "mxc://example.org/a")),
            None,
            "h"
        ));
        assert!(!is_current(None, Some(mxc), "h"));
        let newer = AvatarRecordContent {
            v: CONTENT_VERSION + 1,
            ..record("h", "mxc://example.org/a")
        };
        assert!(!is_current(Some(&newer), Some(mxc), "h"));
    }

    #[test]
    fn a_settled_hash_is_not_due_and_a_failure_waits_ten_minutes() {
        let start = Instant::now();
        let mut memo = PublishMemo::default();
        assert!(memo.due("a", start));
        memo.fail("a", start);
        assert!(!memo.due("a", start + RETRY_AFTER - Duration::from_secs(1)));
        assert!(memo.due("a", start + RETRY_AFTER));
        assert!(
            memo.due("b", start),
            "a changed file is not held by an old failure"
        );
        memo.settle("a");
        assert!(!memo.due("a", start + RETRY_AFTER * 3));
        assert!(memo.due("b", start));
    }

    /// R399 #3: A settled, B failed — the attempt may have changed the
    /// profile — then the file put back to A is asked of the server at
    /// once, never held back by A's old settlement or B's backoff.
    #[test]
    fn a_failure_unsettles_so_the_file_put_back_is_asked_again() {
        let start = Instant::now();
        let mut memo = PublishMemo::default();
        memo.settle("a");
        assert!(!memo.due("a", start));
        memo.fail("b", start);
        assert!(!memo.due("b", start), "the failure waits");
        assert!(memo.due("a", start), "the settled file is asked again");
        let mut held = PublishMemo::default();
        held.settle("a");
        held.held_elsewhere("b", start);
        assert!(held.due("a", start));
        assert!(!held.due("b", start));
        assert!(held.due("b", start + HELD_ELSEWHERE_RETRY));
    }
}
