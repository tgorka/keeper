#!/usr/bin/env python3
"""Isolated story 110.1 native graph proof (Linux / macOS headless).

Runs the real confined worker binaries and records JSON evidence. Every case
asserts its expected outcome; a failed expectation exits nonzero. No network,
credential or user-data access: scratch lives under a new directory given on
the command line, and the only listener is a loopback socket owned here.

usage: graph-proof.py <target-dir> <new-scratch-dir> [--benchmark]
"""
import json, os, platform, re, socket, subprocess, sys, tempfile, time

target, scratch = sys.argv[1], os.path.realpath(sys.argv[2])
benchmark = '--benchmark' in sys.argv[3:]
if os.path.exists(scratch):
    sys.exit('scratch must be new')
os.makedirs(scratch)
root = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
probe = os.path.realpath(os.path.join(target, 'examples', 'native_probe'))
worker = os.path.realpath(os.path.join(target, 'keeper-graph-query'))
driver = os.path.realpath(os.path.join(target, 'keeper-graph-driver'))
mac = platform.system() == 'Darwin'
env = {'GQL_STMT_CACHE': '0'}
if not mac:
    # P1: the supervisor's fixed Linux allocator setting; the worker requires it exactly.
    tunables = 'glibc.malloc.mmap_max=0:glibc.malloc.tcache_count=0'
    env['GLIBC_TUNABLES'] = tunables
evidence = {'platform': platform.platform(), 'machine': platform.machine(), 'command': sys.argv, 'cases': {}}

def sha256(path):
    import hashlib
    with open(path, 'rb') as f:
        return hashlib.sha256(f.read()).hexdigest()

def tool(*command):
    try:
        return subprocess.run(command, capture_output=True, text=True, cwd=root).stdout.strip().splitlines()[0]
    except (OSError, IndexError):
        return None

# Provenance: which source and binaries produced this evidence. The Mac copy has no
# .git, so first-party sources are hashed individually on every platform.
sources = {}
for base in ('src-tauri/crates/keeper-graph-protocol', 'src-tauri/crates/keeper-graph-query/src',
             'src-tauri/crates/keeper-graph-query/examples', 'src-tauri/crates/keeper-graph-query/build.rs',
             'src-tauri/crates/keeper-graph-query/Cargo.toml', 'src-tauri/Cargo.toml', 'rust-toolchain.toml',
             'src-tauri/crates/keeper-graph-query/native-allocation.h', 'src-tauri/crates/keeper-graph-driver',
             'src-tauri/crates/keeper-core/src/graph', 'scripts/graph-proof.py', 'scripts/alloc-interposer.c',
             'src-tauri/Cargo.lock', 'src-tauri/crates/keeper-graph-query/vendor/graphqlite/source-sha256.json',
             'src-tauri/crates/keeper-graph-query/vendor/graphqlite/generated-provenance.json'):
    path = os.path.join(root, base)
    files = [path] if os.path.isfile(path) else sorted(
        os.path.join(d, f) for d, _, fs in os.walk(path) for f in fs if not d.startswith(os.path.join(path, 'target')))
    for f in files:
        sources[os.path.relpath(f, root)] = sha256(f)
evidence['provenance'] = {
    'git_head': tool('git', 'rev-parse', 'HEAD'),
    'git_dirty_files': subprocess.run(['git', 'status', '--porcelain', '--', 'src-tauri', 'scripts', 'docs'], capture_output=True,
                                      text=True, cwd=root).stdout.splitlines() if os.path.isdir(os.path.join(root, '.git')) or
                       os.path.isfile(os.path.join(root, '.git')) else None,
    'rustc': tool('rustc', '+stable', '--version'), 'cargo': tool('cargo', '+stable', '--version'), 'cc': tool('cc', '--version'),
    # Not exhaustive build provenance: compiler/link flags and the bundled SQLite
    # object are not captured here.
    'build_commands': ['cargo +stable build --release --manifest-path src-tauri/Cargo.toml -p keeper-graph-query --bins --examples --features proof-executables',
                       'cargo +stable build --release --manifest-path src-tauri/Cargo.toml -p keeper-graph-driver --bins --examples'],
    'binaries': {name: sha256(path) for name, path in (
        ('keeper-graph-query', os.path.join(target, 'keeper-graph-query')), ('keeper-graph-driver', os.path.join(target, 'keeper-graph-driver')),
        ('native_probe', os.path.join(target, 'examples', 'native_probe')), ('fake_worker', os.path.join(target, 'examples', 'fake_worker')),
        ('allocation_probe', os.path.join(target, 'examples', 'allocation_probe')))},
    'sources': sources,
}
profiles = {}
if mac:
    source = open(os.path.join(root, 'src-tauri/crates/keeper-core/src/graph/mod.rs')).read()
    for name in ('READ', 'BUILD'):
        profiles[name] = re.search(r'const ' + name + r': &str = r#"(.*?)"#;', source, re.S).group(1)
    evidence['os_build'] = subprocess.check_output(['sw_vers'], text=True)

def run(args, db=None, role='READ', pass_fds=(), extra_env=None, timeout=10):
    command = [probe] + args
    if mac and db is not None:
        command = ['/usr/bin/sandbox-exec', '-p', profiles[role], '-D', 'WORKER=' + probe, '-D', 'DB=' + db] + command
    started = time.monotonic()
    process = subprocess.Popen(command, env=dict(env, **(extra_env or {})), stdout=subprocess.PIPE,
                               stderr=subprocess.PIPE, text=True, pass_fds=pass_fds)
    out, err = process.communicate(timeout=timeout)
    return {'exit': process.returncode, 'parent_pid': process.pid, 'stdout': out.strip(), 'stderr': err.strip()[-400:],
            'elapsed_ms': round((time.monotonic() - started) * 1000, 1)}

def case(name, result, ok, fatal=True):
    result['pass'] = bool(ok)
    evidence['cases'][name] = result
    if not ok and fatal:
        print(json.dumps(evidence, indent=1))
        sys.exit(f'proof case failed: {name}')

def fresh(name):
    path = os.path.join(scratch, name)
    open(path, 'wb').close()
    return path

generation = fresh('probe.db')
r = run(['build', generation], generation, 'BUILD')
case('confined_builder', r, r['exit'] == 0 and json.loads(r['stdout'])['pid'] == r['parent_pid'])
sidecars = [n for n in os.listdir(scratch) if n.endswith(('-journal', '-wal', '-shm'))]
with open(generation, 'rb') as f:
    header = f.read(20)
case('rollback_header_no_sidecars', {'header_18_19': list(header[18:20]), 'sidecars': sidecars},
     header[18:20] == b'\x01\x01' and not sidecars)
r = run(['query', generation, 'MATCH (n:Concept)-[e:contains]->(m) RETURN n.id AS node, e.id AS edge ORDER BY edge LIMIT 10'], generation)
rows = json.loads(r['stdout'])['rows'] if r['exit'] == 0 else None
case('confined_query_parallel_edges', r, rows == [{'node': '0' * 25 + '1', 'edge': '0' * 24 + '11'}, {'node': '0' * 25 + '1', 'edge': '0' * 24 + '12'}]
     and json.loads(r['stdout'])['pid'] == r['parent_pid'])
r = run(['query', generation, 'MATCH (n) RETURN count(*) AS total LIMIT 1'], generation, extra_env={'KEEPER_FAKE_TOKEN': 'synthetic'})
case('inherited_environment_refused', r, r['exit'] != 0)
if not mac:
    r = run(['query', generation, 'MATCH (n) RETURN count(*) AS total LIMIT 1'], generation, extra_env={'GLIBC_TUNABLES': tunables + ':glibc.malloc.arena_max=2'})
    case('altered_allocator_tunables_refused', r, r['exit'] != 0 and 'fixed settings' in r['stderr'])
    process = subprocess.run([probe, 'query', generation], input='', env={'GQL_STMT_CACHE': '0'}, capture_output=True, text=True, timeout=10)
    case('missing_allocator_tunables_refused', {'exit': process.returncode, 'stderr': process.stderr.strip()[-200:]},
         process.returncode != 0 and 'allocator tunables' in process.stderr)
# Every role requires GQL_STMT_CACHE=0 at the common entry (with the fixed tunables on
# Linux present): the probe names the refusal, the production worker exits 74 for all
# three roles before reading any frame or writing any result.
no_cache = {k: v for k, v in env.items() if k != 'GQL_STMT_CACHE'}
refusals = {}
for role in ('build', 'query'):
    db = fresh('no-cache-' + role + '.db') if role == 'build' else generation
    process = subprocess.run([probe, role, db], input='', env=no_cache, capture_output=True, text=True, timeout=10)
    refusals['probe_' + role] = {'exit': process.returncode, 'named': 'statement cache' in process.stderr}
for role in ('build', 'query', 'inspect'):
    db = fresh('no-cache-worker-' + role + '.db') if role == 'build' else generation
    process = subprocess.run([worker, role, db], input='', env=no_cache, capture_output=True, text=True, timeout=10)
    refusals['worker_' + role] = {'exit': process.returncode, 'stdout_bytes': len(process.stdout),
                                  'db_bytes': os.path.getsize(db) if role == 'build' else None}
case('missing_statement_cache_refused_every_role', refusals,
     all(v['exit'] != 0 and v['named'] for k, v in refusals.items() if k.startswith('probe_'))
     and all(v['exit'] == 74 and v['stdout_bytes'] == 0 for k, v in refusals.items() if k.startswith('worker_'))
     and refusals['worker_build']['db_bytes'] == 0)
# The worker's first statement makes any panic abort from the hook: no unwinding
# (libgcc never runs), no default-hook message or backtrace formatting. After Linux
# confinement seccomp denies tgkill (EPERM), so glibc 2.43 abort() falls through to its
# ABORT_INSTRUCTION (x86_64 hlt) and the kernel kills with SIGSEGV; macOS raises SIGABRT.
r = run(['panic-aborts', generation], generation)
unwound = run(['panic-unwinds', generation], generation)
case('panic_aborts_without_unwinding', {'with_hook': r, 'without_hook_control': unwound},
     r['exit'] == (-6 if mac else -11) and r['stderr'] == '' and r['stdout'] == ''
     and unwound['exit'] == 101 and 'panicked' in unwound['stderr'])
r = run(['wrong-thread'])
case('wrong_thread_sqlite_callback_exit_73', r, r['exit'] == 73)
r = run(['wrong-thread-native'])
case('wrong_thread_native_path_exit_73', r, r['exit'] == 73)
r = run(['live-statement'])
case('live_statement_registration_exit_73', r, r['exit'] == 73)
r = run(['repeat-entry'])
case('repeated_entry_exit_73', r, r['exit'] == 73)
r = run(['concurrent-entry'])
case('second_thread_entry_exit_73', r, r['exit'] == 73)
r = run(['thread-after-confine', generation], generation)
# Linux: seccomp KillProcess on clone ends the whole process with SIGSYS (31).
# macOS has no thread filter; the new thread's first allocation callback exits 73.
case('thread_after_confinement', r, r['exit'] == (73 if mac else -31))
oom = fresh('oom.db')
r = run(['oom', oom], oom, 'BUILD', timeout=30)
case('sqlite_allocation_budget_exit_72', r, r['exit'] == 72)
r = run(['authz', generation], generation)
authz = json.loads(r['stdout']) if r['exit'] == 0 else {}
# Every denial must be SQLITE_AUTH from the authorizer, not a missing function or
# syntax error; the fixed parent-facing statement still prepares.
case('authorizer_denials_are_sqlite_auth', r, r['exit'] == 0 and authz['denied'] and
     all(v == 'auth' for v in authz['denied'].values()) and authz['controls']['fixed_row_statement'] == 'allowed'
     and authz['controls']['missing_function'].startswith('other') and authz['controls']['syntax_error'].startswith('other'))
r = run(['startup-transition', generation], generation)
closed = json.loads(r['stdout']) if r['exit'] == 0 else {}
full = run(['retained-reprepare', generation, 'full'], generation)
reduced = run(['retained-reprepare', generation, 'reduced'], generation)
full_out = json.loads(full['stdout']) if full['exit'] == 0 else {}
reduced_out = json.loads(reduced['stdout']) if reduced['exit'] == 0 else {}
# R3 (inventory round 3). The literal "no retained statement" claim stays false and is
# recorded as an observation (sqlite3_close BUSY). The asserted invariant uses the
# statement GraphQLite actually retains: the warm-up is the admitted query's own engine
# text, so its reset helper statement is reused by the query. Under the full policy the
# query answers; with one of its reads removed the reuse re-prepares, is denied and
# fails the whole query, never an empty success.
case('startup_transition_reauthorizes_retained_statements',
     {'close': closed, 'close_busy_observed': closed.get('no_retained_statement') is False,
      'full_policy': full_out, 'reduced_policy': reduced_out, 'stderr': full['stderr'] + reduced['stderr']},
     full_out.get('rows') == 1 and reduced_out.get('rows') is None
     and 'native internal authorization failure' in (reduced_out.get('error') or '')
     and 'node_props_text' in (reduced_out.get('error') or ''))

# T12 (inventory round 3, R2(a)): direct per-symbol checks of the audited hooks in a
# fresh process each; refusals must exit with their code and print nothing.
allocation_probe = os.path.realpath(os.path.join(target, 'examples', 'allocation_probe'))
def allocation_case(name):
    process = subprocess.run([allocation_probe, name], env=env, capture_output=True, text=True, timeout=30)
    return {'exit': process.returncode, 'stdout': process.stdout.strip(), 'stderr': process.stderr.strip()[-300:]}
r = allocation_case('positive')
report = json.loads(r['stdout']) if r['exit'] == 0 else {}
case('allocation_hooks_positive', dict(r, report=report),
     r['exit'] == 0 and all(v is True for k, v in report.items() if k != 'residual_charged_bytes'))
families = ['malloc', 'malloc-zero', 'free-null', 'free-live', 'calloc', 'realloc-null', 'realloc-live', 'strdup', 'strndup',
            'sqlite-malloc', 'sqlite-free', 'sqlite-realloc', 'sqlite-size', 'sqlite-roundup', 'sqlite-init', 'sqlite-shutdown']
outcomes = {f: allocation_case('wrong-thread-' + f) for f in families}
case('allocation_hooks_refuse_wrong_thread', {'outcomes': outcomes},
     all(o['exit'] == 73 and not o['stdout'] for o in outcomes.values()))
outcomes = {name: allocation_case(name) for name in ('malloc-overflow', 'calloc-overflow', 'realloc-overflow')}
case('allocation_hooks_overflow_exit_72', {'outcomes': outcomes},
     all(o['exit'] == 72 and not o['stdout'] for o in outcomes.values()))
outcomes = {name: allocation_case(name) for name in ('sqlite-negative-malloc', 'sqlite-negative-roundup')}
case('allocation_hooks_negative_size_exit_73', {'outcomes': outcomes},
     all(o['exit'] == 73 and not o['stdout'] for o in outcomes.values()))

sentinel = os.path.join(scratch, 'sentinel-unrelated.csv')
with open(sentinel, 'w') as f:
    f.write('secret,value\nSENTINEL_SECRET,1\n')
before = open(sentinel).read()
for mode, role in (('native-file', 'READ'), ('native-file-build', 'BUILD')):
    db = generation if role == 'READ' else fresh(mode + '.db')
    r = run([mode, db, sentinel], db, role)
    case(mode + '_sentinel_denied', r, 'SENTINEL_SECRET' not in r['stdout'] + r['stderr'] and open(sentinel).read() == before)
canonical = os.path.join(scratch, 'canonical-replica.db')
with open(canonical, 'wb') as f:
    f.write(b'canonical')
for mode, role in (('canonical-write', 'READ'), ('canonical-write-build', 'BUILD')):
    db = generation if role == 'READ' else fresh(mode + '.db')
    r = run([mode, db, canonical], db, role)
    case(mode + '_canonical_denied', r, r['exit'] == 0 and json.loads(r['stdout']) == {'read_denied': True, 'file_open_denied': True, 'sqlite_write_denied': True}
         and open(canonical, 'rb').read() == b'canonical')

listener = socket.socket(); listener.bind(('127.0.0.1', 0)); listener.listen(4); listener.setblocking(False)
address = '127.0.0.1:%d' % listener.getsockname()[1]
r = run(['network', generation, address], generation)
try:
    listener.accept(); accepted = True
except BlockingIOError:
    accepted = False
case('network_listener_denied', dict(r, listener_accepted=accepted), r['exit'] == 0 and not accepted)

leak_file = os.open(sentinel, os.O_RDONLY)
leak_socket = socket.create_connection(listener.getsockname())
listener.settimeout(5)
peer, _ = listener.accept(); peer.setblocking(False)
os.dup2(leak_file, 40, inheritable=True); os.dup2(leak_socket.fileno(), 41, inheritable=True)
r = run(['fd-probe', generation, '40', '41'], generation, pass_fds=(40, 41))
try:
    received = peer.recv(1)
except BlockingIOError:
    received = b''
closed = json.loads(r['stdout'])['ebadf'] if r['exit'] == 0 else {}
case('inherited_descriptors_closed', dict(r, peer_received=len(received)),
     r['exit'] == 0 and closed == {'40': True, '41': True} and not received)

journal = fresh('journal.db')
r = run(['journal-create', journal], journal, 'BUILD')
out = json.loads(r['stdout']) if r['exit'] == 0 else {}
case('denied_sidecar_create', r, r['exit'] == 0 and out == {'denied': True, 'sidecar_exists': False})

import resource
fake = os.path.realpath(os.path.join(target, 'examples', 'fake_worker'))
for mode in ('oracle', 'limits') + (('benchmark',) if benchmark else ()):
    command = [driver, mode, worker, os.path.join(scratch, 'driver-' + mode)] + ([fake] if mode == 'limits' else [])
    started = time.monotonic()
    process = subprocess.run(command, capture_output=True, text=True, timeout=3600,
                             env=dict(os.environ, KEEPER_FAKE_TOKEN='parent-secret-must-not-reach-worker'))
    lines = [json.loads(line) for line in process.stdout.splitlines() if line.startswith('{')]
    # Max RSS of any waited descendant so far (driver or any reaped worker); macOS reports bytes.
    peak = resource.getrusage(resource.RUSAGE_CHILDREN).ru_maxrss
    case('driver_' + mode, {'exit': process.returncode, 'elapsed_s': round(time.monotonic() - started, 1), 'records': lines,
                            'cumulative_max_rss_kib_any_descendant': peak // 1024 if mac else peak,
                            'stderr_tail': process.stderr[-600:] if process.returncode else ''},
         process.returncode == 0 and lines and lines[-1].get('complete'))
    if mode == 'oracle':
        # Real SQLite failures injected at the builder's checked transaction boundaries
        # (savepoint begin, release, a projection write, commit) through safe hooks.
        export = os.path.join(scratch, 'driver-oracle', 'fixture-personal.ndjson')
        for point in ('savepoint', 'release', 'projection-write', 'commit'):
            db = fresh('fault-' + point + '.db')
            command = [probe, 'build-fault', db, point]
            if mac:
                command = ['/usr/bin/sandbox-exec', '-p', profiles['BUILD'], '-D', 'WORKER=' + probe, '-D', 'DB=' + db] + command
            with open(export, 'rb') as stdin:
                process = subprocess.run(command, stdin=stdin, env=env, capture_output=True, timeout=60)
            out = json.loads(process.stdout) if process.returncode == 0 else {}
            import sqlite3
            try:
                tables = [t for (t,) in sqlite3.connect('file:' + db + '?mode=ro', uri=True)
                          .execute("SELECT name FROM sqlite_schema WHERE type='table'")]
            except sqlite3.DatabaseError as error:
                tables = ['unreadable: ' + str(error)]
            sidecars = [n for n in os.listdir(scratch) if n.startswith('fault-' + point) and n != 'fault-' + point + '.db']
            case('builder_fault_' + point, {'exit': process.returncode, 'probe': out, 'committed_tables': tables, 'sidecars': sidecars},
                 out.get('refused') is True and '_kg_manifest' not in tables and not sidecars)
    if mode == 'limits':
        # Same queries straight to the worker: distinguish allocation (72) from result cap (75).
        directory = os.path.join(scratch, 'driver-limits', 'generations')
        limits_db = max((os.path.join(directory, n) for n in os.listdir(directory) if n.endswith('.db')), key=os.path.getsize)
        wide = ', '.join(f'n.P_626c6f62 AS b{i}' for i in range(16))
        for name, query, code in (
                ('native_materialization_exit_72', f'MATCH (n:Concept) WHERE n.P_626c6f62 IS NOT NULL RETURN {wide} LIMIT 1000', 72),
                ('encoded_result_cap_exit_75', 'MATCH (n:Concept) WHERE n.P_626c6f62 IS NOT NULL RETURN n.P_626c6f62 AS b LIMIT 100', 75)):
            command = [worker, 'query', limits_db]
            if mac:
                command = ['/usr/bin/sandbox-exec', '-p', profiles['READ'], '-D', 'WORKER=' + worker, '-D', 'DB=' + limits_db] + command
            process = subprocess.run(command, input=json.dumps({'source': query, 'parameters': {}}) + '\n',
                                     env=env, capture_output=True, text=True, timeout=10)
            case(name, {'exit': process.returncode, 'stdout_bytes': len(process.stdout)},
                 process.returncode == code and not process.stdout)

# F1: external allocation accounting. A measurement-only LD_PRELOAD interposer
# attributes every libc allocation to the worker executable (Cap-charged) or a shared
# library (uncharged bypass) and reports simultaneous peaks. The proof requires the
# uncharged remainder to be input-independent across a corpus varying rows, string
# length, numeric spelling, sort/count fan-out and allocation count; it does not pass
# on charged usage below 256 MiB alone. Measured last so a failure here does not hide
# the other evidence; it still fails the run.
hub = {'id': '0' * 25 + '1'}
corpus = (
    ('count_hub_10000', 'MATCH (h:Concept)-[e:contains]->(x) WHERE h.id = $id RETURN count(*) AS total LIMIT 1', hub),
    ('rows_1', 'MATCH (h:Concept)-[e:contains]->(x) WHERE h.id = $id RETURN x.id AS id LIMIT 1', hub),
    ('rows_1000_sorted', 'MATCH (h:Concept)-[e:contains]->(x) WHERE h.id = $id RETURN x.id AS id, x.title AS t ORDER BY id LIMIT 1000', hub),
    ('numeric_integer', 'MATCH (n:Concept) WHERE n.P_73636f7265 = $v RETURN n.id AS id LIMIT 1000', {'v': 1}),
    ('numeric_fraction', 'MATCH (n:Concept) WHERE n.P_73636f7265 = $v RETURN n.id AS id LIMIT 1000', {'v': 0.000123456789012345}),
    ('numeric_looking_string_8k', 'MATCH (n:Concept) WHERE n.title = $v RETURN n.id AS id LIMIT 1', {'v': '9' * 8000}),
    ('blob_rows_10', 'MATCH (n:Concept) WHERE n.P_626c6f62 IS NOT NULL RETURN n.P_626c6f62 AS b LIMIT 10', {}),
    ('blob_wide_16', 'MATCH (n:Concept) WHERE n.P_626c6f62 IS NOT NULL RETURN ' + ', '.join(f'n.P_626c6f62 AS b{i}' for i in range(16)) + ' LIMIT 1', {}),
    ('blob_sort_1000_wide_4', 'MATCH (n:Concept) WHERE n.P_626c6f62 IS NOT NULL RETURN ' + ', '.join(f'n.P_626c6f62 AS b{i}' for i in range(4)) + ' ORDER BY b0 DESC LIMIT 4', {}),
)
if mac:
    # SIP strips DYLD_* variables through /usr/bin/sandbox-exec, so no preload can
    # reach the confined worker; system-zone accounting on macOS needs another
    # reviewed mechanism. Recorded as an unproved obligation, never a pass.
    case('memory_uncharged_remainder', {'owed': 'macOS system-zone transient accounting', 'mechanism': None}, False, fatal=False)
else:
    library = os.path.join(scratch, 'alloc-interposer.so')
    source = os.path.join(root, 'scripts', 'alloc-interposer.c')
    subprocess.run(['cc', '-O2', '-Wall', '-Wextra', '-Werror', '-shared', '-fPIC', '-o', library, source, '-ldl'], check=True)
    # Observer self-test (built without builtin malloc/free assumptions so the sample
    # counters are re-read across calls): bootstrap alignment/extent/zeroing/membership,
    # overflow/exhaustion refusal, realloc copy boundaries, post-call samples after free
    # and every realloc outcome, and the deferred aligned-allocation sample. Under P1 the
    # D1 envelope holds; without it tcache-held chunks break it (negative control).
    selftest = os.path.join(scratch, 'alloc-interposer-selftest')
    subprocess.run(['cc', '-O2', '-fno-builtin', '-Wall', '-Wextra', '-Werror', '-DKG_INTERPOSER_SELFTEST', '-o', selftest, source, '-ldl'], check=True)
    process = subprocess.run([selftest], env={'GLIBC_TUNABLES': tunables}, capture_output=True, text=True, timeout=30)
    controls = json.loads(process.stdout) if process.returncode == 0 else {}
    unfixed = subprocess.run([selftest], env={}, capture_output=True, text=True, timeout=30)
    unfixed_controls = json.loads(unfixed.stdout) if unfixed.returncode == 0 else {}
    case('observer_bootstrap_self_test', {'exit': process.returncode, 'controls': controls,
                                          'without_p1_controls': unfixed_controls},
         process.returncode == 0 and controls and all(v == 1 for v in controls.values())
         and unfixed_controls.get('envelope_within_untracked') == 0
         and all(v == 1 for k, v in unfixed_controls.items() if k != 'envelope_within_untracked'))
    def observed(command, stdin_bytes, label):
        process = subprocess.run(command, input=stdin_bytes, env=dict(env, LD_PRELOAD=library),
                                 capture_output=True, timeout=600)
        stderr = process.stderr.decode(errors='replace')
        report = [json.loads(l[len('KG_ALLOC '):]) for l in stderr.splitlines() if l.startswith('KG_ALLOC ')]
        response = json.loads(process.stdout) if process.returncode == 0 else {}
        return dict(report[0] if report else {}, query=label, exit=process.returncode, rows=len(response.get('rows', [])),
                    worker_peak_charged_bytes=response.get('peak_allocated_bytes'))
    samples = [observed([worker, 'query', limits_db], (json.dumps({'source': query, 'parameters': params}) + '\n').encode(), name)
               for name, query, params in corpus]
    # Builder role over the accepted exports the driver already built and published
    # (the deliberately faulted exports are refused by design and are not measured).
    faults = ('-truncated', '-digest', '-before-image', '-dangling', '-gap', '-trailing')
    exports = sorted(os.path.join(dirpath, n) for dirpath, _, names in os.walk(scratch) for n in names
                     if n.endswith('.ndjson') and not n[:-len('.ndjson')].endswith(faults))
    build_samples, skipped_exports = [], []
    for export in exports:
        label = os.path.relpath(export, scratch)
        if os.path.getsize(export) > 128 * 1024 * 1024:
            skipped_exports.append(label)
            continue
        with open(export, 'rb') as f:
            stream = f.read()
        staged = fresh('observed-build-' + label.replace(os.sep, '-') + '.db')
        build_samples.append(observed([worker, 'build', staged], stream, 'build:' + label))
    every = samples + build_samples
    valid = all(s['exit'] == 0 and 'peak_uncharged_remainder_bytes' in s and s['table_overflow'] == 0 for s in every)
    remainder = [s.get('peak_uncharged_remainder_bytes', 0) for s in samples]
    library_bytes = [s.get('peak_library_bytes', 0) for s in samples]
    allocator_remainder = [s.get('peak_allocator_in_use_minus_charged_bytes', 0) for s in samples]
    # The interposer's charged peak must match Cap's own high-water mark: same domain.
    agree = all(s.get('peak_charged_bytes') == s.get('worker_peak_charged_bytes') for s in every)
    result = {'samples': samples, 'build_samples': build_samples, 'charged_accounting_agrees_with_cap': agree,
              'remainder_spread_bytes': max(remainder) - min(remainder),
              'library_bypass_spread_bytes': max(library_bytes) - min(library_bytes),
              'allocator_wide_remainder_spread_bytes': max(allocator_remainder) - min(allocator_remainder),
              'alignment_above_16_calls': sum(s.get('alignment_above_16_calls', 0) for s in every),
              # Separate series, never compared with each other (design §1).
              'max_live_below_32': max(s.get('peak_live_executable_allocations_below_32', 0) for s in every),
              'max_realloc_old_bytes': max(s.get('max_realloc_old_bytes', 0) for s in every),
              'max_reconstructed_overlap_bytes': max(s.get('peak_reconstructed_overlap_bytes', 0) for s in every),
              'max_mmapped_bytes': max(s.get('peak_mmapped_bytes', 0) for s in every),
              'bootstrap_refused': sum(s.get('bootstrap_refused', 0) for s in every)}
    # No acceptance window: until the reviewed accounted-cost envelope (R1) replaces
    # this assertion, any input-dependent uncharged remainder fails the proof.
    ok = (valid and agree and result['remainder_spread_bytes'] == 0 and result['library_bypass_spread_bytes'] == 0
          and result['allocator_wide_remainder_spread_bytes'] == 0 and result['alignment_above_16_calls'] == 0)
    case('memory_uncharged_remainder', result, ok, fatal=False)
    # Mechanism falsifier (design v3 D1 under P1), not the R1 bound: at every post-call
    # sample, allocator in-use <= 2*charged + 64*small32 + library chunks + untracked
    # pre-observer bytes; no mmapped chunk; no wide alignment; and no executable aligned
    # entry, so std's System realloc_fallback was never reached in these runs.
    d1 = {s['query']: {k: s.get(k) for k in ('peak_envelope_excess_bytes', 'untracked_in_use_at_start_bytes',
                                              'peak_mmapped_bytes', 'alignment_above_16_calls',
                                              'executable_aligned_calls', 'allocator_samples')} for s in every}
    case('d1_heap_envelope_under_p1', {'runs': d1, 'build_runs': len(build_samples), 'skipped_exports_over_128_mib': skipped_exports},
         valid and build_samples and all(
             v['peak_envelope_excess_bytes'] is not None
             and v['peak_envelope_excess_bytes'] <= v['untracked_in_use_at_start_bytes']
             and v['peak_mmapped_bytes'] == 0 and v['alignment_above_16_calls'] == 0
             and v['executable_aligned_calls'] == 0 for v in d1.values()), fatal=False)
print(json.dumps(evidence, indent=1))
failed = [name for name, result in evidence['cases'].items() if not result['pass']]
if failed:
    sys.exit('proof cases failed: ' + ', '.join(failed))
