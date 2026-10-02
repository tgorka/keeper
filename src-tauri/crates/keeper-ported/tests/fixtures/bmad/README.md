# BMAD fixture: Winston, the System Architect

- `bmad-agent-architect/customize.toml` is a verbatim copy of BMAD-METHOD v6.12.0's
  `skills/bmad-agent-architect/customize.toml` (installed at
  `~/.claude/plugins/cache/bmad-method/bmad/6.12.0.0/`). MIT, Copyright (c) 2025 BMad Code,
  LLC; attributed in `src/bmad/UPSTREAM.md`.
- `_bmad/custom/bmad-agent-architect.toml` is a team override written for this test: it appends
  a principle and an activation step, adds a literal and a `file:` persistent fact, and replaces
  menu item `CA` by its code.
- `winston-resolved.json` is the golden: what BMAD's own resolver printed for the two layers,
  captured once on 2026-10-02 from this directory with BMAD v6.12.0's script, which is not kept:

```sh
python3 -B _bmad/scripts/resolve_customization.py \
  --skill "$PWD/bmad-agent-architect" --project-root "$PWD" --key agent > winston-resolved.json
```

`bmad::config::tests::merges_winston_like_resolve_customization` compares the Rust merge to it
over JSON values.
