---
type: Zone Guide
title: "{{drive}}'s agents"
---
# {{drive}}'s agents

The agents of {{drive}} live here, one folder each. Each agent's folder holds who it is
(`SOUL.md`), what it runs on and may use (`agent.toml`), what it remembers (`USER.md`,
`MEMORY.md`), and what it wrote itself (`journal/`, `proposals/`).

| entry | what it is |
| --- | --- |
| `_drive.toml` | who may read this drive, and the zones whose files are other people's words |
| `_template/` | the folder copied when a new agent is made (`keeper-agentd agents new <id>`) |
| `_skills/`, `_workflows/` | what every agent of this drive may load or run |
| `AGENTS.md` | the rules for anything handed this folder |
| `<agent>/` | one agent |

A name that begins with `_`, and this guide and `AGENTS.md`, belong to the zone and are never an
agent. keeper wrote these files once, when the zone was seeded; they are yours from that commit, and
no keeper tool writes them again.
