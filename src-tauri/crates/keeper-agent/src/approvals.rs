//! What a call that waited for a person is told when it was not done
//! (AD-393, AD-394; FR-797, NFR-117; rulings R73–R80, R179, R180).
//!
//! The words are shared by every host that parks a call behind a person:
//! [`crate::parked`]'s record port and keeper-hq's runtime. The park and
//! resume over an agent's Matrix room — the request event, the room's
//! `consumed` state, the claim it was fenced on — left keeper in 108.2.

/// What the model is told of a call a person declined.
pub const DENIED: &str = "A person declined this, so keeper did not do it. Nothing was changed.";

/// What the model is told of a call whose approval expired undecided.
pub const EXPIRED: &str =
    "Nobody decided on this in time, so keeper did not do it. Nothing was changed.";

/// What a call of a parked round is told when a stop cut its round's
/// continuation while it may have been running.
pub const INTERRUPTED: &str = "keeper stopped while this ran, so it does not know whether it took effect, and it will not run it again. Check, and ask again if it is still needed.";

/// What the model is told of a call whose request was never sent because
/// its arguments could not be attached: nobody was asked (R180).
pub const UNATTACHED: &str = "keeper could not attach exactly what this asks to its request, so nobody was asked and it was not done. Nothing was changed.";

/// What the model is told of a call approved here whose approval expired
/// before keeper could use it: it was never consumed, so never run (R179).
pub const APPROVED_UNUSED: &str = "This was approved, but keeper could not use the approval before it expired, so it did not do it. Nothing was changed.";

/// Why an approved call does not run when what it would do now is not the
/// action its approval names.
pub const CHANGED: &str = "the action changed after it was approved";

/// What the model is told of an approved call that did not run because
/// `reason` moved after its approval.
pub fn not_done(reason: &str) -> String {
    format!("This was not done: {reason}. Nothing was changed.")
}
