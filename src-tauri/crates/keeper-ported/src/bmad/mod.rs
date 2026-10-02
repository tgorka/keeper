//! BMAD-METHOD, rewritten in Rust (AD-396). BMad™ is a trademark of BMad Code,
//! LLC; keeper uses the name only to describe compatibility. `UPSTREAM.md`
//! beside this file records the licence, the commit and what was ported.
//!
//! - [`config`]: strict TOML layers and BMAD's structural merge, the rule
//!   `resolve_customization.py` applies to an agent's `customize.toml`.
pub mod config;
