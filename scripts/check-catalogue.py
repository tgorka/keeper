#!/usr/bin/env python3
"""Check catalogue compatibility (XR-6), classification (MA-G13), and vectors digests.

KEEPER_CATALOGUE_ROOT overrides the repository root (for self-test fixtures).
KEEPER_CATALOGUE_BASE overrides the base snapshot path ('none' skips compatibility).
"""
import hashlib
import json
import os
from pathlib import Path
import subprocess
import tomllib

ROOT = Path(os.environ.get("KEEPER_CATALOGUE_ROOT", Path(__file__).resolve().parents[1]))
SNAPSHOT = ROOT / "catalogue/snapshot.json"
BREAKING = ROOT / "catalogue/breaking.toml"
CLASSIFICATION = ROOT / "catalogue/classification.toml"

EFFECT_RANK = {"read": 0, "write": 1, "decide": 2, "secret": 3}
WIRE_RULE_KEYS = ("wire_forbid", "wire_fix", "wire_gate", "to_proposal")
ADDED_PART_KEYS = (
    "effect",
    "proposal",
    "wire_forbid",
    "wire_fix",
    "wire_gate",
    "to_proposal",
    "vectors",
    "input_enums",
)


def canonical(value):
    return json.dumps(value, sort_keys=True, separators=(",", ":"))


def part_sha256(part):
    return hashlib.sha256(canonical(part).encode("utf-8")).hexdigest()


def resolve_ref(root, node, depth=0):
    seen = 0
    while isinstance(node, dict) and isinstance(node.get("$ref"), str) and depth + seen < 64:
        ref = node["$ref"]
        if not ref.startswith("#/"):
            break
        cur = root
        for token in ref[2:].split("/"):
            key = token.replace("~1", "/").replace("~0", "~")
            if isinstance(cur, dict) and key in cur:
                cur = cur[key]
            else:
                return node
        node = cur
        seen += 1
    return node


def schema_type(root, node, depth=0):
    if depth > 64 or not isinstance(node, dict):
        return None
    node = resolve_ref(root, node, depth)
    if not isinstance(node, dict):
        return None
    value = node.get("type")
    if isinstance(value, str):
        return (value,)
    if isinstance(value, list):
        return tuple(sorted(str(item) for item in value))
    for key in ("anyOf", "oneOf"):
        branches = node.get(key)
        if isinstance(branches, list):
            found = set()
            for branch in branches:
                branch_type = schema_type(root, branch, depth + 1)
                if branch_type:
                    found.update(branch_type)
            if found:
                return tuple(sorted(found))
    return None


def collect_enums(root, node, at="", depth=0, out=None):
    if out is None:
        out = {}
    if depth > 64 or not isinstance(node, dict):
        return out
    node = resolve_ref(root, node, depth)
    if not isinstance(node, dict):
        return out
    if isinstance(node.get("enum"), list):
        out[at] = list(node["enum"])
    elif "const" in node:
        out.setdefault(at, []).append(node["const"])
    props = node.get("properties")
    if isinstance(props, dict):
        for key, child in props.items():
            escaped = key.replace("~", "~0").replace("/", "~1")
            collect_enums(root, child, f"{at}/{escaped}", depth + 1, out)
    items = node.get("items")
    if isinstance(items, dict):
        collect_enums(root, items, f"{at}/-", depth + 1, out)
    for key in ("allOf", "anyOf", "oneOf"):
        branches = node.get(key)
        if isinstance(branches, list):
            for branch in branches:
                collect_enums(root, branch, at, depth + 1, out)
    return out


def branch_key(root, branch, index):
    """A `oneOf`/`anyOf` branch's identity: its `op` const when it has one
    (the graph's change operations), otherwise its position."""
    node = resolve_ref(root, branch)
    props = node.get("properties") if isinstance(node, dict) else None
    op = props.get("op") if isinstance(props, dict) else None
    if isinstance(op, dict) and "const" in op:
        return f"op={canonical(op['const'])}"
    return str(index)


def walk_branches(op_id, side, base_root, base_node, cur_root, cur_node, path, issues, check, depth):
    """Pair each base branch of `oneOf`/`anyOf` with the current one of the
    same key and check the pair; an input variant that is gone is breaking
    (a payload it accepted is now refused)."""
    for key in ("oneOf", "anyOf"):
        base_branches = base_node.get(key)
        if not isinstance(base_branches, list):
            continue
        cur_branches = cur_node.get(key) if isinstance(cur_node, dict) else None
        if not isinstance(cur_branches, list):
            cur_branches = []
        current = {branch_key(cur_root, b, i): b for i, b in enumerate(cur_branches)}
        for i, branch in enumerate(base_branches):
            name = branch_key(base_root, branch, i)
            label = f"{path}/{key}[{name}]"
            if name not in current:
                if side == "input":
                    issues.append(f"{op_id}: input variant removed: {label}")
                continue
            check(op_id, base_root, branch, cur_root, current[name], label, issues, depth + 1)


def check_output_schema(op_id, base_root, base_node, cur_root, cur_node, path, issues, depth=0):
    if depth > 64:
        return
    base_node = resolve_ref(base_root, base_node, depth)
    cur_node = resolve_ref(cur_root, cur_node, depth)
    if not isinstance(base_node, dict):
        return
    cur_props = cur_node.get("properties") if isinstance(cur_node, dict) else None
    if not isinstance(cur_props, dict):
        cur_props = {}
    base_props = base_node.get("properties")
    if isinstance(base_props, dict):
        for prop_name in sorted(base_props):
            base_prop = base_props[prop_name]
            prop_path = f"{path}/{prop_name}"
            if prop_name not in cur_props:
                issues.append(f"{op_id}: output property removed: {prop_path}")
                continue
            cur_prop = cur_props[prop_name]
            base_t = schema_type(base_root, base_prop, depth + 1)
            cur_t = schema_type(cur_root, cur_prop, depth + 1)
            if base_t is not None and base_t != cur_t:
                issues.append(f"{op_id}: output property {prop_path} type changed: {base_t} -> {cur_t}")
            check_output_schema(op_id, base_root, base_prop, cur_root, cur_prop, prop_path, issues, depth + 1)
    if isinstance(base_node.get("items"), dict) and isinstance(cur_node, dict) and isinstance(cur_node.get("items"), dict):
        check_output_schema(op_id, base_root, base_node["items"], cur_root, cur_node["items"], f"{path}/-", issues, depth + 1)
    walk_branches(op_id, "output", base_root, base_node, cur_root, cur_node, path, issues, check_output_schema, depth)


def check_input_schema(op_id, base_root, base_node, cur_root, cur_node, path, issues, depth=0):
    if depth > 64:
        return
    base_node = resolve_ref(base_root, base_node, depth)
    cur_node = resolve_ref(cur_root, cur_node, depth)
    if not isinstance(base_node, dict) or not isinstance(cur_node, dict):
        return
    base_req = set(base_node.get("required") or [])
    cur_req = set(cur_node.get("required") or [])
    for req in sorted(cur_req - base_req):
        issues.append(f"{op_id}: input property newly required: {path}/{req}")
    base_props = base_node.get("properties")
    cur_props = cur_node.get("properties")
    if isinstance(base_props, dict) and isinstance(cur_props, dict):
        for prop_name in sorted(base_props):
            if prop_name not in cur_props:
                continue
            prop_path = f"{path}/{prop_name}"
            base_prop = base_props[prop_name]
            cur_prop = cur_props[prop_name]
            base_t = schema_type(base_root, base_prop, depth + 1)
            cur_t = schema_type(cur_root, cur_prop, depth + 1)
            if base_t is not None and base_t != cur_t:
                issues.append(f"{op_id}: input property {prop_path} type changed: {base_t} -> {cur_t}")
            check_input_schema(op_id, base_root, base_prop, cur_root, cur_prop, prop_path, issues, depth + 1)
    if isinstance(base_node.get("items"), dict) and isinstance(cur_node.get("items"), dict):
        check_input_schema(op_id, base_root, base_node["items"], cur_root, cur_node["items"], f"{path}/-", issues, depth + 1)
    walk_branches(op_id, "input", base_root, base_node, cur_root, cur_node, path, issues, check_input_schema, depth)


def load_base_snapshot():
    override = os.environ.get("KEEPER_CATALOGUE_BASE")
    if override is not None:
        if override == "none":
            print("no catalogue snapshot at none: compatibility check passes (XR-6)")
            return None
        return json.loads(Path(override).read_text(encoding="utf-8"))
    tags = git("tag", "--list", "v*", "--sort=-v:refname")
    tag = next((line.strip() for line in tags.splitlines() if line.strip()), None)
    if not tag:
        print("no catalogue snapshot at (no v* tag): compatibility check passes (XR-6)")
        return None
    # Absent at the tag (a release before the catalogue) passes; any other
    # git failure is an error, never "no snapshot".
    listed = git("ls-tree", "--name-only", tag, "--", "catalogue/snapshot.json")
    if not listed.strip():
        print(f"no catalogue snapshot at {tag}: compatibility check passes (XR-6)")
        return None
    return json.loads(git("show", f"{tag}:catalogue/snapshot.json"))


def git(*args):
    run = subprocess.run(["git", "-C", str(ROOT), *args], text=True, capture_output=True)
    if run.returncode != 0:
        raise SystemExit(
            f"catalogue compatibility check: git {' '.join(args)} failed ({run.returncode}): {run.stderr.strip()}"
        )
    return run.stdout


def load_breaking():
    if not BREAKING.is_file():
        return set()
    doc = tomllib.loads(BREAKING.read_text(encoding="utf-8"))
    excused = set()
    for entry in doc.get("breaking", []):
        op_id = entry.get("id")
        change = entry.get("change")
        reason = entry.get("reason")
        if isinstance(op_id, str) and isinstance(change, str) and isinstance(reason, str) and reason.strip():
            excused.add((op_id, change))
    return excused


def load_classifications():
    if not CLASSIFICATION.is_file():
        return set()
    doc = tomllib.loads(CLASSIFICATION.read_text(encoding="utf-8"))
    accepted = set()
    for entry in doc.get("classified", []):
        op_id = entry.get("id")
        change = entry.get("change")
        reviewer = entry.get("reviewer")
        reason = entry.get("reason")
        digest = entry.get("sha256")
        if (
            isinstance(op_id, str)
            and isinstance(change, str)
            and isinstance(reviewer, str) and reviewer.strip()
            and isinstance(reason, str) and reason.strip()
            and isinstance(digest, str) and digest.strip()
        ):
            accepted.add((op_id, change, digest))
    return accepted


def main():
    if not SNAPSHOT.is_file():
        raise SystemExit(f"missing catalogue snapshot: {SNAPSHOT}")
    cur_doc = json.loads(SNAPSHOT.read_text(encoding="utf-8"))
    cur_ops = {entry["id"]: entry for entry in cur_doc.get("operations", [])}
    base_doc = load_base_snapshot()
    base_ops = {entry["id"]: entry for entry in base_doc.get("operations", [])} if base_doc is not None else None

    errors = []

    if base_ops is not None:
        excused = load_breaking()
        for op_id in sorted(base_ops):
            base_op = base_ops[op_id]
            if op_id not in cur_ops:
                if (op_id, "removed") not in excused:
                    errors.append(f"{op_id}: operation removed (breaking change 'removed')")
                continue
            cur_op = cur_ops[op_id]
            if base_op.get("kind") != cur_op.get("kind"):
                if (op_id, "kind") not in excused:
                    errors.append(f"{op_id}: kind changed {base_op.get('kind')} -> {cur_op.get('kind')}")
            base_rank = EFFECT_RANK.get(base_op.get("effect"))
            cur_rank = EFFECT_RANK.get(cur_op.get("effect"))
            if base_rank is not None and cur_rank is not None and cur_rank < base_rank:
                if (op_id, "effect") not in excused:
                    errors.append(f"{op_id}: effect became less strict {base_op.get('effect')} -> {cur_op.get('effect')}")
            output_issues = []
            check_output_schema(
                op_id, base_op.get("output", {}), base_op.get("output", {}),
                cur_op.get("output", {}), cur_op.get("output", {}), "", output_issues,
            )
            if output_issues and (op_id, "output") not in excused:
                errors.extend(output_issues)
            input_issues = []
            check_input_schema(
                op_id, base_op.get("input", {}), base_op.get("input", {}),
                cur_op.get("input", {}), cur_op.get("input", {}), "", input_issues,
            )
            if input_issues and (op_id, "input") not in excused:
                errors.extend(input_issues)
            base_out_enums = collect_enums(base_op.get("output", {}), base_op.get("output", {}))
            cur_out_enums = collect_enums(cur_op.get("output", {}), cur_op.get("output", {}))
            enum_issues = []
            for ptr in sorted(base_out_enums):
                cur_set = {canonical(v) for v in cur_out_enums.get(ptr, [])}
                for value in base_out_enums[ptr]:
                    if canonical(value) not in cur_set:
                        enum_issues.append(f"{op_id}: output enum value removed at {ptr or '/'}: {value!r}")
            if enum_issues and (op_id, "enum") not in excused:
                errors.extend(enum_issues)

    accepted = load_classifications()
    class_base = base_ops if base_ops is not None else {}

    def require_classification(op_id, change, part):
        expected = part_sha256(part)
        if (op_id, change, expected) not in accepted:
            errors.append(
                f"{op_id}: missing classification for change='{change}' (expected sha256={expected})"
            )

    for op_id in sorted(cur_ops):
        cur_op = cur_ops[op_id]
        if op_id not in class_base:
            if cur_op.get("effect") in ("read", "write"):
                require_classification(op_id, "added", {k: cur_op.get(k) for k in ADDED_PART_KEYS})
        else:
            base_op = class_base[op_id]
            if cur_op.get("effect") != base_op.get("effect"):
                require_classification(op_id, "class", cur_op.get("effect"))
            if any(cur_op.get(k) != base_op.get(k) for k in WIRE_RULE_KEYS):
                require_classification(op_id, "wire_rules", {k: cur_op.get(k) for k in WIRE_RULE_KEYS})
            if cur_op.get("proposal") != base_op.get("proposal"):
                require_classification(op_id, "proposal", cur_op.get("proposal"))
            if cur_op.get("vectors") != base_op.get("vectors"):
                require_classification(op_id, "vectors", cur_op.get("vectors"))
            base_in_enums = base_op.get("input_enums") or {}
            cur_in_enums = cur_op.get("input_enums") or {}
            enum_added = False
            for ptr, values in cur_in_enums.items():
                base_set = {canonical(v) for v in base_in_enums.get(ptr, [])}
                if any(canonical(v) not in base_set for v in values):
                    enum_added = True
                    break
            if enum_added:
                require_classification(op_id, "enum", cur_in_enums)

    for op_id in sorted(cur_ops):
        vectors = cur_ops[op_id].get("vectors") or {}
        for rel_path in sorted(vectors):
            recorded = vectors[rel_path]
            disk_path = ROOT / rel_path
            if not disk_path.is_file():
                errors.append(f"{op_id}: vectors file missing on disk: {rel_path}")
                continue
            actual = hashlib.sha256(disk_path.read_bytes()).hexdigest()
            if actual != recorded:
                errors.append(
                    f"{op_id}: vectors file {rel_path} digest mismatch (snapshot={recorded}, disk={actual})"
                )

    if errors:
        raise SystemExit("\n".join(errors))
    print(f"catalogue compatibility and classification passed ({len(cur_ops)} operations)")


if __name__ == "__main__":
    main()
