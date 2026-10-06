//! A `run`'s macOS sandbox against real files (96.1 #5, #6; D-33): cases
//! (a)–(h) of the Linux tests through `/usr/bin/sandbox-exec` and the
//! profile `keeper_core::agents::run::sbpl` writes; S-07's two — no
//! process's information (`process-info*`) and no keychain service
//! (`mach-lookup` of `com.apple.SecurityServer`) — each beside a control run
//! outside the sandbox that must succeed; no local Unix socket with or
//! without network (R147, R213); and the timeout with a descendant that
//! left the process group (R213). Runs in CI's macOS job and on hesperia;
//! the Linux dev host never builds it.
#![cfg(target_os = "macos")]

use std::net::{TcpListener, UdpSocket};
use std::os::unix::net::UnixListener;
use std::path::{Path, PathBuf};
use std::time::Duration;

use keeper_agent::run::{Forbidden, Kind, Ran, SandboxHost, Session};
use keeper_core::agents::run::{Exit, Reaped, SandboxTable};
use serde_json::{json, Value};

struct Tree {
    _root: tempfile::TempDir,
    drive: PathBuf,
    workspace: PathBuf,
    home: PathBuf,
    secrets: PathBuf,
    /// A folder the host's `read_exec` grants: a socket there is reachable
    /// by its path, so only the network rule can refuse it.
    sockets: PathBuf,
}

const SESSION: &str = "active/2026-10-06-run";

fn write(path: &Path, text: &str) {
    std::fs::create_dir_all(path.parent().expect("parent")).expect("dirs");
    std::fs::write(path, text).expect("write");
}

fn tree() -> Tree {
    let root = tempfile::tempdir().expect("tempdir");
    let base = root.path().canonicalize().expect("canonical");
    let drive = base.join("My \"Drive\"");
    write(&drive.join("10-notes/x.md"), "a note\n");
    write(&drive.join(".git/config"), "[core]\n");
    write(&drive.join("60-sessions/.keeper/agents.db"), "index");
    let zone = drive.join("60-sessions");
    let workspace = zone.join(SESSION).join("workspace");
    write(&workspace.join("a.txt"), "workspace bytes\n");
    let home = base.join("home");
    write(&home.join(".ssh/id_ed25519"), "PRIVATE KEY");
    let secrets = base.join("secrets");
    write(&secrets.join("x"), "a secret");
    let sockets = base.join("sockets");
    std::fs::create_dir_all(&sockets).expect("sockets");
    Tree {
        _root: root,
        drive,
        workspace,
        home,
        secrets,
        sockets,
    }
}

/// The host as `desktop_sandbox` makes it: the developer folder
/// `xcode-select -p` names read-and-execute (`/usr/bin/python3` and
/// `/usr/bin/git` are shims into it), and the test's own `read_exec` folder.
fn host(tree: &Tree) -> SandboxHost {
    let developer = std::process::Command::new("/usr/bin/xcode-select")
        .arg("-p")
        .output()
        .expect("xcode-select");
    let developer = PathBuf::from(String::from_utf8_lossy(&developer.stdout).trim());
    SandboxHost::probe(
        Kind::SandboxExec,
        "hesperia",
        &SandboxTable {
            read_exec: vec![tree.sockets.clone(), developer],
            env: Vec::new(),
        },
        &Forbidden {
            drives: vec![("tgdrive".to_owned(), tree.drive.clone())],
            secrets: vec![tree.secrets.clone()],
            home: Some(tree.home.clone()),
        },
    )
    .expect("sandbox-exec runs /usr/bin/true")
}

fn run(host: &SandboxHost, tree: &Tree, args: Value) -> Ran {
    let prepared = host
        .prepare(
            &args,
            Session {
                drive: &tree.drive,
                zone: "60-sessions",
                path: SESSION,
            },
            &[("tgdrive".to_owned(), tree.drive.clone())],
        )
        .expect("prepared");
    host.execute(&prepared).expect("ran")
}

fn cat(host: &SandboxHost, tree: &Tree, path: &Path, read: bool, network: bool) -> Ran {
    let read: Vec<&str> = if read { vec!["tgdrive"] } else { Vec::new() };
    run(
        host,
        tree,
        json!({"argv": ["/bin/cat", path.display().to_string()], "read": read, "network": network}),
    )
}

#[test]
fn the_sandbox_holds_files_to_the_plan() {
    let tree = tree();
    let host = host(&tree);
    let ran = run(&host, &tree, json!({"argv": ["/bin/cat", "a.txt"]}));
    assert_eq!(
        (ran.exit, ran.stdout.text().as_str()),
        (Exit::Code(0), "workspace bytes\n")
    );
    let note = tree.drive.join("10-notes/x.md");
    let ran = cat(&host, &tree, &note, true, false);
    assert_eq!(
        (ran.exit, ran.stdout.text().as_str()),
        (Exit::Code(0), "a note\n")
    );
    assert_ne!(cat(&host, &tree, &note, false, true).exit, Exit::Code(0));
    let ran = run(
        &host,
        &tree,
        json!({"argv": ["/bin/cp", "a.txt", note.display().to_string()], "read": ["tgdrive"]}),
    );
    assert_ne!(ran.exit, Exit::Code(0));
    assert_eq!(std::fs::read_to_string(&note).expect("note"), "a note\n");
    for hidden in [".git/config", "60-sessions/.keeper/agents.db"] {
        assert_ne!(
            cat(&host, &tree, &tree.drive.join(hidden), true, false).exit,
            Exit::Code(0),
            "{hidden}"
        );
        assert_ne!(
            cat(&host, &tree, &tree.drive.join(hidden), false, true).exit,
            Exit::Code(0),
            "{hidden}"
        );
    }
    assert_ne!(
        cat(
            &host,
            &tree,
            &tree.home.join(".ssh/id_ed25519"),
            true,
            false
        )
        .exit,
        Exit::Code(0)
    );
    assert_ne!(
        cat(&host, &tree, &tree.secrets.join("x"), true, false).exit,
        Exit::Code(0)
    );
}

/// TCP and UDP follow the plan; a pathname Unix socket is unreachable with
/// network and without (R147, R213), its control outside connecting.
#[test]
fn the_network_follows_the_plan() {
    let tree = tree();
    let host = host(&tree);
    let tcp = TcpListener::bind("127.0.0.1:0").expect("tcp");
    let port = tcp.local_addr().expect("addr").port().to_string();
    let udp = UdpSocket::bind("127.0.0.1:0").expect("udp");
    udp.set_read_timeout(Some(Duration::from_millis(500)))
        .expect("timeout");
    let udp_port = udp.local_addr().expect("addr").port().to_string();
    let socket = tree.sockets.join("agent.sock");
    let _unix = UnixListener::bind(&socket).expect("unix");
    write(&tree.workspace.join("unix.py"), CONNECT);
    let path = socket.display().to_string();
    let unix = |network: bool| {
        run(
            &host,
            &tree,
            json!({"argv": ["/usr/bin/python3", "unix.py", path], "network": network}),
        )
    };
    let control = std::process::Command::new("/usr/bin/python3")
        .arg(tree.workspace.join("unix.py"))
        .arg(&socket)
        .output()
        .expect("python3");
    assert!(
        control.status.success(),
        "the control could not reach the socket: {control:?}"
    );
    let probe = |network: bool| {
        let tcp = run(
            &host,
            &tree,
            json!({"argv": ["/usr/bin/nc", "-z", "-G", "1", "127.0.0.1", port], "network": network}),
        );
        run(
            &host,
            &tree,
            json!({"argv": ["/usr/bin/nc", "-u", "-z", "-w", "1", "127.0.0.1", udp_port], "network": network}),
        );
        let mut buf = [0u8; 64];
        (tcp.exit == Exit::Code(0), udp.recv(&mut buf).is_ok())
    };
    assert_eq!(
        probe(false),
        (false, false),
        "a run without network reached it"
    );
    let ran = unix(false);
    assert_eq!(
        ran.exit,
        Exit::Code(3),
        "a run without network reached a Unix socket: {ran:?}"
    );
    assert_eq!(probe(true), (true, true), "a networked run did not");
    let ran = unix(true);
    assert_eq!(
        ran.exit,
        Exit::Code(3),
        "a networked run reached a Unix socket: {ran:?}"
    );
}

/// What the Unix-socket helper does: connect to the path it is given; exit
/// 0 when it did, 3 when the connection was refused by anything — not
/// Python's own 1, so a helper that never ran is not taken for a refusal.
const CONNECT: &str = r#"
import socket, sys
try:
    socket.socket(socket.AF_UNIX, socket.SOCK_STREAM).connect(sys.argv[1])
except OSError as e:
    print(e.errno)
    sys.exit(3)
"#;

/// What the Mach helper prints: `bootstrap_look_up`'s result for the
/// keychain's service (0 when found).
const LOOKUP: &str = r#"
import ctypes
lib = ctypes.CDLL("/usr/lib/libSystem.B.dylib")
port = ctypes.c_uint(0)
bootstrap = ctypes.c_uint.in_dll(lib, "bootstrap_port")
print(lib.bootstrap_look_up(bootstrap, b"com.apple.SecurityServer", ctypes.byref(port)))
"#;

/// S-07: `ps -E` of a process of this user started with a planted secret
/// in its environment prints it outside and never inside;
/// `bootstrap_look_up` of the keychain's service succeeds outside and fails
/// inside — each control first, so the profile is the refuser.
#[test]
fn no_process_information_and_no_keychain() {
    let tree = tree();
    let host = host(&tree);
    let mut planted = std::process::Command::new("/bin/sleep")
        .arg("30")
        .env("KEEPER_AGENTD_SECRET_X", "planted-secret")
        .spawn()
        .expect("sleep");
    let pid = planted.id().to_string();
    let ps = ["/bin/ps", "-E", "-ww", "-o", "command=", "-p", &pid];
    let control = std::process::Command::new(ps[0])
        .args(&ps[1..])
        .output()
        .expect("ps");
    assert!(
        String::from_utf8_lossy(&control.stdout).contains("planted-secret"),
        "the control's ps does not show the process's environment: {control:?}"
    );
    let ran = run(&host, &tree, json!({ "argv": ps }));
    let _ = planted.kill();
    let _ = planted.wait();
    assert!(!ran.stdout.text().contains("planted-secret"), "{ran:?}");
    write(&tree.workspace.join("lookup.py"), LOOKUP);
    let control = std::process::Command::new("/usr/bin/python3")
        .arg(tree.workspace.join("lookup.py"))
        .output()
        .expect("python3");
    assert_eq!(
        String::from_utf8_lossy(&control.stdout).trim(),
        "0",
        "the control could not look the keychain up"
    );
    let ran = run(
        &host,
        &tree,
        json!({"argv": ["/usr/bin/python3", "lookup.py"]}),
    );
    assert_eq!(ran.exit, Exit::Code(0), "{ran:?}");
    assert_ne!(ran.stdout.text().trim(), "0", "{ran:?}");
}

/// 96.1 #6, R213, R231: a fixture that forks a grandchild which leaves the
/// process group with `setsid` and keeps the pipes open, run with
/// `timeout_s = 1`: the result says `timed out after 1 s`, comes back
/// within the drain's bound, says it swept what left the group — the one
/// grandchild, never "every process" — and the grandchild is gone a second
/// later.
#[test]
fn run_timeout_kills_the_process_group() {
    let tree = tree();
    let host = host(&tree);
    write(
        &tree.workspace.join("fork.py"),
        "import os, sys, time\nif os.fork() == 0:\n    os.setsid()\n    print(os.getpid(), flush=True)\n    time.sleep(60)\n    sys.exit(0)\ntime.sleep(60)\n",
    );
    let started = std::time::Instant::now();
    let ran = run(
        &host,
        &tree,
        json!({"argv": ["/usr/bin/python3", "fork.py"], "timeout_s": 1}),
    );
    assert!(
        started.elapsed() < Duration::from_secs(10),
        "{:?}",
        started.elapsed()
    );
    assert_eq!(ran.exit, Exit::TimedOut(1));
    assert_eq!(ran.reaped, Reaped::Swept { found: 1 });
    assert!(
        ran.render().starts_with("timed out after 1 s"),
        "{}",
        ran.render()
    );
    let pid: i32 = ran.stdout.text().trim().parse().expect("the grandchild");
    std::thread::sleep(Duration::from_secs(1));
    let pid = rustix::process::Pid::from_raw(pid).expect("a pid");
    assert!(
        rustix::process::test_kill_process(pid).is_err(),
        "the grandchild that left the group is alive"
    );
}
