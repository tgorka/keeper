//! `git-lfs-authenticate` end to end, against a stand-in `ssh` (Story 34.17).
//!
//! The unit tests beside `keeper_sync::lfs::ssh` cover parsing in isolation.
//! This file covers the thing they cannot: that the process keeper actually
//! spawns receives the **exact argv** git-lfs sends, and that each of the
//! answers a real server can give is read correctly.
//!
//! That matters more than it looks. Every mistake in this protocol comes back as
//! a server saying "Invalid repository path", or as a bare `401` with no hint
//! which half of the command was wrong — so a wire format verified by reading
//! the code and never by observing it is a format nobody has checked.
//!
//! One test, deliberately: the answers are stages of one story about one remote,
//! and the stand-in is rewritten between them.
#![cfg(unix)]

use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

use keeper_sync::error::Retriability;
use keeper_sync::lfs::ssh::{authenticate_with, Answer, Operation, SshRemote};
use keeper_sync::SyncError;

/// Write an executable stand-in for `ssh` that records its argv and then behaves
/// as `body` says.
///
/// `body` is shell appended after the recording, so a case can print a response,
/// exit non-zero, or both.
fn fake_ssh(dir: &Path, body: &str) -> PathBuf {
    let log = dir.join("argv.txt");
    let script = format!(
        "#!/bin/sh\n\
         # One argument per line, so an argument CONTAINING spaces stays\n\
         # distinguishable from several arguments — which is the entire point of\n\
         # the last one.\n\
         : > '{log}'\n\
         for arg in \"$@\"; do printf '%s\\n' \"$arg\" >> '{log}'; done\n\
         {body}\n",
        log = log.display()
    );
    let path = dir.join("ssh");
    std::fs::write(&path, script).expect("write the ssh stand-in");
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755))
        .expect("make it executable");
    path
}

fn recorded(dir: &Path) -> Vec<String> {
    std::fs::read_to_string(dir.join("argv.txt"))
        .expect("the stand-in ran")
        .lines()
        .map(str::to_owned)
        .collect()
}

#[tokio::test]
async fn the_handshake_sends_the_argv_git_lfs_sends_and_reads_every_answer() {
    let dir = tempfile::tempdir().expect("tempdir");
    let remote = SshRemote::parse("ssh://git@forge.example:2222/owner/repo.git")
        .expect("an ssh remote with a port");

    // ---------------------------------------------------------------------
    // 1. A forge that mints a credential — byte-for-byte Forgejo's
    //    `LFSTokenResponse`: two fields, and no expiry at all.
    // ---------------------------------------------------------------------
    let ssh = fake_ssh(
        dir.path(),
        r#"printf '%s' '{"header":{"Authorization":"Bearer eyJhbGciOiJIUzI1NiJ9.body.sig"},"href":"https://forge.example/owner/repo.git/info/lfs"}'"#,
    );

    let answer = authenticate_with(&remote, Operation::Upload, &ssh)
        .await
        .expect("the forge answered");

    assert_eq!(
        recorded(dir.path()),
        vec![
            // Non-interactivity first. git-lfs sets NONE of these, which is why
            // stock git-lfs can block a daemon forever on a passphrase prompt.
            "-o",
            "BatchMode=yes",
            "-o",
            "NumberOfPasswordPrompts=0",
            "-o",
            "ConnectTimeout=10",
            // The port, BEFORE the host, as ssh requires.
            "-p",
            "2222",
            "git@forge.example",
            // ONE argument. Were this split into three, this assertion would
            // show two more lines — while the server, which shell-splits the
            // command itself, would be none the wiser. That is exactly why it is
            // asserted here rather than trusted.
            "git-lfs-authenticate /owner/repo.git upload",
        ],
        "the wire form is git-lfs's own; a difference here is a server answering \
         \"Invalid repository path\" with no clue which half was wrong"
    );

    let Answer::Granted(credential) = answer else {
        panic!("a forge that answers is a granted credential");
    };
    assert_eq!(
        credential.authorization.as_deref(),
        Some("Bearer eyJhbGciOiJIUzI1NiJ9.body.sig"),
        "the Bearer JWT is the whole reason this module exists: it is the one \
         scheme keeper cannot mint for itself"
    );
    assert_eq!(
        credential.href.as_ref().map(url::Url::as_str),
        Some("https://forge.example/owner/repo.git/info/lfs"),
        "verbatim — the server already put `/info/lfs` in it"
    );
    assert_eq!(
        credential.expires_in_secs, None,
        "Forgejo and Gitea send no expiry and expire the JWT in 24h anyway, which \
         is why keeper imposes its own TTL rather than trusting silence"
    );

    // The operation is an operand, so a download asks for a read token.
    let ssh = fake_ssh(
        dir.path(),
        r#"printf '%s' '{"header":{"Authorization":"Bearer read-only"}}'"#,
    );
    authenticate_with(&remote, Operation::Download, &ssh)
        .await
        .expect("a download credential");
    assert_eq!(
        recorded(dir.path()).last().map(String::as_str),
        Some("git-lfs-authenticate /owner/repo.git download")
    );

    // A scp-style remote drops the leading slash, and only there. Same repo, and
    // both forges trim it anyway — but this is the form git-lfs sends.
    let scp = SshRemote::parse("git@forge.example:owner/repo.git").expect("scp-style");
    authenticate_with(&scp, Operation::Upload, &ssh)
        .await
        .expect("answered");
    assert_eq!(
        recorded(dir.path()),
        vec![
            "-o",
            "BatchMode=yes",
            "-o",
            "NumberOfPasswordPrompts=0",
            "-o",
            "ConnectTimeout=10",
            // No port: scp-style syntax cannot carry one.
            "git@forge.example",
            "git-lfs-authenticate owner/repo.git upload",
        ]
    );

    // A host that looks like an ssh option is separated from the options with
    // `--`. This is the one assertion here with a security consequence: without
    // the separator ssh reads `-oProxyCommand=…` as an OPTION, and
    // `ProxyCommand` runs a shell command — so a crafted remote URL would be
    // arbitrary code execution the moment a large file was staged. git-lfs
    // guards this the same way, and a guard asserted only by reading it is not
    // a guard.
    let hostile = SshRemote::parse("ssh://-oProxyCommand=evil@forge.example/owner/repo.git")
        .expect("it parses; `--` is the defence, not refusal");
    authenticate_with(&hostile, Operation::Upload, &ssh)
        .await
        .expect("answered");
    let argv = recorded(dir.path());
    let separator = argv
        .iter()
        .position(|arg| arg == "--")
        .expect("`--` must be emitted for a host beginning with `-`");
    let host = argv
        .iter()
        .position(|arg| arg == "-oProxyCommand=evil@forge.example")
        .expect("the host is passed through, userinfo and all");
    assert!(
        separator < host,
        "the separator must PRECEDE the host or it defends nothing, got: {argv:?}"
    );
    assert_eq!(
        argv.last().map(String::as_str),
        Some("git-lfs-authenticate /owner/repo.git upload"),
        "and the remote command still arrives intact"
    );

    // ---------------------------------------------------------------------
    // 2. A plain bare repository over ssh: a login shell runs the command and
    //    reports exit 127. There is no LFS server here at all, so this is a
    //    fallback rather than a failure — the derived https endpoint and
    //    whatever token is stored still get their chance.
    // ---------------------------------------------------------------------
    let ssh = fake_ssh(
        dir.path(),
        "echo 'bash: git-lfs-authenticate: command not found' >&2\nexit 127",
    );
    assert_eq!(
        authenticate_with(&remote, Operation::Upload, &ssh)
            .await
            .expect("a missing command is not an error"),
        Answer::NoSshLfs
    );

    // ---------------------------------------------------------------------
    // 3. A forge with LFS switched off. Exit 1, and prose matching no not-found
    //    pattern — so this must NOT be read as "no LFS here" and quietly
    //    downgraded to an unauthenticated https attempt that fails less legibly.
    //    The server's own words are the only diagnostic it gives, so they are
    //    carried through to the message a human reads on the file's own row.
    // ---------------------------------------------------------------------
    let ssh = fake_ssh(
        dir.path(),
        "echo 'Forgejo: Unknown git command' >&2\nexit 1",
    );
    let err = authenticate_with(&remote, Operation::Upload, &ssh)
        .await
        .expect_err("a refusal is an error");
    let message = err.to_string();
    assert!(
        message.contains("Forgejo: Unknown git command"),
        "the server's own words are the only clue that LFS is disabled, got: {message}"
    );
    assert!(
        message.contains("git@forge.example"),
        "and it names which remote refused, got: {message}"
    );

    // ---------------------------------------------------------------------
    // 4. A login banner. git-lfs retries this six times and then reports a JSON
    //    parse error; the banner is still there on the sixth attempt, so it is
    //    reported once with the banner included — because the banner IS the fix.
    //    Note where the banner is: STDOUT, ahead of the body. Quoting stderr
    //    alone left the operator with "the answer was not the expected JSON" and
    //    no sight of the one thing they can act on.
    // ---------------------------------------------------------------------
    let ssh = fake_ssh(
        dir.path(),
        r#"printf '%s' 'Welcome to forge.example
{"href":"https://forge.example/x"}'"#,
    );
    let err = authenticate_with(&remote, Operation::Upload, &ssh)
        .await
        .expect_err("a banner ahead of the JSON is fatal");
    assert!(err.to_string().contains("/owner/repo.git"), "got: {err}");
    assert!(
        err.to_string().contains("Welcome to forge.example"),
        "the banner itself is the fix, so it has to reach the message, got: {err}"
    );

    // ---------------------------------------------------------------------
    // 5. ssh's OWN failures. `ssh(1)` exits 255 for a closed lid, a VPN drop, a
    //    DNS failure or the `ConnectTimeout` this module sets — the server was
    //    never reached, so nothing has been learned about it. Classified
    //    `Permanent` these park the LfsUpload unit on the FIRST failure, and a
    //    parked unit still counts toward `outstanding_count`, so the push stays
    //    held by `LfsUploadPending` until somebody runs `db::unpark`. A lid must
    //    not stop publishing.
    // ---------------------------------------------------------------------
    for stderr in [
        "ssh: connect to host forge.example port 2222: Connection refused",
        "ssh: connect to host forge.example port 2222: Connection timed out",
        "ssh: connect to host forge.example port 2222: Network is unreachable",
        "ssh: Could not resolve hostname forge.example: Name or service not known",
        "kex_exchange_identification: Connection closed by remote host",
        "ssh_exchange_identification: read: Connection reset by peer",
    ] {
        let ssh = fake_ssh(dir.path(), &format!("echo '{stderr}' >&2\nexit 255"));
        let err = authenticate_with(&remote, Operation::Upload, &ssh)
            .await
            .expect_err("ssh could not reach the server");
        assert!(
            matches!(err, SyncError::Network { .. }),
            "ssh's own failure is transient — a park here needs a human to undo \
             it: {stderr} gave {err:?}"
        );
        assert_eq!(
            err.retriability(),
            Retriability::Transient,
            "which is the consequence that matters: the unit retries after \
             backoff rather than parking. {stderr}"
        );
        assert!(
            err.to_string().contains("git@forge.example"),
            "and it names the remote it could not reach, got: {err}"
        );
    }

    // ---------------------------------------------------------------------
    // 6. The other side of exit 255, where a permanent park is exactly right:
    //    no amount of backoff makes an unauthorized key authorized, and retrying
    //    forever would bury the one message that says what to change.
    // ---------------------------------------------------------------------
    for stderr in [
        "git@forge.example: Permission denied (publickey).",
        "Host key verification failed.",
    ] {
        let ssh = fake_ssh(dir.path(), &format!("echo '{stderr}' >&2\nexit 255"));
        let err = authenticate_with(&remote, Operation::Upload, &ssh)
            .await
            .expect_err("the server refused this key");
        assert!(
            matches!(err, SyncError::Config(_)),
            "a refused key is not a network failure: {stderr} gave {err:?}"
        );
        assert_eq!(err.retriability(), Retriability::Permanent, "{stderr}");
        assert!(err.to_string().contains(stderr), "got: {err}");
    }

    // ---------------------------------------------------------------------
    // 7. A `ForceCommand`/`git-shell` wrapper that answers on STDOUT. Only
    //    stderr used to be consulted, so this became "It said: nothing" — and
    //    when what it says is that the command is missing, a park instead of the
    //    https fallback the answer is supposed to be.
    // ---------------------------------------------------------------------
    let ssh = fake_ssh(
        dir.path(),
        "echo 'git-shell: git-lfs-authenticate: command not found'\nexit 1",
    );
    assert_eq!(
        authenticate_with(&remote, Operation::Upload, &ssh)
            .await
            .expect("a missing command is a fallback, whichever stream says so"),
        Answer::NoSshLfs
    );

    let ssh = fake_ssh(
        dir.path(),
        "echo 'sorry, this account is restricted to git commands'\nexit 1",
    );
    let err = authenticate_with(&remote, Operation::Upload, &ssh)
        .await
        .expect_err("a refusal is an error");
    assert!(
        err.to_string().contains("restricted to git commands"),
        "a wrapper's refusal on stdout is the only diagnostic there is, got: {err}"
    );
}
