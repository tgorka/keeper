//! Guard tests of the runtime class (OD-A delta r6 §5.2): every single
//! deviation refuses with its label before any child could exist.

use super::*;
use std::os::unix::fs::{symlink, PermissionsExt};

const LIB: &str = "/usr/lib/x86_64-linux-gnu";

/// A host ELF needing only the closure, to stand for the worker.
fn worker_source() -> PathBuf {
    let allowed = [
        "libc.so.6",
        "libm.so.6",
        "libgcc_s.so.1",
        "ld-linux-x86-64.so.2",
    ];
    let candidates = [
        PathBuf::from("/usr/bin/fdfind"),
        PathBuf::from("/usr/bin/mawk"),
    ];
    candidates
        .into_iter()
        .find(|p| {
            std::fs::read(p)
                .ok()
                .and_then(|b| facts(&b).ok())
                .is_some_and(|f| {
                    f["pt_interp"] == "/lib64/ld-linux-x86-64.so.2"
                        && f["needed"].as_array().is_some_and(|n| {
                            n.iter()
                                .all(|s| allowed.contains(&s.as_str().unwrap_or("")))
                        })
                })
        })
        .expect("a host executable whose closure is libc, libm, libgcc_s and ld.so")
}

struct Fixture {
    root: tempfile::TempDir,
    ledger: Value,
}

fn copy(from: &str, root: &Path, to: &str) {
    let at = inside(root, to);
    std::fs::create_dir_all(at.parent().expect("parent")).expect("dir");
    std::fs::copy(from, &at).expect("copy");
    std::fs::set_permissions(&at, std::fs::Permissions::from_mode(0o755)).expect("mode");
}

fn object(root: &Path, resolved: &str) -> Value {
    let bytes = std::fs::read(inside(root, resolved)).expect("read");
    let f = facts(&bytes).expect("facts");
    json!({"resolved": resolved, "sha256": sha256_hex(&bytes), "build_id": f["build_id"], "facts": f})
}

fn pick(f: &Value, keys: &[&str]) -> Value {
    Value::Object(
        keys.iter()
            .map(|k| ((*k).to_owned(), f[*k].clone()))
            .collect(),
    )
}

impl Fixture {
    fn new() -> Self {
        let root = tempfile::tempdir().expect("root");
        let r = root.path();
        for name in [
            "ld-linux-x86-64.so.2",
            "libc.so.6",
            "libm.so.6",
            "libgcc_s.so.1",
        ] {
            copy(&format!("{LIB}/{name}"), r, &format!("{LIB}/{name}"));
        }
        copy("/etc/ld.so.cache", r, "/etc/ld.so.cache");
        copy(
            worker_source().to_str().expect("utf8"),
            r,
            "/usr/local/libexec/keeper-graph-query",
        );
        std::fs::create_dir_all(inside(r, "/usr/lib64")).expect("lib64");
        symlink("usr/lib64", inside(r, "/lib64")).expect("lib64 link");
        symlink("usr/lib", inside(r, "/lib")).expect("lib link");
        symlink(
            "../lib/x86_64-linux-gnu/ld-linux-x86-64.so.2",
            inside(r, "/usr/lib64/ld-linux-x86-64.so.2"),
        )
        .expect("interp link");
        let interp = object(r, &format!("{LIB}/ld-linux-x86-64.so.2"));
        let closure: Vec<Value> = ["libc.so.6", "libm.so.6", "libgcc_s.so.1"]
            .iter()
            .map(|name| {
                let o = object(r, &format!("{LIB}/{name}"));
                json!({"soname": name, "resolved": o["resolved"], "sha256": o["sha256"],
                       "build_id": o["build_id"], "static": pick(&o["facts"], &CLOSURE_FACTS)})
            })
            .collect();
        let worker = object(r, "/usr/local/libexec/keeper-graph-query");
        let cache = json!({"sha256": sha256_hex(&std::fs::read(inside(r, "/etc/ld.so.cache")).expect("cache"))});
        let ledger = json!({
            "schema": SCHEMA, "class": CLASS, "platform": platform(),
            "build_inputs": {"rust_image": "fixture"},
            "interpreter": {"pt_interp": "/lib64/ld-linux-x86-64.so.2", "resolved": interp["resolved"],
                            "sha256": interp["sha256"], "build_id": interp["build_id"],
                            "static": pick(&interp["facts"], &INTERPRETER_FACTS)},
            "closure": closure,
            "symlinks": {"/lib64": "usr/lib64", "/lib": "usr/lib",
                         "/usr/lib64/ld-linux-x86-64.so.2": "../lib/x86_64-linux-gnu/ld-linux-x86-64.so.2"},
            "ld_so_cache": {"path": "/etc/ld.so.cache", "sha256": cache["sha256"],
                            "entries": {"libc.so.6": "/lib/x86_64-linux-gnu/libc.so.6",
                                        "libm.so.6": "/lib/x86_64-linux-gnu/libm.so.6",
                                        "libgcc_s.so.1": "/lib/x86_64-linux-gnu/libgcc_s.so.1"},
                            "entry_rule": "fixture"},
            "resolution": resolution(),
            "absent": ["/etc/ld.so.preload", "/usr/lib/x86_64-linux-gnu/glibc-hwcaps"],
            "tunables": LINUX_WORKER_GLIBC_TUNABLES,
            "worker": {"path": "/usr/local/libexec/keeper-graph-query", "sha256": worker["sha256"],
                       "profile": "graph-worker", "keeper_rev": "0123456789abcdef0123456789abcdef01234567",
                       "static": pick(&worker["facts"], &WORKER_FACTS)},
            "ownership": {"uid": 0, "gid": 0, "no_write_bits": "group,other", "no_file_capability": true},
            "class_contract": {
                "env": {"GQL_STMT_CACHE": "0", "GLIBC_TUNABLES": "= tunables"},
                "credentials": {"euid_not": 0, "cap_eff": 0, "cap_prm": 0, "cap_amb": 0, "no_new_privs": 1},
                "worker_startup": {"no_new_privs": 1, "AT_SECURE": 0},
                "limits": {"stack_soft_max": MAX_STACK_SOFT, "as_set_by_worker": 536_870_912u64}
            }
        });
        Self { root, ledger }
    }

    fn owner() -> (u32, u32) {
        (
            rustix::process::getuid().as_raw(),
            rustix::process::getgid().as_raw(),
        )
    }

    fn verify(&self, ledger: &Value) -> Result<(), Label> {
        let ledger = Ledger::parse(&ledger.to_string())?;
        ledger
            .verify_tree(self.root.path(), Self::owner())
            .map(drop)
    }

    fn at(&self, path: &str) -> PathBuf {
        inside(self.root.path(), path)
    }
}

/// `runtime_ledger_fixture`: the fixture admits, the recheck admits it
/// again, and each single mutation refuses with its own label.
#[test]
fn runtime_ledger_fixture() {
    let f = Fixture::new();
    let ledger = Ledger::parse(&f.ledger.to_string()).expect("parse");
    let verified = ledger
        .verify_tree(f.root.path(), Fixture::owner())
        .expect("the fixture is in the class");
    assert_eq!(verified.files.len(), 9);

    let mutate = |edit: &dyn Fn(&mut Value)| {
        let mut l = f.ledger.clone();
        edit(&mut l);
        f.verify(&l)
    };
    assert_eq!(
        mutate(&|l| l["closure"][1]["static"]["needed"] = json!(["libz.so.1"])),
        Err("elf.needed")
    );
    assert_eq!(
        mutate(&|l| l["closure"][0]["static"]["pt_tls"] = json!([0, 0])),
        Err("elf.facts")
    );
    assert_eq!(
        mutate(&|l| l["worker"]["static"]["writable_load_page_bytes"] = json!(1)),
        Err("elf.facts")
    );
    assert_eq!(
        mutate(&|l| l["closure"][2]["sha256"] = json!("0".repeat(64))),
        Err("rootfs.sha")
    );
    assert_eq!(
        mutate(&|l| l["ld_so_cache"]["entries"]["libgcc_s.so.1"] =
            json!("/lib/x86_64-linux-gnu/libm.so.6")),
        Err("rootfs.cache_entry")
    );
    assert_eq!(
        mutate(&|l| l["symlinks"]["/lib64"] = json!("usr/lib")),
        Err("rootfs.symlink")
    );
    assert_eq!(
        mutate(&|l| l["tunables"] = json!("glibc.malloc.mmap_max=0")),
        Err("ledger.tunables")
    );
    assert_eq!(
        mutate(&|l| l["schema"] = json!("keeper-graph-runtime-ledger/2")),
        Err("ledger.schema")
    );
    assert_eq!(
        mutate(&|l| l["class"] = json!("host/linux-amd64")),
        Err("ledger.class")
    );
    assert_eq!(
        mutate(&|l| l["class_contract"]["limits"]["stack_soft_max"] = json!(16_777_216)),
        Err("ledger.contract")
    );

    // A DSO owned by another group.
    let (uid, gid) = Fixture::owner();
    assert_eq!(
        ledger
            .verify_tree(f.root.path(), (uid, gid.wrapping_add(1)))
            .err(),
        Some("rootfs.owner")
    );
    assert_eq!(
        ledger
            .verify_tree(f.root.path(), (uid.wrapping_add(1), gid))
            .err(),
        Some("rootfs.owner")
    );
}

/// LOW-2: a cache entry naming another link of the verified inode (here a
/// hard link in a world-writable directory) is refused.
#[test]
fn a_cache_entry_must_be_the_verified_path_itself() {
    let f = Fixture::new();
    let dir = f.at("/opt/w");
    std::fs::create_dir_all(&dir).expect("dir");
    std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o777)).expect("chmod");
    std::fs::hard_link(f.at(&format!("{LIB}/libc.so.6")), dir.join("libc.so.6")).expect("link");
    let mut ledger = f.ledger.clone();
    ledger["ld_so_cache"]["entries"]["libc.so.6"] = json!("/opt/w/libc.so.6");
    assert_eq!(f.verify(&ledger), Err("rootfs.cache_entry"));
}

#[test]
fn a_parent_directory_writable_by_its_group_refuses() {
    let f = Fixture::new();
    std::fs::set_permissions(f.at("/usr/lib"), std::fs::Permissions::from_mode(0o775))
        .expect("chmod");
    assert_eq!(f.verify(&f.ledger), Err("rootfs.owner"));
}

#[test]
fn set_id_and_sticky_bits_on_any_ledger_object_refuse() {
    for (path, mode) in [
        ("/usr/local/libexec/keeper-graph-query", 0o4755),
        ("/usr/local/libexec/keeper-graph-query", 0o2755),
        ("/usr/local/libexec/keeper-graph-query", 0o6755),
        ("/usr/local/libexec/keeper-graph-query", 0o1755),
        ("/usr/lib/x86_64-linux-gnu/libc.so.6", 0o4755),
    ] {
        let f = Fixture::new();
        std::fs::set_permissions(f.at(path), std::fs::Permissions::from_mode(mode)).expect("chmod");
        assert_eq!(f.verify(&f.ledger), Err("rootfs.setid"), "{path} {mode:o}");
    }
}

#[test]
fn a_present_absent_path_and_an_extra_symlink_refuse() {
    let f = Fixture::new();
    std::fs::write(f.at("/etc/ld.so.preload"), b"").expect("preload");
    assert_eq!(f.verify(&f.ledger), Err("rootfs.absent"));

    let f = Fixture::new();
    let worker = f.at("/usr/local/libexec/keeper-graph-query");
    std::fs::rename(&worker, f.at("/usr/local/libexec/real")).expect("move");
    symlink("real", &worker).expect("link");
    assert_eq!(f.verify(&f.ledger), Err("rootfs.symlink"));
}

/// An injected DT_RUNPATH refuses even when the ledger's digest names the
/// mutated bytes.
#[test]
fn an_injected_runpath_refuses() {
    let f = Fixture::new();
    let path = f.at("/usr/lib/x86_64-linux-gnu/libm.so.6");
    let mut bytes = std::fs::read(&path).expect("read");
    // Turn the first DT_NEEDED of its dynamic section into DT_RUNPATH.
    let phoff = u64_at(&bytes, 32).expect("phoff") as usize;
    let phnum = u16_at(&bytes, 56).expect("phnum") as usize;
    let dynamic = (0..phnum)
        .map(|i| phoff + i * 56)
        .find(|at| u32_at(&bytes, *at) == Some(2))
        .map(|at| u64_at(&bytes, at + 8).expect("offset") as usize)
        .expect("PT_DYNAMIC");
    let needed = (0..64)
        .map(|i| dynamic + i * 16)
        .find(|at| u64_at(&bytes, *at) == Some(1))
        .expect("DT_NEEDED");
    bytes[needed..needed + 8].copy_from_slice(&29u64.to_le_bytes());
    std::fs::write(&path, &bytes).expect("write");
    let mut ledger = f.ledger.clone();
    ledger["closure"][1]["sha256"] = json!(sha256_hex(&bytes));
    assert_eq!(f.verify(&ledger), Err("elf.dynamic"));
}

#[test]
fn elf_facts_of_the_host_closure_have_their_known_shape() {
    let libc = facts(&std::fs::read(format!("{LIB}/libc.so.6")).expect("libc")).expect("facts");
    assert_eq!(libc["soname"], "libc.so.6");
    assert_eq!(libc["needed"], json!(["ld-linux-x86-64.so.2"]));
    assert_eq!(libc["bind_now"], true);
    assert!(libc["pt_tls"].is_array());
    assert!(libc["verneed_max"]
        .as_str()
        .is_some_and(|v| v.starts_with("GLIBC_2.")));
    let ld =
        facts(&std::fs::read(format!("{LIB}/ld-linux-x86-64.so.2")).expect("ld")).expect("facts");
    assert_eq!(ld["needed"], json!([]));
    assert_eq!(ld["pt_interp"], Value::Null);
    assert!(ld["writable_load_page_bytes"]
        .as_u64()
        .is_some_and(|w| w > 0 && w % 4096 == 0));
    assert_eq!(facts(b"not an elf"), Err("elf.malformed"));
}

/// `exec_class_refuses` (file half): the owner, mode and capability rule
/// over injected `fstat`/`fgetxattr` outcomes.
#[test]
fn exec_class_refuses_files() {
    use rustix::io::Errno;
    let ok = || Err(Errno::NODATA);
    assert_eq!(file_rule(true, 0, 0, 0o100755, (0, 0), ok()), Ok(()));
    assert_eq!(file_rule(true, 0, 0, 0o100644, (0, 0), ok()), Ok(()));
    for mode in [0o104755, 0o102755, 0o106755, 0o101755] {
        assert_eq!(
            file_rule(true, 0, 0, mode, (0, 0), ok()),
            Err("rootfs.setid"),
            "{mode:o}"
        );
    }
    for mode in [0o100775, 0o100757] {
        assert_eq!(
            file_rule(true, 0, 0, mode, (0, 0), ok()),
            Err("rootfs.owner")
        );
    }
    assert_eq!(
        file_rule(true, 0, 1, 0o100755, (0, 0), ok()),
        Err("rootfs.owner")
    );
    assert_eq!(
        file_rule(false, 0, 0, 0o100755, (0, 0), ok()),
        Err("rootfs.owner")
    );
    for capability in [
        Err(Errno::EXIST),
        Err(Errno::NOTSUP),
        Err(Errno::IO),
        Err(Errno::ACCESS),
        Err(Errno::RANGE),
    ] {
        assert_eq!(
            file_rule(true, 0, 0, 0o100755, (0, 0), capability),
            Err("rootfs.file_capability")
        );
    }
}

fn status(uid: &str, gid: &str, caps: [&str; 3]) -> String {
    format!(
        "Name:\tagentd\nUid:\t{uid}\nGid:\t{gid}\nCapInh:\t0000000000000000\nCapPrm:\t{}\nCapEff:\t{}\nCapBnd:\t00000000a80425fb\nCapAmb:\t{}\nNoNewPrivs:\t1\n",
        caps[0], caps[1], caps[2]
    )
}

/// `exec_class_refuses` (thread half): NNP, the four equal ids, a
/// non-root euid and no permitted, effective or ambient capability.
#[test]
fn exec_class_refuses_threads() {
    let zero = ["0000000000000000"; 3];
    let ids = (1000, 1000, 1000, 1000);
    let good = status("1000\t1000\t1000\t1000", "1000\t1000\t1000\t1000", zero);
    assert_eq!(thread_rule(&good, Some(true), ids), Ok(()));
    assert_eq!(
        thread_rule(&good, Some(false), ids),
        Err("process.no_new_privs")
    );
    assert_eq!(thread_rule(&good, None, ids), Err("process.no_new_privs"));
    for uid in [
        "1001\t1000\t1000\t1000",
        "1000\t1000\t0\t1000",
        "1000\t1000\t1000\t0",
    ] {
        let s = status(uid, "1000\t1000\t1000\t1000", zero);
        assert_eq!(
            thread_rule(&s, Some(true), ids),
            Err("process.credentials"),
            "{uid}"
        );
    }
    for gid in [
        "1001\t1000\t1000\t1000",
        "1000\t1000\t0\t1000",
        "1000\t1000\t1000\t0",
    ] {
        let s = status("1000\t1000\t1000\t1000", gid, zero);
        assert_eq!(
            thread_rule(&s, Some(true), ids),
            Err("process.credentials"),
            "{gid}"
        );
    }
    // The syscalls must agree with the status file.
    assert_eq!(
        thread_rule(&good, Some(true), (1000, 1001, 1000, 1000)),
        Err("process.credentials")
    );
    let root = status("0\t0\t0\t0", "0\t0\t0\t0", zero);
    assert_eq!(
        thread_rule(&root, Some(true), (0, 0, 0, 0)),
        Err("process.caps")
    );
    for (i, cap) in ["CapPrm", "CapEff", "CapAmb"].iter().enumerate() {
        let mut caps = zero;
        caps[i] = "0000000000080000";
        let s = status("1000\t1000\t1000\t1000", "1000\t1000\t1000\t1000", caps);
        assert_eq!(
            thread_rule(&s, Some(true), ids),
            Err("process.caps"),
            "{cap}"
        );
    }
}

/// `platform_prerequisites_refuse`.
#[test]
fn platform_prerequisites_refuse() {
    assert_eq!(platform_rule(4096, 3376), Ok(()));
    assert_eq!(platform_rule(4096, 12_288), Ok(()));
    assert_eq!(platform_rule(16_384, 3376), Err("platform.page_size"));
    assert_eq!(platform_rule(4096, 0), Err("platform.minsigstksz"));
    assert_eq!(platform_rule(4096, 12_289), Err("platform.minsigstksz"));
}

const BASE: &str = "\
22 1 0:21 / / rw,relatime - overlay overlay rw\n\
23 22 0:22 / /proc rw,nosuid - proc proc rw\n\
24 22 0:23 / /dev rw - tmpfs tmpfs rw\n\
25 23 0:24 /null /proc/kcore rw - tmpfs tmpfs rw\n\
26 22 0:25 /a /etc/hosts rw - ext4 /dev/sda rw\n";

/// `process_mounts_procfs`.
#[test]
fn process_mounts_procfs() {
    let named: BTreeSet<String> = [
        "/usr/lib/x86_64-linux-gnu/libc.so.6",
        "/etc/ld.so.cache",
        "/lib64",
        "/usr/local/libexec/keeper-graph-query",
    ]
    .iter()
    .map(|s| (*s).to_owned())
    .collect();
    assert_eq!(mounts_rule(BASE, &named, 77), Ok(()));
    let with = |line: &str| format!("{BASE}{line}\n");
    for line in [
        "30 22 0:30 / /usr/lib rw - ext4 /dev/sdb rw",
        "30 22 0:30 / /lib64 rw - ext4 /dev/sdb rw",
        "30 22 0:30 /c /etc/ld.so.cache ro - ext4 /dev/sdb ro",
        "30 22 0:30 / /usr/local/libexec rw - ext4 /dev/sdb rw",
    ] {
        assert_eq!(
            mounts_rule(&with(line), &named, 77),
            Err("process.mount"),
            "{line}"
        );
    }
    for line in [
        "30 23 0:30 / /proc/self rw - tmpfs tmpfs rw",
        "30 23 0:30 / /proc/thread-self rw - tmpfs tmpfs rw",
        "30 23 0:30 / /proc/77/status rw - tmpfs tmpfs rw",
        "30 22 0:30 / /proc rw - tmpfs tmpfs rw",
    ] {
        assert_eq!(
            mounts_rule(&with(line), &named, 77),
            Err("process.procfs"),
            "{line}"
        );
    }
    assert_eq!(
        mounts_rule(&BASE.replace("- proc proc", "- tmpfs tmpfs"), &named, 77),
        Err("process.procfs")
    );
    // A bind on /etc/localtime is admitted, with its name escaped.
    assert_eq!(
        mounts_rule(
            &with("30 22 0:30 /z /etc/local\\040time ro - ext4 /dev/sdb ro"),
            &named,
            77
        ),
        Ok(())
    );
    // An escaped mount point is unescaped before the rule.
    let spaced: BTreeSet<String> = ["/opt/a b/lib.so".to_owned()].into();
    assert_eq!(
        mounts_rule(
            &with("30 22 0:30 / /opt/a\\040b rw - ext4 /dev/sdb rw"),
            &spaced,
            77
        ),
        Err("process.mount")
    );
    assert_eq!(mounts_rule("garbage\n", &named, 77), Err("process.mount"));
}
