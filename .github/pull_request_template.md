## Summary

<!-- What does this PR change and why? -->

## Provenance checklist

- [ ] Any ported code has its **source and license identified**, and that license is permissive (Apache-2.0 / MIT / BSD / ISC / …).
- [ ] GPL/AGPL projects (e.g. Element X, gomuks) were used **for study only** — no code was copied from them.
- [ ] Any new dependencies (Rust or npm) pass the **license firewall** (`cargo deny check` + the JS license gate `bun run check:licenses`).
- [ ] **No secrets or tokens** (Apple certificates/keys, `.p8` API keys, passwords, Matrix access tokens) are committed.
- [ ] `bun run check:all` passes locally.
