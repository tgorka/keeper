//! NousResearch/hermes-agent, ported (stories 95.1 and 95.3, AD-396): the
//! memory store's semantics ([`memory`]), the threat scan every memory and
//! skill proposal passes first ([`threats`]), the prompts of the review
//! pass the nudges start ([`review`]), and the curator's lifecycle of an
//! agent's skills ([`curator`]). Keeper's adapters — frontmatter, the
//! separator line, the files, a skill's last change — are
//! `keeper_core::agents::{memory, curate}`'s; `UPSTREAM.md` records what was
//! ported, changed and left out.

pub mod curator;
pub mod memory;
pub mod review;
pub mod threats;
