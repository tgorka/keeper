#!/usr/bin/env python3
"""Self-tests for scripts/check-catalogue.py using isolated tempdir fixtures."""
import copy
import hashlib
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile

ROOT = Path(__file__).resolve().parents[1]
SCRIPT = ROOT / "scripts/check-catalogue.py"


def canonical(value):
    return json.dumps(value, sort_keys=True, separators=(",", ":"))


def part_sha256(part):
    return hashlib.sha256(canonical(part).encode("utf-8")).hexdigest()


def sha256_bytes(data):
    return hashlib.sha256(data).hexdigest()


def make_op(op_id, *, effect="read", vector_rel=None, vector_digest=None):
    vectors = {vector_rel: vector_digest} if vector_rel else {}
    return {
        "id": op_id,
        "kind": "query" if effect == "read" else "command",
        "effect": effect,
        "summary": f"summary for {op_id}",
        "scope": "app",
        "proposal": "none",
        "input": {
            "type": "object",
            "properties": {
                "name": {"type": "string"},
                "limit": {"type": "integer"},
            },
            "required": ["name"],
        },
        "output": {
            "type": "object",
            "properties": {
                "ok": {"type": "boolean"},
                "status": {"type": "string", "enum": ["ready", "degraded"]},
            },
            "required": ["ok", "status"],
        },
        "input_enums": {},
        "concise": [],
        "page": None,
        "cli": {"words": [op_id], "positional": []},
        "wire_forbid": [],
        "wire_fix": [],
        "to_proposal": False,
        "wire_gate": bool(vector_rel),
        "vectors": vectors,
    }


def run_check(root, base, expected_ok, expected_text=None, label=""):
    env = dict(os.environ, KEEPER_CATALOGUE_ROOT=str(root))
    if base is not None:
        env["KEEPER_CATALOGUE_BASE"] = str(base)
    else:
        env.pop("KEEPER_CATALOGUE_BASE", None)
    result = subprocess.run(
        [sys.executable, str(SCRIPT)],
        env=env,
        text=True,
        stdout=subprocess.PIPE,
        stderr=subprocess.STDOUT,
    )
    if expected_ok:
        assert result.returncode == 0, f"{label} failed unexpectedly:\n{result.stdout}"
    else:
        assert result.returncode != 0, f"{label} passed unexpectedly:\n{result.stdout}"
    if expected_text is not None:
        assert expected_text in result.stdout, f"{label} missing {expected_text!r} in:\n{result.stdout}"
    return result.stdout


def main():
    with tempfile.TemporaryDirectory(prefix="keeper-catalogue-test-") as temporary:
        root = Path(temporary) / "repo"
        catalogue = root / "catalogue"
        vectors_dir = root / "vectors"
        catalogue.mkdir(parents=True)
        vectors_dir.mkdir(parents=True)

        vec_rel = "vectors/gate.json"
        vec_bytes = b'{"cases": [1]}\n'
        (root / vec_rel).write_bytes(vec_bytes)
        vec_digest = sha256_bytes(vec_bytes)

        base_ops = [
            make_op("app.alpha", effect="read"),
            make_op("graph.apply", effect="write", vector_rel=vec_rel, vector_digest=vec_digest),
        ]
        base_doc = {"operations": base_ops}
        base_path = Path(temporary) / "base-snapshot.json"
        base_path.write_text(json.dumps(base_doc, indent=2), encoding="utf-8")

        def write_state(ops, breaking="", classification=""):
            (catalogue / "snapshot.json").write_text(
                json.dumps({"operations": ops}, indent=2), encoding="utf-8"
            )
            (catalogue / "breaking.toml").write_text(breaking, encoding="utf-8")
            (catalogue / "classification.toml").write_text(classification, encoding="utf-8")

        # 1. Unmutated fixture passes.
        write_state(copy.deepcopy(base_ops))
        run_check(root, base_path, True, "catalogue compatibility and classification passed (2 operations)", "unmutated baseline")

        # 2. Removing an op fails naming the op.
        ops = [copy.deepcopy(base_ops[1])]
        write_state(ops)
        run_check(root, base_path, False, "app.alpha", "removed op RED")

        # 3. Removal named in breaking.toml passes.
        breaking = (
            '[[breaking]]\n'
            'id = "app.alpha"\n'
            'change = "removed"\n'
            'reason = "retired in fixture"\n'
        )
        write_state(ops, breaking=breaking)
        run_check(root, base_path, True, "passed (1 operations)", "removed op excused GREEN")

        # 4. Removing an output field fails naming the op.
        ops = copy.deepcopy(base_ops)
        del ops[0]["output"]["properties"]["ok"]
        write_state(ops)
        run_check(root, base_path, False, "app.alpha", "removed output field RED")

        # 5. Retyping an input field fails naming the op.
        ops = copy.deepcopy(base_ops)
        ops[0]["input"]["properties"]["limit"]["type"] = "string"
        write_state(ops)
        run_check(root, base_path, False, "app.alpha", "retyped input field RED")

        # 6. Adding a required input field fails naming the op.
        ops = copy.deepcopy(base_ops)
        ops[0]["input"]["required"].append("limit")
        write_state(ops)
        run_check(root, base_path, False, "app.alpha", "newly required input field RED")

        # 7. Adding a read op without a classification line fails; with the line passes.
        added = make_op("app.beta", effect="read")
        ops = copy.deepcopy(base_ops) + [added]
        write_state(ops)
        run_check(root, base_path, False, "app.beta", "added read op unclassified RED")

        added_part = {
            k: added[k]
            for k in ("effect", "proposal", "wire_forbid", "wire_fix", "wire_gate", "to_proposal", "vectors", "input_enums")
        }
        cls = (
            '[[classified]]\n'
            'id = "app.beta"\n'
            'change = "added"\n'
            'reviewer = "alice"\n'
            'reason = "read-only metadata"\n'
            f'sha256 = "{part_sha256(added_part)}"\n'
        )
        write_state(ops, classification=cls)
        run_check(root, base_path, True, "passed (3 operations)", "added read op classified GREEN")

        # 8. Changing wire_forbid without a classification line fails.
        ops = copy.deepcopy(base_ops)
        ops[1]["wire_forbid"] = ["/secret"]
        write_state(ops)
        run_check(root, base_path, False, "graph.apply", "wire_forbid unclassified RED")

        # 9. Changing a wire_gate vectors file (and its digest) without a line fails.
        new_vec_bytes = b'{"cases": [1, 2]}\n'
        (root / vec_rel).write_bytes(new_vec_bytes)
        new_vec_digest = sha256_bytes(new_vec_bytes)
        ops = copy.deepcopy(base_ops)
        ops[1]["vectors"][vec_rel] = new_vec_digest
        write_state(ops)
        run_check(root, base_path, False, "graph.apply", "vectors digest unclassified RED")

        # Restore original vector bytes for the no-base check.
        (root / vec_rel).write_bytes(vec_bytes)

        # 10. No base snapshot (KEEPER_CATALOGUE_BASE=none) passes with XR-6 sentence.
        # With no base, every read/write op needs an 'added' classification entry.
        all_cls = []
        for op in base_ops:
            part = {
                k: op[k]
                for k in ("effect", "proposal", "wire_forbid", "wire_fix", "wire_gate", "to_proposal", "vectors", "input_enums")
            }
            all_cls.append(
                '[[classified]]\n'
                f'id = "{op["id"]}"\n'
                'change = "added"\n'
                'reviewer = "alice"\n'
                'reason = "initial catalogue"\n'
                f'sha256 = "{part_sha256(part)}"\n'
            )
        write_state(copy.deepcopy(base_ops), classification="\n".join(all_cls))
        run_check(
            root,
            "none",
            True,
            "compatibility check passes (XR-6)",
            "no base snapshot XR-6 GREEN",
        )

    print("catalogue self-tests passed")


if __name__ == "__main__":
    main()
