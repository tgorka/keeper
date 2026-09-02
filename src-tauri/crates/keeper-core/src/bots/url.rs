//! The base-URL grammar for an AI provider (Story 61.1, FR-369, AD-146).
//!
//! A provider's base URL is the one field a person types by hand, so it is the
//! one field that must be decided rather than trusted. The decision lives here,
//! pure and separately tested, because it is the whole of keeper's answer to the
//! SSRF question and that answer is unusual: **a loopback or private-network
//! endpoint is the normal case**, not the attack. Ollama's own documentation
//! points every user at `http://localhost:11434`, and Hermes' api-server binds
//! `127.0.0.1:8642` by default (research §3.1, §2.2), so a blocklist over
//! private ranges — the OWASP cheat sheet's "case 2" advice for a server taking
//! arbitrary URLs — would reject the two endpoints this feature exists to reach.
//!
//! keeper answers it the way it answers every other reach-outside question:
//! **by disclosure plus an explicit user act**. The grammar therefore accepts a
//! loopback and a private host and reports [`BaseUrl::is_private`] so the
//! surface can say so, and the destination is disclosed again, host-only, in
//! Settings → About (`crate::egress`). What the grammar *refuses* is the set of
//! shapes that cannot be part of an honest disclosure at all: a non-HTTP
//! scheme, a credential smuggled into the authority, a query or fragment that
//! would be silently dropped when a path is appended, and a path that is not a
//! prefix.

use std::net::IpAddr;

/// Why a typed base URL was refused (Story 61.1).
///
/// One variant per refusable shape, each naming the shape rather than the rule:
/// the surface renders these sentences verbatim, and "invalid URL" is the kind
/// of copy that makes a person retype the same string twice.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum BaseUrlError {
    /// Nothing was typed. Distinct from [`BaseUrlError::NotAUrl`] because there
    /// is nothing to point at, and because keeper ships no default endpoint —
    /// an empty field is the normal starting state, not a mistake yet.
    #[error("a provider needs a base URL — there is nothing here yet")]
    Empty,

    /// The string is not a URL at all. Carries what was read so the sentence can
    /// show it back.
    #[error("this does not read as a URL: {0}")]
    NotAUrl(String),

    /// A scheme other than `http`/`https`. `file:`, `data:`, `gopher:` and
    /// their kin are the classic SSRF escapes, and none of them can carry an
    /// OpenAI-compatible chat completion.
    #[error("keeper talks to a provider over http or https — {0} is not something it can speak")]
    UnsupportedScheme(String),

    /// A username or password in the authority (`https://user:pass@host`).
    ///
    /// Refused rather than stripped, and this is the load-bearing one: a base
    /// URL is shown on screens people share, is logged in error text, and is
    /// disclosed in Settings → About. A token pasted into the authority would
    /// leak through all three. A provider's credential goes to the secret port
    /// ([`crate::bots::provider_token_key`]) and nowhere else.
    #[error("a base URL must not carry a username or password — the credential goes in the token field, which keeper stores in the keychain")]
    Userinfo,

    /// No host to reach: `http://` is a scheme and nothing else.
    #[error("a base URL needs a host — this one names none")]
    MissingHost,

    /// A `?query=…`. Every request appends its own path, so a query on the base
    /// would either be dropped or moved before the path — both silent, and one
    /// of them changes where the bytes go.
    #[error("a base URL must not carry a query string — keeper appends its own path to it, and the query would be dropped")]
    Query,

    /// A `#fragment`. Never sent to a server, so it can only mislead the person
    /// reading the field back.
    #[error("a base URL must not carry a #fragment — it is never sent to the server")]
    Fragment,

    /// A path that is not a plain prefix: an empty (`//`), `.` or `..` segment.
    /// A prefix is joined to a request path verbatim, so a traversal segment
    /// would silently retarget every request made through this provider.
    #[error("a base URL may end in a path prefix, but not one containing {0} — keeper joins its own paths onto it")]
    PathNotAPrefix(String),
}

/// A base URL that passed the grammar (Story 61.1).
///
/// Holds the *normalized* form rather than what was typed, so two people who
/// entered `http://localhost:11434` and `http://localhost:11434/` end up with
/// one stored string and one egress row. `host` is kept separately because it
/// is what the disclosure surface shows and what a credential is bound to (a
/// stored token is only ever sent to the host it was entered for).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BaseUrl {
    /// `scheme://host[:port][/prefix]`, lowercased host, no trailing slash.
    /// This is what a provider row stores and what [`crate::bots::Endpoint`]
    /// joins request paths onto.
    pub normalized: String,
    /// The host alone, lowercased, brackets kept for an IPv6 literal — the same
    /// shape `crate::egress::remote_host` yields for a git remote, so the two
    /// disclosures read alike.
    pub host: String,
    /// The explicit port, or `None` when the scheme's default applies. Present
    /// as a fact rather than folded into `host` because "which port" is the
    /// question a person debugging a local Ollama actually asks.
    pub port: Option<u16>,
    /// The host is loopback, private, link-local, or a name that resolves only
    /// on this machine or this LAN.
    ///
    /// **Accepted, and disclosed.** This is not a refusal flag: a private
    /// endpoint is the ordinary case for both supported provider kinds. It
    /// exists so the surface can tell the user which side of their network the
    /// bytes stay on, which is the disclosure that replaces a blocklist.
    pub is_private: bool,
}

/// Decide whether `input` is a base URL keeper can talk to, and what it is
/// (Story 61.1, FR-369).
///
/// Pure and total: every input either yields a [`BaseUrl`] or a
/// [`BaseUrlError`] that names the shape it refused. The normalization is
/// deliberate — a trailing slash is dropped so the stored string is canonical,
/// and the host is lowercased so `HTTP://LocalHost:11434` and
/// `http://localhost:11434` are one provider and one egress row.
pub fn parse_base_url(input: &str) -> Result<BaseUrl, BaseUrlError> {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return Err(BaseUrlError::Empty);
    }

    // Parsed with `url` rather than by hand for the reason `egress::remote_host`
    // gives: userinfo, IDNA, IPv6 literals and default ports are a pile of rules
    // written by people who have seen every shape. A scheme-relative string
    // (`localhost:11434`) *parses* here — as a scheme of `localhost` — so the
    // scheme check below is what rejects it, and it must come first.
    //
    // `EmptyHost` is separated out because it is the ONE parse failure that has
    // its own sentence: `http://` is a person who has typed the scheme and not
    // the endpoint yet, and telling them "this does not read as a URL" would be
    // both true and useless. Every other parse failure is genuinely "not a URL".
    let parsed = url::Url::parse(trimmed).map_err(|error| match error {
        url::ParseError::EmptyHost => BaseUrlError::MissingHost,
        _ => BaseUrlError::NotAUrl(trimmed.to_owned()),
    })?;

    let scheme = parsed.scheme();
    if scheme != "http" && scheme != "https" {
        return Err(BaseUrlError::UnsupportedScheme(scheme.to_owned()));
    }
    if !parsed.username().is_empty() || parsed.password().is_some() {
        return Err(BaseUrlError::Userinfo);
    }
    if parsed.query().is_some() {
        return Err(BaseUrlError::Query);
    }
    if parsed.fragment().is_some() {
        return Err(BaseUrlError::Fragment);
    }
    // Belt and braces after the `EmptyHost` mapping above: for a special scheme
    // `url` will not produce a host-less URL, so neither arm is reachable today
    // — but the function must stay total over whatever a future `url` version
    // parses, and "no host" has exactly one honest answer.
    let Some(host) = parsed.host() else {
        return Err(BaseUrlError::MissingHost);
    };
    let host_text = host.to_string();
    if host_text.is_empty() {
        return Err(BaseUrlError::MissingHost);
    }

    // A path is allowed as a prefix (a reverse proxy mounts Ollama under
    // `/ollama` often enough), but only as a literal one. An empty segment
    // means a `//`, which some proxies collapse and some route differently, so
    // a stored prefix containing one does not describe one destination.
    //
    // `.` and `..` are deliberately NOT tested for here: the WHATWG parser has
    // already resolved them, so `https://h/a/../b` reaches this line as `/b` —
    // a plain prefix, and the join in `Endpoint::url` is literal from then on.
    // A test pins that resolution, because the reason this check is short is
    // easy to mistake for the check having been forgotten.
    let path = parsed.path().trim_end_matches('/');
    for segment in path.split('/').skip(1) {
        if segment.is_empty() {
            return Err(BaseUrlError::PathNotAPrefix("an empty segment".to_owned()));
        }
    }

    // `Position::BeforePath` is scheme + authority exactly as `url` normalized
    // it — lowercased host, bracketed IPv6 literal, default port already
    // dropped — so the canonical form is assembled from the parser's own output
    // rather than reformatted from the parts.
    let origin = &parsed[..url::Position::BeforePath];
    Ok(BaseUrl {
        normalized: format!("{origin}{path}"),
        is_private: host_is_private(&host),
        host: host_text,
        port: parsed.port(),
    })
}

/// Whether a host reaches only this machine or this LAN (Story 61.1).
///
/// Kept as one decision over the parsed [`url::Host`] rather than a string
/// scan, because the interesting cases are numeric: `127.0.0.1`, `::1`,
/// `10.x`/`172.16-31.x`/`192.168.x`, `169.254.x`, `fc00::/7` and `fe80::/10`.
/// The name arms cover the reserved local suffixes plus the bare single-label
/// name, which by definition resolves through the local resolver and nowhere
/// else.
///
/// A name is judged by its spelling and never by resolving it: this crate does
/// no DNS, and a verdict that changed with the network would make the
/// disclosure a moving claim. The flag disclosed here is therefore about the
/// URL, and the epic's honesty rule is satisfied by saying so rather than by
/// pretending to have probed.
fn host_is_private(host: &url::Host<&str>) -> bool {
    match host {
        url::Host::Ipv4(ip) => {
            ip.is_loopback() || ip.is_private() || ip.is_link_local() || ip.is_unspecified()
        }
        url::Host::Ipv6(ip) => {
            let first = ip.segments()[0];
            // `Ipv6Addr::is_unique_local` / `is_unicast_link_local` are still
            // unstable, so the two prefixes are tested directly: fc00::/7
            // (unique local) and fe80::/10 (link local).
            ip.is_loopback()
                || ip.is_unspecified()
                || (first & 0xfe00) == 0xfc00
                || (first & 0xffc0) == 0xfe80
        }
        url::Host::Domain(name) => {
            let lower = name.to_ascii_lowercase();
            // A literal typed into a `Domain` slot by a parser that did not
            // recognize it (rare, but `url` hands back a domain for some
            // shapes) still deserves the numeric verdict.
            if let Ok(ip) = lower.parse::<IpAddr>() {
                return match ip {
                    IpAddr::V4(v4) => v4.is_loopback() || v4.is_private() || v4.is_link_local(),
                    IpAddr::V6(v6) => v6.is_loopback(),
                };
            }
            lower == "localhost"
                || lower.ends_with(".localhost")
                || lower.ends_with(".local")
                || lower.ends_with(".internal")
                || lower.ends_with(".home.arpa")
                || !lower.contains('.')
        }
    }
}

/// Why a bot target — a Hermes profile name or an Ollama model tag — was
/// refused (Story 61.1).
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum BotTargetError {
    /// Nothing was typed.
    #[error("a bot needs a name — there is nothing here yet")]
    Empty,
    /// Longer than [`MAX_BOT_TARGET_CHARS`].
    #[error("a bot name longer than {MAX_BOT_TARGET_CHARS} characters is not one either provider would accept")]
    TooLong,
    /// A character that would change the URL rather than name a bot.
    #[error("a bot name cannot contain {0} — keeper puts it in the request URL, where that character means something else")]
    IllegalCharacter(char),
}

/// The longest bot target keeper stores. A Hermes profile is a directory name
/// under `~/.hermes/profiles/` and an Ollama tag is a `model:tag` pair; neither
/// approaches this, so the cap only bounds what a paste can put in a URL.
pub const MAX_BOT_TARGET_CHARS: usize = 128;

/// Decide whether `input` can be a bot target (Story 61.1, FR-376).
///
/// **Why this is a grammar and not a trim.** For Hermes the target goes into the
/// request path as `/p/{bot}` (research §2.2), so a `/`, a `?`, a `#` or a `..`
/// in it does not name a bot — it retargets the request, and the URL keeper
/// disclosed is no longer the URL keeper called. The allowed set is therefore
/// the intersection of what both providers actually use for a name: ASCII
/// alphanumerics plus `. _ - :` (the colon is Ollama's `model:tag` separator),
/// with `.` never leading, which is what makes `.` and `..` unrepresentable.
pub fn parse_bot_target(input: &str) -> Result<&str, BotTargetError> {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return Err(BotTargetError::Empty);
    }
    if trimmed.chars().count() > MAX_BOT_TARGET_CHARS {
        return Err(BotTargetError::TooLong);
    }
    // A leading dot is refused as the character it is, which covers `.` and
    // `..` without a second rule and without a special-cased message.
    if trimmed.starts_with('.') {
        return Err(BotTargetError::IllegalCharacter('.'));
    }
    if let Some(bad) = trimmed
        .chars()
        .find(|c| !(c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-' | ':')))
    {
        return Err(BotTargetError::IllegalCharacter(bad));
    }
    Ok(trimmed)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The happy path for both provider kinds' documented defaults, and the
    /// normalization that makes two spellings one provider.
    #[test]
    fn the_two_endpoints_this_feature_exists_for_are_accepted_and_normalized() {
        for input in [
            "http://localhost:11434",
            "http://localhost:11434/",
            "  http://localhost:11434  ",
            "HTTP://LocalHost:11434",
        ] {
            let parsed = parse_base_url(input).expect("the Ollama default must parse");
            assert_eq!(
                parsed.normalized, "http://localhost:11434",
                "parse_base_url({input:?}) must normalize to one canonical string"
            );
            assert_eq!(parsed.host, "localhost");
            assert_eq!(parsed.port, Some(11434));
        }

        let hermes = parse_base_url("http://127.0.0.1:8642").expect("the Hermes default");
        assert_eq!(hermes.normalized, "http://127.0.0.1:8642");
        assert_eq!(hermes.host, "127.0.0.1");
    }

    /// A path prefix survives as a prefix — a reverse proxy that mounts a
    /// provider under a subpath is a real deployment, and the trailing slash is
    /// still dropped so the join in `Endpoint::url` never doubles it.
    #[test]
    fn a_path_prefix_is_kept_and_its_trailing_slash_is_dropped() {
        let parsed = parse_base_url("https://gw.example.org/ollama/").expect("prefix accepted");
        assert_eq!(parsed.normalized, "https://gw.example.org/ollama");
        assert_eq!(parsed.host, "gw.example.org");
        assert_eq!(
            parsed.port, None,
            "the https default port is not an explicit port"
        );
        assert!(!parsed.is_private);
    }

    /// The refusal this story's mutation proof targets: a credential in the
    /// authority is refused, never stripped and never stored, because a base URL
    /// is shown on screens, logged in errors, and disclosed in Settings → About.
    #[test]
    fn a_base_url_carrying_userinfo_is_refused_by_name() {
        for input in [
            "https://oauth2:ghp_live@gw.example.org/v1",
            "https://token@gw.example.org",
            "http://user:@localhost:11434",
        ] {
            assert_eq!(
                parse_base_url(input),
                Err(BaseUrlError::Userinfo),
                "parse_base_url({input:?}) must refuse the credential rather than strip it"
            );
        }
    }

    /// Every other refusable shape, each naming itself.
    #[test]
    fn the_grammar_refuses_each_shape_by_name() {
        assert_eq!(parse_base_url(""), Err(BaseUrlError::Empty));
        assert_eq!(parse_base_url("   "), Err(BaseUrlError::Empty));
        assert_eq!(
            parse_base_url("file:///srv/models"),
            Err(BaseUrlError::UnsupportedScheme("file".to_owned()))
        );
        assert_eq!(
            parse_base_url("ws://localhost:8642"),
            Err(BaseUrlError::UnsupportedScheme("ws".to_owned()))
        );
        // The trap `egress::remote_host` documents from the other side: this
        // string parses, as a scheme of `localhost`, so only the scheme check
        // catches it. Without that check it would be stored as a "URL" that no
        // client can call.
        assert_eq!(
            parse_base_url("localhost:11434"),
            Err(BaseUrlError::UnsupportedScheme("localhost".to_owned()))
        );
        assert_eq!(
            parse_base_url("not a url"),
            Err(BaseUrlError::NotAUrl("not a url".to_owned()))
        );
        // A scheme with nothing after it: the state a half-typed field is in,
        // and it gets its own sentence rather than "not a URL".
        assert_eq!(parse_base_url("http://"), Err(BaseUrlError::MissingHost));
        assert_eq!(parse_base_url("https://"), Err(BaseUrlError::MissingHost));
        // Pinned because it surprises: the WHATWG URL parser tolerates the extra
        // slash and reads `v1` as the HOST, so this is accepted as a
        // single-label (LAN) host rather than refused. Accepted deliberately —
        // a bare LAN name IS legitimate here (`http://hermes:8642`), and the
        // grammar cannot tell a typo from a hostname. What protects the user is
        // that the host is then disclosed, which is this module's whole thesis.
        let odd = parse_base_url("http:///v1").expect("a triple slash names host v1");
        assert_eq!(odd.host, "v1");
        assert!(odd.is_private);
        assert_eq!(
            parse_base_url("https://gw.example.org/v1?key=sk-live"),
            Err(BaseUrlError::Query)
        );
        assert_eq!(
            parse_base_url("https://gw.example.org/v1#top"),
            Err(BaseUrlError::Fragment)
        );
        assert_eq!(
            parse_base_url("https://gw.example.org/a//b"),
            Err(BaseUrlError::PathNotAPrefix("an empty segment".to_owned()))
        );
    }

    /// Why the path check does not mention `.` or `..`: the parser resolves
    /// them before this module sees the path, so what gets stored is already a
    /// plain prefix and the join in `Endpoint::url` stays literal.
    ///
    /// Pinned as a test because a short check reads like a forgotten one, and
    /// the next person to widen it would be adding a rule for a shape that
    /// cannot arrive.
    #[test]
    fn a_traversal_segment_is_resolved_by_the_parser_not_stored() {
        let parsed = parse_base_url("https://gw.example.org/a/../b").expect("resolved");
        assert_eq!(parsed.normalized, "https://gw.example.org/b");
        let dotted = parse_base_url("https://gw.example.org/a/./b").expect("resolved");
        assert_eq!(dotted.normalized, "https://gw.example.org/a/b");
        // And a traversal that climbs past the root cannot escape it: there is
        // no shape here that yields a prefix outside the host.
        let climbed = parse_base_url("https://gw.example.org/../../etc").expect("resolved");
        assert_eq!(climbed.normalized, "https://gw.example.org/etc");
    }

    /// The SSRF answer, stated as a test: the private cases are ACCEPTED, and
    /// each one is disclosed. A blocklist here would reject both provider
    /// kinds' documented defaults, which is why this file has none.
    #[test]
    fn loopback_and_lan_hosts_are_accepted_and_marked_private() {
        for input in [
            "http://localhost:11434",
            "http://127.0.0.1:8642",
            "http://[::1]:8642",
            "http://10.0.0.7:11434",
            "http://172.16.4.4:11434",
            "http://192.168.1.20:11434",
            "http://169.254.10.1:11434",
            "http://[fd00::1]:11434",
            "http://[fe80::1]:11434",
            "http://ollama.local:11434",
            "http://gw.internal:11434",
            "http://hermes:8642",
        ] {
            let parsed = parse_base_url(input).expect("a private endpoint is the normal case");
            assert!(
                parsed.is_private,
                "parse_base_url({input:?}) must disclose a private host, not hide it"
            );
        }
    }

    /// The other half of the same claim: a public host is not mislabelled, or
    /// the disclosure would say "stays on your machine" about the open internet.
    #[test]
    fn a_public_host_is_not_marked_private() {
        for input in [
            "https://api.openai.com/v1",
            "https://gw.example.org",
            "http://8.8.8.8:11434",
            "https://[2001:db8::1]",
        ] {
            let parsed = parse_base_url(input).expect("a public endpoint parses");
            assert!(
                !parsed.is_private,
                "parse_base_url({input:?}) must not claim a public host is private"
            );
        }
    }

    /// A bot target names a bot; it never moves the request.
    #[test]
    fn a_bot_target_that_would_retarget_the_url_is_refused() {
        assert_eq!(parse_bot_target("grokbot"), Ok("grokbot"));
        assert_eq!(parse_bot_target("  llama3.1:8b  "), Ok("llama3.1:8b"));
        assert_eq!(parse_bot_target("qwen2.5-coder_7b"), Ok("qwen2.5-coder_7b"));

        assert_eq!(parse_bot_target(""), Err(BotTargetError::Empty));
        assert_eq!(parse_bot_target("   "), Err(BotTargetError::Empty));
        assert_eq!(
            parse_bot_target(".."),
            Err(BotTargetError::IllegalCharacter('.'))
        );
        assert_eq!(
            parse_bot_target("../v1/models"),
            Err(BotTargetError::IllegalCharacter('.'))
        );
        assert_eq!(
            parse_bot_target("team/bot"),
            Err(BotTargetError::IllegalCharacter('/'))
        );
        assert_eq!(
            parse_bot_target("bot?x=1"),
            Err(BotTargetError::IllegalCharacter('?'))
        );
        assert_eq!(
            parse_bot_target("bot#frag"),
            Err(BotTargetError::IllegalCharacter('#'))
        );
        assert_eq!(
            parse_bot_target(&"a".repeat(MAX_BOT_TARGET_CHARS + 1)),
            Err(BotTargetError::TooLong)
        );
    }
}
