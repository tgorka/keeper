//! Egress honesty — the live set of network destinations keeper contacts (Story
//! 11.2, NFR-11, UX-DR17; Story 23.7).
//!
//! keeper makes a *verifiable* egress claim: [`compute_egress`] derives, from live
//! app state, the exact set of hosts the app talks to — each account's Matrix
//! homeserver, plus Beeper's `api.beeper.com` when (and only when) a Beeper account
//! exists, plus the host of every folder-sync profile's git remote, plus the one
//! signed auto-update endpoint. The Settings → About surface renders this set
//! directly (never a hardcoded doc), so the claim can never drift from reality.
//!
//! Bridges are Matrix appservices reached *through* the homeserver (server-side),
//! so they add no distinct client egress — the homeserver entry is their egress. No
//! per-bridge host is fabricated here.
//!
//! Folder-sync remotes are the mirror case (Story 23.7): a profile whose remote is
//! a local path or a pendrive contacts nothing, and the honest disclosure for it is
//! *no entry at all* — a "destination" that is a directory on the user's own disk
//! would be a fabricated network claim. Only a remote with a real host is disclosed,
//! and only its host: see [`remote_host`] for why the rest of the URL is dropped.
//!
//! The function is kept pure over `[(homeserver_url, Provider)]` plus `[remote_url]`
//! (no `Platform`, no Tauri runtime, and no `keeper-sync` dependency — AD-40 keeps
//! this crate free of it) so the whole I/O matrix is unit-testable; the `egress_list`
//! IPC command reads the registry rows and the engine's profile rows and feeds both
//! in as data.

use crate::auth::{is_beeper_homeserver, BEEPER_API_BASE};
use crate::vm::{EgressEndpointVm, EgressKind, Provider};

/// The signed auto-update endpoint keeper checks (Story 11.2, NFR-12).
///
/// This MUST stay in sync with `tauri.conf.json`'s `plugins.updater.endpoints`
/// entry — the updater config and this const name the same destination, and the
/// egress list would be dishonest if they diverged. Changing the release repo or
/// endpoint means changing both.
pub const EGRESS_UPDATE_ENDPOINT: &str =
    "https://github.com/tgorka/keeper/releases/latest/download/latest.json";

/// Beeper's client-facing API host, surfaced as a distinct egress entry exactly when a
/// Beeper account is present. Reuses [`BEEPER_API_BASE`] — the same constant the Beeper
/// login flow connects to — so the disclosed destination can never drift from reality
/// (mirrors how Beeper detection reuses `BEEPER_HOMESERVER` via `is_beeper_homeserver`).
const BEEPER_API_ENDPOINT: &str = BEEPER_API_BASE;

/// The network host a folder-sync remote reaches, or `None` when it reaches no
/// network at all (Story 23.7).
///
/// **Why the host and nothing else.** A remote URL is not a destination — it is a
/// destination plus a repository path, optionally a username, and, in a profile
/// whose credential was pasted into the URL instead of stored in the keychain, a
/// token. Settings → About is a screen people open to reassure themselves and to
/// show other people; rendering `https://oauth2:ghp_live@gitlab.example.org/team/notes.git`
/// there would turn an honesty surface into a credential leak. The host is the
/// complete answer to "where do these bytes go", so it is the whole disclosure.
///
/// **Why the grammar is split rather than handed to one parser.** git accepts two
/// shapes, and only one of them is a URL. `scheme://[user@]host[:port]/path` is,
/// and `keeper-core` already links `url` (`auth.rs` parses homeserver URLs with it),
/// so no dependency is added to handle it — and `url` keeps userinfo out of
/// `host_str`, which is a stripping rule written by people who have seen every
/// shape, not one hand-rolled here. The scp-like `[user@]host:path` shorthand is not
/// a URL at all, and feeding it to `url` is worse than useless: `github.com:t/n.git`
/// *parses*, as a scheme of `github.com` with no host, so a URL-first implementation
/// silently answers "no network" for a real remote. The `://` test is what tells the
/// two apart, and it is git's own distinction.
///
/// **Why `None` is a real answer, not a failure.** A remote that is a local path
/// (`/Volumes/pendrive/vault.git`, `../mirror`, `file:///srv/repos/x.git`) is the
/// case AD-48 exists for, and it contacts nothing. Disclosing a directory as a
/// network destination would be a fabricated claim in the one place that must not
/// fabricate, so such a profile contributes no entry.
fn remote_host(remote_url: &str) -> Option<String> {
    let trimmed = remote_url.trim();
    if trimmed.is_empty() {
        return None;
    }

    // The URL forms — every one git accepts carries an authority, so `://` is a
    // sufficient and exact test. `file:` parses like the rest and is explicitly not
    // egress: it names a directory.
    if trimmed.contains("://") {
        let parsed = url::Url::parse(trimmed).ok()?;
        if parsed.scheme().eq_ignore_ascii_case("file") {
            return None;
        }
        return parsed
            .host_str()
            .filter(|host| !host.is_empty())
            .map(str::to_ascii_lowercase);
    }

    // The scp-like shorthand `[user@]host:path`. git's own rule: it is scp-like only
    // when the separating colon comes before the first slash — otherwise
    // `/srv/repos/a:b` (a legal directory name) would be read as a host.
    //
    // A bracketed IPv6 literal carries colons of its own, so the scan for the
    // separator starts after the closing bracket — and the bracket can be preceded by
    // userinfo (`git@[2001:db8::1]:notes.git`), so it is found rather than assumed to
    // be at position zero. A `[` that appears after a slash is part of a directory
    // name, not a literal, and is ignored.
    let scan_from = match trimmed.find('[') {
        Some(open) if !trimmed[..open].contains('/') => open + trimmed[open..].find(']')?,
        _ => 0,
    };
    let colon = scan_from + trimmed[scan_from..].find(':')?;
    let authority = &trimmed[..colon];
    if authority.contains('/') {
        return None;
    }
    // Everything up to the last `@` is userinfo; git allows a username here and some
    // setups paste more than that, so it is discarded exactly as `url` does above.
    let host = match authority.rsplit_once('@') {
        Some((_userinfo, host)) => host,
        None => authority,
    };
    // A one-character authority is a Windows drive letter (`C:\repos\vault`), never
    // a host — the same carve-out git makes. A longer bare name (`fileserver:repo`)
    // genuinely is a LAN host and is disclosed.
    let drive_letter = host.len() == 1 && host.starts_with(|c: char| c.is_ascii_alphabetic());
    if host.is_empty() || drive_letter {
        return None;
    }
    Some(host.to_ascii_lowercase())
}

/// Compute the live egress destination set from the accounts, the folder-sync
/// profiles' remotes and the update endpoint (Story 11.2, NFR-11; Story 23.7).
///
/// `accounts` is `[(homeserver_url, provider)]` read from the same registry the
/// session-restore path uses. `sync_remotes` is the `remote_url` of every profile
/// in the live folder-sync profile set, passed as plain strings because AD-40 bars
/// this crate from depending on `keeper-sync` — so adding a profile adds its host
/// here on the next read, and removing the profile removes it, with no cache to go
/// stale. The returned order is deterministic: each distinct homeserver (in
/// first-seen order, deduplicated), then `api.beeper.com` once when any account is
/// Beeper, then each distinct sync remote *host* (first-seen, deduplicated — two
/// profiles on one host are one entry, because they are one destination), then the
/// update endpoint last. Never panics and never uses `.unwrap()` — a malformed
/// homeserver URL is surfaced verbatim as a homeserver entry and is *not* treated
/// as Beeper (host detection needs a parseable URL), and a sync remote with no
/// network host (a local path, a pendrive) contributes nothing. Beeper presence
/// follows the single source of truth: `Provider::Beeper` **or**
/// [`is_beeper_homeserver`] on the URL.
pub fn compute_egress(
    accounts: &[(String, Provider)],
    sync_remotes: &[String],
    update_endpoint: &str,
) -> Vec<EgressEndpointVm> {
    let mut endpoints: Vec<EgressEndpointVm> = Vec::new();
    let mut seen_homeservers: Vec<String> = Vec::new();
    let mut any_beeper = false;

    for (homeserver_url, provider) in accounts {
        // An account is Beeper by its durable provider tag OR by its homeserver
        // host (the same OR test used elsewhere). A malformed URL can only be
        // Beeper via the provider tag — `is_beeper_homeserver` returns false for it.
        if *provider == Provider::Beeper || is_beeper_homeserver(homeserver_url) {
            any_beeper = true;
        }

        // Dedup identical homeserver strings so two accounts on one homeserver
        // collapse to a single entry. A malformed URL is shown verbatim (the raw
        // stored string) rather than dropped — honesty over prettiness.
        if !seen_homeservers.iter().any(|h| h == homeserver_url) {
            seen_homeservers.push(homeserver_url.clone());
            endpoints.push(EgressEndpointVm {
                url: homeserver_url.clone(),
                kind: EgressKind::Homeserver,
                label: "Matrix homeserver".to_owned(),
            });
        }
    }

    // `api.beeper.com` appears exactly once, iff any account is Beeper.
    if any_beeper {
        endpoints.push(EgressEndpointVm {
            url: BEEPER_API_ENDPOINT.to_owned(),
            kind: EgressKind::Beeper,
            label: "Beeper account service".to_owned(),
        });
    }

    // One entry per distinct remote host across the live profile set. Deduplicated
    // by host rather than by remote URL: a person with four repositories on one
    // forge has one destination, and four identical rows would overstate the
    // egress surface as surely as omitting one would understate it.
    let mut seen_remote_hosts: Vec<String> = Vec::new();
    for remote_url in sync_remotes {
        let Some(host) = remote_host(remote_url) else {
            continue;
        };
        if seen_remote_hosts.contains(&host) {
            continue;
        }
        seen_remote_hosts.push(host.clone());
        endpoints.push(EgressEndpointVm {
            url: host,
            kind: EgressKind::GitRemote,
            label: "Folder sync remote".to_owned(),
        });
    }

    // The signed-update endpoint is always present — keeper checks it regardless of
    // whether any account is signed in.
    endpoints.push(EgressEndpointVm {
        url: update_endpoint.to_owned(),
        kind: EgressKind::Update,
        label: "Signed app updates".to_owned(),
    });

    endpoints
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The canonical update endpoint used across the matrix tests.
    const UPDATE: &str = EGRESS_UPDATE_ENDPOINT;

    /// The account-only matrix rows below assert the disclosure for a machine with
    /// no folder-sync profile configured. Named rather than written as a bare `&[]`
    /// so each row still reads as a statement about the *whole* live state — "no
    /// profiles" is one of the inputs those rows pin, not an argument they happen
    /// to leave empty (Story 23.7).
    const NO_REMOTES: &[String] = &[];

    /// Convenience: the (kind, url) shape of the result, for terse assertions.
    fn shape(endpoints: &[EgressEndpointVm]) -> Vec<(EgressKind, &str)> {
        endpoints.iter().map(|e| (e.kind, e.url.as_str())).collect()
    }

    /// Matrix row 1 — No accounts: exactly one entry, the update endpoint.
    #[test]
    fn no_accounts_yields_only_the_update_endpoint() {
        let out = compute_egress(&[], NO_REMOTES, UPDATE);
        assert_eq!(shape(&out), vec![(EgressKind::Update, UPDATE)]);
    }

    /// Matrix row 2 — One non-Beeper account: [homeserver, update], no api.beeper.com.
    #[test]
    fn one_non_beeper_account_has_homeserver_and_update_only() {
        let accounts = vec![("https://matrix.example.org".to_owned(), Provider::Password)];
        let out = compute_egress(&accounts, NO_REMOTES, UPDATE);
        assert_eq!(
            shape(&out),
            vec![
                (EgressKind::Homeserver, "https://matrix.example.org"),
                (EgressKind::Update, UPDATE),
            ]
        );
        assert!(
            !out.iter().any(|e| e.kind == EgressKind::Beeper),
            "a non-Beeper account must not surface api.beeper.com"
        );
    }

    /// Matrix row 3a — One Beeper account by provider tag: homeserver + beeper + update.
    #[test]
    fn one_beeper_account_by_provider_adds_beeper_api() {
        let accounts = vec![("https://matrix.beeper.com".to_owned(), Provider::Beeper)];
        let out = compute_egress(&accounts, NO_REMOTES, UPDATE);
        assert_eq!(
            shape(&out),
            vec![
                (EgressKind::Homeserver, "https://matrix.beeper.com"),
                (EgressKind::Beeper, BEEPER_API_ENDPOINT),
                (EgressKind::Update, UPDATE),
            ]
        );
    }

    /// Matrix row 3b — One Beeper account detected by host (provider tag says
    /// Password, but the homeserver host is Beeper's): still adds api.beeper.com.
    #[test]
    fn one_beeper_account_by_host_adds_beeper_api() {
        let accounts = vec![(
            "https://matrix.beeper.com".to_owned(),
            // Deliberately not the Beeper provider tag — host detection must win.
            Provider::Password,
        )];
        let out = compute_egress(&accounts, NO_REMOTES, UPDATE);
        assert_eq!(
            shape(&out),
            vec![
                (EgressKind::Homeserver, "https://matrix.beeper.com"),
                (EgressKind::Beeper, BEEPER_API_ENDPOINT),
                (EgressKind::Update, UPDATE),
            ]
        );
    }

    /// Matrix row 4 — Multiple accounts on the same homeserver: dedup to one entry.
    #[test]
    fn duplicate_homeservers_collapse_to_one_entry() {
        let accounts = vec![
            ("https://matrix.example.org".to_owned(), Provider::Password),
            ("https://matrix.example.org".to_owned(), Provider::Oidc),
        ];
        let out = compute_egress(&accounts, NO_REMOTES, UPDATE);
        assert_eq!(
            shape(&out),
            vec![
                (EgressKind::Homeserver, "https://matrix.example.org"),
                (EgressKind::Update, UPDATE),
            ]
        );
        assert_eq!(
            out.iter()
                .filter(|e| e.kind == EgressKind::Homeserver)
                .count(),
            1,
            "identical homeservers must collapse to a single entry"
        );
    }

    /// Matrix row 5 — Multiple Beeper accounts: api.beeper.com appears exactly once.
    #[test]
    fn multiple_beeper_accounts_add_beeper_api_once() {
        let accounts = vec![
            ("https://matrix.beeper.com".to_owned(), Provider::Beeper),
            ("https://matrix.beeper.com".to_owned(), Provider::Beeper),
        ];
        let out = compute_egress(&accounts, NO_REMOTES, UPDATE);
        assert_eq!(
            out.iter().filter(|e| e.kind == EgressKind::Beeper).count(),
            1,
            "api.beeper.com must appear exactly once for multiple Beeper accounts"
        );
        // The Beeper homeserver also dedups.
        assert_eq!(
            shape(&out),
            vec![
                (EgressKind::Homeserver, "https://matrix.beeper.com"),
                (EgressKind::Beeper, BEEPER_API_ENDPOINT),
                (EgressKind::Update, UPDATE),
            ]
        );
    }

    /// Matrix row 6 — Malformed homeserver URL: entry shown using the raw stored
    /// string, not treated as Beeper, no panic.
    #[test]
    fn malformed_homeserver_url_is_shown_verbatim_and_not_beeper() {
        let accounts = vec![("not a url".to_owned(), Provider::Password)];
        let out = compute_egress(&accounts, NO_REMOTES, UPDATE);
        assert_eq!(
            shape(&out),
            vec![
                (EgressKind::Homeserver, "not a url"),
                (EgressKind::Update, UPDATE),
            ]
        );
        assert!(
            !out.iter().any(|e| e.kind == EgressKind::Beeper),
            "a malformed homeserver URL must not be treated as Beeper"
        );
    }

    /// A malformed URL with the Beeper *provider* tag still surfaces api.beeper.com
    /// (the provider tag is authoritative even when the URL is unparseable) — and the
    /// raw string is shown verbatim.
    #[test]
    fn malformed_url_with_beeper_provider_still_adds_beeper_api() {
        let accounts = vec![("::::garbage".to_owned(), Provider::Beeper)];
        let out = compute_egress(&accounts, NO_REMOTES, UPDATE);
        assert_eq!(
            shape(&out),
            vec![
                (EgressKind::Homeserver, "::::garbage"),
                (EgressKind::Beeper, BEEPER_API_ENDPOINT),
                (EgressKind::Update, UPDATE),
            ]
        );
    }

    /// A mixed fleet — a Beeper account plus a non-Beeper account on two distinct
    /// homeservers — surfaces both homeservers, api.beeper.com once, and the update
    /// endpoint (the Settings → About acceptance scenario).
    #[test]
    fn mixed_fleet_surfaces_both_homeservers_beeper_once_and_update() {
        let accounts = vec![
            ("https://matrix.example.org".to_owned(), Provider::Password),
            ("https://matrix.beeper.com".to_owned(), Provider::Beeper),
        ];
        let out = compute_egress(&accounts, NO_REMOTES, UPDATE);
        assert_eq!(
            shape(&out),
            vec![
                (EgressKind::Homeserver, "https://matrix.example.org"),
                (EgressKind::Homeserver, "https://matrix.beeper.com"),
                (EgressKind::Beeper, BEEPER_API_ENDPOINT),
                (EgressKind::Update, UPDATE),
            ]
        );
    }

    /// The account fixture the sync-remote rows below hold constant, so any change
    /// they show is caused by the profile set and nothing else.
    fn one_account() -> Vec<(String, Provider)> {
        vec![("https://matrix.example.org".to_owned(), Provider::Password)]
    }

    /// Story 23.7 AC, first half — adding a profile changes the disclosed set:
    /// exactly one new entry, carrying that profile's remote host.
    #[test]
    fn adding_a_sync_profile_discloses_exactly_one_entry_for_its_remote_host() {
        let accounts = one_account();
        let before = compute_egress(&accounts, NO_REMOTES, UPDATE);
        let remotes = vec!["https://github.com/tgorka/notes.git".to_owned()];
        let after = compute_egress(&accounts, &remotes, UPDATE);

        assert_eq!(
            shape(&after),
            vec![
                (EgressKind::Homeserver, "https://matrix.example.org"),
                (EgressKind::GitRemote, "github.com"),
                (EgressKind::Update, UPDATE),
            ]
        );
        assert_eq!(
            after.len(),
            before.len() + 1,
            "adding one profile must add exactly one disclosed destination"
        );
    }

    /// Story 23.7 AC, second half — removing the profile removes the entry, and the
    /// disclosure returns to exactly what it was before. Computed from the live set
    /// on every call, so there is no cache that could keep a departed host on screen.
    #[test]
    fn removing_the_sync_profile_removes_its_entry_again() {
        let accounts = one_account();
        let remotes = vec!["https://github.com/tgorka/notes.git".to_owned()];
        let with_profile = compute_egress(&accounts, &remotes, UPDATE);
        let without_profile = compute_egress(&accounts, NO_REMOTES, UPDATE);

        assert!(
            with_profile
                .iter()
                .any(|e| e.kind == EgressKind::GitRemote && e.url == "github.com"),
            "the profile's host must be disclosed while the profile exists"
        );
        assert_eq!(
            shape(&without_profile),
            vec![
                (EgressKind::Homeserver, "https://matrix.example.org"),
                (EgressKind::Update, UPDATE),
            ],
            "removing the profile must remove its entry and nothing else"
        );
    }

    /// Two profiles on one forge are one destination, so the host is disclosed once.
    /// Repeating it would overstate the egress surface — a list that inflates is as
    /// dishonest as one that omits, and this one is read as a security claim.
    #[test]
    fn two_profiles_on_the_same_host_disclose_that_host_once() {
        let remotes = vec![
            "https://github.com/tgorka/notes.git".to_owned(),
            // Same host, different repository — and a different case, which is the
            // same host to DNS and must be the same host here.
            "https://GitHub.com/tgorka/archive.git".to_owned(),
        ];
        let out = compute_egress(&one_account(), &remotes, UPDATE);

        assert_eq!(
            out.iter()
                .filter(|e| e.kind == EgressKind::GitRemote)
                .count(),
            1,
            "two profiles on one host must collapse to a single entry"
        );
        assert_eq!(
            shape(&out),
            vec![
                (EgressKind::Homeserver, "https://matrix.example.org"),
                (EgressKind::GitRemote, "github.com"),
                (EgressKind::Update, UPDATE),
            ]
        );
    }

    /// The interesting case, and the honest one: a profile whose remote is a local
    /// path or a pendrive (AD-48) reaches no network, so it discloses NO network
    /// egress. Naming a directory as a "destination" would fabricate a network claim
    /// in the one surface that exists to be unfabricated.
    #[test]
    fn a_local_path_or_pendrive_remote_discloses_no_network_egress() {
        let remotes = vec![
            "/Volumes/pendrive/vault.git".to_owned(),
            "file:///srv/repos/archive.git".to_owned(),
            "../sibling-mirror".to_owned(),
            "~/Repositories/notes.git".to_owned(),
            "C:\\Users\\tg\\vault".to_owned(),
            // A directory whose own name contains a colon: legal on disk, and not a
            // host. The colon comes after a slash, which is git's own scp-like test.
            "/srv/repos/a:b".to_owned(),
            "".to_owned(),
        ];
        let out = compute_egress(&one_account(), &remotes, UPDATE);

        assert_eq!(
            shape(&out),
            vec![
                (EgressKind::Homeserver, "https://matrix.example.org"),
                (EgressKind::Update, UPDATE),
            ],
            "a remote with no network host must add no egress entry at all"
        );
    }

    /// A username or a token in the remote URL must never reach the disclosed
    /// string. Settings → About is a screen people open to reassure themselves and
    /// show to other people; a token rendered there is a credential leak dressed as
    /// an honesty feature. Only the host survives.
    #[test]
    fn a_username_or_token_in_the_remote_url_never_reaches_the_disclosed_string() {
        let remotes = vec![
            // scp-like shorthand: the `git@` userinfo is dropped.
            "git@gitlab.example.org:team/notes.git".to_owned(),
            // https with a token pasted into the URL instead of the keychain.
            "https://oauth2:ghp_liveTokenValue@forge.example.net/team/archive.git".to_owned(),
            // ssh with an explicit user and port.
            "ssh://deploy@ssh.example.com:2222/srv/git/vault.git".to_owned(),
        ];
        let out = compute_egress(&one_account(), &remotes, UPDATE);

        assert_eq!(
            shape(&out),
            vec![
                (EgressKind::Homeserver, "https://matrix.example.org"),
                (EgressKind::GitRemote, "gitlab.example.org"),
                (EgressKind::GitRemote, "forge.example.net"),
                (EgressKind::GitRemote, "ssh.example.com"),
                (EgressKind::Update, UPDATE),
            ]
        );
        for endpoint in &out {
            for leak in [
                "git@",
                "oauth2",
                "ghp_liveTokenValue",
                "deploy",
                "team/notes.git",
                "team/archive.git",
                "/srv/git/vault.git",
            ] {
                assert!(
                    !endpoint.url.contains(leak),
                    "the disclosed string {:?} must not carry {leak:?} — a disclosure \
                     names the host, never the credential or the path",
                    endpoint.url
                );
            }
        }
    }

    /// The full acceptance scenario: accounts and profiles together, in the pinned
    /// order — homeservers, `api.beeper.com`, each sync host, the update endpoint
    /// last. The update endpoint stays last because it is the one destination that is
    /// true regardless of what the user configured.
    #[test]
    fn a_fleet_with_accounts_and_profiles_discloses_both_in_a_stable_order() {
        let accounts = vec![
            ("https://matrix.example.org".to_owned(), Provider::Password),
            ("https://matrix.beeper.com".to_owned(), Provider::Beeper),
        ];
        let remotes = vec![
            "https://github.com/tgorka/notes.git".to_owned(),
            "/Volumes/pendrive/vault.git".to_owned(),
            "git@gitlab.example.org:team/archive.git".to_owned(),
        ];
        let out = compute_egress(&accounts, &remotes, UPDATE);

        assert_eq!(
            shape(&out),
            vec![
                (EgressKind::Homeserver, "https://matrix.example.org"),
                (EgressKind::Homeserver, "https://matrix.beeper.com"),
                (EgressKind::Beeper, BEEPER_API_ENDPOINT),
                (EgressKind::GitRemote, "github.com"),
                (EgressKind::GitRemote, "gitlab.example.org"),
                (EgressKind::Update, UPDATE),
            ]
        );
    }

    /// NFR-11 exhaustiveness gate: every [`EgressKind`] this build can name is
    /// actually produced by the disclosure matrix above, and the set of kinds is
    /// closed.
    ///
    /// Two halves, and both are load-bearing. The wildcard-free `match` is the
    /// compile-time half: a new variant makes it non-exhaustive and the crate fails
    /// to COMPILE here, so a destination kind can never be added to the VM without
    /// this file acknowledging it. The loop is the runtime half: each kind must be
    /// reachable from real inputs, so a variant that exists but that `compute_egress`
    /// never emits — a disclosure the app claims it can make and cannot — fails too.
    ///
    /// Do NOT add a `_ =>` arm: a wildcard would silently absorb a new variant and
    /// defeat the gate. (Same idiom as `send::tests::exactly_two_legal_dispatch_triggers`.)
    #[test]
    fn every_egress_kind_is_produced_by_the_disclosure_matrix() {
        const ALL_KINDS: &[EgressKind] = &[
            EgressKind::Homeserver,
            EgressKind::Beeper,
            EgressKind::GitRemote,
            EgressKind::Update,
        ];

        // Wildcard-free exhaustive match: a new `EgressKind` variant makes this
        // non-exhaustive and the crate fails to compile HERE. NO `_ =>` arm.
        for kind in ALL_KINDS {
            match kind {
                EgressKind::Homeserver => {}
                EgressKind::Beeper => {}
                EgressKind::GitRemote => {}
                EgressKind::Update => {}
            }
        }

        // Everything switched on at once: a Beeper account and a networked profile.
        let accounts = vec![("https://matrix.beeper.com".to_owned(), Provider::Beeper)];
        let remotes = vec!["https://github.com/tgorka/notes.git".to_owned()];
        let out = compute_egress(&accounts, &remotes, UPDATE);

        for kind in ALL_KINDS {
            assert!(
                out.iter().any(|e| e.kind == *kind),
                "no entry of kind {kind:?} was disclosed for the everything-on fleet — \
                 an EgressKind the app can name but never produces is a claim with no \
                 code behind it"
            );
        }
        assert_eq!(
            out.len(),
            ALL_KINDS.len(),
            "the everything-on fleet must disclose exactly one entry per kind, so this \
             gate keeps covering every kind rather than one kind several times"
        );
    }

    /// The host derivation on its own, over the forms a `remote_url` actually takes
    /// (`SyncProfile::remote_url` documents "https, ssh or a local path"). Kept
    /// separate from the matrix rows so a parsing regression names itself rather
    /// than showing up as a puzzling missing row.
    #[test]
    fn remote_host_derives_the_host_and_only_the_host() {
        for (remote, expected) in [
            ("https://github.com/tgorka/notes.git", Some("github.com")),
            ("http://forge.local/notes.git", Some("forge.local")),
            (
                "ssh://git@ssh.example.com/team/notes.git",
                Some("ssh.example.com"),
            ),
            (
                "ssh://git@ssh.example.com:2222/team/notes.git",
                Some("ssh.example.com"),
            ),
            ("git://git.example.org/notes.git", Some("git.example.org")),
            ("git@github.com:tgorka/notes.git", Some("github.com")),
            ("github.com:tgorka/notes.git", Some("github.com")),
            (
                "https://Token@GITHUB.com/tgorka/notes.git",
                Some("github.com"),
            ),
            ("file:///srv/repos/notes.git", None),
            ("/Volumes/pendrive/notes.git", None),
            ("../mirror", None),
            ("~/Repositories/notes.git", None),
            ("C:\\Users\\tg\\vault", None),
            ("/srv/repos/a:b", None),
            ("   ", None),
            ("@:path", None),
            // A bracketed IPv6 literal carries colons of its own, so the scp-like
            // split is taken after the closing bracket: without that, the host would
            // come out as the single character `[`.
            ("[2001:db8::1]:/srv/repos/notes.git", Some("[2001:db8::1]")),
            ("git@[2001:db8::1]:notes.git", Some("[2001:db8::1]")),
        ] {
            assert_eq!(
                remote_host(remote).as_deref(),
                expected,
                "remote_host({remote:?})"
            );
        }
    }
}
