//! What an agent's turn tells the person when it could not finish.
//!
//! An agent's turn in a session over a Matrix room — the held context, the
//! room's arrivals, the paced edits, the claim's lease — left keeper in
//! 108.2: the agents' runtime serves from the agents' database in keeper-hq
//! ([`crate::served`] runs the turn there). What stays is the sentence a
//! turn cut off by a restart ends with, which keeper-hq's runtime shares.

/// The message for a turn cut off by a restart (C6): it is not re-run,
/// because its tool calls may already have had effects.
pub fn cut_off_sentence(host: &str) -> String {
    format!("My answer was cut off when {host} restarted. Ask again if you still need it.")
}
