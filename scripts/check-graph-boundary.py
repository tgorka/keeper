#!/usr/bin/env python3
"""Fail closed on graph worker linkage, mobile reachability and unsafe inventory drift.

The graph worker `keeper-graph-query` carries exactly the eleven audited allowances U01-U11
(docs/constraints-and-limitations.md) and six named exports; `allocation.rs` and `native.rs`
stay byte-identical to the reviewed 110.1 sources. Core, protocol and driver carry no unsafe.
"""
import hashlib
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

CRATES = ROOT / 'src-tauri/crates'
INVENTORY = {
    'keeper-graph-query/src/native.rs': {'register'},
    'keeper-graph-query/src/allocation.rs': {'install_sqlite', 'native_malloc', 'native_free', 'native_calloc',
                                             'native_realloc', 'native_strdup', 'native_strndup', 'sqlite_malloc',
                                             'sqlite_realloc', 'sqlite_size'},
}
# The reviewed sources of U01-U11 (110.1 accepted head 56196413). Any edit reopens the
# inventory review.
REVIEWED = {
    'keeper-graph-query/src/allocation.rs': 'ee40f960b218ee1845762e9ea928ea7b13f5702d5fe7da2698c8f527a80529a1',
    'keeper-graph-query/src/native.rs': 'cb11cf0ce0e3383019760f7aedda9b48c05cdf6a223c0b3ac74fd89bc9c04f13',
}
EXPORTS = sorted('keeper_graph_' + n for n in ['malloc', 'free', 'calloc', 'realloc', 'strdup', 'strndup'])
SAFE_CALLBACKS = {'sqlite_roundup', 'sqlite_init', 'sqlite_shutdown'}

def strip(source):
    """Blank comments and string/char literals (raw strings too), keeping offsets."""
    out, i, n = list(source), 0, len(source)
    def blank(a, b):
        for k in range(a, min(b, n)):
            if out[k] != '\n':
                out[k] = ' '
    raw = re.compile(r'b?r(#*)"')
    char = re.compile(r"'(\\.[^']*|[^\\'])'")
    while i < n:
        if source.startswith('//', i):
            j = source.find('\n', i); j = n if j < 0 else j; blank(i, j); i = j
        elif source.startswith('/*', i):
            depth, j = 1, i + 2
            while j < n and depth:
                if source.startswith('/*', j): depth += 1; j += 2
                elif source.startswith('*/', j): depth -= 1; j += 2
                else: j += 1
            blank(i, j); i = j
        elif (m := raw.match(source, i)) and not (i and (source[i - 1].isalnum() or source[i - 1] == '_')):
            end = source.find('"' + m.group(1), m.end())
            end = n if end < 0 else end + 1 + len(m.group(1))
            blank(i, end); i = end
        elif source[i] == '"':
            j = i + 1
            while j < n and source[j] != '"':
                j += 2 if source[j] == '\\' else 1
            blank(i, j + 1); i = j + 1
        elif m := char.match(source, i):
            blank(i, m.end()); i = m.end()
        else:
            i += 1
    return ''.join(out)

def block_end(text, start):
    depth = 0
    for k in range(start, len(text)):
        if text[k] == '{': depth += 1
        elif text[k] == '}':
            depth -= 1
            if depth == 0:
                return k + 1
    raise SystemExit('unbalanced braces')

def first_party(crate):
    return [p for p in (CRATES / crate).rglob('*.rs') if 'vendor' not in p.parts and 'target' not in p.parts]

for rel, digest in REVIEWED.items():
    if hashlib.sha256((CRATES / rel).read_bytes()).hexdigest() != digest:
        raise SystemExit(f'{rel} differs from its reviewed bytes')
if (CRATES / 'keeper-graph-query/examples').exists():
    raise SystemExit('the graph worker ships no examples')
if re.search(r'^\[(features|\[example\]\])', (CRATES / 'keeper-graph-query/Cargo.toml').read_text(), re.M):
    raise SystemExit('the graph worker has no cargo features and no examples')
# Every first-party Rust file of the graph crates (src, tests, build.rs), all cfgs
# alike: each `unsafe` token and export attribute must lie inside one of the eleven
# production functions; no other unsafe_code attribute form may exist.
allowances, exports = [], []
for crate in ['keeper-graph-query', 'keeper-graph-protocol', 'keeper-graph-driver']:
    for path in first_party(crate):
        rel = str(path.relative_to(CRATES))
        raw_source = path.read_text()
        text = strip(raw_source)
        spans = []
        for m in re.finditer(r'#\s*!?\s*\[[^\]]*unsafe_code[^\]]*\]', text):
            if m.group(0) != '#[allow(unsafe_code)]':
                raise SystemExit(f'non-inventory unsafe_code attribute in {rel}: {m.group(0)}')
            item = re.compile(r'\s*(?:#\[[^\]]*\]\s*)*(?:pub(?:\([^)]*\))?\s+)?(?:unsafe\s+)?(?:extern\s+\S*\s*)?fn\s+(\w+)').match(text, m.end())
            if not item:
                raise SystemExit(f'unsafe_code allowance not on a function in {rel}')
            name = item.group(1)
            if name in INVENTORY.get(rel, set()):
                allowances.append(name)
            else:
                raise SystemExit(f'allowance outside the approved inventory: {rel}::{name}')
            spans.append((m.start(), block_end(text, text.index('{', item.end()))))
        def inside(position):
            return any(a <= position < b for a, b in spans)
        for m in re.finditer(r'\bunsafe\b', text):
            if not inside(m.start()):
                raise SystemExit(f'unsafe outside an inventoried function in {rel} at byte {m.start()}')
        if re.search(r'\bunsafe\s+impl\b', text) or re.search(r'\bno_mangle\b', text):
            raise SystemExit(f'unsafe impl or no_mangle in {rel}')
        for m in re.finditer(r'export_name\s*=', text):
            if not inside(m.start()):
                raise SystemExit(f'export outside an inventoried function in {rel}')
            exports.append(re.compile(r'export_name\s*=\s*"([^"]+)"').match(raw_source, m.start()).group(1))
        for name in SAFE_CALLBACKS & set(re.findall(r'\bfn\s+(\w+)', text)):
            head = re.search(r'((?:#\[[^\]]*\]\s*)*)((?:pub\s+)?(?:unsafe\s+)?extern\s+\S*\s*fn\s+' + name + r')\b', text)
            if not head or 'unsafe' in head.group(0) or 'export' in head.group(1):
                raise SystemExit(f'safe callback {name} changed shape in {rel}')
for path in (CRATES / 'keeper-core/src/graph').rglob('*.rs'):
    if re.search(r'\bunsafe\b|unsafe_code', strip(path.read_text())):
        raise SystemExit(f'unsafe in credential-bearing graph client: {path}')
if len(allowances) != 11 or sorted(allowances) != sorted(set().union(*INVENTORY.values())):
    raise SystemExit(f'unsafe inventory drift: {sorted(allowances)}')
if sorted(exports) != EXPORTS:
    raise SystemExit(f'export inventory drift: {exports}')
worker = CRATES / 'keeper-graph-query'
for path in (worker / 'src').rglob('*.rs'):
    shipped = strip(path.read_text()).split('mod tests')[0]
    if 'std::thread' in shipped:
        raise SystemExit(f'thread use in shipped worker source: {path}')
    if 'set_limit(' in shipped and path.name == 'allocation.rs':
        raise SystemExit('Cap budget mutation in worker source')
# First-party native inputs: no C/C++/asm implementation outside the hashed vendor
# tree, and the redirect header only declares and maps.
native = [p for p in worker.rglob('*') if p.suffix in ('.c', '.cc', '.cpp', '.m', '.s', '.S')
          and 'vendor' not in p.parts and 'target' not in p.parts]
if native:
    raise SystemExit(f'first-party native implementation: {native}')
if '{' in strip((worker / 'native-allocation.h').read_text()):
    raise SystemExit('native-allocation.h must contain declarations and macros only')
ci = (ROOT/'.github/workflows/ci.yml').read_text()
line = next(line for line in ci.splitlines() if 'cargo check --workspace' in line and 'aarch64-apple-ios' in line)
if re.findall(r'--exclude\s+(\S+)', line) != ['keeper-graph-query','keeper-graph-driver']:
    raise SystemExit('iOS workspace exclusion drift')
print('graph protocol/worker/core/mobile dependency boundaries and exact 11/6 unsafe inventory passed')
