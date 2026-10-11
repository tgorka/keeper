# BMAD fixtures and the goldens BMAD printed

Provenance and licences are in `src/bmad/UPSTREAM.md` (§ Fixtures). In short: the skill
directories are verbatim BMAD v6.12.0 (MIT, Copyright (c) 2025 BMad Code, LLC); `_bmad/config.toml`
and `_bmad/config.user.toml` are this repository's installer-written layers; `_bmad/custom/config.toml`
and `_bmad/custom/bmad-architecture.toml` are **makistack's** fleet overlay (what tgdrive links as
`_bmad/custom/`), not BMAD's; `render/_bmad/` is this repository's two layers with `[modules.gds]`'s
duplicated `planning_artifacts`, `implementation_artifacts` and `project_knowledge` removed;
`architecture.memlog.md` is the architecture's `.memlog.md`; `_bmad/custom/bmad-agent-architect.toml`
(appends a principle and an activation step, adds a literal and a `file:` fact, replaces menu item
`CA` by its code), `all-tokens/**` and `render/_bmad/custom/all-tokens.toml` were written for these
tests.

`expected/` is what BMAD's own scripts printed, captured once on 2026-10-05 with the v6.12.0
scripts in this repository's `_bmad/scripts/` (byte-identical to the tag) and the installed party
skill, from this directory. No test runs Python; the scripts are not kept here.

```sh
S=../../../../../../_bmad/scripts   # this repository's _bmad/scripts
P=~/.claude/plugins/cache/bmad-method/bmad/6.12.0.0/skills/bmad-party-mode/scripts/resolve_party.py
python3 -B $S/resolve_config.py --project-root "$PWD" > expected/central.out
python3 -B $S/resolve_config.py --project-root "$PWD" --key agents > expected/central-agents.out
python3 -B $S/resolve_customization.py --skill "$PWD/bmad-agent-architect" --project-root "$PWD" \
  --key agent > expected/architect-agent.out
python3 -B $S/resolve_customization.py --skill "$PWD/bmad-architecture" --project-root "$PWD" \
  --key workflow > expected/architecture-workflow.out
# resolve_party.py runs the resolvers under <project-root>/_bmad/scripts, so it was given a copy
# of this directory's _bmad/ with the scripts beside it, as $PROJECT:
python3 -B $P --project-root "$PROJECT" --skill "$PWD/bmad-party-mode" > expected/party-default.out
python3 -B $P --project-root "$PROJECT" --skill "$PWD/bmad-party-mode" --list-groups \
  > expected/party-groups.out
python3 -B $P --project-root "$PROJECT" --skill "$PWD/bmad-party-mode" --party code-review-crew \
  > expected/party-code-review-crew.out
python3 -B $P --project-root "$PROJECT" --skill "$PWD/bmad-party-mode" --party no-such-room \
  > expected/party-unknown.out
```

`memlog.py`'s `render(*split(text))` over `architecture.memlog.md` printed the file back
unchanged, so the round-trip test compares the port's output with the fixture itself.

The render goldens (`expected/render-<skill>/`: `replacements.out`, `outputs/**`, `manifest.out`)
and `expected/render-refusals.out` were printed in process by the script below, run here as
`python3 -B port_fixtures.py $S`. It calls `render_skill.py`'s own functions with
`/fixture-root` as the project root and `render/` as the central configuration's root, and does
what `render()` does up to `_publish`, writing the manifest as `_publish` writes it.

```python
import hashlib, json, sys
from pathlib import Path

sys.dont_write_bytecode = True
sys.path.insert(0, sys.argv[1])
import memlog, render_skill as rs
from config_utils import load_central_config, load_customization, load_toml

FIXTURES, ROOT = Path.cwd(), Path("/fixture-root")
EXPECTED = FIXTURES / "expected"
RENDERER = hashlib.sha256((Path(sys.argv[1]) / "render_skill.py").read_bytes()).hexdigest()

def write(path, data):
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_bytes(data)

def render(skill):
    skill_dir = FIXTURES / skill
    sources = rs._load_sources(skill_dir)
    central = load_central_config(FIXTURES / "render")
    custom = any(rs._CUSTOM_TOKEN.search(c) for c in sources.values())
    defaults = load_toml(skill_dir / "customize.toml", required=True) if custom else None
    customization = load_customization(FIXTURES / "render", skill_dir) if custom else {}
    replacements, values = rs._resolve_replacements(sources, central, customization, defaults, ROOT)
    root_hash = rs._hash_bytes(str(ROOT).encode())[:12]
    identity = {"project_root": str(ROOT), "renderer_sha256": RENDERER, "resolved_values": values,
                "source_sha256": {n: rs._hash_bytes(c.encode()) for n, c in sources.items()}}
    generation = rs._hash_bytes(rs._canonical_json(identity))[:20]
    destination = ROOT / "_bmad" / "render" / skill / f"fixture-root-{root_hash}" / generation
    outputs = {n: c.encode() for n, c in rs._render_sources(sources, replacements, destination).items()}
    manifest = {"schema_version": 1, "skill": skill, "project_root": str(ROOT),
                "project_slug": "fixture-root", "root_hash": root_hash, "generation_hash": generation,
                "inputs": identity, "outputs": {n: rs._hash_bytes(c) for n, c in outputs.items()}}
    out = EXPECTED / f"render-{skill}"
    write(out / "replacements.out", (json.dumps(replacements, indent=2, ensure_ascii=False) + "\n").encode())
    for name, content in outputs.items():
        write(out / "outputs" / name, content)
    write(out / "manifest.out",
          json.dumps(manifest, ensure_ascii=False, indent=2, sort_keys=True).encode() + b"\n")

def refusal(case, sources, central, customization=None, defaults=None):
    try:
        replacements, _ = rs._resolve_replacements(sources, central, customization or {}, defaults, ROOT)
        rs._render_sources(sources, replacements, ROOT / "out")
    except rs.RenderError as error:
        return case, str(error)
    raise SystemExit(f"{case} rendered")

def layers(*items):
    return {"workflow": {"layers": list(items)}}

central = {"core": {"name": "x"}, "a": {"key": "1"}, "b": {"key": "2"}}
w = {"workflow.md": "{workflow.word}"}
cases = [
    refusal("missing config path", {"workflow.md": "{{config.a.b}}"}, central),
    refusal("missing short key", {"workflow.md": "{{.nokey}}"}, central),
    refusal("ambiguous short key", {"workflow.md": "{{.key}}"}, central),
    refusal("undeclared snapshot", {"workflow.md": "[[bmad-snapshot:nope.md]]"}, central),
    refusal("unsupported default type", {"workflow.md": "{workflow.flag}"}, central,
            {"workflow": {"flag": True}}, {"workflow": {"flag": True}}),
    refusal("review layer without instruction", {"workflow.md": "{workflow.layers}"}, central,
            layers({"id": "a"}), layers({"id": "a", "instruction": "x"})),
    refusal("duplicate review layer id", {"workflow.md": "{workflow.layers}"}, central,
            layers({"id": "a", "instruction": "x"}, {"id": "a", "instruction": "y"}),
            layers({"id": "a", "instruction": "x"})),
    refusal("string expected", w, central, {"workflow": {"word": 7}}, {"workflow": {"word": "default"}}),
    refusal("customization without customize.toml", w, central),
    refusal("relative project root", {"workflow.md": "{{config.core.name}}"},
            {"core": {"name": "x{project-root}"}}),
]
render("bmad-build")
render("all-tokens")
write(EXPECTED / "render-refusals.out", (json.dumps(dict(cases), indent=2, ensure_ascii=False) + "\n").encode())
text = (FIXTURES / "architecture.memlog.md").read_text(encoding="utf-8")
assert memlog.render(*memlog.split(text)) == text
```

`expected/` is git-tracked as printed: `*.out` files are not formatted by Biome, and the rendered
outputs are Markdown, which Biome does not format either.
