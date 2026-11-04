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
# The production worker under proof. `KG1_WORKER` names a separately built feature-off
# artifact (the shipped recipe, without `proof-executables`); examples, driver and
# probes always come from `target`.
worker = os.path.realpath(os.environ.get('KG1_WORKER') or os.path.join(target, 'keeper-graph-query'))
driver = os.path.realpath(os.path.join(target, 'keeper-graph-driver'))
mac = platform.system() == 'Darwin'
env = {'GQL_STMT_CACHE': '0'}
if not mac:
    # P1′: the supervisor's fixed Linux allocator setting; the worker requires it exactly.
    tunables = 'glibc.malloc.mmap_max=0:glibc.malloc.tcache_count=0:glibc.malloc.trim_threshold=536870912'
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

# Measurement-only native helpers (observer, B0 preloads). `KG1_NATIVE_EXPORT=<dir>`
# copies each compiled object there with a manifest keyed by the exact flags and source
# bytes; `KG1_NATIVE_PREBUILT=<dir>` on a host without a compiler takes the object from
# such a manifest only if that key matches this harness's source and flags exactly.
native_record = {}
def native_cc(flags, output, sources, libs=()):
    import hashlib, shutil
    key = hashlib.sha256(json.dumps([flags, list(libs), [open(s, 'rb').read().hex() for s in sources]]).encode()).hexdigest()
    name = os.path.basename(output)
    prebuilt = os.environ.get('KG1_NATIVE_PREBUILT')
    if prebuilt:
        with open(os.path.join(prebuilt, 'manifest.json')) as f:
            entry = json.load(f).get(name, {})
        if entry.get('key') != key or sha256(os.path.join(prebuilt, name)) != entry.get('sha256'):
            sys.exit(f'prebuilt native object {name} does not match this harness source/flags')
        shutil.copy(os.path.join(prebuilt, name), output)
        os.chmod(output, 0o755)
        native_record[name] = {'mode': 'prebuilt', 'key': key, 'sha256': entry['sha256']}
        return
    subprocess.run(['cc'] + flags + ['-o', output] + sources + list(libs), check=True)
    native_record[name] = {'mode': 'compiled', 'key': key, 'sha256': sha256(output)}
    export = os.environ.get('KG1_NATIVE_EXPORT')
    if export:
        os.makedirs(export, exist_ok=True)
        shutil.copy(output, os.path.join(export, name))
        manifest_path = os.path.join(export, 'manifest.json')
        manifest = json.load(open(manifest_path)) if os.path.exists(manifest_path) else {}
        manifest[name] = {'key': key, 'sha256': native_record[name]['sha256'], 'flags': flags, 'libs': list(libs)}
        with open(manifest_path, 'w') as f:
            json.dump(manifest, f, indent=1)
evidence['native_helpers'] = native_record

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
    # Compiler/link/native-object/DSO/std identities are captured separately by the
    # round's native-provenance record (see the report); this records the recipes.
    # The recipe that produced `target`: its directory name is the cargo profile. The
    # worker artifact must come from the reviewed panic=abort `graph-worker` profile.
    'build_profile': os.path.basename(os.path.normpath(target)),
    'build_commands': ['cargo +stable build --manifest-path src-tauri/Cargo.toml '
                       + ('--release' if os.path.basename(os.path.normpath(target)) == 'release'
                          else '--profile ' + os.path.basename(os.path.normpath(target)))
                       + ' -p keeper-graph-query -p keeper-graph-driver --features keeper-graph-query/proof-executables --bins --examples'],
    'worker_under_proof': {'path': worker, 'sha256': sha256(worker),
                           'feature_off': bool(os.environ.get('KG1_WORKER')),
                           'recipe': ('cargo +stable build --manifest-path src-tauri/Cargo.toml --profile graph-worker '
                                      '-p keeper-graph-query --bin keeper-graph-query (no features), copied aside')
                           if os.environ.get('KG1_WORKER') else 'the proof-feature build above'},
    'binaries': {name: sha256(path) for name, path in (
        ('keeper-graph-query (proof-feature build)', os.path.join(target, 'keeper-graph-query')), ('keeper-graph-driver', os.path.join(target, 'keeper-graph-driver')),
        ('native_probe', os.path.join(target, 'examples', 'native_probe')), ('fake_worker', os.path.join(target, 'examples', 'fake_worker')),
        ('allocation_probe', os.path.join(target, 'examples', 'allocation_probe')), ('budget_probe', os.path.join(target, 'examples', 'budget_probe')))},
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
# The artifacts are built with the graph-worker profile (panic = "abort"): no panic in
# the process can unwind, from exec on, so std's pre-main init panics (signal-stack
# mmap/mprotect failure) also abort. The worker's first statement additionally makes a
# post-main panic silent. After Linux confinement seccomp denies tgkill (EPERM), so
# glibc 2.43 abort() falls through to ABORT_INSTRUCTION (x86_64 hlt) -> SIGSEGV;
# before confinement, and on macOS, it is SIGABRT.
signalled = -6 if mac else -11
r = run(['panic-aborts', generation], generation)
default_hook = run(['panic-default-hook', generation], generation)
case('panic_aborts_without_unwinding', {'with_hook': r, 'default_hook_control': default_hook},
     r['exit'] == signalled and r['stderr'] == '' and r['stdout'] == ''
     and default_hook['exit'] == signalled and 'panicked' in default_hook['stderr'])
# Structural: neither the worker nor the probe links an unwinder entry point that can
# start unwinding (panic_unwind absent); `__rust_start_panic` reaches abort().
if not mac:
    def start_panic_target(binary):
        import re
        text = subprocess.run(['objdump', '-d', '-C', '--no-show-raw-insn', binary], capture_output=True, text=True).stdout
        names = {int(a, 16): n for a, n in re.findall(r'^([0-9a-f]+) <(.*)>:$', text, re.M)}
        relative = {}
        for line in subprocess.run(['readelf', '-rW', binary], capture_output=True, text=True).stdout.splitlines():
            parts = line.split()
            if len(parts) >= 4 and parts[2] == 'R_X86_64_RELATIVE':
                relative[int(parts[0], 16)] = int(parts[3], 16)
        body = re.search(r'^[0-9a-f]+ <__rustc::__rust_start_panic>:\n(.*?)\n\n', text, re.M | re.S)
        slots = [int(s, 16) for s in re.findall(r'call\s+\*0x[0-9a-f]+\(%rip\)\s+# ([0-9a-f]+)', body.group(1))] if body else []
        direct = re.findall(r'call\s+[0-9a-f]+ <([^>]+)>', body.group(1)) if body else []
        return [names.get(relative.get(s), hex(s)) for s in slots] + direct
    # On a host without binutils the same static result may come from a JSON computed
    # off-host for byte-identical binaries (`KG1_STATIC_PRECOMPUTED`): it is accepted
    # only if its recorded sha256s equal the binaries under proof here.
    precomputed = os.environ.get('KG1_STATIC_PRECOMPUTED')
    if precomputed:
        with open(precomputed) as f:
            structure = json.load(f)
        matches = all(structure.get(label, {}).get('sha256') == sha256(binary) for label, binary in (('worker', worker), ('probe', probe)))
        structure = dict(structure, computed_off_host=True, sha256_matches_binaries_under_proof=matches)
    else:
        structure = {}
        for label, binary in (('worker', worker), ('probe', probe)):
            imports = subprocess.run(['nm', '-D', '--undefined-only', binary], capture_output=True, text=True).stdout
            targets = start_panic_target(binary)
            structure[label] = {'sha256': sha256(binary),
                                'unwinder_entry_imports': [s for s in ('_Unwind_RaiseException', '_Unwind_ForcedUnwind', '_Unwind_Resume_or_Rethrow') if s in imports],
                                'start_panic_calls': targets}
        matches = True
        if os.environ.get('KG1_STATIC_EXPORT'):
            with open(os.environ['KG1_STATIC_EXPORT'], 'w') as f:
                json.dump(structure, f, indent=1)
    case('artifact_cannot_unwind', structure,
         matches and all(not structure[k]['unwinder_entry_imports'] and structure[k]['start_panic_calls']
                         and all('abort' in t for t in structure[k]['start_panic_calls']) for k in ('worker', 'probe')))
    # Pre-main: drive std's signal-stack mmap into failure with a tight inherited
    # RLIMIT_AS. Under panic=abort the init panic prints std's message and aborts
    # (SIGABRT, before confinement) with no caught unwind. "initialization or cleanup
    # bug" is printed only by rt's handler after an unwind, and is recorded for every
    # outcome independently of the primary failure (an unwind build prints the
    # alt-stack panic AND that marker). Guard-page mprotect failure is classified but
    # has no positive control; it is not claimed as exercised unless hit.
    import resource
    def premain_sweep(binary):
        outcomes, samples = {}, {}
        for kib in range(2048, 16384, 4):
            def tight(limit=kib * 1024):
                resource.setrlimit(resource.RLIMIT_AS, (limit, limit))
            process = subprocess.run([binary, 'query', generation], input=b'', env=env, capture_output=True,
                                     timeout=10, preexec_fn=tight)
            stderr = process.stderr.decode(errors='replace')
            primary = ('alt_stack' if 'alternative stack' in stderr and 'guard page' not in stderr
                       else 'guard_page' if 'stack guard page' in stderr else 'other')
            caught = 'initialization or cleanup bug' in stderr
            key = primary + ('+caught_unwind' if caught else '')
            outcomes.setdefault(key, {}).setdefault(process.returncode, []).append(kib)
            if primary != 'other' or caught:
                samples.setdefault(key, stderr[-400:])
        summary = {k: {code: [min(v), max(v), len(v)] for code, v in m.items()} for k, m in outcomes.items()}
        startup_hits = {k: m for k, m in outcomes.items() if not k.startswith('other') or k.endswith('+caught_unwind')}
        ok = ('alt_stack' in outcomes and all(not k.endswith('+caught_unwind') for k in outcomes)
              and all(set(m) == {-6} for m in startup_hits.values()))
        return ok, {'outcomes': summary, 'stderr_samples': samples}
    ok, detail = premain_sweep(worker)
    # Optional discriminating control: the same assertion over a panic-unwind build of
    # the same worker (KG1_UNWIND_CONTROL_WORKER) must FAIL.
    control = os.environ.get('KG1_UNWIND_CONTROL_WORKER')
    if control:
        control_ok, control_detail = premain_sweep(control)
        detail['unwind_control'] = {'binary': control, 'sha256': sha256(control), 'assertion_passed': control_ok,
                                    **control_detail}
    case('premain_init_panic_aborts_without_unwinding', detail,
         ok and (not control or detail['unwind_control']['assertion_passed'] is False))
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

# R (enacted 28 MiB): exact Cap acceptance/refusal at the budget, and std's real
# `realloc_fallback` (align 32) on growth, shrink and refused growth, through the same
# Cap<System> and BUDGET as the worker (safe example, no FFI).
budget_probe = os.path.realpath(os.path.join(target, 'examples', 'budget_probe'))
def budget_case(name, extra_env=None, data_limit=None):
    def limit():
        import resource
        resource.setrlimit(resource.RLIMIT_DATA, (data_limit, data_limit))
    process = subprocess.run([budget_probe, name], env=dict(env, **(extra_env or {})), capture_output=True, text=True, timeout=60,
                             preexec_fn=limit if data_limit else None)
    return {'exit': process.returncode, 'report': json.loads(process.stdout) if process.returncode == 0 else {},
            'stderr': process.stderr.strip()[-300:], 'rlimit_data': data_limit}
r = budget_case('cap-boundary')
b = r['report']
case('budget_r_exact_boundary', r, r['exit'] == 0 and b.get('budget') == 28 * 1024 * 1024 and b.get('exact_ok') is True
     and b.get('allocated_at_limit') == b.get('budget') and b.get('over_refused') is True
     and b.get('allocated_after_refusal') == b.get('allocated_before'))
r = budget_case('rust-fallback')
b = r['report']
case('rust_realloc_fallback_grow_shrink_refusal', r, r['exit'] == 0 and b.get('align') == 32
     and b.get('grow_preserved') is True and b.get('shrink_preserved') is True
     and b.get('charged_grown', 0) > b.get('charged_small', 0) > 0 and b.get('charged_shrunk', 0) < b.get('charged_grown', 0)
     and b.get('refused') is True and b.get('retained') is True)
# Failure *inside* std's align-32 `realloc_fallback`: Cap admits the 20 MiB growth (it
# is within R), so the refusal can only come from System's aligned allocation under the
# 8 MiB RLIMIT_DATA. Cap must release its reservation and the old block must keep its
# pointer, capacity, charge and contents. The identical request without the limit
# succeeds (control), so the failure is the limit's, not Cap's.
if not mac:
    failing = budget_case('rust-fallback-failure', data_limit=8 * 1024 * 1024)
    control = budget_case('rust-fallback-failure')
    f, c = failing['report'], control['report']
    case('rust_realloc_fallback_internal_failure', {'under_rlimit_data': failing, 'control_without_limit': control},
         failing['exit'] == 0 and f.get('align') == 32 and f.get('within_budget') is True and f.get('failed') is True
         and f.get('retained') is True and f.get('charged_after') == f.get('charged_before')
         and control['exit'] == 0 and c.get('within_budget') is True and c.get('failed') is False)

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
for mode in ('oracle', 'limits') + (() if mac else ('prerequisites',)) + (('benchmark',) if benchmark else ()):
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

def worker_run(role, db, stdin_bytes, extra_env=None, timeout=60):
    command = [worker, role, db]
    if mac:
        command = ['/usr/bin/sandbox-exec', '-p', profiles['BUILD' if role == 'build' else 'READ'], '-D', 'WORKER=' + worker, '-D', 'DB=' + db] + command
    process = subprocess.run(command, input=stdin_bytes, env=dict(env, **(extra_env or {})), capture_output=True, timeout=timeout)
    return {'exit': process.returncode, 'stdout': process.stdout, 'stderr': process.stderr.decode(errors='replace').strip()[-300:]}
def published(db):
    import sqlite3
    try:
        return '_kg_manifest' in [t for (t,) in sqlite3.connect('file:' + db + '?mode=ro', uri=True)
                                  .execute("SELECT name FROM sqlite_schema WHERE type='table'")]
    except sqlite3.DatabaseError:
        return False
count_query = (json.dumps({'source': 'MATCH (n:Concept) RETURN count(*) AS total LIMIT 1', 'parameters': {}}) + '\n').encode()
baseline_rows = json.loads(worker_run('query', limits_db, count_query)['stdout'] or b'{}').get('rows')

# Adversarial frames at the event-frame limit (2,097,151 content bytes + newline) in
# both input roles: a well-typed event whose `after` carries ~524k one-byte strings (the
# N_max premise) and a query frame of the same size, which the 81,984-byte query bound
# refuses before parsing. Each is a typed whole-request refusal with nothing written or
# published: the builder parses the whole frame and its existing value limits refuse it
# as `query_limit` (75); the query worker refuses the oversize frame (74). The prior
# generation answers unchanged afterwards.
event_limit = 2 * 1024 * 1024 - 1
oracle_export = os.path.join(scratch, 'driver-oracle', 'fixture-personal.ndjson')
with open(oracle_export, 'rb') as f:
    head_line, event_line = f.readline(), f.readline()
manifest_line = json.loads(head_line)
manifest_line['accepted_head'] = 1
event = json.loads(event_line)
event['operations'] = event['operations'][:1]
event['operations'][0]['after']['properties'] = {'p': []}
skeleton = len(json.dumps(event, separators=(',', ':')).encode())
strings = (event_limit - skeleton) // 4
event['operations'][0]['after']['properties'] = {'p': ['a'] * strings}
line = json.dumps(event, separators=(',', ':')).encode()
line = line[:-3] + b' ' * (event_limit - len(line)) + line[-3:]
adversarial_build_frame = json.dumps(manifest_line, separators=(',', ':')).encode() + b'\n' + line + b'\n'
adversarial_query_frame = b'{"source":"' + b'a' * (event_limit - 13) + b'"}' + b'\n'
frames = {}
db = fresh('adversarial-build.db')
r = worker_run('build', db, adversarial_build_frame)
frames['build'] = {'exit': r['exit'], 'event_content_bytes': len(line), 'one_byte_strings': strings,
                   'stdout_bytes': len(r['stdout']), 'published': published(db), 'stderr': r['stderr']}
r = worker_run('query', limits_db, adversarial_query_frame)
frames['query'] = {'exit': r['exit'], 'frame_content_bytes': len(adversarial_query_frame) - 1,
                   'stdout_bytes': len(r['stdout']), 'stderr': r['stderr']}
after_rows = json.loads(worker_run('query', limits_db, count_query)['stdout'] or b'{}').get('rows')
case('adversarial_frames_at_event_limit_both_roles', dict(frames, prior_generation_rows=baseline_rows, after_rows=after_rows),
     len(line) == event_limit and len(adversarial_query_frame) - 1 == event_limit
     and frames['build']['exit'] == 75 and frames['query']['exit'] == 74
     and all(f['stdout_bytes'] == 0 for f in frames.values())
     and not frames['build']['published'] and baseline_rows and after_rows == baseline_rows)

if not mac:
    # B0 on the unmodified worker: VmData admitted at the boundary is reported and <= H0.
    r = worker_run('query', limits_db, count_query)
    out = json.loads(r['stdout']) if r['exit'] == 0 else {}
    case('b0_admitted_unmodified_worker', {'exit': r['exit'], 'boundary_vm_data_bytes': out.get('boundary_vm_data_bytes'),
                                           'boundary_vm_peak_bytes': out.get('boundary_vm_peak_bytes'), 'h0': 1024 * 1024},
         r['exit'] == 0 and 0 < out.get('boundary_vm_data_bytes', 0) <= 1024 * 1024 and out.get('boundary_vm_peak_bytes', 0) > 0)
    # B0 refusal through a test-only preload that grows the brk heap before main (P1′
    # forbids mmapped chunks, so the bytes land in VmData) and then removes LD_PRELOAD
    # from the environment. Over H0 every role exits 74 before reading or writing
    # anything; a small inflation is admitted (control).
    inflate_source = os.path.join(scratch, 'b0-inflate.c')
    with open(inflate_source, 'w') as f:
        # The volatile global keeps -O2 from eliding the otherwise unused allocation.
        f.write('#include <stdlib.h>\n#include <string.h>\nvoid *volatile kg_b0_inflation;\n'
                '__attribute__((constructor)) static void inflate(void) {\n'
                '  char *p = malloc(INFLATE_BYTES); if (p) memset(p, 1, INFLATE_BYTES);\n'
                '  kg_b0_inflation = p;\n  unsetenv("LD_PRELOAD");\n}\n')
    b0 = {}
    for label, size in (('over_h0', 2 * 1024 * 1024), ('control_small', 16 * 1024)):
        library = os.path.join(scratch, f'b0-inflate-{label}.so')
        native_cc(['-O2', '-Wall', '-Werror', '-shared', '-fPIC', f'-DINFLATE_BYTES={size}'], library, [inflate_source])
        staged = fresh(f'b0-{label}-build.db')
        with open(oracle_export, 'rb') as f:
            export_bytes = f.read()
        runs = {'build': worker_run('build', staged, export_bytes, {'LD_PRELOAD': library}),
                'query': worker_run('query', limits_db, count_query, {'LD_PRELOAD': library}),
                'inspect': worker_run('inspect', limits_db, (json.dumps({'id': '0' * 25 + '1', 'include_archived': False}) + '\n').encode(), {'LD_PRELOAD': library})}
        b0[label] = {role: {'exit': x['exit'], 'stdout_bytes': len(x['stdout']), 'stderr': x['stderr'][-160:]} for role, x in runs.items()}
        b0[label]['build_published'] = published(staged)
    after_b0_rows = json.loads(worker_run('query', limits_db, count_query)['stdout'] or b'{}').get('rows')
    case('b0_over_h0_refused_every_role_74', dict(b0, baseline_rows=baseline_rows, after_rows=after_b0_rows),
         all(b0['over_h0'][role]['exit'] == 74 and b0['over_h0'][role]['stdout_bytes'] == 0
             for role in ('build', 'query', 'inspect'))
         and not b0['over_h0']['build_published']
         and all(b0['control_small'][role]['exit'] == 0 for role in ('build', 'query', 'inspect'))
         and b0['control_small']['build_published'] and baseline_rows and after_b0_rows == baseline_rows)
    # B0 end to end through the real supervisor (driver `boundary` mode). The worker it
    # is given is a test-only wrapper that execs the real worker with a measurement
    # preload; the driver selects the variant per call through a control file:
    # a real brk-heap inflation (16 KiB admitted, 2 MiB refused), or `/proc/self/status`
    # replaced (only that path, via open64) with exact-boundary content (1024 kB
    # admitted, 1025 kB refused) and malformed/missing/duplicate/overflow content.
    control_file = os.path.join(scratch, 'b0-variant')
    variant_source = os.path.join(scratch, 'b0-variant.c')
    with open(variant_source, 'w') as f:
        f.write(r'''#define _GNU_SOURCE
#include <dlfcn.h>
#include <fcntl.h>
#include <stdarg.h>
#include <stdlib.h>
#include <string.h>
#include <sys/mman.h>
#include <unistd.h>
static char variant[64];
void *volatile kg_b0_inflation;
__attribute__((constructor)) static void init(void) {
  const char *control = getenv("KG1_B0_CONTROL");
  int fd = control ? open(control, O_RDONLY) : -1;
  if (fd >= 0) { ssize_t n = read(fd, variant, sizeof variant - 1); if (n > 0) variant[n] = 0; close(fd); }
  size_t bytes = !strcmp(variant, "heap_2_mib") ? 2u << 20 : !strcmp(variant, "heap_16_kib") ? 16u << 10 : 0;
  if (bytes) { char *p = malloc(bytes); if (p) memset(p, 1, bytes); kg_b0_inflation = p; }
  unsetenv("LD_PRELOAD");
  unsetenv("KG1_B0_CONTROL");
}
static const char *status(void) {
  if (!strcmp(variant, "exact_h0_1024_kb")) return "Name:\tw\nVmPeak:\t9000 kB\nVmData:\t1024 kB\n";
  if (!strcmp(variant, "over_h0_1025_kb")) return "Name:\tw\nVmPeak:\t9000 kB\nVmData:\t1025 kB\n";
  if (!strcmp(variant, "malformed")) return "Name:\tw\nVmData:\t1x24 kB\n";
  if (!strcmp(variant, "missing")) return "Name:\tw\nVmPeak:\t9000 kB\n";
  if (!strcmp(variant, "duplicate")) return "VmData:\t10 kB\nVmData:\t10 kB\n";
  if (!strcmp(variant, "overflow")) return "VmData:\t99999999999999999999 kB\n";
  return NULL;
}
int open64(const char *path, int flags, ...) {
  static int (*real)(const char *, int, ...);
  if (!real) real = (int (*)(const char *, int, ...))dlsym(RTLD_NEXT, "open64");
  mode_t mode = 0;
  if (flags & (O_CREAT | O_TMPFILE)) { va_list ap; va_start(ap, flags); mode = va_arg(ap, mode_t); va_end(ap); }
  const char *content = status();
  if (content && !strcmp(path, "/proc/self/status")) {
    int m = memfd_create("kg-b0-status", 0);
    if (m < 0 || write(m, content, strlen(content)) != (ssize_t)strlen(content)) _exit(70);
    return m;
  }
  return real(path, flags, mode);
}
''')
    variant_library = os.path.join(scratch, 'b0-variant.so')
    native_cc(['-O2', '-Wall', '-Wextra', '-Werror', '-shared', '-fPIC'], variant_library, [variant_source], ['-ldl'])
    wrapper = os.path.join(scratch, 'b0-wrapper-worker')
    with open(wrapper, 'w') as f:
        f.write('#!/bin/sh\nexec /usr/bin/env -i GQL_STMT_CACHE="$GQL_STMT_CACHE" GLIBC_TUNABLES="$GLIBC_TUNABLES" '
                f'KG1_B0_CONTROL={control_file} LD_PRELOAD={variant_library} {worker} "$@"\n')
    os.chmod(wrapper, 0o755)
    process = subprocess.run([driver, 'boundary', wrapper, os.path.join(scratch, 'driver-boundary'), control_file],
                             capture_output=True, text=True, timeout=600)
    lines = [json.loads(line) for line in process.stdout.splitlines() if line.startswith('{')]
    record = next((l for l in lines if l.get('mode') == 'boundary' and 'outcomes' in l), {})
    expected = {'exact_h0_1024_kb': True, 'heap_16_kib': True, 'over_h0_1025_kb': False, 'heap_2_mib': False,
                'malformed': False, 'missing': False, 'duplicate': False, 'overflow': False}
    case('b0_through_supervisor_boundary_and_malformed', {'exit': process.returncode, 'records': lines,
                                                           'stderr_tail': process.stderr[-600:] if process.returncode else ''},
         process.returncode == 0 and lines and lines[-1].get('complete')
         and {o['variant']: o['admitted'] for o in record.get('outcomes', [])} == expected
         and all(o['typed_and_clean'] and o['prior_generation_rows_unchanged'] for o in record['outcomes']))

# F1: external allocation accounting. A measurement-only LD_PRELOAD interposer
# attributes every libc allocation to the worker executable (Cap-charged) or a shared
# library (uncharged bypass) and samples the allocator after every call. Since round 10
# the case asserts the reviewed accounted-cost invariant (R1 design v7 §7, inventory
# round 10 §4.6), not a constant raw remainder: raw chunk overhead stays input-dependent
# and is recorded, never asserted. LD_PRELOAD changes startup, so this is instrumented
# evidence, kept apart from the unmodified B0 case above.
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
    native_cc(['-O2', '-Wall', '-Wextra', '-Werror', '-shared', '-fPIC'], library, [source], ['-ldl'])
    # Observer self-test (built without builtin malloc/free assumptions so the sample
    # counters are re-read across calls): bootstrap alignment/extent/zeroing/membership,
    # overflow/exhaustion refusal, realloc copy boundaries, post-call samples after free
    # and every realloc outcome, and the deferred aligned-allocation sample. Under P1 the
    # D1 envelope holds; without it tcache-held chunks break it (negative control).
    selftest = os.path.join(scratch, 'alloc-interposer-selftest')
    native_cc(['-O2', '-fno-builtin', '-Wall', '-Wextra', '-Werror', '-DKG_INTERPOSER_SELFTEST'], selftest, [source], ['-ldl'])
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
                                 capture_output=True, timeout=1800)
        stderr = process.stderr.decode(errors='replace')
        report = [json.loads(l[len('KG_ALLOC '):]) for l in stderr.splitlines() if l.startswith('KG_ALLOC ')]
        response = json.loads(process.stdout) if process.returncode == 0 else {}
        return dict(report[0] if report else {}, query=label, exit=process.returncode, rows=len(response.get('rows', [])),
                    worker_peak_charged_bytes=response.get('peak_allocated_bytes'))
    samples = [observed([worker, 'query', limits_db], (json.dumps({'source': query, 'parameters': params}) + '\n').encode(), name)
               for name, query, params in corpus]
    # Builder role over every accepted export the driver built and published (the
    # deliberately faulted exports are refused by design and are not measured).
    faults = ('-truncated', '-digest', '-before-image', '-dangling', '-gap', '-trailing')
    exports = sorted(os.path.join(dirpath, n) for dirpath, _, names in os.walk(scratch) for n in names
                     if n.endswith('.ndjson') and not n[:-len('.ndjson')].endswith(faults))
    build_samples = []
    for export in exports:
        label = os.path.relpath(export, scratch)
        with open(export, 'rb') as f:
            stream = f.read()
        staged = fresh('observed-build-' + label.replace(os.sep, '-') + '.db')
        build_samples.append(observed([worker, 'build', staged], stream, 'build:' + label))
    # Typed refusals are measured too: the adversarial frames (exit 74) and the native
    # materialization that exhausts R (exit 72). Their allocation reports must exist.
    refusal_samples = [
        dict(observed([worker, 'build', fresh('observed-adversarial-build.db')], adversarial_build_frame, 'adversarial_build_frame'), expected_exit=75),
        dict(observed([worker, 'query', limits_db], adversarial_query_frame, 'adversarial_query_frame'), expected_exit=74),
        dict(observed([worker, 'query', limits_db], (json.dumps({'source': f'MATCH (n:Concept) WHERE n.P_626c6f62 IS NOT NULL RETURN {wide} LIMIT 1000', 'parameters': {}}) + '\n').encode(),
                      'native_materialization_exit_72'), expected_exit=72),
    ]
    every = samples + build_samples + refusal_samples
    R, H0, N_MAX, STACK = 28 * 1024 * 1024, 1024 * 1024, 2_101_248, 8 * 1024 * 1024
    ENVELOPE = H0 + 4 * R + 64 * N_MAX + 64 + STACK
    def accounted(s):
        """The accounted-cost invariant for one run: the per-sample D1 envelope
        I - 2S - 64*N32 - L <= I_start held at every post-call sample, with S <= R,
        N32 <= N_max, I_start + L <= H0, no mmapped chunk and no wide/aligned executable
        entry. Structurally X <= 2R + 64 (design D2), so the run's real heap cost is at
        most H0 + 4R + 64*N_max + 64, plus the separate 8 MiB stack term."""
        if 'peak_envelope_excess_bytes' not in s or s.get('table_overflow') != 0 or s.get('bootstrap_refused') != 0:
            return False
        return (s['peak_envelope_excess_bytes'] <= s['untracked_in_use_at_start_bytes']
                and s['peak_charged_bytes'] <= R
                and s['peak_live_executable_allocations_below_32'] <= N_MAX
                and s['untracked_in_use_at_start_bytes'] + s['peak_library_bytes'] <= H0
                and s['peak_mmapped_bytes'] == 0 and s['alignment_above_16_calls'] == 0
                and s['executable_aligned_calls'] == 0)
    def accounted_bound(s):
        X = 2 * s.get('peak_charged_bytes', 0) + 64
        return (s.get('untracked_in_use_at_start_bytes', 0) + s.get('peak_library_bytes', 0) + 2 * s.get('peak_charged_bytes', 0)
                + 64 * s.get('peak_live_executable_allocations_below_32', 0) + 64 + X + STACK)
    exits_ok = all(s['exit'] == s.get('expected_exit', 0) for s in every)
    # Cap and the observer must agree on the charged high-water mark (same domain).
    agree = all(s.get('peak_charged_bytes') == s.get('worker_peak_charged_bytes') for s in samples + build_samples)
    # F_post = 0 falsifier: uncharged library bytes are identical within each role.
    library_spread = {role: max(v) - min(v) for role, v in (
        ('query', [s.get('peak_library_bytes', -1) for s in samples]),
        ('build', [s.get('peak_library_bytes', -1) for s in build_samples]))}
    # Discriminating controls on the predicate itself: each single premise broken on a
    # real sample must be rejected.
    reference = samples[0]
    mutations = {
        'd1_excess_above_untracked': {'peak_envelope_excess_bytes': reference.get('untracked_in_use_at_start_bytes', 0) + 1},
        'charged_above_r': {'peak_charged_bytes': R + 1},
        'small_count_above_n_max': {'peak_live_executable_allocations_below_32': N_MAX + 1},
        'startup_above_h0': {'peak_library_bytes': H0 + 1},
        'mmapped_chunk': {'peak_mmapped_bytes': 4096},
        'wide_alignment': {'alignment_above_16_calls': 1},
        'table_overflow': {'table_overflow': 1},
    }
    controls = {name: not accounted(dict(reference, **change)) for name, change in mutations.items()}
    # Observer control: std's real realloc_fallback (budget_probe, align 32) is seen as
    # executable aligned allocation, which the worker runs above never reach.
    fallback = observed([budget_probe, 'rust-fallback'], b'', 'budget_probe_rust_fallback')
    controls['observer_sees_rust_realloc_fallback'] = fallback.get('executable_aligned_calls', 0) > 0 and fallback.get('alignment_above_16_calls', 0) > 0
    raw = [s.get('peak_uncharged_remainder_bytes', 0) for s in samples]
    result = {'samples': samples, 'build_samples': build_samples, 'refusal_samples': refusal_samples,
              'fallback_control_sample': fallback, 'predicate_controls': controls,
              'charged_accounting_agrees_with_cap': agree, 'exits_as_expected': exits_ok,
              'per_run_invariant': {s['query']: accounted(s) for s in every},
              'max_accounted_bound_bytes': max(accounted_bound(s) for s in every), 'envelope_bytes': ENVELOPE,
              'library_bypass_spread_bytes': library_spread,
              # Recorded, not asserted: raw chunk overhead is input-dependent by design.
              'raw_remainder_spread_bytes': max(raw) - min(raw),
              'max_live_below_32': max(s.get('peak_live_executable_allocations_below_32', 0) for s in every),
              'max_reconstructed_overlap_bytes': max(s.get('peak_reconstructed_overlap_bytes', 0) for s in every),
              'historical_round9_raw_spread_fail_bytes': 253243}
    ok = (exits_ok and agree and build_samples and all(result['per_run_invariant'].values())
          and result['max_accounted_bound_bytes'] <= ENVELOPE <= 256 * 1024 * 1024
          and all(v == 0 for v in library_spread.values()) and all(controls.values()))
    case('memory_uncharged_remainder', result, ok, fatal=False)
    valid = all('peak_envelope_excess_bytes' in s and s.get('table_overflow') == 0 for s in every)
    # Mechanism falsifier (design v3 D1 under P1), not the R1 bound: at every post-call
    # sample, allocator in-use <= 2*charged + 64*small32 + library chunks + untracked
    # pre-observer bytes; no mmapped chunk; no wide alignment; and no executable aligned
    # entry, so std's System realloc_fallback was never reached in these runs.
    d1 = {s['query']: {k: s.get(k) for k in ('peak_envelope_excess_bytes', 'untracked_in_use_at_start_bytes',
                                              'peak_mmapped_bytes', 'alignment_above_16_calls',
                                              'executable_aligned_calls', 'allocator_samples')} for s in every}
    case('d1_heap_envelope_under_p1', {'runs': d1, 'build_runs': len(build_samples)},
         valid and build_samples and all(
             v['peak_envelope_excess_bytes'] is not None
             and v['peak_envelope_excess_bytes'] <= v['untracked_in_use_at_start_bytes']
             and v['peak_mmapped_bytes'] == 0 and v['alignment_above_16_calls'] == 0
             and v['executable_aligned_calls'] == 0 for v in d1.values()), fatal=False)
print(json.dumps(evidence, indent=1))
failed = [name for name, result in evidence['cases'].items() if not result['pass']]
if failed:
    sys.exit('proof cases failed: ' + ', '.join(failed))
