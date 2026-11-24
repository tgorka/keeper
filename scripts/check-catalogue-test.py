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

        # 11. oneOf branches are paired by their `op` const and checked inside.
        def change_op(op_id):
            op = make_op(op_id, effect="write")
            op["input"] = {
                "type": "object",
                "properties": {
                    "ops": {
                        "type": "array",
                        "items": {
                            "oneOf": [
                                {
                                    "type": "object",
                                    "properties": {"op": {"const": "create_node"}, "title": {"type": "string"}},
                                    "required": ["op", "title"],
                                },
                                {
                                    "type": "object",
                                    "properties": {
                                        "op": {"const": "add_tag"},
                                        "expected_revision": {"type": ["null", "string"]},
                                    },
                                    "required": ["op"],
                                },
                            ]
                        },
                    }
                },
                "required": ["ops"],
            }
            return op

        one_base = Path(temporary) / "oneof-base.json"
        one_base.write_text(json.dumps({"operations": [change_op("graph.enqueue")]}), encoding="utf-8")

        def one_of(mutate):
            op = change_op("graph.enqueue")
            mutate(op["input"]["properties"]["ops"]["items"]["oneOf"])
            write_state([op])

        one_of(lambda branches: None)
        run_check(root, one_base, True, "passed (1 operations)", "oneOf unmutated GREEN")
        one_of(lambda branches: branches.reverse())
        run_check(root, one_base, True, "passed (1 operations)", "oneOf reordered GREEN")
        one_of(lambda branches: branches[0]["properties"]["title"].update(type="integer"))
        run_check(root, one_base, False, 'oneOf[op="create_node"]/title type changed', "oneOf retyped field RED")
        one_of(lambda branches: branches[1]["required"].append("expected_revision"))
        run_check(root, one_base, False, 'oneOf[op="add_tag"]/expected_revision', "oneOf newly required RED")
        one_of(lambda branches: branches[1]["properties"]["expected_revision"].update(type="string"))
        run_check(root, one_base, False, "type changed", "oneOf narrowed nullable RED")
        one_of(lambda branches: branches.pop(1))
        run_check(root, one_base, False, 'input variant removed: /ops/-/oneOf[op="add_tag"]', "oneOf variant removed RED")

        # 12. The real catalogue: its own snapshot as base passes; retyping a
        # change op's field or removing a graph op's output member fails.
        real = json.loads((ROOT / "catalogue/snapshot.json").read_text(encoding="utf-8"))
        real_root = Path(temporary) / "real"
        (real_root / "catalogue").mkdir(parents=True)
        for rel in {rel for op in real["operations"] for rel in (op.get("vectors") or {})}:
            (real_root / rel).parent.mkdir(parents=True, exist_ok=True)
            (real_root / rel).write_bytes((ROOT / rel).read_bytes())
        real_base = Path(temporary) / "real-base.json"
        real_base.write_text(json.dumps(real), encoding="utf-8")

        def real_state(mutate):
            doc = copy.deepcopy(real)
            ops = {op["id"]: op for op in doc["operations"]}
            mutate(ops)
            (real_root / "catalogue/snapshot.json").write_text(json.dumps(doc), encoding="utf-8")

        def change_branch(op, name):
            props = op["input"]["properties"]
            items = props.get("change", {}).get("properties", props)["ops"]["items"]
            return next(b for b in items["oneOf"] if b["properties"]["op"].get("const") == name)

        real_state(lambda ops: None)
        run_check(real_root, real_base, True, "catalogue compatibility and classification passed", "real unmutated GREEN")
        real_state(lambda ops: change_branch(ops["graph.apply"], "create_node")["properties"]["title"].update(type="integer"))
        run_check(real_root, real_base, False, 'graph.apply: input property /ops/-/oneOf[op="create_node"]/title type changed', "real retyped op field RED")
        real_state(lambda ops: change_branch(ops["graph.propose"], "add_tag")["required"].append("surprise"))
        run_check(real_root, real_base, False, 'graph.propose: input property newly required: /change/ops/-/oneOf[op="add_tag"]/surprise', "real newly required op field RED")
        real_state(lambda ops: ops["graph.history"]["output"]["properties"].pop("page"))
        run_check(real_root, real_base, False, "graph.history: output property removed: /page", "real removed output member RED")
        real_state(lambda ops: ops["graph.apply"]["output"]["properties"].pop("outcome"))
        run_check(real_root, real_base, False, "graph.apply: output property removed: /outcome", "real removed apply outcome RED")

        # 13. The base from git: no tag, a tag before the catalogue, a tag with
        # it, a missing object and a run outside a repository.
        repo = Path(temporary) / "git-repo"
        repo.mkdir()
        git_env = dict(os.environ, GIT_CEILING_DIRECTORIES=temporary)

        def git(*args):
            subprocess.run(
                ["git", "-C", str(repo), "-c", "user.name=t", "-c", "user.email=t@example.invalid",
                 "-c", "commit.gpgsign=false", "-c", "tag.gpgsign=false", *args],
                check=True, capture_output=True, env=git_env,
            )

        def run_git_check(expected_ok, expected_text, label):
            env = dict(git_env, KEEPER_CATALOGUE_ROOT=str(repo))
            env.pop("KEEPER_CATALOGUE_BASE", None)
            result = subprocess.run(
                [sys.executable, str(SCRIPT)], env=env, text=True,
                stdout=subprocess.PIPE, stderr=subprocess.STDOUT,
            )
            assert (result.returncode == 0) == expected_ok, f"{label}: rc {result.returncode}\n{result.stdout}"
            assert expected_text in result.stdout, f"{label} missing {expected_text!r} in:\n{result.stdout}"

        (repo / "catalogue").mkdir()
        (repo / "vectors").mkdir()
        (repo / vec_rel).write_bytes(vec_bytes)
        (repo / "catalogue/classification.toml").write_text("\n".join(all_cls), encoding="utf-8")
        (repo / "README").write_text("x\n", encoding="utf-8")
        # Outside a repository: git itself fails, so the check fails.
        (repo / "catalogue/snapshot.json").write_text(json.dumps(base_doc), encoding="utf-8")
        run_git_check(False, "git tag --list v* --sort=-v:refname failed", "outside a repository RED")
        git("init", "-q")
        git("add", "README")
        git("commit", "-q", "-m", "before the catalogue")
        run_git_check(True, "no catalogue snapshot at (no v* tag)", "no tag GREEN")
        git("tag", "v0.1.0")
        run_git_check(True, "no catalogue snapshot at v0.1.0", "tag before the catalogue GREEN")
        git("add", "catalogue", "vectors")
        git("commit", "-q", "-m", "catalogue")
        git("tag", "v0.2.0")
        run_git_check(True, "catalogue compatibility and classification passed", "tag with the catalogue GREEN")
        mutated = copy.deepcopy(base_doc)
        del mutated["operations"][0]["output"]["properties"]["status"]
        (repo / "catalogue/snapshot.json").write_text(json.dumps(mutated), encoding="utf-8")
        run_git_check(False, "app.alpha: output property removed: /status", "base read through git RED")
        blob = subprocess.run(
            ["git", "-C", str(repo), "rev-parse", "v0.2.0:catalogue/snapshot.json"],
            check=True, capture_output=True, text=True, env=git_env,
        ).stdout.strip()
        (repo / ".git/objects" / blob[:2] / blob[2:]).unlink()
        run_git_check(False, "git show v0.2.0:catalogue/snapshot.json failed", "missing object RED")

    print("catalogue self-tests passed")


if __name__ == "__main__":
    main()
