# Epic 86 — Browse your repositories and add them as drives

created: '2026-09-24'
source: the owner's request of 2026-09-24 (verbatim below) and the owner's two decisions the same day. Other inputs:
- one research lane, `local://r86-RForge.md` (GitHub REST and device flow, Forgejo 15 at `v15.0.4`, RFC 8693, ZITADEL; probed live where marked);
- the coordinator's live probes against electra's Forgejo and a private GitHub repository;
- the makistack pull request this plan's lane found while writing, tgorka/makistack#863 (`github-broker`).

The coordinator froze the model as `local://epic86-contract.md` and amended it three times during the build wave (A1–A3, below). A2 replaced the broker protocol entirely. After two review lanes (`local://review-core86.md`, `local://review-surface86.md`), a fourth amendment, A4 (`local://epic86-fixwave.md`), tightened where tokens go, whose caches answer, and what a batch may add. Line numbers are at `6289fb4b` (epic 85 merged, PR #405), the commit the build wave started from.
binds: FR-734…FR-745 and NFR-101…NFR-104 (allocated here); AD-333…AD-338; UX-DR120; DW-323…DW-330 (allocated in *What stays out*); D-28 (drafted at the end, for `docs/decisions.md`).
- **The previous ceilings:** FR-733, NFR-100, AD-332 and UX-DR119 (epic 85); DW-322 (epic 85's review wave, `deferred-work.md:6760`); D-27 (`docs/decisions.md:1330`).
- **No earlier allocation.** A grep of `_bmad-output`, `docs`, `src` and `src-tauri/crates` for `epic-86`, FR-734…FR-749, NFR-101…NFR-109, AD-333…AD-339, UX-DR120…UX-DR129, DW-323…DW-339 and `D-28` found none.
see-also:
- epic 85: AD-326 (a credential choice travels in `settings.<device>.toml`), AD-329 (`folder_accepts`), AD-330 (a drive on the account's forge gets the forge token), and its rejected alternative *RFC 8693, one token per service*;
- epic 84: AD-323 (offers and the prefilled add-folder form);
- epic 82: AD-310, AD-315 (the credential choice) and the descriptor's schema;
- AD-27 (absent rather than disabled), AD-40 (the crate split), AD-53 (a computed egress list), AD-62 and D-3 (one clock per host process), D-4 (a destination exists because someone chose it), D-25 (keeper does not administer other people);
- `docs/account.md` § *Browse your repositories* and § *GitHub through your organization's broker*;
- makistack `docs/runbooks/github-broker.md` on branch `feat/github-broker`.

## The owner's ask

Verbatim:

> now want to list button to list the repos in sync to add them got github and forgejo provider (including organizations in github) - list then and help adding them to the list of sync drives. Design best ui/ux for that - for the fetch support the proxy for getting the token based on oath (work in progress in makistack - chceck upcomming prs if needed).

In other words, five asks:
1. **A button in Sync that lists repositories,** so they can be added as drives.
2. **Two providers:** GitHub, including the person's organizations, and Forgejo.
3. **Help adding them** to the list of sync drives, not just a list.
4. **The best UI and UX** keeper can design for it.
5. **The fetch uses makistack's token proxy,** which gets a token from the person's OAuth sign-in. It was work in progress, and upcoming PRs were to be checked.

The owner's decisions (2026-09-24):
1. **GitHub list:** the person's own repositories plus those of every organization they belong to.
2. **Broker:** keeper implements the client side only.
   - The coordinator first recorded that no makistack broker existed: no PR, branch or container had been found.
   - While writing this plan, the lane found tgorka/makistack#863, *feat(github-broker): GitHub App tokens for keeper, git and gh through ZITADEL*. It was opened at 2026-09-24T10:04:59Z and is **open, not merged**.
   - The coordinator then amended the contract (A2): **the proxy the owner meant is #863's `github-broker`.** keeper speaks its protocol, and the RFC 8693 token exchange the contract had frozen is dropped entirely.
   - keeper ships no broker code. That keeps AGENTS.md's rule: "a **client only** — no server-side components".

## What the triage found

| Need | Verdict | Evidence |
| --- | --- | --- |
| A list of the person's repositories on any forge | **absent** | Nothing in keeper lists repositories: `git grep 'user/repos\|repos/search\|installation/repositories\|login/device'` over `src-tauri/crates` and `src` at `6289fb4b` finds nothing. The only forge API keeper calls is `user_url` (`{api_base}/user`), which in `oauth` mode checks the forge username (`docs/account.md:123`). |
| The Forgejo list, with the grant keeper already holds | **broken via `/user/repos`, present via `/repos/search`** | electra's connector grants `openid profile write:repository` (epic 85, GInfra). Forgejo turns that into the token scope `write:repository` (`oauth2.go` `grantAdditionalScopes`). `/user/repos` sits in the `/user` group guarded by `read:user` (`api.go:634-760`, `:722`), and `/user/orgs` needs `read:user` and `read:organization` (`:1218`). So both answer 403. `GET /repos/search?uid=<id>&private=true` needs only the repository scope, and the coordinator verified live that it returns the same set, `keeper/keeper-config` included. Widening the grant would force every person to revoke it under the forge's *Settings › Applications* first (`docs/account.md:1033-1035`; RForge §3). |
| A GitHub sign-in | **absent** | keeper has no GitHub client, and no OAuth App is registered for it. The device flow needs a public `client_id` and no secret (RForge §2). |
| The makistack token proxy | **present, open PR, not RFC 8693** | makistack#863: `POST /v1/token` with the person's ZITADEL JWT as Bearer. The answer is a one-hour **installation** token, `ghs_…`. Its runbook says "`gh api /user` answers 403", so `/user/repos` cannot list with it. It is deployed on electra at `:8455` from the branch. `/healthz` reports `tgbot: unconfigured` and `tgdev: configured`, and `tgdev` is installed nowhere. So today a `tgbot` request answers `app_unconfigured` and a `tgdev` request answers `not_installed`, and no token is served (PR body, *Already live on electra*; `broker.py:312-321`). |
| Opening the add-folder form prefilled from a known remote | **present** | Epic 84's drive offer: `prefill` state in `src/components/settings/sync-section.tsx:342`, handed to `AddFolderForm` at `:526-528`. The form takes `prefill?: DriveOfferVm` (`src/components/sync/add-folder-form.tsx:1236-1247`). `DriveOfferVm` carries the name, remote, branch and a `credential` of `account`, `own` or `none` (`keeper-core/src/org_account/state.rs:98-123`). |
| A drive that signs in through something other than the keychain or the account | **absent** | `credential_source_row` accepts `None`/`"keychain"` and `"account"`, and refuses everything else (`keeper-core/src/registry.rs:1885-1899`). TypeScript: `CredentialSource = "keychain" \| "account"` (`src/lib/ipc/client.ts:7813`). `drive_credential` answers only `account:<id>` rows (`keeper/src/account_ipc.rs:2505-2517`). |
| Deciding whether a folder may take a drive | **present, private** | `folder_accepts`: empty, or a clone whose `origin` is the drive's remote (`keeper/src/account_restore.rs:626-640`). It is `#[cfg(desktop)]` and private to the restore. |
| Knowing that a repository is already a drive here, or on another device | **present in parts** | `normalize_remote` lower-cases the scheme and host and drops userinfo, a trailing `/` and `.git` (`keeper-core/src/org_account/settings_sync.rs:404`). `drives.toml` records name the devices that use each drive, and offers carry them as `devices` (`state.rs:121-122`). |
| Sending a GitHub token on git and LFS | **present for `gho_`, unverified for `ghs_`** | keeper-sync sends a drive token as the Basic user name with an empty password on git (`keeper-sync/src/credential.rs:58-62`) and as `Basic base64("<token>:")` for LFS (`:71-75`). The coordinator verified live with an OAuth token on a private repository: `git ls-remote` ok, LFS batch 200, no auth 401. The broker's runbook documents `x-access-token:<token>` for installation tokens. Token-as-user-name has not been tried with a `ghs_` token, because no installation exists yet (DW-325). |
| A place in the descriptor for more forges and a broker | **absent** | `AccountDescriptor` is `deny_unknown_fields` with `version`, `id`, `name`, `auth` and `config` (`keeper-core/src/org_account/descriptor.rs:39-49`), so a new table must enter the schema. `replaces` compares only `id`, `auth` and `config` (`:413-415`), so a new table can change without a sign-out. A client secret is refused anywhere in the file (`SECRET_REFUSAL`, `:517-518`). |
| Disclosing GitHub and the broker in Settings › About | **absent** | `org_account_egress` names only the descriptor's identity provider, the setup source, the repository, its `api_base` and, in `oauth` mode, the forge's sign-in (`keeper-core/src/egress.rs:279`; wired at `keeper/src/ipc.rs:11928-11932`). `api.github.com` appears today only as keeper-syncd's version check (`docs/egress.md:44`). |
| Opening GitHub's verification page | **present** | `Platform::open_url` (`keeper-core/src/platform.rs:49`). |
| Adding a drive the way the form does, from Rust | **present** | `sync_profile_save` (`keeper/src/sync_ipc.rs:1303`); the iOS folder is `phone_shaped_request`'s (`:1377`); the account sync is started by `note_local_change` (`keeper/src/account_ipc.rs:892`). |

## The one sentence

**keeper can add a drive only from a URL someone types or from another device's offer, and has no way to ask a forge what repositories a person has.** The GitHub token proxy the owner named exists only as an open makistack PR speaking its own protocol, and electra's Forgejo refuses the obvious listing endpoints under the grant keeper holds.

**The fix:**
- **Sources.** A new `forges` module in keeper-core names the person's repository sources: the account's forge, and GitHub, reached through makistack's broker, a connection of the person's own, or both. The descriptor can name more GitHub sources under `[[forges]]`. A second Forgejo is accepted there but has no token path yet (DW-330).
- **Listing.** It lists each source with the token that source can give: the `github-broker` installation tokens per owner, the account's forge token through `/repos/search`, or a device-flow token through `/user/repos`. Each repository is marked with where it already syncs.
- **Adding.** A repository is added one at a time through the existing form, or several at once under a base folder. Each added drive signs in through the source it came from.

## What earlier epics decided, and what this epic amends

| The earlier decision | What it said | What this epic needs | The amendment |
| --- | --- | --- | --- |
| **AD-315** (epic 82) and `credential_source_row` (`registry.rs:1885-1899`) | a drive or provider uses the keychain or the account, and nothing else is stored | A drive added from GitHub signs in with GitHub's token, which is not the account's. | **Extended for drives (AD-336, A1).** `sync.credential_source.<pid>` also accepts `forge:<source-id>`, stored verbatim. Bot providers still refuse it. |
| **AD-326** (epic 85) | a credential choice travels in `settings.<device>.toml` with the value `"account"` | A restored device must also find its GitHub drives signing in through GitHub. | **Extended (AD-336).** `forge:<source-id>` travels as is. It is applied from the file only when the drive's remote origin equals that source's `web_base` origin. |
| **AD-330** (epic 85) | a drive on the account forge's host that uses the account gets the forge's own token | Repositories from the account's forge. | **Held.** They are added with `account:<id>`, so epic 85's routing gives them the forge token. |
| **AD-323** (epic 84) | offers open the add-folder form prefilled, and the person picks the folder | A repository row's *Add…* | **Held and reused (AD-336).** The prefill gains an optional `credential`. |
| **AD-329** (epic 85), `folder_accepts` | a restored drive takes a folder only when it is empty or already that clone | The batch add's folder check. | **Reused (AD-336).** `folder_accepts` becomes `pub(crate)`. |
| **Epic 82's descriptor** (`descriptor.rs:39-49`, `:413-415`) | five top-level fields and `deny_unknown_fields`; editing `auth` or `config` replaces the account | Forges and a broker named by the operator. | **Extended (AD-333).** Two optional tables, `[[forges]]` and `[github_broker]`, both `deny_unknown_fields`. `replaces` is **unchanged**: editing them never signs anyone out. |
| **AD-53** | the egress list is computed from what is configured | GitHub and the broker become destinations. | **Extended (AD-333).** Their hosts are listed only while in use. |
| **AD-62 and D-3** | one clock per host process | Device-flow polling. | **Held.** Polling is a cancellable sleep inside a flow the person started, and nothing is polled in the background. |
| **Epic 85's rejected alternative:** RFC 8693 token exchange | "bigger, needs ZITADEL configured for each audience" | A broker. | **Still rejected, and dropped from this epic's contract (A2).** ZITADEL cannot mint upstream tokens (RForge §4). The one broker that exists speaks its own protocol, and keeper speaks that one. |
| **AGENTS.md:** keeper is "a **client only**" | no server-side components | A broker. | **Held (AD-337, D-28).** The broker is makistack's. keeper only calls it. |

## Decisions this epic takes

The coordinator amended the contract three times during the build wave and once after the review wave (*Contract amendments*, below). The rules below include A1–A4.

- **AD-333: Repository sources.**

  **Binds:** FR-734, FR-735, FR-744; NFR-103; Story 86.1.

  **Prevents:**
  - a forge list that lives in the shell or the webview (AD-40);
  - GitHub hidden behind an organisation account it does not need;
  - a descriptor edit that signs the person out;
  - a token sent to a host the operator did not name, or over plain http;
  - a GitHub connection sent to whatever `api_base` a descriptor names (A4, B1), or to a host the person never saw on the setup sheet;
  - a descriptor entry that is accepted and then silently absent (A4, m10);
  - a source shown with a control that cannot work (AD-27).

  **Rule** (a new keeper-core module, `forges`, not under `org_account`, because GitHub works without an account):

  ```rust
  pub enum ForgeKind { Github, Forgejo }                  // serde lowercase; ts ForgeKindVm
  pub enum TokenVia { Broker, AccountForge, DeviceFlow }  // ts TokenViaVm
  pub struct ForgeSource { pub id: String, pub kind: ForgeKind, pub name: String, pub web_base: String, pub api_base: String,
    pub client_id: Option<String>, pub via: TokenVia }
  pub fn sources(descriptor: Option<&AccountDescriptor>, builtin_github_client_id: Option<&str>) -> Vec<ForgeSource>;
  pub const BUILTIN_GITHUB_CLIENT_ID: Option<&str> = None; // keeper's own public OAuth App; set when the owner registers one
  ```

  - **The account's forge.** When the descriptor's `config.auth` is `oauth` and it has an `api_base`, the source is `id = "account-forge"`, kind Forgejo, named after the account, with `web_base` taken from the forge's issuer, else from its `token_url`, so that a forge under a subpath keeps its path (A4, m9), and `via = AccountForge`, always.
  - **`[[forges]]`** (optional; `deny_unknown_fields`): `{ kind, id?, name?, web_base?, api_base?, client_id? }`.
    - For GitHub, the defaults are `web_base = https://github.com`, `api_base = https://api.github.com` and `id = "github"`.
    - **A Forgejo entry is refused** (A4, m10): "keeper lists only the account's own Forgejo; remove this [[forges]] entry." None of AD-334's legs can get a token for a second Forgejo: Forgejo has no device flow, the broker serves GitHub only, and the account's forge token never goes to another host. The build wave had validated such an entry and then left it out of `sources()` with no sentence, which AD-27 forbids (DW-330, resolved by the refusal).
    - **The API stays with its forge** (A4, B1). A `web_base` on github.com requires `api_base = https://api.github.com`, and any other `web_base` requires an `api_base` on the same host. Otherwise a descriptor could name `api_base = https://collector.example` and receive the person's GitHub connection.
    - Every URL must be https (loopback excepted, for tests) and carry no userinfo, query or fragment. A client secret is refused as anywhere in the descriptor. Ids match `[a-z0-9-]{1,32}`, are unique, and may not be `account-forge`. Origins are compared parsed, so a trailing `/` does not matter (A4, m11).
    - A GitHub entry with neither a broker nor a client id is not a source.
    - `sources()` gives keeper's built-in client id only to a source whose `web_base` and `api_base` are both github.com's defaults (A4, B1).
  - **`[github_broker]`** (optional; `deny_unknown_fields`): `{ url, app? }`.
    - `url` must be https.
    - `app` is the GitHub App to prefer when several of the person's grants cover one owner.
    - When it is present, the `github` source exists with `via = Broker`. A `client_id`, from `[[forges]]` or built in, becomes the fallback used only when the broker says the person has no grants.
    - A `github` entry in `[[forges]]` must then point at github.com, because installation tokens are github.com's.
  - **GitHub without an account.** A `github` source with `via = DeviceFlow` exists whenever `BUILTIN_GITHUB_CLIENT_ID` is set, account or not. It ships as `None`, so today there is no GitHub source without a descriptor that names one (DW-323).
  - **The setup sheet** gains the fact "Gets GitHub access from `<host>`" (`AccountSetupVm.broker_host: Option<String>`), and lists every other host a repository token would be sent to (`AccountSetupVm.forge_hosts: Vec<String>`, A4, B1): the web and API hosts of each GitHub source the descriptor adds, in the switcher's order, once each. The account's own forge is already `repo_host`. So the person sees every such host before *Continue*.
  - **`AccountDescriptor::replaces` is unchanged.** Forges and the broker are neither sign-in nor repository.
  - **Egress (AD-53).** GitHub's hosts and the broker's host join the computed list only while in use. Since A4 (t1), `egress::forge_egress(platform, descriptor, builtin_github_client_id)` applies the in-use test itself, reading whether each source has a device-flow connection, so the shell only calls it. It returns `EgressKind::Forge` rows:
    - while the descriptor names `[github_broker]`: the broker's host (*GitHub access broker*) and `api.github.com` (*GitHub repositories*);
    - for each source with a device-flow connection: its web and API hosts (*`<name>` repositories*).

    The account's own forge is already an `EgressKind::Account` row. The device-code request reaches `github.com` before a connection exists, so that row appears only once the flow the person started succeeds. `docs/egress.md` says so.

- **AD-334: One way to get a forge token.**

  **Binds:** FR-736, FR-737; NFR-101, NFR-102; Story 86.2.

  **Prevents:**
  - a token on disk outside the keychain, in a log, over IPC or in a URL;
  - a token sent anywhere but its own forge or the descriptor's broker;
  - a timer that polls GitHub;
  - a broker token asked for with more permission than the job needs, or than the person's grant allows;
  - a failure for one GitHub owner blanking the whole list;
  - a cached token or list answering for someone who has signed out, or for the next person (A4, M1, M2);
  - a *Cancel* or *Disconnect* undone by a poll already in flight (A4, M4).

  **Rule:** `forges::tokens::forge_token(platform, http, source, account: Option<&AccountDescriptor>) -> Result<String, ForgeError>` has three legs.

  1. **`Broker`** (GitHub only) speaks makistack's `github-broker` protocol, with the account's `oidc::access_token` as `Authorization: Bearer`.
     - `forges::broker::{whoami_request, parse_whoami, token_request(app, owner, repositories: Option<&[String]>, permissions: &[(&str, &str)]), parse_token, classify_error(status, body) -> ForgeError | OwnerNotice}`.
     - **Which grant for an owner** (A4, M3): among the grants that cover the owner, the repository and the permission asked for, the descriptor's `app` when one of them is its; otherwise the first in whoami order (`forges::broker::app_for`). This is the broker's own `policy.decide`, so keeper never asks for what the policy refuses.
     - **A listing token** asks for `permissions: {"metadata": "read"}` and no repositories, or for `{"contents": "read"}` when no grant covering the owner allows `metadata` (`broker::LIST_ASKS`, A4).
     - **A drive token** asks for `repositories: [<repo>]` and `permissions: {"contents": "write"}` when a covering grant allows write, else `{"contents": "read"}`, and that drive only downloads (A4). The owner and the repository come from the drive's remote, `https://github.com/<owner>/<repo>(.git)`.
     - **Caching, in memory only.** A token is keyed by the account id, the session's `sub`, and (app, owner, repositories, permissions), and reused until `expires_at` minus 5 minutes. The whoami answer is kept, under the same identity, until an error or a refresh. A cache hit needs a live session (A4, M1).
     - **Classification** (`classify_error`, as built after A4):
       - a **401**, whatever the error, gives `NeedsSignIn`: "Sign in again to reach GitHub through your organization." A 401 straight after a fresh refresh of the sign-in token is `Refused` instead, "`<broker host>` did not accept your account's sign-in: `<detail>`", because signing in again cannot fix it (A4, m14);
       - `not_installed`, `app_unconfigured`, and `forbidden` or a 403 `github_refused` give per-owner notices (AD-335), as does an owner no grant names: "`<broker host>` gives you no access to `<owner>`'s repositories.";
       - `github_refused` with a 5xx status is `Unreachable`: "GitHub isn't answering `<broker host>` right now." (A4, m4);
       - no grants at all is `ForgeError::NoAccess`, a state of its own, with "Your account has no GitHub access on `<broker host>`. Ask its administrator to add you." The source is `unreachable` with that sentence, and a cached list is never served for it (A4, m2);
       - a 400 `bad_request` gives `Internal`, and the source is unreachable with "keeper couldn't list GitHub's repositories.";
       - any other 5xx (`idp_unavailable`, `broker_error`) makes the source unreachable with "`<broker host>` can't answer right now."; any other status is "`<broker host>` refused keeper (HTTP `<status>`)."; an unreadable answer is "The GitHub broker sent an answer keeper could not read.";
       - no answer at all gives "Can't reach `<host>`.", naming the host that failed (A4, m2).

       A failing whoami is classified the same way, and nothing lists.
  2. **`AccountForge`:** the existing `oidc::forge_token` (`keeper-core/src/org_account/oidc.rs:1281`).
  3. **`DeviceFlow`** keeps a keychain item, `forge/<source-id>/<client_id>/session` (A4, m1), holding JSON `{access_token, refresh_token?, expires_ms?, login, client_id}`. One item per source and client, so a descriptor's own `client_id` never shares keeper's built-in connection. A read path never deletes an item.
     - When `expires_ms` is set, the token is refreshed 60 s early, one refresh per source at a time.
     - A `bad_refresh_token` or a 401 deletes the item and answers `NeedsConnect`.
  - `ForgeError::{NeedsConnect, NeedsSignIn, NoAccess(String), Unreachable(String), Refused(String), Internal(String)}`, each with a sentence.
  - **The device flow:** `forges::device_flow::{start(http, source) -> DeviceCode {user_code, verification_uri, expires_in, interval, client_id, device_code (private)}, poll(http, source, &DeviceCode, cancel: &AtomicBool) -> Result<StoredForgeToken>}`, followed by `GET /user` for the login. The shell stores the result.
    - `start` refuses a `verification_uri` that is not https (loopback http only at `web_base`'s own origin) on `web_base`'s host or a subdomain of it: "`<host>` gave keeper an approval page elsewhere, so keeper does not open it." (A4, m5). `DeviceCode` carries the client id that started it (A4, m12).
    - `interval` is clamped to 1–60 s (default 5), `slow_down` included, and `expires_in` to at most 1,800 s, with saturating arithmetic (A4, M5).
    - Polling waits `interval` seconds between requests, 5 more after each `slow_down`, never past 60. It is a cancellable sleep inside the flow the person started, and `cancel` is checked again after the token answer and after `/user` (A4, M4).
    - When `/user` fails after the token arrives, the session is stored with an empty login, filled in at the next listing (A4, m13).
    - The terminal errors end the flow, each with a sentence: `expired_token` or `token_expired`, `access_denied`, `device_flow_disabled`, `incorrect_client_credentials`, `incorrect_device_code` or `bad_verification_code`, and `unverified_user_email`.
  - **Disconnect** (`forge_disconnect`) deletes the keychain item. GitHub has no revocation endpoint a client without a secret can call, so the sheet says "Also remove keeper under GitHub › Settings › Applications if you want", with the source's `appsUrl` (DW-327).
  - **Forget this account** removes the device-flow sessions of the sources the descriptor names. keeper's built-in GitHub connection stays until the person disconnects it.
  - **One identity at a time** (A4, M1, M2). `forges::forget_identity()` clears the listing cache, the broker's whoami and token caches, and the device-code state. The shell calls it, and clears its stored per-source errors, on sign-in, sign-out, *Forget this account* and setup confirm.

- **AD-335: The listing.**

  **Binds:** FR-736, FR-738, FR-739, FR-740; NFR-102, NFR-104; Story 86.3.

  **Prevents:**
  - a listing that needs a wider Forgejo grant, and with it a revocation on every device;
  - an organisation's repositories hidden with no word about why;
  - an unbounded crawl of an account with thousands of repositories;
  - a list written to disk, or served to someone other than the identity that fetched it (A4, M2);
  - "already syncing" decided by comparing two spellings of one URL;
  - a repository added as a two-way drive whose pushes will be refused forever (A4).

  **Rule** (`forges::listing`: pure parsing and request builders; the shell runs the HTTP):
  - **The model:**

    ```rust
    pub struct ForgeRepo { pub full_name: String, pub owner: String, pub name: String, pub description: Option<String>, pub private: bool,
      pub fork: bool, pub archived: bool, pub template: bool, pub mirror: bool, pub default_branch: String, pub clone_url: String, pub web_url: String,
      pub updated_ms: Option<i64>, pub size_kb: Option<u64>, pub can_push: bool }
    ```
  - **Download-only** (A4). A repository keeper cannot push to is marked `pull_only` when `!can_push || archived || mirror`, with one sentence: "You can only read this repository, so keeper only downloads it.", "This repository is archived, so keeper only downloads it." or "This repository is a mirror, so keeper only downloads it." On the broker path, `can_push` is true only when a grant covering that owner and repository has `contents: write`, and GitHub's `permissions.push`, when present, is not false.
  - **GitHub through the broker** (`via = Broker`):
    - Ask `GET <url>/v1/whoami`.
    - Then, for every owner in the grants (deduplicated, A→Z, at most 20, `OWNER_CAP`), mint a listing token and page `GET https://api.github.com/installation/repositories?per_page=100` by following `link rel=next` (`github::parse_installation_repos` → `{total_count, repositories}`, the same `Repository` fields as `parse_repos`). A listing makes at most 60 requests in all, counting the whoami, each mint and each page (`REQUEST_CAP`, A4, m6).
    - "You" is an owner whose `owner.type` is `User`.
    - **A failure for one owner becomes that owner's notice, and the other owners still list:**
      - `not_installed`: "keeper's GitHub app `<app>` isn't installed on `<owner>`." There is no link, because the app's slug is not known;
      - `app_unconfigured`: "GitHub access for `<owner>` isn't set up on `<broker host>` yet.";
      - `forbidden` or a 403 `github_refused`: "`<broker host>` refused `<owner>`: `<detail>`.";
      - an owner no grant names: "`<broker host>` gives you no access to `<owner>`'s repositories.";
      - a 401 on that owner's listing evicts the token and gives "GitHub didn't accept the token `<broker host>` gave keeper for `<owner>`; refresh to try again." (A4, m3). A 5xx `github_refused` makes the source unreachable (A4, m4).
    - **No grants** (`subject: null`) is its own state, `NoAccess` (A4, m2): the source is `unreachable` with the no-access sentence, and a cached list is never served. When a device-flow client id exists (`canConnect`), the source is `notConnected` with *Connect GitHub* instead.
  - **GitHub through a device-flow connection** (`via = DeviceFlow`):
    - `GET https://api.github.com/user/repos?affiliation=owner,collaborator,organization_member&visibility=all&per_page=100&sort=full_name`, with `Accept: application/vnd.github+json`, `X-GitHub-Api-Version: 2022-11-28` and `Authorization: Bearer`.
    - It never sends `type`, which returns 422 beside `affiliation` or `visibility`.
    - It follows `link rel=next`, serially.
    - "You" is the connected login.
    - Parsers: `github::parse_repos`, `github::next_link`, `github::classify_error(status, headers, body) -> ListNotice | ForgeError`.
  - **The account's forge** (`via = AccountForge`):
    - First, `GET <forge issuer>/login/oauth/userinfo` with the forge token, for the `sub` (`forgejo::parse_userinfo_sub`).
    - Then `GET <api_base>/repos/search?uid=<sub>&private=true&limit=50&page=N&sort=alpha&order=asc`.
    - The body is `{ok, data}` (`forgejo::parse_search`). The page count comes from `X-Total-Count` (`forgejo::pages`), and paging continues while a page is full, even without that header (A4, m8). keeper builds each page against its own `api_base`, because Forgejo's `Link` points at its ROOT_URL.
    - "You" is an owner equal to the forge login. Other owners are grouped by login, organisations and collaborators alike (DW-328).
  - **Caps:** GitHub 10 pages of 100 (per owner on the broker path, at most 20 owners and 60 requests per listing, 1,000 repositories overall); Forgejo 20 pages of 50. Repositories are grouped before the cap cuts, so "you" stays first (A4, m7). Past the cap, `truncated = true` (DW-329).
  - **Notices** are sentences composed in Rust:
    - **a restricted organisation:** "Some organizations haven't approved keeper, so their private repositories are hidden." It links `https://github.com/settings/connections/applications/<client_id>`. keeper detects it from a 403 whose message contains "OAuth App access restrictions";
    - **single sign-on:** "Repositories of organizations that use single sign-on are hidden until you authorize keeper for them." It links the `X-GitHub-SSO: url=` value when present;
    - **truncated:** "Only the first `<n>` are listed; search looks only through those.", where `<n>` is "1,000" at the repository cap. Search runs in the client over what was fetched. (The contract's second sentence, "Showing the first 1,000 repositories. Search to find others.", was not built.);
    - **offline:** the failing request's own sentence, which names the host that failed (for example "Can't reach `<host>`."), then " Showing the list from `<HH:MM>`." when a cached list exists (A4, m2);
    - the per-owner broker notices above;
    - **too many owners** (A4, m6): "`<broker host>` gives you more owners than keeper lists at once; only the first 20 are listed."
  - **The cache** lives in memory, one list per source per identity (the account id and the session's `sub`) per process, and a refresh refetches. A hit needs a live session, and `forges::forget_identity()` empties it (A4, M2). Nothing is written to disk.
  - **Marking** (pure, `forges::mark`) compares each repository with this device's drives and the manifest:
    - `added_as`: the names of this device's drives whose `normalize_remote(remote_url)` equals `normalize_remote(clone_url)`;
    - `elsewhere`: the device slugs of `drives.toml` records with that remote, this device excluded.
  - **Grouping:** "you" first, then organisations and other owners A→Z. Each repository sits in its owner's group.

- **AD-336: Adding.**

  **Binds:** FR-741, FR-742, FR-743; NFR-101; Story 86.4.

  **Prevents:**
  - a drive created over someone's files;
  - a batch that fails whole for one bad row;
  - a second drive of one repository in one folder, or through the batch at all (A4);
  - a drive created inside another drive, or around one, or at a relative path (A4);
  - GitHub's token sent to a drive on another host;
  - a travelling credential choice binding a drive it was not made for, or one this device cannot get a token for (A4, m15).

  **Rule:**
  - **One at a time.** A row's *Add…* opens the existing add-folder form, prefilled with the drive name (the repository's name), the remote (`clone_url`), the branch (`default_branch`), the direction (`pullOnly` for a download-only repository, A4) and the source's `credential`. The path is epic 84's offer prefill, whose type gains an optional `credential`. The person picks the folder.
  - **Several at once:** `forge_repos_add(ForgeAddReq { source_id, base_folder: Option<String>, repos: Vec<ForgeAddItem { full_name, drive_name, folder: Option<String> }> }) -> Vec<ForgeAddResultVm { full_name, profile_id: Option<String>, sentence: Option<String> }>`.
    - **Desktop:** each folder is the item's `folder`, or else `<base_folder>/<drive_name>`. It must be a full path ("Choose a full folder path."; a leading `~/` expands to the home folder). It must be absent (keeper creates it), empty, or a clone whose origin normalises to the repository. That is `account_restore::folder_accepts`, made `pub(crate)` and reused, which ignores `.DS_Store` and `Icon\r` (A4). Otherwise the result says "`<path>` holds other files.".
    - **Never on or around another drive** (A4). A folder that is already some drive's folder, lies inside one, or contains one is refused. Paths are compared canonicalized, and case-insensitively on macOS.
    - **iOS:** the app container's folder, as `phone_shaped_request` assigns it.
    - **Each add** goes through `sync_profile_save`'s path: validation, the sessions-root refresh and the recordings index. Then it sets the credential source, then calls `note_local_change`. A download-only repository is added with the direction `pullOnly`.
    - **Partial success is a success.** Each result names its repository. Undoing a failed row removes the empty folders it created, deepest first (A4).
    - **One batch at a time.** Batches are serialized by a process-wide lock (A4).
    - **The duplicate guard** (A4): a repository that already syncs on this device (`added_as` non-empty) is refused with "Already syncing here as `<name>`.". A second drive of one repository is possible only through the single form, since epic 85 allows two drives on one remote.
  - **The credential choice depends on the source, and Rust decides it** (A4): `ForgeSourceVm.credential` is the exact value to store:
    - a repository on the account's forge gets `account:<id>` (sent as `account`), which epic 85's AD-330 already routes to the forge token by host;
    - GitHub, or any other source, gets `forge:<source-id>`.
  - **`sync.credential_source` accepts `forge:<source-id>`** (A1), with the id `[a-z0-9-]{1,32}`, and stores it verbatim.
    - `sync_credential_source_get` returns it as is.
    - In TypeScript, `CredentialSource = "keychain" | "account" | \`forge:${string}\``.
    - The add-folder form shows *Sign in with `<source name>`* for such a prefill, and hides that choice while the remote's host is not the source's (A4).
    - A bot provider refuses `forge:`.
    - `sync_credential_source_set` refuses `forge:<id>` for a drive whose remote fails `forges::source::remote_on_source`: "This drive's repository isn't on `<host>`." (A4).
  - **`drive_credential`** answers a `forge:<id>` drive with `forge_token` for that source. The drive's remote must pass `remote_on_source`, or the answer is `NeedsSignIn` and no token leaves.
  - **It travels.** The value moves to and from `settings.<device>.toml` unchanged, beside `account`. A value pulled from the file applies only when the drive's remote passes `remote_on_source`, as epic 85 binds `account` only on the account's own hosts, **and** this device can get that source's token: a device-flow session for it exists, or it is broker-served and the account is signed in (A4, m15). Otherwise it stays in the file, unapplied, and the drive keeps the credential it has.

- **AD-337: The broker is documented, and keeper ships none.**

  **Binds:** FR-745; Story 86.6.

  **Prevents:**
  - an operator guessing the wire;
  - a promise that keeper reaches every organisation the person belongs to, when the broker's policy and the GitHub App's installations decide;
  - a broker written into keeper.

  **Rule:** `docs/account.md` gains a section, *GitHub through your organization's broker*, with:
  - the descriptor's `[github_broker]` fields;
  - the `github-broker` protocol exactly as keeper uses it: whoami, and the token requests for listing and for drives;
  - the errors, and the sentences keeper shows for them;
  - the facts that tokens are one-hour installation tokens held only in memory, and that what keeper reaches is decided by the broker's policy and by where the GitHub App is installed;
  - a link to makistack's runbook.

  No broker code ships in keeper.

- **AD-338: IPC, registered on every target.**

  **Binds:** FR-734…FR-742; Stories 86.1–86.5.

  **Rule:**
  - `forges_list() -> Vec<ForgeSourceVm>`.
    - `ForgeSourceVm { id, kind: ForgeKindVm, name, host, via: TokenViaVm, state: ForgeStateVm, login: Option<String>, sentence: Option<String>, credential: String, canConnect: bool, appsUrl: Option<String> }`. The last three are A4's:
      - `credential`: the exact credential-source value a drive from this source stores (`"account"` or `"forge:<id>"`);
      - `canConnect`: a device-flow client id exists for this source, whatever its `via`;
      - `appsUrl`: `https://github.com/settings/connections/applications/<client_id>` for a device-flow GitHub source, else null.
    - `ForgeStateVm = connected | notConnected | needsSignIn | unreachable`. `NoAccess` is `unreachable` with its own sentence (A4).
  - `forge_repos(source_id, refresh) -> ForgeReposVm { source_id, repos: Vec<ForgeRepoVm>, owners: Vec<ForgeOwnerVm { login, is_you, count }>, notices: Vec<ForgeNoticeVm { sentence, link: Option<String> }>, truncated, fetched_ms: Option<number> }`.
    - `ForgeRepoVm` holds the `ForgeRepo` fields in camelCase, with `updatedMs` and `sizeKb` as `number | null`, plus `addedAs: string[]`, `elsewhere: string[]`, and A4's `pullOnly: bool` and `pullOnlySentence: Option<String>`.
  - `forge_connect_start(source_id) -> DeviceCodeVm { userCode, verificationUri, expiresIn }`. The shell holds the device code.
  - `forge_connect_open(source_id)` opens the held code's `verificationUri` through `Platform::open_url`. Nothing else opens a browser (A3).
  - `forge_connect_wait(source_id) -> ForgeSourceVm` resolves on success or on a terminal error. A cancel resolves with `notConnected`.
  - `forge_connect_cancel(source_id)` and `forge_disconnect(source_id) -> ForgeSourceVm`.
  - `forge_repos_add(req: ForgeAddReq) -> Vec<ForgeAddResultVm>`. One batch runs at a time (A4).
  - `forge_default_base_folder() -> Option<String>`: `None` on iOS. On desktop it is `sync.drive_folder`, else `~/keeper/git` (A5, owner, 2026-09-24: "setup in settings what would be the default folder"; it replaced "the parent of the most recently added drive, else `~/Drives`", which put new drives beside `/Volumes/merope`). The sheet decides "desktop" from the platform, not from this answer (A4).
  - `sync_drive_folder_get() / sync_drive_folder_set(folder: Option<String>) -> Option<DriveFolderVm { path, chosen }>` back Settings › Sync › *New drives go in* (A5).
  - `sync_credential_choices(remote_url) -> CredentialChoicesVm { account, forges }`, each a `CredentialChoiceVm { value, label, detail }` authored in Rust (A5). The account is offered, saved (`sync_credential_source_set`) and handed to git (`drive_credential`) only for a remote on one of its own origins (`AccountDescriptor::serves_remote`): the hesperia report showed *Use my makistack account* on a github.com drive, which sent the account's sign-in to GitHub.
  - `AccountSetupVm` gains `forgeHosts: string[]` beside `brokerHost` (A4, B1).

- **UX-DR120: Browse repositories.**

  **Binds:** AD-335, AD-336, AD-338; FR-734, FR-736…FR-742; Story 86.5.

  **Prevents:**
  - a button that opens an empty sheet;
  - a disabled source with no reason given (AD-27);
  - a hidden organisation with no word about why;
  - a batch that silently overwrites a folder;
  - a repository added twice by accident, or added as a drive that can never push (A4);
  - a source that cannot recover without a restart (A4).

  **Rule** (the design lane owns the details and DESIGN.md conformance):
  - **Entry points:** *Browse repositories…* beside the heading of Settings › Sync's permanent *Add a folder* form (`SYNC_ADD_TITLE`, `add-folder-form.tsx:131`), and in the Sync pane's empty state and its add affordance. Both are **absent** when `forges_list` is empty.
  - **The sheet:** a wide right sheet, *Add drives from your repositories*.
    - A segmented source switcher sits at the top, one segment per source (name · host), and remembers the last one used.
    - **Source states:**
      - **`notConnected` with `canConnect`** (A4: whatever `via` says): the sentence "See your GitHub repositories and those of your organizations. keeper asks GitHub for read access to them and to your organizations' names." and *Connect GitHub*. Connecting shows the user code large and in mono, with *Copy code*. *Open github.com* copies the code and calls `forge_connect_open`. "Waiting for you to approve keeper on GitHub…" shows with a spinner and *Cancel*. A failed open or copy shows a sentence, and the code stays selectable (A4).
      - **`needsSignIn`:** the account's own sentence and *Sign in*. Once the sign-in resolves, the sheet retries the listing with `refresh = true` (A4).
      - **`unreachable`:** Rust's sentence (the no-access sentence among them) and *Try again*, which retries the listing with `refresh = true` and then refreshes the sources (A4).
      - **Disconnect** shows while `canConnect` and connected, with the note linking `appsUrl`, hidden when it is null (A4).
    - **Toolbar:** search over name, owner and description; an owner filter (All, You, each organisation); the switches *Forks* and *Archived*, off by default; sort by *Recently updated* or *Name*; and refresh, whose tooltip is the fetch time.
    - **Rows,** grouped by owner under sticky headers with counts. Each row has:
      - a checkbox;
      - the name, with `owner/` muted in the All view;
      - a lock for a private repository;
      - the chips Fork, Archived, Template and Mirror;
      - a one-line description;
      - the updated time and the size;
      - a state on the right: *Syncing here as `<name>`* (with a check, the checkbox disabled, and the name linking to the drive), else *On `<devices>`* (muted), else nothing, plus *Add…*;
      - for a download-only repository, `pullOnlySentence`, muted, with an icon (A4).
    - **Notices** sit above the list, each with its link: restricted organisations, single sign-on, truncation, offline, and the per-owner broker notices.
    - **Keyboard:** Up and Down move from any element of a row, Space toggles, Enter is *Add…*, and ⌘A anywhere in the sheet except a text field selects every visible repository not already added (A4).
    - **The selection** is what is selected, still listed and not added. The footer's count and the batch both use it, and it resets when the source changes. The owner filter resets when its owner goes away (A4).
    - **Each answer belongs to its source.** A listing that arrives for a source no longer shown is dropped (A4).
    - **A sticky footer** appears while something is selected: *`<n>` selected*, *Clear*, and the primary *Add `<n>` drives…*.
  - **The batch step** is a second view of the same sheet; *Back* returns.
    - "Where should they go?": the base folder with *Choose folder*, defaulting to `forge_default_base_folder`. It is absent on iOS, and present on every desktop even when there is no default (A4).
    - A preview row per repository, `<base>/<name>`, with the name editable. A conflict (the folder holds other files, or two rows share a name) shows inline and blocks only that row. A download-only row is marked "Download only". A row's result clears when its name changes (A4).
    - The fact "Signs in with your `<source name>` connection".
    - *Add `<n>` drives* runs the add. Each row shows a spinner, then a check or its sentence.
    - When it is done: "Added `<n>` drives. They start syncing now." and *Done*. Failures stay listed with their sentences.
  - **States:** loading shows skeleton rows. An empty list says "No repositories here yet.", and an empty search says "No repositories match.".
  - **The setup sheet** lists `forgeHosts`, every host that will receive a repository token, among its facts, beside "Gets GitHub access from `<host>`" (A4, B1).
  - **`dev/mock-shell.ts`** covers every state:
    - two sources: Forgejo with the `keeper` organisation, and GitHub with two organisations and a restricted-organisation notice;
    - about forty repositories, some added here, some elsewhere, some forks and archived ones, and some download-only;
    - the connect flow, including a second wait that settles the first;
    - a stored error that *Try again* clears;
    - a batch whose results include one conflict, with added drives that carry every field (A4).

### Alternatives this plan rejected

- **RFC 8693 token exchange against a broker named in the descriptor** (the contract as first frozen). The only broker that exists, makistack#863, speaks another protocol, and ZITADEL cannot be one: its `audience` only narrows and it refuses `resource` (RForge §4). Two conventions for one job is one too many. It was dropped (A2).
- **Widening Forgejo's grant to `read:user read:organization`** so that `/user/repos` and `/user/orgs` work. Every person would first have to revoke keeper under the forge's *Settings › Applications* on the web, because Forgejo refuses a changed scope for an existing grant (RForge §3). `/repos/search` returns the same set under the grant keeper holds.
- **Listing GitHub with the broker's token through `/user/repos`.** Installation tokens get 403 there, as the runbook says. `/installation/repositories` per owner is the endpoint that answers.
- **A GitHub App user-to-server flow in keeper**, with `ghu_` tokens. It needs a client secret for the web flow, or the device flow of an App the owner would register and install anyway. The broker already holds the App, and the device flow of an OAuth App is the fallback.
- **Keeping the GitHub list on disk between launches.** The list is cheap to fetch: 1,000 repositories cost 10 requests of a 5,000-per-hour budget (RForge §5). A file would be one more place a person's private repository names sit.
- **Conditional requests (ETag) on GitHub.** They pay only when polling, and keeper does not poll. Forgejo sends no ETag at all (RForge §3).
- **A broker in keeper** (a local token cache service, or a keeper-hosted proxy). keeper is a client only.
- **A timer that refreshes the list.** It would be a second clock (AD-62, D-3). The list is fetched when the sheet opens and on *Refresh*.
- **Adding every selected repository with the single form, one after another.** The person would pick a folder per repository. A base folder with editable names is one decision for the whole batch.
- **Accepting a second Forgejo in `[[forges]]` and leaving it out of the sheet** (the build wave). Nothing told the operator or the person why it was missing, which AD-27 forbids, and the one place a descriptor fault has a sentence is validation. It is refused (A4).
- **Adding a read-only repository as a two-way drive and letting its pushes fail.** Every push would be refused forever and local edits would pile up unpushed. A download-only drive says so on the row (A4).
- **Asking the broker for `contents: write` on every drive and taking the refusal.** The policy's ceiling is known from whoami, so keeper asks for what a grant allows (A4).

## Contract amendments

The coordinator amended the frozen contract three times during the build wave and once after the review wave. The code is built to them. Where the contract and an amendment disagree, the amendment wins, and A4 wins over everything before it.

- **A1: `forge:<source-id>` on the wire** (the coordinator, while freezing Shell3's and Front3's signatures).
  - `credential_source_row` accepts `forge:<id>` and stores it verbatim, and `sync_credential_source_get` returns it.
  - TypeScript's `CredentialSource` gains `` `forge:${string}` ``.
  - The add-folder prefill gains an optional `credential`, and the form shows *Sign in with `<source name>`* for it.
  - Bot providers refuse `forge:`.
- **A2: The broker is makistack#863, and RFC 8693 is dropped** (the coordinator, on this lane's report).
  - `[token_broker] { url, forges }` becomes `[github_broker] { url, app? }`, and it serves only the `github` source. The account's forge is always `via = AccountForge`.
  - keeper calls `GET /v1/whoami` and `POST /v1/token`, with the account's ZITADEL access token as Bearer.
  - Listing: one `{metadata: read}` token per granted owner, then `GET /installation/repositories`. A failure for one owner is that owner's notice.
  - A drive gets `{repositories: [repo], permissions: {contents: write}}`. A4 narrows this: write only when a grant allows it, else read.
  - The device flow remains as the fallback, dormant while no client id is configured.
- **A3: `forge_connect_open(source_id)`** (the coordinator, from Shell3). It opens the held device code's `verificationUri` through `Platform::open_url`, and nothing else opens a browser. *Open github.com* copies the code and calls it.
- **A4: The review wave** (the coordinator, on ReviewCore86's B1, M1–M5, m1–m15 and t1–t5, and ReviewSurface86's findings 1–21; binding text in `local://epic86-fixwave.md`).
  - **Where a token may go (B1, m11).**
    - A `[[forges]]` entry whose `web_base` is github.com must have `api_base = https://api.github.com`: "`forges[N].api_base` must be https://api.github.com for github.com; keeper sends a GitHub token only to GitHub." Any other `web_base` needs an `api_base` on its own host, with the existing wording: "`forges[N].api_base` points at `<host>`, but the forge's `web_base` is `<web host>`; keeper sends codes and tokens only to the host the confirmation shows." Before this, a descriptor could send the person's GitHub connection to any `api_base` it named.
    - github.com is compared as a parsed origin with an empty path, so a trailing `/`, upper case and `:443` are all github.com.
    - `sources()` gives keeper's built-in client id only to a source whose two bases are both github.com's defaults.
    - `AccountSetupVm.forge_hosts` lists every other host a repository token would be sent to: the web and API hosts of each GitHub source that `sources()` keeps, deduplicated, in switcher order, so a broker adds `github.com` and `api.github.com`. The broker's own host stays in `broker_host`, and the account forge's in `repo_host`. The setup sheet shows them in its facts.
  - **A second Forgejo is refused (m10; DW-330 resolved).** A `[[forges]]` entry with `kind = "forgejo"` fails validation with "keeper lists only the account's own Forgejo; remove this [[forges]] entry." It was accepted and then silently left out of `sources()`.
  - **The account forge's `web_base` (m9)** comes from the forge's issuer, else from its `token_url`, so a forge under a subpath keeps its path. It no longer comes from the repository URL's origin.
  - **Caches belong to one identity (M1, M2; surface #1, #8).**
    - The listing cache and the broker's whoami and token caches are also keyed by the account id and the session's `sub`, and a hit needs a live session.
    - `forges::forget_identity()` clears them and the device-code state core holds. The shell calls it on sign-in, sign-out, *Forget this account* and setup confirm, and clears its stored per-source errors at the same moments.
    - `forge_repos(id, refresh = true)` always retries and replaces a stored error.
  - **The grant that fits (M3; t4).**
    - `app_for` chooses among the grants that cover the owner, the repository and the permission asked for, as the broker's `policy.decide` does. The descriptor's `app` wins only among those.
    - A drive asks for `contents: write` only when a covering grant allows it. Otherwise it asks for `contents: read`, and the drive only downloads.
    - On the broker path, `can_push` is true only when a covering grant has `contents: write` and GitHub's `permissions.push`, when present, is not false.
    - The test broker now refuses what the real one refuses.
  - **Download-only repositories (surface #7).**
    - `ForgeRepoVm.pullOnly` is `!can_push || archived || mirror`.
    - `pullOnlySentence` is one of "You can only read this repository, so keeper only downloads it.", "This repository is archived, so keeper only downloads it." and "This repository is a mirror, so keeper only downloads it."
    - Such a repository is added with the direction `pullOnly`, by the single form and by the batch alike. Its row shows the sentence, and the batch preview marks it "Download only".
  - **The device flow, hardened.**
    - The keychain item is `forge/<id>/<client_id>/session`, one per source and client, and a read path never deletes one (m1). A descriptor source with its own `client_id` no longer shares, or deletes, keeper's built-in connection.
    - `DeviceCode` carries the client id that started it (m12).
    - `cancel` is checked again after the token answer and after `/user`, so a poll in flight cannot undo *Cancel* or *Disconnect* (M4; t3).
    - `interval` is clamped to 1–60 s (default 5) and `expires_in` to at most 1,800 s, with saturating arithmetic (M5). A hostile device-code answer could panic keeper before.
    - `start` refuses a `verification_uri` that is not https on `web_base`'s host or a subdomain of it: "`<host>` gave keeper an approval page elsewhere, so keeper does not open it." (m5; surface #9).
    - When `/user` fails after GitHub has issued the token, the session is stored with an empty login and the login is filled in at the next listing (m13).
  - **Broker answers (m2, m3, m4, m14).**
    - Having no access is its own `ForgeError::NoAccess(sentence)`. The source is `unreachable` with that sentence, and a cached list is never served for it. It is no longer shown as an outage.
    - A 401 on `/installation/repositories` evicts that token and gives a notice worded for the broker.
    - `github_refused` with a 5xx status is `Unreachable`, not a refusal.
    - A broker 401 straight after a fresh token refresh is `Refused` with the broker's detail, not another sign-in.
    - An error names the host that actually failed.
  - **Listing bounds (m6, m7, m8; t2).**
    - The broker path lists at most 20 owners (`OWNER_CAP`) and makes at most 60 requests per listing (`REQUEST_CAP`: the whoami, a mint per owner and every page).
    - Repositories are grouped before truncating, so "you" stays first when the cap cuts.
    - Paging continues while a page is full, even without `X-Total-Count`, up to the cap.
  - **What a batch refuses (surface #3, #4, #6, #12, #13).**
    - A repository already syncing on this device: "Already syncing here as `<name>`." A second drive of a repository is added only through the single form.
    - A base or folder that is not a full path: "Choose a full folder path." A leading `~/` expands to the home folder.
    - A folder that is already some drive's folder, lies inside one, or contains one. Paths are compared canonicalized, and case-insensitively on macOS.
    - Two batches never run at once.
    - `folder_accepts` ignores `.DS_Store` and `Icon\r`, so an ordinary Finder folder counts as empty.
    - Undoing a failed row removes the empty folders it created, deepest first.
  - **The credential, decided in Rust (surface #10, #21; m15; t5).**
    - `ForgeSourceVm.credential` is the exact value a drive from that source stores (`account` or `forge:<id>`). TypeScript no longer computes it.
    - `forges::source::remote_on_source(source, remote_url)` is the one origin rule, and `drive_token`, the shell and the tests use it. `sync_credential_source_set` refuses `forge:<id>` for a drive whose remote fails it: "This drive's repository isn't on `<host>`." The add-folder form hides the choice while the remote's host is not the source's.
    - A pulled `forge:<id>` applies only where this device can get that token: a device-flow session for that source exists, or the source is broker-served and the account is signed in. Otherwise it stays in the file, unapplied, like any value this device cannot apply.
    - The origin tests pin the bypass spellings: `https://github.com.evil.com`, userinfo in the URL, a trailing-dot host, `:443`, an uppercase host and `http://`.
  - **The source VM (surface #2, #19).** `ForgeSourceVm` gains `canConnect` (a device-flow client id exists, whatever `via` says) and `appsUrl` (`https://github.com/settings/connections/applications/<client_id>` for a device-flow GitHub source, else null). The sheet offers *Connect GitHub* on `canConnect`, not on `via`, and shows *Disconnect* when `canConnect` and connected.
  - **Egress (t1).** The in-use test moves into core: `egress::forge_egress(platform, descriptor, builtin_github_client_id)` checks each source's device-flow connection itself, so the shell only calls it and the test covers it.
  - **The sheet (surface #1, #4, #5, #11, #15–#18).**
    - *Try again* and *Sign in* (once the sign-in resolves) retry the listing with `refresh = true`, then refresh the sources.
    - The selection is what is selected, listed and not added, and the footer's count and the batch both use it.
    - Each listing is tagged with its source, and an answer for another source is dropped.
    - A row's result clears when its name changes.
    - A failed open or copy shows a sentence.
    - Up and Down work from any element of a row, and ⌘A works anywhere in the sheet except a text field.
    - The owner filter and the selection reset when their owner or source goes away.
    - The base-folder field shows on every desktop, even when there is no default.

## Verified facts, with sources

Markers, as in the research digest:
- **[SRC]:** a cited document or source says it.
- **[LIVE]:** a probe saw it on 2026-09-24.
- **[INFERENCE]:** a conclusion.

### GitHub

- **`GET /user/repos`** [SRC https://docs.github.com/en/rest/repos/repos#list-repositories-for-the-authenticated-user]:
  - `affiliation` takes `owner,collaborator,organization_member`, where `organization_member` covers every repository on every team the user is on;
  - `type` returns **422** alongside `affiliation` or `visibility`;
  - `per_page` is at most 100;
  - pagination follows the `link` header, which is absent when there is one page;
  - unauthenticated it gives `401 Requires authentication` [LIVE].
- **Scopes** [SRC scopes-for-oauth-apps]:
  - `repo` grants private and organisation repositories, and there is no read-only repository scope;
  - `/user/orgs` needs `read:org` or `user`.
- **`/user/repos` takes only user tokens.** For a GitHub App it needs a *user* access token [SRC permissions-required-for-github-apps]. An installation token answers 403 on `/user` (makistack runbook, `github-broker.md` § *Things that behave differently*).
- **The device flow** [SRC https://docs.github.com/en/apps/oauth-apps/building-oauth-apps/authorizing-oauth-apps#device-flow]:
  - `POST https://github.com/login/device/code` with `client_id` and `scope` returns `device_code`, `user_code`, `verification_uri`, `expires_in` (900) and `interval` (5);
  - keeper then polls `POST https://github.com/login/oauth/access_token` with `grant_type=urn:ietf:params:oauth:grant-type:device_code`;
  - `slow_down` adds 5 s;
  - it needs **no client secret**, and a device-flow token is refreshed without one too;
  - the device flow must be switched on in the app's settings;
  - the code endpoint accepts 50 submissions per hour per app;
  - a user, app and scope hold at most 10 tokens, and an 11th revokes the oldest.
- **Token lifetimes** [SRC]:
  - OAuth App tokens do not expire by default, but are revoked after a year unused;
  - an app that opted in gets 8-hour tokens and 6-month refresh tokens, and a refresh replaces both;
  - an invalid refresh token gives `bad_refresh_token`.
- **OAuth App access restrictions** [SRC about-oauth-app-access-restrictions; https://support.atlassian.com/jira/kb/dvcs-only-lists-public-repositories-in-org/]:
  - they are on by default for new organisations;
  - an unapproved app sees only the organisation's public resources, and its private repositories vanish from listings **without an error**;
  - a request aimed at the organisation gets a 403 whose message says the organisation "has enabled OAuth App access restrictions";
  - the documented review link is `https://github.com/settings/connections/applications/<client_id>`, and approval is requested by the person and granted by an organisation owner.
- **SAML single sign-on** [SRC https://docs.github.com/en/rest/authentication/authenticating-to-the-rest-api]:
  - `X-GitHub-SSO: url=…` offers the authorisation URL;
  - `partial-results; organizations=…` means results from those organisations were dropped.
- **Rate limits** [SRC rate-limits]: a user token gets 5,000 requests per hour, shared with the person's other tokens; requests should be serial.
- **`/installation/repositories`** returns `{total_count, repositories: [Repository]}`, and each repository's `owner.type` is `User` or `Organization` [SRC; the coordinator read it for A2].

### Forgejo 15 (`v15.0.4`, electra)

- **Scopes** [SRC `routers/api/v1/api.go`, `models/auth/access_token_scope.go`, `services/auth/method/oauth2.go`]:
  - an OAuth grant of `openid profile write:repository` becomes the token scope `write:repository`;
  - `/user/repos` needs `read:user` and `read:repository`, and `/user/orgs` needs `read:user` and `read:organization`. Both answer 403 to keeper's token;
  - changing a grant's scope needs the grant revoked first.
- **`/repos/search`** with `uid=<id>&private=true`:
  - it is guarded only by the repository scope and returns the same set as `/user/repos`: owned repositories, organisation repositories reached through a team, and collaborator repositories [LIVE, the coordinator: `keeper/keeper-config` is included];
  - the body is `{ok, data}` with `X-Total-Count`, and `limit` is clamped to 50 [LIVE `max_response_items: 50`];
  - `Link` is built from ROOT_URL, not from the host called [LIVE], and there is no `ETag` [LIVE].
- **`GET <issuer>/login/oauth/userinfo`** with the forge token gives the `sub`, with no scope check [the coordinator, LIVE].
- **A repository's `owner`** has no type field [LIVE].
- **An edge case:** a read-only collaborator on another *user's public* repository is not listed, because `refreshAccesses` stores write access and above only [SRC `models/perm/access/access.go`; not tested live].

### makistack's `github-broker` (#863, open, not merged)

Sources: the PR body; `docs/runbooks/github-broker.md`, `docker/github-broker/app/broker.py` and `policy.yml` on branch `feat/github-broker` (commit `7f32b70`).
- **Deployed from the branch** at `https://electra.siren-alsephina.ts.net:8455`. It is tailnet only and reached through Caddy with no SSO gate, because callers send bearer tokens.
- **`/healthz`** reports `tgbot: unconfigured` and `tgdev: configured`. `tgdev` is installed nowhere, so a token request answers `not_installed` [PR body, *Already live on electra*].
- **`GET /v1/whoami`** returns `{sub, subject, grants: [{app, owners, repositories: "*" | [name], permissions}]}`. `subject` is `null` for a person the policy does not name (`broker.py:274-284`).
- **`POST /v1/token`** takes `{app, owner, repositories?, permissions?}` and returns `{token, expires_at, app, owner, repositories, permissions}` (`broker.py:287-340`).
- **Errors** are `{error, detail}` (`broker.py:255-332`):
  - 401 `unauthenticated`;
  - 503 `idp_unavailable`;
  - 400 `bad_request`;
  - 403 `forbidden`;
  - 503 `app_unconfigured`;
  - 404 `not_installed`;
  - 403 or 502 `github_refused`, with `github_status`;
  - 502 `broker_error`.
- **Tokens** are installation tokens (`ghs_…`) that live one hour. The broker hands out a cached token while it has 10 minutes left (`TOKEN_MIN_REMAINING`).
- **The caller's JWT** is verified against ZITADEL's JWKS, with `iss` exact and `aud` one of `BROKER_AUDIENCES`, which include the `keeper` project (`broker.py:195-220`; runbook, *Accepted audiences*).
- **The policy** keys grants by ZITADEL `sub`, and each grant is a ceiling. `tgorka` may use `tgbot` on `tgorka` and `neuraffica`, and `tgdev` on `tgorka`; marta has no subject (`policy.yml:30-41`; runbook, *keeper*).

### Git and LFS

- **GitHub** accepts an OAuth token as the Basic user name with an empty password for git and LFS [LIVE, the coordinator, private repository]. It is not verified for an installation token, and the runbook documents `x-access-token:<token>` (DW-325).
- **Forgejo** accepts an OAuth2 token as the user name [SRC `services/auth/method/basic.go`; LIVE, RForge §3].

### ZITADEL

- ZITADEL implements RFC 8693, but `audience` may only narrow and `resource` always returns `invalid_target`. It cannot hand out GitHub or Forgejo tokens [SRC https://zitadel.com/docs/guides/integrate/token-exchange].

### Not established

- **GitHub:**
  - the exact endpoints that return the access-restriction 403;
  - whether an OAuth token in a single-sign-on organisation gets `partial-results` (documented for classic PATs only);
  - whether `x-access-token:` Basic works for `gho_` tokens;
  - nothing on GitHub was tested with an installation token.
- **Forgejo:**
  - the collaborator-only case was not tried live;
  - no rate-limit headers were seen.

## Requirements allocated here

| id | statement | story | AD |
| --- | --- | --- | --- |
| FR-734 | Settings › Sync, beside *Add a folder*, and the Sync pane's empty state and add affordance, offer *Browse repositories…* whenever keeper has at least one repository source. Without one, the button is absent. | 86.1, 86.5 | AD-333, AD-338, UX-DR120 |
| FR-735 | keeper's repository sources are: the account's forge, when the account signs in to its repository through a forge that names an `api_base`; and GitHub, when the descriptor names a `[github_broker]`, or when a GitHub OAuth client exists (a `[[forges]]` GitHub entry's `client_id`, or keeper's built-in one, which only a source on github.com's own two addresses receives). GitHub through keeper's built-in client needs no account. keeper lists only the account's own Forgejo: a Forgejo entry under `[[forges]]` is refused (A4; DW-330 resolved). | 86.1 | AD-333 |
| FR-736 | With a `[github_broker]`, keeper asks the broker, as the signed-in person, which GitHub owners they may reach. It lists each owner's repositories with a one-hour, read-only token for that owner, using a grant that covers what it asks for. A drive's token covers only its repository, with write access to its contents when a grant allows it and read access otherwise, and then the drive only downloads. An owner the broker cannot serve shows why and does not stop the others. A person with no access sees that, in its own sentence, and is told to ask the broker's administrator, unless they can connect GitHub themselves. | 86.2, 86.3 | AD-334, AD-335 |
| FR-737 | Without a broker grant, and with a GitHub client id, the person connects GitHub by entering a code on github.com. keeper shows the code, copies it, opens the page and waits, and *Cancel* stops it, even when GitHub's answer is already on its way. The connection stays in the keychain under its source and client, refreshes itself when GitHub made it expiring, and ends at *Disconnect*. That does not revoke it on GitHub, and the sheet says where to. | 86.2, 86.5 | AD-334, UX-DR120 |
| FR-738 | A GitHub list through the person's own connection holds their repositories, those they collaborate on, and those of every organisation they belong to. An organisation that has not approved keeper, or one behind single sign-on, is named in a notice with its link. | 86.3 | AD-335 |
| FR-739 | The account's forge lists the person's own repositories, their organisations' repositories reached through a team, and the repositories they collaborate on, with the sign-in keeper already holds and no new consent. | 86.3 | AD-335 |
| FR-740 | Each repository shows whether it already syncs on this device (and as which drive), or on which other devices, and says when keeper can only download it (read-only, archived or a mirror). Repositories are grouped by owner, the person's own first, and can be searched, filtered by owner, shown with or without forks and archived ones, and sorted by update or name. | 86.3, 86.5 | AD-335, UX-DR120 |
| FR-741 | *Add…* on a repository opens the add-folder form filled in with the name, remote, branch, direction (download-only where keeper cannot push) and the source's sign-in. The person chooses the folder. The source's sign-in is offered only while the remote is on the source's host. | 86.4, 86.5 | AD-336, UX-DR120 |
| FR-742 | Several repositories can be added at once under one base folder on a desktop, each name editable. A row whose folder holds other files, is not a full path, is or lies inside or around another drive's folder, or whose name repeats, is blocked alone. A repository already syncing on this device is refused with its sentence. A repository keeper cannot push to is added download-only. The rest are added and start syncing, and each row reports its outcome. On iPhone and iPad keeper places the folders. | 86.4, 86.5 | AD-336, UX-DR120 |
| FR-743 | A drive added from a source signs in through that source: through the account for the account's forge, and through the source's own connection or broker otherwise. The choice travels with the device's settings, and is applied from the repository only to a drive whose remote is on that source's host, on a device that can get that source's token. | 86.4 | AD-336 |
| FR-744 | An operator can name GitHub sources (`[[forges]]`) and a GitHub broker (`[github_broker]`) in the descriptor. keeper refuses plain http, a secret, a duplicate id, a Forgejo entry, a GitHub source on github.com whose API is not `api.github.com`, any other source whose API is on another host, and a GitHub source moved off github.com while a broker serves it. The setup sheet names the broker's host and every host that will receive a repository token. Editing either table never signs anyone out. | 86.1, 86.6 | AD-333 |
| FR-745 | `docs/account.md` documents the broker protocol keeper speaks, the errors it honours and what it shows for each, and how to run the broker. keeper ships no broker. | 86.6 | AD-337 |
| NFR-101 | **A forge token goes only where it belongs.** A token lives in the keychain (`forge/<source-id>/<client_id>/session`) or in memory (broker tokens and the listing cache, both kept for one identity and cleared at every sign-in, sign-out, forget and setup). It never crosses IPC, and never reaches a log, a URL or the config repository. Only an https address receives one (loopback excepted, for tests), and only its own forge's origin, whose API must sit on that forge's own host, or the descriptor's broker. | 86.2, 86.4 | AD-334, AD-336 |
| NFR-102 | **No clock of its own.** keeper polls nothing in the background. Device-flow polling is a cancellable wait inside a flow the person started, and a list is fetched when the sheet asks for it. | 86.2, 86.3 | AD-334, AD-335 |
| NFR-103 | **No source, no change.** Without an account and without a GitHub client id, nothing new appears and no request is made. GitHub's hosts and the broker's host join Settings › About's list only while they are in use. | 86.1 | AD-333 |
| NFR-104 | **Bounded and forgotten.** A source lists at most 1,000 repositories, with serial requests; a broker listing reaches at most 20 owners in at most 60 requests. The list lives in memory for the process and for one identity, and nothing about it is written to disk. | 86.3 | AD-335 |

**Held, not restated:**
- NFR-92: without a configured account, nothing account-related changes. GitHub through keeper's own client is the one source that needs no account, and it is dormant while `BUILTIN_GITHUB_CLIENT_ID` is `None`.
- NFR-100: no secret enters the config repository, and `forge:<source-id>` names a source, not a credential.

## Stories

Every story names its rung in the three-rung stack (*Stack*, below).
- **The shell is by inspection.** Everything under `src-tauri/crates/keeper/**` awaits CI's macOS job and the Mac gate on hesperia.
- **Generated bindings** (`src/lib/ipc/gen/*.ts`) are regenerated by keeper-core's ts-export run and never hand-edited.
- **Every new core test and every front behaviour test is mutation-proved:** mutate, run, restore, and read the diff to confirm the restore.

### 86.1 — Your repository sources
**Intent:** "list button to list the repos in sync … github and forgejo provider". **Rung:** the module, the descriptor tables, `AccountSetupVm.broker_host` and egress are on **epic86-core** (lane CoreForge). The `forges_list` command and its registration are on **epic86-surface** (lane Shell3). AD-333, AD-338.
**Files:**
- `keeper-core/src/forges/mod.rs` and `source.rs` (new), and `lib.rs` (`pub mod forges;`);
- `org_account/descriptor.rs` (`ForgeEntry`, `GithubBroker`, `validate_forges`);
- `org_account/state.rs` (`broker_host`, `forge_hosts`, `setup_vm`);
- `egress.rs`;
- `keeper/src/forge_ipc.rs` (new) and the command registration.

**Acceptance:**
- *`sources()`* (mutation-proved):
  - no descriptor and no built-in id gives no source;
  - a built-in id alone gives `github` with `DeviceFlow`;
  - an `oauth` descriptor with `api_base` gives `account-forge` with `AccountForge`, whose `web_base` keeps a subpath forge's path (A4, m9);
  - a `[github_broker]` gives `github` with `Broker`, even when a client id exists;
  - `[[forges]]` GitHub entries keep their order and defaults;
  - the built-in client id reaches only a source on github.com's own two addresses (A4, B1).
- *Descriptor* (mutation-proved):
  - refused: an http `web_base`, `api_base` or broker `url` (loopback allowed); a `client_secret` inside `[[forges]]`; two entries with one id, or one named `account-forge`; a `github` entry off github.com while a broker is configured;
  - refused since A4: any Forgejo entry, with "keeper lists only the account's own Forgejo; remove this [[forges]] entry." (m10); a github.com `web_base` with any `api_base` but `https://api.github.com`, and any other `web_base` whose `api_base` is on another host (B1);
  - accepted since A4: `https://github.com/`, because origins are compared parsed (m11);
  - editing `[[forges]]` or `[github_broker]` leaves `replaces` false.
- *The setup sheet:* `broker_host` is the broker's host, and `None` without one; `forge_hosts` lists every host that will receive a repository token (A4, B1).
- *Egress* (mutation-proved; A4, t1: the in-use test is core's): no descriptor and no connection, no entry; the broker's host and `api.github.com` while the broker is configured; a connected source's web and API hosts, and none for a source not in use; a host is listed once.
- *Shell, by inspection:* `forges_list` is registered on every target and composes each source's state.

**binds:** FR-734, FR-735, FR-744, NFR-103, AD-333, AD-338

### 86.2 — A token from the broker, the forge or GitHub
**Intent:** "for the fetch support the proxy for getting the token based on oath". **Rung:** the parsers, request builders and decisions are on **epic86-core** (lane CoreForge); the broker runtime, the device-flow runtime and the keychain are on **epic86-surface** (lane Shell3). AD-334, AD-338, A2, A3.
**Files:**
- `keeper-core/src/forges/tokens.rs`, `broker.rs` and `device_flow.rs`;
- `keeper/src/forge_ipc.rs` (`forge_connect_start`, `forge_connect_open`, `forge_connect_wait`, `forge_connect_cancel`, `forge_disconnect`, the per-source cancel flag, the in-memory token cache);
- `keeper/src/account_ipc.rs` (*Forget this account* removes the descriptor's device-flow sessions).

**Acceptance:**
- *Broker* (mutation-proved):
  - the whoami parse covers `repositories: "*"` and a list, and `subject: null`;
  - `app_for` (A4, M3; the fake broker now matches `policy.decide`, t4): it picks only a grant that covers the owner, the repository and the permission; the descriptor's app wins only among those; a grant whose ceiling lacks the permission, or whose list lacks the repository, is passed over;
  - a listing request carries `{metadata: read}` and no repositories; a drive request carries `[repo]` and `{contents: write}` when a covering grant allows write, else `{contents: read}` (A4);
  - `can_push` is true only under a covering `contents: write` grant, and false when GitHub's `permissions.push` is false (A4);
  - the token parse reads `expires_at`;
  - classification: a 401 gives `NeedsSignIn`, except straight after a fresh refresh, which gives `Refused` with the detail (A4, m14); 404 `not_installed` and 503 `app_unconfigured` give owner notices; 403 `forbidden` and a 403 `github_refused` give an owner notice with the detail; a 5xx `github_refused` gives `Unreachable` (A4, m4); no grants gives `NoAccess` (A4, m2); a 400 gives `Internal`; any other 5xx gives `Unreachable`;
  - caches (A4, M1): a whoami or token cached for one account id and `sub` is not served to another, nor without a live session, and `forget_identity()` empties both.
- *Device flow* (mutation-proved):
  - `authorization_pending` keeps polling; `slow_down` adds 5 s; each terminal error ends the flow with its sentence; a cancel ends it as `notConnected`;
  - a cancel set while the token answer or `/user` is in flight still ends it as `notConnected`, and nothing is stored (A4, M4; t3);
  - `interval` 0 and `u64::MAX`, and a huge `expires_in`, are clamped without a panic (A4, M5);
  - a `verification_uri` that is not https on `web_base`'s host or a subdomain of it is refused with its sentence (A4, m5);
  - the stored client id is the one that started the code (A4, m12);
  - a `/user` failure after the token keeps the session, with an empty login (A4, m13).
- *Token lifetime* (mutation-proved): a stored token with `expires_ms` refreshes 60 s early; `bad_refresh_token` clears it and gives `NeedsConnect`; the keychain key is `forge/<id>/<client_id>/session`, so two client ids of one source keep two items, and a read never deletes one (A4, m1).
- *Shell, by inspection:*
  - a broker token is cached by identity and (app, owner, repositories, permissions) until `expires_at` minus 5 minutes, and never written;
  - `forges::forget_identity()` and the stored errors are cleared on `account_sign_in`, `account_sign_out`, `account_forget` and `account_setup_confirm` (A4);
  - the error map is cloned and its lock dropped before any keychain read (A4);
  - only `forge_connect_open` opens a browser;
  - the device code never crosses IPC;
  - *Disconnect* deletes `forge/<id>/<client_id>/session`;
  - *Forget this account* deletes the descriptor's sessions and keeps the built-in one.
- *Live (owed, when the broker serves):* on hesperia with the account signed in, `whoami` answers `tgorka`'s grants and a `tgbot` listing token lists `tgorka` and `neuraffica`.

**binds:** FR-736, FR-737, NFR-101, NFR-102, AD-334, AD-338

### 86.3 — Your repositories, by owner
**Intent:** "list the repos … (including organizations in github)". **Rung:** the parsers, the notices, marking and grouping are on **epic86-core** (lane CoreForge); `forge_repos` and the paging are on **epic86-surface** (lane Shell3). AD-335.
**Files:**
- `keeper-core/src/forges/listing.rs` (or `github.rs` and `forgejo.rs`) and `mark.rs`;
- `keeper/src/forge_ipc.rs` (`forge_repos`, the per-source in-memory cache, the stored errors).

**Acceptance:**
- *GitHub* (mutation-proved): a page parses with and without `link rel=next`; the installation page parses; a 403 naming "OAuth App access restrictions" gives the restricted notice with the review link; `X-GitHub-SSO` gives the single-sign-on notice; the eleventh page is not fetched and sets `truncated` (A4, t2: the cap itself is exercised).
- *Broker listing* (mutation-proved; A4): at most 20 owners and 60 requests; a 401 on one owner's installation listing evicts that token and gives the broker-worded notice while the other owners list (m3); repositories are grouped before the cap cuts, so "you" stays first (m7).
- *Forgejo* (mutation-proved): `{ok, data}` parses; `pages(total)` rounds up and stops at 20; without `X-Total-Count`, paging continues while a page is full (A4, m8); the userinfo `sub` parses.
- *Download-only* (mutation-proved; A4): `pullOnly` is true for a repository that cannot be pushed, is archived or is a mirror, each with its sentence, and false otherwise.
- *Marking* (mutation-proved): `https://Git.Acme.dev/a/b.git/` and `https://git.acme.dev/a/b` mark one drive; `elsewhere` excludes this device.
- *Grouping:* "you" first, then A→Z.
- *The cache* (mutation-proved; A4, M2): a list cached for one identity is not served to another, nor without a live session; `NoAccess` never serves the cache.
- *Shell, by inspection:* one failing broker owner leaves the others listed; requests are serial; nothing is written to disk; `forge_repos(id, true)` always retries and replaces a stored error (A4).
- *On hesperia (owed):* electra's Forgejo lists `keeper/keeper-config` and the person's own repositories with the grant keeper holds, and no re-consent.

**binds:** FR-736, FR-738, FR-739, FR-740, NFR-102, NFR-104, AD-335

### 86.4 — Add one, or several
**Intent:** "help adding them to the list of sync drives". **Rung:** `forge:` validation, its translation and the origin rule are on **epic86-core** (lane CoreForge). `forge_repos_add`, `forge_default_base_folder`, `drive_credential`'s `forge:` branch and `folder_accepts` made `pub(crate)` are on **epic86-surface** (lane Shell3). AD-336, A1.
**Files:**
- `keeper-core/src/registry.rs` (`credential_source_row`);
- `org_account/settings_sync.rs` (the `forge:` value and the origin rule);
- `keeper/src/forge_ipc.rs`, `account_ipc.rs` (`drive_credential`, `sync_credential_source_get`) and `account_restore.rs` (`folder_accepts`).

**Acceptance:**
- *Registry* (mutation-proved): `forge:github` is stored verbatim and read back; `forge:` with an id outside `[a-z0-9-]{1,32}` is refused; a bot provider refuses `forge:`.
- *Travel* (mutation-proved): `forge:github` goes to and from `settings.<device>.toml` unchanged; a pulled value applies to a drive on `https://github.com/…` and not to one on another host; and only where this device can get that source's token (a device-flow session, or a broker source with the account signed in), else it stays unapplied (A4, m15).
- *The origin rule* (mutation-proved; A4, t5): the tests pin how `remote_on_source` treats `https://github.com.evil.com`, userinfo in the URL, a trailing-dot host, `:443`, an uppercase host and `http://`, so a regression in any spelling fails. `drive_token` and the shell use this one rule.
- *Shell, by inspection:*
  - the batch uses `<base>/<name>` or the item's folder, refuses one that is not a full path ("Choose a full folder path."; `~/` expands), creates an absent folder, and refuses one that holds other files with its sentence (A4);
  - it refuses a folder that is, lies inside, or contains a drive's folder, comparing canonicalized paths, case-insensitively on macOS (A4);
  - it refuses a repository already syncing on this device ("Already syncing here as `<name>`.") (A4);
  - batches are serialized; a failed row's created folders are removed deepest first; `folder_accepts` ignores `.DS_Store` and `Icon\r` (A4);
  - a download-only repository is added with the direction `pullOnly` (A4);
  - each add runs `sync_profile_save`'s path, sets the credential and calls `note_local_change`;
  - iOS uses the container's folder;
  - `sync_credential_source_set` refuses `forge:<id>` for a drive whose remote fails `remote_on_source`, with "This drive's repository isn't on `<host>`." (A4);
  - `drive_credential` answers a `forge:` drive only when its remote passes `remote_on_source`.
- *On hesperia (owed):* a batch of two repositories from electra's Forgejo adds two drives that fetch with the forge token. Once the broker serves, a GitHub repository added through it fetches and pushes (DW-325).

**binds:** FR-741, FR-742, FR-743, NFR-101, AD-336

### 86.5 — The browse sheet
**Intent:** "Design best ui/ux for that". **Rung:** **epic86-surface** (lane Front3). UX-DR120, A3.
**Files:** the sheet and its batch view under `src/components/sync/` or `src/components/settings/`, the entry points in `sync-section.tsx` and the Sync pane, the add-folder form's `forge:` credential option, the client wrappers in `src/lib/ipc/client.ts`, fixtures, and `dev/mock-shell.ts`.

**Acceptance (mutation-proved):**
- the entry points are absent when there are no sources;
- the connect flow shows the code, copies it, and *Open github.com* copies it and opens the page; a failed open or copy shows a sentence (A4);
- *Connect GitHub* follows `canConnect`, not `via`, and *Disconnect* shows when `canConnect` and connected, with `appsUrl` (A4, #2, #19);
- *Try again* and *Sign in* retry the listing with `refresh = true` (A4, #1);
- rows show *Syncing here as* and *On `<devices>`*, and an added row's checkbox is disabled;
- a download-only row shows its sentence, and the batch preview marks it "Download only" (A4);
- search, the owner filter and the *Forks* and *Archived* switches narrow the list;
- the selection is selected ∩ listed ∩ not added, for the count and the batch alike (A4, #4);
- an answer for another source is dropped (A4, #5);
- a conflict in the batch preview blocks only its row;
- batch results render per row, and a row's result clears when its name changes (A4, #11);
- the add-folder form hides the `forge:` choice while the remote's host is not the source's (A4, #21);
- the setup sheet renders `forgeHosts` (A4);
- per-owner notices and the no-access sentence render.

`bun run check:design` passes on the new files. Real-browser proof of the sheet is owed (the `prove-a-keeper-frontend-change-in-a-real-browser` procedure).

**binds:** FR-734, FR-737, FR-740, FR-741, FR-742, UX-DR120

### 86.6 — The broker, documented for operators
**Intent:** "the proxy … (work in progress in makistack - chceck upcomming prs if needed)". **Rung:** this document is on **epic86-plan**; `docs/account.md` and `docs/egress.md` ride **epic86-surface**, because they describe what it ships (lane EpicDoc3). AD-337.
**Files:** `docs/account.md` (§ *Repository sources in the descriptor*, § *Browse your repositories*, § *GitHub through your organization's broker*, the credential values, the keychain items, the security notes and the operator notes) and `docs/egress.md` (the GitHub and broker rows).

**Acceptance:**
- the protocol in `docs/account.md` matches `broker.py` at `7f32b70`: paths, bodies, the permission sets keeper asks for, and each error with the sentence keeper shows;
- #863 is cited as open, deployed and not merged;
- after the review wave (A4), the docs describe the refused Forgejo entry and the API-host rule, the setup sheet's token hosts, the per-client keychain item, grant-fit and download-only drives, the batch's refusals, the travel rule, the no-access state, and caches kept per identity; the sentences quoted are the code's;
- `docs/egress.md`'s *On this Mac* list is untouched, so `about-section.test.tsx`'s mirror stays green.

**binds:** FR-744, FR-745, AD-337

## What stays out

- **A broker in keeper,** or any server (D-28).
- **Widening the Forgejo grant.** It would force a revocation on every device.
- **GitLab, Bitbucket and Gitea as named kinds.** Gitea shares Forgejo's API, but only Forgejo 15 was verified. A Gitea `[[forges]]` entry is not offered.
- **A second Forgejo.** keeper has no token path for one, so a Forgejo `[[forges]]` entry is refused (A4; DW-330, done).
- **Organisation management:** approving keeper for an organisation, installing the GitHub App, and adding a person to the broker's policy. These belong to GitHub's owners and the broker's administrator (D-25).
- **Creating a repository from keeper.**

Deferred, with the ledger entries allocated here so a later planner finds them. The coordinator applied them to `_bmad-output/implementation-artifacts/deferred-work.md` on 2026-09-24, and that ledger is the source of truth; the copies below mirror it after the review wave.

```markdown
### DW-323: GitHub through keeper's own sign-in stays dormant until the owner registers keeper's GitHub OAuth App.

origin: epic 86's plan, 2026-09-24 (AD-333, AD-334)
location: `src-tauri/crates/keeper-core/src/forges/source.rs` (`BUILTIN_GITHUB_CLIENT_ID = None`), `src-tauri/crates/keeper-core/src/forges/device_flow.rs`
reason: the device flow needs a public client id, and keeper has none yet. `BUILTIN_GITHUB_CLIENT_ID` ships as `None`, so without a descriptor that names a `github` forge with a `client_id`, or a `[github_broker]`, there is no GitHub source at all. The *Connect GitHub* flow, the keychain session and the refresh are built and tested against their parsers, but no one can reach them. Registering the app is the owner's act: an OAuth App (not a GitHub App) named keeper, with the device flow switched on, no secret used, and the scopes `repo read:org` requested by keeper. Its client id is not a secret. Revisit when the owner registers it: set the constant, and run the connect flow once live on hesperia.
status: open

### DW-324: The GitHub broker is an open makistack PR, and serves no token yet.

origin: epic 86's plan, 2026-09-24 (AD-334, AD-337; amendment A2)
location: tgorka/makistack#863 (`feat/github-broker`, `docker/github-broker/`, `docs/runbooks/github-broker.md`), `src-tauri/crates/keeper-core/src/forges/broker.rs`
reason: keeper speaks `github-broker`'s protocol as read from its source at `7f32b70`, but the PR is not merged. It is deployed on electra from the branch, where `tgbot` has no credentials and `tgdev` is installed nowhere, so every token request answers `app_unconfigured` or `not_installed`. keeper shows those as per-owner notices, and lists nothing from GitHub through the broker today. The protocol may still change before merge. What the broker reaches is also not "every organization the person belongs to": it is the owners the policy grants, intersected with where the app is installed (`tgorka`, `neuraffica` for `tgbot`). Revisit when #863 merges: diff `broker.py`'s routes and bodies against `forges::broker`, then run 86.2's and 86.3's owed live checks.
status: done 2026-09-24
resolution: makistack#863 merged at `e32513c` (2026-09-24 19:10 UTC). The protocol keeper speaks is unchanged from `7f32b70`; only the apps were renamed (`tgbot` → `tgorka`, `tgdev` → `tgorka-dev`), which keeper never names — it takes the app from `/v1/whoami`. `/healthz` reports both apps configured. The first live listing on hesperia then showed "GitHub refused the list (HTTP 403)" for every owner: keeper's GitHub requests carried no `User-Agent`, which api.github.com refuses with a plain-text 403. Measured with a broker `ghs_` token: 403 without one, 200 with `User-Agent: keeper`. `forges::github::get` and the device-flow form now send it, and the broker-listing test's fake refuses a request without one, as GitHub does.

### DW-325: Git and LFS with a broker's installation token are unverified in keeper-sync's spelling.

origin: epic 86's plan, 2026-09-24 (AD-336; contract fact on git)
location: `src-tauri/crates/keeper-sync/src/credential.rs:58-62` (`AccessToken::git`), `:71-75` (`lfs_basic`)
reason: keeper-sync sends a drive token as the Basic user name with an empty password. The coordinator verified that live on GitHub for an OAuth token (`gho_`): `git ls-remote` succeeded and LFS batch answered 200. The broker hands out installation tokens (`ghs_`), and its runbook documents `x-access-token:<token>`, the password form. GitHub's documentation says the user name is ignored when the token is the password, and a 2012 post (updated 2021) documents the token as the user name, but neither names installation tokens. keeper-sync is left unchanged, as the contract requires. Revisit once `tgbot` is installed: the coordinator runs `git ls-remote` and an LFS batch with a `ghs_` token as the user name, and if GitHub refuses it, keeper-sync gains the `x-access-token` spelling for GitHub hosts.
status: done 2026-09-24
resolution: measured against github.com with a broker token for `tgorka/gh-app-sandbox` (private, `contents: write`): the installation token as the Basic user name is refused (git `ls-remote` fails, LFS batch 401), and as the password of `x-access-token` it is accepted (git `ls-remote` lists `HEAD`, LFS batch 200). keeper-sync's `AccessToken` now spells a `ghs_` token as `x-access-token:<token>` for git and LFS alike; every other token keeps the user-name spelling Forgejo and OAuth/PAT GitHub tokens accept. Test: `credential::an_installation_token_is_the_password_of_x_access_token` (mutation-proved).

### DW-326: An organization that has not approved keeper's GitHub OAuth App hides its private repositories, and only its owners can fix that.

origin: epic 86's plan, 2026-09-24 (AD-335)
location: `src-tauri/crates/keeper-core/src/forges/github.rs` (`classify_error`, the restricted-organization notice)
reason: OAuth App access restrictions are on by default for new organizations. Until an owner approves keeper, the organization's private repositories are missing from `/user/repos` without an error, and its public ones still show. keeper can only say so and link the review page, `https://github.com/settings/connections/applications/<client_id>`, where the person requests approval. The notice appears only when GitHub answers a 403 naming the restriction. A listing that silently drops the private repositories gives keeper nothing to detect, so the notice can be missing while repositories are missing too. The broker path has no such approval: there the GitHub App's installation decides. Revisit if people report missing organization repositories with no notice: a per-organization probe (`GET /orgs/<org>/repos?per_page=1&type=private`) would name them, at one request per organization.
status: open

### DW-327: Disconnecting GitHub does not revoke keeper's access on GitHub.

origin: epic 86's plan, 2026-09-24 (AD-334)
location: `src-tauri/crates/keeper/src/forge_ipc.rs` (`forge_disconnect`), `src-tauri/crates/keeper-core/src/forges/device_flow.rs`
reason: GitHub's token-revocation endpoint (`DELETE /applications/{client_id}/token`) authenticates with the app's client secret, and keeper, a public client, has none. *Disconnect* deletes `forge/<source-id>/<client_id>/session` (and any item an earlier build kept at `forge/<source-id>/session`) from this device's keychain, and the sheet says "Also remove keeper under GitHub › Settings › Applications if you want", linking the source's `appsUrl` (`https://github.com/settings/connections/applications/<client_id>`). Until the person does, the grant stays on GitHub, and a copy of the token elsewhere (none exists by design) would keep working. An unused OAuth token is revoked by GitHub after a year. Revisit if GitHub offers a secret-less revocation for device-flow tokens, or if the broker grows a revoke endpoint.
status: open

### DW-328: A Forgejo repository's owner is classified by comparing logins, because Forgejo does not say what the owner is.

origin: epic 86's plan, 2026-09-24 (AD-335)
location: `src-tauri/crates/keeper-core/src/forges/forgejo.rs` (`parse_search`), `src-tauri/crates/keeper-core/src/forges/mark.rs` (grouping)
reason: Forgejo's `Repository.owner` carries no `type`, and keeper cannot call `/user/orgs` (it needs `read:user` and `read:organization`, which keeper's grant lacks). An owner equal to the forge login is "you". Every other owner is a group of its own, so an organization and a person whose repository you collaborate on look alike, and the owner filter cannot offer "organizations only". On the broker path "you" is inferred the other way round: any owner whose `owner.type` is `User`, because an installation token cannot read `/user`. So another person's account covered by a grant would also read as "you". Revisit if the grouping misleads: telling organizations apart on Forgejo needs either `read:organization` in the grant (and so a revocation on every device) or a per-owner lookup the `write:repository` grant is allowed to make, which has not been verified.
status: open

### DW-329: A source lists at most 1,000 repositories, and search looks only through those.

origin: epic 86's plan, 2026-09-24 (AD-335, NFR-104)
location: `src-tauri/crates/keeper-core/src/forges/listing.rs` (the caps: GitHub 10 pages × 100, per owner on the broker path, with at most 20 owners and 60 requests per listing, 1,000 repositories overall; Forgejo 20 × 50, paging while a page is full; repositories are grouped before the cap cuts, so "you" stays first)
reason: keeper fetches serially and keeps the list in memory, so a cap bounds both the wait and the memory. Past it, the sheet says "Only the first 1,000 are listed; search looks only through those." A repository beyond the cap can still be added by typing its URL into *Add a folder*. Which repositories fall past the cap depends on the endpoint's order: `/user/repos` is asked for `sort=full_name` and Forgejo's search for `sort=alpha`, so there the ones late in the alphabet are dropped, not the oldest; `/installation/repositories` documents no order. Revisit if a person with more repositories asks: a server-side search (GitHub's `GET /search/repositories`, Forgejo's `repos/search?q=`) would reach the rest.
status: open

### DW-330: A Forgejo forge listed under `[[forges]]` was accepted but could not be browsed.

origin: epic 86's build wave, 2026-09-24 (AD-333, AD-334; CoreForge's `sources()`; ReviewCore86 m10)
location: `src-tauri/crates/keeper-core/src/org_account/descriptor.rs` (`validate_forges`), `src-tauri/crates/keeper-core/src/forges/source.rs` (`sources`)
reason: the build wave's descriptor accepted `[[forges]] kind = "forgejo"` with its `web_base` and `api_base`, as the contract froze it, but AD-334 gives keeper no way to get a token for it: Forgejo has no device flow, the broker is GitHub's only, and the account's forge token belongs to the account's own forge (sending it to another host would break NFR-101). `sources()` therefore left the entry out, and the browse sheet did not show it, with no sentence anywhere but `docs/account.md` (AD-27). Only the account's own forge is browsable on Forgejo. Revisit when a second Forgejo is wanted: its own OAuth sign-in (a public PKCE client per forge, like `config.auth`'s forge leg), or a broker that serves Forgejo.
status: done 2026-09-24
resolution: DW-330 — resolved by epic 86's review wave (amendment A4, m10): `validate_forges` now refuses a Forgejo `[[forges]]` entry with "keeper lists only the account's own Forgejo; remove this [[forges]] entry.", so the operator sees why at setup instead of the source silently missing. A second Forgejo stays out of scope until it has its own token path.
```

## The failure shape this epic must not repeat

**A token in the wrong place.** This epic adds three kinds of forge token: the broker's installation tokens, a device-flow connection and its refresh token, and the account's forge token reused for listing. It also adds a new credential value that travels in the repository.
- The rule is keychain or memory, https only, and only to the token's own forge origin or the descriptor's broker.
- `forge:<source-id>` names a source and never a secret.
- A drive's origin must match its source's origin before a token is handed out.

A review that finds a forge token in a VM, a log line, a URL, a file, or a request to a host other than its forge or broker is a blocker.

**A list that pretends to be complete.** An organisation's repositories can vanish because it has not approved keeper, because it uses single sign-on, because the broker's app is not installed there, or because the list stopped at 1,000. Every one of those has a sentence. A review that finds a listing path that swallows a failure without a notice, or lets one owner's failure blank the other owners, is a blocker.

**A batch that eats a folder.** The batch writes into folders the person did not pick one by one. A folder is used only when it is absent, empty or already that clone. A review that finds a path around `folder_accepts`, or a batch that stops at its first failure, is a blocker.

## Sprint-status entry

The coordinator applied this under `development_status:`, above the epic-85 block, and it now lives in `_bmad-output/implementation-artifacts/sprint-status.yaml`. The text as it stands after the review wave:

```yaml
  # Epic 86: the owner's ask on the epic-85 build — a button in Sync that lists repositories to add as drives, for GitHub (with the person's organizations) and Forgejo, with the best UI/UX keeper can design, fetching through makistack's OAuth token proxy. Owner decided 2026-09-24: GitHub lists the person's repos plus every organization they belong to; keeper builds the client side only. The proxy turned out to exist as tgorka/makistack#863 (github-broker: open, deployed on electra :8455 from its branch, not merged; tgbot unconfigured, tgdev installed nowhere), so contract amendment A2 dropped RFC 8693 and keeper speaks github-broker's own /v1/whoami + /v1/token.
  # Stack rungs: epic86-plan (this epic file + ledgers), then epic86-core (keeper-core forges/ module: sources, tokens, broker, device_flow, listing, mark; descriptor [[forges]] + [github_broker]; AccountSetupVm.broker_host; registry forge:<id>; settings_sync's forge: travel and origin rule; egress; bindings and the TS fixture ripple), then epic86-surface (the shell's forge_ipc.rs with the broker and device-flow runtimes, forge_repos_add, drive_credential's forge: branch, folder_accepts pub(crate), registration and egress wiring; the browse sheet, entry points, batch view, the add-folder form's forge: option, mock-shell; docs/account.md and docs/egress.md). The shell crate is by inspection and awaits CI's macOS job.
  # Contract amendments A1 (forge:<source-id> on the wire; bots refuse it), A2 (the broker is makistack#863's github-broker; RFC 8693 dropped; [github_broker] { url, app? } serves only github; per-owner installation/repositories listing with per-owner notices; drive tokens scoped to one repo, contents: write) and A3 (forge_connect_open opens the verification page; nothing else opens a browser) are in the epic's text.
  # Amendment A4, the review wave (ReviewCore86 B1, M1–M5, m1–m15, t1–t5; ReviewSurface86 1–21; local://epic86-fixwave.md; lanes CoreFix, ShellFix, FrontFix): a github.com web_base needs api.github.com and any other api_base stays on its web_base's host, and the setup sheet lists every host that receives a token (AccountSetupVm.forge_hosts); a Forgejo [[forges]] entry is refused (DW-330 done); the keychain item is forge/<id>/<client_id>/session and reads never delete; the broker picks a grant covering owner + repo + permission, and a drive gets contents: write only when a grant allows it, else read (download-only); read-only, archived and mirror repos are added pullOnly with a sentence; the batch refuses repos already synced here, relative paths, nested drives and folders a drive already uses; a pulled forge:<id> applies only where this device can get that token; no-access is its own state and sentence; caches are per identity and cleared on sign-in, sign-out, forget and setup; ForgeSourceVm gains credential, canConnect, appsUrl and ForgeRepoVm gains pullOnly, pullOnlySentence.
  # Owed on hesperia: the install; electra's Forgejo listing keeper/keeper-config under the existing grant, and a two-repo batch that fetches on the forge token; once #863 serves, whoami + a tgbot listing of tgorka and neuraffica and a GitHub drive fetching and pushing (DW-325); once the owner registers keeper's OAuth App, the Connect GitHub flow (DW-323). Front's real-browser proof of the sheet is owed too.
  epic-86: in-progress
  86-1-your-repository-sources: in-progress
  86-2-a-token-from-the-broker-the-forge-or-github: in-progress
  86-3-your-repositories-by-owner: in-progress
  86-4-add-one-or-several: in-progress
  86-5-the-browse-sheet: in-progress
  86-6-the-broker-documented-for-operators: review
  # DW-323…DW-330 are opened by this plan. DW-330 (a Forgejo [[forges]] entry validated but not browsable) is done: A4 refuses the entry at validation.

```

## docs/decisions.md entry

The coordinator applied this to `docs/decisions.md`, after D-27, and that file is the source of truth. The text as it stands after the review wave:

```markdown
## D-28 — Repository tokens come from the forge or the organization's broker; keeper runs no server

The owner asked for a button that lists a person's repositories on GitHub (with their
organizations) and on Forgejo, and adds them as drives, fetching through the OAuth token
proxy being built in makistack. keeper is a client only, and a list of private
repositories needs a token that can read them. Epic 86 decides where those tokens come
from, and what keeper does and does not run to get them.

- **What changes:** keeper names repository sources: the account's forge, and GitHub,
  through the broker the descriptor names under `[github_broker]` or through a
  device-flow connection with a public OAuth client. It lists each, marks what already
  syncs where, and adds one repository through the existing form or several under a
  base folder. A drive added from a source signs in through it (`account:<id>` on the
  account's forge, `forge:<source-id>` otherwise), and that choice travels with the
  device's settings. A repository keeper cannot push to (read-only, archived, a mirror)
  is added as a download-only drive. (AD-333…AD-338; FR-734…FR-745; NFR-101…NFR-104)
- **The broker is the organization's, and keeper speaks its protocol.** makistack's
  `github-broker` (tgorka/makistack#863) turns the person's ZITADEL sign-in into one-hour
  GitHub App installation tokens under its own policy. keeper calls `/v1/whoami` and
  `/v1/token` and ships no broker code. RFC 8693 was considered and dropped: ZITADEL cannot
  mint upstream tokens, and the one broker that exists speaks another protocol, so a
  second convention would have been one keeper spoke to nobody.
- **The least token for each job:** a listing token reads metadata only; a drive's token
  covers its one repository, with contents write only when one of the person's grants
  allows it, and read otherwise. keeper chooses the grant the broker's own policy would,
  so it never asks for what the policy refuses. Forgejo is listed through
  `/repos/search` under the grant keeper already holds, rather than widening the grant and
  making every person revoke it first.
- **Tokens stay where secrets stay, and with the person who got them:** the keychain for a
  device-flow connection, one item per source and OAuth client; memory for broker tokens
  and the list, kept for the signed-in identity and forgotten at every sign-in, sign-out,
  forget and setup. Nothing about a list is written to disk, and nothing is polled in the
  background.
- **A token goes only to a host the person saw:** its own forge, whose API must sit on the
  forge's own host (`api.github.com` for github.com), or the broker; and the setup sheet
  lists every such host before anything is written. A descriptor cannot name a second
  Forgejo, because keeper would have no token for it. A travelling `forge:<source-id>`
  applies only where this device can get that token.
- **What it amends:** AD-315's credential choice gains `forge:<source-id>` for drives (not
  for bot providers); AD-326's travelling choice carries it, applied only on that source's
  host. D-25 holds: approving keeper for an organization, installing the GitHub App and
  adding someone to the broker's policy are the organization's acts, not keeper's.
- **Revisited by the review wave (epic 86, amendment A4):** the rules above already carry
  it. It closed a path by which a descriptor's `api_base` could receive a person's GitHub
  connection, made caches answer only for the identity that filled them, and turned a
  silently dropped Forgejo entry into a refusal (DW-330).
- **Revisit triggers:** #863 merging with a changed protocol (DW-324); GitHub refusing an
  installation token as the Basic user name (DW-325); the owner registering keeper's
  OAuth App (DW-323); a second broker for Forgejo.
- **Status / owner:** decided. Owner is the architect. Epic 86 implements it:
  `keeper_core::forges`, the descriptor's `[[forges]]` and `[github_broker]`, and the
  shell's `forge_ipc`.
```

## Stack

Three rungs, by layer as in epics 80–85:
1. **`epic86-plan`:** this document and the ledgers: the sprint-status entry, `deferred-work.md` DW-323…DW-330, and D-28 in `docs/decisions.md`.
2. **`epic86-core`:**
   - keeper-core's `forges/` (`mod.rs`, `source.rs`, `tokens.rs`, `broker.rs`, `device_flow.rs`, `listing.rs` or `github.rs` and `forgejo.rs`, and `mark.rs`) and `lib.rs`;
   - `org_account/descriptor.rs` (`[[forges]]`, `[github_broker]` and their validation);
   - `org_account/state.rs` (`AccountSetupVm.broker_host`);
   - `org_account/settings_sync.rs` (`forge:` travel and the origin rule);
   - `registry.rs` (`forge:<id>`);
   - `egress.rs`;
   - the regenerated bindings, and every TypeScript `AccountSetupVm` literal that must name `brokerHost`.

   **Shell hunks that ride this rung,** because the new fields break existing shell code: any `AccountSetupVm` or `AccountDescriptor` literal in `keeper/src` that has no `..Default` (the descriptor gains `forges` and `github_broker`). Without them the rung does not compile alone on CI's macOS job. The coordinator moves them at stack time. `bindings:check` must be green on this rung alone.
3. **`epic86-surface`:**
   - the shell's `forge_ipc.rs` (the commands, the broker and device-flow runtimes, the batch add and the default folder);
   - `drive_credential`'s `forge:` branch and `sync_credential_source_get`;
   - `folder_accepts` made `pub(crate)`;
   - the command registration and the egress wiring;
   - the front: the sheet, the entry points, the batch view, the add-folder form's `forge:` option, the client wrappers and `dev/mock-shell.ts`;
   - `docs/account.md` and `docs/egress.md`.
