//! The messenger's own decisions that are not Matrix plumbing (Epic 100).
//!
//! What a line typed into the messenger composer means when it starts with a
//! slash ([`commands`], Story 100.5, R385), and what a `/` in the middle of
//! a message offers ([`inline`], Story 101.3, R418).

pub mod commands;
pub mod inline;
