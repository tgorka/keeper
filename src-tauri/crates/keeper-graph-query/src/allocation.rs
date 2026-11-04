//! Audited worker-only allocation domain. See docs/constraints-and-limitations.md.
use std::alloc::{alloc, dealloc, Layout};
use std::cell::Cell;
use std::ffi::{c_char, c_int, c_void, CStr};
use std::sync::atomic::{AtomicBool, Ordering};

pub const BUDGET: usize = 256 * 1024 * 1024;
#[global_allocator]
static ALLOCATOR: cap::Cap<std::alloc::System> = cap::Cap::new(std::alloc::System, BUDGET);
thread_local! { static MAIN: Cell<bool> = const { Cell::new(false) }; }
static INSTALLED: AtomicBool = AtomicBool::new(false);
static ENTERED: AtomicBool = AtomicBool::new(false);

/// Grants allocation-callback permission to the calling thread, once per process.
/// Any second claim (repeated, or from another thread, concurrently or later)
/// terminates with 73, so at most one thread can ever hold the permission.
/// Callbacks and helpers never grant it.
pub fn enter() {
    if ENTERED.swap(true, Ordering::SeqCst) {
        std::process::exit(73);
    }
    MAIN.with(|flag| flag.set(true));
}
pub(crate) fn require_main() {
    if !MAIN.try_with(Cell::get).unwrap_or(false) {
        std::process::exit(73);
    }
}
pub fn allocated() -> usize {
    ALLOCATOR.allocated()
}
pub fn peak_allocated() -> usize {
    ALLOCATOR.max_allocated()
}
fn limit() -> ! {
    std::process::exit(72)
}
fn positive_payload(size: usize) -> usize {
    size.max(1)
        .checked_add(15)
        .map(|n| n & !15)
        .unwrap_or_else(|| limit())
}
#[repr(C, align(16))]
#[derive(Clone, Copy)]
struct Header {
    total: usize,
    payload: usize,
}
const _: () = assert!(std::mem::size_of::<Header>() == 16);

// U02 SAFETY: SQLite 3.53.2 sqlite3_config copies sqlite3_mem_methods. Called
// exactly once on marked main, before any SQLite API; callbacks live forever,
// pAppData is null, varargs use the pinned header's exact types. Non-OK exits.
#[allow(unsafe_code)]
pub fn install_sqlite() {
    require_main();
    if INSTALLED.swap(true, Ordering::SeqCst) {
        std::process::exit(73);
    }
    let methods = libsqlite3_sys::sqlite3_mem_methods {
        xMalloc: Some(sqlite_malloc),
        xFree: Some(native_free),
        xRealloc: Some(sqlite_realloc),
        xSize: Some(sqlite_size),
        xRoundup: Some(sqlite_roundup),
        xInit: Some(sqlite_init),
        xShutdown: Some(sqlite_shutdown),
        pAppData: std::ptr::null_mut(),
    };
    // SAFETY: exact callback ABI and pre-initialization ordering above.
    let codes = unsafe {
        [
            libsqlite3_sys::sqlite3_config(libsqlite3_sys::SQLITE_CONFIG_MALLOC, &methods),
            libsqlite3_sys::sqlite3_config(libsqlite3_sys::SQLITE_CONFIG_MMAP_SIZE, 0_i64, 0_i64),
            libsqlite3_sys::sqlite3_config(
                libsqlite3_sys::SQLITE_CONFIG_STMTJRNL_SPILL,
                -1 as c_int,
            ),
        ]
    };
    if codes.iter().any(|rc| *rc != libsqlite3_sys::SQLITE_OK) {
        std::process::exit(74);
    }
}

// U03 SAFETY: C malloc ABI, no borrowed input. A positive checked Layout covers
// header and payload at 16-byte alignment on the two supported 64-bit targets.
// All bytes pass through the global Cap; ownership transfers to U04/U06.
#[allow(unsafe_code)]
#[unsafe(export_name = "keeper_graph_malloc")]
unsafe extern "C" fn native_malloc(size: usize) -> *mut c_void {
    require_main();
    let payload = positive_payload(size);
    let total = payload.checked_add(16).unwrap_or_else(|| limit());
    let layout = Layout::from_size_align(total, 16).unwrap_or_else(|_| limit());
    // SAFETY: positive valid layout; null is handled before any pointer use.
    let base = unsafe { alloc(layout) };
    if base.is_null() {
        limit();
    }
    // SAFETY: freshly allocated aligned block has header plus payload bytes.
    unsafe {
        base.cast::<Header>().write(Header { total, payload });
        base.add(16).cast()
    }
}

// U04 SAFETY: free accepts null or one live pointer from this allocation domain,
// never a foreign/stale pointer. Header is ownership metadata, not authentication.
// Thread check precedes pointer access, and exact original Layout is restored.
#[allow(unsafe_code)]
#[unsafe(export_name = "keeper_graph_free")]
unsafe extern "C" fn native_free(pointer: *mut c_void) {
    require_main();
    if pointer.is_null() {
        return;
    }
    // SAFETY: the caller transfers a live owned allocation with our 16-byte header.
    unsafe {
        let base = pointer.cast::<u8>().sub(16);
        let header = base.cast::<Header>().read();
        let layout = Layout::from_size_align(header.total, 16).unwrap_or_else(|_| limit());
        dealloc(base, layout);
    }
}

// U05 SAFETY: calloc ABI; checked product and positive allocation from U03.
// Every returned payload byte is valid for zeroing, including the zero-size case.
#[allow(unsafe_code)]
#[unsafe(export_name = "keeper_graph_calloc")]
unsafe extern "C" fn native_calloc(count: usize, size: usize) -> *mut c_void {
    require_main();
    let size = count.checked_mul(size).unwrap_or_else(|| limit());
    // SAFETY: U03 returns a live non-null block of positive_payload(size) bytes.
    unsafe {
        let p = native_malloc(size);
        p.cast::<u8>().write_bytes(0, positive_payload(size));
        p
    }
}

// U06 SAFETY: realloc ABI, null or live owned pointer. A separate allocation
// stays charged alongside the old block until bounded nonoverlapping copy ends.
// Zero size also returns a positive non-null allocation; no upstream OOM branch.
#[allow(unsafe_code)]
#[unsafe(export_name = "keeper_graph_realloc")]
unsafe extern "C" fn native_realloc(pointer: *mut c_void, size: usize) -> *mut c_void {
    require_main();
    // SAFETY: source ownership contract above, U03 creates a disjoint allocation.
    unsafe {
        let replacement = native_malloc(size);
        if !pointer.is_null() {
            let header = pointer.cast::<u8>().sub(16).cast::<Header>().read();
            std::ptr::copy_nonoverlapping(
                pointer.cast::<u8>(),
                replacement.cast::<u8>(),
                header.payload.min(positive_payload(size)),
            );
            native_free(pointer);
        }
        replacement
    }
}

// U07 SAFETY: pinned C callers supply a valid NUL-terminated string alive for
// the scan/copy. Returned bytes include NUL and are freed only by U04/U06.
#[allow(unsafe_code)]
#[unsafe(export_name = "keeper_graph_strdup")]
unsafe extern "C" fn native_strdup(source: *const c_char) -> *mut c_char {
    require_main();
    if source.is_null() {
        std::process::exit(73);
    }
    // SAFETY: valid C string and disjoint correctly-sized destination.
    unsafe {
        let bytes = CStr::from_ptr(source).to_bytes_with_nul();
        let p = native_malloc(bytes.len()).cast::<c_char>();
        std::ptr::copy_nonoverlapping(source, p, bytes.len());
        p
    }
}

// U08 SAFETY: source is readable until first NUL or maximum bytes (whichever
// comes first). Bison's strndup call follows this contract. No over-read at NUL.
#[allow(unsafe_code)]
#[unsafe(export_name = "keeper_graph_strndup")]
unsafe extern "C" fn native_strndup(source: *const c_char, maximum: usize) -> *mut c_char {
    require_main();
    if source.is_null() {
        std::process::exit(73);
    }
    // SAFETY: caller's readable extent, checked size, disjoint owned destination.
    unsafe {
        let mut len = 0;
        while len < maximum && source.add(len).read() != 0 {
            len += 1;
        }
        let size = len.checked_add(1).unwrap_or_else(|| limit());
        let p = native_malloc(size).cast::<c_char>();
        std::ptr::copy_nonoverlapping(source, p, len);
        p.add(len).write(0);
        p
    }
}

// U09 SAFETY: sqlite3_mem_methods.xMalloc ABI from libsqlite3-sys 0.38.2;
// convert nonnegative int, then transfer the U03 allocation to SQLite.
#[allow(unsafe_code)]
unsafe extern "C" fn sqlite_malloc(size: c_int) -> *mut c_void {
    require_main();
    let size = usize::try_from(size).unwrap_or_else(|_| std::process::exit(73));
    // SAFETY: positive checked allocation, no borrowed native memory.
    unsafe { native_malloc(size) }
}

// U10 SAFETY: SQLite owns null or a live xMalloc/xRealloc block. Exact C ABI,
// nonnegative size, U06 preserves ownership and never returns NULL.
#[allow(unsafe_code)]
unsafe extern "C" fn sqlite_realloc(pointer: *mut c_void, size: c_int) -> *mut c_void {
    require_main();
    let size = usize::try_from(size).unwrap_or_else(|_| std::process::exit(73));
    // SAFETY: pinned SQLite callback ownership contract.
    unsafe { native_realloc(pointer, size) }
}

// U11 SAFETY: SQLite supplies a non-null live block from this domain. Return
// the charged payload capacity, checked to fit the exact xSize int return ABI.
#[allow(unsafe_code)]
unsafe extern "C" fn sqlite_size(pointer: *mut c_void) -> c_int {
    require_main();
    if pointer.is_null() {
        std::process::exit(73);
    }
    // SAFETY: live allocation includes aligned readable header.
    let size = unsafe { pointer.cast::<u8>().sub(16).cast::<Header>().read().payload };
    c_int::try_from(size).unwrap_or_else(|_| std::process::exit(73))
}
extern "C" fn sqlite_roundup(size: c_int) -> c_int {
    require_main();
    let size = usize::try_from(size).unwrap_or_else(|_| std::process::exit(73));
    c_int::try_from(positive_payload(size)).unwrap_or_else(|_| limit())
}
extern "C" fn sqlite_init(app_data: *mut c_void) -> c_int {
    require_main();
    if !app_data.is_null() {
        std::process::exit(73);
    }
    libsqlite3_sys::SQLITE_OK
}
extern "C" fn sqlite_shutdown(app_data: *mut c_void) {
    require_main();
    if !app_data.is_null() {
        std::process::exit(73);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const CASE: &str = "KG_ALLOCATION_TEST_CASE";

    /// Runs `body` in a fresh copy of this test binary (each case needs a process
    /// whose one-shot entry is still unclaimed) and returns its exit code.
    fn isolated(test: &str, case: &str, body: impl FnOnce()) -> Option<i32> {
        if std::env::var(CASE).as_deref() == Ok(case) {
            body();
            std::process::exit(0);
        }
        std::process::Command::new(std::env::current_exe().expect("test binary"))
            .args([test, "--exact", "--nocapture", "--test-threads=1"])
            .env(CASE, case)
            .status()
            .expect("child")
            .code()
    }

    #[test]
    fn safe_sqlite_callbacks_refuse_any_thread_but_the_entered_one() {
        let test = "allocation::tests::safe_sqlite_callbacks_refuse_any_thread_but_the_entered_one";
        type Callback = fn();
        let callbacks: [(&str, Callback); 3] = [
            ("roundup", || {
                sqlite_roundup(16);
            }),
            ("init", || {
                sqlite_init(std::ptr::null_mut());
            }),
            ("shutdown", || sqlite_shutdown(std::ptr::null_mut())),
        ];
        for (name, callback) in callbacks {
            let code = isolated(test, name, || {
                enter();
                let _ = std::thread::spawn(callback).join();
            });
            assert_eq!(code, Some(73), "{name} ran on a thread without permission");
        }
    }

    #[test]
    fn entry_is_one_shot_across_threads() {
        let test = "allocation::tests::entry_is_one_shot_across_threads";
        assert_eq!(
            isolated(test, "repeat", || {
                enter();
                enter();
            }),
            Some(73)
        );
        assert_eq!(
            isolated(test, "other-thread", || {
                enter();
                let _ = std::thread::spawn(enter).join();
            }),
            Some(73)
        );
    }

    #[test]
    fn entered_thread_gets_positive_sixteen_byte_roundup_and_null_app_data_only() {
        let test = "allocation::tests::entered_thread_gets_positive_sixteen_byte_roundup_and_null_app_data_only";
        let ok = isolated(test, "main", || {
            enter();
            assert_eq!(sqlite_roundup(0), 16);
            assert_eq!(sqlite_roundup(1), 16);
            assert_eq!(sqlite_roundup(17), 32);
            assert_eq!(sqlite_init(std::ptr::null_mut()), libsqlite3_sys::SQLITE_OK);
            sqlite_shutdown(std::ptr::null_mut());
        });
        assert_eq!(ok, Some(0));
        let mut marker = 0u8;
        let foreign = isolated(test, "app-data", move || {
            enter();
            sqlite_init(std::ptr::addr_of_mut!(marker).cast());
        });
        assert_eq!(foreign, Some(73));
    }
}
