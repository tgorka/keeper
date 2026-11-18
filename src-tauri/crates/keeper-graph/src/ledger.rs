//! The runtime DSO ledger (OD-A delta r6 §4, Q3/Q6/Q19): the one Linux
//! runtime class R1 admits, `agent-image/linux-amd64/trixie/r1`, as the
//! reviewed ledger keeper-hq embeds into the binary that spawns workers.
//!
//! [`Ledger::verify_rootfs`] checks a tree against it (§4.4 steps 1–4):
//! every ledger path resolved in-root component by component through
//! exactly the ledger's symlinks, opened without following, a root-owned
//! regular file with no group/other write bit, no set-id or sticky bit and
//! no `security.capability`, every parent directory root-owned with no
//! group/other write bit, `/` included, and its SHA-256; each closure
//! object's `ld.so.cache` entry reaching the same inode; every `absent`
//! path absent; the ELF facts of every object, read from its verified
//! bytes, equal to the ledger's. [`Ledger::verify_process`] checks the
//! calling process (steps 6 and 7: mounts, procfs, platform) and
//! [`spawning_thread`] the thread about to spawn (step 5: `no_new_privs`,
//! equal real/effective/saved/filesystem ids, a non-root euid and no
//! capability, ambient ones read from the thread's own `CapAmb`, which
//! `capget` does not return). A refusal is one fixed label, never file
//! content.
//!
//! Before every spawn [`Verified::recheck`] re-resolves every path by its
//! current name and requires the identity the full verification saw
//! (device, inode, size, mtime, ctime) with the owner, mode and capability
//! rules on the re-opened files, the absences, mounts, procfs and platform
//! again; the caller runs [`spawning_thread`] last, with nothing between it
//! and the spawn.

use std::collections::{BTreeMap, BTreeSet};
use std::fs::File;
use std::io::Read;
use std::os::unix::fs::{MetadataExt, OpenOptionsExt};
use std::path::{Path, PathBuf};

use serde::Deserialize;
use serde_json::{json, Map, Value};
use sha2::{Digest, Sha256};

use super::protocol::worker::LINUX_WORKER_GLIBC_TUNABLES;

/// The ledger schema this build reads.
pub const SCHEMA: &str = "keeper-graph-runtime-ledger/3";
/// The one runtime class R1 admits.
pub const CLASS: &str = "agent-image/linux-amd64/trixie/r1";
/// The soft `RLIMIT_STACK` the class admits at most (P6c).
pub const MAX_STACK_SOFT: u64 = 8 * 1024 * 1024;

/// Why the runtime is outside the class: one fixed label.
pub type Label = &'static str;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Object {
    #[serde(default)]
    soname: Option<String>,
    #[serde(default)]
    pt_interp: Option<String>,
    resolved: String,
    sha256: String,
    build_id: String,
    #[serde(rename = "static")]
    facts: Map<String, Value>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Cache {
    path: String,
    sha256: String,
    entries: BTreeMap<String, String>,
    #[allow(dead_code)]
    entry_rule: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct WorkerEntry {
    path: String,
    sha256: String,
    profile: String,
    keeper_rev: String,
    #[serde(rename = "static")]
    facts: Map<String, Value>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Document {
    schema: String,
    class: String,
    platform: Value,
    #[allow(dead_code)]
    build_inputs: Map<String, Value>,
    interpreter: Object,
    closure: Vec<Object>,
    symlinks: BTreeMap<String, String>,
    ld_so_cache: Cache,
    resolution: Value,
    absent: Vec<String>,
    tunables: String,
    worker: WorkerEntry,
    ownership: Value,
    class_contract: Value,
}

/// A parsed ledger.
pub struct Ledger(Document);

/// The fields of a ledger whose values the code enforces: a ledger stating
/// anything else describes a class this build does not implement.
fn platform() -> Value {
    json!({
        "os": "linux", "arch": "x86_64",
        "auxv": {"AT_PAGESZ": {"eq": 4096}, "AT_MINSIGSTKSZ": {"min": 1, "max": 12288}},
        "host": {"landlock_abi_min": 6, "seccomp_filter": true}
    })
}

fn resolution() -> Value {
    json!([
        {"from": "worker", "needed": "libm.so.6", "via": "cache", "object": "closure[1]"},
        {"from": "worker", "needed": "libgcc_s.so.1", "via": "cache", "object": "closure[2]"},
        {"from": "worker", "needed": "libc.so.6", "via": "cache", "object": "closure[0]"},
        {"from": "worker", "needed": "ld-linux-x86-64.so.2", "via": "loaded-soname", "object": "interpreter"},
        {"from": "closure[*]", "needed": "every DT_NEEDED", "via": "loaded-soname", "object": "the ledger object with that soname"}
    ])
}

const CLOSURE: [&str; 3] = ["libc.so.6", "libm.so.6", "libgcc_s.so.1"];
const INTERPRETER_FACTS: [&str; 7] = [
    "soname",
    "needed",
    "pt_interp",
    "pt_tls",
    "writable_load_page_bytes",
    "bind_now",
    "verneed_max",
];
const CLOSURE_FACTS: [&str; 6] = [
    "needed",
    "pt_interp",
    "pt_tls",
    "writable_load_page_bytes",
    "bind_now",
    "verneed_max",
];
const WORKER_FACTS: [&str; 4] = [
    "needed",
    "pt_tls",
    "writable_load_page_bytes",
    "verneed_max",
];

fn hex64(s: &str) -> bool {
    super::protocol::worker::is_hex_digest(s)
}

fn absolute(path: &str) -> bool {
    path.starts_with('/')
        && !path.ends_with('/')
        && path[1..]
            .split('/')
            .all(|c| !c.is_empty() && c != "." && c != "..")
}

fn keys(map: &Map<String, Value>, wanted: &[&str]) -> bool {
    map.len() == wanted.len() && wanted.iter().all(|k| map.contains_key(*k))
}

impl Ledger {
    /// Parse a ledger and refuse one this build does not implement: another
    /// schema or class, enforced fields other than the code's, a closure
    /// other than libc, libm and libgcc_s, or a malformed entry.
    pub fn parse(text: &str) -> Result<Self, Label> {
        let d: Document = serde_json::from_str(text).map_err(|_| "ledger.unreadable")?;
        if d.schema != SCHEMA {
            return Err("ledger.schema");
        }
        if d.class != CLASS {
            return Err("ledger.class");
        }
        if d.platform != platform() || d.resolution != resolution() {
            return Err("ledger.platform");
        }
        if d.tunables != LINUX_WORKER_GLIBC_TUNABLES {
            return Err("ledger.tunables");
        }
        let own = &d.ownership;
        let contract = &d.class_contract;
        if own["uid"] != 0
            || own["gid"] != 0
            || own["no_file_capability"] != true
            || contract["env"] != json!({"GQL_STMT_CACHE": "0", "GLIBC_TUNABLES": "= tunables"})
            || contract["credentials"]["euid_not"] != 0
            || contract["credentials"]["cap_eff"] != 0
            || contract["credentials"]["cap_prm"] != 0
            || contract["credentials"]["cap_amb"] != 0
            || contract["credentials"]["no_new_privs"] != 1
            || contract["worker_startup"]["no_new_privs"] != 1
            || contract["worker_startup"]["AT_SECURE"] != 0
            || contract["limits"]["stack_soft_max"] != MAX_STACK_SOFT
            || contract["limits"]["as_set_by_worker"] != 512 * 1024 * 1024
        {
            return Err("ledger.contract");
        }
        let sonames: Vec<_> = d.closure.iter().map(|o| o.soname.as_deref()).collect();
        if sonames != CLOSURE.map(Some).to_vec()
            || d.interpreter.soname.is_some()
            || d.interpreter.pt_interp.is_none()
            || d.closure.iter().any(|o| o.pt_interp.is_some())
            || !keys(&d.interpreter.facts, &INTERPRETER_FACTS)
            || d.closure.iter().any(|o| !keys(&o.facts, &CLOSURE_FACTS))
            || !keys(&d.worker.facts, &WORKER_FACTS)
            || d.ld_so_cache
                .entries
                .keys()
                .map(String::as_str)
                .collect::<Vec<_>>()
                != {
                    let mut s = CLOSURE.to_vec();
                    s.sort_unstable();
                    s
                }
        {
            return Err("ledger.objects");
        }
        let objects = std::iter::once(&d.interpreter).chain(&d.closure);
        for o in objects {
            if !absolute(&o.resolved)
                || !hex64(&o.sha256)
                || o.build_id.is_empty()
                || !o.build_id.bytes().all(|b| b.is_ascii_hexdigit())
            {
                return Err("ledger.objects");
            }
        }
        let paths = d
            .interpreter
            .pt_interp
            .iter()
            .chain(d.ld_so_cache.entries.values())
            .chain([&d.ld_so_cache.path, &d.worker.path])
            .chain(d.symlinks.keys())
            .chain(&d.absent);
        if paths.into_iter().any(|p| !absolute(p))
            || !hex64(&d.ld_so_cache.sha256)
            || !hex64(&d.worker.sha256)
            || d.worker.profile != "graph-worker"
            || d.worker.keeper_rev.len() != 40
            || !d
                .worker
                .keeper_rev
                .bytes()
                .all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
            || !d.absent.iter().any(|p| p == "/etc/ld.so.preload")
        {
            return Err("ledger.objects");
        }
        Ok(Self(d))
    }

    /// The worker the ledger admits: its path and digest.
    #[must_use]
    pub fn worker(&self) -> (&str, &str) {
        (&self.0.worker.path, &self.0.worker.sha256)
    }

    /// Every path the ledger names, for the mount rule.
    fn named(&self) -> BTreeSet<String> {
        let d = &self.0;
        std::iter::once(&d.interpreter)
            .chain(&d.closure)
            .map(|o| &o.resolved)
            .chain(d.interpreter.pt_interp.iter())
            .chain(d.ld_so_cache.entries.values())
            .chain([&d.ld_so_cache.path, &d.worker.path])
            .chain(d.symlinks.keys())
            .chain(&d.absent)
            .cloned()
            .collect()
    }

    /// §4.4 steps 1–4 on the tree at `root` (`/` in production), with
    /// ownership from `fstat` always.
    pub fn verify_rootfs(&self, root: &Path) -> Result<Verified, Label> {
        self.verify_tree(root, (0, 0))
    }

    fn verify_tree(&self, root: &Path, owner: (u32, u32)) -> Result<Verified, Label> {
        if !cfg!(all(target_os = "linux", target_arch = "x86_64")) {
            return Err("platform.target");
        }
        let d = &self.0;
        let mut files = Vec::new();
        let mut parents = BTreeSet::new();
        let mut check = |named: &str, resolved: &str, sha: &str| -> Result<Vec<u8>, Label> {
            let at = resolve(root, named, &d.symlinks)?;
            if at != resolved {
                return Err("rootfs.path");
            }
            let (file, bytes) = open_checked(root, &at, owner, true)?;
            if sha256_hex(&bytes) != sha {
                return Err("rootfs.sha");
            }
            parents.extend(ancestors(&at));
            files.push(Checked {
                named: named.to_owned(),
                resolved: at,
                identity: file,
            });
            Ok(bytes)
        };
        let interp_at = d.interpreter.pt_interp.as_deref().ok_or("ledger.objects")?;
        let interp = check(interp_at, &d.interpreter.resolved, &d.interpreter.sha256)?;
        let mut closure = Vec::new();
        for o in &d.closure {
            closure.push(check(&o.resolved, &o.resolved, &o.sha256)?);
        }
        let cache = &d.ld_so_cache;
        check(&cache.path, &cache.path, &cache.sha256)?;
        let worker = check(&d.worker.path, &d.worker.path, &d.worker.sha256)?;
        // Each closure object's cache value path reaches the same inode.
        for o in &d.closure {
            let soname = o.soname.as_deref().ok_or("ledger.objects")?;
            let entry = cache.entries.get(soname).ok_or("rootfs.cache_entry")?;
            let at = resolve(root, entry, &d.symlinks).map_err(|_| "rootfs.cache_entry")?;
            // The path ld.so opens must be the verified object's own path
            // (whose every parent is held to the ownership rule), not merely
            // another name of its inode.
            if at != o.resolved {
                return Err("rootfs.cache_entry");
            }
            let held = files
                .iter()
                .find(|f| f.resolved == o.resolved)
                .ok_or("rootfs.cache_entry")?;
            let meta =
                std::fs::symlink_metadata(inside(root, &at)).map_err(|_| "rootfs.cache_entry")?;
            if (meta.dev(), meta.ino()) != (held.identity.dev, held.identity.ino) {
                return Err("rootfs.cache_entry");
            }
            files.push(Checked {
                named: entry.clone(),
                resolved: at,
                identity: held.identity,
            });
        }
        for parent in &parents {
            parent_ok(root, parent, owner)?;
        }
        absent(root, &d.absent)?;
        // Step 4: ELF facts from the verified bytes.
        let interp_facts = facts(&interp)?;
        expect(&d.interpreter, &interp_facts, &INTERPRETER_FACTS)?;
        let mut sonames: BTreeSet<String> = CLOSURE.iter().map(|s| (*s).to_owned()).collect();
        if let Some(Value::String(own)) = interp_facts.get("soname") {
            sonames.insert(own.clone());
        }
        for (o, bytes) in d.closure.iter().zip(&closure) {
            let f = facts(bytes)?;
            expect(o, &f, &CLOSURE_FACTS)?;
            if f.get("soname").and_then(Value::as_str) != o.soname.as_deref()
                || !needed_within(&f, &sonames)
            {
                return Err("elf.needed");
            }
        }
        let wf = facts(&worker)?;
        for key in WORKER_FACTS {
            if wf.get(key) != d.worker.facts.get(key) {
                return Err(if key == "needed" {
                    "elf.needed"
                } else {
                    "elf.facts"
                });
            }
        }
        if wf.get("pt_interp").and_then(Value::as_str) != d.interpreter.pt_interp.as_deref() {
            return Err("elf.interp");
        }
        if !needed_within(&wf, &sonames) {
            return Err("elf.needed");
        }
        Ok(Verified {
            root: root.to_owned(),
            owner,
            files,
            parents,
            named: self.named(),
            symlinks: d.symlinks.clone(),
            absent: d.absent.clone(),
        })
    }

    /// §4.4 steps 6 and 7 on the calling process.
    pub fn verify_process(&self) -> Result<(), Label> {
        process(&self.named())
    }
}

fn needed_within(f: &Map<String, Value>, sonames: &BTreeSet<String>) -> bool {
    f.get("needed").and_then(Value::as_array).is_some_and(|n| {
        n.iter()
            .all(|s| s.as_str().is_some_and(|s| sonames.contains(s)))
    })
}

fn expect(o: &Object, f: &Map<String, Value>, wanted: &[&str]) -> Result<(), Label> {
    if o.build_id
        != f.get("build_id")
            .and_then(Value::as_str)
            .unwrap_or_default()
    {
        return Err("elf.build_id");
    }
    for key in wanted {
        if f.get(*key) != o.facts.get(*key) {
            return Err(if *key == "needed" {
                "elf.needed"
            } else {
                "elf.facts"
            });
        }
    }
    Ok(())
}

/// One checked file's identity.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Identity {
    dev: u64,
    ino: u64,
    size: u64,
    mtime: (i64, i64),
    ctime: (i64, i64),
}

struct Checked {
    named: String,
    resolved: String,
    identity: Identity,
}

/// A tree that passed the full verification, with what the per-spawn
/// recheck compares.
pub struct Verified {
    root: PathBuf,
    owner: (u32, u32),
    files: Vec<Checked>,
    parents: BTreeSet<String>,
    named: BTreeSet<String>,
    symlinks: BTreeMap<String, String>,
    absent: Vec<String>,
}

impl Verified {
    /// The per-spawn recheck (§4.5): every path re-resolved by its current
    /// name to the same resolved path and identity, the owner, mode and
    /// capability rules on the re-opened files and parents, the absences,
    /// then the process's mounts, procfs and platform.
    pub fn recheck(&self) -> Result<(), Label> {
        for f in &self.files {
            let at = resolve(&self.root, &f.named, &self.symlinks)?;
            if at != f.resolved {
                return Err("rootfs.path");
            }
            let (identity, _) = open_checked(&self.root, &at, self.owner, false)?;
            if identity != f.identity {
                return Err("rootfs.identity");
            }
        }
        for parent in &self.parents {
            parent_ok(&self.root, parent, self.owner)?;
        }
        absent(&self.root, &self.absent)?;
        process(&self.named)
    }
}

fn inside(root: &Path, path: &str) -> PathBuf {
    root.join(path.trim_start_matches('/'))
}

fn ancestors(path: &str) -> Vec<String> {
    let mut out = vec!["/".to_owned()];
    let mut at = String::new();
    let parts: Vec<_> = path[1..].split('/').collect();
    for part in &parts[..parts.len().saturating_sub(1)] {
        at.push('/');
        at.push_str(part);
        out.push(at.clone());
    }
    out
}

/// Resolve `path` inside `root`, component by component, following only
/// the ledger's symlinks with exactly their targets.
fn resolve(root: &Path, path: &str, symlinks: &BTreeMap<String, String>) -> Result<String, Label> {
    if !absolute(path) {
        return Err("rootfs.path");
    }
    let mut pending: Vec<String> = path[1..].split('/').rev().map(str::to_owned).collect();
    let mut current: Vec<String> = Vec::new();
    let mut hops = 0;
    while let Some(part) = pending.pop() {
        match part.as_str() {
            "" | "." => continue,
            ".." => {
                current.pop();
                continue;
            }
            _ => {}
        }
        let candidate = format!(
            "/{}",
            current
                .iter()
                .chain([&part])
                .cloned()
                .collect::<Vec<_>>()
                .join("/")
        );
        let meta =
            std::fs::symlink_metadata(inside(root, &candidate)).map_err(|_| "rootfs.path")?;
        if meta.file_type().is_symlink() {
            hops += 1;
            let target = std::fs::read_link(inside(root, &candidate)).map_err(|_| "rootfs.path")?;
            let target = target.to_str().ok_or("rootfs.symlink")?;
            if hops > 40 || symlinks.get(&candidate).map(String::as_str) != Some(target) {
                return Err("rootfs.symlink");
            }
            if target.starts_with('/') {
                current.clear();
            }
            pending.extend(target.split('/').rev().map(str::to_owned));
        } else {
            if !pending.is_empty() && !meta.is_dir() {
                return Err("rootfs.path");
            }
            current.push(part);
        }
    }
    Ok(format!("/{}", current.join("/")))
}

/// The file rule (§4.4 step 2.3) over one `fstat` and one
/// `security.capability` lookup.
fn file_rule(
    regular: bool,
    uid: u32,
    gid: u32,
    mode: u32,
    owner: (u32, u32),
    capability: Result<(), rustix::io::Errno>,
) -> Result<(), Label> {
    if !regular || (uid, gid) != owner || mode & 0o022 != 0 {
        return Err("rootfs.owner");
    }
    if mode & 0o7000 != 0 {
        return Err("rootfs.setid");
    }
    match capability {
        Err(rustix::io::Errno::NODATA) => Ok(()),
        _ => Err("rootfs.file_capability"),
    }
}

fn capability(file: &File) -> Result<(), rustix::io::Errno> {
    let mut value = [0u8; 64];
    match rustix::fs::fgetxattr(file, "security.capability", &mut value[..]) {
        Ok(_) => Err(rustix::io::Errno::EXIST),
        Err(e) => Err(e),
    }
}

fn open_checked(
    root: &Path,
    at: &str,
    owner: (u32, u32),
    read: bool,
) -> Result<(Identity, Vec<u8>), Label> {
    let mut file = std::fs::OpenOptions::new()
        .read(true)
        .custom_flags(rustix::fs::OFlags::NOFOLLOW.bits() as i32)
        .open(inside(root, at))
        .map_err(|_| "rootfs.path")?;
    let meta = file.metadata().map_err(|_| "rootfs.path")?;
    file_rule(
        meta.file_type().is_file(),
        meta.uid(),
        meta.gid(),
        meta.mode(),
        owner,
        capability(&file),
    )?;
    let identity = Identity {
        dev: meta.dev(),
        ino: meta.ino(),
        size: meta.size(),
        mtime: (meta.mtime(), meta.mtime_nsec()),
        ctime: (meta.ctime(), meta.ctime_nsec()),
    };
    let mut bytes = Vec::new();
    if read {
        file.read_to_end(&mut bytes).map_err(|_| "rootfs.path")?;
    }
    Ok((identity, bytes))
}

fn parent_ok(root: &Path, parent: &str, owner: (u32, u32)) -> Result<(), Label> {
    let meta = std::fs::symlink_metadata(inside(root, parent)).map_err(|_| "rootfs.path")?;
    if !meta.is_dir() || (meta.uid(), meta.gid()) != owner || meta.mode() & 0o022 != 0 {
        return Err("rootfs.owner");
    }
    Ok(())
}

fn absent(root: &Path, paths: &[String]) -> Result<(), Label> {
    for path in paths {
        match std::fs::symlink_metadata(inside(root, path)) {
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            _ => return Err("rootfs.absent"),
        }
    }
    Ok(())
}

fn sha256_hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

// ---- ELF facts (step 4): read from verified bytes, never by executing.

fn u16_at(b: &[u8], at: usize) -> Option<u64> {
    Some(u64::from(u16::from_le_bytes(
        b.get(at..at + 2)?.try_into().ok()?,
    )))
}
fn u32_at(b: &[u8], at: usize) -> Option<u64> {
    Some(u64::from(u32::from_le_bytes(
        b.get(at..at + 4)?.try_into().ok()?,
    )))
}
fn u64_at(b: &[u8], at: usize) -> Option<u64> {
    Some(u64::from_le_bytes(b.get(at..at + 8)?.try_into().ok()?))
}
fn cstr(b: &[u8], at: usize) -> Option<String> {
    let rest = b.get(at..)?;
    let end = rest.iter().position(|c| *c == 0)?;
    String::from_utf8(rest[..end].to_vec()).ok()
}

/// The frozen facts of one x86-64 ELF object, as the ledger states them:
/// `soname`, `needed` (DT_NEEDED order), `pt_interp`, `pt_tls`
/// (`[memsz, align]`), `writable_load_page_bytes` (each writable PT_LOAD's
/// `memsz + (vaddr mod 4096)` rounded up to pages), `bind_now`,
/// `verneed_max` (the highest `GLIBC_x.y` needed) and `build_id`. Any
/// DT_RPATH, DT_RUNPATH, DT_AUDIT or DT_DEPAUDIT refuses.
pub(crate) fn facts(b: &[u8]) -> Result<Map<String, Value>, Label> {
    elf(b).ok_or("elf.malformed")?
}

fn elf(b: &[u8]) -> Option<Result<Map<String, Value>, Label>> {
    if b.get(..4)? != b"\x7fELF" || *b.get(4)? != 2 || *b.get(5)? != 1 || u16_at(b, 18)? != 62 {
        return None;
    }
    let (phoff, phentsize, phnum) = (u64_at(b, 32)?, u16_at(b, 54)?, u16_at(b, 56)?);
    let mut loads = Vec::new();
    let (mut interp, mut tls, mut dynamic, mut wbytes, mut build_id) =
        (None, None, None, 0u64, None);
    for i in 0..phnum {
        let at = usize::try_from(phoff + i * phentsize).ok()?;
        let (kind, flags) = (u32_at(b, at)?, u32_at(b, at + 4)?);
        let (offset, vaddr, filesz, memsz, align) = (
            u64_at(b, at + 8)?,
            u64_at(b, at + 16)?,
            u64_at(b, at + 32)?,
            u64_at(b, at + 40)?,
            u64_at(b, at + 48)?,
        );
        match kind {
            1 => {
                loads.push((vaddr, offset, filesz));
                if flags & 2 != 0 {
                    wbytes += (memsz + (vaddr & 4095) + 4095) & !4095;
                }
            }
            2 => dynamic = Some((offset, filesz)),
            3 => interp = Some(cstr(b, usize::try_from(offset).ok()?)?),
            4 => {
                // PT_NOTE: NT_GNU_BUILD_ID (3) named "GNU".
                let (mut n, end) = (
                    usize::try_from(offset).ok()?,
                    usize::try_from(offset + filesz).ok()?,
                );
                while n + 12 <= end {
                    let (namesz, descsz, ty) = (
                        u32_at(b, n)? as usize,
                        u32_at(b, n + 4)? as usize,
                        u32_at(b, n + 8)?,
                    );
                    let name = n + 12;
                    let desc = name + ((namesz + 3) & !3);
                    if ty == 3 && b.get(name..name + namesz)? == b"GNU\0" {
                        build_id = Some(
                            b.get(desc..desc + descsz)?
                                .iter()
                                .map(|x| format!("{x:02x}"))
                                .collect::<String>(),
                        );
                    }
                    n = desc + ((descsz + 3) & !3);
                }
            }
            7 => tls = Some(json!([memsz, align])),
            _ => {}
        }
    }
    let offset_of = |vaddr: u64| {
        loads
            .iter()
            .find(|(v, _, f)| vaddr >= *v && vaddr < v + f)
            .map(|(v, o, _)| o + (vaddr - v))
    };
    let (mut needed_at, mut soname_at, mut strtab, mut verneed, mut verneednum) =
        (Vec::new(), None, None, None, 0);
    let mut bind_now = false;
    if let Some((offset, size)) = dynamic {
        let mut at = usize::try_from(offset).ok()?;
        let end = at + usize::try_from(size).ok()?;
        while at + 16 <= end {
            let (tag, val) = (u64_at(b, at)?, u64_at(b, at + 8)?);
            match tag {
                0 => break,
                1 => needed_at.push(val),
                5 => strtab = Some(val),
                14 => soname_at = Some(val),
                15 | 29 | 0x6fff_fefc | 0x6fff_fefb => return Some(Err("elf.dynamic")),
                24 => bind_now = true,
                30 if val & 8 != 0 => bind_now = true,
                0x6fff_fffb if val & 1 != 0 => bind_now = true,
                0x6fff_fffe => verneed = Some(val),
                0x6fff_ffff => verneednum = val,
                _ => {}
            }
            at += 16;
        }
    }
    let strtab = match strtab {
        Some(v) => Some(usize::try_from(offset_of(v)?).ok()?),
        None => None,
    };
    let name = |off: u64| -> Option<String> { cstr(b, strtab? + usize::try_from(off).ok()?) };
    let mut needed = Vec::new();
    for off in needed_at {
        needed.push(Value::String(name(off)?));
    }
    let soname = match soname_at {
        Some(off) => Value::String(name(off)?),
        None => Value::Null,
    };
    let mut best: Option<Vec<u64>> = None;
    if let Some(v) = verneed {
        let mut at = usize::try_from(offset_of(v)?).ok()?;
        for _ in 0..verneednum {
            let (cnt, aux, next) = (u16_at(b, at + 2)?, u32_at(b, at + 8)?, u32_at(b, at + 12)?);
            let mut a = at + usize::try_from(aux).ok()?;
            for _ in 0..cnt {
                let (vname, vnext) = (u32_at(b, a + 8)?, u32_at(b, a + 12)?);
                if let Some(version) = name(vname)?.strip_prefix("GLIBC_") {
                    let parts: Option<Vec<u64>> =
                        version.split('.').map(|p| p.parse().ok()).collect();
                    if let Some(parts) = parts.filter(|p| p.len() >= 2) {
                        if best.as_ref().is_none_or(|b| parts > *b) {
                            best = Some(parts);
                        }
                    }
                }
                if vnext == 0 {
                    break;
                }
                a += usize::try_from(vnext).ok()?;
            }
            if next == 0 {
                break;
            }
            at += usize::try_from(next).ok()?;
        }
    }
    let verneed_max = best.map_or(Value::Null, |v| {
        Value::String(format!(
            "GLIBC_{}",
            v.iter().map(u64::to_string).collect::<Vec<_>>().join(".")
        ))
    });
    let mut out = Map::new();
    out.insert("soname".into(), soname);
    out.insert("needed".into(), Value::Array(needed));
    out.insert(
        "pt_interp".into(),
        interp.map_or(Value::Null, Value::String),
    );
    out.insert("pt_tls".into(), tls.unwrap_or(Value::Null));
    out.insert("writable_load_page_bytes".into(), json!(wbytes));
    out.insert("bind_now".into(), Value::Bool(bind_now));
    out.insert("verneed_max".into(), verneed_max);
    out.insert(
        "build_id".into(),
        build_id.map_or(Value::Null, Value::String),
    );
    Some(Ok(out))
}

// ---- The process (steps 5–7).

/// One `/proc/self/mountinfo` line's mount point and filesystem type,
/// octal escapes undone; `None` for a line that does not parse.
fn mount_line(line: &str) -> Option<(String, String)> {
    let fields: Vec<&str> = line.split(' ').collect();
    let dash = fields.iter().position(|f| *f == "-")?;
    if dash < 6 {
        return None;
    }
    let point = unescape(fields.get(4)?)?;
    Some((point, (*fields.get(dash + 1)?).to_owned()))
}

fn unescape(s: &str) -> Option<String> {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'\\' {
            let code = std::str::from_utf8(b.get(i + 1..i + 4)?).ok()?;
            out.push(u8::from_str_radix(code, 8).ok()?);
            i += 4;
        } else {
            out.push(b[i]);
            i += 1;
        }
    }
    String::from_utf8(out).ok()
}

/// Step 6 over a mountinfo text: no mount at or above a ledger path (`/`
/// excepted), exactly one `proc` mount at `/proc`, nothing mounted at or
/// below `/proc/self`, `/proc/thread-self` or `/proc/<pid>`.
pub(crate) fn mounts_rule(
    mountinfo: &str,
    named: &BTreeSet<String>,
    pid: u32,
) -> Result<(), Label> {
    let own = format!("/proc/{pid}");
    let guarded = ["/proc/self", "/proc/thread-self", own.as_str()];
    let mut procs = 0;
    for line in mountinfo.lines() {
        let (point, fstype) = mount_line(line).ok_or("process.mount")?;
        if point == "/proc" {
            if fstype != "proc" {
                return Err("process.procfs");
            }
            procs += 1;
            continue;
        }
        if guarded
            .iter()
            .any(|g| point == *g || point.starts_with(&format!("{g}/")))
        {
            return Err("process.procfs");
        }
        if point != "/"
            && named
                .iter()
                .any(|p| *p == point || p.starts_with(&format!("{point}/")))
        {
            return Err("process.mount");
        }
    }
    if procs != 1 {
        return Err("process.procfs");
    }
    Ok(())
}

fn status_field<'a>(status: &'a str, name: &str) -> Option<&'a str> {
    status
        .lines()
        .find_map(|l| l.strip_prefix(name).and_then(|r| r.strip_prefix(':')))
        .map(str::trim)
}

/// Step 5 over the spawning thread's `status`, its `no_new_privs` flag and
/// its `getuid`/`geteuid`/`getgid`/`getegid`.
pub(crate) fn thread_rule(
    status: &str,
    no_new_privs: Option<bool>,
    ids: (u32, u32, u32, u32),
) -> Result<(), Label> {
    if no_new_privs != Some(true) {
        return Err("process.no_new_privs");
    }
    let four = |name: &str| -> Option<Vec<u32>> {
        let v: Option<Vec<u32>> = status_field(status, name)?
            .split_whitespace()
            .map(|x| x.parse().ok())
            .collect();
        v.filter(|v| v.len() == 4)
    };
    let (Some(uid), Some(gid)) = (four("Uid"), four("Gid")) else {
        return Err("process.credentials");
    };
    let (ruid, euid, rgid, egid) = ids;
    if uid.iter().any(|u| *u != uid[0])
        || gid.iter().any(|g| *g != gid[0])
        || (uid[0], uid[1], gid[0], gid[1]) != (ruid, euid, rgid, egid)
    {
        return Err("process.credentials");
    }
    if euid == 0 {
        return Err("process.caps");
    }
    for cap in ["CapPrm", "CapEff", "CapAmb"] {
        match status_field(status, cap).map(|v| u64::from_str_radix(v, 16)) {
            Some(Ok(0)) => {}
            _ => return Err("process.caps"),
        }
    }
    Ok(())
}

/// Step 7's auxv half over the process's page size and signal-frame size.
pub(crate) fn platform_rule(page: usize, minsigstksz: usize) -> Result<(), Label> {
    if page != 4096 {
        return Err("platform.page_size");
    }
    if !(1..=12_288).contains(&minsigstksz) {
        return Err("platform.minsigstksz");
    }
    Ok(())
}

fn process(named: &BTreeSet<String>) -> Result<(), Label> {
    let pid = std::process::id();
    let mountinfo =
        std::fs::read_to_string("/proc/self/mountinfo").map_err(|_| "process.procfs")?;
    mounts_rule(&mountinfo, named, pid)?;
    let link = std::fs::read_link("/proc/self").map_err(|_| "process.procfs")?;
    let status = std::fs::read_to_string("/proc/self/status").map_err(|_| "process.procfs")?;
    if link.to_str() != Some(pid.to_string().as_str())
        || status_field(&status, "Pid") != Some(pid.to_string().as_str())
    {
        return Err("process.procfs");
    }
    platform_rule(
        rustix::param::page_size(),
        rustix::param::linux_minsigstksz(),
    )?;
    landlock_ok()?;
    if status_field(&status, "Seccomp").is_none()
        || status_field(&status, "Seccomp_filters").is_none()
    {
        return Err("platform.seccomp");
    }
    Ok(())
}

fn landlock_ok() -> Result<(), Label> {
    use landlock::{
        Access, AccessFs, AccessNet, CompatLevel, Compatible, Ruleset, RulesetAttr, Scope, ABI,
    };
    let abi = ABI::V6;
    Ruleset::default()
        .set_compatibility(CompatLevel::HardRequirement)
        .handle_access(AccessFs::from_all(abi))
        .and_then(|r| r.handle_access(AccessNet::from_all(abi)))
        .and_then(|r| r.scope(Scope::from_all(abi)))
        .and_then(|r| r.create())
        .map(drop)
        .map_err(|_| "platform.landlock")
}

/// Step 5 on the calling thread: run it last before the spawn, on the
/// thread that spawns, with nothing awaited between.
pub fn spawning_thread() -> Result<(), Label> {
    let status =
        std::fs::read_to_string("/proc/thread-self/status").map_err(|_| "process.procfs")?;
    thread_rule(
        &status,
        rustix::thread::no_new_privs().ok(),
        (
            rustix::process::getuid().as_raw(),
            rustix::process::geteuid().as_raw(),
            rustix::process::getgid().as_raw(),
            rustix::process::getegid().as_raw(),
        ),
    )
}

#[cfg(test)]
#[path = "ledger_tests.rs"]
mod tests;
