//! `cfg(phone)`: an iPhone or Android build (story 98.2, AD-413).
//!
//! A phone reads sessions and moves cards; it never writes a session's log or
//! an approval. What writes either — `writer`, the session worker in `agent`,
//! the run writer in `cards`, the park and resume in `approvals`, and every
//! module built on them — is `#[cfg(not(phone))]`, so a phone code path that
//! names one does not compile.
//!
//! `KEEPER_AGENT_PHONE=1` sets the cfg on any host, so the phone's half is
//! checked on Linux too (`bun run check:agent-phone`); the target is what sets
//! it for a real phone build.

fn main() {
    println!("cargo::rustc-check-cfg=cfg(phone)");
    println!("cargo::rerun-if-env-changed=KEEPER_AGENT_PHONE");
    println!("cargo::rerun-if-changed=build.rs");
    let os = std::env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    let asked = std::env::var_os("KEEPER_AGENT_PHONE").is_some_and(|value| value == "1");
    if matches!(os.as_str(), "ios" | "android") || asked {
        println!("cargo::rustc-cfg=phone");
    }
}
