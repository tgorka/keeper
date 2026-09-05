//! A push keeper does itself (Story 66.5, AD-202).
//!
//! The phone has no `git` binary and may not spawn one (iOS denies
//! `posix_spawn` to third-party apps), and gitoxide has no push and will not
//! grow one (see [`super::cli`]). What remains is the wire protocol itself,
//! which is small: `git-receive-pack` over smart HTTP is one `GET` that
//! advertises refs and capabilities, and one `POST` carrying the ref updates
//! and a pack of the objects the remote lacks. This module speaks exactly that
//! — protocol v0, `report-status`, `side-band-64k`, one update per call — and
//! nothing more. The desktop keeps `git push` (AD-41); this is the phone's
//! engine.
//!
//! # What is refused before a byte is sent
//!
//! AD-50 forbids force-pushing a lane. `git push` enforces that server-side
//! (`non-fast-forward`), and the desktop relies on it. Here the guard is
//! **client-side**: the advertisement names the remote's tip, and unless that
//! tip is an ancestor of the local one — or the remote has no such ref yet —
//! the push is refused as [`SyncError::Diverged`] with no `POST` at all. A
//! remote tip this repository has never fetched is refused the same way: there
//! is no way to prove it is an ancestor, so it is not one.
//!
//! # The pack
//!
//! Every object reachable from the local tip and not from the remote's, found
//! the way `git pack-objects --revs` finds them: the commits are walked with
//! the remote tip hidden, and each commit contributes its tree entries that
//! differ from its parents' (`gix-pack`'s `TreeAdditionsComparedToAncestor`).
//! Objects are written as plain base entries — no deltas, no thin pack — which
//! costs bandwidth on a large history and buys a pack that any
//! `git-receive-pack` accepts without negotiation. A note or a captured file on
//! a phone is a handful of objects; the trade is right for the caller this
//! serves.
//!
//! **LFS pointers are blobs here.** A pointer file is committed as its pointer
//! text and travels in the pack as that text; nothing in this module talks to
//! the LFS endpoint. That is the same sequencing the desktop's `do_push` keeps:
//! the objects the pointers name must already be on the remote — the caller
//! drains `lfs::stage`'s upload queue first and refuses to publish while any
//! upload is outstanding (`SyncError::LfsUploadPending`) — or a peer clones a
//! tree of pointer text with no error anywhere.
//!
//! # Credentials and diagnostics
//!
//! The credential is sent as HTTP Basic through the same sensitive header path
//! the LFS client uses (AD-53), and the request URL never carries userinfo: a
//! URL that arrives with `user:token@` is stripped before it is used and the
//! `credential` argument is the only secret this push may present. Every
//! diagnostic — the server's own `ng` and side-band lines included — passes
//! through [`cli::scrub_userinfo`] before it becomes an error (NFR-26).

use std::{collections::HashSet, path::Path, sync::atomic::AtomicBool};

use base64::engine::general_purpose::STANDARD as BASE64;
use base64::Engine as _;
use gix::{hash::ObjectId, objs::FindExt as _};
use gix_pack::data::output::{
    count::{objects::ObjectExpansion, objects_unthreaded},
    Count, Entry,
};
use reqwest::header::{HeaderMap, HeaderValue, ACCEPT, AUTHORIZATION, CONTENT_TYPE};
use url::Url;

use crate::{
    error::{Result, SyncError},
    git::{
        cli::{self, scrub_userinfo},
        fetch::Credential,
    },
    lfs::{batch, pktline},
};

/// The service both requests name.
const SERVICE: &str = "git-receive-pack";
/// Media type of the `POST` body.
const REQUEST_TYPE: &str = "application/x-git-receive-pack-request";
/// Media type the `POST` answer is expected in.
const RESULT_TYPE: &str = "application/x-git-receive-pack-result";
/// The advertisement's first packet, which the smart HTTP protocol requires a
/// client to check before reading refs.
const SERVICE_LINE: &str = "# service=git-receive-pack";

/// Ceiling on the bytes read back from either request.
///
/// An advertisement lists refs and a result carries a report plus whatever a
/// hook printed; both are small. A server streaming megabytes at a push is
/// broken, and this crate's promise is bounded allocation whatever a server
/// sends (NFR-23).
const RESPONSE_CAP: usize = 4 * 1024 * 1024;

/// How many hex digits an id is shown with in a sentence.
const SHORT_ID: usize = 7;

/// What a push did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PushReport {
    /// The remote's ref moved. `false` means it already pointed at [`new`]
    /// and no request beyond the advertisement was made.
    ///
    /// [`new`]: Self::new
    pub updated: bool,
    /// Where the remote's ref was before the push; `None` for a ref the remote
    /// did not have.
    pub remote_old: Option<ObjectId>,
    /// The local tip that was pushed.
    pub new: ObjectId,
    /// Objects written into the pack.
    pub objects: usize,
    /// Every non-empty line the server sent back — its `report-status` lines
    /// and its side-band progress — scrubbed of userinfo. On a refusal the
    /// same lines make the error's message; kept here so a caller can show
    /// what a hook said even when the push succeeded.
    pub server_lines: Vec<String>,
}

/// Push local `refs/heads/<lane>` to the same ref at `remote_url`.
///
/// Never a force (AD-50): refused client-side, before any bytes are sent,
/// unless the remote's tip is an ancestor of the local one or absent. See the
/// module docs for the pack, the credential and the LFS precondition the
/// caller owes.
///
/// `client` is the crate's shared [`crate::http::client`]. The object walk and
/// the pack are built on a blocking thread; the two HTTP round trips are
/// awaited. Blocking callers should drive this from the runtime rather than
/// wrapping it in `spawn_blocking` themselves.
pub async fn push(
    client: &reqwest::Client,
    repo: &Path,
    remote_url: &str,
    lane: &str,
    credential: Option<&Credential>,
) -> Result<PushReport> {
    let base = parse_remote(remote_url)?;
    let host = base.host_str().unwrap_or("unknown host").to_owned();
    let auth = credential
        .map(|credential| basic_header(credential, &host))
        .transpose()?;
    let refname = format!("refs/heads/{lane}");

    let advertisement = discover(client, &base, auth.as_ref(), &host).await?;
    let remote_old = advertisement.tip_of(&refname);

    let repo_path = repo.to_path_buf();
    let lane_owned = lane.to_owned();
    let prepared =
        tokio::task::spawn_blocking(move || prepare(&repo_path, &lane_owned, remote_old))
            .await
            .map_err(|err| SyncError::Journal(format!("push task failed: {err}")))??;

    let (new, pack, objects) = match prepared {
        Prepared::UpToDate { new } => {
            return Ok(PushReport {
                updated: false,
                remote_old,
                new,
                objects: 0,
                server_lines: Vec::new(),
            });
        }
        Prepared::Send { new, pack, objects } => (new, pack, objects),
    };

    let old = remote_old.unwrap_or_else(|| ObjectId::null(new.kind()));
    let capabilities = advertisement.request_capabilities();
    let body = request_body(&old, &new, &refname, &capabilities, &pack)?;

    let mut headers = HeaderMap::with_capacity(3);
    headers.insert(CONTENT_TYPE, HeaderValue::from_static(REQUEST_TYPE));
    headers.insert(ACCEPT, HeaderValue::from_static(RESULT_TYPE));
    if let Some(auth) = &auth {
        headers.insert(AUTHORIZATION, auth.clone());
    }
    let mut url = base.clone();
    append_path(&mut url, SERVICE);
    let response = client
        .post(url)
        .headers(headers)
        .body(body)
        .send()
        .await
        .map_err(|err| network(&host, &err))?;
    check_status(response.status().as_u16(), &host)?;
    let bytes = bounded_bytes(response, &host).await?;

    let result = if advertisement.has("side-band-64k") {
        demux_side_band(&bytes)?
    } else {
        Demuxed {
            report: bytes,
            ..Demuxed::default()
        }
    };
    let report = if advertisement.has("report-status") {
        parse_report(&result.report)?
    } else {
        // A server without `report-status` says nothing on success and closes
        // the stream; git treats that as accepted. Nothing older than 1.6
        // omits it, and refusing to talk to such a server would be a wall for
        // no reason.
        Report::default()
    };

    let mut server_lines: Vec<String> = Vec::new();
    server_lines.extend(report.lines.iter().map(|line| scrub_userinfo(line)));
    server_lines.extend(result.fatal.iter().map(|line| scrub_userinfo(line)));
    server_lines.extend(result.progress.iter().map(|line| scrub_userinfo(line)));

    let refused = !report.refused.is_empty() || !result.fatal.is_empty();
    if refused || report.unpack_failed.is_some() {
        // The refusal itself first, then whatever the hook printed: line one
        // is the verdict, the rest is the reason, and `one_line` joins them.
        let text: String = report
            .unpack_failed
            .iter()
            .chain(&report.refused)
            .chain(&result.fatal)
            .chain(&result.progress)
            .map(|line| scrub_userinfo(line))
            .collect::<Vec<_>>()
            .join("\n");
        let label = cli::repo_label(repo);
        return Err(
            cli::classify_message(&text, &label, Some(&host), &[]).unwrap_or_else(|| {
                SyncError::Git(format!("{host} refused the push: {}", cli::one_line(&text)))
            }),
        );
    }
    if advertisement.has("report-status") && !report.accepted.iter().any(|name| name == &refname) {
        // A report that names neither `ok` nor `ng` for the ref we sent is a
        // server that did not understand the request; saying "pushed" would
        // be a lie the next fetch exposes.
        return Err(SyncError::Git(format!(
            "{host} answered the push without a status for {refname}: {}",
            cli::one_line(&scrub_userinfo(&server_lines.join("\n")))
        )));
    }

    Ok(PushReport {
        updated: true,
        remote_old,
        new,
        objects,
        server_lines,
    })
}

/// The outcome of the local half: nothing to send, or a pack.
enum Prepared {
    UpToDate {
        new: ObjectId,
    },
    Send {
        new: ObjectId,
        pack: Vec<u8>,
        objects: usize,
    },
}

/// Resolve the local tip, apply the fast-forward guard and build the pack.
///
/// Blocking: object reads and zlib. Runs on a `spawn_blocking` thread.
fn prepare(repo_path: &Path, lane: &str, remote_old: Option<ObjectId>) -> Result<Prepared> {
    let repo = super::repo::open_read_only(repo_path, false)?;
    let refname = format!("refs/heads/{lane}");
    let new = repo
        .find_reference(refname.as_str())
        .map_err(|err| {
            SyncError::Git(format!(
                "no local branch {lane} to push: {}",
                super::fetch::flatten(&err)
            ))
        })?
        .into_fully_peeled_id()
        .map_err(|err| SyncError::Git(format!("could not peel {refname}: {err}")))?
        .detach();

    if let Some(old) = remote_old {
        if old == new {
            return Ok(Prepared::UpToDate { new });
        }
        fast_forward_guard(&repo, repo_path, lane, old, new)?;
    }

    let (pack, objects) = build_pack(&repo, new, remote_old)?;
    Ok(Prepared::Send { new, pack, objects })
}

/// AD-50 at the client: refuse unless `old` is an ancestor of `new`.
///
/// The sentence names both ids so the reader can `git log` either side, and
/// says which of the two failure shapes it is — a tip never fetched, or a
/// history that was rewritten — because the remedies differ (fetch first vs.
/// reset the phone's copy).
fn fast_forward_guard(
    repo: &gix::Repository,
    repo_path: &Path,
    lane: &str,
    old: ObjectId,
    new: ObjectId,
) -> Result<()> {
    let label = cli::repo_label(repo_path);
    if !repo.has_object(old) {
        return Err(SyncError::Diverged {
            profile: label,
            reason: format!(
                "the remote's {lane} is at {}, which this copy has never fetched; local {lane} is at {} — fetch first, keeper never force-pushes a lane",
                old.to_hex_with_len(SHORT_ID),
                new.to_hex_with_len(SHORT_ID),
            ),
        });
    }
    let fast_forward = repo
        .merge_base(old, new)
        .map(|base| base.detach() == old)
        .unwrap_or(false);
    if fast_forward {
        return Ok(());
    }
    Err(SyncError::Diverged {
        profile: label,
        reason: format!(
            "non-fast-forward: the remote's {lane} is at {} and local {lane} at {} does not descend from it — keeper never force-pushes a lane",
            old.to_hex_with_len(SHORT_ID),
            new.to_hex_with_len(SHORT_ID),
        ),
    })
}

/// Write a v2 pack of every object reachable from `new` and not from `old`.
///
/// Returns the pack bytes — header, base entries, SHA-1 trailer — and the
/// object count. See the module docs for why there are no deltas.
fn build_pack(
    repo: &gix::Repository,
    new: ObjectId,
    old: Option<ObjectId>,
) -> Result<(Vec<u8>, usize)> {
    let git = |what: &str, err: &dyn std::error::Error| {
        SyncError::Git(format!("{what}: {}", super::fetch::flatten(err)))
    };

    // The commits the remote lacks, newest first, and every parent that sits
    // on the far side of the boundary. `gix-pack`'s ancestor comparison counts
    // a commit's parents and their trees as well — it needs them to diff — and
    // the remote already holds those, so they are dropped from the pack below.
    let mut wanted: Vec<ObjectId> = Vec::new();
    let mut boundary: HashSet<ObjectId> = HashSet::new();
    let mut parent_pairs: Vec<(ObjectId, ObjectId)> = Vec::new();
    let walk = repo
        .rev_walk([new])
        .with_hidden(old)
        .all()
        .map_err(|err| git("could not walk the commits to push", &err))?;
    for info in walk {
        let info = info.map_err(|err| git("could not walk the commits to push", &err))?;
        for parent in &info.parent_ids {
            parent_pairs.push((info.id, *parent));
        }
        wanted.push(info.id);
    }
    let new_set: HashSet<ObjectId> = wanted.iter().copied().collect();
    for (_, parent) in parent_pairs {
        if new_set.contains(&parent) {
            continue;
        }
        boundary.insert(parent);
        if let Ok(commit) = repo.find_commit(parent) {
            if let Ok(tree) = commit.tree_id() {
                boundary.insert(tree.detach());
            }
        }
    }

    let mut input = wanted
        .iter()
        .map(|id| Ok::<_, Box<dyn std::error::Error + Send + Sync + 'static>>(*id));
    let (counts, _) = objects_unthreaded(
        &*repo.objects,
        &mut input,
        &gix::progress::Discard,
        &AtomicBool::new(false),
        ObjectExpansion::TreeAdditionsComparedToAncestor,
    )
    .map_err(|err| git("could not count the objects to push", &err))?;
    let counts: Vec<Count> = counts
        .into_iter()
        .filter(|count| !boundary.contains(&count.id))
        .collect();

    let mut buf = Vec::new();
    let mut entries = Vec::with_capacity(counts.len());
    for count in &counts {
        let data = repo
            .objects
            .find(&count.id, &mut buf)
            .map_err(|err| git("could not read an object to push", &err))?;
        entries.push(
            Entry::from_data(count, &data, gix::zlib::Compression::DEFAULT)
                .map_err(|err| git("could not compress an object to push", &err))?,
        );
    }

    let objects = entries.len();
    let total = u32::try_from(objects)
        .map_err(|_| SyncError::Git(format!("{objects} objects is more than one pack can hold")))?;
    let mut writer = gix_pack::data::output::bytes::FromEntriesIter::new(
        std::iter::once(Ok::<_, std::io::Error>(entries)),
        Vec::with_capacity(64 * 1024),
        total,
        gix_pack::data::Version::V2,
        new.kind(),
    );
    for written in writer.by_ref() {
        written.map_err(|err| git("could not write the pack", &err))?;
    }
    Ok((writer.into_write(), objects))
}

/// The remote URL without userinfo and without a trailing slash.
fn parse_remote(raw: &str) -> Result<Url> {
    let mut url = Url::parse(raw.trim()).map_err(|_| {
        SyncError::Config(format!(
            "{} is not an HTTP remote keeper can push to",
            scrub_userinfo(raw)
        ))
    })?;
    if !matches!(url.scheme(), "http" | "https") {
        return Err(SyncError::Config(format!(
            "{} is not an HTTP remote keeper can push to",
            scrub_userinfo(raw)
        )));
    }
    // The credential argument is the only secret this push presents (AD-53).
    // `set_username` only fails for URLs that cannot carry one, which the
    // scheme check above rules out.
    let _ = url.set_username("");
    let _ = url.set_password(None);
    url.set_fragment(None);
    url.set_query(None);
    let trimmed = url.path().trim_end_matches('/').to_owned();
    url.set_path(&trimmed);
    Ok(url)
}

/// `base` + `/segment`, keeping `base`'s own path.
fn append_path(url: &mut Url, segment: &str) {
    let mut path = url.path().to_owned();
    path.push('/');
    path.push_str(segment);
    url.set_path(&path);
}

/// The `Authorization` value for `credential`: RFC 7617 Basic, marked
/// sensitive through the same door the LFS client uses.
fn basic_header(credential: &Credential, host: &str) -> Result<HeaderValue> {
    let raw = format!(
        "Basic {}",
        BASE64.encode(format!("{}:{}", credential.username, credential.secret))
    );
    batch::sensitive_auth(&raw, host)
}

/// A transport failure, described without the URL (which reqwest may embed).
fn network(host: &str, err: &reqwest::Error) -> SyncError {
    SyncError::Network {
        host: host.to_owned(),
        reason: batch::transport_reason(err).to_owned(),
    }
}

/// The HTTP status table for both requests.
fn check_status(status: u16, host: &str) -> Result<()> {
    match status {
        200..=299 => Ok(()),
        401 => Err(SyncError::Auth {
            host: host.to_owned(),
        }),
        403 => Err(SyncError::Forbidden {
            host: host.to_owned(),
        }),
        // Forges answer 404 both for a repository that does not exist and for
        // one these credentials may not see; the wire does not say which.
        404 => Err(SyncError::Config(format!(
            "{host} has no repository at that URL, or it is not visible to these credentials"
        ))),
        500..=599 => Err(SyncError::Network {
            host: host.to_owned(),
            reason: format!("server error {status}"),
        }),
        other => Err(SyncError::Config(format!(
            "{host} answered {SERVICE} with an unexpected status {other}"
        ))),
    }
}

/// Read a response body up to [`RESPONSE_CAP`].
async fn bounded_bytes(mut response: reqwest::Response, host: &str) -> Result<Vec<u8>> {
    let mut out = Vec::new();
    loop {
        match response.chunk().await {
            Ok(Some(chunk)) => {
                if out.len() + chunk.len() > RESPONSE_CAP {
                    return Err(SyncError::Git(format!(
                        "{host} sent more than {RESPONSE_CAP} bytes answering {SERVICE}"
                    )));
                }
                out.extend_from_slice(&chunk);
            }
            Ok(None) => return Ok(out),
            Err(err) => return Err(network(host, &err)),
        }
    }
}

/// `GET …/info/refs?service=git-receive-pack`, parsed.
async fn discover(
    client: &reqwest::Client,
    base: &Url,
    auth: Option<&HeaderValue>,
    host: &str,
) -> Result<Advertisement> {
    let mut url = base.clone();
    append_path(&mut url, "info/refs");
    url.set_query(Some(&format!("service={SERVICE}")));
    let mut headers = HeaderMap::with_capacity(2);
    headers.insert(ACCEPT, HeaderValue::from_static("*/*"));
    if let Some(auth) = auth {
        headers.insert(AUTHORIZATION, auth.clone());
    }
    let response = client
        .get(url)
        .headers(headers)
        .send()
        .await
        .map_err(|err| network(host, &err))?;
    check_status(response.status().as_u16(), host)?;
    let bytes = bounded_bytes(response, host).await?;
    parse_advertisement(&bytes)
        .map_err(|reason| SyncError::Git(format!("{host} did not advertise {SERVICE}: {reason}")))
}

/// What the remote advertised.
#[derive(Debug, Default, PartialEq, Eq)]
struct Advertisement {
    /// `(refname, id)` for every ref the remote listed.
    refs: Vec<(String, ObjectId)>,
    /// The server's capability words, `agent=…` included.
    capabilities: Vec<String>,
}

impl Advertisement {
    fn tip_of(&self, refname: &str) -> Option<ObjectId> {
        self.refs
            .iter()
            .find(|(name, _)| name == refname)
            .map(|(_, id)| *id)
    }

    fn has(&self, capability: &str) -> bool {
        self.capabilities.iter().any(|word| word == capability)
    }

    /// The capabilities this client asks for: the two it understands, each
    /// only if the server offered it, plus its own `agent`.
    fn request_capabilities(&self) -> Vec<String> {
        let mut out = Vec::with_capacity(3);
        for wanted in ["report-status", "side-band-64k"] {
            if self.has(wanted) {
                out.push(wanted.to_owned());
            }
        }
        out.push(format!("agent={}", crate::AGENT));
        out
    }
}

/// Parse the smart HTTP advertisement: the service line, a flush, then refs.
///
/// The first ref line carries `\0` and the capability list; an empty
/// repository sends `<null-id> capabilities^{}\0<caps>` in its place. Pure, so
/// the wire shapes are tested without a server.
fn parse_advertisement(bytes: &[u8]) -> std::result::Result<Advertisement, String> {
    let mut input = bytes;
    match pktline::read(&mut input).map_err(|err| err.to_string())? {
        pktline::Packet::Data(line)
            if line.strip_suffix(b"\n").unwrap_or(&line) == SERVICE_LINE.as_bytes() => {}
        other => return Err(format!("expected `{SERVICE_LINE}`, got {other:?}")),
    }
    match pktline::read(&mut input).map_err(|err| err.to_string())? {
        pktline::Packet::Flush => {}
        other => {
            return Err(format!(
                "expected a flush after the service line, got {other:?}"
            ))
        }
    }

    let mut advertisement = Advertisement::default();
    let mut first = true;
    loop {
        let line = match pktline::read(&mut input).map_err(|err| err.to_string())? {
            pktline::Packet::Flush | pktline::Packet::Eof => break,
            pktline::Packet::Data(line) => line,
        };
        let line = line.strip_suffix(b"\n").unwrap_or(&line);
        let (entry, capabilities) = match line.iter().position(|byte| *byte == 0) {
            Some(nul) => (&line[..nul], Some(&line[nul + 1..])),
            None => (line, None),
        };
        if first {
            let capabilities = capabilities.ok_or("the first ref line carries no capabilities")?;
            advertisement.capabilities = String::from_utf8_lossy(capabilities)
                .split(' ')
                .filter(|word| !word.is_empty())
                .map(str::to_owned)
                .collect();
            first = false;
        }
        let entry = std::str::from_utf8(entry).map_err(|_| "a ref line is not UTF-8")?;
        let (hex, name) = entry
            .split_once(' ')
            .ok_or_else(|| format!("malformed ref line {entry:?}"))?;
        if name == "capabilities^{}" {
            continue;
        }
        let id = ObjectId::from_hex(hex.as_bytes())
            .map_err(|err| format!("bad id in {entry:?}: {err}"))?;
        advertisement.refs.push((name.to_owned(), id));
    }
    if first {
        return Err("the advertisement listed no refs and no capabilities".into());
    }
    Ok(advertisement)
}

/// The `POST` body: one update command with capabilities, a flush, the pack.
fn request_body(
    old: &ObjectId,
    new: &ObjectId,
    refname: &str,
    capabilities: &[String],
    pack: &[u8],
) -> Result<Vec<u8>> {
    let mut body = Vec::with_capacity(128 + pack.len());
    let command = format!("{old} {new} {refname}\0{}\n", capabilities.join(" "));
    pktline::write_data(&mut body, command.as_bytes())?;
    pktline::write_flush(&mut body)?;
    body.extend_from_slice(pack);
    Ok(body)
}

/// A side-band-64k stream, split by band.
#[derive(Debug, Default, PartialEq, Eq)]
struct Demuxed {
    /// Band 1: the `report-status` pkt-lines, concatenated.
    report: Vec<u8>,
    /// Band 2: progress and hook stderr, as non-empty lines.
    progress: Vec<String>,
    /// Band 3: fatal messages, as non-empty lines.
    fatal: Vec<String>,
}

/// Split a side-band-64k response into its bands.
fn demux_side_band(bytes: &[u8]) -> Result<Demuxed> {
    let mut input = bytes;
    let mut out = Demuxed::default();
    let mut progress = Vec::new();
    let mut fatal = Vec::new();
    loop {
        let payload = match pktline::read(&mut input)? {
            pktline::Packet::Flush | pktline::Packet::Eof => break,
            pktline::Packet::Data(payload) => payload,
        };
        let Some((band, rest)) = payload.split_first() else {
            continue;
        };
        match band {
            1 => out.report.extend_from_slice(rest),
            2 => progress.extend_from_slice(rest),
            3 => fatal.extend_from_slice(rest),
            other => {
                return Err(SyncError::Git(format!(
                    "unknown side-band {other} in the {SERVICE} result"
                )))
            }
        }
    }
    out.progress = text_lines(&progress);
    out.fatal = text_lines(&fatal);
    Ok(out)
}

/// Non-empty, trimmed lines of a band, splitting on `\r` as well: progress
/// meters redraw with carriage returns and would otherwise fold into one.
fn text_lines(bytes: &[u8]) -> Vec<String> {
    String::from_utf8_lossy(bytes)
        .split(['\n', '\r'])
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(str::to_owned)
        .collect()
}

/// The `report-status` lines, sorted by what they mean.
#[derive(Debug, Default, PartialEq, Eq)]
struct Report {
    /// Every line, verbatim.
    lines: Vec<String>,
    /// `unpack <reason>` when the reason was not `ok`.
    unpack_failed: Option<String>,
    /// Refs the server accepted.
    accepted: Vec<String>,
    /// `ng <ref> <reason>` lines, verbatim.
    refused: Vec<String>,
}

/// Parse the `report-status` pkt-lines.
fn parse_report(bytes: &[u8]) -> Result<Report> {
    let mut input = bytes;
    let mut report = Report::default();
    loop {
        let line = match pktline::read(&mut input)? {
            pktline::Packet::Flush | pktline::Packet::Eof => break,
            pktline::Packet::Data(line) => line,
        };
        let line = String::from_utf8_lossy(&line)
            .trim_end_matches('\n')
            .to_owned();
        if let Some(status) = line.strip_prefix("unpack ") {
            if status != "ok" {
                report.unpack_failed = Some(line.clone());
            }
        } else if let Some(name) = line.strip_prefix("ok ") {
            report.accepted.push(name.to_owned());
        } else if line.starts_with("ng ") {
            report.refused.push(line.clone());
        }
        report.lines.push(line);
    }
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pkt(payload: &str) -> Vec<u8> {
        let mut out = Vec::new();
        pktline::write_data(&mut out, payload.as_bytes()).expect("pkt");
        out
    }

    fn band(band: u8, payload: &[u8]) -> Vec<u8> {
        let mut data = vec![band];
        data.extend_from_slice(payload);
        let mut out = Vec::new();
        pktline::write_data(&mut out, &data).expect("pkt");
        out
    }

    const A: &str = "1111111111111111111111111111111111111111";
    const B: &str = "2222222222222222222222222222222222222222";

    #[test]
    fn the_advertisement_yields_refs_and_capabilities() {
        let mut wire = pkt("# service=git-receive-pack\n");
        wire.extend_from_slice(b"0000");
        wire.extend(pkt(&format!(
            "{A} refs/heads/main\0report-status side-band-64k delete-refs ofs-delta agent=git/2.45\n"
        )));
        wire.extend(pkt(&format!("{B} refs/heads/notes\n")));
        wire.extend_from_slice(b"0000");

        let parsed = parse_advertisement(&wire).expect("parses");
        assert_eq!(
            parsed.tip_of("refs/heads/notes"),
            Some(ObjectId::from_hex(B.as_bytes()).expect("hex"))
        );
        assert_eq!(parsed.tip_of("refs/heads/absent"), None);
        assert!(parsed.has("report-status"));
        assert!(parsed.has("side-band-64k"));
        assert!(!parsed.has("quiet"));
        assert_eq!(
            parsed.request_capabilities(),
            vec![
                "report-status".to_owned(),
                "side-band-64k".to_owned(),
                format!("agent={}", crate::AGENT),
            ]
        );
    }

    #[test]
    fn an_empty_remote_advertises_capabilities_and_no_refs() {
        let mut wire = pkt("# service=git-receive-pack\n");
        wire.extend_from_slice(b"0000");
        wire.extend(pkt(&format!(
            "{} capabilities^{{}}\0report-status delete-refs\n",
            "0".repeat(40)
        )));
        wire.extend_from_slice(b"0000");

        let parsed = parse_advertisement(&wire).expect("parses");
        assert!(parsed.refs.is_empty());
        assert!(parsed.has("report-status"));
        assert!(!parsed.has("side-band-64k"));
        // Only what the server offered is asked for.
        assert_eq!(
            parsed.request_capabilities(),
            vec![
                "report-status".to_owned(),
                format!("agent={}", crate::AGENT)
            ]
        );
    }

    #[test]
    fn a_response_that_is_not_the_service_is_refused() {
        let wire = b"<html>login</html>";
        let err = parse_advertisement(wire).expect_err("not an advertisement");
        assert!(err.contains("pkt-line"), "{err}");

        let mut wire = pkt("# service=git-upload-pack\n");
        wire.extend_from_slice(b"0000");
        let err = parse_advertisement(&wire).expect_err("the wrong service");
        assert!(err.contains("# service=git-receive-pack"), "{err}");
    }

    #[test]
    fn the_request_is_one_command_a_flush_and_the_pack() {
        let old = ObjectId::from_hex(A.as_bytes()).expect("hex");
        let new = ObjectId::from_hex(B.as_bytes()).expect("hex");
        let body = request_body(
            &old,
            &new,
            "refs/heads/notes",
            &["report-status".to_owned(), "agent=x".to_owned()],
            b"PACK",
        )
        .expect("body");
        let command = format!("{A} {B} refs/heads/notes\0report-status agent=x\n");
        let mut expected = pkt(&command);
        expected.extend_from_slice(b"0000PACK");
        assert_eq!(body, expected);
    }

    #[test]
    fn side_bands_are_split_and_the_report_parsed() {
        let mut report = pkt("unpack ok\n");
        report.extend(pkt("ok refs/heads/notes\n"));
        report.extend(pkt("ng refs/heads/other pre-receive hook declined\n"));
        report.extend_from_slice(b"0000");

        let mut wire = band(2, b"remote: hello\rremote: 50%\r");
        wire.extend(band(1, &report[..12]));
        wire.extend(band(1, &report[12..]));
        wire.extend(band(3, b"fatal: out of space\n"));
        wire.extend_from_slice(b"0000");

        let demuxed = demux_side_band(&wire).expect("demux");
        assert_eq!(demuxed.progress, vec!["remote: hello", "remote: 50%"]);
        assert_eq!(demuxed.fatal, vec!["fatal: out of space"]);
        let parsed = parse_report(&demuxed.report).expect("report");
        assert_eq!(parsed.unpack_failed, None);
        assert_eq!(parsed.accepted, vec!["refs/heads/notes"]);
        assert_eq!(
            parsed.refused,
            vec!["ng refs/heads/other pre-receive hook declined"]
        );
        assert_eq!(parsed.lines.len(), 3);
    }

    #[test]
    fn an_unpack_failure_is_kept() {
        let mut report = pkt("unpack index-pack abnormal exit\n");
        report.extend(pkt("ng refs/heads/notes unpacker error\n"));
        report.extend_from_slice(b"0000");
        let parsed = parse_report(&report).expect("report");
        assert_eq!(
            parsed.unpack_failed.as_deref(),
            Some("unpack index-pack abnormal exit")
        );
        assert_eq!(parsed.accepted, Vec::<String>::new());
    }

    #[test]
    fn the_remote_url_loses_userinfo_query_and_trailing_slash() {
        let url = parse_remote("https://alice:tok3n@forge.example/o/r.git/?x=1#f").expect("url");
        assert_eq!(url.as_str(), "https://forge.example/o/r.git");
        assert_eq!(url.username(), "");
        assert_eq!(url.password(), None);

        let mut refs = url.clone();
        append_path(&mut refs, "info/refs");
        assert_eq!(refs.path(), "/o/r.git/info/refs");

        let err = parse_remote("git@forge.example:o/r.git").expect_err("not http");
        assert!(!err.to_string().contains("tok3n"));
        let err = parse_remote("ssh://alice:tok3n@forge.example/o/r.git").expect_err("not http");
        assert!(!err.to_string().contains("tok3n"), "{err}");
        assert!(err.to_string().contains("***@forge.example"), "{err}");
    }

    #[test]
    fn the_basic_header_is_the_lfs_spelling_for_a_token() {
        let token = crate::credential::AccessToken::new("tkn-123");
        let header = basic_header(&token.git(), "forge.example").expect("header");
        assert_eq!(header.to_str().expect("ascii"), token.lfs_basic());
        assert!(header.is_sensitive());
    }

    #[test]
    fn statuses_map_onto_the_taxonomy() {
        assert!(check_status(200, "h").is_ok());
        assert!(matches!(
            check_status(401, "h"),
            Err(SyncError::Auth { .. })
        ));
        assert!(matches!(
            check_status(403, "h"),
            Err(SyncError::Forbidden { .. })
        ));
        assert!(matches!(check_status(404, "h"), Err(SyncError::Config(_))));
        assert!(matches!(
            check_status(502, "h"),
            Err(SyncError::Network { .. })
        ));
        assert!(matches!(check_status(418, "h"), Err(SyncError::Config(_))));
    }
}
