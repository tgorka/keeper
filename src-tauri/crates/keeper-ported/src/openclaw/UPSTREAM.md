repository: https://github.com/openclaw/openclaw
commit: 07c176c3
licence: written from documentation; no upstream code is copied
copyright: OpenClaw's LICENSE reads MIT, "Copyright (c) 2026 OpenClaw Foundation", and points to THIRD_PARTY_NOTICES.md, whose two entries are MIT — the likely reason GitHub's API reports the licence as "Other". No source file was read, so none is reproduced.
files read: docs/concepts/dreaming.md (the deep phase's gates :76, session transcript ingestion :86-90, consolidation safety :92-110, the deep ranking signals :163-172, scheduling's `0 3 * * *` :214, maxPriorEntryLossFraction's default :310), docs/cli/memory.md (the scheduled defaults :555-563, the untrusted/system exclusion :415), LICENSE, THIRD_PARTY_NOTICES.md
ported: the structural exclusion (untrusted or system provenance; only interactive sessions), minScore 0.75, minRecallCount 3, minUniqueQueries 3, recencyHalfLifeDays 14, maxAgeDays 30, maxPriorEntryLossFraction 0.25, the six deep-ranking weights, the rule that all three thresholds must pass (`gates.rs`)
not ported: the light and REM phases, the tool-free model completion that chooses merges and supersessions (keeper's night never composes prose: it applies the agent's own proposals), DREAMS.md and the dream diary, relevance and conceptual richness (keeper has no retrieval store or concept tags to measure them; DW-392), phase reinforcement boosts, maxPromotedSnippetTokens (keeper's caps are Hermes' characters)
changed: The documentation names the signals and weights but not how a signal becomes a number, so keeper's measure is its own (epic 95 Q4): frequency = min(1, recall/3), query diversity = min(1, distinct sessions/3), consolidation = min(1, distinct UTC days/3), recency = 0.5^(age/14) from the newest proposal; the score is the weighted sum of these four divided by their weights' total (0.64), so the 0.75 threshold keeps its meaning on a 0..1 scale. A candidate older than maxAgeDays expires rather than staying pending. The loss check counts the previous entries absent from the rewrite (a replaced entry is lost) and refuses a loss above the fraction, so exactly a quarter passes.
revisit: when OpenClaw's dreaming documentation changes its defaults, weights or the structural gate, re-read both files at the new commit and update the constants and the golden table below

# openclaw

`gates.rs` is written from OpenClaw's documentation alone; no code of OpenClaw or of any other
project was read for it. Keeper's candidates (a pending proposal keyed by target, op and folded
text), its evidence rule and its routing live in `keeper_core::agents::consolidate`.

How keeper reads the structural exclusion: a proposal at `untrusted` integrity is untrusted
provenance; a `scheduled` session is a cron session; a `delegated` one is a subagent; `foreground`
and `review` sessions are interactive. A `gate` session's proposal never reaches this module: the
night skips it and the weekly curator expires it (AD-402, R29 F4).

## Golden table

What `gates::score` and `gates::gate` make of measured candidates; `gates_and_score_table` reads
this table and asserts every row.

| recall | sessions | days | age (days) | score | gate |
| --- | --- | --- | --- | --- | --- |
| 3 | 3 | 3 | 0 | 1.000 | promote |
| 2 | 2 | 2 | 0 | 0.745 | pending |
| 3 | 3 | 3 | 14 | 0.883 | promote |
| 3 | 3 | 3 | 30 | 0.819 | promote |
| 3 | 3 | 3 | 31 | 0.816 | expired |
| 3 | 3 | 1 | 0 | 0.896 | promote |
| 3 | 3 | 1 | 19 | 0.753 | promote |
| 3 | 3 | 1 | 20 | 0.749 | pending |
| 5 | 2 | 3 | 0 | 0.922 | pending |
| 1 | 1 | 1 | 0 | 0.490 | pending |
| 4 | 4 | 4 | 7 | 0.931 | promote |
