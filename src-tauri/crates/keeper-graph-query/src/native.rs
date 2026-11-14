use std::sync::atomic::{AtomicBool, Ordering};
static REGISTERED: AtomicBool = AtomicBool::new(false);

// U01 SAFETY: GraphQLite 995a8b9 extension.c:677, SQLite 3.53.2.
// The worker exclusively owns this fresh connection on marked main. Its allocator
// callbacks never return NULL; all registration arguments are pinned constants.
// Reject repeat registration and every live/cached statement BEFORE C entry:
// upstream double-frees on create_function_v2 failure, inside the call itself.
// On any non-OK return exit without Drop of the partly registered connection.
#[allow(unsafe_code)]
pub fn register(connection: &rusqlite::Connection) {
    crate::allocation::require_main();
    if REGISTERED.swap(true, Ordering::SeqCst) {
        std::process::exit(73);
    }
    extern "C" {
        fn sqlite3_graphqlite_init(
            db: *mut libsqlite3_sys::sqlite3,
            error: *mut *mut std::ffi::c_char,
            api: *const libsqlite3_sys::sqlite3_api_routines,
        ) -> std::ffi::c_int;
    }
    // SAFETY: connection owns the live handle; no concurrent thread/user exists.
    let handle = unsafe { connection.handle() };
    // SAFETY: SQLite handle valid, null enumerates the first prepared statement.
    if !unsafe { libsqlite3_sys::sqlite3_next_stmt(handle, std::ptr::null_mut()) }.is_null() {
        std::process::exit(73);
    }
    let mut error = std::ptr::null_mut();
    // SAFETY: guarded connection, permanent hooks, SQLITE_CORE makes null pApi valid.
    let rc = unsafe { sqlite3_graphqlite_init(handle, &mut error, std::ptr::null()) };
    if rc != libsqlite3_sys::SQLITE_OK {
        std::process::exit(74);
    }
}
