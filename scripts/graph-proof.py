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
evidence = {'platform': platform.platform(), 'machine': platform.machine(), 'cases': {}}
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

def case(name, result, ok):
    result['pass'] = bool(ok)
    evidence['cases'][name] = result
    if not ok:
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
r = run(['wrong-thread'])
case('wrong_thread_callback_exit_73', r, r['exit'] == 73)
r = run(['live-statement'])
case('live_statement_registration_exit_73', r, r['exit'] == 73)
oom = fresh('oom.db')
r = run(['oom', oom], oom, 'BUILD', timeout=30)
case('sqlite_allocation_budget_exit_72', r, r['exit'] == 72)
r = run(['authz', generation], generation)
authz = json.loads(r['stdout']) if r['exit'] == 0 else {}
case('authorizer_denials', r, r['exit'] == 0 and all(authz['denied'].values()) and authz['plain_read_allowed'])

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
for mode in ('oracle', 'limits') + (('benchmark',) if benchmark else ()):
    command = [driver, mode, worker, os.path.join(scratch, 'driver-' + mode)]
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
    if mode == 'limits':
        # Same queries straight to the worker: distinguish allocation (72) from result cap (75).
        limits_db = [os.path.join(scratch, 'driver-limits', 'generations', n)
                     for n in os.listdir(os.path.join(scratch, 'driver-limits', 'generations')) if n.endswith('.db')][0]
        wide = ', '.join(f'n.P_626c6f62 AS b{i}' for i in range(16))
        for name, query, code in (
                ('native_materialization_exit_72', f'MATCH (n:Concept) WHERE n.P_626c6f62 IS NOT NULL RETURN {wide} LIMIT 1000', 72),
                ('encoded_result_cap_exit_75', 'MATCH (n:Concept) WHERE n.P_626c6f62 IS NOT NULL RETURN n.P_626c6f62 AS b LIMIT 100', 75)):
            command = [worker, 'query', limits_db]
            if mac:
                command = ['/usr/bin/sandbox-exec', '-p', profiles['READ'], '-D', 'WORKER=' + worker, '-D', 'DB=' + limits_db] + command
            process = subprocess.run(command, input=json.dumps({'source': query, 'parameters': '{}'}) + '\n',
                                     env=env, capture_output=True, text=True, timeout=10)
            case(name, {'exit': process.returncode, 'stdout_bytes': len(process.stdout)},
                 process.returncode == code and not process.stdout)
        # Uncharged remainder = worker peak RSS minus charged allocation, across input sizes.
        hub = json.dumps({'id': '0' * 25 + '1'})
        samples = []
        for name, query, params in (
                ('count', 'MATCH (h:Concept)-[e:contains]->(x) WHERE h.id = $id RETURN count(*) AS total LIMIT 1', hub),
                ('rows_1000', 'MATCH (h:Concept)-[e:contains]->(x) WHERE h.id = $id RETURN x.id AS id, x.title AS t ORDER BY id LIMIT 1000', hub),
                ('blob_rows_10', 'MATCH (n:Concept) WHERE n.P_626c6f62 IS NOT NULL RETURN n.P_626c6f62 AS b LIMIT 10', '{}'),
                ('blob_wide_16', 'MATCH (n:Concept) WHERE n.P_626c6f62 IS NOT NULL RETURN ' + ', '.join(f'n.P_626c6f62 AS b{i}' for i in range(16)) + ' LIMIT 1', '{}'),
                ('blob_sort_1000_wide_4', 'MATCH (n:Concept) WHERE n.P_626c6f62 IS NOT NULL RETURN ' + ', '.join(f'n.P_626c6f62 AS b{i}' for i in range(4)) + ' ORDER BY b0 DESC LIMIT 4', '{}')):
            command = [worker, 'query', limits_db]
            if mac:
                command = ['/usr/bin/sandbox-exec', '-p', profiles['READ'], '-D', 'WORKER=' + worker, '-D', 'DB=' + limits_db] + command
            process = subprocess.run(command, input=json.dumps({'source': query, 'parameters': params}) + '\n',
                                     env=env, capture_output=True, text=True, timeout=10)
            if process.returncode:
                case('memory_remainder_' + name, {'exit': process.returncode}, False)
            response = json.loads(process.stdout)
            samples.append({'query': name, 'rows': len(response['rows']), 'response_bytes': len(process.stdout),
                            'peak_charged_bytes': response['peak_allocated_bytes'], 'peak_rss_bytes': response['peak_rss_bytes'],
                            'uncharged_remainder_bytes': response['peak_rss_bytes'] - response['peak_allocated_bytes']})
        remainders = [s['uncharged_remainder_bytes'] for s in samples]
        charged = [s['peak_charged_bytes'] for s in samples]
        case('memory_remainder_measurement', {'samples': samples, 'remainder_spread_bytes': max(remainders) - min(remainders),
                                              'peak_charged_spread_bytes': max(charged) - min(charged)},
             all(c < 256 * 1024 * 1024 for c in charged))
print(json.dumps(evidence, indent=1))
