//! Separate proof executable for the audited allocation hooks (inventory test row T12).
//! Never shipped; built only with the `proof-executables` feature. It links the same
//! worker library and allocation domain, installs the SQLite hooks in the production
//! order, then exercises one fixed case per process. The parent records the exit code:
//! 0 with a JSON report for the positive case, 73 for refusals off the entered thread
//! or of invalid arguments, 72 for budget/overflow refusals.
#[cfg(any(target_os = "linux", target_os = "macos"))]
fn main() {
    let name = std::env::args().nth(1).unwrap_or_default();
    let Some(case) = Case::parse(&name) else {
        std::process::exit(2);
    };
    keeper_graph_query::proof::enter();
    keeper_graph_query::allocation::install_sqlite();
    match exercise_allocator_case(case) {
        Ok(report) => {
            use std::io::Write;
            let _ = writeln!(std::io::stdout(), "{report}");
        }
        Err(failure) => {
            use std::io::Write;
            let _ = writeln!(std::io::stderr(), "{failure}");
            std::process::exit(1);
        }
    }
}
#[cfg(not(any(target_os = "linux", target_os = "macos")))]
fn main() {
    std::process::exit(74);
}

#[derive(Clone, Copy)]
enum Case {
    Positive,
    WrongThread(Family),
    MallocOverflow,
    CallocOverflow,
    ReallocOverflow,
    SqliteNegativeMalloc,
    SqliteNegativeRoundup,
}
#[derive(Clone, Copy)]
enum Family {
    Malloc,
    MallocZero,
    FreeNull,
    FreeLive,
    Calloc,
    ReallocNull,
    ReallocLive,
    Strdup,
    Strndup,
    SqliteMalloc,
    SqliteFree,
    SqliteRealloc,
    SqliteSize,
    SqliteRoundup,
    SqliteInit,
    SqliteShutdown,
}
const FAMILIES: [(&str, Family); 16] = [
    ("malloc", Family::Malloc),
    ("malloc-zero", Family::MallocZero),
    ("free-null", Family::FreeNull),
    ("free-live", Family::FreeLive),
    ("calloc", Family::Calloc),
    ("realloc-null", Family::ReallocNull),
    ("realloc-live", Family::ReallocLive),
    ("strdup", Family::Strdup),
    ("strndup", Family::Strndup),
    ("sqlite-malloc", Family::SqliteMalloc),
    ("sqlite-free", Family::SqliteFree),
    ("sqlite-realloc", Family::SqliteRealloc),
    ("sqlite-size", Family::SqliteSize),
    ("sqlite-roundup", Family::SqliteRoundup),
    ("sqlite-init", Family::SqliteInit),
    ("sqlite-shutdown", Family::SqliteShutdown),
];
impl Case {
    fn parse(name: &str) -> Option<Self> {
        Some(match name {
            "positive" => Self::Positive,
            "malloc-overflow" => Self::MallocOverflow,
            "calloc-overflow" => Self::CallocOverflow,
            "realloc-overflow" => Self::ReallocOverflow,
            "sqlite-negative-malloc" => Self::SqliteNegativeMalloc,
            "sqlite-negative-roundup" => Self::SqliteNegativeRoundup,
            other => Self::WrongThread(
                FAMILIES
                    .iter()
                    .find(|(n, _)| Some(*n) == other.strip_prefix("wrong-thread-"))?
                    .1,
            ),
        })
    }
}

// T12 SAFETY (test-only, never shipped): calls the six existing C exports and the
// callbacks SQLite itself received, obtained with SQLITE_CONFIG_GETMALLOC after the
// checked installation and before any SQLite initialization, on the entered thread.
// Every pointer passed is one this function obtained from the same domain and still
// owns (no stale, foreign or double-freed pointer is manufactured); C strings are
// NUL-terminated literals; each block is freed exactly once. Cross-thread cases move
// only a fixture address, and the callee's thread check runs before any access.
#[allow(unsafe_code)]
fn exercise_allocator_case(case: Case) -> Result<serde_json::Value, String> {
    use std::ffi::{c_char, c_int, c_void, CStr};
    extern "C" {
        fn keeper_graph_malloc(size: usize) -> *mut c_void;
        fn keeper_graph_free(pointer: *mut c_void);
        fn keeper_graph_calloc(count: usize, size: usize) -> *mut c_void;
        fn keeper_graph_realloc(pointer: *mut c_void, size: usize) -> *mut c_void;
        fn keeper_graph_strdup(source: *const c_char) -> *mut c_char;
        fn keeper_graph_strndup(source: *const c_char, maximum: usize) -> *mut c_char;
    }
    let mut methods = libsqlite3_sys::sqlite3_mem_methods {
        xMalloc: None,
        xFree: None,
        xRealloc: None,
        xSize: None,
        xRoundup: None,
        xInit: None,
        xShutdown: None,
        pAppData: std::ptr::null_mut(),
    };
    // SAFETY: GETMALLOC copies the installed table into `methods`; SQLite is configured
    // but not initialized, so configuration calls are permitted.
    let rc = unsafe {
        libsqlite3_sys::sqlite3_config(
            libsqlite3_sys::SQLITE_CONFIG_GETMALLOC,
            &mut methods as *mut libsqlite3_sys::sqlite3_mem_methods,
        )
    };
    if rc != libsqlite3_sys::SQLITE_OK {
        return Err(format!("GETMALLOC refused: {rc}"));
    }
    let x_malloc = methods.xMalloc.ok_or("xMalloc missing")?;
    let x_free = methods.xFree.ok_or("xFree missing")?;
    let x_realloc = methods.xRealloc.ok_or("xRealloc missing")?;
    let x_size = methods.xSize.ok_or("xSize missing")?;
    let x_roundup = methods.xRoundup.ok_or("xRoundup missing")?;
    let x_init = methods.xInit.ok_or("xInit missing")?;
    let x_shutdown = methods.xShutdown.ok_or("xShutdown missing")?;
    if !methods.pAppData.is_null() {
        return Err("installed pAppData is not null".into());
    }
    let aligned = |p: *mut c_void| !p.is_null() && (p as usize).is_multiple_of(16);
    match case {
        Case::Positive => unsafe {
            let mut report = serde_json::Map::new();
            let mut check = |name: &str, ok: bool| {
                report.insert(name.into(), serde_json::Value::Bool(ok));
            };
            // Two simultaneously live zero-size blocks must be distinct, non-null, aligned.
            let zero = keeper_graph_malloc(0);
            let zero_second = keeper_graph_malloc(0);
            check(
                "malloc_zero_unique_aligned",
                aligned(zero) && aligned(zero_second) && zero != zero_second,
            );
            let calloc_a = keeper_graph_calloc(0, 7);
            let calloc_b = keeper_graph_calloc(7, 0);
            check(
                "calloc_zero_factors",
                aligned(calloc_a) && aligned(calloc_b) && calloc_a != calloc_b,
            );
            let realloc_null_zero = keeper_graph_realloc(std::ptr::null_mut(), 0);
            check("realloc_null_zero", aligned(realloc_null_zero));
            let live = keeper_graph_malloc(32);
            let realloc_live_zero = keeper_graph_realloc(live, 0);
            check("realloc_live_zero", aligned(realloc_live_zero));
            let zeroed = keeper_graph_calloc(64, 3) as *mut u8;
            check(
                "calloc_zeroed",
                aligned(zeroed.cast()) && (0..192).all(|i| *zeroed.add(i) == 0),
            );
            let grow = keeper_graph_malloc(100) as *mut u8;
            for i in 0..100 {
                *grow.add(i) = i as u8;
            }
            let grown = keeper_graph_realloc(grow.cast(), 5000) as *mut u8;
            let grow_ok = aligned(grown.cast()) && (0..100).all(|i| *grown.add(i) == i as u8);
            let shrunk = keeper_graph_realloc(grown.cast(), 10) as *mut u8;
            check(
                "realloc_grow_shrink_copy",
                grow_ok && aligned(shrunk.cast()) && (0..10).all(|i| *shrunk.add(i) == i as u8),
            );
            keeper_graph_free(std::ptr::null_mut());
            check("free_null_noop", true);
            let source = c"keeper";
            let dup = keeper_graph_strdup(source.as_ptr());
            check(
                "strdup",
                aligned(dup.cast()) && CStr::from_ptr(dup).to_bytes() == b"keeper",
            );
            let bounded = keeper_graph_strndup(source.as_ptr(), 3);
            let short = keeper_graph_strndup(c"ab".as_ptr(), 10);
            check(
                "strndup_bounded_terminated",
                aligned(bounded.cast())
                    && aligned(short.cast())
                    && CStr::from_ptr(bounded).to_bytes() == b"kee"
                    && CStr::from_ptr(short).to_bytes() == b"ab",
            );
            check(
                "sqlite_roundup",
                x_roundup(0) == 16 && x_roundup(1) == 16 && x_roundup(17) == 32,
            );
            let block = x_malloc(100);
            let block_ok = aligned(block) && x_size(block) >= x_roundup(100);
            let regrown = x_realloc(block, 300);
            check(
                "sqlite_size_consistent",
                block_ok && aligned(regrown) && x_size(regrown) >= x_roundup(300),
            );
            let sqlite_zero = x_malloc(0);
            check("sqlite_malloc_zero", aligned(sqlite_zero));
            check(
                "sqlite_init_shutdown",
                x_init(std::ptr::null_mut()) == libsqlite3_sys::SQLITE_OK,
            );
            x_shutdown(std::ptr::null_mut());
            // U06 charges the replacement while the old block is still charged.
            let before = keeper_graph_query::allocation::allocated();
            let big = keeper_graph_malloc(1 << 20);
            let with_old = keeper_graph_query::allocation::allocated();
            let bigger = keeper_graph_realloc(big, 2 << 20);
            let peak = keeper_graph_query::allocation::peak_allocated();
            check(
                "realloc_old_and_new_charged_together",
                with_old > before && peak >= with_old + (2 << 20),
            );
            for pointer in [
                zero,
                zero_second,
                calloc_a,
                calloc_b,
                realloc_null_zero,
                realloc_live_zero,
                zeroed.cast(),
                shrunk.cast(),
                dup.cast(),
                bounded.cast(),
                short.cast(),
                bigger,
            ] {
                keeper_graph_free(pointer);
            }
            x_free(regrown);
            x_free(sqlite_zero);
            let residual = keeper_graph_query::allocation::allocated();
            if report.values().any(|v| v != &serde_json::Value::Bool(true)) {
                return Err(format!("positive case failed: {report:?}"));
            }
            report.insert("residual_charged_bytes".into(), residual.into());
            Ok(serde_json::Value::Object(report))
        },
        Case::WrongThread(family) => {
            // Fixtures are owned by the entered thread; only their address crosses.
            let fixture = unsafe { keeper_graph_malloc(64) } as usize;
            let sqlite_fixture = unsafe { x_malloc(64) } as usize;
            let outcome = std::thread::spawn(move || unsafe {
                let p = fixture as *mut c_void;
                let q = sqlite_fixture as *mut c_void;
                match family {
                    Family::Malloc => {
                        let _ = keeper_graph_malloc(16);
                    }
                    Family::MallocZero => {
                        let _ = keeper_graph_malloc(0);
                    }
                    Family::FreeNull => keeper_graph_free(std::ptr::null_mut()),
                    Family::FreeLive => keeper_graph_free(p),
                    Family::Calloc => {
                        let _ = keeper_graph_calloc(2, 8);
                    }
                    Family::ReallocNull => {
                        let _ = keeper_graph_realloc(std::ptr::null_mut(), 0);
                    }
                    Family::ReallocLive => {
                        let _ = keeper_graph_realloc(p, 128);
                    }
                    Family::Strdup => {
                        let _ = keeper_graph_strdup(c"x".as_ptr());
                    }
                    Family::Strndup => {
                        let _ = keeper_graph_strndup(c"xy".as_ptr(), 1);
                    }
                    Family::SqliteMalloc => {
                        let _ = x_malloc(16);
                    }
                    Family::SqliteFree => x_free(q),
                    Family::SqliteRealloc => {
                        let _ = x_realloc(q, 128);
                    }
                    Family::SqliteSize => {
                        let _ = x_size(q);
                    }
                    Family::SqliteRoundup => {
                        let _ = x_roundup(16);
                    }
                    Family::SqliteInit => {
                        let _ = x_init(std::ptr::null_mut());
                    }
                    Family::SqliteShutdown => x_shutdown(std::ptr::null_mut()),
                }
            })
            .join();
            Err(format!("wrong-thread callback returned: {outcome:?}"))
        }
        Case::MallocOverflow => {
            let _ = unsafe { keeper_graph_malloc(usize::MAX) };
            Err("overflowing malloc returned".into())
        }
        Case::CallocOverflow => {
            let _ = unsafe { keeper_graph_calloc(usize::MAX, 2) };
            Err("overflowing calloc returned".into())
        }
        Case::ReallocOverflow => {
            let live = unsafe { keeper_graph_malloc(16) };
            let _ = unsafe { keeper_graph_realloc(live, usize::MAX) };
            Err("overflowing realloc returned".into())
        }
        Case::SqliteNegativeMalloc => {
            let _ = unsafe { x_malloc(-1 as c_int) };
            Err("negative xMalloc returned".into())
        }
        Case::SqliteNegativeRoundup => {
            let _ = unsafe { x_roundup(-1 as c_int) };
            Err("negative xRoundup returned".into())
        }
    }
}
