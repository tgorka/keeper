//! A `run`'s Linux sandbox against real files and this host's real kernel
//! (96.1 #4, #6–#8; D-33, R141, R147, R213), through the binary production
//! uses: keeper-agentd re-executing itself as the trampoline.
//!
//! The kernel decides: a host whose landlock is below ABI 6 fails these
//! tests at the probe — they never skip. Every refusal that is keeper's
//! filter's is tried outside the sandbox first, as a control that must not
//! fail with `EPERM`, so the `EPERM` inside is keeper's filter's and not the
//! host's (Docker's seccomp profile, Yama); the process calls target a
//! child of the helper's own, in its own landlock domain, so landlock does
//! not refuse them either. A call this host already refuses fails its test
//! loudly: those calls are in
//! `the_calls_this_container_refuses_are_refused_by_keepers_filter`, run
//! with `--ignored` on electra's host OS. The generated filter itself is
//! checked call by call in `keeper-agent`'s `run::linux` unit test.
#![cfg(target_os = "linux")]

use std::net::{TcpListener, UdpSocket};
use std::os::unix::net::UnixListener;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use keeper_agent::run::{Forbidden, Kind, Ran, SandboxHost, Session, TRAMPOLINE_ARG};
use keeper_core::agents::run::{Exit, Reaped, SandboxTable, STREAM_CAP};
use serde_json::{json, Value};

/// Runs start one at a time: the descriptor test makes a descriptor
/// inheritable for a moment, and no other run may start then.
static STARTS: Mutex<()> = Mutex::new(());

const SESSION: &str = "active/2026-10-06-run";

/// A drive, a session inside it, a fake home, a fake secrets store and a
/// folder `[sandbox] read_exec` grants.
struct Tree {
    _root: tempfile::TempDir,
    base: PathBuf,
    drive: PathBuf,
    workspace: PathBuf,
    home: PathBuf,
    secrets: PathBuf,
    credentials: PathBuf,
    tools: PathBuf,
}

fn write(path: &Path, text: &str) {
    std::fs::create_dir_all(path.parent().expect("parent")).expect("dirs");
    std::fs::write(path, text).expect("write");
}

fn tree() -> Tree {
    let root = tempfile::tempdir().expect("tempdir");
    let base = root.path().canonicalize().expect("canonical");
    let drive = base.join("tgdrive");
    write(&drive.join("10-notes/x.md"), "a note\n");
    write(&drive.join(".git/config"), "[core]\n");
    write(&drive.join(".keeper/agents.db"), "index");
    write(&drive.join("60-sessions/.keeper/agents.db"), "index");
    let zone = drive.join("60-sessions");
    let workspace = zone.join(SESSION).join("workspace");
    write(&workspace.join("a.txt"), "workspace bytes\n");
    write(&workspace.join("probe.py"), PROBE);
    let home = base.join("home");
    write(&home.join(".ssh/id_ed25519"), "PRIVATE KEY");
    let secrets = base.join("state/keeper-agentd/secrets");
    write(&secrets.join("x"), "a secret");
    let credentials = base.join("credentials");
    write(&credentials.join("x"), "a credential");
    let tools = base.join("tools/bin");
    std::fs::create_dir_all(&tools).expect("tools");
    Tree {
        _root: root,
        base,
        drive,
        workspace,
        home,
        secrets,
        credentials,
        tools,
    }
}

fn forbidden(tree: &Tree) -> Forbidden {
    Forbidden {
        drives: vec![("tgdrive".to_owned(), tree.drive.clone())],
        secrets: vec![tree.secrets.clone(), tree.credentials.clone()],
        home: Some(tree.home.clone()),
    }
}

fn host_with(tree: &Tree, read_exec: Vec<PathBuf>) -> SandboxHost {
    let _start = STARTS.lock().unwrap_or_else(|p| p.into_inner());
    SandboxHost::probe(
        Kind::Trampoline {
            program: PathBuf::from(env!("CARGO_BIN_EXE_keeper-agentd")),
            args: vec![TRAMPOLINE_ARG.into()],
        },
        "electra",
        &SandboxTable {
            read_exec,
            env: Vec::new(),
        },
        &forbidden(tree),
    )
    .expect("this kernel enforces landlock ABI 6 and the seccomp filter")
}

fn host(tree: &Tree) -> SandboxHost {
    host_with(tree, Vec::new())
}

fn session(tree: &Tree) -> Session<'_> {
    Session {
        drive: &tree.drive,
        zone: "60-sessions",
        path: SESSION,
    }
}

fn try_run(host: &SandboxHost, tree: &Tree, args: Value) -> Result<Ran, String> {
    let prepared = host.prepare(
        &args,
        session(tree),
        &[("tgdrive".to_owned(), tree.drive.clone())],
    )?;
    let _start = STARTS.lock().unwrap_or_else(|p| p.into_inner());
    host.execute(&prepared)
}

fn run(host: &SandboxHost, tree: &Tree, args: Value) -> Ran {
    try_run(host, tree, args).expect("ran")
}

fn cat(host: &SandboxHost, tree: &Tree, path: &Path, read: bool) -> Ran {
    let read: Vec<&str> = if read { vec!["tgdrive"] } else { Vec::new() };
    run(
        host,
        tree,
        json!({"argv": ["cat", path.display().to_string()], "read": read}),
    )
}

fn denied(ran: &Ran, what: &str) {
    assert_ne!(ran.exit, Exit::Code(0), "{what} was read: {ran:?}");
    assert!(
        ran.stderr.text().contains("Permission denied"),
        "{what}: {}",
        ran.stderr.text()
    );
}

/// A real BusyBox with its `env`, `nice` and `timeout` applets:
/// `KEEPER_TEST_BUSYBOX`, else the host's. Ubuntu's own leaves `nice` out;
/// Alpine's `busybox-static` has them all.
fn test_busybox() -> PathBuf {
    std::env::var_os("KEEPER_TEST_BUSYBOX")
        .map(PathBuf::from)
        .or_else(|| {
            ["/usr/bin/busybox", "/bin/busybox"]
                .map(PathBuf::from)
                .into_iter()
                .find(|path| path.exists())
        })
        .expect("a BusyBox: set KEEPER_TEST_BUSYBOX to one with the env, nice and timeout applets")
}

#[test]
fn the_sandbox_holds_files_to_the_plan() {
    let tree = tree();
    let host = host(&tree);
    // (a)
    let ran = run(&host, &tree, json!({"argv": ["/bin/cat", "a.txt"]}));
    assert_eq!(ran.exit, Exit::Code(0), "{ran:?}");
    assert_eq!(ran.stdout.text(), "workspace bytes\n");
    // (b) a drive named in `read` is read without network; never with it,
    // and never when not named (R142).
    let note = tree.drive.join("10-notes/x.md");
    let ran = cat(&host, &tree, &note, true);
    assert_eq!(
        (ran.exit, ran.stdout.text().as_str()),
        (Exit::Code(0), "a note\n")
    );
    denied(&cat(&host, &tree, &note, false), "a drive not named");
    denied(
        &run(
            &host,
            &tree,
            json!({"argv": ["cat", note.display().to_string()], "network": true}),
        ),
        "a drive with network",
    );
    // (c) writing it fails, and its bytes are unchanged.
    let ran = run(
        &host,
        &tree,
        json!({"argv": ["cp", "a.txt", note.display().to_string()], "read": ["tgdrive"]}),
    );
    denied(&ran, "a drive write");
    assert_eq!(std::fs::read_to_string(&note).expect("note"), "a note\n");
    // (d), (h) `.git/` and every `.keeper/`, at any depth, with or without
    // network.
    for hidden in [
        ".git/config",
        ".keeper/agents.db",
        "60-sessions/.keeper/agents.db",
    ] {
        denied(&cat(&host, &tree, &tree.drive.join(hidden), true), hidden);
        denied(
            &run(
                &host,
                &tree,
                json!({"argv": ["cat", tree.drive.join(hidden).display().to_string()], "network": true}),
            ),
            hidden,
        );
    }
    // (e), (f)
    denied(
        &cat(&host, &tree, &tree.home.join(".ssh/id_ed25519"), true),
        "~/.ssh",
    );
    denied(
        &cat(&host, &tree, &tree.secrets.join("x"), true),
        "agentd's secrets",
    );
    denied(
        &cat(&host, &tree, &tree.credentials.join("x"), true),
        "$CREDENTIALS_DIRECTORY",
    );
}

/// R96R-02: every grant is checked as it resolves when the run starts — a
/// `read_exec` link the probe allowed, retargeted at the host's secrets, is
/// refused rather than granted.
#[test]
fn a_retargeted_read_exec_link_is_refused_at_the_run() {
    let tree = tree();
    let link = tree.base.join("toolchain");
    std::os::unix::fs::symlink(&tree.tools, &link).expect("link");
    let host = host_with(&tree, vec![link.clone()]);
    let ran = run(&host, &tree, json!({"argv": ["true"]}));
    assert_eq!(ran.exit, Exit::Code(0), "{ran:?}");
    std::fs::remove_file(&link).expect("rm");
    std::os::unix::fs::symlink(&tree.secrets, &link).expect("retarget");
    let refused = try_run(
        &host,
        &tree,
        json!({"argv": ["cat", link.join("x").display().to_string()]}),
    )
    .expect_err("refused");
    assert!(refused.contains("this host's secrets"), "{refused}");
}

/// (i) S-07: agentd's environment and memory are behind `/proc`, which
/// landlock grants nothing of.
#[test]
fn proc_of_the_parent_is_unreadable() {
    std::env::set_var("KEEPER_AGENTD_SECRET_X", "planted-secret");
    let tree = tree();
    let host = host(&tree);
    let me = std::process::id();
    for file in ["environ", "mem", "status"] {
        let ran = cat(
            &host,
            &tree,
            Path::new(&format!("/proc/{me}/{file}")),
            false,
        );
        assert_ne!(ran.exit, Exit::Code(0), "{file}");
        assert!(!ran.stdout.text().contains("planted-secret"), "{file}");
    }
}

/// The helper the sandboxed program runs: each call it is asked to make,
/// and the errno it got (0 when it went through). The process calls target
/// a child it forked itself, in its own landlock domain. Whatever a call
/// made — a System V message queue, semaphore set or memory segment, a
/// POSIX queue — is removed however the helper ends, each removed one
/// named on standard error (`made msg=<id>`), and its child is killed and
/// reaped (R231).
const PROBE: &str = r#"
import ctypes, os, socket, sys, time
libc = ctypes.CDLL(None, use_errno=True)
libc.syscall.restype = ctypes.c_long
def call(nr, *args):
    ctypes.set_errno(0)
    r = libc.syscall(ctypes.c_long(nr), *[ctypes.c_long(a) for a in args])
    return (0, r) if r != -1 else (ctypes.get_errno(), r)
def buf(text):
    return ctypes.addressof(ctypes.create_string_buffer(text))
def errno_of(work):
    try:
        work()
        return 0
    except OSError as e:
        return e.errno
def in_child(nr, *args):
    pid = os.fork()
    if pid == 0:
        os._exit(call(nr, *args)[0])
    return os.waitstatus_to_exitcode(os.waitpid(pid, 0)[1])
what, rest = sys.argv[1], sys.argv[2:]
out = {}
if what in ("calls", "container"):
    child = os.fork()
    if child == 0:
        time.sleep(30)
        os._exit(0)
    made = []
    def keep(kind, got, remove):
        if got[0] == 0:
            made.append((kind, got[1], remove))
        return got[0]
    try:
        time.sleep(0.1)
        if what == "calls":
            out["ptrace"] = call(101, 16, child, 0, 0)[0]
            if out["ptrace"] == 0:
                os.waitpid(child, 0)
                call(101, 17, child, 0, 0)
            mem = ctypes.create_string_buffer(8)
            iov = (ctypes.c_void_p * 2)(ctypes.addressof(mem), 8)
            out["process_vm_readv"] = call(310, child, ctypes.addressof(iov), 1, ctypes.addressof(iov), 1, 0)[0]
            out["process_vm_writev"] = call(311, child, ctypes.addressof(iov), 1, ctypes.addressof(iov), 1, 0)[0]
            attr = ctypes.create_string_buffer(128)
            ctypes.c_uint32.from_buffer(attr, 0).value = 1
            ctypes.c_uint32.from_buffer(attr, 4).value = 128
            out["perf_event_open"] = call(298, ctypes.addressof(attr), 0, -1, -1, 0)[0]
            out["x32_ptrace"] = call(0x40000000 | 521, 16, child, 0, 0)[0]
            # IPC_RMID (0) through msgctl (71), semctl (66), shmctl (31).
            out["msgget"] = keep("msg", call(68, 0, 0o1600), lambda q: call(71, q, 0, 0))
            out["semget"] = keep("sem", call(64, 0, 1, 0o1600), lambda q: call(66, q, 0, 0))
            out["shmget"] = keep("shm", call(29, 0, 4096, 0o1600), lambda q: call(31, q, 0, 0))
            out["mq_open"] = keep("mq", call(240, buf(b"keeper-probe"), os.O_CREAT | os.O_RDWR, 0o600, 0),
                                  lambda q: (os.close(q), call(241, buf(b"keeper-probe"))))
            out["bpf"] = call(321, 0, 0, 0)[0]
            out["personality"] = in_child(135, 0xffffffff)
            out["unshare"] = in_child(272, 0x10000000)
            out["setsid"] = in_child(112)
            out["setpgid"] = in_child(109, 0, 0)
        else:
            out["kcmp"] = call(312, os.getpid(), child, 0, 0, 0)[0]
            err, pidfd = call(434, child, 0)
            out["pidfd_getfd"] = err if err else call(438, pidfd, 0, 0)[0]
            params = ctypes.create_string_buffer(120)
            out["io_uring_setup"] = call(425, 1, ctypes.addressof(params))[0]
            out["io_uring_enter"] = call(426, 999, 0, 0, 0, 0, 0)[0]
            out["io_uring_register"] = call(427, 999, 0, 0, 0)[0]
            out["keyctl"] = call(250, 0, -3, 0, 0, 0)[0]
            out["add_key"] = call(248, buf(b"user"), buf(b"k"), buf(b"v"), 1, -3)[0]
            out["request_key"] = call(249, buf(b"user"), buf(b"k"), 0, -3)[0]
    finally:
        for kind, ident, remove in made:
            remove(ident)
            sys.stderr.write(f"made {kind}={ident}\n")
        os.kill(child, 9)
        os.waitpid(child, 0)
elif what == "fds":
    for n in rest:
        out["fd" + n] = errno_of(lambda: os.fstat(int(n)))
elif what == "meta":
    target = rest[0]
    out["chmod"] = errno_of(lambda: os.chmod(target, 0o600))
    out["utime"] = errno_of(lambda: os.utime(target, (0, 0)))
    out["chown"] = errno_of(lambda: os.chown(target, os.getuid(), os.getgid()))
    out["setxattr"] = errno_of(lambda: os.setxattr(target, b"user.keeper", b"x"))
    # The `*at` and descriptor forms, each its own call to the kernel.
    path = ctypes.create_string_buffer(target.encode())
    out["fchmodat"] = call(268, -100, ctypes.addressof(path), 0o600)[0]
    out["fchownat"] = call(260, -100, ctypes.addressof(path), os.getuid(), os.getgid(), 0)[0]
    fd = os.open(target, os.O_RDONLY)
    out["fchmod"] = errno_of(lambda: os.fchmod(fd, 0o600))
    out["fchown"] = errno_of(lambda: os.fchown(fd, os.getuid(), os.getgid()))
    os.close(fd)
elif what == "net":
    tcp, udp, unix = int(rest[0]), int(rest[1]), rest[2]
    def connect():
        s = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
        s.settimeout(2)
        s.connect(("127.0.0.1", tcp))
    out["tcp"] = errno_of(connect)
    def send():
        socket.socket(socket.AF_INET, socket.SOCK_DGRAM).sendto(b"udp-arrived", ("127.0.0.1", udp))
    out["udp"] = errno_of(send)
    def local():
        socket.socket(socket.AF_UNIX, socket.SOCK_STREAM).connect(unix)
    out["unix"] = errno_of(local)
    out["kill"] = errno_of(lambda: os.kill(os.getppid(), 0))
    def pair():
        a, b = socket.socketpair()
        a.send(b"x")
        assert b.recv(1) == b"x"
    out["socketpair"] = errno_of(pair)
print(" ".join(f"{k}={v}" for k, v in sorted(out.items())))
"#;

/// `name=errno` pairs.
fn errnos(text: &str) -> Vec<(String, i32)> {
    text.split_whitespace()
        .filter_map(|pair| pair.split_once('='))
        .map(|(name, errno)| (name.to_owned(), errno.parse().expect("an errno")))
        .collect()
}

const EPERM: i32 = 1;

/// The probe run directly by this process — the control. Each System V
/// object it made is gone after it (R231): none is left to fill the host's
/// limits for the next run.
fn control(tree: &Tree, args: &[&str]) -> Vec<(String, i32)> {
    let out = std::process::Command::new("python3")
        .arg(tree.workspace.join("probe.py"))
        .args(args)
        .output()
        .expect("python3");
    assert!(out.status.success(), "the control failed: {out:?}");
    for line in String::from_utf8_lossy(&out.stderr).lines() {
        let Some((kind, id)) = line
            .strip_prefix("made ")
            .and_then(|made| made.split_once('='))
        else {
            continue;
        };
        if kind == "mq" {
            continue;
        }
        let table = std::fs::read_to_string(format!("/proc/sysvipc/{kind}")).expect("sysvipc");
        assert!(
            !table
                .lines()
                .skip(1)
                .any(|row| row.split_whitespace().nth(1) == Some(id)),
            "the control left System V {kind} {id} behind"
        );
    }
    errnos(&String::from_utf8_lossy(&out.stdout))
}

fn sandboxed(host: &SandboxHost, tree: &Tree, args: &[&str], network: bool) -> Vec<(String, i32)> {
    let mut argv = vec!["python3", "probe.py"];
    argv.extend_from_slice(args);
    let ran = run(host, tree, json!({"argv": argv, "network": network}));
    assert_eq!(ran.exit, Exit::Code(0), "{ran:?}");
    errnos(&ran.stdout.text())
}

/// Each call reaches the kernel outside — its control fails with anything
/// but `EPERM`, so nothing on this host refuses it — and fails with `EPERM`
/// inside, with network and without: keeper's filter refuses it.
fn refused_by_keepers_filter(what: &str, expected: usize) {
    let tree = tree();
    let host = host(&tree);
    let controls = control(&tree, &[what]);
    assert_eq!(controls.len(), expected, "{controls:?}");
    let host_refuses: Vec<&String> = controls
        .iter()
        .filter(|(_, errno)| *errno == EPERM)
        .map(|(name, _)| name)
        .collect();
    assert!(
        host_refuses.is_empty(),
        "this host already refuses {host_refuses:?} ({controls:?}): run this test on a host OS"
    );
    for network in [false, true] {
        let inside = sandboxed(&host, &tree, &[what], network);
        assert_eq!(inside.len(), expected, "{inside:?}");
        for (name, errno) in &inside {
            assert_eq!(*errno, EPERM, "{name} (network {network}): {inside:?}");
        }
    }
}

/// (j) S-07, R147, R213: reaching into another process, an x32 call, the
/// System V IPC and message queues, `bpf`, `personality`, a namespace and
/// leaving the process group — each refused by keeper's filter, each
/// control going through on this host.
#[test]
fn the_kernel_calls_are_refused_by_keepers_filter() {
    refused_by_keepers_filter("calls", 14);
}

/// (j) R147, R213, the calls this dev container's Docker profile refuses
/// first — `kcmp`, `pidfd_getfd`, io_uring, the keyrings: owed on electra's
/// host OS, where nothing else refuses them.
#[test]
#[ignore = "this container's Docker seccomp profile refuses these calls first; run on electra's host OS"]
fn the_calls_this_container_refuses_are_refused_by_keepers_filter() {
    refused_by_keepers_filter("container", 8);
}

/// R96R-03, R213: a run cannot change the metadata of a file outside every
/// writable grant — its mode, times, owner, extended attributes — that this
/// process can change; nor, by the same rule, of one inside `workspace/`.
#[test]
fn metadata_outside_the_workspace_is_unchanged() {
    use std::os::unix::fs::PermissionsExt;
    let tree = tree();
    let tools = tree.tools.join("tool");
    write(&tools, "x");
    std::fs::set_permissions(&tools, std::fs::Permissions::from_mode(0o644)).expect("mode");
    let host = host_with(&tree, vec![tree.tools.clone()]);
    let scratch = tree.base.join("scratch");
    write(&scratch, "x");
    let outside = control(&tree, &["meta", &scratch.display().to_string()]);
    assert_eq!(outside.len(), 8, "{outside:?}");
    for (name, errno) in &outside {
        assert_ne!(*errno, EPERM, "the control of {name}: {outside:?}");
    }
    let before = std::fs::metadata(&tools).expect("meta");
    for target in [tools.display().to_string(), "a.txt".to_owned()] {
        let inside = sandboxed(&host, &tree, &["meta", &target], false);
        assert_eq!(inside.len(), 8, "{inside:?}");
        for (name, errno) in &inside {
            assert_eq!(*errno, EPERM, "{name} on {target}: {inside:?}");
        }
    }
    let after = std::fs::metadata(&tools).expect("meta");
    assert_eq!(after.permissions().mode(), before.permissions().mode());
    assert_eq!(after.modified().ok(), before.modified().ok());
}

/// (g), (k), (l) R147: without network no TCP connect and no UDP datagram
/// leaves; with it both do. In every run a pathname Unix socket is
/// unreachable and agentd cannot be signalled, while `socketpair` works —
/// each control passing outside first, so the refusal is the sandbox's.
#[test]
fn the_network_and_local_sockets_follow_the_plan() {
    let tree = tree();
    let host = host(&tree);
    let tcp = TcpListener::bind("127.0.0.1:0").expect("tcp");
    let udp = UdpSocket::bind("127.0.0.1:0").expect("udp");
    udp.set_read_timeout(Some(Duration::from_millis(500)))
        .expect("timeout");
    let socket = tree.base.join("agent.sock");
    let _unix = UnixListener::bind(&socket).expect("unix");
    let ports = [
        tcp.local_addr().expect("addr").port().to_string(),
        udp.local_addr().expect("addr").port().to_string(),
        socket.display().to_string(),
    ];
    let args: Vec<&str> = std::iter::once("net")
        .chain(ports.iter().map(String::as_str))
        .collect();
    let mut buf = [0u8; 64];
    let arrived = |udp: &UdpSocket, buf: &mut [u8]| {
        udp.recv(buf)
            .is_ok_and(|read| &buf[..read] == b"udp-arrived")
    };

    let outside = control(&tree, &args);
    for (name, errno) in &outside {
        assert_eq!(
            *errno, 0,
            "the control of {name} failed outside: {outside:?}"
        );
    }
    assert!(arrived(&udp, &mut buf), "the control's datagram");

    let without = sandboxed(&host, &tree, &args, false);
    let errno = |pairs: &[(String, i32)], name: &str| {
        pairs
            .iter()
            .find(|(found, _)| found == name)
            .map(|(_, errno)| *errno)
            .expect(name)
    };
    for name in ["tcp", "udp", "unix", "kill"] {
        assert_ne!(
            errno(&without, name),
            0,
            "{name} without network: {without:?}"
        );
    }
    assert_eq!(errno(&without, "socketpair"), 0);
    assert!(
        !arrived(&udp, &mut buf),
        "a datagram left a run without network"
    );

    let with = sandboxed(&host, &tree, &args, true);
    assert_eq!(errno(&with, "tcp"), 0, "{with:?}");
    assert_eq!(errno(&with, "udp"), 0, "{with:?}");
    assert!(arrived(&udp, &mut buf), "the networked run's datagram");
    assert_ne!(errno(&with, "unix"), 0, "{with:?}");
    assert_ne!(errno(&with, "kill"), 0, "{with:?}");
}

/// R96R-05, R213: a descriptor inherited beyond the standard streams — a
/// readable secret file, a connected socket — is closed before the program
/// starts: the run goes and the program finds neither (`EBADF`), where the
/// same program run outside, inheriting them the same way, finds both.
/// Every run here also starts under cargo's own leaked descriptors.
#[test]
fn an_inherited_descriptor_never_reaches_the_program() {
    use rustix::io::{fcntl_setfd, FdFlags};
    use std::os::fd::AsRawFd;
    const EBADF: i32 = 9;
    let tree = tree();
    let host = host(&tree);
    let secret = std::fs::File::open(tree.secrets.join("x")).expect("secret");
    let (ours, theirs) = std::os::unix::net::UnixStream::pair().expect("pair");
    let numbers = [secret.as_raw_fd(), theirs.as_raw_fd()].map(|fd| fd.to_string());
    let args = ["fds", numbers[0].as_str(), numbers[1].as_str()];
    let mut argv = vec!["python3", "probe.py"];
    argv.extend_from_slice(&args);
    let prepared = host
        .prepare(&json!({ "argv": argv }), session(&tree), &[])
        .expect("prepared");
    let (outside, ran) = {
        let _start = STARTS.lock().unwrap_or_else(|p| p.into_inner());
        fcntl_setfd(&secret, FdFlags::empty()).expect("inheritable");
        fcntl_setfd(&theirs, FdFlags::empty()).expect("inheritable");
        let outside = control(&tree, &args);
        let ran = host.execute(&prepared);
        fcntl_setfd(&secret, FdFlags::CLOEXEC).expect("cloexec");
        fcntl_setfd(&theirs, FdFlags::CLOEXEC).expect("cloexec");
        (outside, ran)
    };
    assert!(
        outside.iter().all(|(_, errno)| *errno == 0) && outside.len() == 2,
        "the control did not inherit them: {outside:?}"
    );
    let ran = ran.expect("it ran");
    assert_eq!(ran.exit, Exit::Code(0), "{ran:?}");
    let inside = errnos(&ran.stdout.text());
    assert_eq!(inside.len(), 2, "{inside:?}");
    for (name, errno) in &inside {
        assert_eq!(*errno, EBADF, "{name} reached the program: {inside:?}");
    }
    drop((ours, theirs, secret));
}

/// R96R-12, R213: what runs is checked once more inside the sandbox, right
/// before the program starts — a program replaced after the run was
/// prepared does not run, nor one replaced after agentd's own last look,
/// which only the trampoline's check can see.
#[test]
fn a_program_replaced_after_its_check_does_not_run() {
    let tree = tree();
    let tool = tree.tools.join("mytool");
    std::fs::copy("/usr/bin/true", &tool).expect("copy");
    let host = host_with(&tree, vec![tree.tools.clone()]);
    let prepared = host
        .prepare(&json!({"argv": ["mytool"]}), session(&tree), &[])
        .expect("prepared");
    std::fs::copy("/usr/bin/false", &tool).expect("replace");
    {
        let _start = STARTS.lock().unwrap_or_else(|p| p.into_inner());
        let refused = host.execute(&prepared).expect_err("refused");
        assert!(refused.contains("its program changed"), "{refused}");
    }
    // The trampoline started through a shell that, once armed, replaces
    // the program between agentd's check and the trampoline's.
    std::fs::copy("/usr/bin/true", &tool).expect("restore");
    let armed = tree.base.join("armed");
    let script = format!(
        "if [ -e '{}' ]; then /usr/bin/cp /usr/bin/false '{}'; fi; exec '{}' {TRAMPOLINE_ARG} \"$1\"",
        armed.display(),
        tool.display(),
        env!("CARGO_BIN_EXE_keeper-agentd"),
    );
    let late = {
        let _start = STARTS.lock().unwrap_or_else(|p| p.into_inner());
        SandboxHost::probe(
            Kind::Trampoline {
                program: PathBuf::from("/bin/sh"),
                args: vec!["-c".into(), script.into(), "sh".into()],
            },
            "electra",
            &SandboxTable {
                read_exec: vec![tree.tools.clone()],
                env: Vec::new(),
            },
            &forbidden(&tree),
        )
        .expect("the shell starts the trampoline")
    };
    let ran = try_run(&late, &tree, json!({"argv": ["mytool"]})).expect("unarmed, it runs");
    assert_eq!(ran.exit, Exit::Code(0), "{ran:?}");
    write(&armed, "");
    let refused = try_run(&late, &tree, json!({"argv": ["mytool"]})).expect_err("refused");
    assert!(refused.contains("its program changed"), "{refused}");
}

/// The trampoline started through a Python wrapper that, once `armed`
/// exists, hands it a standard output whose pipe is already full: the
/// trampoline then blocks writing its sentinel — after its last check of
/// the programs, holding `tool` open if it checked it through a handle —
/// and the wrapper, seeing that (or, after five seconds blocked, without
/// it: a trampoline that holds no handle of `tool` never will), puts
/// `failing` — a script that exits 1 — at `tool`'s path before it drains
/// the pipe.
const SWAP_AFTER_CHECK: &str = r#"
import os, shutil, sys, time
armed, tool, failing, agentd, arg, plan = sys.argv[1:7]
if not os.path.exists(armed):
    os.execv(agentd, [agentd, arg, plan])
r, w = os.pipe()
os.set_blocking(w, False)
try:
    while True:
        os.write(w, b"\0" * 65536)
except BlockingIOError:
    pass
os.set_blocking(w, True)
pid = os.fork()
if pid == 0:
    os.dup2(w, 1)
    os.execv(agentd, [agentd, arg, plan])
os.close(w)
def holds_program():
    try:
        fds = os.listdir(f"/proc/{pid}/fd")
    except OSError:
        return False
    for fd in fds:
        try:
            if os.readlink(f"/proc/{pid}/fd/{fd}") == tool:
                return True
        except OSError:
            pass
    return False
def waiting():
    with open(f"/proc/{pid}/stat") as stat:
        return stat.read().rsplit(")", 1)[1].split()[0] == "S"
blocked = None
while not (holds_program() and waiting()):
    if waiting():
        blocked = blocked or time.monotonic()
        if time.monotonic() - blocked > 5:
            break
    else:
        blocked = None
    time.sleep(0.01)
shutil.copy(failing, tool + ".new")
os.rename(tool + ".new", tool)
while os.read(r, 65536):
    pass
sys.exit(os.waitstatus_to_exitcode(os.waitpid(pid, 0)[1]))
"#;

/// R96R2-08, R231, R96R3-01, R247, R96R4-02/03, R260: the trampoline
/// starts each program it checked by the handle it checked it through —
/// the program started, the one `env` starts, the one a nested wrapper
/// (`env nice`) starts, a wrapper `env` starts by its path, the one
/// BusyBox's `env` or `nice` starts, directly or behind `env`, and the one
/// `env` starts after an assignment given after `--`, here a workspace
/// link named `NAME=value`: one put at its path after that last check,
/// while the trampoline waits to say the sandbox holds, is not what runs,
/// and the wrappers still do what they do. Unarmed, the same wrapper runs
/// each as usual (the control).
#[test]
fn a_program_swapped_after_the_last_check_is_not_what_runs() {
    let tree = tree();
    let tool = tree.tools.join("mytool");
    let nice = tree.tools.join("nice");
    let nice_arg = nice.display().to_string();
    let busybox = tree.tools.join("busybox");
    std::fs::copy(test_busybox(), &busybox).expect("copy busybox");
    let bb_path = busybox.display().to_string();
    let bb = bb_path.as_str();
    std::os::unix::fs::symlink("/usr/bin/true", tree.workspace.join("NAME=value")).expect("link");
    std::fs::copy("/usr/bin/true", &tool).expect("copy");
    // `env` reads `./NAME=value` after `--` as an assignment, as it runs:
    // the program bound is `mytool`, never what the link leads to, and the
    // assignment is a setting given inline.
    let assigned = host_with(&tree, vec![tree.tools.clone()])
        .prepare(
            &json!({"argv": ["env", "--", "./NAME=value", "mytool"]}),
            session(&tree),
            &[],
        )
        .expect("prepared");
    assert_eq!(
        assigned.exec_binding["program"]["path"],
        json!(tool.display().to_string()),
        "{}",
        assigned.exec_binding
    );
    assert!(assigned.call_facts.held_code, "{:?}", assigned.operands);
    // Not `/usr/bin/false`: on this host it is the multi-call binary
    // `nice` is too, the same bytes.
    let failing = tree.base.join("failing");
    write(&failing, "#!/bin/sh\nexit 1\n");
    std::fs::set_permissions(
        &failing,
        std::os::unix::fs::PermissionsExt::from_mode(0o755),
    )
    .expect("mode");
    let forms: [(&[&str], &Path); 8] = [
        (&["mytool"], &tool),
        (&["env", "mytool"], &tool),
        (&["env", "nice", "-n", "3", "mytool"], &tool),
        (&["env", &nice_arg, "mytool"], &nice),
        (&[bb, "env", "mytool"], &tool),
        (&[bb, "nice", "-n", "3", "mytool"], &tool),
        (&["env", bb, "env", "mytool"], &tool),
        (&["env", "--", "./NAME=value", "mytool"], &tool),
    ];
    let armed = tree.base.join("armed");
    for (argv, swapped) in forms {
        std::fs::copy("/usr/bin/true", &tool).expect("copy");
        std::fs::copy("/usr/bin/nice", &nice).expect("copy nice");
        let _ = std::fs::remove_file(&armed);
        let swapping = {
            let _start = STARTS.lock().unwrap_or_else(|p| p.into_inner());
            SandboxHost::probe(
                Kind::Trampoline {
                    program: PathBuf::from("/usr/bin/python3"),
                    args: vec![
                        "-c".into(),
                        SWAP_AFTER_CHECK.into(),
                        armed.clone().into(),
                        swapped.into(),
                        failing.clone().into(),
                        env!("CARGO_BIN_EXE_keeper-agentd").into(),
                        TRAMPOLINE_ARG.into(),
                    ],
                },
                "electra",
                &SandboxTable {
                    read_exec: vec![tree.tools.clone()],
                    env: Vec::new(),
                },
                &forbidden(&tree),
            )
            .expect("the wrapper starts the trampoline")
        };
        let ran = try_run(&swapping, &tree, json!({ "argv": argv })).expect("unarmed, it ran");
        assert_eq!(ran.exit, Exit::Code(0), "{argv:?} unarmed: {ran:?}");
        // Replaced after the run was prepared, before any check: refused.
        let prepared = swapping
            .prepare(&json!({ "argv": argv }), session(&tree), &[])
            .expect("prepared");
        let original = std::fs::read(swapped).expect("original");
        std::fs::copy(&failing, swapped).expect("replace");
        let refused = {
            let _start = STARTS.lock().unwrap_or_else(|p| p.into_inner());
            swapping.execute(&prepared).expect_err("refused")
        };
        assert!(
            refused.contains("changed after keeper checked it"),
            "{argv:?}: {refused}"
        );
        std::fs::write(swapped, original).expect("restore");
        write(&armed, "");
        let ran = try_run(&swapping, &tree, json!({ "argv": argv })).expect("it ran");
        assert_eq!(
            std::fs::read(swapped).expect("swapped"),
            std::fs::read(&failing).expect("failing"),
            "{argv:?}: the wrapper put another program at the path"
        );
        assert_eq!(
            ran.exit,
            Exit::Code(0),
            "{argv:?}: the swapped-in program ran: {ran:?}"
        );
    }
    // What each wrapper does it still does: `env` sets — an assignment
    // after `--` too — and `nice` lowers the priority, BusyBox's as well,
    // of the program it was handed by its descriptor.
    write(
        &tree.workspace.join("kept.py"),
        "import os\nprint(os.environ.get('FOO'), os.nice(0))\n",
    );
    let host = host_with(&tree, vec![tree.tools.clone()]);
    let said = |argv: &[&str]| run(&host, &tree, json!({ "argv": argv })).stdout.text();
    let plain = said(&["env", "FOO=kept", "python3", "kept.py"]);
    let (foo, base) = plain.trim().split_once(' ').expect("two words");
    assert_eq!(foo, "kept", "{plain}");
    let base: i32 = base.parse().expect("a niceness");
    assert_eq!(
        said(&["env", "--", "FOO=kept", "python3", "kept.py"]),
        plain
    );
    let niced = format!("kept {}", (base + 7).min(19));
    for argv in [
        &["env", "FOO=kept", "nice", "-n", "7", "python3", "kept.py"][..],
        &[
            bb, "env", "FOO=kept", bb, "nice", "-n", "7", "python3", "kept.py",
        ],
    ] {
        assert_eq!(said(argv).trim(), niced, "{argv:?}");
    }
}

/// An agent-authored wrapper: it prints what each descriptor it inherited
/// beyond the standard streams reads.
const READS_ITS_DESCRIPTORS: &str = "#!/usr/bin/python3
import os
for fd in range(3, 256):
    try:
        os.lseek(fd, 0, os.SEEK_SET)
        print(os.read(fd, 65536).decode(errors='replace'))
    except OSError:
        pass
";

/// R96R4-01, R260: a program a wrapper starts is opened for it only inside
/// the sandbox, as the run's own grants let it be: a workspace wrapper
/// named like `env` given an executable outside every grant is refused,
/// and reads nothing of it; given one its grants let it read, it runs and
/// reads that program through the descriptor it was handed (the control,
/// so the refusal is not a wrapper that reads nothing).
#[test]
fn a_wrapper_is_handed_no_program_outside_the_runs_grants() {
    use std::os::unix::fs::PermissionsExt;
    let tree = tree();
    let host = host_with(&tree, vec![tree.tools.clone()]);
    let executable = |path: &Path, text: &str| {
        write(path, text);
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).expect("mode");
    };
    executable(&tree.workspace.join("env"), READS_ITS_DESCRIPTORS);
    let private = tree.base.join("unrequested/private-helper.sh");
    executable(&private, "#!/bin/sh\n# private-helper-bytes\n");
    let allowed = tree.tools.join("allowed-helper.sh");
    executable(&allowed, "#!/bin/sh\n# allowed-helper-bytes\n");
    let ran = run(
        &host,
        &tree,
        json!({"argv": ["./env", allowed.display().to_string()]}),
    );
    assert_eq!(ran.exit, Exit::Code(0), "{ran:?}");
    assert!(
        ran.stdout.text().contains("allowed-helper-bytes"),
        "the control read no handed descriptor: {ran:?}"
    );
    let refused = try_run(
        &host,
        &tree,
        json!({"argv": ["./env", private.display().to_string()]}),
    );
    let Err(refused) = refused else {
        panic!("a program outside every grant was handed on: {refused:?}");
    };
    assert!(refused.contains("not one this run may read"), "{refused}");
    assert!(!refused.contains("private-helper-bytes"), "{refused}");
}

/// R96R4-02, R260: what a BusyBox applet starts is bound and handed on as
/// any wrapper's program is — `busybox env ./tool`, `busybox nice ./tool`
/// and `env busybox env ./tool` run code the session holds, at its tier —
/// while `busybox ls` is BusyBox's own applet (the control: T2, and it
/// runs). A session allowance of `env busybox ls` does not cover `env
/// busybox env <program>` nor `env busybox env ./tool`. An applet whose
/// options keeper does not read is refused, and nothing runs.
#[test]
fn what_busybox_starts_is_bound_and_handed_on() {
    use keeper_core::agents::label::Integrity;
    use keeper_core::agents::run::{allowance_key, RunAllowance, BUSYBOX_APPLET};
    use keeper_core::agents::session::SessionKind;
    use keeper_core::agents::tier::{classify, AgentTool, Context, Tier};
    let tree = tree();
    let busybox = tree.tools.join("busybox");
    std::fs::copy(test_busybox(), &busybox).expect("copy busybox");
    let bb_path = busybox.display().to_string();
    let bb = bb_path.as_str();
    std::fs::copy("/usr/bin/true", tree.workspace.join("tool")).expect("tool");
    std::fs::copy("/usr/bin/true", tree.tools.join("mytool")).expect("mytool");
    let host = host_with(&tree, vec![tree.tools.clone()]);
    let prepare = |argv: &[&str]| {
        host.prepare(&json!({ "argv": argv }), session(&tree), &[])
            .expect("prepared")
    };
    let context = Context {
        delegated: false,
        unattended: false,
        integrity: Integrity::Owner,
        via_kvm: false,
        grant: None,
    };
    let tier = |prepared: &keeper_agent::run::Prepared| {
        classify(AgentTool::Run, &prepared.call_facts, &context).tier
    };
    for argv in [
        &[bb, "env", "./tool"][..],
        &[bb, "nice", "-n", "3", "./tool"],
        &["env", bb, "env", "./tool"],
    ] {
        let held = prepare(argv);
        assert_eq!(tier(&held), Tier::T4, "{argv:?}: {:?}", held.operands);
        assert!(
            held.operands
                .iter()
                .any(|operand| operand.path.as_deref() == Some("tool")),
            "{argv:?}: {:?}",
            held.operands
        );
        let ran = run(&host, &tree, json!({ "argv": argv }));
        assert_eq!(ran.exit, Exit::Code(0), "{argv:?}: {ran:?}");
    }
    let applet = prepare(&[bb, "ls"]);
    assert_eq!(tier(&applet), Tier::T2, "{:?}", applet.operands);
    let ran = run(&host, &tree, json!({ "argv": [bb, "ls"] }));
    assert_eq!(ran.exit, Exit::Code(0), "{ran:?}");
    assert!(ran.stdout.text().contains("a.txt"), "{ran:?}");
    let listing = prepare(&["env", bb, "ls"]);
    let allowance = RunAllowance {
        approval: "01ALLOW".to_owned(),
        key: allowance_key(&listing.exec_binding),
        ends: chrono::Utc::now() + chrono::Duration::hours(1),
    };
    let covers = |prepared: &keeper_agent::run::Prepared| {
        allowance.covers(
            &prepared.exec_binding,
            tier(prepared),
            SessionKind::Conversation,
            chrono::Utc::now(),
        )
    };
    assert!(covers(&listing), "the allowance covers its own run");
    let nested = prepare(&["env", bb, "env", "mytool"]);
    assert_eq!(tier(&nested), Tier::T2);
    assert!(!covers(&nested), "{}", nested.exec_binding);
    assert!(!covers(&prepare(&["env", bb, "env", "./tool"])));
    let refused = try_run(
        &host,
        &tree,
        json!({"argv": [bb, "timeout", "5", "touch", "timeout-ran"]}),
    )
    .expect_err("refused");
    assert_eq!(refused, BUSYBOX_APPLET);
    assert!(
        !tree.workspace.join("timeout-ran").exists(),
        "a refused applet ran"
    );
}

/// Builds C `source` into `out` with the host's `cc` — the linker cargo
/// links these tests with — as a program, or a shared library when
/// `shared`.
fn compile(tree: &Tree, source: &str, out: &Path, shared: bool) {
    let name = out.file_name().expect("name").to_string_lossy();
    let file = tree.base.join(format!("{name}.c"));
    std::fs::write(&file, source).expect("source");
    let mut cc = std::process::Command::new("cc");
    if shared {
        cc.args(["-shared", "-fPIC"]);
    }
    let built = cc.arg("-o").arg(out).arg(&file).output().expect("cc");
    assert!(built.status.success(), "{built:?}");
}

/// R96R5-01/02, R269: what real BusyBox would run or set where keeper's
/// reading differs from it is refused, never modelled. `busybox nice -n 3
/// -n./payload /usr/bin/true` runs `workspace/-n./payload` — outside, the
/// control, it makes its marker — and is refused with nothing made, while
/// one adjustment runs and binds the program after it; `busybox env -u
/// LD_PRELOAD=./payload.so /usr/bin/true` loads that library — outside it
/// makes its marker — and is refused in every spelling, while `-u NAME`
/// runs. A refused form is refused before any allowance is looked at, so
/// the benign form's allowance never reaches it.
#[test]
fn what_busybox_would_run_or_set_unread_is_refused() {
    use keeper_core::agents::run::{BUSYBOX_NICE, UNSET_ASSIGNS};
    let tree = tree();
    let busybox = tree.tools.join("busybox");
    std::fs::copy(test_busybox(), &busybox).expect("copy busybox");
    let bb_path = busybox.display().to_string();
    let bb = bb_path.as_str();
    std::fs::create_dir_all(tree.workspace.join("-n.")).expect("-n.");
    compile(
        &tree,
        "#include <fcntl.h>\n#include <unistd.h>\nint main(void) { return close(open(\"payload-ran\", O_CREAT | O_WRONLY, 0644)); }\n",
        &tree.workspace.join("-n./payload"),
        false,
    );
    compile(
        &tree,
        "#include <fcntl.h>\n#include <unistd.h>\n__attribute__((constructor)) static void ran(void) { close(open(\"library-ran\", O_CREAT | O_WRONLY, 0644)); }\n",
        &tree.workspace.join("payload.so"),
        true,
    );
    let host = host_with(&tree, vec![tree.tools.clone()]);
    let outside = |args: &[&str], marker: &str| {
        let done = std::process::Command::new(bb)
            .args(args)
            .current_dir(&tree.workspace)
            .output()
            .expect("busybox");
        assert!(done.status.success(), "{args:?}: {done:?}");
        let made = tree.workspace.join(marker);
        assert!(made.exists(), "{args:?} outside made no {marker}");
        std::fs::remove_file(made).expect("marker");
    };
    let refused = |argv: &[&str], reason: &str, marker: &str| {
        let refusal = try_run(&host, &tree, json!({ "argv": argv })).expect_err("refused");
        assert_eq!(refusal, reason, "{argv:?}");
        assert!(!tree.workspace.join(marker).exists(), "{argv:?} ran");
    };
    let runs = |argv: &[&str]| {
        let prepared = host
            .prepare(&json!({ "argv": argv }), session(&tree), &[])
            .expect("prepared");
        let ran = run(&host, &tree, json!({ "argv": argv }));
        assert_eq!(ran.exit, Exit::Code(0), "{argv:?}: {ran:?}");
        prepared
    };

    let variant = [bb, "nice", "-n", "3", "-n./payload", "/usr/bin/true"];
    outside(&variant[1..], "payload-ran");
    refused(&variant, BUSYBOX_NICE, "payload-ran");
    refused(
        &[bb, "nice", "-n3", "-n./payload", "/usr/bin/true"],
        BUSYBOX_NICE,
        "payload-ran",
    );
    let benign = runs(&[bb, "nice", "-n", "3", "/usr/bin/true"]);
    let bound = benign.exec_binding["program"]["path"]
        .as_str()
        .map(|path| Path::new(path).canonicalize().expect("bound program"));
    assert_eq!(
        bound,
        Some(Path::new("/usr/bin/true").canonicalize().expect("true")),
        "{}",
        benign.exec_binding
    );

    let variant = [bb, "env", "-u", "LD_PRELOAD=./payload.so", "/usr/bin/true"];
    outside(&variant[1..], "library-ran");
    for argv in [
        &variant[..],
        &[bb, "env", "-uLD_PRELOAD=./payload.so", "/usr/bin/true"],
        &[
            bb,
            "env",
            "--unset=LD_PRELOAD=./payload.so",
            "/usr/bin/true",
        ],
    ] {
        refused(argv, UNSET_ASSIGNS, "library-ran");
    }
    runs(&[bb, "env", "-u", "NAME", "/usr/bin/true"]);
}

/// R96R2-04, R231: an option's value attached runs on this host exactly
/// as it would apart — run outside, `env -S<string>` runs a shell string
/// and `git rebase -x<command>` a command line, each making its marker —
/// and keeper refuses both, so neither marker is made by a run.
#[test]
fn an_attached_command_line_is_refused_as_it_would_run() {
    let tree = tree();
    let host = host(&tree);
    let repo = tree.workspace.join("repo");
    std::fs::create_dir_all(&repo).expect("repo");
    let git = |args: &[&str]| {
        let done = std::process::Command::new("git")
            .args(["-c", "user.name=t", "-c", "user.email=t@example.org"])
            .args(args)
            .current_dir(&repo)
            .output()
            .expect("git");
        assert!(done.status.success(), "{done:?}");
    };
    git(&["init", "-q"]);
    for n in ["1", "2"] {
        git(&["commit", "-q", "--allow-empty", "-m", n]);
    }
    let marker = |name: &str| tree.base.join(name);
    let packed = format!("-Ssh -c 'touch {}'", marker("env").display());
    let rebase = format!("-xtouch {}", marker("rebase").display());
    let outside = std::process::Command::new("env")
        .arg(&packed)
        .status()
        .expect("env");
    assert!(
        outside.success() && marker("env").exists(),
        "env -S ran no shell"
    );
    git(&["rebase", "-q", &rebase, "HEAD~1"]);
    assert!(marker("rebase").exists(), "rebase -x ran no command");
    for name in ["env", "rebase"] {
        std::fs::remove_file(marker(name)).expect("rm");
    }
    for argv in [
        json!(["env", packed]),
        json!(["git", "-C", "repo", "rebase", rebase, "HEAD~1"]),
    ] {
        let refused = try_run(&host, &tree, json!({ "argv": argv })).expect_err("refused");
        assert!(refused.contains("keeper never runs"), "{refused}");
    }
    for name in ["env", "rebase"] {
        assert!(!marker(name).exists(), "{name}'s marker was made");
    }
}

/// R96R2-06, R231: a repository whose git folder is apart from it — `git
/// init --separate-git-dir`, a `.git` file pointing at `meta/` — runs what
/// `meta/config` names (outside, its alias makes a marker): that
/// configuration is code the session holds, bound by its hash, and changed
/// after the run was prepared the run does not start; naming the git
/// folder with `--git-dir` is refused.
#[test]
fn a_separate_git_folder_is_code_the_session_holds() {
    let tree = tree();
    let host = host(&tree);
    let marker = tree.base.join("alias-ran");
    let made = std::process::Command::new("git")
        .args(["init", "-q", "--separate-git-dir=meta", "repo"])
        .current_dir(&tree.workspace)
        .status()
        .expect("git");
    assert!(made.success());
    let config = tree.workspace.join("meta/config");
    let mut text = std::fs::read_to_string(&config).expect("config");
    text.push_str(&format!("[alias]\n\tmark = !touch {}\n", marker.display()));
    std::fs::write(&config, &text).expect("alias");
    let outside = std::process::Command::new("git")
        .args(["-C", "repo", "mark"])
        .current_dir(&tree.workspace)
        .status()
        .expect("git");
    assert!(outside.success() && marker.exists(), "git ran no alias");
    std::fs::remove_file(&marker).expect("rm");
    let prepared = host
        .prepare(
            &json!({"argv": ["git", "-C", "repo", "status"]}),
            session(&tree),
            &[],
        )
        .expect("prepared");
    assert!(prepared.call_facts.held_code, "{:?}", prepared.operands);
    assert!(
        prepared
            .operands
            .iter()
            .any(|operand| operand.path.as_deref() == Some("meta/config")),
        "{:?}",
        prepared.operands
    );
    std::fs::write(&config, text.replace("mark =", "other =")).expect("drift");
    let refused = {
        let _start = STARTS.lock().unwrap_or_else(|p| p.into_inner());
        host.execute(&prepared).expect_err("refused")
    };
    assert!(
        refused.contains("workspace/meta/config changed"),
        "{refused}"
    );
    let refused = try_run(
        &host,
        &tree,
        json!({"argv": ["git", "--git-dir=meta", "status"]}),
    )
    .expect_err("refused");
    assert!(refused.contains("--git-dir"), "{refused}");
    assert!(!marker.exists(), "a run made the marker");
}

/// R96R3-03, R247: a bare repository whose `objects/` or `refs/` is a link,
/// and a repository whose `.git` is a link to its git folder, are ones git
/// reads — outside, each one's alias makes a marker — and ones keeper does
/// not read as git does: a run there is refused, even `ls`, and none makes
/// the marker. Such a link put there after a run was prepared, or an alias
/// added after to a configuration that ran nothing, stops the run before
/// it starts. A project's own file named `commondir` refuses nothing.
#[test]
fn a_repository_keeper_cannot_read_as_git_does_refuses_the_run() {
    use std::os::unix::fs::symlink;
    let tree = tree();
    let host = host(&tree);
    let workspace = &tree.workspace;
    let marker = tree.base.join("alias-ran");
    let git = |args: &[&str]| {
        let done = std::process::Command::new("git")
            .args(args)
            .current_dir(workspace)
            .output()
            .expect("git");
        assert!(done.status.success(), "{args:?}: {done:?}");
    };
    let alias = |config: &Path| {
        let mut text = std::fs::read_to_string(config).expect("config");
        text.push_str(&format!("[alias]\n\tmark = !touch {}\n", marker.display()));
        std::fs::write(config, text).expect("alias");
    };
    write(
        &workspace.join("notes/commondir"),
        "the common folder of these notes\n",
    );
    assert_eq!(
        run(&host, &tree, json!({"argv": ["ls"]})).exit,
        Exit::Code(0)
    );
    for shape in ["objects", "refs", ".git"] {
        let linked = workspace.join("linked");
        if shape == ".git" {
            git(&["init", "-q", "--separate-git-dir=meta", "linked"]);
            std::fs::remove_file(linked.join(".git")).expect("rm pointer");
            symlink("../meta", linked.join(".git")).expect("link");
            alias(&workspace.join("meta/config"));
        } else {
            git(&["init", "-q", "--bare", "linked"]);
            std::fs::rename(linked.join(shape), workspace.join("store")).expect("move");
            symlink("../store", linked.join(shape)).expect("link");
            alias(&linked.join("config"));
        }
        git(&["-C", "linked", "mark"]);
        assert!(marker.exists(), "{shape}: git read no repository");
        std::fs::remove_file(&marker).expect("rm");
        for argv in [json!(["git", "-C", "linked", "mark"]), json!(["ls"])] {
            let refused = try_run(&host, &tree, json!({ "argv": argv })).expect_err("refused");
            assert!(
                refused.contains("is a link where git keeps"),
                "{shape}: {refused}"
            );
        }
        assert!(!marker.exists(), "{shape}: a run made the marker");
        for made in ["linked", "store", "meta"] {
            let _ = std::fs::remove_dir_all(workspace.join(made));
        }
    }
    let later = workspace.join("later.git");
    git(&["init", "-q", "--bare", "later.git"]);
    let prepare = || {
        host.prepare(&json!({"argv": ["ls"]}), session(&tree), &[])
            .expect("prepared")
    };
    let execute = |prepared| {
        let _start = STARTS.lock().unwrap_or_else(|p| p.into_inner());
        host.execute(prepared)
    };
    let prepared = prepare();
    std::fs::rename(later.join("objects"), workspace.join("store")).expect("move");
    symlink("../store", later.join("objects")).expect("link");
    let refused = execute(&prepared).expect_err("a link put there since");
    assert!(refused.contains("is a link where git keeps"), "{refused}");
    std::fs::remove_file(later.join("objects")).expect("rm link");
    std::fs::rename(workspace.join("store"), later.join("objects")).expect("back");
    let prepared = prepare();
    assert!(!prepared.call_facts.held_code, "{:?}", prepared.operands);
    alias(&later.join("config"));
    let refused = execute(&prepared).expect_err("an alias added since");
    assert!(
        refused.contains("workspace/later.git/config changed"),
        "{refused}"
    );
    assert!(!marker.exists(), "a run made the marker");
}

/// 96.1 #6, R213: a child that tries to leave the process group cannot,
/// and a grandchild holding the pipes open dies with the group: the run
/// says `timed out after 1 s` within the drain's bound, and nothing of it
/// is alive a second later.
#[test]
fn run_timeout_kills_the_process_group() {
    let tree = tree();
    let host = host(&tree);
    write(
        &tree.workspace.join("fork.py"),
        "import os, sys, time\nif os.fork() == 0:\n    try:\n        os.setsid()\n        print('left', flush=True)\n    except OSError as e:\n        print('stayed', e.errno, flush=True)\n    if os.fork() == 0:\n        print(os.getpid(), flush=True)\n        time.sleep(60)\n    sys.exit(0)\ntime.sleep(60)\n",
    );
    let started = Instant::now();
    let ran = run(
        &host,
        &tree,
        json!({"argv": ["python3", "fork.py"], "timeout_s": 1}),
    );
    assert!(
        started.elapsed() < Duration::from_secs(10),
        "{:?}",
        started.elapsed()
    );
    assert_eq!(ran.exit, Exit::TimedOut(1));
    // R231: on Linux what ended is the whole group, which nothing left.
    assert_eq!(ran.reaped, Reaped::Group);
    assert!(
        ran.render().starts_with("timed out after 1 s"),
        "{}",
        ran.render()
    );
    let text = ran.stdout.text();
    let mut lines = text.lines();
    assert_eq!(lines.next(), Some("stayed 1"), "{text}");
    let grandchild: i32 = lines.next().expect("its pid").parse().expect("a pid");
    std::thread::sleep(Duration::from_secs(1));
    let pid = rustix::process::Pid::from_raw(grandchild).expect("a pid");
    assert!(
        rustix::process::test_kill_process(pid).is_err(),
        "the grandchild is alive"
    );
}

/// 96.1 #7: each stream keeps its first 64 KiB and says how much there was.
#[test]
fn run_output_is_capped_and_says_so() {
    let tree = tree();
    let host = host(&tree);
    let ran = run(
        &host,
        &tree,
        json!({"argv": ["/usr/bin/yes"], "timeout_s": 1}),
    );
    assert_eq!(ran.stdout.bytes.len(), STREAM_CAP);
    assert!(ran.stdout.total > STREAM_CAP as u64, "{}", ran.stdout.total);
    assert!(ran.render().contains(&format!(
        "stdout (truncated: {{shown: {STREAM_CAP}, total: {}}})",
        ran.stdout.total
    )));
    write(
        &tree.workspace.join("err.py"),
        "import sys\nsys.stderr.write('e' * 100000)\nprint('out')\n",
    );
    let ran = run(&host, &tree, json!({"argv": ["python3", "err.py"]}));
    assert_eq!(ran.stdout.text(), "out\n");
    assert_eq!(
        (ran.stderr.bytes.len(), ran.stderr.total),
        (STREAM_CAP, 100_000)
    );
}

/// R96R-20: invalid UTF-8 on both streams, each over the cap — the result
/// the model reads, through the tool renderer's own bound, discloses what
/// it shows of each stream in the stream's bytes and is not cut again.
#[test]
fn binary_output_is_disclosed_through_the_tool_result() {
    let tree = tree();
    let host = host(&tree);
    write(
        &tree.workspace.join("bin.py"),
        "import sys\nsys.stdout.buffer.write(b'\\xff' * 100000)\nsys.stdout.flush()\nsys.stderr.buffer.write(b'\\xfe' * 100000)\n",
    );
    let ran = run(&host, &tree, json!({"argv": ["python3", "bin.py"]}));
    let shown =
        keeper_core::bots::tools::render_result(&keeper_core::bots::tools::ToolOutcome::Answered {
            text: ran.render(),
        });
    assert_eq!(shown, ran.render(), "the tool result cut it again");
    assert!(shown.len() <= keeper_core::bots::tools::MAX_TOOL_RESULT_BYTES);
    for name in ["stdout", "stderr"] {
        let head = shown
            .lines()
            .find(|line| line.starts_with(name))
            .expect("its head");
        assert!(head.contains("truncated: {shown: "), "{head}");
        let shown_bytes: u64 = head
            .split("shown: ")
            .nth(1)
            .and_then(|rest| rest.split(',').next())
            .and_then(|n| n.parse().ok())
            .expect("shown");
        assert!(shown_bytes < 100_000, "{head}");
        assert!(head.contains("total: 100000"), "{head}");
    }
}

/// 96.1 #8: the program sees exactly the allow-list, an empty `HOME` of its
/// own outside the workspace, and both of its folders are gone after it.
#[test]
fn run_environment_is_the_allow_list() {
    std::env::set_var("KEEPER_AGENTD_SECRET_X", "planted-secret");
    std::env::set_var("AWS_SECRET_ACCESS_KEY", "AKIAIOSFODNN7EXAMPLE");
    let tree = tree();
    let host = host(&tree);
    let ran = run(&host, &tree, json!({"argv": ["/usr/bin/env"]}));
    let text = ran.stdout.text();
    let mut names: Vec<&str> = text
        .lines()
        .filter_map(|line| line.split_once('=').map(|(name, _)| name))
        .collect();
    names.sort_unstable();
    assert_eq!(
        names,
        [
            "GIT_CONFIG_GLOBAL",
            "GIT_CONFIG_NOSYSTEM",
            "GIT_TERMINAL_PROMPT",
            "HOME",
            "LANG",
            "NO_COLOR",
            "PATH",
            "TERM",
            "TMPDIR"
        ],
        "{text}"
    );
    assert!(!text.contains("planted-secret"));
    assert!(!text.contains("AKIA"));
    write(
        &tree.workspace.join("home.py"),
        "import os\nprint(os.environ['HOME'])\nprint(os.environ['TMPDIR'])\nprint(len(os.listdir(os.environ['HOME'])))\nprint(len(os.listdir(os.environ['TMPDIR'])))\n",
    );
    let ran = run(&host, &tree, json!({"argv": ["python3", "home.py"]}));
    let text = ran.stdout.text();
    let lines: Vec<&str> = text.lines().collect();
    assert_eq!(lines.len(), 4, "{ran:?}");
    assert_eq!(lines[2], "0", "HOME is empty");
    assert_eq!(lines[3], "0", "TMPDIR is empty");
    for dir in &lines[..2] {
        assert!(!Path::new(dir).starts_with(&tree.workspace), "{dir}");
        assert!(!Path::new(dir).starts_with(&tree.drive), "{dir}");
        assert!(!Path::new(dir).exists(), "{dir} outlived the run");
    }
}

/// 96.1 #8, R96R-09: git runs no hook the session holds — not with a
/// `hooksPath` given on the argv either, which is refused — and reads no
/// configuration from its workspace beyond its repository's.
#[test]
fn run_git_runs_no_hook_and_reads_no_workspace_config() {
    let tree = tree();
    let host = host(&tree);
    let repo = tree.workspace.join("repo");
    std::fs::create_dir_all(&repo).expect("repo");
    let init = std::process::Command::new("git")
        .args(["init", "-q"])
        .current_dir(&repo)
        .status()
        .expect("git");
    assert!(init.success());
    for (key, value) in [("user.name", "t"), ("user.email", "t@example.org")] {
        let set = std::process::Command::new("git")
            .args(["config", key, value])
            .current_dir(&repo)
            .status()
            .expect("git");
        assert!(set.success());
    }
    let marker = tree.workspace.join("hook-ran");
    let hook = repo.join(".git/hooks/pre-commit");
    write(
        &hook,
        &format!(
            "#!/usr/bin/python3\nopen({:?}, 'w').write('ran')\n",
            marker.display().to_string()
        ),
    );
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&hook, std::fs::Permissions::from_mode(0o755)).expect("mode");
    }
    write(
        &tree.workspace.join(".gitconfig"),
        "[user]\n\tname = planted\n[core]\n\thooksPath = .git/hooks\n",
    );
    let ran = run(
        &host,
        &tree,
        json!({"argv": ["git", "-C", "repo", "commit", "--allow-empty", "-m", "x"]}),
    );
    assert_eq!(ran.exit, Exit::Code(0), "{ran:?}");
    for argv in [
        json!([
            "git",
            "-C",
            "repo",
            "-c",
            "core.hooksPath=.git/hooks",
            "commit",
            "--allow-empty",
            "-m",
            "x"
        ]),
        json!([
            "env",
            "GIT_CONFIG_PARAMETERS='core.hooksPath'='.git/hooks'",
            "git",
            "-C",
            "repo",
            "commit",
            "--allow-empty",
            "-m",
            "x"
        ]),
    ] {
        let refused = try_run(&host, &tree, json!({ "argv": argv })).expect_err("refused");
        assert!(refused.contains("keeper"), "{refused}");
    }
    let ran = run(
        &host,
        &tree,
        json!({"argv": ["git", "-C", "repo", "config", "--show-origin", "--list"]}),
    );
    assert_eq!(ran.exit, Exit::Code(0), "{ran:?}");
    for line in ran.stdout.text().lines() {
        assert!(
            line.starts_with("file:.git/config") || line.starts_with("command line:"),
            "{line}"
        );
    }
    assert!(!ran.stdout.text().contains("planted"));
    assert!(!marker.exists(), "the hook ran");
}
