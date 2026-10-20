//! What a person sends into an agent's room besides words (story 101.2,
//! R417; story 102.4): a file, a photo, a recording or a video, as an
//! `m.file`, `m.image`, `m.audio` or `m.video`, with a caption or without
//! one.
//!
//! The message is a turn whose text is its caption ([`caption_of`]). The
//! host downloads the file — at most [`MAX_BYTES`], counted as the body
//! arrives, within [`DOWNLOAD_WITHIN`], dropped when the turn stops — and
//! stores it, as it was sent, in the session's `attachments/` folder beside
//! the log, where the drive's own LFS rules route a large one when it
//! syncs; the turn's `user` line names it, so the model reads "Attached
//! files" and opens it with `drive_read`. A PDF's text is written beside it
//! as `<name>.txt`, read by a worker process with bounds ([`Worker`]); a
//! photo the model can see goes to it as an image part, made smaller to
//! fit what its model is shown when it is larger — by a worker too
//! ([`smaller`]). A
//! recording or a video is stored and named; no agent listens to or
//! watches one yet ([`not_heard`]).
//! Whatever could not be done is a sentence ([`Intake::notes`]) the room
//! and the model both read, and a WARN line in the log (R381).

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use keeper_core::bots::chat::{CancelSignal, ContentPart};
use keeper_core::bots::deliverable;
use keeper_core::bots::tools::FILE_CONTENT_IS_DATA;
use serde_json::Value;

/// The most bytes one message's file may have for an agent to get it: the
/// file is refused before it is downloaded when the message says it is
/// larger, and at the first byte past it as it arrives (R444/01).
pub const MAX_BYTES: u64 = 25 * 1024 * 1024;

/// The most bytes of a PDF's text written beside it: the rest is cut off,
/// and the file says where.
pub const MAX_PDF_TEXT_BYTES: usize = 4 * 1024 * 1024;

/// How long a file's whole download may take — [`MAX_BYTES`] at about
/// 90 kB/s — before the turn goes on without it (R444/02).
pub const DOWNLOAD_WITHIN: Duration = Duration::from_secs(300);

/// How long a PDF's reader may run, by the clock, before it is killed and
/// the turn goes on without the text (R444/03).
pub const PDF_READ: Duration = Duration::from_secs(60);

/// How much memory a PDF's reader may map beyond what it maps when it
/// starts — enforced on Linux only (R444/03).
pub const PDF_MEMORY: u64 = 1024 * 1024 * 1024;

/// How many seconds of CPU a PDF's reader may use (R444/03).
pub const PDF_CPU_SECONDS: u64 = 60;

/// How long a photo's copier may run, by the clock, before it is killed
/// and the photo is not shown (story 102.4 #4, R489).
pub const IMAGE_COPY_WITHIN: Duration = Duration::from_secs(60);

/// How many seconds of CPU a photo's copier may use.
pub const IMAGE_COPY_CPU_SECONDS: u64 = 60;

/// How much memory a photo's copier may map beyond what it maps when it
/// starts — enforced on Linux only: its working memory
/// ([`SHRINK_WORKING_MEMORY`]), the photo it reads, and room for the
/// codecs' own rows and tables. A Mac has no address-space limit: there
/// the working memory [`smaller`] counts from the photo's header before it
/// decodes is the copier's only memory bound (R512).
pub const IMAGE_COPY_MEMORY: u64 = SHRINK_WORKING_MEMORY + MAX_BYTES + 128 * 1024 * 1024;

/// The bounds a host takes a person's file in under (R444): how long its
/// download may take, the PDF reader, and the photo copier — `None` where
/// this host has none, and then a PDF's text is not read and a photo
/// larger than its model is shown is not made smaller.
#[derive(Clone)]
pub struct Bounds {
    pub download_within: Duration,
    pub pdf: Option<Arc<Worker>>,
    pub images: Option<Arc<Worker>>,
}

impl Bounds {
    /// This host's: [`DOWNLOAD_WITHIN`], and its own binary as the reader
    /// and the copier ([`Worker::pdf_reader`], [`Worker::image_copier`]).
    pub fn this_host() -> Bounds {
        Bounds {
            download_within: DOWNLOAD_WITHIN,
            pdf: Worker::pdf_reader().map(Arc::new),
            images: Worker::image_copier().map(Arc::new),
        }
    }
}

/// The message types that carry a person's file. A recording and a video
/// are taken too: stored and named, never dropped unsaid (story 102.4 #1).
const FILE_MSGTYPES: [&str; 4] = ["m.file", "m.image", "m.audio", "m.video"];

/// The message types whose file no agent listens to or watches yet.
const UNHEARD_MSGTYPES: [&str; 2] = ["m.audio", "m.video"];

/// Whether `content` is a message carrying a file: an `m.file`, `m.image`,
/// `m.audio` or `m.video`.
pub fn is_attachment(content: &Value) -> bool {
    content["msgtype"]
        .as_str()
        .is_some_and(|msgtype| FILE_MSGTYPES.contains(&msgtype))
}

/// The caption of a file message, as Matrix 1.10 puts it: `body` when the
/// message also names a `filename` that `body` is not; otherwise `body` is
/// only the file's name and there is no caption.
pub fn caption_of(content: &Value) -> String {
    let body = content["body"].as_str().unwrap_or_default();
    match content["filename"].as_str() {
        Some(filename) if filename != body => body.to_owned(),
        _ => String::new(),
    }
}

/// One file a message carries, as the message names it.
#[derive(Debug, Clone, PartialEq)]
pub struct Sent {
    /// The plain name it is stored under ([`file_name`]).
    pub name: String,
    /// Its media type, lowercase: the message's, else (none, or only
    /// `application/octet-stream`) read off its name.
    pub mime: String,
    /// The size the message says it has.
    pub declared: Option<u64>,
    /// Where its bytes are: the encrypted file's object, or an `mxc://`
    /// URI string in a room without encryption; `None` when the message
    /// names neither — a file message no file can be fetched for, which is
    /// said, never taken for a message without one (R444/06).
    pub source: Option<Value>,
}

/// The file an `m.file` or `m.image` `content` carries, as it names it.
pub fn sent_of(content: &Value) -> Sent {
    let source = if content["file"].is_object() {
        Some(content["file"].clone())
    } else {
        content["url"]
            .as_str()
            .map(|url| Value::String(url.to_owned()))
    };
    let named = content["filename"]
        .as_str()
        .or(content["body"].as_str())
        .unwrap_or_default();
    let mime = content["info"]["mimetype"]
        .as_str()
        .map(str::to_ascii_lowercase)
        .filter(|mime| !mime.is_empty() && mime != "application/octet-stream")
        .unwrap_or_else(|| mime_of_name(named).to_owned());
    Sent {
        name: file_name(named, &mime),
        declared: content["info"]["size"].as_u64(),
        mime,
        source,
    }
}

/// How long a stored name may be, in bytes, its extension included.
const MAX_NAME_BYTES: usize = 120;

/// `named` — what a person's client called the file — as a plain file name
/// of the session: its last path segment, without control characters or
/// the characters a Mac or Windows volume refuses, without leading dots or
/// trailing dots and spaces, at most [`MAX_NAME_BYTES`] long with its
/// extension kept; `attachment.<ext>` when nothing is left.
pub fn file_name(named: &str, mime: &str) -> String {
    let last = named.rsplit(['/', '\\']).next().unwrap_or_default();
    let cleaned: String = last
        .chars()
        .filter(|c| !c.is_control())
        .map(|c| {
            if matches!(c, ':' | '*' | '?' | '"' | '<' | '>' | '|') {
                '-'
            } else {
                c
            }
        })
        .collect();
    let trimmed = cleaned
        .trim()
        .trim_start_matches('.')
        .trim_end_matches(['.', ' '])
        .trim();
    if trimmed.is_empty() {
        return format!("attachment.{}", extension_of_mime(mime));
    }
    if trimmed.len() <= MAX_NAME_BYTES {
        return trimmed.to_owned();
    }
    let (stem, extension) = match trimmed.rsplit_once('.') {
        Some((stem, extension)) if !stem.is_empty() && extension.len() <= 16 => {
            (stem, Some(extension))
        }
        _ => (trimmed, None),
    };
    let room = MAX_NAME_BYTES - extension.map_or(0, |extension| extension.len() + 1);
    let mut end = room.min(stem.len());
    while !stem.is_char_boundary(end) {
        end -= 1;
    }
    match extension {
        Some(extension) => format!("{}.{extension}", stem[..end].trim_end()),
        None => stem[..end].trim_end().to_owned(),
    }
}

/// What keeper does with a file of a kind.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Kind {
    /// A picture a model may be shown: PNG, JPEG, WEBP or GIF.
    Image,
    /// A photo in a format no model is shown — HEIC, HEIF, TIFF… — named
    /// by it (`HEIC`): stored, and said not to be shown (R444/05).
    Unshown(String),
    /// A PDF: its text is written beside it.
    Pdf,
    /// Text `drive_read` reads as it is: plain text, Markdown, CSV, JSON….
    Text,
    /// A recording or a video: stored and named, and the agent told it
    /// cannot listen to or watch it yet (story 102.4 #1, DW-1454).
    Unheard,
    /// Anything else: stored, named, not read.
    Other,
}

/// The kind of the file `content` carries, as [`sent_of`] read it: an
/// `m.audio` or `m.video` is [`Kind::Unheard`] whatever type it names,
/// anything else by its type and name ([`kind_of`]).
pub fn kind_of_message(content: &Value, sent: &Sent) -> Kind {
    let msgtype = content["msgtype"].as_str().unwrap_or_default();
    if UNHEARD_MSGTYPES.contains(&msgtype) {
        Kind::Unheard
    } else {
        kind_of(&sent.mime, &sent.name)
    }
}

/// The kind of a file of media type `mime` named `name`.
pub fn kind_of(mime: &str, name: &str) -> Kind {
    let mime = mime.split(';').next().unwrap_or_default().trim();
    let named = mime_of_name(name);
    if deliverable::IMAGE_MIMES.contains(&mime) {
        Kind::Image
    } else if let Some(format) = mime.strip_prefix("image/") {
        Kind::Unshown(format.trim_start_matches("x-").to_ascii_uppercase())
    } else if mime == "application/pdf" || named == "application/pdf" {
        Kind::Pdf
    } else if mime.starts_with("text/") || TEXT_MIMES.contains(&mime) || named.starts_with("text/")
    {
        Kind::Text
    } else if ["audio/", "video/"]
        .iter()
        .any(|top| mime.starts_with(top) || named.starts_with(top))
    {
        Kind::Unheard
    } else {
        Kind::Other
    }
}

/// Media types other than `text/*` whose bytes are text.
const TEXT_MIMES: [&str; 5] = [
    "application/json",
    "application/csv",
    "application/x-yaml",
    "application/yaml",
    "application/xml",
];

/// The media type a file's name says, for a name the message gave no type
/// for: the kinds [`kind_of`] tells apart, else `application/octet-stream`.
fn mime_of_name(name: &str) -> &'static str {
    let extension = name
        .rsplit_once('.')
        .map(|(_, extension)| extension.to_ascii_lowercase())
        .unwrap_or_default();
    match extension.as_str() {
        "pdf" => "application/pdf",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "webp" => "image/webp",
        "gif" => "image/gif",
        "heic" => "image/heic",
        "heif" => "image/heif",
        "tif" | "tiff" => "image/tiff",
        "csv" => "text/csv",
        "tsv" => "text/tab-separated-values",
        "md" | "markdown" => "text/markdown",
        "txt" | "log" => "text/plain",
        "json" => "application/json",
        "yaml" | "yml" => "application/yaml",
        "xml" => "application/xml",
        "mp3" => "audio/mpeg",
        "m4a" => "audio/mp4",
        "wav" => "audio/wav",
        "ogg" | "oga" | "opus" => "audio/ogg",
        "mp4" | "m4v" => "video/mp4",
        "mov" => "video/quicktime",
        "webm" => "video/webm",
        _ => "application/octet-stream",
    }
}

/// The extension a file of `mime` with no usable name is stored with.
fn extension_of_mime(mime: &str) -> &'static str {
    match mime {
        "application/pdf" => "pdf",
        "image/png" => "png",
        "image/jpeg" => "jpg",
        "image/webp" => "webp",
        "image/gif" => "gif",
        "image/heic" => "heic",
        "image/heif" => "heif",
        "text/csv" => "csv",
        "text/markdown" => "md",
        "text/plain" => "txt",
        "application/json" => "json",
        _ => "bin",
    }
}

/// What reading a PDF's text found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PdfText {
    /// Its text, at most [`MAX_PDF_TEXT_BYTES`] of it.
    Text(String),
    /// It reads, and holds no text — a scan, most likely.
    NoText,
    /// It does not read as a PDF, or its reader stopped on it: why.
    Unreadable(String),
}

/// The text of the PDF `bytes`, by `pdf-extract`, in the process that
/// reads it — only ever the PDF reader [`Worker`], under its bounds. A panic
/// in the parser is caught and is an unreadable PDF.
fn pdf_text(bytes: &[u8]) -> PdfText {
    let read = std::panic::catch_unwind(|| pdf_extract::extract_text_from_mem(bytes));
    match read {
        Ok(Ok(text)) if text.chars().any(char::is_alphanumeric) => {
            PdfText::Text(bounded(text.trim().to_owned()))
        }
        Ok(Ok(_)) => PdfText::NoText,
        Ok(Err(error)) => PdfText::Unreadable(error.to_string()),
        Err(_) => PdfText::Unreadable("the PDF reader stopped on it".to_owned()),
    }
}

/// The argument a host's own binary is started with to read one PDF's
/// text and exit: `<binary> __keeper-pdf-text <out> <memory> <cpu>`
/// ([`served_as_worker`]). agentd and the desktop app both answer it
/// before anything else runs.
pub const PDF_READER_ARG: &str = "__keeper-pdf-text";

/// The argument a host's own binary is started with to make one photo's
/// smaller copy and exit:
/// `<binary> __keeper-image-copy <out> <memory> <cpu> <limit> <mime>`
/// ([`served_as_worker`], [`smaller`]). agentd and the desktop app both
/// answer it before anything else runs.
pub const IMAGE_COPIER_ARG: &str = "__keeper-image-copy";

/// The workers' exits. The PDF reader's: the text, written to `<out>`; no
/// text; unreadable, why written to `<out>`. The copier's: a copy that
/// fits, written to `<out>`; over its working memory; still too large
/// after every try; not decodable. Either's: refused — its bounds or its
/// input failed. None is 0: a binary that does not answer its argument and
/// exits 0 has done nothing — an unreadable PDF, a photo not shown — never
/// an empty answer.
const READ_TEXT: i32 = 40;
const READ_NO_TEXT: i32 = 41;
const READ_UNREADABLE: i32 = 42;
const REFUSED: i32 = 43;
const COPY_FITS: i32 = 50;
const COPY_OVER_BUDGET: i32 = 51;
const COPY_STILL_TOO_LARGE: i32 = 52;
const COPY_UNDECODABLE: i32 = 53;

/// The most bytes the PDF reader may write: the bounded text, its cut
/// note, and room to spare. `RLIMIT_FSIZE` holds it to this; the host
/// reads no more of it than this. The copier's is its limit.
const PDF_OUT_MAX: u64 = MAX_PDF_TEXT_BYTES as u64 + 4096;

/// A worker (R444/03; story 102.4 #4, R489): a child process — this
/// host's own binary started with [`PDF_READER_ARG`] or
/// [`IMAGE_COPIER_ARG`] — that gets a person's file on its stdin and
/// writes what it made of it to a file of its own, under bounds it sets on
/// itself before it reads a byte: on Linux `RLIMIT_AS` (what it maps at
/// start plus `memory`) and `RLIMIT_CPU`; on a Mac `RLIMIT_CPU` only —
/// macOS does not enforce an address-space limit, so there the clock and
/// the CPU bound it, and a photo's copier's memory only what [`smaller`]
/// counts before it decodes (R512); everywhere `RLIMIT_FSIZE` on its
/// output and no core file. Past `within` by the clock, or when the turn
/// stops, it is killed and reaped. A parser or a codec that runs away dies
/// alone, and one that cannot be interrupted is stopped all the same: the
/// host only reads a file.
#[derive(Debug, Clone)]
pub struct Worker {
    /// The binary, and the arguments before its role's.
    pub program: PathBuf,
    pub args: Vec<OsString>,
    /// How long it may run by the clock.
    pub within: Duration,
    /// How much it may map beyond what it maps at start (Linux).
    pub memory: u64,
    /// How many CPU seconds it may use.
    pub cpu_seconds: u64,
}

impl Worker {
    /// This host's own binary as the PDF reader, with [`PDF_READ`],
    /// [`PDF_MEMORY`] and [`PDF_CPU_SECONDS`]; `None` when the binary
    /// cannot be found.
    pub fn pdf_reader() -> Option<Worker> {
        Worker::this_binary(PDF_READ, PDF_MEMORY, PDF_CPU_SECONDS)
    }

    /// This host's own binary as the photo copier, with
    /// [`IMAGE_COPY_WITHIN`], [`IMAGE_COPY_MEMORY`] and
    /// [`IMAGE_COPY_CPU_SECONDS`]; `None` when the binary cannot be found.
    pub fn image_copier() -> Option<Worker> {
        Worker::this_binary(IMAGE_COPY_WITHIN, IMAGE_COPY_MEMORY, IMAGE_COPY_CPU_SECONDS)
    }

    fn this_binary(within: Duration, memory: u64, cpu_seconds: u64) -> Option<Worker> {
        let program = std::env::current_exe().ok()?;
        Some(Worker {
            program,
            args: Vec::new(),
            within,
            memory,
            cpu_seconds,
        })
    }

    /// The text of the PDF `bytes`, read by this worker; `None` when
    /// `stop` fired first.
    pub async fn read_pdf(&self, bytes: Vec<u8>, stop: &CancelSignal) -> Option<PdfText> {
        let out = match scratch() {
            Ok(out) => out,
            Err(error) => {
                return Some(PdfText::Unreadable(format!(
                    "no file for the PDF reader's text: {error}"
                )))
            }
        };
        let read = match self.run(PDF_READER_ARG, &out, &[], bytes, stop).await {
            Err(error) => Some(PdfText::Unreadable(format!(
                "the PDF reader did not start: {error}"
            ))),
            Ok(Ended::Stopped) => None,
            Ok(Ended::TimedOut) => Some(PdfText::Unreadable(format!(
                "its reader ran past {} s and was stopped",
                self.within.as_secs_f32()
            ))),
            Ok(Ended::Lost(why)) => Some(PdfText::Unreadable(why)),
            Ok(Ended::Exited(status)) => Some(match status.code() {
                Some(READ_TEXT) => match read_out(&out, PDF_OUT_MAX).map(String::from_utf8) {
                    Ok(Ok(text)) => PdfText::Text(text),
                    Ok(Err(_)) => PdfText::Unreadable("its text is not UTF-8".to_owned()),
                    Err(error) => PdfText::Unreadable(format!("its text did not read: {error}")),
                },
                Some(READ_NO_TEXT) => PdfText::NoText,
                Some(READ_UNREADABLE) => PdfText::Unreadable(
                    read_out(&out, PDF_OUT_MAX)
                        .ok()
                        .and_then(|why| String::from_utf8(why).ok())
                        .unwrap_or_else(|| "it did not read".to_owned()),
                ),
                _ => PdfText::Unreadable(format!("its reader stopped on it ({status})")),
            }),
        };
        let _ = std::fs::remove_file(&out);
        read
    }

    /// A copy of the photo `bytes` of type `mime` of at most `limit`
    /// bytes, made by this worker ([`smaller`]); `None` when `stop` fired
    /// first.
    pub async fn copy_image(
        &self,
        mime: &str,
        bytes: Vec<u8>,
        limit: usize,
        stop: &CancelSignal,
    ) -> Option<Copied> {
        let out = match scratch() {
            Ok(out) => out,
            Err(error) => return Some(Copied::Failed(format!("no file for the copy: {error}"))),
        };
        let role = [limit.to_string(), mime.to_owned()];
        let copied = match self.run(IMAGE_COPIER_ARG, &out, &role, bytes, stop).await {
            Err(error) => Some(Copied::Failed(format!("the copier did not start: {error}"))),
            Ok(Ended::Stopped) => None,
            Ok(Ended::TimedOut) => Some(Copied::Failed(format!(
                "the copier ran past {} s and was stopped",
                self.within.as_secs_f32()
            ))),
            Ok(Ended::Lost(why)) => Some(Copied::Failed(why)),
            Ok(Ended::Exited(status)) => Some(match status.code() {
                Some(COPY_FITS) => match read_out(&out, limit as u64) {
                    Ok(jpeg) if !jpeg.is_empty() => Copied::Fits(jpeg),
                    Ok(_) => Copied::Failed("the copier wrote no copy".to_owned()),
                    Err(error) => Copied::Failed(format!("its copy did not read: {error}")),
                },
                Some(COPY_OVER_BUDGET) => Copied::OverBudget,
                Some(COPY_STILL_TOO_LARGE) => Copied::StillTooLarge,
                Some(COPY_UNDECODABLE) => Copied::Undecodable,
                _ => Copied::Failed(format!("the copier stopped on it ({status})")),
            }),
        };
        let _ = std::fs::remove_file(&out);
        copied
    }

    /// Run this worker as `role` — writing to `out`, `extra` after its
    /// bounds — with `input` on its stdin, until it exits, runs past
    /// [`Self::within`] or `stop` fires; killed and reaped whenever it did
    /// not end by itself. `Err` when it did not start.
    async fn run(
        &self,
        role: &str,
        out: &Path,
        extra: &[String],
        input: Vec<u8>,
        stop: &CancelSignal,
    ) -> std::io::Result<Ended> {
        use tokio::io::AsyncWriteExt as _;
        let mut child = tokio::process::Command::new(&self.program)
            .args(&self.args)
            .arg(role)
            .arg(out)
            .arg(self.memory.to_string())
            .arg(self.cpu_seconds.to_string())
            .args(extra)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .kill_on_drop(true)
            .spawn()?;
        let stdin = child.stdin.take();
        let feed = async move {
            if let Some(mut stdin) = stdin {
                // A worker that died early closed its end: what it said
                // by its exit is the answer.
                let _ = stdin.write_all(&input).await;
            }
        };
        let mut stop = stop.clone();
        let ended = {
            let run = async { tokio::join!(feed, child.wait()).1 };
            tokio::select! {
                biased;
                () = stop.cancelled() => Ended::Stopped,
                ran = tokio::time::timeout(self.within, run) => match ran {
                    Ok(Ok(status)) => Ended::Exited(status),
                    Ok(Err(error)) => Ended::Lost(error.to_string()),
                    Err(_) => Ended::TimedOut,
                },
            }
        };
        if !matches!(ended, Ended::Exited(_)) {
            // Killed and waited for: no worker outlives its answer.
            let _ = child.kill().await;
        }
        Ok(ended)
    }
}

/// How a worker's run ended.
enum Ended {
    Exited(std::process::ExitStatus),
    TimedOut,
    Stopped,
    Lost(String),
}

/// A new, empty file only this user may read, for one worker's answer.
fn scratch() -> std::io::Result<PathBuf> {
    let path = std::env::temp_dir().join(format!(
        "keeper-worker-{}-{}.out",
        std::process::id(),
        ulid::Ulid::new()
    ));
    let mut open = std::fs::OpenOptions::new();
    open.write(true).create_new(true);
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::mode(&mut open, 0o600);
    open.open(&path)?;
    Ok(path)
}

/// At most `max` bytes of the worker's file.
fn read_out(out: &Path, max: u64) -> std::io::Result<Vec<u8>> {
    use std::io::Read as _;
    let mut bytes = Vec::new();
    std::fs::File::open(out)?
        .take(max)
        .read_to_end(&mut bytes)?;
    Ok(bytes)
}

/// Be a worker when `args` — this process's arguments after its program —
/// start with [`PDF_READER_ARG`] or [`IMAGE_COPIER_ARG`]: set the bounds,
/// read the file from stdin, write what was made of it to `<out>`, and
/// answer the exit code the process ends with. `None` for every other run.
/// A host calls this first, before it starts anything.
pub fn served_as_worker(args: impl IntoIterator<Item = OsString>) -> Option<i32> {
    let mut args = args.into_iter();
    let role = args.next()?;
    let reading = role == PDF_READER_ARG;
    if !reading && role != IMAGE_COPIER_ARG {
        return None;
    }
    let number = |arg: Option<OsString>| arg?.to_str()?.parse::<u64>().ok();
    let (Some(out), Some(memory), Some(cpu)) = (
        args.next().map(PathBuf::from),
        number(args.next()),
        number(args.next()),
    ) else {
        return Some(REFUSED);
    };
    if reading {
        return Some(read_as_worker(&out, memory, cpu));
    }
    let (Some(limit), Some(mime)) = (
        number(args.next()),
        args.next().and_then(|mime| mime.into_string().ok()),
    ) else {
        return Some(REFUSED);
    };
    Some(copy_as_worker(&out, memory, cpu, limit, &mime))
}

/// The person's file, from stdin: at most one byte more than [`MAX_BYTES`].
fn stdin_bytes() -> Option<Vec<u8>> {
    use std::io::Read as _;
    let mut bytes = Vec::new();
    std::io::stdin()
        .lock()
        .take(MAX_BYTES + 1)
        .read_to_end(&mut bytes)
        .ok()?;
    Some(bytes)
}

fn read_as_worker(out: &Path, memory: u64, cpu_seconds: u64) -> i32 {
    if bound_self(memory, cpu_seconds, PDF_OUT_MAX).is_err() {
        return REFUSED;
    }
    let Some(bytes) = stdin_bytes() else {
        return REFUSED;
    };
    let (code, said) = match pdf_text(&bytes) {
        PdfText::Text(text) => (READ_TEXT, text),
        PdfText::NoText => (READ_NO_TEXT, String::new()),
        PdfText::Unreadable(why) => (READ_UNREADABLE, why),
    };
    match std::fs::write(out, said) {
        Ok(()) => code,
        Err(_) => REFUSED,
    }
}

fn copy_as_worker(out: &Path, memory: u64, cpu_seconds: u64, limit: u64, mime: &str) -> i32 {
    let Ok(within) = usize::try_from(limit) else {
        return REFUSED;
    };
    if bound_self(memory, cpu_seconds, limit).is_err() {
        return REFUSED;
    }
    let Some(bytes) = stdin_bytes() else {
        return REFUSED;
    };
    match smaller(mime, &bytes, within) {
        Copied::Fits(jpeg) => match std::fs::write(out, jpeg) {
            Ok(()) => COPY_FITS,
            Err(_) => REFUSED,
        },
        Copied::OverBudget => COPY_OVER_BUDGET,
        Copied::StillTooLarge => COPY_STILL_TOO_LARGE,
        Copied::Undecodable => COPY_UNDECODABLE,
        Copied::Failed(_) => REFUSED,
    }
}

/// The worker's bounds on itself, set before it reads its input: at most
/// `out_max` bytes of output.
fn bound_self(memory: u64, cpu_seconds: u64, out_max: u64) -> std::io::Result<()> {
    #[cfg(unix)]
    {
        use rustix::process::{setrlimit, Resource, Rlimit};
        let cap = |value: u64| Rlimit {
            current: Some(value),
            maximum: Some(value),
        };
        setrlimit(Resource::Core, cap(0))?;
        setrlimit(Resource::Cpu, cap(cpu_seconds.max(1)))?;
        setrlimit(Resource::Fsize, cap(out_max))?;
        #[cfg(target_os = "linux")]
        setrlimit(Resource::As, cap(mapped_now()?.saturating_add(memory)))?;
    }
    #[cfg(not(target_os = "linux"))]
    let _ = memory;
    #[cfg(not(unix))]
    let _ = (cpu_seconds, out_max);
    Ok(())
}

/// How many bytes this process maps now: `/proc/self/status`'s `VmSize`.
#[cfg(target_os = "linux")]
fn mapped_now() -> std::io::Result<u64> {
    let status = std::fs::read_to_string("/proc/self/status")?;
    status
        .lines()
        .find_map(|line| line.strip_prefix("VmSize:"))
        .and_then(|size| {
            size.trim()
                .trim_end_matches("kB")
                .trim()
                .parse::<u64>()
                .ok()
        })
        .map(|kib| kib * 1024)
        .ok_or_else(|| std::io::Error::other("no VmSize in /proc/self/status"))
}

/// `text` cut to [`MAX_PDF_TEXT_BYTES`] at a character boundary, saying so
/// at its end when it was.
fn bounded(mut text: String) -> String {
    if text.len() <= MAX_PDF_TEXT_BYTES {
        return text;
    }
    let mut end = MAX_PDF_TEXT_BYTES;
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    text.truncate(end);
    text.push_str("\n\n[keeper cut the text here: the rest of the PDF is not in this file.]");
    text
}

/// The name a PDF's text is written under, beside it: `<name>.txt`.
pub fn text_name(stored: &str) -> String {
    format!("{stored}.txt")
}

/// A picture a person sent, as stored: its name, media type and bytes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Picture {
    pub name: String,
    pub mime: String,
    pub bytes: Vec<u8>,
}

/// What became of a message's file before its turn: the files the `user`
/// line names, the pictures the model may be shown with it, and a sentence
/// for each thing that could not be done.
#[derive(Debug, Default)]
pub struct Intake {
    /// Drive-relative paths in the home drive, in the order written.
    pub stored: Vec<String>,
    /// Each picture to show the model, if it can see ([`shown`]).
    pub pictures: Vec<Picture>,
    /// What the person and the model are told.
    pub notes: Vec<String>,
}

/// What the turn's user message gains for a model that answers `vision`
/// (`None`: it did not say), and the notes of the whole intake: each
/// picture as an image part unless the model said it cannot see (R417) —
/// a picture larger than `model` is shown
/// ([`deliverable::max_image_bytes`]) goes as a smaller JPEG copy made by
/// `copier` ([`Worker::copy_image`], [`smaller`]), and one keeper cannot
/// make small enough — or has no copier for — gets a note naming the limit
/// (story 102.4 #4, R489). A copy `stop` cut short is neither shown nor
/// said: the turn is over. The image parts are file content, so they come
/// after one text part of their own saying so ([`FILE_CONTENT_IS_DATA`],
/// the sentence every drive read carries — AD-159, NFR-48), apart from the
/// person's caption. When there are notes, one text part holds them under
/// [`NOTES_FOR_THE_MODEL`].
pub async fn shown(
    pictures: Vec<Picture>,
    mut notes: Vec<String>,
    vision: Option<bool>,
    model: &str,
    copier: Option<&Worker>,
    stop: &CancelSignal,
) -> (Vec<ContentPart>, Vec<String>) {
    let limit = deliverable::max_image_bytes(model);
    let mut images = Vec::with_capacity(pictures.len());
    for picture in pictures {
        if let deliverable::ImageOffer::Refuse(_) = deliverable::image_offer(vision, model) {
            notes.push(cannot_see(model, &picture.name));
            continue;
        }
        if picture.bytes.len() <= limit {
            images.push(deliverable::image_content_part(
                &picture.mime,
                &picture.bytes,
            ));
            continue;
        }
        let Picture { name, mime, bytes } = picture;
        let size = bytes.len() as u64;
        let copied = match copier {
            // No process is started for a format no copy is made of.
            Some(_) if copyable(&mime).is_none() => Copied::Undecodable,
            Some(copier) => match copier.copy_image(&mime, bytes, limit, stop).await {
                Some(copied) => copied,
                None => continue,
            },
            None => Copied::Failed("this host has no copier".to_owned()),
        };
        match copied {
            Copied::Fits(jpeg) => images.push(deliverable::image_content_part("image/jpeg", &jpeg)),
            why => {
                tracing::info!(
                    ?why,
                    "agents: no copy of a photo small enough for its model"
                );
                notes.push(too_large_to_show(&name, size, model, limit));
            }
        }
    }
    let mut parts = Vec::with_capacity(images.len() + 2);
    if !images.is_empty() {
        parts.push(ContentPart::Text(FILE_CONTENT_IS_DATA.to_owned()));
        parts.extend(images);
    }
    if !notes.is_empty() {
        let listed: Vec<String> = notes.iter().map(|note| format!("- {note}")).collect();
        parts.push(ContentPart::Text(format!(
            "{NOTES_FOR_THE_MODEL}\n{}",
            listed.join("\n")
        )));
    }
    (parts, notes)
}

/// How a byte count reads in a sentence: whole kB below a megabyte, one
/// decimal of MB above — rounded up, in `u128`, so the largest size a
/// message may claim reads too (R444/08).
fn human(bytes: u64) -> String {
    const MB: u128 = 1024 * 1024;
    let bytes = u128::from(bytes);
    if bytes >= MB {
        let tenths = (bytes * 10).div_ceil(MB);
        format!("{}.{} MB", tenths / 10, tenths % 10)
    } else {
        format!("{} kB", bytes.div_ceil(1024))
    }
}

/// A file over [`MAX_BYTES`].
pub fn too_large(name: &str, bytes: u64) -> String {
    format!(
        "{name} is {} and an agent takes at most {} from one message, so it did not get the file.",
        human(bytes),
        human(MAX_BYTES)
    )
}

/// A host with no way to download from the room.
pub fn no_download(name: &str) -> String {
    format!("This agent's host cannot download files from the room, so it did not get {name}.")
}

/// A download that failed.
pub fn not_downloaded(name: &str) -> String {
    format!("keeper could not download {name} from the room, so the agent did not get it. Send it again.")
}

/// A download that did not end within the host's bound.
pub fn download_timed_out(name: &str, within: Duration) -> String {
    format!(
        "keeper stopped downloading {name} after {} s, so the agent did not get it. Send it again, or send a smaller file.",
        within.as_secs()
    )
}

/// A file message that names no file keeper can fetch (R444/06).
pub fn no_source(name: &str) -> String {
    format!("{name} arrived without a file keeper can fetch, so the agent did not get it. Send it again.")
}

/// A photo in a format no model is shown (R444/05).
pub fn format_not_shown(name: &str, format: &str) -> String {
    format!("{name} was saved for the agent, but this photo's format ({format}) can't be shown to the model.")
}

/// A host that cannot read a PDF's text.
pub fn no_pdf_reader(name: &str) -> String {
    format!("This agent's host cannot read the text of PDFs, so the agent knows only that {name} was sent.")
}

/// A file that could not be stored in the session.
pub fn not_stored(name: &str, why: &str) -> String {
    format!("keeper could not save {name} in this conversation's folder, so the agent did not get it: {why}")
}

/// A PDF with no text.
pub fn no_text(name: &str) -> String {
    format!("{name} has no text keeper can read — it may be a scan — so the agent knows only that it was sent.")
}

/// A PDF whose text could not be read.
pub fn pdf_unreadable(name: &str) -> String {
    format!("keeper could not read the text of {name}, so the agent knows only that it was sent.")
}

/// A PDF's text that could not be stored beside it.
pub fn text_not_stored(name: &str, why: &str) -> String {
    format!(
        "keeper could not save the text of {name} beside it, so the agent cannot read it: {why}"
    )
}

/// A photo for a model that said it cannot see.
pub fn cannot_see(model: &str, name: &str) -> String {
    format!("{model} cannot see images, so {name} was saved for the agent but not shown to it.")
}

/// A photo larger than `model` is shown (`limit`), that keeper could not
/// make small enough.
pub fn too_large_to_show(name: &str, bytes: u64, model: &str, limit: usize) -> String {
    format!(
        "{name} is {}, more than the {} {model} is shown, and keeper could not make a small enough copy, so it was saved for the agent but not shown to it.",
        human(bytes),
        human(limit as u64)
    )
}

/// A recording or a video no agent listens to or watches yet, stored at
/// `path` (drive-relative) — said, never dropped (story 102.4 #1).
pub fn not_heard(name: &str, path: &str) -> String {
    format!("keeper does not let agents listen to or watch {name} yet; it was saved at {path}.")
}

/// How many smaller copies keeper tries before it says a photo is too
/// large to show.
const SHRINK_TRIES: usize = 6;

/// The most working memory making a photo's smaller copy may take — at its
/// peak, decoding it or copying it ([`working_memory`]) — counted from its
/// header before a pixel is decoded (R489, R512): a photo that needs more
/// is too large to show, never a host out of memory. A Mac has no
/// address-space limit, so there this count is the copier's only memory
/// bound.
pub const SHRINK_WORKING_MEMORY: u64 = 512 * 1024 * 1024;

/// What the PNG decoder may allocate for a photo's chunks — its text, its
/// colour profile inflated, its other metadata — and its output row: the
/// decoder counts them against this itself (`png::Limits`, which `image`
/// sets from `Limits::max_alloc`) and refuses a chunk past it (R512).
const PNG_METADATA: u64 = 32 * 1024 * 1024;

/// Rows the PNG decoder holds beside its output while it inflates and
/// unfilters (`png` 0.18): up to four behind the row it works on — or
/// 128 KiB — before its buffer shifts back, that row, the previous one and
/// the output row, each buffer at most doubled as it grows. Sixteen.
const PNG_ROWS: u64 = 16;

/// Lines of a JPEG component's padded width, of 2-byte coefficients, the
/// decoder holds for one row of MCUs beside the whole image (`zune-jpeg`
/// 0.5): its coefficients (up to 4 × 8 lines), their upsampled copy (up to
/// twice that), its row and the row above, and the first row upsampled
/// (up to 16 × 2 × 8) — 360 at the largest sampling, 4 × 4. Four hundred.
const JPEG_ROW_LINES: u64 = 400;

/// What a decoder holds whatever the photo's size: its tables, its
/// inflater's window, the floors of its buffers.
const CODEC_STATE: u64 = 1024 * 1024;

/// The JPEG quality of a smaller copy.
const SHRINK_QUALITY: u8 = 85;

/// What a photo's transparency is flattened onto, as JPEG has none: white,
/// as a viewer draws a transparent picture.
pub const BACKGROUND: [u8; 3] = [255, 255, 255];

/// What making a smaller copy of a photo came to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Copied {
    /// A JPEG copy within the limit.
    Fits(Vec<u8>),
    /// Its header says a copy would take more working memory than
    /// [`SHRINK_WORKING_MEMORY`], or a chunk of it more than
    /// [`PNG_METADATA`]: refused before a pixel is decoded.
    OverBudget,
    /// Every one of [`SHRINK_TRIES`] copies was still over the limit.
    StillTooLarge,
    /// Not a PNG or JPEG — the formats this build decodes — or bytes that
    /// do not decode as one.
    Undecodable,
    /// The copy could not be made — its copier did not start, ran past
    /// its clock, or ended without an answer: why.
    Failed(String),
}

/// What a photo's decoder allocates beside its decoded pixels, as its
/// header — read by that decoder — says.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Decoding {
    /// PNG: its chunks, within [`PNG_METADATA`], and [`PNG_ROWS`] rows.
    Png,
    /// JPEG, of `input` bytes: `components` components, sampled at most
    /// `h_max` × `v_max` — the largest sampling of any of them.
    Jpeg {
        input: u64,
        components: u64,
        h_max: u64,
        v_max: u64,
    },
}

/// The working memory, in bytes, of making a copy of at most `limit`
/// bytes of a `width` × `height` photo of `color` decoded as `decoding`
/// says: the larger of its two peaks (R512). Decoding holds the decoded
/// pixels and what the decoder allocates beside them, which is gone once
/// they are decoded:
///
/// - PNG: [`PNG_METADATA`] and [`PNG_ROWS`] rows;
/// - JPEG: the compressed bytes, copied in, and its marker segments copied
///   out of them (extended XMP twice); every component's coefficients, the
///   whole image of them — a progressive JPEG's always, a sequential one's
///   when its first scan does not hold every component — at most the image
///   padded to its MCUs each, as the decoder does not say how each
///   component is sampled; and [`JPEG_ROW_LINES`] lines of each.
///
/// Copying holds the decoded pixels, their flattened RGB copy (none when
/// they are RGB already), the largest scaled copy — never larger than the
/// flattened one — and the encoded copy kept. `None` when it does not fit
/// in a `u64`.
fn working_memory(
    width: u32,
    height: u32,
    color: image::ColorType,
    decoding: Decoding,
    limit: usize,
) -> Option<u64> {
    let (width, height) = (u64::from(width), u64::from(height));
    let pixels = width.checked_mul(height)?;
    let depth = u64::from(color.bytes_per_pixel());
    let decoded = pixels.checked_mul(depth)?;
    let rgb = pixels.checked_mul(3)?;
    let flattened = if color == image::ColorType::Rgb8 {
        0
    } else {
        rgb
    };
    let copying = decoded
        .checked_add(flattened)?
        .checked_add(rgb)?
        .checked_add(u64::try_from(limit).ok()?)?;
    let beside = match decoding {
        Decoding::Png => {
            let row = width.checked_mul(depth)?.checked_add(1)?;
            row.checked_mul(PNG_ROWS)?.checked_add(PNG_METADATA)?
        }
        Decoding::Jpeg {
            input,
            components,
            h_max,
            v_max,
        } => {
            let padded = |side: u64, sampling: u64| {
                let mcu = sampling.checked_mul(8)?;
                side.div_ceil(mcu).checked_mul(mcu)
            };
            let (wide, high) = (padded(width, h_max)?, padded(height, v_max)?);
            let coefficients = wide.checked_mul(high)?.checked_mul(2)?;
            let rows = wide.checked_mul(JPEG_ROW_LINES * 2)?;
            coefficients
                .checked_add(rows)?
                .checked_mul(components)?
                .checked_add(input.checked_mul(3)?)?
        }
    };
    let decoding = decoded.checked_add(beside)?.checked_add(CODEC_STATE)?;
    Some(decoding.max(copying))
}

/// The JPEG `bytes`' dimensions and what decoding them allocates, from
/// their header as the decoder `image` runs reads it — given the options
/// `image` gives it. `None` when that decoder reads no header there.
fn jpeg_decoding(bytes: &[u8]) -> Option<(u32, u32, Decoding)> {
    use zune_jpeg::zune_core::{bytestream::ZCursor, options::DecoderOptions};
    use zune_jpeg::SampleRatios;
    let options = DecoderOptions::default()
        .set_strict_mode(false)
        .set_max_width(usize::MAX)
        .set_max_height(usize::MAX);
    let mut decoder = zune_jpeg::JpegDecoder::new_with_options(ZCursor::new(bytes), options);
    decoder.decode_headers().ok()?;
    let info = decoder.info()?;
    let (h_max, v_max) = match info.sample_ratio {
        SampleRatios::None => (1, 1),
        SampleRatios::V => (1, 2),
        SampleRatios::H => (2, 1),
        SampleRatios::HV => (2, 2),
        SampleRatios::Generic(h, v) => (u64::try_from(h).ok()?, u64::try_from(v).ok()?),
    };
    let decoding = Decoding::Jpeg {
        input: u64::try_from(bytes.len()).ok()?,
        components: u64::from(info.components),
        h_max,
        v_max,
    };
    Some((u32::from(info.width), u32::from(info.height), decoding))
}

/// The format of a photo of type `mime` a smaller copy is made of: the
/// formats this build decodes, PNG and JPEG.
fn copyable(mime: &str) -> Option<image::ImageFormat> {
    match mime {
        "image/png" => Some(image::ImageFormat::Png),
        "image/jpeg" => Some(image::ImageFormat::Jpeg),
        _ => None,
    }
}

/// A JPEG copy of the photo `bytes` of type `mime` of at most `limit`
/// bytes: encoded at its size first, then scaled down until it fits. Its
/// header is read first, by the decoder that decodes it, and a photo whose
/// [`working_memory`] is over [`SHRINK_WORKING_MEMORY`] — or a PNG one of
/// whose chunks is over [`PNG_METADATA`] — is refused before a pixel is
/// decoded (R512); the first copy is made of the flattened pixels
/// themselves, a later one of one scaled copy at a time, and an encoded
/// copy over the limit keeps no more than the limit (R489). Transparency
/// is composited onto [`BACKGROUND`]. Run only in a [`Worker`], which the
/// host can stop: no codec here can be interrupted. The stored file is
/// the person's, untouched; only what the model is shown is smaller.
pub fn smaller(mime: &str, bytes: &[u8], limit: usize) -> Copied {
    use image::{ImageDecoder, ImageEncoder};
    let Some(format) = copyable(mime) else {
        return Copied::Undecodable;
    };
    let jpeg = if format == image::ImageFormat::Jpeg {
        let Some(header) = jpeg_decoding(bytes) else {
            return Copied::Undecodable;
        };
        Some(header)
    } else {
        None
    };
    let mut reader = image::ImageReader::with_format(std::io::Cursor::new(bytes), format);
    let mut limits = image::Limits::default();
    // What `image` hands the PNG decoder for its chunks; the JPEG decoder
    // has no such allowance, and its allocations are counted instead.
    limits.max_alloc = Some(PNG_METADATA);
    reader.limits(limits);
    // The header only: the size and the colours.
    let decoder = match reader.into_decoder() {
        Ok(decoder) => decoder,
        Err(image::ImageError::Limits(_)) => return Copied::OverBudget,
        Err(_) => return Copied::Undecodable,
    };
    let (width, height) = decoder.dimensions();
    let decoding = match jpeg {
        None => Decoding::Png,
        Some((w, h, decoding)) if (w, h) == (width, height) => decoding,
        // Not the header `image` decodes: what that one allocates is not
        // known.
        Some(_) => return Copied::OverBudget,
    };
    let needs = working_memory(width, height, decoder.color_type(), decoding, limit);
    if needs.is_none_or(|needs| needs > SHRINK_WORKING_MEMORY) {
        return Copied::OverBudget;
    }
    let Ok(decoded) = image::DynamicImage::from_decoder(decoder) else {
        return Copied::Undecodable;
    };
    let flat = flattened(decoded);
    let mut scale = 1.0_f64;
    for _ in 0..SHRINK_TRIES {
        let (w, h) = (
            ((f64::from(width) * scale).round() as u32).clamp(1, width.max(1)),
            ((f64::from(height) * scale).round() as u32).clamp(1, height.max(1)),
        );
        let scaled;
        let pixels = if (w, h) == (width, height) {
            &flat
        } else {
            scaled = image::imageops::thumbnail(&flat, w, h);
            &scaled
        };
        let mut kept = Kept {
            bytes: Vec::with_capacity(limit),
            limit,
            total: 0,
        };
        let encoded = image::codecs::jpeg::JpegEncoder::new_with_quality(&mut kept, SHRINK_QUALITY)
            .write_image(pixels.as_raw(), w, h, image::ExtendedColorType::Rgb8);
        if let Err(error) = encoded {
            return Copied::Failed(format!("the JPEG encoder stopped: {error}"));
        }
        if kept.total <= limit {
            return Copied::Fits(kept.bytes);
        }
        // Bytes go roughly with pixels: scale the area by what is over,
        // and a tenth more, so the next copy most likely fits.
        scale *= (limit as f64 / kept.total as f64).sqrt() * 0.9;
    }
    Copied::StillTooLarge
}

/// `decoded` as RGB, its transparency composited onto [`BACKGROUND`]: the
/// pixels themselves when they are RGB already, else one RGB copy made
/// pixel by pixel — never a whole RGBA copy beside it.
fn flattened(decoded: image::DynamicImage) -> image::RgbImage {
    use image::GenericImageView as _;
    if let image::DynamicImage::ImageRgb8(rgb) = decoded {
        return rgb;
    }
    let (width, height) = decoded.dimensions();
    image::RgbImage::from_fn(width, height, |x, y| {
        let image::Rgba([r, g, b, a]) = decoded.get_pixel(x, y);
        let a = u32::from(a);
        // `c·a + under·(255 − a)` is at most 255·255: no overflow.
        let mix = |c: u8, under: u8| {
            ((u32::from(c) * a + u32::from(under) * (255 - a) + 127) / 255) as u8
        };
        image::Rgb([
            mix(r, BACKGROUND[0]),
            mix(g, BACKGROUND[1]),
            mix(b, BACKGROUND[2]),
        ])
    })
}

/// An encoder's output that keeps at most `limit` bytes and counts them
/// all: a copy over the limit costs no more memory than one within it.
struct Kept {
    bytes: Vec<u8>,
    limit: usize,
    total: usize,
}

impl std::io::Write for Kept {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.total = self.total.saturating_add(buf.len());
        if self.total <= self.limit {
            self.bytes.extend_from_slice(buf);
        } else if self.bytes.capacity() > 0 {
            self.bytes = Vec::new();
        }
        Ok(buf.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

/// The heading the model reads the notes under, in the turn they belong to.
pub const NOTES_FOR_THE_MODEL: &str = "keeper, about the files in this message:";

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// Matrix 1.10's caption rule: `body` is a caption only beside a
    /// `filename` it differs from; a bare file, or one whose body is its
    /// name, has none.
    #[test]
    fn a_caption_is_the_body_beside_a_different_file_name() {
        let captioned =
            json!({"msgtype": "m.file", "body": "what is the total?", "filename": "q.pdf"});
        assert_eq!(caption_of(&captioned), "what is the total?");
        let named = json!({"msgtype": "m.file", "body": "q.pdf", "filename": "q.pdf"});
        assert_eq!(caption_of(&named), "");
        let bare = json!({"msgtype": "m.image", "body": "IMG_1.jpg"});
        assert_eq!(caption_of(&bare), "");
    }

    /// A message's file: its encrypted object or its plain URI, its name
    /// from `filename` before `body`, its type from `info` before its name.
    #[test]
    fn a_messages_file_is_read_from_file_or_url() {
        let encrypted = json!({
            "msgtype": "m.file", "body": "see this", "filename": "Q3 report.PDF",
            "file": {"url": "mxc://x/1", "v": "v2"}, "info": {"size": 42},
        });
        let sent = sent_of(&encrypted);
        assert_eq!(sent.name, "Q3 report.PDF");
        assert_eq!(sent.mime, "application/pdf");
        assert_eq!(sent.declared, Some(42));
        assert_eq!(sent.source.expect("a file")["url"], "mxc://x/1");

        let plain = json!({
            "msgtype": "m.image", "body": "cat.png", "url": "mxc://x/2",
            "info": {"mimetype": "image/PNG"},
        });
        let sent = sent_of(&plain);
        assert_eq!(sent.source, Some(json!("mxc://x/2")));
        assert_eq!(sent.mime, "image/png");
        // R444/06: a file message without its file is still one, named.
        let bare = sent_of(&json!({"msgtype": "m.file", "body": "x.pdf"}));
        assert_eq!((bare.name.as_str(), bare.source), ("x.pdf", None));
    }

    /// A name is a plain file name of the session whatever the client sent.
    #[test]
    fn a_sent_name_becomes_a_plain_file_name() {
        assert_eq!(file_name("../../etc/passwd", "text/plain"), "passwd");
        assert_eq!(file_name("C:\\Users\\a\\b.csv", "text/csv"), "b.csv");
        assert_eq!(file_name(".hidden.md", "text/markdown"), "hidden.md");
        assert_eq!(file_name("a:b?c\u{7}.txt", "text/plain"), "a-b-c.txt");
        assert_eq!(file_name("...", "image/jpeg"), "attachment.jpg");
        assert_eq!(file_name("", "application/pdf"), "attachment.pdf");
        let long = format!("{}.pdf", "é".repeat(100));
        let cut = file_name(&long, "application/pdf");
        assert!(cut.len() <= MAX_NAME_BYTES, "{}", cut.len());
        assert!(cut.ends_with(".pdf"), "{cut}");
    }

    /// The kinds, by type first and by name where the type says nothing;
    /// HEIC is a photo no model is shown, named by its format (R444/05).
    #[test]
    fn a_file_is_an_image_a_pdf_text_or_other() {
        assert_eq!(kind_of("image/jpeg", "p.jpg"), Kind::Image);
        assert_eq!(
            kind_of("image/heic", "p.heic"),
            Kind::Unshown("HEIC".to_owned())
        );
        assert_eq!(
            kind_of("image/x-tiff", "p.tif"),
            Kind::Unshown("TIFF".to_owned())
        );
        assert_eq!(kind_of("application/zip", "p.png"), Kind::Other);
        assert_eq!(kind_of("application/pdf", "x"), Kind::Pdf);
        assert_eq!(kind_of("application/octet-stream", "x.pdf"), Kind::Pdf);
        assert_eq!(kind_of("text/csv; charset=utf-8", "d.csv"), Kind::Text);
        assert_eq!(kind_of("application/octet-stream", "d.csv"), Kind::Text);
        assert_eq!(kind_of("application/json", "d"), Kind::Text);
        assert_eq!(kind_of("application/zip", "d.zip"), Kind::Other);
    }

    /// Story 102.4 #1: a recording and a video are files an agent is sent —
    /// taken, stored, said not to be listened to — by their message type
    /// whatever type they name, and by type or name inside an `m.file`.
    #[test]
    fn a_recording_or_a_video_is_taken_and_unheard() {
        for msgtype in ["m.audio", "m.video"] {
            let content = json!({
                "msgtype": msgtype, "body": "what is said here?", "filename": "clip.bin",
                "file": {"url": "mxc://x/3", "v": "v2"},
                "info": {"mimetype": "application/octet-stream"},
            });
            assert!(is_attachment(&content), "{msgtype}");
            assert_eq!(caption_of(&content), "what is said here?");
            assert_eq!(kind_of_message(&content, &sent_of(&content)), Kind::Unheard);
        }
        let file = json!({"msgtype": "m.file", "body": "memo.m4a", "url": "mxc://x/4"});
        assert_eq!(kind_of_message(&file, &sent_of(&file)), Kind::Unheard);
        assert_eq!(kind_of("video/quicktime", "x"), Kind::Unheard);
        assert_eq!(kind_of("application/zip", "d.zip"), Kind::Other);
        assert!(!is_attachment(
            &json!({"msgtype": "m.location", "body": "here"})
        ));
    }

    /// A PDF's text is read; a page that only draws holds none; bytes that
    /// are no PDF are unreadable, never a panic.
    #[test]
    fn a_pdfs_text_is_read_or_said_to_be_missing() {
        let fixtures =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/attachments");
        let quarterly = std::fs::read(fixtures.join("quarterly.pdf")).expect("fixture");
        match pdf_text(&quarterly) {
            PdfText::Text(text) => assert!(text.contains("Quarterly total 4217"), "{text}"),
            other => panic!("no text: {other:?}"),
        }
        let scan = std::fs::read(fixtures.join("scan.pdf")).expect("fixture");
        assert_eq!(pdf_text(&scan), PdfText::NoText);
        assert!(matches!(pdf_text(b"not a pdf"), PdfText::Unreadable(_)));
    }

    /// Text past the cap is cut at a character and says it was.
    #[test]
    fn a_long_pdf_text_is_cut_and_says_so() {
        let long = "ż".repeat(MAX_PDF_TEXT_BYTES);
        let cut = bounded(long);
        assert!(cut.len() < MAX_PDF_TEXT_BYTES + 200);
        assert!(cut.ends_with("not in this file.]"));
        assert_eq!(bounded("short".to_owned()), "short");
    }

    /// R417: a picture goes to a model that can see or did not say; one
    /// that said it cannot gets a note instead, as does a picture over the
    /// size a model is shown that no copy is made of — here, a host with no
    /// copier; the notes ride the message as one text part.
    #[tokio::test]
    async fn a_picture_is_shown_unless_the_model_said_it_cannot_see() {
        let (_handle, stop) = keeper_core::bots::chat::cancellation();
        let photo = || Picture {
            name: "photo.jpg".to_owned(),
            mime: "image/jpeg".to_owned(),
            bytes: vec![0xff, 0xd8, 0xff],
        };
        let images = |parts: &[ContentPart]| {
            parts
                .iter()
                .filter(|part| matches!(part, ContentPart::Image { .. }))
                .count()
        };
        for vision in [Some(true), None] {
            let (parts, notes) =
                shown(vec![photo()], Vec::new(), vision, "llava", None, &stop).await;
            assert_eq!(images(&parts), 1, "{vision:?}");
            assert!(notes.is_empty(), "{vision:?}");
            // R444/07: the photo is file content, after the sentence that
            // says file content is data.
            assert!(
                matches!(&parts[..], [ContentPart::Text(said), ContentPart::Image { .. }] if said == FILE_CONTENT_IS_DATA),
                "{parts:?}"
            );
        }
        let (parts, notes) =
            shown(vec![photo()], Vec::new(), Some(false), "qwen3", None, &stop).await;
        assert_eq!(images(&parts), 0);
        assert_eq!(notes, [cannot_see("qwen3", "photo.jpg")]);
        assert!(matches!(&parts[..], [ContentPart::Text(text)] if text.contains(&notes[0])));

        let huge = Picture {
            bytes: vec![0; deliverable::MAX_IMAGE_BYTES + 1],
            ..photo()
        };
        let earlier = no_text("scan.pdf");
        let (parts, notes) = shown(
            vec![huge],
            vec![earlier.clone()],
            Some(true),
            "llava",
            None,
            &stop,
        )
        .await;
        assert_eq!(images(&parts), 0);
        assert_eq!(notes.len(), 2);
        assert_eq!(notes[0], earlier);
    }

    /// Noise no codec compresses, `side` pixels square, as a PNG.
    fn noise_png(side: u32) -> Vec<u8> {
        use image::ImageEncoder;
        let mut state = 0x2545_f491_u32;
        let pixels: Vec<u8> = (0..side * side * 3)
            .map(|_| {
                state ^= state << 13;
                state ^= state >> 17;
                state ^= state << 5;
                (state >> 24) as u8
            })
            .collect();
        let mut png = Vec::new();
        image::codecs::png::PngEncoder::new(&mut png)
            .write_image(&pixels, side, side, image::ExtendedColorType::Rgb8)
            .expect("png");
        png
    }

    /// The data URI of each image part.
    fn image_urls(parts: &[ContentPart]) -> Vec<&str> {
        parts
            .iter()
            .filter_map(|part| match part {
                ContentPart::Image { url, .. } => Some(url.as_str()),
                _ => None,
            })
            .collect()
    }

    /// Story 102.4 #4: a photo over what its model is shown goes as a
    /// smaller JPEG copy that fits, made by the copier in a process of its
    /// own — Anthropic's limit for a Claude model, however its gateway names
    /// it — while a model with a larger limit is shown the photo as sent; a
    /// photo keeper cannot decode, or a host with no copier, is refused with
    /// a sentence naming the limit.
    #[tokio::test]
    async fn a_photo_over_the_models_limit_is_made_smaller_or_refused() {
        let (_handle, stop) = keeper_core::bots::chat::cancellation();
        let copier = worker(Duration::from_secs(60), IMAGE_COPY_MEMORY, None);
        let png = noise_png(1200);
        assert!(
            png.len() > deliverable::ANTHROPIC_MAX_IMAGE_BYTES,
            "{}",
            png.len()
        );
        assert!(png.len() <= deliverable::MAX_IMAGE_BYTES, "{}", png.len());
        let photo = Picture {
            name: "screen.png".to_owned(),
            mime: "image/png".to_owned(),
            bytes: png,
        };
        let claude = "claude-haiku-5-5";
        let (parts, notes) = shown(
            vec![photo.clone()],
            Vec::new(),
            None,
            claude,
            Some(&copier),
            &stop,
        )
        .await;
        assert!(notes.is_empty(), "{notes:?}");
        let urls = image_urls(&parts);
        assert_eq!(urls.len(), 1);
        let encoded = urls[0]
            .strip_prefix("data:image/jpeg;base64,")
            .expect("a JPEG copy");
        assert!(
            encoded.len() <= 5 * 1024 * 1024,
            "the copy's base64 is {} bytes",
            encoded.len()
        );

        let (parts, notes) = shown(
            vec![photo.clone()],
            Vec::new(),
            None,
            "llava",
            Some(&copier),
            &stop,
        )
        .await;
        assert!(notes.is_empty(), "{notes:?}");
        assert!(image_urls(&parts)[0].starts_with("data:image/png;base64,"));

        let Copied::Fits(copy) = smaller(&photo.mime, &photo.bytes, 100 * 1024) else {
            panic!("no copy that fits");
        };
        assert!(copy.len() <= 100 * 1024, "{}", copy.len());
        assert!(copy.starts_with(&[0xff, 0xd8]), "a JPEG");

        let (parts, notes) =
            shown(vec![photo.clone()], Vec::new(), None, claude, None, &stop).await;
        assert!(image_urls(&parts).is_empty());
        assert_eq!(
            notes,
            [too_large_to_show(
                "screen.png",
                photo.bytes.len() as u64,
                claude,
                deliverable::ANTHROPIC_MAX_IMAGE_BYTES
            )]
        );

        let webp = Picture {
            name: "big.webp".to_owned(),
            mime: "image/webp".to_owned(),
            bytes: vec![0; deliverable::ANTHROPIC_MAX_IMAGE_BYTES + 1],
        };
        let model = "anthropic/claude-opus-4";
        let (parts, notes) = shown(
            vec![webp],
            Vec::new(),
            Some(true),
            model,
            Some(&copier),
            &stop,
        )
        .await;
        assert!(image_urls(&parts).is_empty());
        assert_eq!(
            notes,
            [too_large_to_show(
                "big.webp",
                deliverable::ANTHROPIC_MAX_IMAGE_BYTES as u64 + 1,
                model,
                deliverable::ANTHROPIC_MAX_IMAGE_BYTES
            )]
        );
        assert!(notes[0].contains("3.8 MB"), "{}", notes[0]);
    }

    /// A PNG that says it is `width` × `height` pixels of PNG colour type
    /// `color` and holds one empty deflate block: a few dozen bytes,
    /// whatever it claims.
    fn claimed_png(width: u32, height: u32, color: u8) -> Vec<u8> {
        let mut png = b"\x89PNG\r\n\x1a\n".to_vec();
        let mut header = width.to_be_bytes().to_vec();
        header.extend_from_slice(&height.to_be_bytes());
        header.extend_from_slice(&[8, color, 0, 0, 0]);
        png_chunk(&mut png, b"IHDR", &header);
        // zlib: a final fixed-Huffman block that only ends, Adler-32 of
        // nothing.
        png_chunk(&mut png, b"IDAT", &[0x78, 0x01, 0x03, 0x00, 0, 0, 0, 1]);
        png_chunk(&mut png, b"IEND", &[]);
        png
    }

    /// R489/01: a photo whose header claims more pixels than a copy's
    /// working memory holds is refused from that header, before a pixel is
    /// decoded — a few dozen bytes saying 20 000 × 20 000 grey (400 MB
    /// decoded, 2.8 GB with its copies). The budget counts the colours: a
    /// 9 000 × 9 000 RGBA one is refused for its flattened copy, while the
    /// RGB one, needing none, is within it and goes on to be decoded — and
    /// these bytes do not decode.
    #[test]
    fn a_photo_too_large_to_copy_is_refused_from_its_header() {
        let limit = 1024 * 1024;
        let grey = claimed_png(20_000, 20_000, 0);
        assert!(grey.len() < 100, "{}", grey.len());
        assert_eq!(smaller("image/png", &grey, limit), Copied::OverBudget);
        assert_eq!(
            smaller("image/png", &claimed_png(9_000, 9_000, 6), limit),
            Copied::OverBudget
        );
        assert_eq!(
            smaller("image/png", &claimed_png(9_000, 9_000, 2), limit),
            Copied::Undecodable
        );
    }

    /// A JPEG's header and no picture: an 8-bit frame of type `sof` (`0xc0`
    /// sequential, `0xc2` progressive), `width` × `height`, one component
    /// per `sampling` byte (its horizontal factor high, its vertical low),
    /// one quantization table and one DC table, a scan of every component's
    /// DC — a few dozen bytes, whatever it claims.
    fn claimed_jpeg(width: u16, height: u16, sof: u8, sampling: &[u8]) -> Vec<u8> {
        fn segment(jpeg: &mut Vec<u8>, marker: u8, data: &[u8]) {
            jpeg.extend_from_slice(&[0xff, marker]);
            jpeg.extend_from_slice(&((data.len() + 2) as u16).to_be_bytes());
            jpeg.extend_from_slice(data);
        }
        let components = sampling.len() as u8;
        let mut jpeg = vec![0xff, 0xd8];
        let mut table = vec![0];
        table.extend([1; 64]);
        segment(&mut jpeg, 0xdb, &table);
        let mut frame = vec![8];
        frame.extend_from_slice(&height.to_be_bytes());
        frame.extend_from_slice(&width.to_be_bytes());
        frame.push(components);
        for (id, factors) in (1..).zip(sampling) {
            frame.extend([id, *factors, 0]);
        }
        segment(&mut jpeg, sof, &frame);
        // One code, one bit long, for the symbol 0.
        let mut dc = vec![0x00, 1];
        dc.extend([0; 15]);
        dc.push(0);
        segment(&mut jpeg, 0xc4, &dc);
        let mut scan = vec![components];
        for id in 1..=components {
            scan.extend([id, 0x00]);
        }
        scan.extend([0, 0, 0]);
        segment(&mut jpeg, 0xda, &scan);
        jpeg.extend([0; 16]);
        jpeg.extend([0xff, 0xd9]);
        jpeg
    }

    /// R512 (review `102attachments-2` 01): a JPEG's working memory counts
    /// what its decoder allocates beside its pixels, read from its header
    /// by that decoder. A progressive 9 000 × 9 000 4:4:4 photo — within
    /// the budget by its decoded pixels and their copies alone — is over it
    /// once its coefficients are counted, an image of `i16`s per component;
    /// and a 48-megapixel 4:2:0 phone photo of 16 MiB is within it.
    #[test]
    fn a_jpegs_decoder_is_counted_in_its_working_memory() {
        let limit = deliverable::ANTHROPIC_MAX_IMAGE_BYTES;
        let progressive = claimed_jpeg(9_000, 9_000, 0xc2, &[0x11, 0x11, 0x11]);
        let (width, height, decoding) = jpeg_decoding(&progressive).expect("a header");
        assert_eq!((width, height), (9_000, 9_000));
        assert_eq!(
            decoding,
            Decoding::Jpeg {
                input: progressive.len() as u64,
                components: 3,
                h_max: 1,
                v_max: 1,
            }
        );
        let pixels_and_copies = 9_000 * 9_000 * 3 * 2 + limit as u64;
        assert!(pixels_and_copies <= SHRINK_WORKING_MEMORY);
        let needs = working_memory(width, height, image::ColorType::Rgb8, decoding, limit);
        assert!(
            needs.is_some_and(|needs| needs > SHRINK_WORKING_MEMORY),
            "{needs:?}"
        );

        let (.., subsampled) =
            jpeg_decoding(&claimed_jpeg(8_064, 6_048, 0xc0, &[0x22, 0x11, 0x11]))
                .expect("a header");
        let Decoding::Jpeg {
            components,
            h_max,
            v_max,
            ..
        } = subsampled
        else {
            panic!("{subsampled:?}");
        };
        assert_eq!((components, h_max, v_max), (3, 2, 2));
        let phone = Decoding::Jpeg {
            input: 16 * 1024 * 1024,
            components,
            h_max,
            v_max,
        };
        let needs = working_memory(8_064, 6_048, image::ColorType::Rgb8, phone, limit);
        assert!(
            needs.is_some_and(|needs| needs <= SHRINK_WORKING_MEMORY),
            "{needs:?}"
        );
    }

    /// R512 (review `102attachments-2` 01): the photo above is refused
    /// before any large allocation — by `smaller`, and by a copier whose
    /// memory, on Linux, is far below what decoding it takes, which answers
    /// that it is over budget rather than dying in the decoder.
    #[tokio::test]
    async fn a_progressive_jpeg_too_large_to_decode_is_refused_before_decoding() {
        let (_handle, stop) = keeper_core::bots::chat::cancellation();
        let limit = deliverable::ANTHROPIC_MAX_IMAGE_BYTES;
        let jpeg = claimed_jpeg(9_000, 9_000, 0xc2, &[0x11, 0x11, 0x11]);
        assert_eq!(smaller("image/jpeg", &jpeg, limit), Copied::OverBudget);
        let copier = worker(Duration::from_secs(30), 64 * 1024 * 1024, None);
        let copied = copier.copy_image("image/jpeg", jpeg, limit, &stop).await;
        assert_eq!(copied, Some(Copied::OverBudget));
    }

    /// R512 (review `102attachments-2` 01): a PNG's decoder holds its
    /// chunks and a few rows beside its pixels, never an image of its own,
    /// so a 9 000 × 9 000 RGB PNG is within the working memory the same
    /// progressive JPEG is not.
    #[test]
    fn a_pngs_decoder_is_counted_in_its_working_memory() {
        let limit = deliverable::ANTHROPIC_MAX_IMAGE_BYTES;
        let needs = working_memory(9_000, 9_000, image::ColorType::Rgb8, Decoding::Png, limit);
        assert!(
            needs.is_some_and(|needs| needs <= SHRINK_WORKING_MEMORY),
            "{needs:?}"
        );
    }

    /// R512 (review `102attachments-2` 01): a PNG's chunks are held to
    /// [`PNG_METADATA`] by its decoder: a colour profile of a few MB
    /// inflating to 600 MB is let go at that allowance — the photo is still
    /// copied — by a copier whose memory, on Linux, is far below what
    /// inflating it takes; a decoder allowed more dies there.
    #[tokio::test]
    async fn a_pngs_chunks_are_held_to_their_allowance() {
        use image::ImageEncoder;
        let (_handle, stop) = keeper_core::bots::chat::cancellation();
        let mut png = Vec::new();
        image::codecs::png::PngEncoder::new(&mut png)
            .write_image(&[128; 8 * 8 * 3], 8, 8, image::ExtendedColorType::Rgb8)
            .expect("png");
        let mut profile = b"icc\0\0".to_vec();
        profile.extend(zeros_deflated(600 * 1024 * 1024));
        let mut iccp = Vec::new();
        png_chunk(&mut iccp, b"iCCP", &profile);
        // After the signature and the IHDR chunk.
        png.splice(33..33, iccp);
        assert!(png.len() < 8 * 1024 * 1024, "{}", png.len());
        let copier = worker(Duration::from_secs(30), 128 * 1024 * 1024, None);
        let limit = deliverable::ANTHROPIC_MAX_IMAGE_BYTES;
        let copied = copier.copy_image("image/png", png, limit, &stop).await;
        assert!(matches!(copied, Some(Copied::Fits(_))), "{copied:?}");
    }

    /// R489/05: a photo's transparency is composited onto [`BACKGROUND`]
    /// before its JPEG is made — a transparent red pixel is the background,
    /// a half-transparent black one halfway to it — never the colour the
    /// transparency hid.
    #[test]
    fn transparency_is_flattened_onto_the_background() {
        use image::ImageEncoder;
        let side = 32_u32;
        let pixels: Vec<u8> = (0..side * side)
            .flat_map(|at| {
                if at % side < side / 2 {
                    [255, 0, 0, 0]
                } else {
                    [0, 0, 0, 128]
                }
            })
            .collect();
        let mut png = Vec::new();
        image::codecs::png::PngEncoder::new(&mut png)
            .write_image(&pixels, side, side, image::ExtendedColorType::Rgba8)
            .expect("png");
        let Copied::Fits(jpeg) = smaller("image/png", &png, 1024 * 1024) else {
            panic!("no copy");
        };
        let copy = image::load_from_memory_with_format(&jpeg, image::ImageFormat::Jpeg)
            .expect("a JPEG")
            .to_rgb8();
        let near =
            |got: [u8; 3], want: [u8; 3]| got.iter().zip(want).all(|(g, w)| g.abs_diff(w) <= 6);
        let clear = copy.get_pixel(side / 4, side / 2).0;
        assert!(near(clear, BACKGROUND), "{clear:?}");
        let half = BACKGROUND.map(|under| ((u32::from(under) * 127 + 127) / 255) as u8);
        let shaded = copy.get_pixel(3 * side / 4, side / 2).0;
        assert!(near(shaded, half), "{shaded:?} for {half:?}");
    }

    /// R444/08: a size reads rounded up, never overflowing — the largest a
    /// message may claim too — and a size past the bound never reads as it.
    #[test]
    fn a_size_reads_whatever_a_message_claims() {
        assert!(too_large("x", u64::MAX).contains("17592186044416.0 MB"));
        assert!(too_large("x", MAX_BYTES).starts_with("x is 25.0 MB"));
        assert!(too_large("x", MAX_BYTES + 1).starts_with("x is 25.1 MB"));
        assert_eq!(human(1024 * 1024 - 1), "1024 kB");
    }

    /// This test binary as a worker ([`worker_child`]), with the bounds a
    /// test can wait for.
    fn worker(within: Duration, memory: u64, stall: Option<&Path>) -> Worker {
        let mut args: Vec<OsString> =
            ["attachments::tests::worker_child", "--exact", "--nocapture"]
                .map(OsString::from)
                .to_vec();
        // A filter no test matches: libtest still runs the one `--exact`
        // names, and the child reads it.
        args.extend(stall.map(|pid| OsString::from(format!("{STALL}{}", pid.display()))));
        Worker {
            program: std::env::current_exe().expect("this test binary"),
            args,
            within,
            memory,
            cpu_seconds: 20,
        }
    }

    /// The worker, when this binary was started as one; one that stalls,
    /// its pid written where its `stall:` argument says, when it has one.
    #[test]
    fn worker_child() {
        let args: Vec<OsString> = std::env::args_os().collect();
        let Some(at) = args
            .iter()
            .position(|arg| arg == PDF_READER_ARG || arg == IMAGE_COPIER_ARG)
        else {
            return;
        };
        let stall = args
            .iter()
            .find_map(|arg| arg.to_str()?.strip_prefix(STALL).map(PathBuf::from));
        if let Some(pid) = stall {
            std::fs::write(pid, std::process::id().to_string()).expect("pid");
            loop {
                std::thread::sleep(Duration::from_secs(60));
            }
        }
        std::process::exit(served_as_worker(args[at..].iter().cloned()).expect("argv"));
    }

    /// The argument that makes a test reader stall, naming its pid's file.
    const STALL: &str = "stall:";

    /// One PNG chunk of `kind` holding `data`, at the end of `png`.
    fn png_chunk(png: &mut Vec<u8>, kind: &[u8; 4], data: &[u8]) {
        fn crc32(bytes: &[u8]) -> u32 {
            let mut crc = !0_u32;
            for &byte in bytes {
                crc ^= u32::from(byte);
                for _ in 0..8 {
                    crc = if crc & 1 == 1 {
                        (crc >> 1) ^ 0xedb8_8320
                    } else {
                        crc >> 1
                    };
                }
            }
            !crc
        }
        png.extend_from_slice(&(data.len() as u32).to_be_bytes());
        let start = png.len();
        png.extend_from_slice(kind);
        png.extend_from_slice(data);
        let crc = crc32(&png[start..]);
        png.extend_from_slice(&crc.to_be_bytes());
    }

    /// A zlib stream inflating to `inflated` zero bytes from about 1/160 of
    /// that, written by hand — one fixed-Huffman block of a zero then
    /// copies of 258 bytes from one back — so making it costs nothing.
    fn zeros_deflated(inflated: usize) -> Vec<u8> {
        struct Bits {
            out: Vec<u8>,
            held: u64,
            count: u32,
        }
        impl Bits {
            fn put(&mut self, value: u64, count: u32) {
                self.held |= value << self.count;
                self.count += count;
                while self.count >= 8 {
                    self.out.push(self.held as u8);
                    self.held >>= 8;
                    self.count -= 8;
                }
            }
            /// A Huffman code, its first bit its highest.
            fn code(&mut self, code: u64, count: u32) {
                let reversed =
                    (0..count).fold(0, |r, i| r | (((code >> i) & 1) << (count - 1 - i)));
                self.put(reversed, count);
            }
        }
        let copies = (inflated - 1) / 258;
        let mut bits = Bits {
            out: vec![0x78, 0x01],
            held: 0,
            count: 0,
        };
        bits.put(1, 1); // the last block
        bits.put(1, 2); // fixed Huffman codes
        bits.code(0x30, 8); // the literal 0
        for _ in 0..copies {
            bits.code(0xc5, 8); // length 258
            bits.code(0, 5); // distance 1
        }
        bits.code(0, 7); // the block's end
        if bits.count > 0 {
            bits.out.push(bits.held as u8);
        }
        let total = (1 + copies * 258) as u64;
        let adler = (((total % 65521) as u32) << 16) | 1;
        bits.out.extend_from_slice(&adler.to_be_bytes());
        bits.out
    }

    /// A PDF whose one content stream inflates to `inflated` zero bytes
    /// ([`zeros_deflated`]): a decompression bomb, made here, not
    /// committed.
    fn bomb(inflated: usize) -> Vec<u8> {
        let stream = zeros_deflated(inflated);
        let mut pdf = b"%PDF-1.4\n".to_vec();
        let mut offsets = Vec::new();
        let mut object = |pdf: &mut Vec<u8>, body: &[u8]| {
            offsets.push(pdf.len());
            pdf.extend_from_slice(format!("{} 0 obj\n", offsets.len()).as_bytes());
            pdf.extend_from_slice(body);
            pdf.extend_from_slice(b"\nendobj\n");
        };
        object(&mut pdf, b"<< /Type /Catalog /Pages 2 0 R >>");
        object(&mut pdf, b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>");
        object(
            &mut pdf,
            b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Contents 4 0 R >>",
        );
        let mut contents = format!(
            "<< /Length {} /Filter /FlateDecode >>\nstream\n",
            stream.len()
        )
        .into_bytes();
        contents.extend_from_slice(&stream);
        contents.extend_from_slice(b"\nendstream");
        object(&mut pdf, &contents);
        let xref = pdf.len();
        pdf.extend_from_slice(
            format!("xref\n0 {}\n0000000000 65535 f \n", offsets.len() + 1).as_bytes(),
        );
        for offset in &offsets {
            pdf.extend_from_slice(format!("{offset:010} 00000 n \n").as_bytes());
        }
        pdf.extend_from_slice(
            format!(
                "trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n",
                offsets.len() + 1
            )
            .as_bytes(),
        );
        pdf
    }

    /// Whether the process `pid` is still there, a zombie too.
    fn alive(pid: &str) -> bool {
        std::process::Command::new("ps")
            .args(["-p", pid, "-o", "pid="])
            .output()
            .is_ok_and(|ps| !String::from_utf8_lossy(&ps.stdout).trim().is_empty())
    }

    /// R444/03: the reader reads a PDF's text in its own process, says when
    /// one has none, and calls bytes that are no PDF unreadable.
    #[tokio::test]
    async fn the_reader_reads_text_in_a_process_of_its_own() {
        let (_handle, stop) = keeper_core::bots::chat::cancellation();
        let reader = worker(Duration::from_secs(30), PDF_MEMORY, None);
        let fixtures =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/attachments");
        let quarterly = std::fs::read(fixtures.join("quarterly.pdf")).expect("fixture");
        match reader.read_pdf(quarterly, &stop).await {
            Some(PdfText::Text(text)) => assert!(text.contains("Quarterly total 4217"), "{text}"),
            other => panic!("no text: {other:?}"),
        }
        let scan = std::fs::read(fixtures.join("scan.pdf")).expect("fixture");
        assert_eq!(reader.read_pdf(scan, &stop).await, Some(PdfText::NoText));
        assert!(matches!(
            reader.read_pdf(b"not a pdf".to_vec(), &stop).await,
            Some(PdfText::Unreadable(_))
        ));
    }

    /// R444/03, R454: a decompression bomb — a small PDF inflating far past
    /// the reader's memory — ends its reader, not the host. Everywhere the
    /// clock and the CPU bound it, so the read is over in time; on Linux the
    /// address-space allowance is what ends it, so the PDF is unreadable. A
    /// Mac enforces no address-space limit, so there what it read is not
    /// asked.
    #[tokio::test]
    async fn a_decompression_bomb_ends_its_reader_not_the_host() {
        let (_handle, stop) = keeper_core::bots::chat::cancellation();
        let pdf = bomb(768 * 1024 * 1024);
        assert!(pdf.len() < 8 * 1024 * 1024, "{}", pdf.len());
        let reader = worker(Duration::from_secs(20), 256 * 1024 * 1024, None);
        let began = std::time::Instant::now();
        let read = reader.read_pdf(pdf, &stop).await;
        assert!(
            began.elapsed() < Duration::from_secs(25),
            "{read:?} after {:?}",
            began.elapsed()
        );
        #[cfg(target_os = "linux")]
        assert!(matches!(read, Some(PdfText::Unreadable(_))), "{read:?}");
    }

    /// R444/03, R489/01: a worker that runs past its clock is killed and
    /// reaped — gone, not a zombie — and the PDF is unreadable, the photo's
    /// copy not made; a turn that stops kills its worker the same way, and
    /// has no answer. The codec in the copier cannot be interrupted; its
    /// process can.
    #[tokio::test]
    async fn a_worker_past_its_clock_or_its_turn_is_killed_and_reaped() {
        let dir = tempfile::tempdir().expect("dir");
        for copying in [false, true] {
            for stopping in [false, true] {
                let pid_file = dir.path().join(format!("pid-{copying}-{stopping}"));
                let (handle, stop) = keeper_core::bots::chat::cancellation();
                let within = Duration::from_millis(if stopping { 30_000 } else { 1_500 });
                let memory = if copying {
                    IMAGE_COPY_MEMORY
                } else {
                    PDF_MEMORY
                };
                let worker = worker(within, memory, Some(&pid_file));
                let stopper = async {
                    if stopping {
                        while !pid_file.exists() {
                            tokio::time::sleep(Duration::from_millis(50)).await;
                        }
                        handle.cancel();
                    }
                };
                let began = std::time::Instant::now();
                let work = async {
                    if copying {
                        let png = noise_png(64);
                        let copied = worker.copy_image("image/png", png, 1024, &stop).await;
                        if !stopping {
                            assert!(matches!(copied, Some(Copied::Failed(_))), "{copied:?}");
                        }
                        copied.is_some()
                    } else {
                        let read = worker.read_pdf(b"%PDF-1.4".to_vec(), &stop).await;
                        if !stopping {
                            assert!(matches!(read, Some(PdfText::Unreadable(_))), "{read:?}");
                        }
                        read.is_some()
                    }
                };
                let (answered, ()) = tokio::join!(work, stopper);
                assert!(
                    began.elapsed() < Duration::from_secs(15),
                    "{:?}",
                    began.elapsed()
                );
                assert_eq!(answered, !stopping, "copying {copying}");
                let pid = std::fs::read_to_string(&pid_file).expect("the worker ran");
                assert!(!alive(pid.trim()), "worker {pid} outlived its work");
            }
        }
    }
}
