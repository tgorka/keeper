#!/usr/bin/env python3
"""Regenerate keeper-ported's okf parity fixtures by running the drive's own
OKF tools (tgdrive `.okf/bin/`) on inputs this script writes.

The tools are run, never read: every expected value below is what they
answered. No default test runs Python; the Rust tests read the files this
writes, and the `#[ignore]` real-drive test runs `--drive-config`.

Usage: KEEPER_TGDRIVE=/path/to/tgdrive python3 generate.py
       KEEPER_TGDRIVE=/path/to/tgdrive python3 -B generate.py --drive-config

`--drive-config` writes nothing: it prints the drive's own `load_config` of
its own `.okf/config.yaml` as JSON, which is kept out of this repository
until OA-95-2 is answered (DW-713).

Writes, beside this script:
  config.json       load_config over config.yaml (with PyYAML, as the drive runs)
  yaml.json         where the drive's fallback parser disagrees with PyYAML on config.yaml
  match.jsonl       _match(pattern, path) over a probe table
  paths.jsonl       is_excluded and bundle_for over config.yaml
  drive-paths.jsonl is_excluded and bundle_for over the drive's own config
  docs.jsonl        read_doc's meta, title and description over written files
  links.jsonl       okf_links.resolve(base, target) over a probe table
  index.jsonl       the listings `okf index` wrote for a written drive, with
                    the documents and folders each one lists
"""

import json
import os
import shutil
import subprocess
import sys
import tempfile

HERE = os.path.dirname(os.path.abspath(__file__))
DRIVE = os.environ.get("KEEPER_TGDRIVE", "/workspace/tgdrive")


def jsonl(name, rows):
    with open(os.path.join(HERE, name), "w", encoding="utf-8") as out:
        for row in rows:
            out.write(json.dumps(row, ensure_ascii=False, sort_keys=True) + "\n")


def plain(value):
    if hasattr(value, "__dict__"):
        return {k: plain(v) for k, v in vars(value).items()}
    if isinstance(value, (list, tuple)):
        return [plain(v) for v in value]
    if isinstance(value, dict):
        return {k: plain(v) for k, v in value.items()}
    return value


PATTERNS = [
    "a/**", "a/**/w/**", "*.c-*.md", "a/*", "a/b", "a", "**/x.md", "a?c", "[ab]/x",
    "/a/x", "a/", "a/b/**", "*", "*.md", "a*", "**", "a/**/x", "x.md", "a*/x", "a/*/x",
    "x*.md", "*x.md", "?.md", "d/*.md", "*/x.md", "a/b*", "**x", "a**", "a/*/**", "a\\*",
    "a/*b", "a.*", "a/**/b/c", "?", "a/?", "x?md", "x.m*", "d*", "*d", "x*", "b",
    "a?b", "*/", "**/", "x/a?b", "a/b/", "a/b*/", "a/b*/*", "*/**", "a/x*/**", "a/**/",
    "a/**x", "a/***", "A*", ".*", "*-*", "*c", "a b", "60-sessions/**/workspace/**",
    "*.sync-conflict-*.md",
]
PROBES = [
    "", "a", "a/", "a/b", "a/b/", "a/b/c", "a/x", "ab/x", "ab/c/x", "a/b/w/x", "a/**/w/x",
    "a/w/x", "n.c-1.md", "d/n.c-1.md", "abc", "a/c", "b/x", "x.md", "d/x.md", "a/x.md",
    "/a/x", "a/b.md", "A/b", "ax", "a.md", "d/e/x.md", "a/b/x", "a/b/c/x", "a//x",
    "d/x1.md", "x1.md", "d/e/x1.md", "d/ax.md", "d/a.md", "[ab]/x", "a/bc", "a/bx",
    "ab", "a*", "a/cb", "a/c/b", "a.b", "a/x/b/c", "/", "x/a/b", "x/acb", "a/bc/d",
    "a/b*/d", "*/b", "a/x*/q", "a/xy/q", "a/b/x", "a/**x", "a/***b", "a/.b", ".b",
    ".a/b", "a-b/c", "a/b-c", "a b", "60-sessions/active/s/workspace/a.md",
    "60-sessions/**/workspace/a.md", "notes/a.sync-conflict-1.md",
    "a.sync-conflict-x/b.md", "x/y", "d/xy", "dx.md", "e/dx.md", "x/d", "dd", "acb",
]

# The paths acceptance 1 names, and more of the same kinds, over the drive's
# own config.
DRIVE_PATHS = [
    "00-inbox/x.md", "00-inbox/README.md", "00-inbox/AGENTS.md", "00-inbox/log.md",
    "00-inbox/sub/README.md", "00-inbox/MIGRATION-tgnotes.md", "99-temp/x.md",
    "99-temp/README.md", "99-temp/a/AGENTS.md", "30-work/clients/acme/a.md",
    "30-work/projects/keeper/a.md", "30-work/README.md", "notes/a.sync-conflict-1.md",
    "a.sync-conflict-2.md", "10-notes/x/b.sync-conflict-3.md", "10-notes/.keeper/x.md",
    "10-notes/a.md", "10-notes/journal/2026/x.md", "60-sessions/.keeper/x.md",
    "50-library/books/x/README.md", "50-library/README.md", "50-library/audiobooks/a.md",
    "50-library/courses/c/x.md", "50-library/music/m.md", "50-library/articles/a.md",
    "60-sessions/_template/README.md", "60-sessions/active/s/README.md",
    "60-sessions/active/s/workspace/a.md", "60-sessions/**/workspace/a.md",
    "60-sessions/_bmad-output/x.md", "80-agents/nixi/MEMORY.md", "80-agents/_skills/s/SKILL.md",
    "_bmad/core/x.md", "recordings/x.md", ".okf/tmp/x.md", ".okf/OKF-0.2-digest.md",
    "20-records/a.md", "40-media/a.md", "70-comms/a.md", "90-archive/a.md", "README.md",
    "10-notesx/a.md",
]

LINKS = [
    ("10-notes", "a.md"), ("10-notes", "sub/"), ("10-notes", "../x.md"),
    ("10-notes", "/20-records/y.md"), ("", "a%20b.md"), ("10-notes", "a.md#h"),
    ("10-notes", "https://x.y/z"), ("10-notes", "./a.md"), ("a/b", "../../../x.md"),
    ("10-notes", "mailto:q"), ("10-notes", "a.md?x=1"), ("", "#frag"),
    ("10-notes", "a%2Fb.md"), ("10-notes", ""), ("a", "b/../c.md"), ("a", "//x/y"),
    ("a", "x:y.md"), ("a", "a b.md"), ("10-notes/a.md", "b.md"), (".", "x.md"),
    ("a", "%41.md"), ("a", "%25.md"), ("a", "b%20c%2C.md"), ("a", "C:x"), ("a", "1:x"),
    ("a", "a+b.md"), ("a", "x.md#a%20b"), ("a", ".."), ("a", "."), ("a", "/"),
    ("a", "./"), ("a", "b//c.md"), ("a", "HTTP://x"), ("a", "b.md?"), ("", "x/../../y"),
    ("a", " x.md"), ("a", "<x.md>"), ("a/", "x.md"), ("a", "b/./c/"), ("a", "?q"),
    ("a", "https://x/y?q=1#f"), ("a", "x.md?q#f"), ("a", "x.md#f?q"), ("a", "a-b:c"),
    ("a", "a.b:c"), ("", "/"), ("", ""), ("a b", "c d.md"), ("a", "%20"),
]

DOCS = {
    "a.md": "---\ntype: Note\ntitle: T1\ndescription: D1\n---\n# H\nbody\n",
    "b.md": "---\ntype: Note\n---\n\nintro\n# Heading One\n## Sub\n",
    "my-file_name.md": "no fm\n#Not\n#  Spaced  Head  \n",
    "e.md": "---\ntype: Note\ntitle: \"\"\ndescription: \n---\n",
    "f.md": "---\ntype: Note\ndescription: >-\n  folded\n  text\n---\nbody\n",
    "h.md": "---\ntype: Note\n---\n```\n# not a heading\n```\n# Real\n",
    "i.md": "---\ntitle: [a, b]\ndescription: 3\n---\n",
    "x-y_z.md": "body only\n",
    "s.md": "---\ntitle: \"  T  \"\ndescription: \"  D  x  \"\n---\n",
    "n.md": "---\ntitle: 5\n---\n# H5\n",
    "two.md": "# First\n# Second\n",
    "blank.md": "---\ntitle:\n---\n#   \n# Real\n",
    "ind.md": "  # Indented\n",
    "cr.md": "---\r\ntitle: CR\r\n---\r\n",
    "open.md": "---\ntype: x\n",
    "tab.md": "# Tab\there\n",
    "ws.md": "---\ntitle: \"a   b\"\ndescription: \"line\\none\"\n---\n",
    "blanktitle.md": "---\ntitle: \"   \"\n---\n# Fallback\n",
    "flow.md": "---\ntype: Session\ntags: [a, \"b c\"]\nwindow: {from: x, to: y}\n---\n",
    "lists.md": "---\ntype: Note\nsources:\n  - id: one\n    resource: /a.md\n  - id: two\n    resource: b.md\n---\n",
    "bools.md": "---\ntype: Note\nhuman_reviewed: false\nstatus: draft\nn: 12\nz: ~\n---\n",
    "comment.md": "---\n# a comment\ntype: Note # trailing\ntitle: 'It''s'\n---\n",
    "block.md": "---\ntype: Note\ndescription: |\n  kept\n  lines\n---\n",
    "keep.md": "---\ntype: Note\ndescription: >\n  folded\n\n  para\n---\n",
    "index.md": "",
    "log.md": "## 2026-10-06\n",
    "nested.md": "---\ntype: Note\ngenerated:\n  by: agent:nixi@electra\n  at: x\n---\n",
}

# A small drive for `okf index`: its config is config.yaml.
TREE = {
    "README.md": "---\ntype: Guide\ntitle: Shelf\ndescription: The root.\n---\n",
    "top.md": "---\ntype: Note\ntitle: Top note\ndescription: At the root.\n---\n",
    "with space.md": "---\ntype: Note\ndescription: A spaced name.\n---\n# Spaced\n",
    "notes/README.md": "---\ntype: Guide\ntitle: Notes guide\n---\n",
    "notes/idea.md": "---\ntype: Note\ntitle: An idea\ndescription: Worth keeping — maybe.\n---\n",
    "notes/sub/child.md": "---\ntype: Note\ntitle: Child\n---\n",
    "notes/deep/README.md": "---\ntype: Guide\n---\n# Deep\n",
    "work/README.md": "---\ntype: Guide\ntitle: Work\n---\n",
    "work/plan.md": "---\ntype: Plan\ntitle: \"Plan [v2] (draft)\"\ndescription: \"Has - a dash, and [brackets].\"\n---\n",
    "work/clients/acme/x.md": "---\ntype: Note\n---\n",
    "inbox/README.md": "---\ntype: Guide\n---\n# Inbox\n",
    "inbox/drop.md": "unsorted\n",
}


def main():
    sys.path.insert(0, os.path.join(DRIVE, ".okf", "bin"))
    import yaml  # the drive's tools use PyYAML when it is installed
    import okf_lib
    import okf_links

    synth = tempfile.mkdtemp(prefix="okf-synth-")
    try:
        shutil.copytree(os.path.join(DRIVE, ".okf", "bin"), os.path.join(synth, ".okf", "bin"))
        shutil.copy(os.path.join(HERE, "config.yaml"), os.path.join(synth, ".okf", "config.yaml"))
        config_path = os.path.join(synth, ".okf", "config.yaml")
        text = open(config_path, encoding="utf-8").read()
        cfg = okf_lib.load_config(config_path)
        with open(os.path.join(HERE, "config.json"), "w", encoding="utf-8") as out:
            json.dump(plain(cfg), out, ensure_ascii=False, indent=1, sort_keys=True)
            out.write("\n")
        with open(os.path.join(HERE, "yaml.json"), "w", encoding="utf-8") as out:
            json.dump(
                {"pyyaml": yaml.safe_load(text), "fallback": okf_lib._fallback_parse(text)},
                out, ensure_ascii=False, indent=1, sort_keys=True,
            )
            out.write("\n")

        jsonl("match.jsonl", [
            {"pattern": p, "path": q, "match": okf_lib._match(p, q)}
            for p in PATTERNS for q in PROBES
        ])

        def answers(config, paths):
            rows = []
            for path in paths:
                bundle = config.bundle_for(path)
                rows.append({
                    "path": path,
                    "excluded": okf_lib.is_excluded(config, path),
                    "bundle": bundle.name if bundle else None,
                })
            return rows

        probe_paths = sorted(set(PROBES + DRIVE_PATHS + list(TREE) + [
            "inbox", "inbox/x.md", "inbox/README.md", "inbox/sub/README.md", "tmp/x.md",
            "tmp/README.md", "work/clients/acme/x.md", "work/clientsx/a.md", "notes",
            "notes/deep", "notes/deep/a.md", "notes/deeper/a.md", "notes/.keeper/x.md",
            "a/drafts/x.md", "drafts/x.md", "archive1.md", "x/archive1.md", "exact/file.md",
            "x/exact/file.md", "sessions/a/workspace/x.md", "sessions/**/workspace/x.md",
            "work", "work/a.md", "workx/a.md",
        ]))
        jsonl("paths.jsonl", answers(cfg, probe_paths))
        drive_cfg = okf_lib.load_config(os.path.join(DRIVE, ".okf", "config.yaml"))
        jsonl("drive-paths.jsonl", answers(drive_cfg, DRIVE_PATHS))

        docs_dir = os.path.join(synth, "docs")
        os.makedirs(docs_dir)
        rows = []
        for name, body in sorted(DOCS.items()):
            path = os.path.join(docs_dir, name)
            with open(path, "w", encoding="utf-8", newline="") as out:
                out.write(body)
            doc = okf_lib.read_doc(path, "docs/" + name)
            meta = doc.meta if isinstance(doc.meta, dict) else {}
            rows.append({
                "name": name, "text": body, "meta": meta, "title": doc.title,
                "description": doc.description, "reserved": doc.reserved,
                "error": doc.error is not None,
            })
        jsonl("docs.jsonl", rows)

        rows = []
        for base, target in LINKS:
            kind, value = okf_links.resolve(base, target)
            rows.append({"base": base, "target": target, "kind": kind, "value": value})
        jsonl("links.jsonl", rows)

        shutil.rmtree(docs_dir)
        for rel, body in TREE.items():
            path = os.path.join(synth, rel)
            os.makedirs(os.path.dirname(path), exist_ok=True)
            with open(path, "w", encoding="utf-8") as out:
                out.write(body)
        subprocess.run([os.path.join(synth, ".okf", "bin", "okf"), "index"], cwd=synth,
                       check=True, stdout=subprocess.DEVNULL)
        rows = []
        for root, dirs, files in sorted(os.walk(synth)):
            dirs[:] = sorted(d for d in dirs if d != ".okf")
            if "index.md" not in files:
                continue
            rel_dir = os.path.relpath(root, synth).replace(os.sep, "/")
            rel_dir = "" if rel_dir == "." else rel_dir
            listing = open(os.path.join(root, "index.md"), encoding="utf-8").read()
            documents = []
            for name in sorted(files):
                rel = f"{rel_dir}/{name}" if rel_dir else name
                doc = okf_lib.read_doc(os.path.join(root, name), rel)
                if name.endswith(".md") and not doc.reserved and not okf_lib.is_excluded(cfg, rel):
                    documents.append({"path": rel, "title": doc.title, "description": doc.description})
            rows.append({"dir": rel_dir, "text": listing, "documents": documents})
        jsonl("index.jsonl", rows)
    finally:
        shutil.rmtree(synth)


def drive_config():
    sys.dont_write_bytecode = True
    sys.path.insert(0, os.path.join(DRIVE, ".okf", "bin"))
    import okf_lib

    cfg = okf_lib.load_config(os.path.join(DRIVE, ".okf", "config.yaml"))
    json.dump(plain(cfg), sys.stdout, ensure_ascii=False, sort_keys=True)


if __name__ == "__main__":
    if sys.argv[1:] == ["--drive-config"]:
        drive_config()
    else:
        main()
