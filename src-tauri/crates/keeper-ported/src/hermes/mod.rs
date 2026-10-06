//! NousResearch/hermes-agent, ported (story 95.1, AD-396): the memory
//! store's semantics ([`memory`]), the threat scan every memory and skill
//! proposal passes first ([`threats`]), and the prompts of the review pass
//! the nudges start ([`review`]). Keeper's adapter — frontmatter, the
//! separator line, the files — is `keeper_core::agents::memory`'s;
//! `UPSTREAM.md` records what was ported, changed and left out.

pub mod memory;
pub mod review;
pub mod threats;
