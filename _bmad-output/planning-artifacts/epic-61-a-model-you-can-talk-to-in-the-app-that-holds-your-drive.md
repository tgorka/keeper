# Epic 61 — A model you can talk to, in the app that already holds your drive

created: '2026-09-02'
source: the owner's fourteen-line capability request, written after v0.8.24. Grounded by six read-only repository scouts and six external librarians over `main` @ `9e06c2b`, 2026-09-02, synthesized into `research-ai-chat-2026-09-02.md`. Every verdict below carries the `file:line` or the URL it was read from.
binds: FR-369…FR-393 (allocated here); NFR-46…NFR-49 (allocated here); AD-146…AD-160 (new); AD-27 (no dead buttons, consumed), AD-40 (`keeper-sync` is core-free, consumed), AD-55/AD-56 (decisions live in `keeper-core`, the shell is a call site — consumed and load-bearing), AD-58 (bytes never cross IPC, consumed), AD-65 (the one lexical containment rule, consumed verbatim), AD-89 (write routing, consumed), AD-137 (a capability gates its surface, consumed), AD-139 (no JSON blobs in rows, consumed), AD-C7 (one component, two modes, consumed), AD-53 (egress is computed, never hand-listed — extended here)
see-also: `research-ai-chat-2026-09-02.md` (this epic's evidence base; cited below as §n). Epic 60 stays **reserved** for the general exec kind that epic 59 deferred (`epic-59-…:6`, `docs/decisions.md` D-3) — this epic does not build it and does not take its number. Epic 62 (`epic-62-*`, not yet written) owns talk mode and the wake word, for the reason stated in *What stays out*.

## What he said

> I want to add feature in the keeper to talk to the models mainly with hermes agent, but also
> oolama (remore and local) - using openai api standard.
> - i want to have a change that i talk
> - i want to support bot mode (from hermes) - look the hermes, grokbot etc
> - i want to discover what bots are possible on my hermes model
> - i want to discover what models are possible on hermes and ollama
> - i want to pin the bots (main bots) i want to talk to. (add shape, color and image/emoticon for the bot)
> - i want to see the session list/archive, continue the session
> - i want to see the history
> - i want to have the chat and some metadata in the chat - maybe with switch on off (look at passeo, t3code)
> - i want to use the slash commands (look telegram, passeo, t3code how is it used)
> - i want to configure multi tenant (oolama and hermes)
> - i want to have tool calls for access in the drive data as a filesystem - and use of agent.md, claude.md, okf etc
> - for local data access i want to have permission (read,write,none, drive, part of the drive)
> - i want to have talk mode for the hermes voice and support for hermes wake word, (you can use neutts)
> - i want it use image mode (with past from clibboard, deliverable mode (look hermes)
> - i want in the future to onnect to omp as well

Fourteen asks. Thirteen of them are one feature; one of them is a different feature wearing the
same coat, and this epic says so out loud rather than half-building it.

## What exists, and what does not

| his ask | verdict | evidence |
| --- | --- | --- |
| talk to Hermes over the OpenAI API | **absent, and reachable** | Hermes 0.21.0 ships `gateway/platforms/api_server.py`, an aiohttp server on `127.0.0.1:8642`, bearer-authenticated with `API_SERVER_KEY`, exposing `POST /v1/chat/completions`, `GET /v1/models`, `GET /v1/capabilities`, `GET /api/model/options` and the whole `/api/sessions*` family (§2.2). MIT-licensed, and a client links none of it. |
| talk to Ollama, local and remote | **absent, and reachable** | `/v1/chat/completions` with streaming, tools, vision and JSON mode; `/api/tags` already carries a `capabilities` array per model (§3.1-3.2). No auth middleware exists at all — `api_key` is accepted and discarded (§3.5). |
| bot mode | **absent; the primitive on the other side is a *profile*** | "a Bot **is** a Hermes profile — isolated config, memory, skills, credentials, and chat history under `~/.hermes/profiles/<name>/`" (§2.6). Every api-server route is registered a second time at `/p/{profile}<path>`, so a bearer client can *address* any bot by URL prefix (§2.2). |
| discover which bots exist | **partly impossible over the door keeper is allowed through** | The bearer route table has **no profile roster** (§2.7). The two doors that list bots are the dashboard's `GET /api/profiles` (auth `X-Hermes-Session-Token`) and the TUI-gateway websocket RPC `profiles.list`. This epic ships verification, not enumeration, and says which. |
| discover which models exist | **available on both, differently** | Hermes: `/v1/models` returns aliases only; `/api/model/options` returns providers, curated model lists and per-model `supports_tools` / `supports_vision` / `supports_reasoning` / `context_window` (§2.8). Ollama: `/api/tags` returns `capabilities` for every local model in one round trip (§3.2). |
| pin bots, with shape, colour, an emoji | **half-present, and the missing half is deliberate** | Hand-ordered pins with drag-to-reorder exist (`registry.rs:260-296`, `pins-strip.tsx`); a user-chosen **icon** exists (`space-icons.ts:505-525`); a user-chosen **colour** does not — hue is assigned, never chosen (`account-hue.ts:12-21`). `DESIGN.md:172` is the reason: "Every state colour is paired with a shape: a filled/hollow/dashed lamp, never a bare dot." |
| session list, archive, continue | **the closest prior art in the app** | Work sessions ship a board, an archive and lineage; the "continue" chips are written but inert (`session-detail.tsx:485-495`). Hermes also keeps server-side sessions with ids, unique titles, parents and a 500-row default page (§2.9). Two truths, and this epic picks one. |
| history | **exists as a shape to copy** | The recordings archive's list/filter/sort/paginate shape (epic 42) and the find-bar conventions (`spec-find-bar-matches-the-ui.md`). |
| chat with metadata, switchable | **absent, with no precedent anywhere** | grep `Show (details\|source\|raw\|metadata\|debug)\|developer mode` over `src` → 0 hits. The timeline shows send-state and edited captions only (§12). |
| slash commands | **absent in the composer, present in the note editor** | `slash-menu.ts:58-135` is a working, tested `/` menu with filtering and keyboard model. The Matrix composer has none (grep `startsWith("/")` → path code only). Hermes' own registry is server-side and enumerable **only** over the websocket RPC `commands.catalog` (§14). |
| multi-tenant config | **one precedent, and it is the right one** | `SyncProfile` is the tree's only multi-tenant record: several endpoints of the same kind, side by side, each with its own credential (§8.1). |
| tool calls onto the drive as a filesystem | **every primitive exists; nothing composes them — and only one of his two back ends can be given them** | `browse::resolve`, `plain_segments`, `WriteScope::route` are the containment rule (`browse.rs:623`, `files_write.rs:57`), reused across a verb boundary once already (`engine.rs:10159-10160`). Reading a file may hit a *pointer* file, not content (epic 56, §6.4). **But Hermes executes its own tools on the Hermes host**: `/v1/capabilities` declares `tool_execution: "server"`, no HTTP route registers a client tool, and api-server sessions default to `unattended_mode: deny` (§2.13, §2.6). A client-side drive tool is therefore reachable on an **Ollama** model advertising `tools`, and on Hermes it is not keeper's to offer. |
| permission: read / write / none, whole drive or part | **absent — there is nothing to extend** | "What exists is *routing* and *disclosure*, not permission" (§7.5). `IncognitoScope`, `MediaPolicy`, `VirtualPolicy` are sync policies; TCC prompts are the OS's. This epic mints keeper's first user-granted, revocable capability. |
| talk mode, wake word, neutts | **absent, and it is a *different epic* — see below** | No STT, no TTS, no wake word, no speech framework is linked (§10). Transcription is a six-times-restated *won't* for the recording feature, enforced by a CI source scan that fails on the token `transc`+`ri` (`zero-egress.test.ts:82`). |
| image mode, clipboard paste, deliverable mode | **the plumbing exists, pointed the other way** | Pasted images already reach Rust as a percent-decoded raw IPC body, never base64 through JSON (AD-58, `notes_vault.rs:1851-1853`). Deliverable mode is a *gateway* feature: Hermes strips `MEDIA:` directives and absolute paths from the visible text and uploads the file (§2.11) — over the api-server a client receives the path itself. |
| omp, later | **deliberately not designed for** | A third provider kind is one story on this epic's own foundation. Designing an abstraction for it today, with no endpoint to read, is the mistake this epic refuses (DW-214). |

**The through-line, and the epic's spine.** keeper already holds the drive, the notes, the sessions
and the tasks; what it does not have is a way to *ask something about them*. So this is not a chat
client bolted onto a messenger — it is a fifth keeper surface with the same four rules as the other
four: **one vocabulary** (provider → bot → session → message), **every decision in `keeper-core`**
(AD-55/AD-56), **an egress row that is computed, never hand-written** (AD-53), and **no affordance
that lies** (AD-27). Two of his asks are, underneath, the same question — "what can this endpoint
do?" — and two others are the same answer: a grant is the only reason a tool call is allowed to
touch a byte.

**Where the risk lives versus where the test looks.** Every previous epic that failed review failed
because the assertion sat on a pure function while the risk sat in the impure shell. Here the impure
shell is explicit and each item is named in the story that owns it: a **real SSE socket** that dies
mid-frame (61.2), a **real 401 and a real 404** from a wrong base URL (61.3), a **real file** that is
a pointer and not content (61.11), a **real `rename`** under the sync watcher (61.11), and a **real
revocation** that must stop an in-flight tool call (61.10). A test that proves any of these through a
pure function has proven nothing.

**The vocabulary, frozen before the first line of code.** A **provider** is an endpoint keeper can
reach: a kind, a base URL, a credential. A **bot** is *(provider, model-or-profile, identity)* — and
that triple is what makes his two back ends one feature rather than two: a Hermes bot is a
*profile*, addressed by the `/p/{profile}` prefix its api-server already registers for every route
(§2.2); an Ollama bot is a *model tag*, addressed in the request body. Both are things you pin, name,
give a shape, and hold a conversation with. A **session** belongs to one bot; a **message** belongs
to one session. The surface is `Bots`, at ⌘9, and the code says `bots` everywhere — one word, so
that nobody has to translate between the sidebar, the command names and the tables.

## The two decisions this epic makes before any story starts

**1. One wire protocol, and the divergences are data, not code paths.** He asked for "openai api
standard" and both back ends honour it, so `POST /v1/chat/completions` with `stream: true` is the
only chat transport keeper implements. The native dialects stay unimplemented, and the price is
written down rather than discovered: Ollama's `/v1` layer ignores `tool_choice`, refuses
`image_url` (base64 data URIs only), and **cannot set the context window** — `num_ctx` needs the
native `/api/chat` (§3.5, §4). Those become rows in a per-kind quirk table, not a second client.
Model and capability *discovery* is the one place the native endpoints are used, because that is
where they are strictly better (`/api/tags` carries capabilities; `/v1/models` does not).

**2. keeper owns the conversation; the server's session id is a reference, not the truth.** Hermes
has real server-side sessions, and it is tempting to make them the store. They are the wrong truth
for three sourced reasons: compression mints a *new* continuation session with a renamed title
(§2.10), the stored-response cache is 100 rows LRU (§2.10), and Ollama has no session concept at
all. keeper's own store is the truth for both kinds; a Hermes `session_id` is persisted beside the
row so `fork` and `latest-descendant` remain reachable later.

## What the binds mean

| id | statement | story | AD |
| --- | --- | --- | --- |
| FR-369 | keeper holds several AI providers side by side, each with a kind, a base URL and its own credential | 61.1 | AD-146 |
| FR-370 | a provider's credential is stored where secrets are stored, never in a row and never on a screen | 61.1 | AD-147 |
| FR-371 | every configured provider appears as a derived egress destination, as a host and never a URL | 61.1 | AD-148 |
| FR-372 | keeper streams a chat completion over the OpenAI-compatible wire and can stop it | 61.2 | AD-149 |
| FR-373 | a reply, its tool calls and its usage survive reassembly from fragmented deltas | 61.2 | AD-149 |
| FR-374 | a stream that stalls is ended by silence, not by a whole-request deadline | 61.2 | NFR-46 |
| FR-375 | keeper reports what an endpoint is and what it can do, or says why it cannot tell | 61.3 | AD-150 |
| FR-376 | a bot is addressed by profile prefix, verified by probe, and never invented | 61.3 | AD-150 |
| FR-377 | a model's vision and tool support is read from the endpoint, not assumed | 61.3 | AD-151 |
| FR-378 | there is a surface at ⌘9 where you talk to a model, and it narrates its own state honestly | 61.4 | AD-152 |
| FR-379 | a provider is added, tested and removed from Settings | 61.4 | AD-146 |
| FR-380 | an answer renders as prose and code, with no remote fetch and no HTML injection | 61.5 | AD-153 |
| FR-381 | conversations are listed, searched, renamed, archived and resumed | 61.6 | AD-154 |
| FR-382 | a resumed conversation replays from keeper's own store | 61.6 | AD-154 |
| FR-383 | bots are pinned in a hand order, with a shape, a bounded colour and a mark | 61.7 | AD-155 |
| FR-384 | per-message metadata is recorded always and shown on request | 61.8 | AD-156 |
| FR-385 | the composer accepts slash commands, and refuses unknown ones with a reason | 61.9 | AD-157 |
| FR-386 | a grant names a scope and a mode, is visible, and is revocable | 61.10 | AD-158 |
| FR-387 | a write is never auto-approved by a grant alone | 61.10 | AD-158 |
| FR-388 | every tool call that touched the drive is auditable after the fact | 61.10 | NFR-47 |
| FR-389 | the model reads and writes the drive through keeper's own containment rule | 61.11 | AD-159 |
| FR-390 | file content reaches the model as data, never as instructions | 61.11 | NFR-48 |
| FR-391 | `AGENTS.md`, `CLAUDE.md`, the drive's own `.okf/OKF-0.2-digest.md` and their kin are found, merged and budgeted; a note read carries its OKF type and trust actor | 61.11 | AD-159 |
| FR-392 | an image is pasted into the conversation and sent only to a model that can see | 61.12 | AD-160 |
| FR-393 | a path a model names is opened only inside a grant | 61.12 | AD-160 |
| NFR-46 | a stalled stream is abandoned after bounded silence, not bounded total time | 61.2 | AD-149 |
| NFR-47 | a tool-call audit row is written before the effect, and survives a crash | 61.10 | AD-158 |
| NFR-48 | no tool result can widen its own grant, and no file content is trusted as a directive | 61.11 | NFR-48 |
| NFR-49 | every byte read or written by a tool call is bounded, and the bound is disclosed to the model | 61.11 | AD-159 |

## Stack order

```
61.1   a provider is something keeper remembers      (store, kinds, base-URL grammar, keychain, health, derived egress)
61.2   one wire, and the divergences are data        (SSE client, delta reassembly, cancel, silence timeout, quirk table)
61.3   what can this endpoint actually do            (models + capabilities per kind, bot probe, honest refusal to enumerate)
61.4   a conversation you can have                   (⌘9 pane, capability flag, provider/bot picker, Settings section, stream, stop, retry)
61.5   the answer is readable                        (markdown subset over the lezer stack already in the tree, code blocks, copy, no remote fetch)
61.6   a list, an archive, and continue               (conversation list, search, rename, archive, resume from keeper's store)
61.7   pinned bots with an identity                   (hand order, shape + bounded colour + mark, contrast-checked)
61.8   metadata you can switch on                     (model, tokens, time-to-first-token, finish reason, request id; one toggle)
61.9   slash commands that refuse honestly            (client-side registry, autocomplete, unknown-command refusal)
61.10  a grant you can see and revoke                 (scope × mode, approval, revocation mid-flight, audit)
61.11  the drive as tools                             (fs tool surface over the existing containment rule, caps, pointer files, context files)
61.12  an image you can paste, a path you can open    (vision parts without base64 over IPC, deliverable paths inside a grant)
61.13  the gates, the docs, and the honest row        (egress chapter, D-4, cargo-deny notes, the checks that would now cry wolf)
```

61.1 and 61.2 are the foundation and are the only strictly serial pair — 61.2 cannot be tested
without a provider record, and 61.3 cannot be written without the client. 61.4 depends on 61.1-61.3.
61.5, 61.7, 61.8 and 61.9 are parallel refinements of the surface 61.4 builds. 61.10 precedes 61.11
absolutely: **the grant exists before the first tool can read a byte**, and a story order that
inverts that ships an unguarded filesystem for one PR's duration. 61.12 depends on 61.3 (the vision
capability) and 61.10 (the grant). 61.13 is last because it corrects what the twelve before it made
false.

## Acceptance, per story

**61.1** — **A provider is a record, not a setting.** A row holds a kind (`hermes` | `ollama`), a
display name, a base URL, an optional bot/profile prefix, a timeout override and a health snapshot;
the credential lives behind the existing secret port (`platform.rs:34-42`), and a row that has lost
its secret says so instead of failing at send time. The base-URL grammar is a `keeper-core` decision
with its own tests: scheme must be `http` or `https`, no userinfo, no path beyond a prefix, a
loopback host is legitimate and a private-network host is legitimate **and both are disclosed** —
the SSRF question is answered by disclosure and an explicit user act, not by a blocklist (§3.10).
Egress is *derived*: `compute_egress` gains a row per configured provider, host-only, reusing
`egress::remote_host` so userinfo is stripped by the same function that strips it for git remotes
(`egress.rs:72-124`). No JSON blob in a row (AD-139).

**61.2** — **The client is one framer and one state machine, and both are tested against a socket
that misbehaves.** `keeper-core::llm` builds the request, frames SSE (`data:` lines, `[DONE]`,
keep-alive comments, a frame split across two `Bytes` chunks), and reassembles a message from
index-keyed `tool_calls` fragments whose `arguments` arrive as partial JSON. Timeouts follow the
tree's existing insight verbatim — a whole-request deadline is the wrong question, silence is the
right one (`keeper-sync/src/http.rs:24-40`) — and because `keeper-core` may not depend on
`keeper-sync` (AD-40, `check:core-sync-free`), the *policy is restated with a comment naming its
source*, not copied by accident. Cancellation is an abort that leaves a partial assistant row
persisted and marked partial, never a silently discarded reply. Acceptance is a local test server
that: sends a valid stream; sends a frame in two chunks; dies mid-message; sends `[DONE]` with no
usage; returns 401, 404 and 500; and holds the socket open sending nothing.

**61.3** — **Discovery answers three questions and refuses a fourth with its reason.** Per kind:
models (`/api/tags` for Ollama because it carries `capabilities`; `/v1/models` plus
`/api/model/options` for Hermes), per-model vision/tools/context where the endpoint states them, and
health. The fourth question — *which bots exist* — has no bearer-authenticated answer (§2.7), so
keeper ships **verification**: a named profile is probed at `/p/<name>/v1/models` and either exists
or does not, and the empty state says, in the house voice, that a roster needs a door keeper is not
given. A capability keeper could not read is `unknown`, never `false` — an `unknown` vision flag
offers the paste affordance with a warning; a `false` one hides it (AD-27).

**61.4** — **A fifth surface, built the way the other four were.** `PrimaryView` gains one member;
`CapabilitiesVm` gains one flag computed in the shell (`ipc.rs:1426-1459`), mirrored in the frozen
store default *and* `dev/mock-shell.ts` or typecheck fails; the sidebar entry is **absent, not
disabled**, where the flag is off (`sidebar-pane.tsx:106-109`); the chord is ⌘9 — the only free digit
— bound by a hook cloned from `use-tasks-shortcut.ts` with its five guards in order and its
no-dead-chord rule. The flag is a genuinely new condition and not a synonym for `sessions`: chat
needs neither `git` nor `sync.db`, while the drive-tool half needs `sync` exactly — so the pane is
gated on the new flag and **the grant affordance is gated on `sync`**, which is the honest split.
Settings gains a Providers section (add, test, edit, remove) built as one component in two modes
(AD-C7). Streaming renders progressively, Stop stops, Retry re-sends, and an unreachable endpoint
produces the same vocabulary the app already uses for an unreachable remote (§8.9).

**61.5** — **An answer is prose and code, and nothing else is executed.** Rendering reuses the
markdown stack already in `package.json` (`@lezer/markdown`, `@codemirror/lang-markdown`) — no new
dependency, no `dangerouslySetInnerHTML`, no remote asset fetch, which is the same position
`note_protocol.rs:31-34` already takes: "a note that auto-fetches a URL an agent wrote is a tracking
pixel". Fenced code blocks get a copy control and a language label; an unterminated fence mid-stream
renders as code and closes itself; a 200 kB reply does not lock the webview.

**61.6** — **The list is the archive, and continue means replay.** Conversations list newest-first
with search over titles and bodies, rename, archive (reversible) and delete (confirmed, and the
confirmation names what happens to which object — the house's chain-of-custody rule). Resume replays
from keeper's store; where a Hermes `session_id` is held, the detail shows it and says plainly that
the remote may have compressed it into a successor (§2.10). Titles are minted locally from the first
user message — no extra model call, because a silent second request to a paid endpoint is exactly
the kind of surprise this app does not ship.

**61.7** — **A pin is a shape and a mark first, a colour second.** Order is hand-set and persisted
with the `registry.rs:260-296` pattern; identity is a shape (from a closed set), a mark (the existing
`space-icons.ts` picker, extended with a short emoji/grapheme option) and a colour **from a bounded
palette whose every member is contrast-checked against both themes by `scripts/check-design.mjs`** —
because `DESIGN.md:168` already records that "no colour of any hue passes AA on both themes" for an
unconstrained pick, and `DESIGN.md:172` requires colour to be paired with a shape. A free colour
picker is therefore rejected, not deferred.

**61.8** — **Metadata is always recorded and never in the way.** Every assistant row stores model,
provider, prompt/completion tokens where the endpoint reports them, time-to-first-token, total
duration, finish reason, tool-call count and the provider's request id. One toggle — persisted, in
the pane and in the palette — shows a compact caption per message, and an expander shows the rest.
Absent numbers render as absent, never as zero: an endpoint that omits `usage` is a fact about the
endpoint, and this is the app that already refuses to print a number it did not measure.

**61.9** — **The registry is client-side, and it says so.** A `keeper-core` registry (name,
description, argument mode, whether it needs a provider) drives an autocomplete cloned from
`slash-menu.ts:58-135`. Shipped: `/new`, `/bot`, `/model`, `/metadata`, `/grant`, `/image`,
`/history`, `/help`. An unknown command is a refusal that names the nearest match and does **not**
get sent to the model as prose — with one deliberate exception the empty state teaches: a literal
leading slash is escaped, because a model may legitimately be asked about `/etc`. Hermes' own
server-side commands are **not** proxied: they are enumerable only over a websocket RPC keeper does
not speak (§14), and a picker that silently drops `/compact` on Ollama would be an affordance that
lies.

**61.10** — **A grant is keeper's first permission, so it is built like a promise.** A grant is
(provider, optional bot) × (scope: the whole drive, or one profile, or one subtree) × (mode: none |
read | write), stored, listed in Settings and in the pane's header, and revocable in one act.
Revocation is checked **at every tool call**, not at conversation start, and an in-flight tool call
whose grant vanished fails with that reason — this is the impure-shell test the story must carry.
`write` never means auto-approved: the first write in a conversation, and every write outside an
already-approved subtree, asks; "always for this subtree" is a durable *grant edit*, so the answer
to "what can it change?" is one list and not a hidden history of clicks. An audit row is written
before the effect (NFR-47) and the reader of that log is a human, so it names paths and not ids.

**Coordinator correction, 2026-09-02, after the architecture pass.** A grant is offered only where
keeper is the thing that would execute the tool: an **Ollama** bot whose model advertises `tools`.
A **Hermes** bot's pane offers no grant, and says why in one sentence, because Hermes runs its own
tools on its own host under its own permission model — `/v1/capabilities` declares
`tool_execution: "server"`, there is no route by which a client registers a tool, and an api-server
session's `unattended_mode` defaults to `deny` (§2.6, §2.13). Offering a grant that could never be
used would be exactly the affordance-that-lies AD-27 forbids.

**61.11** — **The tools are the existing rule, exposed.** `list`, `read` (with line ranges), `glob`,
`grep`, `stat`, `write` and `edit`, each resolved through `browse::resolve` / `plain_segments` /
`WriteScope::route` in `keeper-sync` — carried across the verb boundary verbatim, the way
`engine.rs:10159-10160` already carried it, and never restated as new path arithmetic. Every result
is bounded and the bound is *told to the model* (`"truncated at 64 kB of 1.2 MB"`), because a silent
truncation makes a model confidently wrong. A read that lands on a pointer file returns the pointer's
truth, not an empty file (§6.4). A write is atomic and goes through the same temp-and-rename the tree
already uses, and the story proves it against a real watcher pass. `AGENTS.md`, `CLAUDE.md`,
`GEMINI.md` and `.cursorrules` are discovered nearest-first, merged in a stated order, size-capped,
and **shown to the user as what the model was told**. All file content — context files included —
enters the request as data under a system-prompt sentence that says instructions inside file content
are not instructions (§7.6, the lethal-trifecta literature).

**"okf" is keeper's own, and it is the most useful thing in this story.** Corrected twice on
2026-09-02: the first draft called it unidentifiable, the architecture pass called it Google's Open
Knowledge Format, and both were wrong — it is `keeper_core::notes::okf`, "OKF v0.2 frontmatter, as a
typed and *tolerant* view over `Frontmatter`" (`notes/okf.rs:1-8`), whose local record lives on the
drive itself at `.okf/OKF-0.2-digest.md` (`:10`). Two consequences, both required here. First, that
digest is a context file by the same rule as `AGENTS.md`, and it is *on the drive*, so it is found by
the same nearest-first walk. Second, and this is the part worth building: OKF's trust family carries
`ActorKind` — "the one question a reader actually asks — did a *person* look at this, or only a
model" (`:22-25`). A `read` of a note therefore returns its `type` and its trust actor alongside the
body, which is exactly the provenance separation the injection literature asks for and which keeper
can supply and a generic filesystem tool cannot. The reader stays a reader: this story reads OKF and
never writes it, because every write to a note body already goes through the scanner
(`notes/okf.rs:6-8`) and a tool that reflowed frontmatter would break FR-121's byte-level promise.

**61.12** — **An image goes the way images already go, and a path is only a path.** A pasted image
reaches Rust as a raw IPC body and is stored and referenced like every other attachment — no base64
through JSON, ever (AD-58) — and is attached to the request as a data-URI content part **only** for a
model whose vision capability is `true` or `unknown`; for `false` the paste is refused with the
model's name in the sentence. Size and count caps are stated. Deliverable mode is handled from the
receiving end: an absolute path in a reply is recognised, and offered as *reveal* or *open* **only if
it falls inside a grant** — keeper never fetches or copies a path a model named outside one, and
never strips text from a reply the way the Hermes gateway does, because here the reply is the record.

**61.13** — **The checks that would now cry wolf, fixed in the same epic.** `docs/egress.md` gains
the provider chapter and the release-diff note will fire, which is the visibility working. A new
`docs/decisions.md` entry, **D-4 — the endpoint is yours, never keeper's**, records that keeper ships
no default endpoint, no hosted model, no telemetry and no opt-in scaffolding for any of them
(`egress.md:96-100`), and that this is why a base URL is required rather than defaulted. The
recording zero-egress gate is left **untouched and unwidened**: no file this epic adds may match its
globs, and the epic states the rule — *the AI surface never lives in a recording path and never adds
a verb to `command-palette/actions.ts` that the scan would read as an upload or a transcription*
(`zero-egress.test.ts:39-84`). `deny.toml` and the manifests get the licence justification comment
the house requires for each new crate, and `docs/project-context.md` gets the two new invariants.

## What stays out, and why

- **Talk mode, the wake word, and neutts — Epic 62, not a story here.** The evidence closes it:
  every credible permissive stack needs new native dependencies that **cannot be compiled or
  exercised on the development host**, and the largest open licence question in the research is the
  one that would ship in the bundle — the pretrained keyword-spotting and ASR model weights carry
  dataset terms that the code's Apache-2.0 licence does not imply (§10, `[UNVERIFIED]` 9). The
  chosen direction is already recorded so 62 does not re-research it: `cpal` + Silero VAD +
  statically-linked `ort` + `sherpa-onnx` KWS built `-no-tts` + `whisper.cpp` via `whisper-rs` +
  `AVSpeechSynthesizer` for output; openWakeWord is **rejected** (CC BY-NC-SA weights), Porcupine
  **rejected** (proprietary, telemetry, mandatory key), Piper and sherpa-onnx-with-TTS **rejected**
  (GPL-3.0 via espeak-ng), NeuTTS **second-best only**, and only if voice cloning becomes a
  requirement, because its repository licence reaches Outputs above a revenue threshold. Epic 62
  also owns the thing this epic must not blur: the recording feature's promise that it transcribes
  nothing is stated in six places and enforced by a source scan, and a voice surface that borrows
  the recording pipeline would make that promise false.
- **Proxying Hermes' server-side slash commands** — enumerable only over a websocket RPC (§14). A
  hand-copied list would rot on the next Hermes release. DW-209.
- **The native `/api/chat` dialect**, and with it `num_ctx` on Ollama and `think` levels. DW-210.
- **A second model call to title a conversation.** DW-211.
- **Embeddings, RAG, or any index over the drive.** Not asked for, and it would make a second source
  of truth about the user's files. DW-212.
- **Running commands**: no tool executes a shell string. `docs/decisions.md` D-3 stands, and the
  general exec kind remains Epic 60's, unbuilt. This is stated because a filesystem tool surface is
  exactly where someone will propose `bash`. DW-213.
- **omp as a provider kind.** The enum stays closed at two. DW-214.
- **Giving a Hermes bot the drive, by being an MCP server rather than a tool client.** This is the
  honest route and it is a real one: Hermes loads `mcp_servers:` from its own config over stdio or
  HTTP (§2.14), so keeper could *serve* the same seven verbs, behind the same grant, to a bot running
  on another host. It is deferred and not rejected — it inverts the direction of the connection, it
  needs its own threat model (a listening socket is not an outbound request), and it must not be
  designed from the client side by analogy. DW-215.

## Design notes

Reuse, do not rebuild: `slash-menu.ts` for the `/` affordance; `pins-strip.tsx` + `registry.rs` for
order; `space-icons.ts` for the mark; the recordings-archive list shape for history; `window-list.tsx`
for virtualization the chat timeline never got; `media-viewer.tsx` for an attached image; the
`Channel<T>` subscribe-and-return-an-id pattern with the DW-4 cleanup rule for every stream. Do not
reuse `message-bubble.tsx`: it is Matrix-typed down to send-state and read receipts (§12), and
widening it to carry a second domain's row would couple the two surfaces for the sake of a rounded
rectangle.
