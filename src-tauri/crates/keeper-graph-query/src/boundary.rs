//! P6 (story 110.1, R1 design v7): the input-boundary admission guard B0.
//!
//! The worker opens its own `/proc/self/status` after inherited descriptors are
//! closed and before confinement, and checks it immediately after confinement, before
//! any generation content or input frame is read: one fixed 4 KiB `pread`, exactly one
//! valid `VmData:` line, checked KiB conversion, the descriptor closed. `VmData` above
//! `H0` (or anything malformed or unavailable) refuses the run (exit 74) with nothing
//! read, written or published. B0 admits this run only; it is no retroactive bound on
//! the pre-boundary interval, which the design bounds by class. `VmPeak` is recorded
//! beside it as an observation, never as an allocation oracle.
use std::sync::atomic::{AtomicU64, Ordering};

/// H0: the reserve for everything live at the boundary, Cap-charged or not.
pub const H0: u64 = 1024 * 1024;
#[cfg(target_os = "linux")]
const STATUS_BYTES: usize = 4096;

static VM_DATA: AtomicU64 = AtomicU64::new(0);
static VM_PEAK: AtomicU64 = AtomicU64::new(0);

/// `VmData` and `VmPeak` in bytes as admitted at B0 (0 before B0, or off Linux).
pub fn recorded() -> (u64, u64) {
    (
        VM_DATA.load(Ordering::Relaxed),
        VM_PEAK.load(Ordering::Relaxed),
    )
}

#[derive(Debug, PartialEq, Eq)]
pub enum Refusal {
    Unreadable,
    Malformed,
    Missing,
    Duplicate,
    Overflow,
    OverReserve(u64),
}

/// One `Field:\t<digits> kB` value in bytes, from complete lines of `status`. A line cut
/// by the fixed read size is ignored, so a field must appear whole to count.
fn field(status: &[u8], name: &[u8]) -> Result<Option<u64>, Refusal> {
    let complete = match status.iter().rposition(|&b| b == b'\n') {
        Some(end) => &status[..=end],
        None => return Ok(None),
    };
    let mut found = None;
    for line in complete.split(|&b| b == b'\n') {
        let Some(rest) = line.strip_prefix(name) else {
            continue;
        };
        let Some(rest) = rest.strip_prefix(b":") else {
            continue;
        };
        if found.is_some() {
            return Err(Refusal::Duplicate);
        }
        let value = rest.trim_ascii();
        let digits = value.strip_suffix(b" kB").ok_or(Refusal::Malformed)?;
        if digits.is_empty() || !digits.iter().all(u8::is_ascii_digit) {
            return Err(Refusal::Malformed);
        }
        let mut kib: u64 = 0;
        for &d in digits {
            kib = kib
                .checked_mul(10)
                .and_then(|v| v.checked_add(u64::from(d - b'0')))
                .ok_or(Refusal::Overflow)?;
        }
        found = Some(kib.checked_mul(1024).ok_or(Refusal::Overflow)?);
    }
    Ok(found)
}

/// The admission decision over the bytes of one `status` read.
pub fn admit_status(status: &[u8]) -> Result<(u64, Option<u64>), Refusal> {
    let data = field(status, b"VmData")?.ok_or(Refusal::Missing)?;
    if data > H0 {
        return Err(Refusal::OverReserve(data));
    }
    // VmPeak is informational: a malformed one does not change admission.
    Ok((data, field(status, b"VmPeak").ok().flatten()))
}

/// The owned descriptor, opened before confinement.
#[cfg(target_os = "linux")]
pub struct Status(std::fs::File);

#[cfg(target_os = "linux")]
impl Status {
    pub fn open() -> std::io::Result<Self> {
        std::fs::File::open("/proc/self/status").map(Self)
    }
    /// B0: consumes and closes the descriptor.
    pub fn admit(self) -> Result<(), Refusal> {
        use std::os::unix::fs::FileExt;
        let mut buffer = [0u8; STATUS_BYTES];
        let read = self
            .0
            .read_at(&mut buffer, 0)
            .map_err(|_| Refusal::Unreadable)?;
        drop(self);
        let (data, peak) = admit_status(&buffer[..read])?;
        VM_DATA.store(data, Ordering::Relaxed);
        VM_PEAK.store(peak.unwrap_or(0), Ordering::Relaxed);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn status(lines: &str) -> Vec<u8> {
        format!("Name:\tkeeper-graph-query\nVmPeak:\t  8536 kB\n{lines}Threads:\t1\n").into_bytes()
    }

    #[test]
    fn admits_exactly_at_the_reserve_and_refuses_one_kib_above() {
        assert_eq!(
            admit_status(&status("VmData:\t    1024 kB\n")),
            Ok((1024 * 1024, Some(8536 * 1024)))
        );
        assert_eq!(
            admit_status(&status("VmData:\t    1025 kB\n")),
            Err(Refusal::OverReserve(1025 * 1024))
        );
    }

    #[test]
    fn refuses_missing_duplicate_malformed_and_overflowing_values() {
        assert_eq!(admit_status(&status("")), Err(Refusal::Missing));
        assert_eq!(
            admit_status(&status("VmData:\t 10 kB\nVmData:\t 10 kB\n")),
            Err(Refusal::Duplicate)
        );
        for bad in [
            "VmData:\t\n",
            "VmData:\t 10\n",
            "VmData:\t -1 kB\n",
            "VmData:\t 1x kB\n",
            "VmData:\t 10 MB\n",
        ] {
            assert_eq!(
                admit_status(&status(bad)),
                Err(Refusal::Malformed),
                "{bad:?}"
            );
        }
        assert_eq!(
            admit_status(&status("VmData:\t 18446744073709551615 kB\n")),
            Err(Refusal::Overflow)
        );
        assert_eq!(
            admit_status(&status("VmData:\t 99999999999999999999999 kB\n")),
            Err(Refusal::Overflow)
        );
    }

    #[test]
    fn ignores_a_line_cut_by_the_fixed_read_and_similar_field_names() {
        // A VmData line without its newline (cut by the 4 KiB read) does not count.
        assert_eq!(
            admit_status(b"Name:\tx\nVmData:\t 10 kB"),
            Err(Refusal::Missing)
        );
        // Only the exact field name counts.
        assert_eq!(
            admit_status(&status("VmDataX:\t 5000 kB\nVmData:\t 12 kB\n")),
            Ok((12 * 1024, Some(8536 * 1024)))
        );
    }
}
