#!/usr/bin/env python3
"""Fail closed on graph worker linkage, mobile reachability and unsafe inventory drift.

The graph worker `keeper-graph-query` carries exactly the eleven audited allowances U01-U11
(docs/constraints-and-limitations.md) and six named exports; `allocation.rs` and `native.rs`
stay byte-identical to the reviewed 110.1 sources. Protocol and driver carry no unsafe; core
carries only K01, the worker's parent-death `pre_exec` alone in its byte-pinned file.
"""
import hashlib
import json
import pathlib
import re
import signal
import subprocess
import sys
import tomllib

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
# A person's graph reader (`human-reads`) is compiled only for keeper's own headless driver:
# not in keeper-core's default closure, not enabled through `[workspace.dependencies]`, and
# enabled by no workspace member but keeper-graph-driver (the agent runtime must never obtain
# a HumanReader). Checked on the parsed manifests, then on cargo's resolved feature graph.
HUMAN = 'human-reads'


def default_closure(features):
    seen, todo = set(), list(features.get('default', []))
    while todo:
        name = todo.pop()
        if name in seen:
            continue
        seen.add(name)
        todo.extend(features.get(name, []))
    return seen


def dependency_tables(doc):
    for key in ('dependencies', 'dev-dependencies', 'build-dependencies'):
        yield doc.get(key, {})
    for target in doc.get('target', {}).values():
        for key in ('dependencies', 'dev-dependencies', 'build-dependencies'):
            yield target.get(key, {})


def enables_human_reads(doc, inherited):
    """Whether a member manifest enables keeper-core's human-reads."""
    for table in dependency_tables(doc):
        dep = table.get('keeper-core')
        if isinstance(dep, dict):
            if HUMAN in dep.get('features', []) or (dep.get('workspace') is True and inherited):
                return True
    return any(v in ('keeper-core/' + HUMAN, 'keeper-core?/' + HUMAN)
               for values in doc.get('features', {}).values() for v in values)


def human_reads_violations(root_doc, members):
    """`members`: {crate name: parsed manifest}. Every way human-reads reaches a non-driver."""
    found = []
    if HUMAN in default_closure(members['keeper-core'].get('features', {})):
        found.append('keeper-core enables human-reads by default')
    root_core = root_doc.get('workspace', {}).get('dependencies', {}).get('keeper-core')
    inherited = isinstance(root_core, dict) and HUMAN in root_core.get('features', [])
    if inherited:
        found.append('[workspace.dependencies] keeper-core enables human-reads')
    enablers = sorted(name for name, doc in members.items()
                      if name != 'keeper-core' and enables_human_reads(doc, inherited))
    if enablers != ['keeper-graph-driver']:
        found.append(f'human-reads enabled outside the headless driver: {enablers}')
    return found


ROOT_TEXT = MANIFEST.read_text()
LOCK = MANIFEST.parent / 'Cargo.lock'


def members():
    """Every workspace member, from cargo itself, by package name: wherever it lives."""
    meta = json.loads(subprocess.check_output(
        ['cargo', 'metadata', '--no-deps', '--format-version', '1', '--manifest-path', str(MANIFEST)],
        text=True))
    return {p['name']: pathlib.Path(p['manifest_path']) for p in meta['packages']
            if p['id'] in meta['workspace_members']}


def parsed(root_text, member_text):
    return tomllib.loads(root_text), {n: tomllib.loads(t) for n, t in member_text.items()}


MEMBER_PATH = members()
MEMBER_TEXT = {n: p.read_text() for n, p in MEMBER_PATH.items()}
violations = human_reads_violations(*parsed(ROOT_TEXT, MEMBER_TEXT))
if violations:
    raise SystemExit('; '.join(violations))
# Self-tests: each mutation of the (passing) real manifests must fail the guard (r2 review
# M1, M3, M5).
CORE = MEMBER_TEXT['keeper-core']
if 'human-reads = []\n' not in CORE or '[workspace.dependencies]\n' not in ROOT_TEXT:
    raise SystemExit('guard self-test anchors missing')
MUTANTS = {
    'M1 default after the feature': (ROOT_TEXT, {**MEMBER_TEXT, 'keeper-core': CORE.replace(
        'human-reads = []\n', 'human-reads = []\ndefault = ["human-reads"]\n')}),
    'M3 transitive default': (ROOT_TEXT, {**MEMBER_TEXT, 'keeper-core': CORE.replace(
        'human-reads = []\n', 'human-reads = []\ndefault = ["people"]\npeople = ["human-reads"]\n')}),
    'M5 workspace dependency': (ROOT_TEXT.replace(
        '[workspace.dependencies]\n',
        '[workspace.dependencies]\nkeeper-core = { path = "crates/keeper-core", features = ["human-reads"] }\n'),
        MEMBER_TEXT),
}
for label, (root_text, member_text) in MUTANTS.items():
    if not human_reads_violations(*parsed(root_text, member_text)):
        raise SystemExit(f'human-reads guard self-test not caught: {label}')


# The resolved graph, over every target, every feature and every edge kind (normal, build,
# dev): no member but the driver can compile keeper-core with human-reads. keeper-core
# itself is the default-closure check above (`--all-features` would trivially enable it).
def resolves_human_reads(member):
    run = subprocess.run(
        ['cargo', 'tree', '--manifest-path', str(MANIFEST), '-p', member,
         '-e', 'normal,build,dev,features', '--all-features', '--target', 'all',
         '-i', 'keeper-core', '--prefix', 'none'], text=True, capture_output=True)
    if run.returncode != 0:
        if 'did not match any packages' in run.stderr:
            return False  # the member does not depend on keeper-core
        raise SystemExit(f'cargo tree failed for {member}: {run.stderr.strip()}')
    return f'keeper-core feature "{HUMAN}"' in run.stdout


def resolved_violations(names):
    return [f'{member} resolves keeper-core human-reads: {enabled}'
            for member in sorted(names) if member != 'keeper-core'
            for enabled in [resolves_human_reads(member)]
            if enabled != (member == 'keeper-graph-driver')]


def all_violations():
    """Both layers over the members cargo enumerates now (used by the on-disk self-tests)."""
    path = members()
    text = {n: p.read_text() for n, p in path.items()}
    return (human_reads_violations(*parsed(MANIFEST.read_text(), text))
            + resolved_violations(path))


found = resolved_violations(MEMBER_PATH)
if found:
    raise SystemExit('; '.join(found))


class Mutation:
    """On-disk self-test edits, undone byte for byte on every exit path: normal return, an
    exception, SIGINT, SIGTERM or SIGHUP (then exit 128 + n). The lock file is always restored."""

    def __init__(self):
        self.saved = {LOCK: LOCK.read_bytes()}
        self.created = []

    def write(self, path, text):
        if path not in self.saved:
            if path.exists():
                self.saved[path] = path.read_bytes()
            else:
                self.created.append(path)
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(text)

    def restore(self):
        for path, data in self.saved.items():
            path.write_bytes(data)
        for path in reversed(self.created):
            path.unlink(missing_ok=True)
        for path in reversed(self.created):
            for parent in path.parents:
                if parent == MANIFEST.parent:
                    break
                try:
                    parent.rmdir()
                except OSError:
                    break
        self.saved, self.created = {}, []

    def __enter__(self):
        self.previous = {}

        def stop(number, _frame):
            self.restore()
            sys.exit(128 + number)

        for number in (signal.SIGTERM, signal.SIGHUP):
            self.previous[number] = signal.signal(number, stop)
        return self

    def __exit__(self, *_):
        try:
            self.restore()
        finally:
            for number, handler in self.previous.items():
                signal.signal(number, handler)


# Self-tests on disk, each of which must fail the guard:
# - N8 / N11 (r3 review): an indirect keeper-core feature implying human-reads, enabled from a
#   macOS-only table or from a non-default member feature; invisible to the manifest scan,
#   caught by the resolved check;
# - X3 (r4 review): a new workspace member outside `crates/` enabling human-reads, which pins
#   the enumeration of members by cargo rather than by directory.
MACOS = "[target.'cfg(target_os = \"macos\")'.dependencies]\n"
FULL = 'human-reads = []\nfull = ["human-reads"]\n'
OUTSIDE = MANIFEST.parent / 'kg1-guard-selftest/people'
SELF_TESTS = {
    'N8 indirect feature from a macOS table': lambda m: (
        m.write(MEMBER_PATH['keeper-core'], CORE.replace('human-reads = []\n', FULL)),
        m.write(MEMBER_PATH['keeper'], MEMBER_TEXT['keeper'].replace(
            MACOS, MACOS + 'keeper-core = { path = "../keeper-core", features = ["full"] }\n'))),
    'N11 indirect feature from a non-default member feature': lambda m: (
        m.write(MEMBER_PATH['keeper-core'], CORE.replace('human-reads = []\n', FULL)),
        m.write(MEMBER_PATH['keeper-agent'],
                MEMBER_TEXT['keeper-agent'] + '\n[features]\npeople = ["keeper-core/full"]\n')),
    'X3 a member outside crates/': lambda m: (
        m.write(MANIFEST, ROOT_TEXT.replace(
            'members = [', 'members = ["kg1-guard-selftest/people", ', 1)),
        m.write(OUTSIDE / 'Cargo.toml',
                '[package]\nname = "people"\nversion = "0.0.0"\nedition = "2021"\npublish = false\n\n'
                '[dependencies]\nkeeper-core = { path = "../../crates/keeper-core", features = ["human-reads"] }\n'),
        m.write(OUTSIDE / 'src/lib.rs', '')),
}
if FULL in CORE or 'members = [' not in ROOT_TEXT or MACOS not in MEMBER_TEXT['keeper'] or OUTSIDE.exists():
    raise SystemExit('guard self-test anchors missing')
for label, apply in SELF_TESTS.items():
    with Mutation() as mutation:
        apply(mutation)
        caught = bool(all_violations())
    if not caught:
        raise SystemExit(f'human-reads guard self-test not caught: {label}')
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
# K01 (owner amendment 2026-11-13): keeper-core's one unsafe item, the graph worker's
# parent-death `pre_exec`, alone in `graph/parent_death.rs`, whose reviewed bytes are pinned.
# No other keeper-core source names unsafe_code, and no other graph client file names unsafe.
K01 = CRATES / 'keeper-core/src/graph/parent_death.rs'
K01_SHA256 = 'a3687b5ccc0209643dbcf6a7d92aedba2b8cc98347a6cfe63d735d114993631a'
if hashlib.sha256(K01.read_bytes()).hexdigest() != K01_SHA256:
    raise SystemExit(f'K01 drifted from its reviewed bytes: {K01}')
k01 = strip(K01.read_text())
if (re.findall(r'#\s*!?\s*\[[^\]]*unsafe_code[^\]]*\]', k01) != ['#[allow(unsafe_code)]']
        or len(re.findall(r'\bunsafe\b', k01)) != 1
        or not re.search(r'#\[allow\(unsafe_code\)\]\s*pub\(crate\) fn die_with_parent\(', k01)):
    raise SystemExit('K01 is not exactly one allowance on die_with_parent with one unsafe block')
for path in (CRATES / 'keeper-core/src').rglob('*.rs'):
    if path == K01:
        continue
    text = strip(path.read_text())
    if 'unsafe_code' in text or ('/graph/' in str(path) and re.search(r'\bunsafe\b', text)):
        raise SystemExit(f'unsafe outside K01 in keeper-core: {path}')
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
# P1 measurement tooling (110.5). What this checks, and only this: the keeper-graph-*
# crates' examples directories hold exactly these two files; the protocol and driver
# manifests, parsed as TOML, declare no `bin` or `example` target and hold no string value
# naming an `examples` path component; and no file under those two crates' `src/` names the
# tooling. Other crates' manifests, packaging and build scripts are not inspected here.
MEASUREMENT = {'keeper-graph-protocol/examples/rc_corpus.rs', 'keeper-graph-driver/examples/rc_spawner.rs'}
examples = {str(p.relative_to(CRATES)) for p in CRATES.glob('*/examples/*') if p.parent.parent.name.startswith('keeper-graph')}
if examples != MEASUREMENT:
    raise SystemExit(f'graph example drift: {sorted(examples)}')
def strings(value):
    if isinstance(value, str):
        yield value
    elif isinstance(value, dict):
        for v in value.values():
            yield from strings(v)
    elif isinstance(value, list):
        for v in value:
            yield from strings(v)
for crate in ['keeper-graph-protocol', 'keeper-graph-driver']:
    manifest = tomllib.loads((CRATES / crate / 'Cargo.toml').read_text())
    if 'bin' in manifest or 'example' in manifest or any(
            'examples' in pathlib.PurePosixPath(s.replace('\\', '/')).parts for s in strings(manifest)):
        raise SystemExit(f'{crate} declares an example or bin target')
    for path in (CRATES / crate / 'src').rglob('*'):
        if path.is_file() and re.search(rb'examples/|rc_corpus|rc_spawner', path.read_bytes()):
            raise SystemExit(f'shipped source refers to measurement tooling: {path}')
print('graph protocol/worker/core/mobile dependency boundaries, exact 11/6 worker and K01 core unsafe inventory passed')
