//! The one redactor a log or frontend-error text passes through before it may
//! leave this device for PostHog (R461, as fixed by R495).
//!
//! It fails closed: where a class cannot tell where a secret ends, everything
//! after its start goes. A log line is redacted per part — the message, then
//! each field's RAW value ([`redact_field`]) before anything quotes or
//! escapes it — and only then joined and capped ([`cap`]), so no cap can cut
//! a secret into a fragment no class recognises. Text that had to be cut
//! before it reached here (a webview's report) is [`bound`]ed first, which
//! drops the token the cut went through.
//!
//! Two passes. Classes 1–8 are found as byte spans of the text, and classes
//! 9–10 (long hex and base64 runs) on what the first pass left, so a run
//! cannot start in a host name and carry the host away with the path after
//! it. Within a pass, overlapping spans merge into one (the earliest span's
//! marker wins), so a byte any class flags is never written out. Patterns are
//! the `regex` crate's: linear time, no backtracking, so a hostile message
//! cannot stall the thread that logged it. The classes, in the order a tie at
//! one position is resolved:
//!
//! 1. `syt_` / `phc_` / `phx_` / `phs_` tokens → `<token>`
//! 2. the credential after `Bearer` → `<token>`
//! 3. a credential assignment — a name ending in `password`, `passwd`,
//!    `passphrase`, `secret`, `token`, `key`, `authorization`, `cookie` or
//!    `credential(s)`, then `=` or `:`, in any quoting or escaping — →
//!    `<redacted>` **from the assignment to the end of the text**
//! 4. a URL's user information → `<redacted>`, and its path, query and
//!    fragment → `/<redacted>`; the scheme and host stay
//! 5. an email address → `<email>`
//! 6. a Matrix user id, room id, room alias or event id with a server →
//!    `<matrix-id>`
//! 7. a Matrix room or event id without a server (`$…`, `!…`: room versions
//!    with hashed ids) → `<matrix-id>`
//! 8. a home path (`~/`, `/Users/`, `/home/`, `/root/`, the phone
//!    containers, `C:\Users\` with any number of backslashes, `C:/Users/`;
//!    `Users` in any ASCII letter case, since macOS and Windows volumes are
//!    case-insensitive and keep the spelling a person typed) → `<path>`,
//!    together with everything after its root: to the end of the text — the
//!    field's value, or the message — in every quoting. No quote, space or
//!    backslash can say for certain where a filename ends (one may hold any
//!    of them), so nothing after a home root is kept (R515). A home root
//!    inside a `file://` URL (`file:///Users/…`, `file:///C:/Users/…`) is
//!    one too; it is looked for before class 4, so the URL's own marker
//!    never stands in for `<path>` and its path's end is never guessed (R518)
//! 9. a run of 16 or more hex digits → `<hex>`
//! 10. a run of 24 or more base64 characters → `<base64>`, whatever its
//!     letters' case or digits (R461; no exemption)
//!
//! A field whose NAME ends in one of class 3's words is replaced whole by
//! [`redact_field`] before any class runs.

use std::sync::LazyLock;

use regex::Regex;

/// The most characters of a redacted message that leave; R461.
pub const MESSAGE_CAP: usize = 1_000;

/// The words a credential's name ends in (class 3, and a field's name).
const CREDENTIAL_WORDS: &[&str] = &[
    "password",
    "passwd",
    "passphrase",
    "secret",
    "token",
    "key",
    "authorization",
    "cookie",
    "credential",
    "credentials",
];

/// How far a class's match reaches.
#[derive(Clone, Copy)]
enum Extent {
    /// The capture group's own span.
    Group,
    /// From the end of the capture group to the end of the text.
    ToEnd,
    /// From the capture group's start to the end of the text.
    FromStart,
}

struct Class {
    pattern: Regex,
    /// The capture group the extent is measured from; 0 is the whole match.
    group: usize,
    extent: Extent,
    marker: &'static str,
}

/// `Users` matches in any ASCII case (R518): `/users/alice` names
/// `/Users/alice` on a case-insensitive volume. Spelled out letter by letter
/// rather than `(?i)`, whose Unicode folding would also admit `ſ` (U+017F).
const HOME_ROOTS: &str = r"(?:~|/[Uu][Ss][Ee][Rr][Ss]|/home|/root|/private/var/mobile|/var/mobile|/data/user|/data/data|/storage/emulated|[A-Za-z]:(?:\\+|/)[Uu][Ss][Ee][Rr][Ss])";

fn class(pattern: &str, group: usize, extent: Extent, marker: &'static str) -> Class {
    Class {
        // A literal of this file, compiled by every test below.
        pattern: Regex::new(pattern).expect("redaction pattern"),
        group,
        extent,
        marker,
    }
}

/// Classes 1–8 of the module header, found on the original text.
static STRUCTURE: LazyLock<Vec<Class>> = LazyLock::new(|| {
    vec![
        class(
            r"\b(?:syt|phc|phx|phs)_[A-Za-z0-9_\-.=]+",
            0,
            Extent::Group,
            "<token>",
        ),
        class(
            r#"(?i)\bbearer[ \t]+([^\s"'<>\\]+)"#,
            1,
            Extent::Group,
            "<token>",
        ),
        // The separator is group 1; `::` (a Rust path) is not one.
        class(
            r#"(?i)[A-Za-z0-9_\-]*(?:password|passwd|passphrase|secret|token|key|authorization|cookie|credentials?)(?:\\*["'])?[ \t]*(=|:)(?:[^:]|$)"#,
            1,
            Extent::ToEnd,
            "<redacted>",
        ),
        // A home root inside a `file://` URL (R518): `<path>` from the URL
        // path's first slash to the end of the text. Ahead of the URL
        // classes, so a tie at that slash is this class's.
        class(
            &format!(r#"(?i:file://(?:localhost)?)(/?{HOME_ROOTS}(?:\\+|/))"#),
            1,
            Extent::FromStart,
            "<path>",
        ),
        // User information: everything between `://` and an `@` before the host.
        class(
            r#"(?i)\b[a-z][a-z0-9+.\-]*://([^\s/?#@"'<>\\]+)@"#,
            1,
            Extent::Group,
            "<redacted>",
        ),
        // Path, query and fragment: from the first `/`, `?` or `#` after the host.
        class(
            r#"(?i)\b[a-z][a-z0-9+.\-]*://[^\s/?#"'<>\\]*([/?#][^\s"'<>]+)"#,
            1,
            Extent::Group,
            "/<redacted>",
        ),
        class(
            r"[A-Za-z0-9._%+\-]+@[A-Za-z0-9\-]+(?:\.[A-Za-z0-9\-]+)*\.[A-Za-z]{2,}",
            0,
            Extent::Group,
            "<email>",
        ),
        class(
            r"(?:^|[^A-Za-z0-9._/+\-])([@!#$][A-Za-z0-9._=/+\-]+:(?:\[[0-9A-Fa-f:.]+\]|[A-Za-z0-9\-]+(?:\.[A-Za-z0-9\-]+)*)(?::[0-9]{1,5})?)",
            1,
            Extent::Group,
            "<matrix-id>",
        ),
        class(
            r"(?:^|[^A-Za-z0-9._/+\-])([$!][A-Za-z0-9+/_\-]{16,}=*)",
            1,
            Extent::Group,
            "<matrix-id>",
        ),
        class(
            &format!(r#"(?:^|[\s"'`=(\[{{,:;<>|\\])({HOME_ROOTS}(?:\\+|/))"#),
            1,
            Extent::FromStart,
            "<path>",
        ),
    ]
});

/// Classes 9–10, found on the text the first pass left: a run never starts
/// in a host and swallows the URL path or home path that follows it, because
/// those are markers by then and `<`/`>` end every run.
static RUNS: LazyLock<Vec<Class>> = LazyLock::new(|| {
    vec![
        class(r"[0-9A-Fa-f]{16,}", 0, Extent::Group, "<hex>"),
        class(r"[A-Za-z0-9+/_\-]{24,}=*", 0, Extent::Group, "<base64>"),
    ]
});

/// `text` as it may leave this device: every span a class flags replaced by
/// that class's marker, capped at [`MESSAGE_CAP`] characters.
pub fn redact(text: &str) -> String {
    cap(redact_uncapped(text))
}

/// [`redact`] without the cap: one part of a line, joined with the others
/// before the whole is capped.
pub fn redact_uncapped(text: &str) -> String {
    apply(&RUNS, &apply(&STRUCTURE, text))
}

/// One field of a log event, from its RAW value — a string field as
/// written, anything else as its `Debug`/`Display` text — before anything
/// quotes or escapes it. A field named like a credential, or whose value
/// holds a credential assignment anywhere, is replaced whole; everything
/// else goes through the classes, a home path running to the value's end.
pub fn redact_field(name: &str, raw: &str) -> String {
    if credential_name(name) || STRUCTURE[2].pattern.is_match(raw) {
        return "<redacted>".to_owned();
    }
    redact_uncapped(raw)
}

/// Whether a field's name says it holds a credential.
fn credential_name(name: &str) -> bool {
    let name = name.to_ascii_lowercase();
    CREDENTIAL_WORDS.iter().any(|word| name.ends_with(word))
}

/// Text that arrived from outside (the webview) cut to at most `max`
/// characters BEFORE it is redacted. A cut that went through a token drops
/// that token whole — and the one it may have ended at, since a cut that
/// lands on a boundary cannot be told from one inside a token — so no
/// fragment shorter than its class's threshold is left to slip through.
pub fn bound(text: &str, max: usize) -> String {
    if text.chars().count() <= max {
        return text.to_owned();
    }
    let cut: String = text.chars().take(max).collect();
    let kept = cut.trim_end_matches(|c: char| !c.is_whitespace());
    format!("{kept}…")
}

/// One pass: every class's spans, merged where they overlap (the earliest
/// span's marker, the earlier class on a tie at one start).
fn apply(classes: &[Class], text: &str) -> String {
    let mut spans: Vec<(usize, usize, usize)> = Vec::new();
    for (index, class) in classes.iter().enumerate() {
        for captures in class.pattern.captures_iter(text) {
            let Some(found) = captures.get(class.group) else {
                continue;
            };
            let (start, end) = match class.extent {
                Extent::Group => (found.start(), found.end()),
                Extent::ToEnd => (found.end(), text.len()),
                Extent::FromStart => (found.start(), text.len()),
            };
            if start < end {
                spans.push((start, end, index));
            }
        }
    }
    spans.sort_unstable_by_key(|&(start, _, index)| (start, index));
    let mut merged: Vec<(usize, usize, &'static str)> = Vec::new();
    for (start, end, index) in spans {
        match merged.last_mut() {
            Some(last) if start < last.1 => last.1 = last.1.max(end),
            _ => merged.push((start, end, classes[index].marker)),
        }
    }
    let mut out = String::with_capacity(text.len());
    let mut at = 0;
    for (start, end, marker) in merged {
        out.push_str(&text[at..start]);
        out.push_str(marker);
        at = end;
    }
    out.push_str(&text[at..]);
    out
}

/// At most [`MESSAGE_CAP`] characters; a cut message ends in `…`. Runs only
/// on redacted text.
pub fn cap(text: String) -> String {
    if text.chars().count() <= MESSAGE_CAP {
        return text;
    }
    let mut capped: String = text.chars().take(MESSAGE_CAP - 1).collect();
    capped.push('…');
    capped
}

#[cfg(test)]
mod tests {
    use super::*;

    #[track_caller]
    fn check(input: &str, expected: &str) {
        assert_eq!(redact(input), expected, "redacting {input:?}");
    }

    #[test]
    fn ordinary_sentences_pass_unchanged() {
        for text in [
            "sync: the push was refused by the server (HTTP 403)",
            "agents: claim renewal failed; retrying in 60 s",
            "crates/keeper/src/ipc.rs:42",
            "keeper::panic: keeper panicked at line 7",
            "keeper::token::refresh: the provider said no",
            "error: expected 3 items, got 2",
            "see https://forge.example.com and https://us.i.posthog.com/",
            "the keyword: none",
        ] {
            check(text, text);
        }
    }

    #[test]
    fn matrix_ids_with_and_without_a_server_are_redacted() {
        check(
            "invite @alice:example.org failed",
            "invite <matrix-id> failed",
        );
        check("room !OGEhHVWSdvArJzumhm:matrix.org", "room <matrix-id>");
        check("alias #keeper:matrix.org gone", "alias <matrix-id> gone");
        check(
            "event $abc:localhost:8448 missing",
            "event <matrix-id> missing",
        );
        check("(@bob:server.example)", "(<matrix-id>)");
        // Room versions with hashed ids carry no server, in any alphabet.
        check(
            "event $abcdefghijklmnopqrstuvwxyzabcdefghijklmnopq missing",
            "event <matrix-id> missing",
        );
        check("v12 room !AbCdEfGhIjKlMnOpQrStUvWx", "v12 room <matrix-id>");
        check(
            r#"event_id="$Rqnc-F-dvnEYJTyHq_iKxU2bZ1CI92-kuZq3a5lr5Zg""#,
            r#"event_id="<matrix-id>""#,
        );
    }

    #[test]
    fn email_addresses_are_redacted() {
        check("mail bob.smith+x@mail.example.com now", "mail <email> now");
        check("<marta@tgorka.dev>", "<<email>>");
    }

    #[test]
    fn a_urls_path_query_and_user_information_go_but_its_host_stays() {
        check(
            "GET https://matrix.example.org/_matrix/client/v3/rooms?filter=x failed",
            "GET https://matrix.example.org/<redacted> failed",
        );
        check(
            "fetch https://host.example?q=private",
            "fetch https://host.example/<redacted>",
        );
        check(
            "clone https://user:pw@git.example.com/a/b.git",
            "clone https://<redacted>/<redacted>",
        );
        check("mxc://server.org/AbCd", "mxc://server.org/<redacted>");
    }

    /// R515: from a home root to the end of the text, whatever quotes the
    /// path — no quote, space or backslash after the root is a certain end.
    #[test]
    fn a_home_path_takes_the_rest_of_the_text_in_every_quoting() {
        // Unquoted, as `%path.display()` writes it.
        check(
            "open /Users/marta/My Drive/private note.md failed",
            "open <path>",
        );
        check("read ~/notes/plan.md: denied", "read <path>");
        check("path=/home/dev/x.txt", "path=<path>");
        check(r"C:\Users\marta\file.txt", "<path>");
        check("C:/Users/marta/file.txt", "<path>");
        check(
            "/var/mobile/Containers/Data/Application/X/Documents/a b",
            "<path>",
        );
        // Quoted, in every escaping: the closing quote and all after it go.
        check(
            r#"Error { path: "/Users/marta/My Drive/Marta's plan.md", kind: NotFound }"#,
            r#"Error { path: "<path>"#,
        );
        check(
            "path='/Users/alice/My Drive/private note.md' then",
            "path='<path>",
        );
        check(
            r#"path="C:\\Users\\alice\\private.txt" kind=x"#,
            r#"path="<path>"#,
        );
        check(
            r#"error="failed: \"/Users/a/My Drive/x y.md\" was gone""#,
            r#"error="failed: \"<path>"#,
        );
        check(
            r#"path="/Users/alice/a\" private note.md" kind=x"#,
            r#"path="<path>"#,
        );
        check(
            r#"path="/Users/alice/it\'s private.md" kind=x"#,
            r#"path="<path>"#,
        );
        check(r#"dir "C:\\Users\\alice\\" gone"#, r#"dir "<path>"#);
        // A filename holding a quote and ending in a literal backslash, as
        // a hand-written `'{}'` / `"{}"` of its `display()` gives it.
        check(
            r"copy '/Users/alice/a' private note\' to /tmp/public",
            "copy '<path>",
        );
        check(
            r#"copy "/Users/alice/a" private note\" to /tmp/public"#,
            r#"copy "<path>"#,
        );
        // Two paths in one message: the first root takes both.
        check(
            r#"copy '/tmp/source' to "/Users/alice/a" private note\""#,
            r#"copy '/tmp/source' to "<path>"#,
        );
        check(
            "move /Users/alice/one.md to ~/two private.md",
            "move <path>",
        );
        // A quoted secret after a path never comes back out.
        check(
            r#"open "/Users/alice/x.md" with "two private words""#,
            r#"open "<path>"#,
        );
    }

    /// R518: `Users` in any case is a home root, and so is a home root
    /// inside a `file://` URL; both take the rest of the text.
    #[test]
    fn a_home_root_in_any_case_or_inside_a_file_url_takes_the_rest() {
        check("mkdir /users/alice/a b.md failed", "mkdir <path>");
        check("mkdir /uSeRs/alice/a b.md failed", "mkdir <path>");
        check(r"mkdir C:\users\alice\a b.md failed", "mkdir <path>");
        check(r#"path="c:\\USERS\\alice\\a b.md" x"#, r#"path="<path>"#);
        check("mkdir C:/uSERS/alice/a b.md failed", "mkdir <path>");
        check(
            r#"open file:///Users/alice/a b.md then "private tail""#,
            "open file://<path>",
        );
        check(
            r#"open FILE:///users/alice/a b.md then "private tail""#,
            "open FILE://<path>",
        );
        check(
            "open file:///home/alice/a b.md then x",
            "open file://<path>",
        );
        check("open file:///C:/Users/alice/a b.md x", "open file://<path>");
        check(
            "open file:///c:/users/alice/a%20b.md x",
            "open file://<path>",
        );
        check(
            "open file://localhost/Users/alice/a b.md x",
            "open file://localhost<path>",
        );
    }

    #[test]
    fn long_hex_runs_are_redacted_and_short_ones_stay() {
        check("commit e19114c ok", "commit e19114c ok");
        check("sha256 9f5f8010aa66bb77cc88dd99 bad", "sha256 <hex> bad");
        check("id 0123456789abcdef!", "id <hex>!");
        check("id 0123456789abcde", "id 0123456789abcde");
    }

    /// R461: a run of 24 or more base64 characters, whatever it reads as.
    #[test]
    fn every_base64_run_of_twenty_four_or_more_is_redacted() {
        check(
            "nonce dGhpcyBpcyBhIHNlY3JldCB2YWx1ZQ== used",
            "nonce <base64> used",
        );
        check("dGhpcyBpcyBhIHNlY3JldCB2", "<base64>");
        check("abcdefghijklmnopqrstuvwxyzab", "<base64>");
        check("ABCDEFGHIJKLMNOPQRSTUVWXYZ", "<base64>");
        check("a_snake_case_identifier_that_is_long", "<base64>");
        check("dGhpcyBpcyBhIHNlY3JldC", "dGhpcyBpcyBhIHNlY3JldC");
    }

    #[test]
    fn tokens_are_redacted_by_prefix_and_after_bearer() {
        // Built from fragments so the repository's secret hook, which looks
        // for `syt_` and twenty more token characters, never sees one whole.
        let token = ["syt", "_YWxpY2U", "_abcdEFGH", "_0z1y2x"].concat();
        check(&format!("login {token} done"), "login <token> done");
        check("phc_publicTokenValue1", "<token>");
        check("bearer xyz", "bearer <token>");
    }

    /// A credential assignment fails closed: from the assignment to the end,
    /// in every quoting and escaping a message or `Debug` can produce.
    #[test]
    fn everything_after_a_credential_assignment_goes() {
        check("access_token=abc123&x=1", "access_token=<redacted>");
        check("api_key=hunter2 sent", "api_key=<redacted>");
        check("PASSWORD='two words' then more", "PASSWORD=<redacted>");
        check(
            r#"Login { password: "hunter2", user: "m" }"#,
            r#"Login { password:<redacted>"#,
        );
        check(
            r#"error="password=\"two private words\"""#,
            r#"error="password=<redacted>"#,
        );
        check(
            r#"body {\"secret\": \"two private words\"}"#,
            r#"body {\"secret\":<redacted>"#,
        );
        check(
            "Authorization: Bearer abc.def.ghi",
            "Authorization:<redacted>",
        );
        check("secret=s3cr3t", "secret=<redacted>");
    }

    #[test]
    fn a_field_is_redacted_from_its_raw_value() {
        assert_eq!(redact_field("password", "anything at all"), "<redacted>");
        assert_eq!(redact_field("api_key", "x"), "<redacted>");
        assert_eq!(
            redact_field("error", r#"login failed: password="two private words""#),
            "<redacted>"
        );
        assert_eq!(
            redact_field("root", "/Users/alice/My Drive/private note.md"),
            "<path>"
        );
        assert_eq!(redact_field("count", "3"), "3");
    }

    #[test]
    fn a_bound_never_leaves_the_token_it_cut() {
        assert_eq!(bound("short", 10), "short");
        let text = format!("{}0123456789abcdef0123456789abcdef", "x ".repeat(95));
        let cut = bound(&text, 200);
        assert!(!cut.contains("0123456789"), "{cut}");
        assert_eq!(redact(&cut), cut);
        let cut = bound("see @alice:example.org", 14);
        assert_eq!(cut, "see …");
    }

    #[test]
    fn a_long_message_is_capped_at_a_thousand_characters() {
        let long = "word ".repeat(400);
        let capped = redact(&long);
        assert_eq!(capped.chars().count(), MESSAGE_CAP);
        assert!(capped.ends_with('…'));
        let exact = "x ".repeat(MESSAGE_CAP / 2);
        assert_eq!(redact(&exact), exact);
    }

    #[test]
    fn overlapping_classes_never_leave_a_flagged_byte() {
        let text = "https://h.example/p?access_token=syt_abc_def&u=@a:b.org /Users/m/f";
        let out = redact(text);
        for secret in ["syt_", "@a:b.org", "/Users/m", "access_token", "/p?"] {
            assert!(!out.contains(secret), "{secret} leaked in {out}");
        }
        assert_eq!(out, "https://h.example/<redacted>");
    }
}
