# Agents

Agents are named souls that live in a drive, work in that drive's sessions and talk over
Matrix. This is the operator document: what is on disk, what keeper reads and writes, what
it never touches, and what to do when something is wrong. The design reasoning lives in
`_bmad-output/planning-artifacts/architecture/architecture-keeper-2026-07-03/ARCHITECTURE-AGENTS.md`
(AD-360…AD-416) and the evidence in `_bmad-output/planning-artifacts/research-agents-2026-10-02.md`;
the decisions are D-31…D-36 in `docs/decisions.md`.

## The agents zone

An agent lives in a zone of its own inside a drive, `80-agents/` by default, next to the drive's
sessions zone. keeper finds the zone only through the folder's configuration, never by guessing a
folder name.

```text
<drive>/80-agents/          the subfolder named by [folder.agents]; default "80-agents"
  README.md                 the zone's guide, the owner's to edit
  AGENTS.md                 rules for anything handed the folder: data to keeper's agents, never obeyed
  _drive.toml               the drive's id, principal, owner and readers
  _template/                skeleton copied when a new agent is made
  _skills/<name>/SKILL.md   skills shared by this drive's agents
  _workflows/<name>/        workflows (a BMAD skill plus workflow.toml)
  <agent>/                  one folder per agent: agent.toml, SOUL.md, USER.md, MEMORY.md,
                            journal/, proposals/
```

Every top-level name that begins with `_`, and `README.md` and `AGENTS.md`, belong to the zone
itself and are never an agent. Every other folder is an agent's home. Files and hidden folders are
never homes.

### Turning the zone on

The flag is `agents` on the profile, written in the folder file as:

```toml
[folder.sessions]

[folder.agents]            # an empty table means "keeps agents, in 80-agents"
subfolder = "80-agents"
```

The Sync form shows the same flag as **This folder keeps agents**. The switch is on screen only
while **This folder has sessions** is on.

keeper refuses a subfolder that is empty, `.`, absolute, or that escapes the folder (`..`). It also
refuses an agents zone that is, contains, or sits inside the notes vault, the recordings root, the
sessions zone, the task ledger or the voices bank. Each refusal names the other zone. tgdrive's
layout, `80-agents` beside `60-sessions`, is the ordinary case.

**The flag needs `[folder.sessions]`.** An agent's sessions live in the same drive's sessions zone,
so a zone without one hosts agents that could never work. keeper refuses it with this sentence, and
gives the same sentence when a save turns sessions off under a folder that keeps agents:

> This folder keeps agents, so it needs a sessions zone: an agent's sessions live in this folder's
> sessions zone. Add [folder.sessions].

Turning the flag off removes the profile's setting and no file. The homes stay on disk.

Every machine that syncs the drive needs a keeper that knows `[folder.agents]`; the release
notes of the release that carries it name that release. An older keeper refuses the whole
`[folder]` table when it holds a key it does not know, the working keys beside it included. Until
every machine runs such a keeper, leave `[folder.agents]` out of a real drive's folder file and arm
the flag per machine from the Sync form instead.

### `_drive.toml`

A zone hosts an agent only when its `_drive.toml` reads. The declaration names the drive's
audience, so keeper never guesses who an agent speaks to.

```toml
version    = 1
id         = "neuradrive"
title      = "neuradrive"
principal  = "neuraffica"
owner      = "@tgorka:example.org"
readers    = ["@marta:example.org", "@tgorka:example.org"]
local_only = false

[integrity]                # optional
untrusted = ["00-inbox/**", "70-comms/**", "recordings/**"]
```

| key | type | default | rule |
| --- | --- | --- | --- |
| `version` | integer | required | `1`. A higher one is refused as written by a newer keeper. |
| `id` | string | required | `[a-z0-9][a-z0-9-]{0,31}` |
| `title` | string | the `id` | at most 64 characters |
| `principal` | string | required | `[a-z0-9-]{1,32}`: the process that hosts this drive's agents |
| `owner` | string | required | a Matrix user id that is also in `readers` |
| `readers` | list of strings | required | at least one Matrix user id, no duplicates. Order does not matter: keeper reads them sorted. |
| `local_only` | boolean | `false` | every agent homed here must use a local model |
| `[integrity].untrusted` | list of globs | see below | drive-relative patterns whose files are read as `untrusted`, whoever wrote them |

When the file has no `[integrity]` table, `untrusted` is `00-inbox/**`, `70-comms/**` and
`recordings/**`: what arrives from outside the drive's readers. A present table replaces that
default entirely; `untrusted = []`, or a table with no `untrusted`, means nothing is untrusted
by place.

The grammar is exact. An unknown key, at the top or inside `[integrity]`, is refused with its name
("_drive.toml has \`reader\`, which is not one of its keys."). So are a value of the wrong type
("\`local_only\` in _drive.toml must be true or false, not text."), a reader that is not a Matrix
id, a duplicate reader, an owner who is not a reader, and a glob that does not compile.

A zone with no `_drive.toml` hosts nothing and says:

> This agents zone has no _drive.toml, so it hosts no agent. Write one naming the drive's readers.

A zone whose `_drive.toml` is refused hosts nothing either, and says
"This agents zone's _drive.toml is refused, so it hosts no agent." followed by the refusal.

In the Files view the zone's folder carries the agents mark, from the configuration and never from
its name.
