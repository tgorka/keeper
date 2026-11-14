//! The worker's execution-class and platform self-check (OD-A delta r6 §4.4
//! step 8 and B5): the first thing `engine::start` does after the
//! environment check, before inherited descriptors are closed, any limit is
//! set, confinement is entered or anything of a generation is opened.
//!
//! It corroborates the exec that happened; the supervisor's own checks
//! before the spawn are the admission. From the process's auxiliary
//! vector: exactly one each of `AT_UID`, `AT_EUID`, `AT_GID`, `AT_EGID` and
//! `AT_SECURE` before `AT_NULL` (else `auxv`), `AT_SECURE` 0 (else
//! `at_secure`), `AT_UID == AT_EUID` (else `uid`), `AT_GID == AT_EGID` (else
//! `gid`); then the inherited `no_new_privs` flag set (else `no_new_privs`):
//! it survives every `execve`, so seeing it here proves the exec ran under
//! it, which Landlock's later status cannot. Then the platform: `AT_PAGESZ`
//! 4096 and `AT_MINSIGSTKSZ` in 1..=12288. A refusal writes its fixed label
//! to stdout and stderr and exits 74; it allocates nothing.
//!
//! The vector is read from `/proc/self/auxv` into a fixed stack buffer
//! (the delta's C4 row 5b), with raw `open`/`read`.

use keeper_graph_protocol::worker::{EXECUTION_CLASS_REFUSED, EXIT_REFUSED, PLATFORM_REFUSED};

const AT_NULL: u64 = 0;
const AT_PAGESZ: u64 = 6;
const AT_UID: u64 = 11;
const AT_EUID: u64 = 12;
const AT_GID: u64 = 13;
const AT_EGID: u64 = 14;
const AT_SECURE: u64 = 23;
const AT_MINSIGSTKSZ: u64 = 51;

/// The only page size of the admitted class.
pub const PAGE_SIZE: u64 = 4096;
/// The largest signal-frame size of the admitted class.
pub const MAX_MINSIGSTKSZ: u64 = 12_288;
/// The largest auxiliary vector the check reads.
const AUXV_BYTES: usize = 4096;

/// Why the check refused.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Refused {
    /// The execution class, with its label.
    Class(&'static str),
    /// The page size or signal-stack size.
    Platform,
}

/// The decision over one auxiliary vector (native-endian `(type, value)`
/// word pairs) and the `no_new_privs` flag (`None` when it could not be
/// read).
pub fn decide(auxv: &[u8], no_new_privs: Option<bool>) -> Result<(), Refused> {
    const WORD: usize = std::mem::size_of::<u64>();
    let mut seen = [0u8; 5];
    let mut values = [0u64; 5];
    let (mut page, mut sigstack) = (None, None);
    let mut terminated = false;
    for pair in auxv.chunks(2 * WORD) {
        let (Some(kind), Some(value)) = (word(pair, 0), word(pair, WORD)) else {
            return Err(Refused::Class("auxv"));
        };
        if kind == AT_NULL {
            terminated = true;
            break;
        }
        let slot = match kind {
            AT_UID => Some(0),
            AT_EUID => Some(1),
            AT_GID => Some(2),
            AT_EGID => Some(3),
            AT_SECURE => Some(4),
            AT_PAGESZ => {
                page = Some(value);
                None
            }
            AT_MINSIGSTKSZ => {
                sigstack = Some(value);
                None
            }
            _ => None,
        };
        if let Some(slot) = slot {
            seen[slot] += 1;
            values[slot] = value;
        }
    }
    if !terminated || seen.iter().any(|count| *count != 1) {
        return Err(Refused::Class("auxv"));
    }
    let [uid, euid, gid, egid, secure] = values;
    if secure != 0 {
        return Err(Refused::Class("at_secure"));
    }
    if uid != euid {
        return Err(Refused::Class("uid"));
    }
    if gid != egid {
        return Err(Refused::Class("gid"));
    }
    if no_new_privs != Some(true) {
        return Err(Refused::Class("no_new_privs"));
    }
    if page != Some(PAGE_SIZE) || !sigstack.is_some_and(|s| (1..=MAX_MINSIGSTKSZ).contains(&s)) {
        return Err(Refused::Platform);
    }
    Ok(())
}

fn word(pair: &[u8], at: usize) -> Option<u64> {
    let bytes: [u8; 8] = pair.get(at..at + 8)?.try_into().ok()?;
    Some(u64::from_ne_bytes(bytes))
}

/// Read this process's auxiliary vector into `buffer`; its length, or
/// `None` when it cannot be read whole.
fn read_auxv(buffer: &mut [u8; AUXV_BYTES]) -> Option<usize> {
    use rustix::fs::{open, Mode, OFlags};
    let fd = open(
        c"/proc/self/auxv",
        OFlags::RDONLY | OFlags::CLOEXEC | OFlags::NOFOLLOW,
        Mode::empty(),
    )
    .ok()?;
    let mut len = 0;
    loop {
        let rest = buffer.get_mut(len..)?;
        if rest.is_empty() {
            // A vector this large is outside the class; never truncated.
            return None;
        }
        match rustix::io::read(&fd, rest) {
            Ok(0) => return Some(len),
            Ok(n) => len += n,
            Err(rustix::io::Errno::INTR) => continue,
            Err(_) => return None,
        }
    }
}

/// Run the check on this process; on refusal write the fixed label and
/// exit 74.
pub fn check() {
    let mut buffer = [0u8; AUXV_BYTES];
    let decision = match read_auxv(&mut buffer) {
        Some(len) => decide(&buffer[..len], rustix::thread::no_new_privs().ok()),
        None => Err(Refused::Class("auxv")),
    };
    match decision {
        Ok(()) => {}
        Err(Refused::Class(label)) => refuse(&[EXECUTION_CLASS_REFUSED, label, "\n"]),
        Err(Refused::Platform) => refuse(&[PLATFORM_REFUSED, "\n"]),
    }
}

fn refuse(parts: &[&str]) -> ! {
    for fd in [rustix::stdio::stdout(), rustix::stdio::stderr()] {
        for part in parts {
            let _ = rustix::io::write(fd, part.as_bytes());
        }
    }
    std::process::exit(EXIT_REFUSED)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn auxv(entries: &[(u64, u64)], terminated: bool) -> Vec<u8> {
        let mut out = Vec::new();
        for (kind, value) in entries {
            out.extend_from_slice(&kind.to_ne_bytes());
            out.extend_from_slice(&value.to_ne_bytes());
        }
        if terminated {
            out.extend_from_slice(&[0u8; 16]);
        }
        out
    }

    fn good() -> Vec<(u64, u64)> {
        vec![
            (AT_PAGESZ, 4096),
            (AT_MINSIGSTKSZ, 3376),
            (AT_UID, 1000),
            (AT_EUID, 1000),
            (AT_GID, 1000),
            (AT_EGID, 1000),
            (AT_SECURE, 0),
        ]
    }

    fn with(kind: u64, value: u64) -> Vec<u8> {
        let entries: Vec<_> = good()
            .into_iter()
            .map(|(k, v)| if k == kind { (k, value) } else { (k, v) })
            .collect();
        auxv(&entries, true)
    }

    #[test]
    fn the_admitted_class_passes() {
        assert_eq!(decide(&auxv(&good(), true), Some(true)), Ok(()));
        assert_eq!(decide(&with(AT_MINSIGSTKSZ, 12_288), Some(true)), Ok(()));
    }

    /// `worker_exec_class_self_check` (delta §5.2): each injected deviation
    /// gives its own label, the class before the platform and secure mode
    /// before the missing flag.
    #[test]
    fn worker_exec_class_self_check() {
        assert_eq!(
            decide(&with(AT_SECURE, 1), Some(false)),
            Err(Refused::Class("at_secure"))
        );
        assert_eq!(
            decide(&with(AT_EUID, 0), Some(true)),
            Err(Refused::Class("uid"))
        );
        assert_eq!(
            decide(&with(AT_EGID, 1001), Some(true)),
            Err(Refused::Class("gid"))
        );
        assert_eq!(
            decide(&auxv(&good(), true), Some(false)),
            Err(Refused::Class("no_new_privs"))
        );
        assert_eq!(
            decide(&auxv(&good(), true), None),
            Err(Refused::Class("no_new_privs"))
        );
        let missing: Vec<_> = good().into_iter().filter(|(k, _)| *k != AT_EGID).collect();
        assert_eq!(
            decide(&auxv(&missing, true), Some(true)),
            Err(Refused::Class("auxv"))
        );
        let mut twice = good();
        twice.push((AT_SECURE, 0));
        assert_eq!(
            decide(&auxv(&twice, true), Some(true)),
            Err(Refused::Class("auxv"))
        );
        assert_eq!(
            decide(&auxv(&good(), false), Some(true)),
            Err(Refused::Class("auxv"))
        );
        let mut torn = auxv(&good(), true);
        torn.truncate(torn.len() - 3);
        assert_eq!(decide(&torn, Some(true)), Err(Refused::Class("auxv")));
    }

    /// `worker_platform_self_check` (delta §5.2).
    #[test]
    fn worker_platform_self_check() {
        assert_eq!(
            decide(&with(AT_PAGESZ, 16_384), Some(true)),
            Err(Refused::Platform)
        );
        for sigstack in [0, 12_289] {
            assert_eq!(
                decide(&with(AT_MINSIGSTKSZ, sigstack), Some(true)),
                Err(Refused::Platform)
            );
        }
        let absent: Vec<_> = good()
            .into_iter()
            .filter(|(k, _)| *k != AT_MINSIGSTKSZ)
            .collect();
        assert_eq!(
            decide(&auxv(&absent, true), Some(true)),
            Err(Refused::Platform)
        );
    }

    /// The reader takes this process's real vector whole.
    #[test]
    fn this_process_vector_reads_whole() {
        let mut buffer = [0u8; AUXV_BYTES];
        let len = read_auxv(&mut buffer).expect("auxv");
        assert_eq!(len % 16, 0);
        let nnp = rustix::thread::no_new_privs().expect("prctl");
        let decision = decide(&buffer[..len], Some(true));
        assert!(
            decision.is_ok() || decision == Err(Refused::Platform),
            "this test process is an ordinary exec: {decision:?} (nnp {nnp})"
        );
    }
}
