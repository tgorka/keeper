//! A first-party git-LFS client (Epic 25, AD-46).
//!
//! # Why this is ours
//!
//! Large content **must** go through LFS in this engine, so the client is not
//! optional and cannot be left to a binary that may not be installed:
//!
//! * gitoxide has **no streaming object read** — `try_find` always fills a
//!   buffer, so a 3 GB blob is a 3 GB allocation — and **`gix-lfs` is an empty
//!   `0.0.0` placeholder**. There is nothing upstream to call.
//! * The `rustutils/git-lfs` family (`git-lfs-pointer`, `git-lfs-store`,
//!   `git-lfs-api`, `git-lfs-transfer`) is a single-author, two-star repository
//!   published in May 2026, and every crate pins **`reqwest 0.12`** where this
//!   workspace resolves **`0.13.4`**. Adopting them would link a second
//!   reqwest + hyper + rustls stack, which the tree's no-second-TLS-stack rule
//!   forbids outright.
//! * Shelling out to `git-lfs` would add a runtime prerequisite beyond the
//!   `git` binary AD-41 already requires.
//!
//! So we implement the subset we need — pointers, a local store, the batch API
//! and the `basic` transfer adapter — over the `reqwest` we already have. The
//! locking API, the `tus` and `ssh` adapters, custom transfer agents and the
//! never-implemented `multipart` proposal are all deliberately absent: Forgejo
//! implements only `basic`, so nothing else would ever be selected.
//!
//! # Modules
//!
//! | Module | Responsibility |
//! |---|---|
//! | [`pointer`] | the pointer file format — parse, validate, render |
//! | [`store`] | `<git-dir>/lfs/objects/…`, the content-addressed store |
//! | [`endpoint`] | deriving and overriding the LFS server URL |
//! | [`batch`] | `POST /objects/batch`, the only negotiation step |
//! | [`basic`] | the `basic` transfer adapter: resumable `GET`, single `PUT` |
//! | [`listing`] | which LFS paths this clone holds, and which it only names |
//! | [`hydrate`] | asking for one path's content, and the refusal that protects the bytes there |
//! | [`virtual_policy`] | which paths' content may stay unmaterialized |
//!
//! # The quirks this implementation encodes
//!
//! Each of these is a real, verified behaviour of a server we must work with,
//! and each is load-bearing. Simplifying any of them away reintroduces a bug
//! that is expensive to rediscover, so they are listed here as well as
//! commented at their handling sites.
//!
//! **Forgejo** (`main @ 10beaf54`):
//!
//! 1. **`Accept` must be first, or 415.** `services/lfs/server.go:59` compares
//!    `strings.Split(header, ";")[0]` against `application/vnd.git-lfs+json`.
//!    reqwest sends no default `Accept`, so every LFS JSON request sets it —
//!    including the `verify` POST. See [`batch::LFS_ACCEPT`].
//! 2. **`Content-Range`'s complete-length is wrong.** The server emits
//!    `bytes {from}-{to}/{size-from}`. Only the **start** byte may be
//!    validated. See [`basic::parse_content_range_start`].
//! 3. **`Range` offsets are parsed with 32 bits.** Resume at or above 2 GiB
//!    returns the wrong bytes. See [`basic::RESUME_OFFSET_CEILING`].
//! 4. **No `expires_in` / `expires_at`.** Action URLs never advertise expiry, so
//!    a mid-transfer 401/403 is treated as "re-authenticate and retry once"
//!    rather than pre-empted by a refresh timer.
//! 5. **`Authorization` is echoed into action headers** and is the credential
//!    the data transfer actually uses — pre-signing must not be assumed.
//! 6. **`verify` actions get `Accept` force-injected** as a workaround for
//!    [git-lfs#3662](https://github.com/git-lfs/git-lfs/issues/3662).
//! 7. **`basic` is the only adapter**, and the response omits the `transfer`
//!    key entirely, so a missing key must be read as `basic`.
//! 8. **`SERVE_DIRECT` hrefs are pre-signed**; re-attaching a credential makes
//!    S3 answer 400. Keyed off the batch response's `authenticated` flag.
//! 9. **Upload dedup needs a batch round trip.** The PUT body is hashed as
//!    proof of possession, so an upload can never be skipped client-side.
//! 10. **`413` means two different things** — batch too large, or a per-owner
//!     quota — told apart only by the response body.
//! 11. **The server-side object layout differs from the client's**
//!     (`oid[0:2]/oid[2:4]/oid[4:]`, tail only). See [`store::LfsStore::object_path`].
//!
//! **The pointer format itself** (`git-lfs/docs/spec.md`):
//!
//! 12. **The encoding is unique**, so re-emitting a semantically identical but
//!     differently-spelled pointer changes its git blob hash — a phantom
//!     modification on every sync. See [`pointer::Pointer::is_canonical`].
//! 13. **An empty file is its own pointer** and passes through unchanged.
//! 14. **Unknown keys must survive** a parse-and-regenerate cycle.

pub mod audit;
pub mod basic;
pub mod batch;
pub mod endpoint;
pub mod filter;
pub mod hydrate;
pub mod listing;
pub mod local;
pub mod pktline;
pub mod pointer;
pub mod prune;
pub mod ssh;
pub mod stage;
pub mod store;
pub mod virtual_policy;
