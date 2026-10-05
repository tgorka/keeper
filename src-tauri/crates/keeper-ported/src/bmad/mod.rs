//! BMAD-METHOD, rewritten in Rust (AD-396). BMad™ is a trademark of BMad Code,
//! LLC; keeper uses the name only to describe compatibility. `UPSTREAM.md`
//! beside this file records the licence, the commit and what was ported.
//!
//! - [`config`]: strict TOML layers and BMAD's structural merge — the central
//!   configuration `resolve_config.py` prints and the customization
//!   `resolve_customization.py` applies to a skill's `customize.toml`.
//! - [`render`]: `render_skill.py`'s format-B render, pure: tokens, the
//!   generation's identity and manifest, and the check of an existing one.
//! - [`memlog`]: `memlog.py`'s memory log, as functions over the file's text.
//! - [`party`]: `resolve_party.py`'s party-mode roster and its projections.
pub mod config;
pub mod memlog;
pub mod party;
mod py;
mod py_printable;
pub mod render;
