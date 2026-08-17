//! The git `clean`/`smudge` filter, shared by every binary that can be one
//! (Story 34.19, DW-121).
//!
//! # Why this is shared rather than owned by the daemon
//!
//! [`crate::engine::Engine::open`] registers `filter.lfs.clean` /
//! `filter.lfs.smudge` as `std::env::current_exe()`, whichever executable that
//! is — the daemon in a CLI run, the app binary in a desktop run. The
//! implementation lived in `keeper-syncd` alone, so the claim in that comment
//! ("both understand `lfs clean|smudge`") was only half true: the app binary had
//! no such subcommand and the filter failed on **every** invocation from the
//! desktop app.
//!
//! It failed silently, which is why it stood so long. keeper also sets
//! `filter.lfs.required=false` — deliberately, so a moved binary degrades to
//! pointer files instead of hard-failing every git command in the repository —
//! and git cannot tell a filter that failed from one that was never configured.
//! In both cases it stores the bytes it was handed.
//!
//! The consequence is not cosmetic. A racily-clean LFS entry (one whose mtime is
//! not older than the index) is re-read by git as a *content* comparison, which
//! runs this filter. With a working one the worktree bytes are cleaned back into
//! the pointer the index already holds and the path reads clean; without one the
//! raw gigabytes are hashed and every LFS-tracked file reads as modified. The two
//! platforms keeper ships to do not even agree on the failure: the same fixture
//! reports MODIFIED on Linux/ext4 and CLEAN on macOS/APFS.
//!
//! So the logic lives here, once, and both binaries call it. AD-52 makes the
//! engine shared verbatim between the app and the daemon; a filter with two
//! implementations would be the same mistake one layer down.
//!
//! # Contract
//!
//! git hands the filter one file on stdin and reads the replacement from stdout.
//! Neither direction may write anything else to stdout — not a log line, not a
//! warning — because every byte is content.
//!
//! # Why there is a second, long-running shape (DW-140)
//!
//! Registering `clean`/`smudge` is not enough, and the reason is a git config
//! rule rather than anything about this code: **when `filter.<drv>.process` is
//! defined, git uses it and ignores `clean`/`smudge` entirely**. `git lfs
//! install` writes `filter.lfs.process` into `~/.gitconfig`, and a *global*
//! process driver outranks a *repository-local* clean/smudge pair. So on any
//! machine where the real git-lfs was ever installed — every developer's — the
//! filter this module registers was never once invoked. It was not a fallback;
//! it was dead code.
//!
//! What ran instead was git-lfs, which downloads objects itself, and when it
//! cannot resolve one it dies mid-protocol:
//!
//! ```text
//! error: external filter 'git-lfs filter-process' failed
//! error: external filter 'git-lfs filter-process' is not available anymore
//!        although not all paths have been filtered
//! ```
//!
//! Under `filter.lfs.required=false` git swallows that and writes **zero
//! bytes** — for that path *and every remaining path in the same checkout*. On
//! 2026-08-16 one object missing from the server turned 122 recordings, 74 GB
//! of pointers, into 122 empty files in a single fast-forward. The pointers
//! survived only because nothing committed the worktree before it was noticed.
//!
//! Hence [`run_process`]. Owning `filter.lfs.process` is the only way to stop a
//! globally-installed git-lfs from silently outranking us, and a filter that
//! answers every path itself is the only way the cascade cannot start.
//!
//! ## `required` stays false, deliberately
//!
//! The flag was never the hazard on its own. `required=true` makes a failed
//! smudge a hard error — which, verified on the same repository, leaves the
//! path **deleted** from the worktree, and a sync engine watching that folder
//! then commits the deletion. Neither setting is safe while the filter can die
//! mid-stream; both are safe once it cannot. So the fix is here, not in the
//! flag: [`run_process`] answers a per-path failure with `status=error` and
//! **never** exits. git falls back for that one path, the process stays up, and
//! the next path is filtered normally. `required=false` then keeps doing the
//! job it was chosen for — a worktree whose keeper binary moved still checks
//! out, as pointers.

use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use crate::error::{Result, SyncError};
use crate::lfs::pktline::{self, Packet};
use crate::lfs::pointer::{Pointer, MAX_POINTER_BYTES};
use crate::lfs::store::LfsStore;

/// What an `lfs …` argument list asks this binary to be.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Invocation {
    /// One file on stdin, its replacement on stdout, then exit.
    Single { direction: Direction, repo: PathBuf },
    /// Stay up and serve git's long-running protocol.
    Process { repo: PathBuf },
    /// An `lfs` verb this build does not implement. Claimed so it can be
    /// refused loudly rather than mistaken for an ordinary launch.
    Unsupported,
}

/// Which direction git is asking for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Direction {
    /// stdin = worktree bytes, stdout = pointer. Staging a file.
    Clean,
    /// stdin = pointer, stdout = worktree bytes. Checking a file out.
    Smudge,
}

/// Run one filter invocation against the repository at `repo`.
///
/// `repo` is the **working tree** root, as `%f`'s sibling `--repo` argument
/// supplies it; the object store is `<repo>/.git/lfs`.
///
/// Generic over the streams rather than locking stdio itself, which is what makes
/// it testable: a filter that could only be exercised by spawning a process and
/// feeding it a pipe is a filter nobody tests, and this one went untested and
/// unimplemented for exactly that shape of reason.
///
/// Blocking and streaming. Nothing here sizes a buffer from the file — a clean
/// hashes straight into the store as the bytes arrive (NFR-23), which is the
/// whole reason LFS exists in this engine.
pub fn run(
    repo: &Path,
    direction: Direction,
    input: &mut impl Read,
    output: &mut impl Write,
) -> Result<()> {
    let store = LfsStore::in_git_dir(repo.join(".git"));
    store.ensure_layout()?;
    match direction {
        Direction::Smudge => smudge(&store, repo, input, output),
        Direction::Clean => clean(&store, repo, input, output),
    }
}

/// Pointer in, content out.
fn smudge(
    store: &LfsStore,
    repo: &Path,
    input: &mut impl Read,
    output: &mut impl Write,
) -> Result<()> {
    // A pointer is under 1 KiB by specification, so reading it whole is bounded.
    // One byte over is proof it is not a pointer, and it passes straight through.
    let mut buffer = Vec::with_capacity(MAX_POINTER_BYTES);
    // `by_ref` so the reader stays usable for the pass-through branch below,
    // which has to stream whatever remains after this prefix.
    Read::by_ref(input)
        .take(MAX_POINTER_BYTES as u64 + 1)
        .read_to_end(&mut buffer)
        .map_err(|err| SyncError::io("read smudge input", repo, err))?;

    let resolved = Pointer::parse(&buffer)
        .filter(|pointer| store.contains(&pointer.oid, pointer.size))
        .map(|pointer| store.object_path(&pointer.oid));

    match resolved {
        Some(path) => {
            let mut object = std::fs::File::open(&path)
                .map_err(|err| SyncError::io("open lfs object", &path, err))?;
            std::io::copy(&mut object, output)
                .map_err(|err| SyncError::io("stream lfs object", &path, err))?;
        }
        // Not a pointer, or an object we do not hold yet. Passing the bytes
        // through unchanged is what git-lfs does, and it keeps a partial fetch
        // usable instead of failing the checkout.
        None => {
            output
                .write_all(&buffer)
                .map_err(|err| SyncError::io("pass through pointer", repo, err))?;
            std::io::copy(input, output)
                .map_err(|err| SyncError::io("pass through remainder", repo, err))?;
        }
    }
    output
        .flush()
        .map_err(|err| SyncError::io("flush smudge output", repo, err))
}

/// Content in, pointer out.
///
/// # An input that is already a pointer is re-emitted, not hashed
///
/// Pointer text in the worktree is not a corner case: it is a state this very
/// module produces. [`smudge`]'s `None` arm writes the pointer back whenever the
/// object is not in the store, and under `LfsMode::PointerOnly` every LFS path
/// holds pointer text permanently. So `git add`, `git commit -a`, `git stash`
/// and a racily-clean re-read all hand those ~130 bytes to this function, and
/// hashing them emits `Pointer::new(hash(P), len(P))` — a pointer naming a
/// pointer.
///
/// That fails in two stages. First the path reads as MODIFIED, because the
/// emitted pointer is not the one the index holds — enough for
/// `git merge -X theirs` or `--ff-only` to refuse with "local changes would be
/// overwritten". Then, if the index does take the clean, the commit replaces the
/// only reference every peer has to the real object with a reference to 130
/// bytes of text, and the object's oid is no longer named anywhere in the tree.
///
/// Story 34.19 makes the desktop app binary a filter, which is exactly where a
/// human runs plain `git` by hand, so the blast radius is wider here than it was
/// when only the daemon could be one. Upstream git-lfs guards the same case —
/// `CleanPointerError`, whose handler re-emits the original bytes.
///
/// Re-emitting rather than re-encoding is deliberate. [`Pointer::parse`] accepts
/// non-canonical spellings (a legacy version URL, unsorted keys), and rendering
/// one of those afresh would change the blob hash and make the path read
/// modified for a second, subtler reason.
fn clean(
    store: &LfsStore,
    repo: &Path,
    input: &mut impl Read,
    output: &mut impl Write,
) -> Result<()> {
    // The bounded-prefix trick [`smudge`] uses, for the same reason: a pointer is
    // under 1 KiB by specification, so one byte past the ceiling is proof this is
    // content. `by_ref` so the reader stays usable for the streaming branch.
    let mut head = Vec::with_capacity(MAX_POINTER_BYTES);
    Read::by_ref(input)
        .take(MAX_POINTER_BYTES as u64 + 1)
        .read_to_end(&mut head)
        .map_err(|err| SyncError::io("read clean input", repo, err))?;

    // A short read is EOF, so a `head` inside the ceiling IS the whole input —
    // which is what makes "this file is a pointer" answerable after one bounded
    // read. The emptiness guard is not redundant: `Pointer::parse` reads no bytes
    // as the *empty pointer*, and an empty file must keep taking the path below,
    // where it stores the empty object and `render` emits nothing for it.
    if !head.is_empty() && head.len() <= MAX_POINTER_BYTES && Pointer::parse(&head).is_some() {
        output
            .write_all(&head)
            .map_err(|err| SyncError::io("re-emit pointer", repo, err))?;
        return output
            .flush()
            .map_err(|err| SyncError::io("flush clean output", repo, err));
    }

    // Hashed straight into the object store as it streams, so the object is
    // already present by the time the pointer naming it is emitted. A crash
    // between the two costs a re-clean, never a pointer to nothing. `head` is
    // chained back on rather than re-read: stdin does not rewind, and those first
    // bytes are as much of the object as any other.
    let (oid, size) = store.insert_streaming(head.as_slice().chain(input))?;
    let pointer = Pointer::new(oid, size);
    output
        .write_all(pointer.render().as_bytes())
        .map_err(|err| SyncError::io("write pointer", repo, err))?;
    output
        .flush()
        .map_err(|err| SyncError::io("flush clean output", repo, err))
}

/// Serve git's long-running filter protocol until the pipe closes (DW-140).
///
/// `repo` comes from the registered command line, exactly as it does for the
/// single-shot verbs: the protocol tells us a `pathname` but never a worktree,
/// and the object store is the one thing this process cannot guess.
///
/// # The shape of the conversation
///
/// ```text
/// git> git-filter-client / version=2 / 0000      handshake
/// git< git-filter-server / version=2 / 0000
/// git> capability=clean / capability=smudge / capability=delay / 0000
/// git< capability=clean / capability=smudge / 0000   (never delay — see below)
/// git> command=smudge / pathname=a/b.mov / 0000      one file
/// git> CONTENT… / 0000
/// git< status=success / 0000
/// git< FILTERED… / 0000
/// git< 0000                                      trailing status list
/// ```
///
/// `delay` is not advertised. It buys concurrency for a filter that fetches
/// over the network, which this one never does — [`smudge`] serves what the
/// store already holds and passes the pointer through otherwise. Advertising it
/// would add a whole second state machine for no latency we have.
///
/// # Why the request is always drained before the response starts
///
/// git writes a file's whole content and only then reads the answer. A filter
/// that starts answering while the request is still arriving deadlocks the
/// moment both pipes fill — 64 KiB in, on the pipes macOS gives us. So every
/// path here consumes its request to the flush packet first. For a `clean` that
/// costs nothing: the content streams straight into the object store as it
/// arrives, which is the streaming property NFR-23 asks for. For a `smudge` the
/// request is a ~130-byte pointer, and the one case where it is not — a blob
/// committed as raw content before `.gitattributes` existed, which this
/// repository really does contain — spills to the store's scratch directory
/// rather than to memory.
pub fn run_process(repo: &Path, input: &mut impl Read, output: &mut impl Write) -> Result<()> {
    let store = LfsStore::in_git_dir(repo.join(".git"));
    store.ensure_layout()?;

    if !handshake(input, output)? {
        return Ok(());
    }

    while let Some(keys) = pktline::read_text_list(input)? {
        // An empty list before EOF is not something git sends; treating it as
        // "nothing to do" beats inventing an error for it.
        if keys.is_empty() {
            continue;
        }
        let command = value_of(&keys, "command");
        let direction = match command.as_deref() {
            Some("clean") => Some(Direction::Clean),
            Some("smudge") => Some(Direction::Smudge),
            _ => None,
        };
        let Some(direction) = direction else {
            // A command we never advertised. Refusing this one path keeps the
            // process alive for the paths we did advertise, which is the whole
            // discipline this module now runs on.
            drain(input)?;
            pktline::write_line(output, "status=error")?;
            pktline::write_flush(output)?;
            flush(output)?;
            continue;
        };
        serve_one(&store, repo, direction, input, output)?;
    }
    Ok(())
}

/// Agree on version 2 and on the capabilities we will actually honour.
///
/// Returns `false` when git hung up during the handshake, which is what a
/// capability probe looks like from in here.
fn handshake(input: &mut impl Read, output: &mut impl Write) -> Result<bool> {
    let Some(hello) = pktline::read_text_list(input)? else {
        return Ok(false);
    };
    if !hello.iter().any(|line| line == "git-filter-client") {
        return Err(SyncError::Git(format!("not a filter handshake: {hello:?}")));
    }
    // git offers a set and expects the highest common version back. Version 2
    // is the only one that has ever existed; saying so explicitly means a
    // future git that offers 3 gets a definite 2 rather than silence.
    if !hello.iter().any(|line| line == "version=2") {
        return Err(SyncError::Git(format!(
            "no supported filter protocol version in {hello:?}"
        )));
    }
    pktline::write_line(output, "git-filter-server")?;
    pktline::write_line(output, "version=2")?;
    pktline::write_flush(output)?;
    flush(output)?;

    let Some(_offered) = pktline::read_text_list(input)? else {
        return Ok(false);
    };
    pktline::write_line(output, "capability=clean")?;
    pktline::write_line(output, "capability=smudge")?;
    pktline::write_flush(output)?;
    flush(output)?;
    Ok(true)
}

/// Filter exactly one file, and never fail the process while doing it.
fn serve_one(
    store: &LfsStore,
    repo: &Path,
    direction: Direction,
    input: &mut impl Read,
    output: &mut impl Write,
) -> Result<()> {
    let body = match direction {
        Direction::Clean => {
            // The pointer is ~130 bytes however large the content was, so the
            // answer is always inline; the gigabytes went into the store as
            // they streamed past.
            let mut pointer = Vec::with_capacity(MAX_POINTER_BYTES);
            let mut content = Request::new(input);
            let outcome = clean(store, repo, &mut content, &mut pointer);
            content.drain()?;
            outcome.map(|()| Body::Inline(pointer))
        }
        Direction::Smudge => {
            let mut content = Request::new(input);
            let outcome = plan_smudge(store, repo, &mut content);
            content.drain()?;
            outcome
        }
    };

    let body = match body {
        Ok(body) => body,
        Err(err) => {
            // One path's failure, reported as one path's failure. The process
            // survives, so git filters the rest of the checkout normally and
            // falls back to the stored bytes for this one — instead of the
            // "not all paths have been filtered" cascade that empties files.
            tracing::warn!(error = %err, "lfs filter: refusing one path");
            pktline::write_line(output, "status=error")?;
            pktline::write_flush(output)?;
            return flush(output);
        }
    };

    pktline::write_line(output, "status=success")?;
    pktline::write_flush(output)?;
    let outcome = write_body(&body, output);
    // The trailing list is where a failure that only became visible mid-stream
    // is reported; git's own documentation shows exactly this exchange.
    pktline::write_flush(output)?;
    match outcome {
        Ok(()) => pktline::write_flush(output)?,
        Err(err) => {
            tracing::warn!(error = %err, "lfs filter: a body failed mid-stream");
            pktline::write_line(output, "status=error")?;
            pktline::write_flush(output)?;
        }
    }
    flush(output)
}

/// What a served path answers with, chosen without holding it in memory.
enum Body {
    /// Small enough to have been built already: a pointer, either direction.
    Inline(Vec<u8>),
    /// Stream this file back. Either an object from the store, or the spill of
    /// a request that turned out not to be a pointer at all.
    File(PathBuf),
    /// A spill this process owns and must delete once it has been sent.
    Spill(PathBuf),
}

/// Decide what a smudge answers, reading only a bounded prefix of the request.
fn plan_smudge(
    store: &LfsStore,
    repo: &Path,
    content: &mut Request<'_, impl Read>,
) -> Result<Body> {
    let mut head = Vec::with_capacity(MAX_POINTER_BYTES);
    Read::by_ref(content)
        .take(MAX_POINTER_BYTES as u64 + 1)
        .read_to_end(&mut head)
        .map_err(|err| SyncError::io("read smudge request", repo, err))?;

    if head.len() <= MAX_POINTER_BYTES {
        if let Some(pointer) = Pointer::parse(&head) {
            return Ok(if store.contains(&pointer.oid, pointer.size) {
                Body::File(store.object_path(&pointer.oid))
            } else {
                // The object we do not hold. Passing the pointer through is
                // what keeps a partial fetch usable, and — unlike an empty
                // file — it is a state the clean filter turns back into itself.
                Body::Inline(head)
            });
        }
        // Not a pointer, and it all fits: hand the bytes straight back.
        return Ok(Body::Inline(head));
    }

    // Over the pointer ceiling: a blob committed as raw content. Rare, and
    // degenerate — it is the case LFS exists to prevent — but this repository
    // has real ones from before `.gitattributes` was written, so it has to
    // work. Spilling keeps memory bounded whatever the size.
    let path = store
        .tmp_dir()
        .join(format!("smudge-{}.spill", std::process::id()));
    let mut spill = std::fs::File::create(&path)
        .map_err(|err| SyncError::io("create smudge spill", &path, err))?;
    spill
        .write_all(&head)
        .map_err(|err| SyncError::io("write smudge spill", &path, err))?;
    std::io::copy(content, &mut spill)
        .map_err(|err| SyncError::io("spill smudge request", &path, err))?;
    Ok(Body::Spill(path))
}

/// Send a body as content packets.
fn write_body(body: &Body, output: &mut impl Write) -> Result<()> {
    match body {
        Body::Inline(bytes) => {
            for chunk in bytes.chunks(pktline::MAX_DATA) {
                pktline::write_data(output, chunk)?;
            }
            Ok(())
        }
        Body::File(path) | Body::Spill(path) => {
            let file = std::fs::File::open(path)
                .map_err(|err| SyncError::io("open lfs object", path, err));
            let result = file.and_then(|mut file| {
                let mut buffer = vec![0u8; pktline::MAX_DATA];
                loop {
                    let read = file
                        .read(&mut buffer)
                        .map_err(|err| SyncError::io("stream lfs object", path, err))?;
                    if read == 0 {
                        return Ok(());
                    }
                    pktline::write_data(output, &buffer[..read])?;
                }
            });
            if let Body::Spill(path) = body {
                // Best effort: a leftover spill costs disk, and failing the
                // path because the cleanup failed would trade a small leak for
                // a checkout that does not happen.
                let _ = std::fs::remove_file(path);
            }
            result
        }
    }
}

/// The content list of one request, as a [`Read`] that stops at its flush.
///
/// This is what lets [`clean`] and the smudge planner stay ordinary readers of
/// a stream, unaware that the stream is packetized — and therefore stay the
/// same code the single-shot path uses and the existing tests cover.
struct Request<'a, R: Read> {
    input: &'a mut R,
    /// Bytes of the current packet not yet handed out.
    pending: Vec<u8>,
    cursor: usize,
    done: bool,
}

impl<'a, R: Read> Request<'a, R> {
    fn new(input: &'a mut R) -> Self {
        Self {
            input,
            pending: Vec::new(),
            cursor: 0,
            done: false,
        }
    }

    /// Consume whatever is left of this request.
    ///
    /// Mandatory, not tidiness: a reader that stops early — [`smudge`] does,
    /// the moment it has the pointer — leaves packets in the pipe that the next
    /// command would parse as its own, and the protocol never recovers.
    fn drain(&mut self) -> Result<()> {
        while !self.done {
            // Discarding first is what makes this terminate. [`Self::fill`] is a
            // no-op while buffered bytes remain unread — that is what keeps it
            // cheap on the `read` path — so draining without dropping them spins
            // forever on the first packet the reader did not consume. Which is
            // every drain that has anything to do.
            self.pending.clear();
            self.cursor = 0;
            self.fill()?;
        }
        Ok(())
    }

    /// Pull the next packet in, marking the request finished at its flush.
    fn fill(&mut self) -> Result<()> {
        if self.done || self.cursor < self.pending.len() {
            return Ok(());
        }
        match pktline::read(self.input)? {
            Packet::Data(bytes) => {
                self.pending = bytes;
                self.cursor = 0;
            }
            Packet::Flush => self.done = true,
            Packet::Eof => {
                return Err(SyncError::Git(
                    "the filter's request ended without a flush".into(),
                ))
            }
        }
        Ok(())
    }
}

impl<R: Read> Read for Request<'_, R> {
    fn read(&mut self, out: &mut [u8]) -> std::io::Result<usize> {
        loop {
            if self.cursor < self.pending.len() {
                let take = (self.pending.len() - self.cursor).min(out.len());
                out[..take].copy_from_slice(&self.pending[self.cursor..self.cursor + take]);
                self.cursor += take;
                return Ok(take);
            }
            if self.done {
                return Ok(0);
            }
            self.fill()
                .map_err(|err| std::io::Error::other(err.to_string()))?;
        }
    }
}

/// Read and discard one content list, for a command we are not going to serve.
fn drain(input: &mut impl Read) -> Result<()> {
    Request::new(input).drain()
}

/// Push what we have written out to git, which is blocked reading it.
fn flush(output: &mut impl Write) -> Result<()> {
    output
        .flush()
        .map_err(|err| SyncError::Git(format!("could not flush the filter response: {err}")))
}

/// The value of one `key=value` metadata line.
fn value_of(keys: &[String], key: &str) -> Option<String> {
    keys.iter()
        .find_map(|line| line.strip_prefix(&format!("{key}=")))
        .map(str::to_owned)
}

/// Parse the filter invocation out of a raw argument list, if it is one.
///
/// Returns `None` for any argv that is not a filter call, which is the answer a
/// normal application launch needs: a GUI binary must not grow a CLI that
/// swallows the arguments macOS passes it. Finder launches pass none, and older
/// macOS passes `-psn_0_12345`; neither begins with `lfs`.
///
/// Deliberately hand-rolled rather than a `clap` surface. The app binary would
/// otherwise gain a whole argument parser — with its own `--help`, its own error
/// text on stdout, and its own opinions about unknown flags — to serve one
/// invocation whose exact shape [`crate::git::repo`] writes itself.
///
/// The accepted shape is what that writer produces:
///
/// ```text
/// <exe> lfs clean          --repo <dir> [<path>]
/// <exe> lfs smudge         --repo <dir> [<path>]
/// <exe> lfs filter-process --repo <dir>
/// ```
///
/// `<path>` is git's `%f`, advisory only: the object is addressed by digest, and
/// the path is not consulted for anything. It is accepted so the registered
/// command line can keep carrying it, because it is what makes a failing filter
/// legible in a `GIT_TRACE` log.
///
/// # Why an unrecognised `lfs` verb is not `None`
///
/// `None` means "ordinary launch", and for the app binary that means *show the
/// GUI*. A GUI that opens on `lfs filter-process` writes nothing to stdout and
/// exits when the single-instance guard sees the app already running — which
/// git, under `required=false`, records as a successful filter that produced
/// zero bytes. That is the exact mechanism that emptied 122 files, reached by
/// running an older binary against a config a newer one wrote. So anything
/// beginning with `lfs` is claimed here, and an unknown verb is reported as
/// [`Invocation::Unsupported`] for the caller to fail loudly on.
pub fn parse_args<I, S>(args: I) -> Option<Invocation>
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    let mut args = args.into_iter().map(|arg| arg.as_ref().to_owned());
    if args.next().as_deref() != Some("lfs") {
        return None;
    }
    let verb = args.next();
    let direction = match verb.as_deref() {
        Some("clean") => Some(Direction::Clean),
        Some("smudge") => Some(Direction::Smudge),
        Some("filter-process") => None,
        // `lfs` alone, or a verb from a future build. Claimed, not run.
        _ => return Some(Invocation::Unsupported),
    };
    // `--repo` is required and may be followed by the advisory path in either
    // order, so the list is scanned rather than read positionally.
    let mut repo = None;
    let mut rest = args.collect::<Vec<_>>().into_iter().peekable();
    while let Some(arg) = rest.next() {
        match arg.as_str() {
            "--repo" => repo = rest.next(),
            other if other.starts_with("--repo=") => {
                repo = Some(other.trim_start_matches("--repo=").to_owned());
            }
            // The advisory path, or an argument a future writer added. Neither
            // is a reason to refuse to filter.
            _ => {}
        }
    }
    // A direction with no repository is not actionable — the object store
    // location is the one thing a filter cannot guess — but it is still an
    // `lfs` invocation, so it is refused rather than mistaken for a launch.
    let Some(repo) = repo.filter(|value| !value.is_empty()).map(PathBuf::from) else {
        return Some(Invocation::Unsupported);
    };
    Some(match direction {
        Some(direction) => Invocation::Single { direction, repo },
        None => Invocation::Process { repo },
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// How many objects the store holds, for a test that has to prove nothing
    /// new was written.
    fn stored_objects(store: &LfsStore) -> usize {
        fn walk(dir: &Path) -> usize {
            let Ok(entries) = std::fs::read_dir(dir) else {
                return 0;
            };
            entries
                .flatten()
                .map(|entry| {
                    let path = entry.path();
                    if path.is_dir() {
                        walk(&path)
                    } else {
                        1
                    }
                })
                .sum()
        }
        walk(&store.root().join("objects"))
    }

    /// The exact command line `git::repo::enforce_local_config_with_filter`
    /// writes, minus the program. Getting this wrong means the filter silently
    /// does nothing, which is the defect this module exists to end.
    #[test]
    fn the_registered_command_line_is_recognised() {
        assert_eq!(
            parse_args(["lfs", "clean", "--repo", "/w/folder", "notes/a.bin"]).expect("a clean"),
            Invocation::Single {
                direction: Direction::Clean,
                repo: PathBuf::from("/w/folder")
            }
        );
        assert_eq!(
            parse_args(["lfs", "smudge", "--repo", "/w/folder", "notes/a.bin"]).expect("a smudge"),
            Invocation::Single {
                direction: Direction::Smudge,
                repo: PathBuf::from("/w/folder")
            }
        );
        // The process form carries no `%f`: the protocol names each path itself.
        assert_eq!(
            parse_args(["lfs", "filter-process", "--repo", "/w/folder"]).expect("a process"),
            Invocation::Process {
                repo: PathBuf::from("/w/folder")
            }
        );

        // git may hand `%f` over with no path when it has none to give, and a
        // `--repo=` spelling is the same request.
        assert!(parse_args(["lfs", "clean", "--repo", "/w"]).is_some());
        assert_eq!(
            parse_args(["lfs", "clean", "--repo=/w"]).expect("joined form"),
            Invocation::Single {
                direction: Direction::Clean,
                repo: PathBuf::from("/w")
            }
        );
    }

    /// A GUI binary must not grow a CLI that eats what the OS passes it.
    #[test]
    fn an_ordinary_launch_is_not_a_filter_invocation() {
        for argv in [
            vec![],
            // What Finder and older macOS actually pass.
            vec!["-psn_0_774République"],
            vec!["--flag"],
            // Not the first argument.
            vec!["serve", "lfs", "clean", "--repo", "/w"],
        ] {
            assert!(parse_args(argv.clone()).is_none(), "{argv:?}");
        }
    }

    /// The regression that emptied 122 files, in miniature: an `lfs` argv this
    /// build cannot serve must be *claimed and refused*, never mistaken for a
    /// launch. Returning `None` here sends the app binary into its GUI path,
    /// where it writes nothing to stdout and exits — which git, under
    /// `required=false`, records as a filter that succeeded with zero bytes.
    #[test]
    fn an_lfs_verb_this_build_cannot_serve_is_claimed_not_ignored() {
        for argv in [
            // The right verb, no direction.
            vec!["lfs"],
            // A verb from some future build, reached by running an older binary
            // against a config a newer one wrote.
            vec!["lfs", "explode", "--repo", "/w"],
            vec!["lfs", "filter-process-v3", "--repo", "/w"],
            // A direction with no repository is not actionable: the object store
            // location is the one thing the filter cannot guess.
            vec!["lfs", "clean"],
            vec!["lfs", "clean", "--repo", ""],
            vec!["lfs", "filter-process"],
        ] {
            assert_eq!(
                parse_args(argv.clone()),
                Some(Invocation::Unsupported),
                "{argv:?}"
            );
        }
    }

    /// One request as git would send it: metadata list, then the content list.
    fn request(command: &str, pathname: &str, content: &[u8]) -> Vec<u8> {
        let mut wire = Vec::new();
        pktline::write_line(&mut wire, &format!("command={command}")).expect("command");
        pktline::write_line(&mut wire, &format!("pathname={pathname}")).expect("pathname");
        pktline::write_flush(&mut wire).expect("flush");
        for chunk in content.chunks(pktline::MAX_DATA) {
            pktline::write_data(&mut wire, chunk).expect("content");
        }
        pktline::write_flush(&mut wire).expect("flush");
        wire
    }

    /// The handshake git opens with, up to and including its capability offer.
    fn hello() -> Vec<u8> {
        let mut wire = Vec::new();
        pktline::write_line(&mut wire, "git-filter-client").expect("client");
        pktline::write_line(&mut wire, "version=2").expect("version");
        pktline::write_flush(&mut wire).expect("flush");
        for capability in ["capability=clean", "capability=smudge", "capability=delay"] {
            pktline::write_line(&mut wire, capability).expect("capability");
        }
        pktline::write_flush(&mut wire).expect("flush");
        wire
    }

    /// One served path, as it comes back off the wire.
    #[derive(Debug)]
    struct Answer {
        status: Vec<String>,
        body: Vec<u8>,
        /// The trailing list, where a mid-stream failure is reported.
        trailer: Vec<String>,
    }

    /// Read the server side of a whole conversation back.
    fn replies(wire: &[u8]) -> Vec<Answer> {
        let mut wire = wire;
        // The two handshake lists.
        pktline::read_text_list(&mut wire).expect("server hello");
        pktline::read_text_list(&mut wire).expect("server capabilities");

        let mut answers = Vec::new();
        while let Some(status) = pktline::read_text_list(&mut wire).expect("status") {
            if status.is_empty() {
                break;
            }
            let mut body = Vec::new();
            // An error status is sent *instead* of a body, so there is nothing
            // to read before the next command's status list.
            if status.iter().all(|line| line != "status=error") {
                loop {
                    match pktline::read(&mut wire).expect("body") {
                        Packet::Data(bytes) => body.extend_from_slice(&bytes),
                        Packet::Flush => break,
                        Packet::Eof => break,
                    }
                }
            }
            let trailer = if status.iter().any(|line| line == "status=error") {
                Vec::new()
            } else {
                pktline::read_text_list(&mut wire)
                    .expect("trailer")
                    .unwrap_or_default()
            };
            answers.push(Answer {
                status,
                body,
                trailer,
            });
        }
        answers
    }

    /// Version 2, and only the capabilities this filter actually honours —
    /// `delay` is offered by git and deliberately not taken.
    #[test]
    fn the_handshake_agrees_on_version_two_and_refuses_delay() {
        let dir = tempfile::tempdir().expect("tempdir");
        let mut out = Vec::new();
        run_process(dir.path(), &mut hello().as_slice(), &mut out).expect("handshake");

        let mut wire = out.as_slice();
        assert_eq!(
            pktline::read_text_list(&mut wire).expect("hello"),
            Some(vec!["git-filter-server".to_owned(), "version=2".to_owned()])
        );
        assert_eq!(
            pktline::read_text_list(&mut wire).expect("capabilities"),
            Some(vec![
                "capability=clean".to_owned(),
                "capability=smudge".to_owned()
            ])
        );
    }

    /// The regression this whole module was rewritten for. An object the store
    /// does not hold must come back as its **pointer** — never as the zero
    /// bytes a dying `git-lfs filter-process` leaves behind, and never as a
    /// failure that takes the rest of the checkout with it.
    #[test]
    fn a_smudge_without_the_object_answers_with_the_pointer_not_emptiness() {
        let dir = tempfile::tempdir().expect("tempdir");
        let store = LfsStore::in_git_dir(dir.path().join(".git"));
        store.ensure_layout().expect("layout");
        let pointer = Pointer::new(
            "9f86d081884c7d659a2feaa0c55ad015a3bf4f1b2b0b822cd15d6c15b0f00a08".to_owned(),
            4,
        )
        .render();

        let mut script = hello();
        script.extend(request("smudge", "a/big.mov", pointer.as_bytes()));
        let mut out = Vec::new();
        run_process(dir.path(), &mut script.as_slice(), &mut out).expect("served");

        let answers = replies(&out);
        assert_eq!(answers.len(), 1);
        assert_eq!(answers[0].status, vec!["status=success".to_owned()]);
        assert_eq!(answers[0].body, pointer.as_bytes());
        assert!(!answers[0].body.is_empty(), "never zero bytes");
    }

    #[test]
    fn a_smudge_serves_the_object_the_store_holds() {
        let dir = tempfile::tempdir().expect("tempdir");
        let store = LfsStore::in_git_dir(dir.path().join(".git"));
        store.ensure_layout().expect("layout");
        // Several packets' worth, so the framing loop is exercised.
        let payload = vec![7u8; pktline::MAX_DATA * 2 + 11];
        let (oid, size) = store
            .insert_streaming(payload.as_slice())
            .expect("insert the object");
        let pointer = Pointer::new(oid, size).render();

        let mut script = hello();
        script.extend(request("smudge", "a/big.mov", pointer.as_bytes()));
        let mut out = Vec::new();
        run_process(dir.path(), &mut script.as_slice(), &mut out).expect("served");

        let answers = replies(&out);
        assert_eq!(answers[0].status, vec!["status=success".to_owned()]);
        assert_eq!(answers[0].body, payload);
        assert!(answers[0].trailer.is_empty(), "no mid-stream failure");
    }

    #[test]
    fn a_clean_over_the_protocol_stores_the_object_and_answers_with_its_pointer() {
        let dir = tempfile::tempdir().expect("tempdir");
        let store = LfsStore::in_git_dir(dir.path().join(".git"));
        store.ensure_layout().expect("layout");
        let payload = vec![3u8; pktline::MAX_DATA + 5];

        let mut script = hello();
        script.extend(request("clean", "a/big.mov", &payload));
        let mut out = Vec::new();
        run_process(dir.path(), &mut script.as_slice(), &mut out).expect("served");

        let answers = replies(&out);
        assert_eq!(answers[0].status, vec!["status=success".to_owned()]);
        let pointer = Pointer::parse(&answers[0].body).expect("a pointer came back");
        assert_eq!(pointer.size, payload.len() as u64);
        assert!(
            store.contains(&pointer.oid, pointer.size),
            "the object is in the store before the pointer naming it was emitted"
        );
    }

    /// The property that makes `required=false` safe again: one path's refusal
    /// is one path's refusal. git's own failure mode — "is not available
    /// anymore although not all paths have been filtered" — is a *process* that
    /// died, and every path after it is what gets emptied. So the test that
    /// matters is not that the bad path fails, it is that the next one does not.
    #[test]
    fn a_refused_path_does_not_take_the_rest_of_the_checkout_with_it() {
        let dir = tempfile::tempdir().expect("tempdir");
        let store = LfsStore::in_git_dir(dir.path().join(".git"));
        store.ensure_layout().expect("layout");
        let payload = b"the next path still works".to_vec();
        let (oid, size) = store
            .insert_streaming(payload.as_slice())
            .expect("insert the object");
        let good = Pointer::new(oid, size).render();

        let mut script = hello();
        // A command we never advertised — the shape of an argv a future git
        // sends and this build does not implement.
        script.extend(request("transmogrify", "a/one.mov", b"whatever"));
        script.extend(request("smudge", "a/two.mov", good.as_bytes()));
        let mut out = Vec::new();
        run_process(dir.path(), &mut script.as_slice(), &mut out).expect("the process survived");

        let answers = replies(&out);
        assert_eq!(answers.len(), 2, "both paths were answered");
        assert_eq!(answers[0].status, vec!["status=error".to_owned()]);
        assert_eq!(answers[1].status, vec!["status=success".to_owned()]);
        assert_eq!(answers[1].body, payload, "the good path is intact");
    }

    /// A smudge stops reading the moment it has the pointer, which leaves the
    /// rest of that request's packets in the pipe. Anything left there is read
    /// as the *next* command's metadata and the conversation never recovers —
    /// so the drain is load-bearing, and this proves it by serving a second
    /// path after a request that carried trailing bytes.
    #[test]
    fn a_request_the_filter_stopped_reading_is_still_drained() {
        let dir = tempfile::tempdir().expect("tempdir");
        let store = LfsStore::in_git_dir(dir.path().join(".git"));
        store.ensure_layout().expect("layout");
        let (oid, size) = store
            .insert_streaming(b"served".as_slice())
            .expect("insert");
        let pointer = Pointer::new(oid, size).render();

        // A pointer followed by bytes the smudge will never look at.
        let mut padded = pointer.as_bytes().to_vec();
        padded.extend_from_slice(&vec![0u8; 5_000]);

        let mut script = hello();
        script.extend(request("smudge", "a/one.mov", &padded));
        script.extend(request("smudge", "a/two.mov", pointer.as_bytes()));
        let mut out = Vec::new();
        run_process(dir.path(), &mut script.as_slice(), &mut out).expect("served");

        let answers = replies(&out);
        assert_eq!(answers.len(), 2, "the second command was still understood");
        assert_eq!(answers[1].body, b"served");
    }

    #[test]
    fn a_clean_stores_the_object_and_emits_the_pointer_naming_it() {
        let dir = tempfile::tempdir().expect("tempdir");
        let repo = dir.path();
        // Several chunks' worth, so this exercises the streaming loop rather
        // than one read.
        let payload = vec![9u8; 300_000];

        let mut out = Vec::new();
        run(repo, Direction::Clean, &mut payload.as_slice(), &mut out).expect("clean");

        let pointer = Pointer::parse(&out).expect("the output is a pointer");
        assert_eq!(pointer.size, payload.len() as u64);
        let store = LfsStore::in_git_dir(repo.join(".git"));
        assert!(
            store.contains(&pointer.oid, pointer.size),
            "the object is in the store before the pointer naming it is emitted"
        );
        assert_eq!(
            std::fs::read(store.object_path(&pointer.oid)).expect("read back"),
            payload
        );
    }

    /// The one that costs an object rather than a round trip.
    ///
    /// A `clean` whose input is already a pointer must re-emit it. Pointer text
    /// in the worktree is a state this module itself produces — `smudge` leaves
    /// it for every object the store does not hold, and `LfsMode::PointerOnly`
    /// leaves it there permanently — so `git add`, `git commit -a` and a
    /// racily-clean re-read all reach here with it. Hashing it emits a pointer
    /// naming the pointer: the path reads MODIFIED, and a commit that takes the
    /// clean replaces every peer's only reference to the real object with 130
    /// bytes of text.
    #[test]
    fn a_clean_of_a_pointer_re_emits_it_rather_than_naming_the_pointer() {
        let dir = tempfile::tempdir().expect("tempdir");
        let repo = dir.path();
        let store = LfsStore::in_git_dir(repo.join(".git"));

        // The fixture is `smudge`'s OWN output for an object the store does not
        // hold, rather than a hand-written pointer: these are the exact bytes the
        // filter leaves in the worktree, which is what makes the round trip the
        // thing under test.
        let published = Pointer::new("b".repeat(64), 4_096).render().into_bytes();
        let mut worktree = Vec::new();
        run(
            repo,
            Direction::Smudge,
            &mut published.as_slice(),
            &mut worktree,
        )
        .expect("smudge an object the store does not hold");
        assert_eq!(
            worktree, published,
            "which is where the pointer text comes from"
        );

        let before = stored_objects(&store);
        let mut out = Vec::new();
        run(repo, Direction::Clean, &mut worktree.as_slice(), &mut out).expect("clean");

        assert_eq!(
            out, published,
            "byte-for-byte, so the index entry is unchanged and the path reads \
             clean; anything else is a phantom modification at best and the loss \
             of the object at worst"
        );
        assert_eq!(
            stored_objects(&store),
            before,
            "and nothing was stored — hashing pointer text creates an object \
             nobody will ever ask for"
        );
    }

    /// The other half of the prefix read: content that merely *starts* like a
    /// pointer must keep every byte.
    #[test]
    fn a_clean_keeps_every_byte_of_content_that_only_begins_like_a_pointer() {
        let dir = tempfile::tempdir().expect("tempdir");
        let repo = dir.path();
        let store = LfsStore::in_git_dir(repo.join(".git"));

        // A real pointer's bytes and then a great deal more. The prefix read stops
        // one byte past the pointer ceiling, so a version that forgot to chain
        // `head` back on would store the tail alone — and silently, because the
        // pointer it emitted would describe those bytes perfectly.
        let mut long = Pointer::new("c".repeat(64), 1).render().into_bytes();
        long.resize(long.len() + MAX_POINTER_BYTES * 3, b'z');

        // And the boundary: one byte more than a pointer is content, and this one
        // fits inside `head`, so nothing is chained on at all.
        let mut barely = Pointer::new("d".repeat(64), 1).render().into_bytes();
        barely.push(b'!');

        for payload in [long, barely] {
            let mut out = Vec::new();
            run(repo, Direction::Clean, &mut payload.as_slice(), &mut out).expect("clean");
            let pointer = Pointer::parse(&out).expect("content, so a fresh pointer");
            assert_eq!(
                pointer.size,
                payload.len() as u64,
                "the pointer must account for all {} bytes",
                payload.len()
            );
            assert_eq!(
                std::fs::read(store.object_path(&pointer.oid)).expect("read back"),
                payload,
                "and the store must hold all of them"
            );
        }
    }

    #[test]
    fn a_smudge_returns_the_content_the_pointer_names() {
        let dir = tempfile::tempdir().expect("tempdir");
        let repo = dir.path();
        let payload = vec![4u8; 200_000];

        // Clean first, so the store holds the object a smudge has to find.
        let mut pointer_bytes = Vec::new();
        run(
            repo,
            Direction::Clean,
            &mut payload.as_slice(),
            &mut pointer_bytes,
        )
        .expect("clean");

        let mut restored = Vec::new();
        run(
            repo,
            Direction::Smudge,
            &mut pointer_bytes.as_slice(),
            &mut restored,
        )
        .expect("smudge");
        assert_eq!(restored, payload, "the round trip is byte-exact");
    }

    /// The tolerance that keeps a partial fetch usable. git-lfs does the same:
    /// a checkout must not fail because an object has not been downloaded yet.
    #[test]
    fn a_smudge_passes_through_what_it_cannot_resolve() {
        let dir = tempfile::tempdir().expect("tempdir");
        let repo = dir.path();

        // Ordinary content, which is what a smudge sees for any non-LFS path
        // that happens to be routed through the filter.
        let plain = b"this is not a pointer, it is a note\n";
        let mut out = Vec::new();
        run(repo, Direction::Smudge, &mut &plain[..], &mut out).expect("smudge");
        assert_eq!(out, plain);

        // A well-formed pointer for an object the store does not hold. Passing
        // it through leaves the pointer text in the worktree, which is
        // recoverable; failing here would break the whole checkout.
        let orphan = Pointer::new("b".repeat(64), 4_096).render().into_bytes();
        let mut out = Vec::new();
        run(repo, Direction::Smudge, &mut orphan.as_slice(), &mut out).expect("smudge");
        assert_eq!(out, orphan);
    }

    /// A file larger than the pointer ceiling cannot be a pointer, and the
    /// prefix read must not swallow the rest of it.
    #[test]
    fn a_smudge_of_something_far_too_large_streams_all_of_it() {
        let dir = tempfile::tempdir().expect("tempdir");
        let big = vec![7u8; MAX_POINTER_BYTES * 3];
        let mut out = Vec::new();
        run(dir.path(), Direction::Smudge, &mut big.as_slice(), &mut out).expect("smudge");
        assert_eq!(
            out.len(),
            big.len(),
            "the bounded prefix read must not truncate the pass-through"
        );
        assert_eq!(out, big);
    }
}
