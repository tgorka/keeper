//! Egress honesty — the live set of network destinations keeper contacts (Story
//! 11.2, NFR-11, UX-DR17).
//!
//! keeper makes a *verifiable* egress claim: [`compute_egress`] derives, from live
//! app state, the exact set of hosts the app talks to — each account's Matrix
//! homeserver, plus Beeper's `api.beeper.com` when (and only when) a Beeper account
//! exists, plus the one signed auto-update endpoint. The Settings → About surface
//! renders this set directly (never a hardcoded doc), so the claim can never drift
//! from reality.
//!
//! Bridges are Matrix appservices reached *through* the homeserver (server-side),
//! so they add no distinct client egress — the homeserver entry is their egress. No
//! per-bridge host is fabricated here.
//!
//! The function is kept pure over `[(homeserver_url, Provider)]` (no `Platform`, no
//! Tauri runtime) so the whole I/O matrix is unit-testable; the `egress_list` IPC
//! command reads the registry rows and feeds them in.

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

/// Compute the live egress destination set from the accounts and the update
/// endpoint (Story 11.2, NFR-11).
///
/// `accounts` is `[(homeserver_url, provider)]` read from the same registry the
/// session-restore path uses. The returned order is deterministic: each distinct
/// homeserver (in first-seen order, deduplicated), then `api.beeper.com` once when
/// any account is Beeper, then the update endpoint last. Never panics and never
/// uses `.unwrap()` — a malformed homeserver URL is surfaced verbatim as a
/// homeserver entry and is *not* treated as Beeper (host detection needs a parseable
/// URL). Beeper presence follows the single source of truth: `Provider::Beeper`
/// **or** [`is_beeper_homeserver`] on the URL.
pub fn compute_egress(
    accounts: &[(String, Provider)],
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

    /// Convenience: the (kind, url) shape of the result, for terse assertions.
    fn shape(endpoints: &[EgressEndpointVm]) -> Vec<(EgressKind, &str)> {
        endpoints.iter().map(|e| (e.kind, e.url.as_str())).collect()
    }

    /// Matrix row 1 — No accounts: exactly one entry, the update endpoint.
    #[test]
    fn no_accounts_yields_only_the_update_endpoint() {
        let out = compute_egress(&[], UPDATE);
        assert_eq!(shape(&out), vec![(EgressKind::Update, UPDATE)]);
    }

    /// Matrix row 2 — One non-Beeper account: [homeserver, update], no api.beeper.com.
    #[test]
    fn one_non_beeper_account_has_homeserver_and_update_only() {
        let accounts = vec![("https://matrix.example.org".to_owned(), Provider::Password)];
        let out = compute_egress(&accounts, UPDATE);
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
        let out = compute_egress(&accounts, UPDATE);
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
        let out = compute_egress(&accounts, UPDATE);
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
        let out = compute_egress(&accounts, UPDATE);
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
        let out = compute_egress(&accounts, UPDATE);
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
        let out = compute_egress(&accounts, UPDATE);
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
        let out = compute_egress(&accounts, UPDATE);
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
        let out = compute_egress(&accounts, UPDATE);
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
}
