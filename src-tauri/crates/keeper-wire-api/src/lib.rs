//! keeper's local socket envelope, `keeper.wire.v1` (story 123.3; MA/AD-3 as
//! amended by O-123-WIRE-1, MA/AD-4).
//!
//! - [`frame`]: the frames, one JSON object per line, `serde` types and no
//!   generated code; [`schema`] commits their JSON Schema and checks a change
//!   against the last release's.
//! - [`error`]: keeper's fixed sentence for each code and error kind (m1).
//! - [`place`]: `<data dir>/wire/wire.sock`, the path-length check and the
//!   directory checks; [`lock`]: the data directory's lock.
//! - [`client`]: a client that refuses a listener that is not its user's,
//!   says `hello`, carries many calls on one connection and sends `cancel`
//!   when its caller drops a call.
//!
//! Role `transport` (MA-G3): no first-party edge, and none of `tonic`,
//! `prost`, `hyper` or `tower`.

pub mod client;
pub mod error;
pub mod frame;
pub mod lock;
pub mod place;
pub mod schema;

pub use error::sentence;
pub use frame::*;
pub use lock::{admit, Admission, DataLock, LockError};
pub use place::{socket_path, PlaceError};

/// Whether `id` is a ULID in its canonical spelling: 26 characters of
/// Crockford base32, upper case, the first at most `7`. Any other spelling
/// is not a call id here.
#[must_use]
pub fn is_call_id(id: &str) -> bool {
    id.len() == 26
        && id.as_bytes()[0] <= b'7'
        && id
            .bytes()
            .all(|b| matches!(b, b'0'..=b'9' | b'A'..=b'H' | b'J' | b'K' | b'M' | b'N' | b'P'..=b'T' | b'V'..=b'Z'))
}

/// A fresh call id.
#[must_use]
pub fn new_call_id() -> String {
    ulid::Ulid::new().to_string()
}
