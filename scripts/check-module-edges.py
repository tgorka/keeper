#!/usr/bin/env python3
"""Enforce workspace roles over normal/build edges, never dev-only edges.

Declare every member in module-roles.json. Contracts and provider isolation use
resolved transitive trees; implementation and leaf rules constrain direct
first-party edges. All targets and features count. Composition roots alone may
reach providers; a provider may reach itself but not another provider.

KEEPER_MODULE_MANIFEST / KEEPER_MODULE_ROLES select isolated acceptance fixtures.
Cargo diagnostics pass through and a failed command can never print 'passed'.
"""
import json
import os
from pathlib import Path
import re
import subprocess

ROOT = Path(__file__).resolve().parents[1]
MANIFEST = Path(os.environ.get("KEEPER_MODULE_MANIFEST", ROOT / "src-tauri/Cargo.toml"))
ROLES = Path(os.environ.get("KEEPER_MODULE_ROLES", ROOT / "scripts/module-roles.json"))
KINDS = {"contract", "composition-root", "legacy", "port", "provider",
         "implementation", "path-leaf", "folder-leaf"}


def cargo(*args):
    result = subprocess.run(["cargo", *args, "--manifest-path", str(MANIFEST)],
                            text=True, stdout=subprocess.PIPE)
    if result.returncode:
        raise SystemExit(result.returncode)
    return result.stdout


def tree(package, direct=False):
    args = ["tree", "-p", package, "--all-features", "--target", "all",
            "-e", "normal,build", "--prefix", "none", "--format", "{p}"]
    if direct:
        args += ["--depth", "1"]
    return {match.group(1) for line in cargo(*args).splitlines()
            if (match := re.match(r"^(\S+) v\S+", line))}


roles = json.loads(ROLES.read_text())
metadata = json.loads(cargo("metadata", "--no-deps", "--format-version", "1"))
member_ids = set(metadata["workspace_members"])
members = {p["name"] for p in metadata["packages"] if p["id"] in member_ids}
errors = []
for name in sorted(members):
    if roles.get(name) not in KINDS:
        errors.append(f"{name}: missing or invalid declared role")
for name in sorted(roles.keys() - members):
    errors.append(f"{name}: role declared for a non-member")
if errors:
    raise SystemExit("\n".join(errors))

for name in sorted(members):
    role = roles[name]
    resolved = tree(name) - {name}
    for dependency in sorted(resolved):
        other = roles.get(dependency)
        first_party = dependency in members or dependency.startswith("keeper-") or dependency == "keeper"
        if first_party and other is None:
            errors.append(f"{name}: first-party dependency {dependency} has no declared role")
        if role != "composition-root" and other == "provider":
            errors.append(f"{name} ({role}) reaches provider {dependency}")
        if role == "contract" and (
            other in {"port", "provider", "implementation", "composition-root", "legacy"}
            or re.fullmatch(r"syncular-.*|rusqlite|libsqlite3-sys|tokio|tauri.*|matrix-sdk.*", dependency)
        ):
            errors.append(f"{name} (contract) reaches forbidden {dependency}")
    if role in {"implementation", "path-leaf", "folder-leaf"}:
        for dependency in sorted(tree(name, direct=True) - {name}):
            if dependency not in roles:
                continue  # Third-party dependencies are not governed by these direct-edge rules.
            allowed = (
                role == "implementation" and roles[dependency] in {"port", "contract", "path-leaf", "folder-leaf"}
                or role == "folder-leaf" and roles[dependency] == "path-leaf"
            )
            if not allowed:
                errors.append(f"{name} ({role}) has forbidden direct edge to {dependency}")
if errors:
    raise SystemExit("\n".join(errors))
print(f"module dependency roles passed ({len(members)} workspace members)")
