#!/usr/bin/env python3
"""Fail closed on graph worker linkage, mobile reachability and unsafe inventory drift."""
import pathlib
import re
import subprocess

ROOT = pathlib.Path(__file__).resolve().parents[1]
MANIFEST = ROOT / 'src-tauri/Cargo.toml'

def tree(package, target='all'):
    return subprocess.check_output(['cargo', 'tree', '--manifest-path', str(MANIFEST), '-p', package,
                                    '--all-features', '--target', target, '-e', 'normal,build', '--prefix', 'none'], text=True)

def reject(text, pattern):
    bad = [line for line in text.splitlines() if re.match(pattern + r' v', line)]
    if bad:
        raise SystemExit('forbidden graph dependency: ' + ', '.join(bad))

reject(tree('keeper-graph-protocol'), r'(keeper-(core|graph-query|graph-driver)|matrix-sdk.*|syncular.*|rusqlite|libsqlite3-sys|tauri.*|reqwest|hyper|tokio|cap|nix|landlock|seccompiler)')
reject(tree('keeper-graph-query'), r'(keeper-core|matrix-sdk.*|syncular.*|tauri.*|tokio|rayon.*)')
for package in ['keeper-core', 'keeper-graph-driver']:
    reject(tree(package), r'(keeper-graph-query|graphqlite.*)')
for target in ['aarch64-apple-ios', 'aarch64-linux-android']:
    for package in ['keeper', 'keeper-core', 'keeper-agent', 'keeper-graph-protocol']:
        reject(tree(package, target), r'(keeper-graph-query|keeper-graph-driver|graphqlite.*|cap|landlock|seccompiler)')
    reject(tree('keeper-graph-query', target), r'(rusqlite|libsqlite3-sys|cap|nix|rustix|landlock|seccompiler)')

worker = ROOT / 'src-tauri/crates/keeper-graph-query/src'
actual = []
exports = []
for path in worker.rglob('*.rs'):
    source = path.read_text()
    actual += re.findall(r'#\[allow\(unsafe_code\)\]\s*(?:#\[[^\n]+\]\s*)*(?:pub(?:\([^)]*\))?\s+)?(?:unsafe\s+)?(?:extern\s+"C"\s+)?fn\s+(\w+)', source)
    exports += re.findall(r'export_name\s*=\s*"([^"]+)"', source)
    if 'std::thread' in source or 'set_limit(' in source and path.name == 'allocation.rs':
        raise SystemExit(f'thread/runtime budget mutation in {path}')
expected = {'register','install_sqlite','native_malloc','native_free','native_calloc','native_realloc','native_strdup','native_strndup','sqlite_malloc','sqlite_realloc','sqlite_size'}
if len(actual) != 11 or set(actual) != expected:
    raise SystemExit(f'unsafe inventory drift: {actual}')
if sorted(exports) != sorted('keeper_graph_' + n for n in ['malloc','free','calloc','realloc','strdup','strndup']):
    raise SystemExit(f'export inventory drift: {exports}')
for name in ['keeper-graph-protocol/src','keeper-graph-driver/src','keeper-core/src/graph']:
    for path in (ROOT/'src-tauri/crates'/name).rglob('*.rs'):
        if re.search(r'\bunsafe\s*(?:\{|fn|impl|extern|\()|#\[allow\(unsafe_code\)\]', path.read_text()):
            raise SystemExit(f'unsafe outside graph worker: {path}')
ci = (ROOT/'.github/workflows/ci.yml').read_text()
line = next(line for line in ci.splitlines() if 'cargo check --workspace' in line and 'aarch64-apple-ios' in line)
if re.findall(r'--exclude\s+(\S+)', line) != ['keeper-graph-query','keeper-graph-driver']:
    raise SystemExit('iOS workspace exclusion drift')
print('graph protocol/worker/core/mobile dependency boundaries and exact 11/6 unsafe inventory passed')
