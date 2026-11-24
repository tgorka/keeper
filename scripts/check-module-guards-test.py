#!/usr/bin/env python3
"""On-disk acceptance tests. --ci runs the complete CI guard suite per case.

Fixtures use real Cargo resolution, never mocked metadata or tree output. The
core mutations restore original bytes on exceptions, SIGINT, SIGTERM and SIGHUP.
"""
import argparse
import json
import os
from pathlib import Path
import signal
import subprocess
import sys
import tempfile

ROOT = Path(__file__).resolve().parents[1]
SCRIPTS = ROOT / "scripts"
CI = argparse.ArgumentParser()
CI.add_argument("--ci", action="store_true")
ARGS = CI.parse_args()


def run(label, guard, expected=None, env=None):
    command = (["bash", str(SCRIPTS / "check-crate-graph.sh")] if ARGS.ci else
               [sys.executable, str(SCRIPTS / guard)])
    result = subprocess.run(command, cwd=ROOT, env=env, text=True,
                            stdout=subprocess.PIPE, stderr=subprocess.STDOUT)
    print(f"=== {label}: exit {result.returncode} ===\n{result.stdout}", flush=True)
    if expected is None:
        assert result.returncode == 0, label
    else:
        assert result.returncode != 0 and expected in result.stdout, label


def stop(number, _frame):
    raise SystemExit(128 + number)


for number in (signal.SIGINT, signal.SIGTERM, signal.SIGHUP):
    signal.signal(number, stop)

core = ROOT / "src-tauri/crates/keeper-core/src"
lib = core / "lib.rs"
original = lib.read_bytes()
scratch = core / "scratch.rs"
directory = core / "scratch"
assert not scratch.exists() and not directory.exists(), "scratch fixture already exists"
try:
    scratch.write_text("")
    lib.write_bytes(original + b"\npub mod scratch;\n")
    run("core module + file RED", "check-core-frozen.py", "scratch")
finally:
    lib.write_bytes(original)
    scratch.unlink(missing_ok=True)
run("core module reverted GREEN", "check-core-frozen.py")
try:
    directory.mkdir()
    run("core bare directory RED", "check-core-frozen.py", "scratch")
finally:
    directory.rmdir()
run("core directory reverted GREEN", "check-core-frozen.py")

with tempfile.TemporaryDirectory(prefix="keeper-module-guards-") as temporary:
    root = Path(temporary)
    names = {"keeper-scratch": "implementation", "keeper-core": "legacy",
             "keeper-sync": "legacy", "keeper-port": "port",
             "keeper-provider": "provider", "keeper-testkit": "composition-root",
             "keeper-paths": "path-leaf", "keeper-folders": "folder-leaf",
             "keeper-scratch-api": "contract", "keeper-app": "composition-root",
             "keeper-rt": "runtime", "keeper-tp": "transport", "keeper-ad": "adapter"}
    (root / "Cargo.toml").write_text(
        '[workspace]\nresolver = "2"\nmembers = ' + json.dumps(list(names)) +
        '\nexclude = ["bridge", "tokio", "serde", "serde_json", "rusqlite", "tonic", "prost", "hyper", "tower"]\n')
    roles = root / "roles.json"
    roles.write_text(json.dumps(names))
    env = dict(os.environ, KEEPER_MODULE_MANIFEST=str(root / "Cargo.toml"),
               KEEPER_MODULE_ROLES=str(roles), CARGO_NET_OFFLINE="true",
               CARGO_TERM_COLOR="never")

    def crate(name, dependency=None, kind="dependencies", feature=False):
        path = root / name
        path.mkdir(exist_ok=True)
        (path / "lib.rs").write_text("")
        text = f'[package]\nname = "{name}"\nversion = "0.0.0"\nedition = "2021"\n[lib]\npath = "lib.rs"\n'
        deps = [dependency] if isinstance(dependency, str) else list(dependency or [])
        if deps:
            text += f'[{kind}]\n'
            for dep in deps:
                text += f'{dep} = {{ path = "../{dep}"'
                text += ', optional = true }\n' if feature else ' }\n'
            if feature:
                text += '[features]\nextra = [' + ', '.join(f'"dep:{dep}"' for dep in deps) + ']\n'
        (path / "Cargo.toml").write_text(text)

    for name in [*names, "bridge", "tokio", "serde", "serde_json", "rusqlite", "tonic", "prost", "hyper", "tower"]:
        crate(name)
    run("fixture baseline GREEN", "check-module-edges.py", env=env)
    cases = [
        ("implementation normal core", "keeper-scratch", "keeper-core", "dependencies", False),
        ("implementation build provider", "keeper-scratch", "keeper-provider", "build-dependencies", False),
        ("path leaf port", "keeper-paths", "keeper-port", "dependencies", False),
        ("folder leaf port", "keeper-folders", "keeper-port", "dependencies", False),
        ("folder leaf sync", "keeper-folders", "keeper-sync", "dependencies", False),
        ("port provider", "keeper-port", "keeper-provider", "dependencies", False),
        ("legacy build provider", "keeper-core", "keeper-provider", "build-dependencies", False),
        ("optional provider", "keeper-port", "keeper-provider", "dependencies", True),
        ("target provider", "keeper-core", "keeper-provider", 'target.\'cfg(target_os = "windows")\'.dependencies', False),
        ("runtime implementation", "keeper-rt", "keeper-scratch", "dependencies", False),
        ("transport rusqlite", "keeper-tp", "rusqlite", "dependencies", False),
        ("transport tonic", "keeper-tp", "tonic", "dependencies", False),
        ("transport prost", "keeper-tp", "prost", "dependencies", False),
        ("transport hyper", "keeper-tp", "hyper", "dependencies", False),
        ("transport tower", "keeper-tp", "tower", "dependencies", False),
        ("transport port", "keeper-tp", "keeper-port", "dependencies", False),
        ("implementation runtime", "keeper-scratch", "keeper-rt", "dependencies", False),
        ("port transport", "keeper-port", "keeper-tp", "dependencies", False),
        ("provider adapter", "keeper-provider", "keeper-ad", "dependencies", False),
        ("contract runtime", "keeper-scratch-api", "keeper-rt", "dependencies", False),
        ("path leaf transport", "keeper-paths", "keeper-tp", "dependencies", False),
        ("folder leaf adapter", "keeper-folders", "keeper-ad", "dependencies", False),
        ("legacy runtime", "keeper-core", "keeper-rt", "dependencies", False),
    ]
    for label, name, dependency, kind, feature in cases:
        crate(name, dependency, kind, feature)
        run(label + " RED", "check-module-edges.py", dependency, env)
        crate(name)
        run(label + " reverted GREEN", "check-module-edges.py", env=env)
    del names["keeper-scratch"]
    roles.write_text(json.dumps(names))
    run("undeclared member RED", "check-module-edges.py", "keeper-scratch", env)
    names["keeper-scratch"] = "implementation"
    roles.write_text(json.dumps(names))
    run("undeclared member reverted GREEN", "check-module-edges.py", env=env)
    for name, dependency, kind in [
        ("keeper-scratch", "keeper-testkit", "dev-dependencies"),
        ("keeper-folders", "keeper-paths", "dependencies"),
        ("keeper-app", "keeper-provider", "dependencies"),
        ("keeper-core", "keeper-provider", "dev-dependencies"),
        ("keeper-rt", "keeper-scratch-api", "dependencies"),
        ("keeper-tp", ["keeper-scratch-api", "tokio", "serde_json"], "dependencies"),
    ]:
        crate(name, dependency, kind)
        run(f"allowed {name} {kind} {dependency} GREEN", "check-module-edges.py", env=env)
        crate(name)
    crate("keeper-scratch-api", "bridge")
    crate("bridge", "tokio")
    run("transitive contract tokio RED", "check-module-edges.py", "tokio", env)
    crate("bridge", "serde")
    run("transitive contract serde GREEN", "check-module-edges.py", env=env)
    crate("keeper-scratch-api")
    crate("bridge", "keeper-provider")
    crate("keeper-core", "bridge")
    run("transitive legacy provider RED", "check-module-edges.py", "keeper-provider", env)
    crate("keeper-core")
    run("transitive legacy reverted GREEN", "check-module-edges.py", env=env)
    # A real cargo parse error must remain an error, including its diagnostic.
    (root / "Cargo.toml").write_text("[invalid\n")
    run("cargo failure RED", "check-module-edges.py", "error: unclosed table, expected `]`", env)

print("module guard on-disk acceptance tests passed")
