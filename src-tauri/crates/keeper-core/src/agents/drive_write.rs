//! `drive_write` (109.27, R-HQ-87): an agent's request to change one file of
//! a drive a person ticked, which waits for that person and is carried out by
//! their own keeper as one commit.
//!
//! Pure, and shared by both sides of the write: keeper-hq's runtime parses the
//! call, checks it and parks it; the deciding person's keeper parses the same
//! `args` again from the `tool_calls` row, recomputes the digest, shows the
//! preview and builds the commit. One parser, one set of caps, one summary and
//! one digest, so the two never disagree about what was asked.
//!
//! **Where a path may land is not decided here.** keeper-core never joins a
//! root and a path (AD-65): `keeper_sync::files_write::agent_write_check`
//! decides it over the checkout, on both sides, and its `WriteRefusal`
//! sentence travels verbatim as [`Refusal::Path`] (AD-159). This module checks
//! only what the arguments alone say, and what a file's current bytes say
//! about the change ([`Request::post_image`]).
//!
//! **Every write waits for a person at T3 and is never granted**: nothing here
//! classifies a call or offers a scope; the runtime builds the classification
//! by hand and the server refuses any other tier (contract § 2.3, § 6.1).

use std::fmt;

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::agents::approval::{binding_digest, row_checkpoint, FloatAt, Preconditions};
use crate::agents::tier::AgentTool;

/// The tool's name, as the model calls it and as `tool_calls.name` holds it.
pub const TOOL: &str = "drive_write";

/// The most UTF-8 bytes one text argument (`content`, `old_text`,
/// `new_text`, `text`) may hold.
pub const MAX_TEXT_BYTES: usize = 262_144;

/// The most bytes a file may hold after the change: a file a person can still
/// open in keeper's editor (`crate::text_file::TEXT_EDIT_MAX_BYTES`).
pub const MAX_FILE_BYTES: usize = 1_000_000;

/// The most bytes `path` or `to` may hold. Each of its names is held to
/// `keeper_sync::files_write::MAX_NAME_BYTES` where the path is checked.
pub const MAX_PATH_BYTES: usize = 1_024;

/// How many lines of context a preview hunk keeps around a change.
pub const PREVIEW_CONTEXT: usize = 3;

/// A side longer than this many lines is shown as [`Preview::whole`], not
/// as hunks.
pub const PREVIEW_MAX_LINES: usize = 5_000;

/// How many lines an [`Excerpt`] shows.
pub const PREVIEW_EXCERPT_LINES: usize = 200;

/// The most changed lines a preview diffs: past it the two sides share too
/// little for hunks to help, and the diff's memory grows with the square of
/// it, so the new text is shown as [`Preview::whole`].
pub const PREVIEW_MAX_EDITS: usize = 1_000;

/// What a call asks for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Op {
    Create,
    Replace,
    Edit,
    Append,
    Move,
    Delete,
}

impl Op {
    pub const ALL: [Op; 6] = [
        Op::Create,
        Op::Replace,
        Op::Edit,
        Op::Append,
        Op::Move,
        Op::Delete,
    ];

    /// The word the model sends.
    pub fn as_str(self) -> &'static str {
        match self {
            Op::Create => "create",
            Op::Replace => "replace",
            Op::Edit => "edit",
            Op::Append => "append",
            Op::Move => "move",
            Op::Delete => "delete",
        }
    }

    fn from_word(word: &str) -> Option<Op> {
        Op::ALL.into_iter().find(|op| op.as_str() == word)
    }

    /// The arguments this op takes beyond `drive`, `op` and `path`; it needs
    /// every one of them.
    fn takes(self) -> &'static [&'static str] {
        match self {
            Op::Create => &["content"],
            Op::Replace => &["expected", "content"],
            Op::Edit => &["expected", "old_text", "new_text"],
            Op::Append => &["expected", "text"],
            Op::Move => &["expected", "to"],
            Op::Delete => &["expected"],
        }
    }
}

/// The change itself, with the arguments only its op takes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Change {
    Create { content: String },
    Replace { content: String },
    Edit { old_text: String, new_text: String },
    Append { text: String },
    Move { to: String },
    Delete,
}

/// One `drive_write` call, read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Request {
    pub drive: String,
    /// Drive-relative, `/`-joined, as the model sent it: checked for where it
    /// lands by `keeper_sync::files_write::agent_write_check`, never here.
    pub path: String,
    /// The git blob id of the file the agent read; `None` only for `create`.
    pub expected: Option<String>,
    pub change: Change,
}

/// Why a call is refused. The model is told [`fmt::Display`]'s sentence;
/// [`Refusal::code`] names it.
///
/// keeper-sync's answers map onto these the same way on both sides:
/// `WriteRefusal::Missing` is [`Refusal::Absent`], `WriteRefusal::Exists` is
/// [`Refusal::Exists`] for `path` and [`Refusal::ToExists`] for `to`,
/// `WriteRefusal::IsDirectory` is [`Refusal::Folder`], and every other
/// `WriteRefusal` or `BrowseRefusal` is [`Refusal::Path`] with its own
/// sentence.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Refusal {
    /// `dw.args`: the arguments do not read.
    Args { why: String },
    /// `dw.unticked`
    Unticked { drive: String },
    /// `dw.unreadable`
    Unreadable { drive: String, why: String },
    /// `dw.path`: keeper-sync's sentence, verbatim.
    Path { sentence: String },
    /// `dw.absent`
    Absent { path: String },
    /// `dw.exists`
    Exists { path: String },
    /// `dw.to_exists`
    ToExists { to: String },
    /// `dw.stale`
    Stale {
        path: String,
        expected: String,
        now: String,
    },
    /// `dw.binary`
    Binary { path: String },
    /// `dw.folder`
    Folder { path: String },
    /// `dw.edit_none`
    EditNone { path: String },
    /// `dw.edit_many`
    EditMany { path: String, n: usize },
    /// `dw.arg_size`
    ArgSize { arg: &'static str, n: usize },
    /// `dw.file_size`
    FileSize { path: String, n: usize },
    /// `dw.nul`
    Nul { arg: &'static str },
    /// `dw.noop`
    Noop { what: &'static str },
    /// `dw.trust`
    Trust { why: String },
}

impl Refusal {
    /// The refusal's code (contract § 7.3).
    pub fn code(&self) -> &'static str {
        match self {
            Self::Args { .. } => "dw.args",
            Self::Unticked { .. } => "dw.unticked",
            Self::Unreadable { .. } => "dw.unreadable",
            Self::Path { .. } => "dw.path",
            Self::Absent { .. } => "dw.absent",
            Self::Exists { .. } => "dw.exists",
            Self::ToExists { .. } => "dw.to_exists",
            Self::Stale { .. } => "dw.stale",
            Self::Binary { .. } => "dw.binary",
            Self::Folder { .. } => "dw.folder",
            Self::EditNone { .. } => "dw.edit_none",
            Self::EditMany { .. } => "dw.edit_many",
            Self::ArgSize { .. } => "dw.arg_size",
            Self::FileSize { .. } => "dw.file_size",
            Self::Nul { .. } => "dw.nul",
            Self::Noop { .. } => "dw.noop",
            Self::Trust { .. } => "dw.trust",
        }
    }

    fn args(why: impl Into<String>) -> Self {
        Self::Args { why: why.into() }
    }
}

/// Said after every refusal that does not say so already: the call was
/// refused before anyone was asked.
const NOTHING_ASKED: &str = "Nothing was asked.";

impl fmt::Display for Refusal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Args { why } => write!(f, "drive_write's arguments do not read: {why}.")?,
            Self::Unticked { drive } => write!(
                f,
                "{drive} is not a drive ticked for this session. A person ticks the drives an \
                 agent may use."
            )?,
            Self::Unreadable { drive, why } => {
                write!(f, "{drive}'s tracked files could not be read here ({why}).")?
            }
            Self::Path { sentence } => f.write_str(sentence)?,
            Self::Absent { path } => write!(
                f,
                "\"{path}\" is not in this drive; create makes a new file."
            )?,
            Self::Exists { path } => write!(
                f,
                "\"{path}\" already exists; read it, then replace, edit or append with its \
                 expected."
            )?,
            Self::ToExists { to } => {
                write!(f, "\"{to}\" already exists; nothing is moved over a file.")?
            }
            Self::Stale {
                path,
                expected,
                now,
            } => write!(
                f,
                "\"{path}\" changed since you read it (you sent {expected}; it is {now} here). \
                 Read it again, then ask again."
            )?,
            Self::Binary { path } => write!(
                f,
                "\"{path}\" is not text, so it can only be moved or deleted."
            )?,
            Self::Folder { path } => write!(
                f,
                "\"{path}\" is a folder; drive_write changes one file. Create a file inside it \
                 to make a folder."
            )?,
            Self::EditNone { path } => write!(
                f,
                "old_text does not appear in \"{path}\"; read it again and quote the passage \
                 exactly."
            )?,
            Self::EditMany { path, n } => write!(
                f,
                "old_text appears {n} times in \"{path}\"; quote enough around it to name one \
                 place."
            )?,
            Self::ArgSize { arg, n } => write!(
                f,
                "{arg} is {n} bytes; one call sends at most {MAX_TEXT_BYTES} bytes of new text."
            )?,
            Self::FileSize { path, n } => write!(
                f,
                "\"{path}\" would be {n} bytes; an agent writes files of at most \
                 {MAX_FILE_BYTES} bytes."
            )?,
            Self::Nul { arg } => write!(
                f,
                "{arg} holds a NUL character; drive_write writes text only."
            )?,
            Self::Noop { what } => write!(f, "This changes nothing ({what}).")?,
            // It says so already: "was not asked".
            Self::Trust { why } => {
                return write!(
                    f,
                    "drive_write was not asked: this session's trust could not be read ({why})."
                )
            }
        }
        write!(f, " {NOTHING_ASKED}")
    }
}

impl std::error::Error for Refusal {}

impl Request {
    /// Read a call's arguments, or say why they do not read.
    ///
    /// A `null` argument is an absent one: models send them for parameters
    /// they do not use. Per-op arguments are checked here rather than by a
    /// `oneOf` in the schema, which providers handle unevenly: an op without
    /// one it needs, or with one it does not take, is refused. Then the caps
    /// and the NUL rule over the arguments alone, and the two requests that
    /// could change nothing (`new_text` = `old_text`, `to` = `path`).
    pub fn parse(args: &Value) -> Result<Request, Refusal> {
        let Some(object) = args.as_object() else {
            return Err(Refusal::args("they are not one JSON object"));
        };
        let given = |key: &str| object.get(key).filter(|value| !value.is_null());
        let op = given("op")
            .and_then(Value::as_str)
            .and_then(Op::from_word)
            .ok_or_else(|| {
                Refusal::args("op is one of create, replace, edit, append, move, delete")
            })?;
        let word = op.as_str();
        // What the op does not take first: a stray argument says the model
        // meant another op, which is the likelier mistake to name.
        if let Some((key, _)) = object.iter().find(|(key, value)| {
            !value.is_null()
                && !matches!(key.as_str(), "drive" | "op" | "path")
                && !op.takes().contains(&key.as_str())
        }) {
            return Err(Refusal::args(format!("{word} takes no {}", shown_key(key))));
        }
        let string = |key: &str| -> Result<String, Refusal> {
            given(key)
                .and_then(Value::as_str)
                .map(str::to_owned)
                .ok_or_else(|| Refusal::args(format!("{word} needs {key}")))
        };
        // An empty one is none: the drive, the path, the new path, an edit's
        // passage and an append's text.
        let named = |key: &str| -> Result<String, Refusal> {
            string(key).and_then(|value| {
                if value.is_empty() {
                    Err(Refusal::args(format!("{word} needs {key}")))
                } else {
                    Ok(value)
                }
            })
        };
        let drive = named("drive")?;
        let path = named("path")?;
        let change = match op {
            Op::Create => Change::Create {
                content: string("content")?,
            },
            Op::Replace => Change::Replace {
                content: string("content")?,
            },
            Op::Edit => Change::Edit {
                old_text: named("old_text")?,
                new_text: string("new_text")?,
            },
            Op::Append => Change::Append {
                text: named("text")?,
            },
            Op::Move => Change::Move { to: named("to")? },
            Op::Delete => Change::Delete,
        };
        let expected = match op {
            Op::Create => None,
            _ => {
                let expected = string("expected")?;
                if !is_blob_id(&expected) {
                    return Err(Refusal::args(
                        "expected is the 40-character version drive_read printed",
                    ));
                }
                Some(expected)
            }
        };
        let request = Request {
            drive,
            path,
            expected,
            change,
        };
        request.check_arguments()?;
        Ok(request)
    }

    /// The caps, the NUL rule and the no-op rule over the arguments alone.
    fn check_arguments(&self) -> Result<(), Refusal> {
        let mut paths = vec![("path", self.path.as_str())];
        if let Change::Move { to } = &self.change {
            paths.push(("to", to.as_str()));
        }
        for (arg, path) in paths {
            if path.len() > MAX_PATH_BYTES {
                return Err(Refusal::args(format!(
                    "{arg} is {} bytes; a path is at most {MAX_PATH_BYTES} bytes",
                    path.len()
                )));
            }
            if path.contains('\0') {
                return Err(Refusal::Nul { arg });
            }
        }
        for (arg, text) in self.texts() {
            if text.len() > MAX_TEXT_BYTES {
                return Err(Refusal::ArgSize { arg, n: text.len() });
            }
            if text.contains('\0') {
                return Err(Refusal::Nul { arg });
            }
        }
        match &self.change {
            Change::Edit { old_text, new_text } if old_text == new_text => Err(Refusal::Noop {
                what: "new_text is old_text",
            }),
            Change::Move { to } if *to == self.path => Err(Refusal::Noop { what: "to is path" }),
            _ => Ok(()),
        }
    }

    /// The text arguments this request carries, named.
    fn texts(&self) -> Vec<(&'static str, &str)> {
        match &self.change {
            Change::Create { content } | Change::Replace { content } => {
                vec![("content", content.as_str())]
            }
            Change::Edit { old_text, new_text } => vec![
                ("old_text", old_text.as_str()),
                ("new_text", new_text.as_str()),
            ],
            Change::Append { text } => vec![("text", text.as_str())],
            Change::Move { .. } | Change::Delete => Vec::new(),
        }
    }

    pub fn op(&self) -> Op {
        match self.change {
            Change::Create { .. } => Op::Create,
            Change::Replace { .. } => Op::Replace,
            Change::Edit { .. } => Op::Edit,
            Change::Append { .. } => Op::Append,
            Change::Move { .. } => Op::Move,
            Change::Delete => Op::Delete,
        }
    }

    /// The new path of a `move`.
    pub fn to(&self) -> Option<&str> {
        match &self.change {
            Change::Move { to } => Some(to),
            _ => None,
        }
    }

    /// The approval's line (contract § 4.4): keeper's template over the
    /// parsed arguments, never words the model wrote.
    pub fn summary(&self) -> String {
        let (path, drive) = (&self.path, &self.drive);
        match &self.change {
            Change::Create { content } => {
                format!("Create `{path}` in {drive} ({} bytes)", content.len())
            }
            Change::Replace { content } => format!(
                "Replace the text of `{path}` in {drive} ({} bytes)",
                content.len()
            ),
            Change::Edit { old_text, new_text } => format!(
                "Edit `{path}` in {drive}: {} bytes become {} bytes",
                old_text.len(),
                new_text.len()
            ),
            Change::Append { text } => {
                format!("Append {} bytes to `{path}` in {drive}", text.len())
            }
            Change::Move { to } => format!("Move `{path}` to `{to}` in {drive}"),
            Change::Delete => format!("Delete `{path}` in {drive}"),
        }
    }

    /// Whether the file the agent read is the file here: `now` is the git
    /// blob id of the file's whole bytes here (`keeper_sync::blob_id`), or
    /// `None` when nothing is at `path`. A `create` expects nothing there.
    pub fn guard(&self, now: Option<&str>) -> Result<(), Refusal> {
        match (&self.expected, now) {
            (None, None) => Ok(()),
            (None, Some(_)) => Err(Refusal::Exists {
                path: self.path.clone(),
            }),
            (Some(_), None) => Err(Refusal::Absent {
                path: self.path.clone(),
            }),
            (Some(expected), Some(now)) if expected == now => Ok(()),
            (Some(expected), Some(now)) => Err(Refusal::Stale {
                path: self.path.clone(),
                expected: expected.clone(),
                now: now.to_owned(),
            }),
        }
    }

    /// The file's text after the change, from `current` — its bytes now,
    /// `None` when nothing is at `path` — or `Ok(None)` for a `move` or a
    /// `delete`, which write no text; or why the change cannot be made.
    ///
    /// `replace`, `edit` and `append` need the current file to be text
    /// (UTF-8, no NUL); `move` and `delete` take any file. An `edit`'s
    /// `old_text` must start at exactly one byte offset of the file,
    /// overlapping occurrences counted. Bytes are kept exactly: no newline
    /// is added or normalised. The result is at most [`MAX_FILE_BYTES`].
    pub fn post_image(&self, current: Option<&[u8]>) -> Result<Option<String>, Refusal> {
        let path = || self.path.clone();
        let text = |bytes: &[u8]| -> Result<String, Refusal> {
            match std::str::from_utf8(bytes) {
                Ok(text) if !text.contains('\0') => Ok(text.to_owned()),
                _ => Err(Refusal::Binary { path: path() }),
            }
        };
        let existing = || current.ok_or_else(|| Refusal::Absent { path: path() });
        let post = match &self.change {
            Change::Create { content } => {
                if current.is_some() {
                    return Err(Refusal::Exists { path: path() });
                }
                content.clone()
            }
            Change::Replace { content } => {
                if text(existing()?)? == *content {
                    return Err(Refusal::Noop {
                        what: "the file already holds this text",
                    });
                }
                content.clone()
            }
            Change::Edit { old_text, new_text } => {
                let before = text(existing()?)?;
                let at = match occurrences(before.as_bytes(), old_text.as_bytes()) {
                    (0, _) => return Err(Refusal::EditNone { path: path() }),
                    (1, at) => at,
                    (n, _) => return Err(Refusal::EditMany { path: path(), n }),
                };
                let mut after = String::with_capacity(before.len() + new_text.len());
                after.push_str(&before[..at]);
                after.push_str(new_text);
                after.push_str(&before[at + old_text.len()..]);
                after
            }
            Change::Append { text: tail } => {
                let mut after = text(existing()?)?;
                after.push_str(tail);
                after
            }
            Change::Move { .. } | Change::Delete => {
                existing()?;
                return Ok(None);
            }
        };
        if post.len() > MAX_FILE_BYTES {
            return Err(Refusal::FileSize {
                path: path(),
                n: post.len(),
            });
        }
        Ok(Some(post))
    }
}

/// An argument's name as a refusal shows it: at most 40 characters, so a
/// model that sent a paragraph as a key is not quoted back at length.
fn shown_key(key: &str) -> String {
    let mut chars = key.chars();
    let head: String = chars.by_ref().take(40).collect();
    if chars.next().is_some() {
        format!("{head}…")
    } else {
        head
    }
}

/// Whether `text` is a git blob id as keeper writes one: 40 lowercase hex.
fn is_blob_id(text: &str) -> bool {
    text.len() == 40 && text.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
}

/// How many byte offsets of `haystack` `needle` starts at, overlapping ones
/// counted, and the first of them (0 when none). Knuth–Morris–Pratt: linear
/// in both, so a file of one repeated character asked for a long run of it
/// costs no more than reading it. A non-empty UTF-8 needle only ever matches
/// at a character boundary of UTF-8 text.
fn occurrences(haystack: &[u8], needle: &[u8]) -> (usize, usize) {
    if needle.is_empty() || needle.len() > haystack.len() {
        return (0, 0);
    }
    let mut fail = vec![0usize; needle.len()];
    let mut k = 0;
    for i in 1..needle.len() {
        while k > 0 && needle[i] != needle[k] {
            k = fail[k - 1];
        }
        if needle[i] == needle[k] {
            k += 1;
        }
        fail[i] = k;
    }
    let (mut count, mut first) = (0, 0);
    let mut k = 0;
    for (i, &byte) in haystack.iter().enumerate() {
        while k > 0 && byte != needle[k] {
            k = fail[k - 1];
        }
        if byte == needle[k] {
            k += 1;
        }
        if k == needle.len() {
            if count == 0 {
                first = i + 1 - needle.len();
            }
            count += 1;
            k = fail[k - 1];
        }
    }
    (count, first)
}

/// The tool as the model is offered it (contract § 3.2), over the turn's
/// ticked `drives`.
pub fn spec(drives: &[String]) -> crate::bots::chat::ToolSpec {
    let mut drives: Vec<&str> = drives.iter().map(String::as_str).collect();
    drives.sort_unstable();
    drives.dedup();
    crate::bots::chat::ToolSpec {
        name: TOOL.to_owned(),
        description: format!(
            "Change one file of a drive a person ticked for this conversation: create a text \
             file, replace its whole text, edit one exact passage, append to its end, move or \
             rename it, or delete it. Nothing changes until a person approves it: the call \
             waits for their decision, and their own keeper writes it into the drive as them, \
             as one commit. Read the file with drive_read first and pass its `expected`, the \
             version you read; if the file changed since, nothing is written and you are told \
             so. Text files only: at most {MAX_TEXT_BYTES} bytes of new text per call, and at \
             most {MAX_FILE_BYTES} bytes in the file after the change. Never a folder, \
             anything under .git or .keeper, git's own files (.gitattributes, .gitignore, \
             .gitmodules, .lfsconfig), a file the drive ignores, or one kept in its large-file \
             store. A folder is made by creating or moving a file into it. The person sees \
             exactly what you send."
        ),
        parameters: json!({
            "type": "object",
            "properties": {
                "drive": {"type": "string", "enum": drives, "description": "Which drive: one a person ticked for this conversation."},
                "op": {"type": "string", "enum": ["create", "replace", "edit", "append", "move", "delete"], "description": "create: a new file at path (nothing may be there). replace: path's whole text becomes content. edit: the one place old_text appears becomes new_text. append: text is added at the end of path. move: path moves to to. delete: path is deleted."},
                "path": {"type": "string", "description": "The file, relative to the drive's root and `/`-joined. No `..`, no leading `/`."},
                "expected": {"type": "string", "description": "The file's version as drive_read printed it after `expected`. Required for every op except create."},
                "content": {"type": "string", "description": "create, replace: the file's whole new text."},
                "old_text": {"type": "string", "description": "edit: the passage to replace; it must appear exactly once in the file."},
                "new_text": {"type": "string", "description": "edit: what replaces it; may be empty."},
                "text": {"type": "string", "description": "append: what is added at the end, exactly as given (start it with a newline if you want one)."},
                "to": {"type": "string", "description": "move: the new path, relative to the drive's root; nothing may be there."}
            },
            "required": ["drive", "op", "path"],
            "additionalProperties": false
        }),
    }
}

/// The `binding_digest` of a `drive_write` approval (contract § 4.3): the
/// approval's id, session and agent, the call's `args` as `tool_calls.args`
/// holds them, no `exec_binding`, the call's [`row_checkpoint`] and the
/// default preconditions. What the runtime parks with and what a person's
/// device recomputes before it offers Allow.
pub fn digest(
    approval_id: &str,
    session: &str,
    agent: &str,
    args: &Value,
    turn: &str,
    call: &str,
) -> Result<String, FloatAt> {
    // As `ApprovalRecord::new` serialises them: no float, no map key, so
    // this cannot fail.
    let preconditions = serde_json::to_value(Preconditions::default()).unwrap_or(Value::Null);
    binding_digest(
        approval_id,
        session,
        agent,
        AgentTool::DriveWrite.as_wire(),
        args,
        &Value::Null,
        &row_checkpoint(turn, call).sha256,
        &preconditions,
    )
}

/// One line of a preview.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LineKind {
    Context,
    Add,
    Remove,
}

/// A line as a card shows it: its text without the line break.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PreviewLine {
    pub kind: LineKind,
    pub text: String,
}

/// One run of changes with up to [`PREVIEW_CONTEXT`] lines of context on
/// each side. `old_start` and `new_start` are the 1-based numbers of the
/// hunk's first line on each side; a side the hunk shows no line of is the
/// number of the line it comes after (0 at the start of the file).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Hunk {
    pub old_start: usize,
    pub new_start: usize,
    pub lines: Vec<PreviewLine>,
}

/// A text's first [`PREVIEW_EXCERPT_LINES`] lines and how many more it has.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Excerpt {
    pub lines: Vec<String>,
    pub more: usize,
}

/// What a person is shown of a change: hunks of a line diff; or, when a
/// side is over [`PREVIEW_MAX_LINES`] lines or the sides differ in more than
/// [`PREVIEW_MAX_EDITS`] lines, the new text's beginning (`whole`); or, for
/// a deletion, the deleted text's beginning (`deleted`). Empty for a move,
/// which changes no text.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Preview {
    pub hunks: Vec<Hunk>,
    pub whole: Option<Excerpt>,
    pub deleted: Option<Excerpt>,
}

/// The preview of a change from `before` (the file here, `None` when there
/// is none) to `after` (the post-image; `None` when the file goes away, or
/// for a move, whose text does not change).
pub fn preview(before: Option<&str>, after: Option<&str>) -> Preview {
    match (before, after) {
        (None, None) => Preview::default(),
        (Some(before), None) => Preview {
            deleted: Some(excerpt(&lines_of(before))),
            ..Preview::default()
        },
        (before, Some(after)) => {
            let old = lines_of(before.unwrap_or(""));
            let new = lines_of(after);
            let whole = || Preview {
                whole: Some(excerpt(&new)),
                ..Preview::default()
            };
            if old.len() > PREVIEW_MAX_LINES || new.len() > PREVIEW_MAX_LINES {
                return whole();
            }
            match edits(&old, &new, PREVIEW_MAX_EDITS) {
                Some(script) => Preview {
                    hunks: hunks(&old, &new, &script),
                    ..Preview::default()
                },
                None => whole(),
            }
        }
    }
}

/// `text`'s lines, each with its line break, so a last line without one
/// differs from the same line with one.
fn lines_of(text: &str) -> Vec<&str> {
    text.split_inclusive('\n').collect()
}

/// A line without its line break.
fn shown(line: &str) -> String {
    let line = line.strip_suffix('\n').unwrap_or(line);
    line.strip_suffix('\r').unwrap_or(line).to_owned()
}

fn excerpt(lines: &[&str]) -> Excerpt {
    Excerpt {
        lines: lines
            .iter()
            .take(PREVIEW_EXCERPT_LINES)
            .map(|line| shown(line))
            .collect(),
        more: lines.len().saturating_sub(PREVIEW_EXCERPT_LINES),
    }
}

/// One step of an edit script over two line lists.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Step {
    Keep,
    Remove,
    Add,
}

/// A shortest edit script from `old` to `new` (Myers, after the common
/// prefix and suffix), or `None` when it needs more than `max` removals and
/// additions.
fn edits(old: &[&str], new: &[&str], max: usize) -> Option<Vec<Step>> {
    let prefix = old.iter().zip(new).take_while(|(a, b)| a == b).count();
    let suffix = old[prefix..]
        .iter()
        .rev()
        .zip(new[prefix..].iter().rev())
        .take_while(|(a, b)| a == b)
        .count();
    let middle = myers(
        &old[prefix..old.len() - suffix],
        &new[prefix..new.len() - suffix],
        max,
    )?;
    let mut script = vec![Step::Keep; prefix];
    script.extend(middle);
    script.extend(std::iter::repeat_n(Step::Keep, suffix));
    Some(script)
}

/// Myers' greedy shortest edit script, keeping each round's furthest
/// reaches (only the band a round can touch) to walk back through.
fn myers(a: &[&str], b: &[&str], max: usize) -> Option<Vec<Step>> {
    let (n, m) = (a.len() as isize, b.len() as isize);
    let max = max.min(a.len() + b.len()) as isize;
    // `v[k + offset]` is the furthest x reached on diagonal k = x - y.
    let offset = max + 1;
    let mut v = vec![0isize; (2 * max + 3) as usize];
    // `trace[d]`: v over k in -(d+1)..=d+1 as it stood before round d.
    let mut trace: Vec<Vec<isize>> = Vec::new();
    let mut done = false;
    'rounds: for d in 0..=max {
        trace.push(v[(offset - d - 1) as usize..=(offset + d + 1) as usize].to_vec());
        for k in (-d..=d).step_by(2) {
            let at = |k: isize| v[(k + offset) as usize];
            let mut x = if k == -d || (k != d && at(k - 1) < at(k + 1)) {
                at(k + 1)
            } else {
                at(k - 1) + 1
            };
            let mut y = x - k;
            while x < n && y < m && a[x as usize] == b[y as usize] {
                x += 1;
                y += 1;
            }
            v[(k + offset) as usize] = x;
            if x >= n && y >= m {
                done = true;
                break 'rounds;
            }
        }
    }
    if !done {
        return None;
    }
    let mut steps = Vec::new();
    let (mut x, mut y) = (n, m);
    for (d, band) in trace.iter().enumerate().rev() {
        let d = d as isize;
        let at = |k: isize| band[(k + d + 1) as usize];
        let k = x - y;
        let prev_k = if k == -d || (k != d && at(k - 1) < at(k + 1)) {
            k + 1
        } else {
            k - 1
        };
        let prev_x = at(prev_k);
        let prev_y = prev_x - prev_k;
        while x > prev_x && y > prev_y {
            steps.push(Step::Keep);
            x -= 1;
            y -= 1;
        }
        if d > 0 {
            steps.push(if x == prev_x { Step::Add } else { Step::Remove });
        }
        x = prev_x;
        y = prev_y;
    }
    steps.reverse();
    Some(steps)
}

/// The script's changes grouped into hunks with [`PREVIEW_CONTEXT`] lines
/// of context; two changes with at most twice that many unchanged lines
/// between them share one.
fn hunks(old: &[&str], new: &[&str], script: &[Step]) -> Vec<Hunk> {
    // Each step with the 0-based line it stands at on each side.
    let mut placed = Vec::with_capacity(script.len());
    let (mut i, mut j) = (0, 0);
    for &step in script {
        placed.push((step, i, j));
        match step {
            Step::Keep => {
                i += 1;
                j += 1;
            }
            Step::Remove => i += 1,
            Step::Add => j += 1,
        }
    }
    let changed: Vec<usize> = placed
        .iter()
        .enumerate()
        .filter(|(_, (step, _, _))| *step != Step::Keep)
        .map(|(at, _)| at)
        .collect();
    let mut out = Vec::new();
    let mut at = 0;
    while at < changed.len() {
        let start = changed[at].saturating_sub(PREVIEW_CONTEXT);
        let mut last = changed[at];
        while at + 1 < changed.len() && changed[at + 1] - last <= 2 * PREVIEW_CONTEXT + 1 {
            at += 1;
            last = changed[at];
        }
        let end = (last + PREVIEW_CONTEXT + 1).min(placed.len());
        let (_, old_at, new_at) = placed[start];
        let lines: Vec<PreviewLine> = placed[start..end]
            .iter()
            .map(|&(step, i, j)| match step {
                Step::Keep => PreviewLine {
                    kind: LineKind::Context,
                    text: shown(old[i]),
                },
                Step::Remove => PreviewLine {
                    kind: LineKind::Remove,
                    text: shown(old[i]),
                },
                Step::Add => PreviewLine {
                    kind: LineKind::Add,
                    text: shown(new[j]),
                },
            })
            .collect();
        let shows = |kind: LineKind| lines.iter().any(|line| line.kind != kind);
        out.push(Hunk {
            old_start: if shows(LineKind::Add) {
                old_at + 1
            } else {
                old_at
            },
            new_start: if shows(LineKind::Remove) {
                new_at + 1
            } else {
                new_at
            },
            lines,
        });
        at += 1;
    }
    out
}

#[cfg(test)]
#[path = "drive_write_tests.rs"]
mod tests;
