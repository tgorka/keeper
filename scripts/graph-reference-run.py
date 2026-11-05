#!/usr/bin/env python3
"""Story 110.1 reference-host runner (proof-only; never shipped).

The reviewed startup accounting (R1 design v7 §5.2/§5.3) is tied to the pinned loader
and libc (libc6 2.43-2ubuntu2.4, libgcc-s1 16-20260322-1ubuntu1). A reference host with
another libc is a different configuration, so the proof does not run against the host's
libc: `bundle` builds a minimal root filesystem holding the *pinned* DSOs at their
original absolute paths (the same PT_INTERP, the same ld.so.cache, no RUNPATH, the
unmodified binaries) plus a Python interpreter, and the proof runs chrooted into it in
a private mount namespace. Nothing is mounted on, or written to, the host outside the
bundle directory except the transient /etc/ld.so.preload of `preload-host`.

  bundle DEST          (build host) write DEST/rootfs with DEST/rootfs/bundle
  preflight OUT        (reference host) record the host; refuse (exit 3) unless it can
                       run the bundle
  reference OUT        root-started systemd scope (AllowedCPUs=0-3, CPUQuota=400%,
                       MemoryMax=8G, MemorySwapMax=0) -> private mount namespace ->
                       chroot into the pinned rootfs -> `inner`
  inner OUT            (inside scope and chroot) verify the effective limits and fail
                       closed (exit 4) otherwise, run the full proof with --benchmark
                       against the shipped-recipe worker, record memory.peak,
                       memory.events and PSI, exit 5 on MEMORY_STARVED
  preload-chroot OUT   P6c system-preload refusal through the real supervisor in the
                       pinned rootfs: its /etc/ld.so.preload is what that loader reads
  preload-host OUT     the same against the host's real /etc/ld.so.preload (sudo), with
                       the driver and worker on the host libc (a recorded deviation)

Both preload cases create an empty file, then a dangling symlink, while the driver
waits; remove it on any exit (finally + signal handlers) and verify it absent after.
"""
import json
import os
import platform
import shutil
import signal
import stat
import subprocess
import sys
import time

HERE = os.path.dirname(os.path.abspath(__file__))
BUNDLE = os.path.dirname(HERE)
ROOT = os.path.dirname(BUNDLE)
PINNED_GLIBC = '2.43-2ubuntu2.4'
PINNED_LIBGCC = '16-20260322-1ubuntu1'
LIMITS = {'cpuset.cpus.effective': '0-3', 'cpu.max': '400000 100000',
          'memory.max': str(8 * 1024 ** 3), 'memory.swap.max': '0'}
# MEMORY_STARVED: any OOM event, any swap use (cgroup or host), or more than one second
# of full memory stall inside the scope.
FULL_STALL_LIMIT_US = 1_000_000
PATH = 'PATH=/usr/bin:/bin'


def chrooted(path):
    """The absolute path inside the chroot of a file under the bundle."""
    return '/bundle/' + os.path.relpath(path, BUNDLE)


def write(path, value):
    with open(path, 'w') as f:
        json.dump(value, f, indent=1)


def text(path):
    try:
        with open(path) as f:
            return f.read().strip()
    except OSError as error:
        return f'unreadable: {error}'


def run(*command):
    try:
        return subprocess.run(command, capture_output=True, text=True, timeout=30).stdout.strip()
    except (OSError, subprocess.TimeoutExpired):
        return None


def sha256(path):
    import hashlib
    with open(path, 'rb') as f:
        return hashlib.sha256(f.read()).hexdigest()


# ---------------------------------------------------------------- bundle (build host)

def bundle(dest):
    repo = os.path.dirname(HERE)
    target = os.path.join(repo, 'src-tauri', 'target')
    rootfs = os.path.join(dest, 'rootfs')
    if os.path.exists(dest):
        sys.exit('bundle destination must be new')
    staged = os.path.join(rootfs, 'bundle')
    binaries = {
        'target/graph-worker/keeper-graph-driver': 'graph-worker/keeper-graph-driver',
        'target/graph-worker/keeper-graph-query': 'graph-worker/keeper-graph-query',
        'feature-off/keeper-graph-query': 'graph-worker-feature-off/keeper-graph-query',
        'release/keeper-graph-query': 'release/keeper-graph-query',
    }
    for name in ('native_probe', 'allocation_probe', 'budget_probe', 'fake_worker'):
        binaries[f'target/graph-worker/examples/{name}'] = f'graph-worker/examples/{name}'
    for rel, source in binaries.items():
        os.makedirs(os.path.dirname(os.path.join(staged, rel)), exist_ok=True)
        shutil.copy2(os.path.join(target, source), os.path.join(staged, rel))
    # The sources graph-proof.py hashes for provenance, and the scripts themselves.
    for rel in ('src-tauri/crates/keeper-graph-protocol', 'src-tauri/crates/keeper-graph-query/src',
                'src-tauri/crates/keeper-graph-query/examples', 'src-tauri/crates/keeper-graph-query/build.rs',
                'src-tauri/crates/keeper-graph-query/Cargo.toml', 'src-tauri/Cargo.toml', 'rust-toolchain.toml',
                'src-tauri/crates/keeper-graph-query/native-allocation.h', 'src-tauri/crates/keeper-graph-driver',
                'src-tauri/crates/keeper-core/src/graph', 'scripts/graph-proof.py', 'scripts/alloc-interposer.c',
                'scripts/graph-reference-run.py', 'scripts/kg1_reference_monitor.py', 'scripts/kg1-reference-transcript.sh',
                'scripts/kg1-jail-exec.py',
                'src-tauri/Cargo.lock',
                'src-tauri/crates/keeper-graph-query/vendor/graphqlite/source-sha256.json',
                'src-tauri/crates/keeper-graph-query/vendor/graphqlite/generated-provenance.json'):
        source, copy = os.path.join(repo, rel), os.path.join(staged, rel)
        os.makedirs(os.path.dirname(copy), exist_ok=True)
        (shutil.copytree if os.path.isdir(source) else shutil.copy2)(source, copy)
    native = os.environ.get('KG1_NATIVE_EXPORT_DIR')
    if not native:
        sys.exit('KG1_NATIVE_EXPORT_DIR (a graph-proof.py KG1_NATIVE_EXPORT directory) is required')
    shutil.copytree(native, os.path.join(staged, 'native'))
    # Root filesystem: the pinned DSO closure of every executable, at original paths.
    executables = [os.path.realpath(shutil.which(t)) for t in
                   ('python3', 'sh', 'env', 'mkfifo', 'kill', 'sha256sum', 'stat', 'cat', 'objdump', 'readelf', 'nm')]
    dynload = sorted(os.path.join(d, f) for d, _, fs in os.walk('/usr/lib/python3.14/lib-dynload') for f in fs if f.endswith('.so'))
    closure = set()
    for path in executables + dynload + [os.path.join(staged, rel) for rel in binaries]:
        for line in run('ldd', path).splitlines():
            parts = line.split()
            lib = parts[2] if '=>' in parts and len(parts) > 2 and parts[2].startswith('/') else (parts[0] if parts and parts[0].startswith('/') else None)
            if lib:
                closure.add(lib)

    def place(path, tree=False):
        """Copy `path` into the rootfs at the same absolute path, recreating every
        symlink on the way (including the merged-/usr top-level links) and following
        it for the rest of the path. Directories are copied whole only with `tree`."""
        parts = path.strip('/').split('/')
        current = '/'
        for i, part in enumerate(parts):
            current = os.path.join(current, part)
            mirror = os.path.join(rootfs, current.lstrip('/'))
            if os.path.islink(current):
                if not os.path.lexists(mirror):
                    os.makedirs(os.path.dirname(mirror), exist_ok=True)
                    os.symlink(os.readlink(current), mirror)
                resolved = os.path.normpath(os.path.join(os.path.dirname(current), os.readlink(current)))
                rest = parts[i + 1:]
                if rest or tree or not os.path.isdir(resolved):
                    place(os.path.join(resolved, *rest), tree)
                return
        mirror = os.path.join(rootfs, path.lstrip('/'))
        if os.path.isdir(path):
            if tree and not os.path.exists(mirror):
                shutil.copytree(path, mirror, symlinks=True, ignore=shutil.ignore_patterns('test', 'tests', '__pycache__', 'idlelib', 'tkinter', 'turtledemo'))
            else:
                os.makedirs(mirror, exist_ok=True)
        elif not os.path.exists(mirror):
            os.makedirs(os.path.dirname(mirror), exist_ok=True)
            shutil.copy2(path, mirror)
    for top in ('bin', 'lib', 'lib64', 'sbin'):
        place('/' + top)
    for tool in ('python3', 'sh', 'env', 'mkfifo', 'kill', 'sha256sum', 'stat', 'cat', 'objdump', 'readelf', 'nm'):
        place(shutil.which(tool))
    for path in executables + sorted(closure) + ['/etc/ld.so.cache']:
        place(path)
    place('/usr/lib/python3.14', tree=True)
    for d in ('proc', 'sys', 'dev', 'tmp', 'etc', 'bundle/out'):
        os.makedirs(os.path.join(rootfs, d), exist_ok=True)
    os.chmod(os.path.join(rootfs, 'tmp'), 0o1777)
    packages = {}
    for lib in sorted(closure):
        owner = run('dpkg', '-S', os.path.realpath(lib)) or run('dpkg', '-S', lib) or ''
        package = owner.split(':')[0] if ':' in owner else None
        packages[lib] = {'realpath': os.path.realpath(lib), 'sha256': sha256(os.path.realpath(lib)), 'package': package,
                         'version': run('dpkg-query', '-W', '-f=${Version}', package) if package else None}
    manifest = {'pinned_glibc': run('dpkg-query', '-W', '-f=${Version}', 'libc6'),
                'pinned_libgcc_s1': run('dpkg-query', '-W', '-f=${Version}', 'libgcc-s1'),
                'dsos': packages, 'executables': {p: sha256(p) for p in executables},
                'ld_so_cache_sha256': sha256('/etc/ld.so.cache'),
                'binaries': {rel: sha256(os.path.join(staged, rel)) for rel in binaries}}
    write(os.path.join(staged, 'rootfs-manifest.json'), manifest)
    with open(os.path.join(staged, 'SHA256SUMS'), 'w') as f:
        for d, _, fs in sorted(os.walk(rootfs)):
            for name in sorted(fs):
                path = os.path.join(d, name)
                if not os.path.islink(path) and not path.endswith(('/bundle/SHA256SUMS', '/bundle/TOPOLOGY.json')):
                    f.write(f'{sha256(path)}  {os.path.relpath(path, rootfs)}\n')
    write(os.path.join(staged, 'TOPOLOGY.json'), topology(rootfs))
    print(json.dumps({'bundle': rootfs, 'dsos': len(packages), 'pinned_glibc': manifest['pinned_glibc']}))
    return 0


def topology(rootfs):
    """Every entry of the rootfs except the run-output directory: type, permission bits
    and, for links, the exact target. Ownership is checked separately (the extracting
    user)."""
    import stat
    entries = {}
    for d, dirs, fs in os.walk(rootfs):
        rel_d = os.path.relpath(d, rootfs)
        if rel_d == 'bundle/out' or rel_d.startswith('bundle/out/'):
            dirs[:] = []
            continue
        for name in dirs + fs:
            path = os.path.join(d, name)
            rel = os.path.relpath(path, rootfs)
            if rel in ('bundle/TOPOLOGY.json',):
                continue
            st = os.lstat(path)
            kind = 'link' if stat.S_ISLNK(st.st_mode) else 'dir' if stat.S_ISDIR(st.st_mode) else 'file'
            entry = {'type': kind}
            if kind == 'link':
                entry['target'] = os.readlink(path)
            else:
                entry['mode'] = oct(st.st_mode & 0o7777)
            entries[rel] = entry
    return entries


# ------------------------------------------------------------ reference host (outside)

def host_snapshot(where):
    meminfo = dict(line.split(':', 1) for line in text('/proc/meminfo').splitlines() if ':' in line)
    vmstat = dict(line.split() for line in text('/proc/vmstat').splitlines() if len(line.split()) == 2)
    return {'time': time.time(), 'MemTotal': meminfo.get('MemTotal', '').strip(),
            'MemAvailable': meminfo.get('MemAvailable', '').strip(), 'SwapTotal': meminfo.get('SwapTotal', '').strip(),
            'SwapFree': meminfo.get('SwapFree', '').strip(), 'pswpin': vmstat.get('pswpin'), 'pswpout': vmstat.get('pswpout'),
            'psi_memory': text('/proc/pressure/memory'), 'psi_cpu': text('/proc/pressure/cpu'), 'psi_io': text('/proc/pressure/io'),
            'disk_free_bytes': shutil.disk_usage(where).free, 'loadavg': text('/proc/loadavg')}


def preflight(directory):
    os.makedirs(directory, exist_ok=True)
    tools = {t: shutil.which(t) for t in ('sudo', 'systemd-run', 'dpkg-query')}
    tools['python3'] = HOST_PYTHON if os.access(HOST_PYTHON, os.X_OK) else None
    manifest = json.load(open(os.path.join(BUNDLE, 'rootfs-manifest.json')))
    sums = subprocess.run(['sha256sum', '-c', '--quiet', 'bundle/SHA256SUMS'], cwd=ROOT, capture_output=True, text=True)
    lsm = text('/sys/kernel/security/lsm')
    controllers = text('/sys/fs/cgroup/cgroup.controllers').split()
    record = {'uname': platform.uname()._asdict(), 'os_release': text('/etc/os-release'),
              'host_glibc_package': run('dpkg-query', '-W', '-f=${Version}', 'libc6'),
              'host_libgcc_s1_package': run('dpkg-query', '-W', '-f=${Version}', 'libgcc-s1'),
              'rootfs_pinned_glibc': manifest['pinned_glibc'], 'rootfs_pinned_libgcc_s1': manifest['pinned_libgcc_s1'],
              'systemd': (run('systemctl', '--version') or '').splitlines()[:1], 'nproc': os.cpu_count(),
              'cpu_flags': next((l.split(':', 1)[1].split() for l in text('/proc/cpuinfo').splitlines() if l.startswith('flags')), []),
              'cpu_model': next((l.split(':', 1)[1].strip() for l in text('/proc/cpuinfo').splitlines() if l.startswith('model name')), None),
              'host': host_snapshot(ROOT), 'tools': tools, 'lsm': lsm, 'cgroup_controllers': controllers,
              'bundle_sha256_check': {'exit': sums.returncode, 'output': (sums.stdout + sums.stderr)[-2000:]},
              'host_preload_present': os.path.lexists('/etc/ld.so.preload'),
              'rootfs_preload_present': os.path.lexists(os.path.join(ROOT, 'etc', 'ld.so.preload'))}
    problems = []
    if sums.returncode != 0:
        problems.append('bundle hashes do not verify')
    expected_topology = json.load(open(os.path.join(BUNDLE, 'TOPOLOGY.json')))
    actual_topology = topology(ROOT)
    drift = sorted(set(expected_topology) ^ set(actual_topology)) + sorted(
        k for k in set(expected_topology) & set(actual_topology) if expected_topology[k] != actual_topology[k])
    owners = {os.lstat(os.path.join(ROOT, rel)).st_uid for rel in actual_topology}
    interpreter = os.path.realpath(os.path.join(ROOT, 'lib64/ld-linux-x86-64.so.2'))
    pinned_interp = manifest['dsos'].get('/lib64/ld-linux-x86-64.so.2', {})
    record['topology'] = {'entries': len(actual_topology), 'drift': drift[:50], 'owners': sorted(owners),
                          'topology_sha256': sha256(os.path.join(BUNDLE, 'TOPOLOGY.json')),
                          'interpreter_resolves_inside_rootfs': interpreter.startswith(ROOT + '/'),
                          'interpreter_sha256_matches_pin': interpreter.startswith(ROOT + '/') and sha256(interpreter) == pinned_interp.get('sha256')}
    if drift:
        problems.append(f'rootfs topology drift: {drift[:10]}')
    if owners != {os.getuid()}:
        problems.append(f'rootfs entries not all owned by the invoking user: {sorted(owners)}')
    if not record['topology']['interpreter_sha256_matches_pin']:
        problems.append('PT_INTERP path does not resolve to the pinned loader inside the rootfs')
    if manifest['pinned_glibc'] != PINNED_GLIBC or manifest['pinned_libgcc_s1'] != PINNED_LIBGCC:
        problems.append('bundle rootfs does not carry the pinned glibc/libgcc')
    if 'landlock' not in lsm.split(','):
        problems.append('landlock LSM not active')
    if record['host_preload_present'] or record['rootfs_preload_present']:
        problems.append('an ld.so.preload already exists')
    problems += [f'missing tool {t}' for t, p in tools.items() if not p]
    if record['host']['disk_free_bytes'] < 4 * 1024 ** 3:
        problems.append('less than 4 GiB free disk')
    if 'cpuset' not in controllers or 'memory' not in controllers:
        problems.append('cgroup v2 cpuset/memory controllers unavailable')
    record['problems'] = problems
    write(os.path.join(directory, 'preflight.json'), record)
    print(json.dumps({'preflight': 'ok' if not problems else 'refused', 'problems': problems}))
    return 0 if not problems else 3


HOST_PYTHON = '/usr/bin/python3'
JAIL_EXEC = os.path.join(HERE, 'kg1-jail-exec.py')


def jail(*command, root=None, uid=None, gid=None, launcher=JAIL_EXEC):
    """Root: private mount namespace, the pinned rootfs's /proc /sys /dev bound from the
    host, then chroot as the invoking user, all inside one process that ends in execve
    (`kg1-jail-exec.py`), so the command keeps the scope leader's pid and no launcher
    stays in the scope. The namespace (and every mount) ends with the command; nothing
    is mounted on the host. `root`/`uid`/`gid` are overridable only for the self-test."""
    return [HOST_PYTHON, '-I', '-S', launcher, root or ROOT, str(os.getuid() if uid is None else uid),
            str(os.getgid() if gid is None else gid), PATH, 'HOME=/bundle', 'PYTHONDONTWRITEBYTECODE=1', '--'] + list(command)


def bounded(command, timeout=30, check=True):
    """Every controller helper is time-bounded; a hang or failure is an error, not a pass."""
    try:
        result = subprocess.run(command, capture_output=True, text=True, timeout=timeout)
    except subprocess.TimeoutExpired as error:
        raise RuntimeError(f'{command} exceeded {timeout}s') from error
    if check and result.returncode != 0:
        raise RuntimeError(f'{command} failed ({result.returncode}): {result.stderr.strip()[-300:]}')
    return result


SCOPE_PROPERTIES = ('LoadState', 'ActiveState', 'SubState', 'ControlGroup')


def scope_state(unit, query=bounded):
    """Named systemd properties of the scope from a successful query, or (None, reason).
    A failed, empty or incomplete response is never read as absence."""
    try:
        result = query(['systemctl', 'show'] + [a for p in SCOPE_PROPERTIES for a in ('-p', p)] + [f'{unit}.scope'], check=False)
    except Exception as error:
        return None, f'show failed: {error!r}'
    if result.returncode != 0:
        return None, f'show exit {result.returncode}'
    props = {}
    for line in result.stdout.splitlines():
        key, sep, value = line.partition('=')
        if not sep or key in props:
            return None, f'malformed show line {line!r}'
        props[key] = value
    if set(props) != set(SCOPE_PROPERTIES):
        return None, f'show returned {sorted(props)}'
    return props, None


def cgroup_empty_or_gone(cgroup, root='/sys/fs/cgroup'):
    """True only on a positive observation: the scope's recorded cgroup directory is gone,
    or its recursive membership is readable and empty. Unknown identity is not gone."""
    sys.path.insert(0, HERE)
    import kg1_reference_monitor as monitor
    if not cgroup:
        return False, 'scope cgroup identity was never established'
    path = root + cgroup
    if not os.path.lexists(path):
        return True, 'cgroup directory gone'
    try:
        members = monitor.scope_members(path)
    except monitor.Invalid as error:
        if not os.path.lexists(path):
            return True, 'cgroup directory removed during the walk'
        return False, f'cgroup unreadable: {error}'
    return (not members), f'members {members}'


def establish_scope(unit, query=bounded, timeout=30, sleep=time.sleep):
    """The scope's cgroup, from a successful query that shows it active; None otherwise."""
    end = time.monotonic() + timeout
    while time.monotonic() < end:
        props, _ = scope_state(unit, query)
        if props and props['ActiveState'] == 'active' and props['ControlGroup'].startswith('/'):
            return props['ControlGroup']
        sleep(0.2)
    return None


def scope_gone(unit, cgroup, query=bounded, timeout=30, sleep=time.sleep, kill=True, cgroup_root='/sys/fs/cgroup'):
    """Kill the scope's whole cgroup and positively verify that it is gone: a successful
    systemd query shows the unit unloaded (or inactive with no cgroup) AND the recorded
    cgroup is gone or empty. Every failure is recorded and leaves gone=False; nothing
    raises."""
    record = {'unit': unit, 'cgroup': cgroup, 'attempts': []}
    if kill:
        try:
            record['kill_exit'] = query(['sudo', '-n', 'systemctl', 'kill', '--signal=SIGKILL', f'{unit}.scope'], check=False).returncode
        except Exception as error:
            record['kill_error'] = repr(error)
    end = time.monotonic() + timeout
    while True:
        props, reason = scope_state(unit, query)
        empty, detail = cgroup_empty_or_gone(cgroup, cgroup_root)
        unit_gone = props is not None and (props['LoadState'] == 'not-found' or
                                           (props['ActiveState'] in ('inactive', 'failed') and props['ControlGroup'] == ''))
        record['attempts'].append({'props': props, 'query_failure': reason, 'cgroup': detail, 'unit_gone': unit_gone})
        if unit_gone and empty:
            record['gone'] = True
            return record
        if time.monotonic() >= end:
            record['gone'] = False
            return record
        sleep(0.5)


def owned(kind, *fields, ledger=None):
    """Append one created resource (exact unit name, or path with its lstat identity) to
    the outer transcript's ledger, so its trap and gate act only on what this run made."""
    with open(ledger or os.environ['KG1_OWNED_LEDGER'], 'a') as f:
        f.write(' '.join([kind] + [str(x) for x in fields]) + '\n')
        f.flush()
        os.fsync(f.fileno())


def require_ledger(what):
    """Real (scope-creating) runs refuse to start outside the outer transcript."""
    if not os.environ.get('KG1_OWNED_LEDGER'):
        print(json.dumps({what: 'refused', 'reason': 'KG1_OWNED_LEDGER not set; run through kg1-reference-transcript.sh'}))
        return False
    return True


def register_unit(unit):
    """Recorded BEFORE systemd-run, so a unit can never exist unrecorded."""
    owned('unit', unit)


def reference(directory, rehearsal=False):
    """The measured reference run, or (`rehearsal`) the same scope, limits, launcher and
    monitor with a trivial monitored command: the launcher resolves the bare `sh`, the
    stack guard execs the pinned interpreter, and the real monitor samples membership and
    telemetry. No preload entry is created in either mode."""
    sys.path.insert(0, HERE)
    import kg1_reference_monitor as monitor
    mode = 'rehearsal' if rehearsal else 'reference'
    if not require_ledger(mode):
        return 3
    os.makedirs(directory, exist_ok=True)
    if preflight(directory) != 0:
        return 3
    unit = f'kg1-1101-{mode}-{os.getpid()}'
    inside = chrooted(directory)
    heartbeat = os.path.join(directory, 'heartbeat.json')
    jailed = (jail('sh', '-c', STACK_GUARD, 'sh', '/usr/bin/python3', chrooted(os.path.abspath(__file__)), 'rehearse-inner', inside)
              if rehearsal else jail('/usr/bin/python3', chrooted(os.path.abspath(__file__)), 'inner', inside))
    command = ['sudo', '-n', 'systemd-run', '--scope', '--quiet', f'--unit={unit}',
               '-p', 'AllowedCPUs=0-3', '-p', 'CPUQuota=400%', '-p', 'MemoryMax=8G', '-p', 'MemorySwapMax=0', '--'] + jailed
    register_unit(unit)
    started = time.time()
    process = subprocess.Popen(command, start_new_session=True)
    identity = {'cgroup': establish_scope(unit)}
    events = []
    if identity['cgroup'] is None:
        events.append({'event': 'scope_identity_not_established'})
    else:
        # Independent of the monitor inside the scope: a stalled or dead monitor (heartbeat
        # not advancing for 15 s, longer than its 10 s bounded cleanup) or a signal to this
        # process kills the whole scope cgroup. watch() never lets a cleanup error escape.
        events = monitor.watch(process, heartbeat, lambda: scope_gone(unit, identity['cgroup']), stall_s=15.0)
    after = scope_gone(unit, identity['cgroup'])
    try:
        process.wait(timeout=60)
    except subprocess.TimeoutExpired:
        events.append({'event': 'scope_process_not_reaped'})
    outer = {'command': command, 'unit': unit, 'scope_cgroup': identity['cgroup'], 'exit': process.returncode,
             'watchdog_events': events, 'scope_after': after, 'elapsed_s': round(time.time() - started, 1)}
    write(os.path.join(directory, 'outer.json'), outer)
    if events or not after['gone']:
        print(json.dumps({'reference': 'MONITOR_INVALID', 'watchdog_events': events, 'scope_gone': after['gone']}))
        return 7
    return process.returncode


# --------------------------------------------------------- inside the scope and chroot

def scope_topology(cgroup, prefix='kg1-1101-reference-'):
    """The verified dedicated scope and every ancestor's restriction (none tighter, none
    empty or malformed)."""
    sys.path.insert(0, HERE)
    import kg1_reference_monitor as monitor
    problems = []
    if not os.path.basename(cgroup).startswith(prefix) or not cgroup.endswith('.scope'):
        problems.append(f'not the dedicated kg1 {prefix} scope: {cgroup}')
    try:
        ancestors = monitor.ancestors(cgroup, read=lambda p: text(p))
    except monitor.Invalid as error:
        return [], problems + [f'ancestors unreadable: {error}']
    for entry in ancestors:
        problems += monitor.ancestor_problems(entry)
    return ancestors, problems


def auxv():
    names = {6: 'AT_PAGESZ', 16: 'AT_HWCAP', 23: 'AT_SECURE', 26: 'AT_HWCAP2', 51: 'AT_MINSIGSTKSZ'}
    import struct
    data = open('/proc/self/auxv', 'rb').read()
    values = {}
    for i in range(0, len(data), 16):
        key, value = struct.unpack('QQ', data[i:i + 16])
        if key in names:
            values[names[key]] = value
    return values


REHEARSAL_COMMAND_S = 5


def inner(directory, rehearsal=False):
    import resource
    os.makedirs(directory, exist_ok=True)
    sys.path.insert(0, HERE)
    import kg1_reference_monitor as monitor
    cgroup = '/sys/fs/cgroup' + text('/proc/self/cgroup').split('::', 1)[-1]
    effective = {name: text(os.path.join(cgroup, name)) for name in LIMITS}
    affinity = sorted(os.sched_getaffinity(0))
    ancestors, problems = scope_topology(cgroup, 'kg1-1101-rehearsal-' if rehearsal else 'kg1-1101-reference-')
    soft, hard = resource.getrlimit(resource.RLIMIT_STACK)
    if effective != LIMITS or affinity != [0, 1, 2, 3]:
        problems.append('effective scope limits or affinity not verified')
    if hard != resource.RLIM_INFINITY:
        problems.append(f'inherited hard RLIMIT_STACK is {hard}, not unlimited')
    diagnostics = subprocess.run(['/lib64/ld-linux-x86-64.so.2', '--list-diagnostics'], env={}, capture_output=True, text=True)
    with open(os.path.join(directory, 'ld-so-diagnostics.txt'), 'w') as f:
        f.write(diagnostics.stdout)
    record = {'mode': 'rehearsal' if rehearsal else 'reference',
              'predicate_version': monitor.PREDICATE_VERSION, 'monitor_sha256': sha256(os.path.join(HERE, 'kg1_reference_monitor.py')),
              'runner_sha256': sha256(os.path.abspath(__file__)), 'cgroup': cgroup, 'expected': LIMITS, 'effective': effective,
              'affinity': affinity, 'ancestors': ancestors, 'stack_rlimit_inherited': {'soft': soft, 'hard': 'unlimited' if hard == resource.RLIM_INFINITY else hard},
              'auxv': auxv(), 'ld_so_diagnostics_sha256': sha256(os.path.join(directory, 'ld-so-diagnostics.txt')),
              'pinned_libc_in_chroot': platform.libc_ver(),
              'documented_deviation': 'owner-approved physical MemTotal 8,323,547,136 B (about 7.75 GiB) < MemoryMax=8G; '
                                      'never a physically clean 8-GiB machine', 'problems': problems}
    if problems:
        write(os.path.join(directory, 'reference.json'), record)
        print(json.dumps({'reference': 'refused', 'problems': problems}))
        return 4
    scratch = os.path.join(directory, 'scratch')
    env = {'PATH': '/usr/bin:/bin', 'HOME': '/bundle', 'KG1_WORKER': os.path.join(BUNDLE, 'feature-off', 'keeper-graph-query'),
           'KG1_UNWIND_CONTROL_WORKER': os.path.join(BUNDLE, 'release', 'keeper-graph-query'),
           'KG1_NATIVE_PREBUILT': os.path.join(BUNDLE, 'native')}

    def soft_stack():
        # Soft 8 MiB only; the hard limit stays unlimited so P6c's case can raise it.
        resource.setrlimit(resource.RLIMIT_STACK, (8 * 1024 * 1024, resource.RLIM_INFINITY))
    # Rehearsal: the same registered monitor (default baseline, interval, thresholds)
    # around a trivial command in the pinned interpreter; no proof, no benchmark.
    proof = ([sys.executable, '-c', f'import time; time.sleep({REHEARSAL_COMMAND_S})'] if rehearsal else
             [sys.executable, os.path.join(BUNDLE, 'scripts', 'graph-proof.py'), os.path.join(BUNDLE, 'target', 'graph-worker'),
              scratch, '--benchmark'])
    with open(os.path.join(directory, 'proof.json'), 'w') as stdout, open(os.path.join(directory, 'proof.err'), 'w') as stderr:
        result = monitor.run_monitored(
            proof, os.path.join(directory, 'samples.jsonl'), lambda: monitor.collect(cgroup),
            env=env, stdout=stdout, stderr=stderr, preexec_fn=soft_stack, self_pid=os.getpid(),
            members=lambda: monitor.scope_members(cgroup), heartbeat_path=os.path.join(directory, 'heartbeat.json'))
    shutil.rmtree(scratch, ignore_errors=True)
    record['monitor'] = result
    write(os.path.join(directory, 'reference.json'), record)
    print(json.dumps({'reference': result['verdict'], 'latched': result['latched'], 'proof_exit': result['proof_exit']}))
    return {'PASS': 0, 'PROOF_FAILED': 1, 'MEMORY_STARVED': 5, 'HOST_CONTENDED': 6, 'MONITOR_INVALID': 7}[result['verdict']]


# --------------------------------------------------------------------- P6c preload

STACK_GUARD = 'test "$(ulimit -H -s)" = unlimited || exit 97; ulimit -S -s 8192 && exec "$@"'


PRELOAD_LINK_TARGET = '/nonexistent/kg1-1101-preload-target'
# Publication through an owned staging inode. The staging entry is created exclusively
# (O_EXCL / symlink), its identity is appended and fsynced to the ledger as both the
# staging and the preload record, and only then is that same inode published at the
# preload path with link(2), which fails on any existing entry. So the preload path is
# only ever ours by recorded identity; a foreign entry (EEXIST) is never adopted.
EXCLUSIVE_PUBLISH = r'''import os, sys
kind, stage, path, ledger = sys.argv[1:5]
if kind == 'file':
    fd = os.open(stage, os.O_WRONLY | os.O_CREAT | os.O_EXCL, 0o644)
    st = os.fstat(fd)
    os.close(fd)
else:
    os.symlink(sys.argv[5], stage)
    st = os.lstat(stage)
with open(ledger, 'a') as f:
    f.write(f'stage {st.st_dev} {st.st_ino} {stage}\nfile {st.st_dev} {st.st_ino} {path}\n')
    f.flush()
    os.fsync(f.fileno())
os.link(stage, path, follow_symlinks=False)
os.unlink(stage)
'''


def ledger_identities(ledger):
    """The last recorded (dev, ino) per path from `file`/`stage` ledger entries."""
    ids = {}
    if os.path.exists(ledger):
        with open(ledger) as f:
            for line in f:
                parts = line.split()
                if len(parts) == 4 and parts[0] in ('file', 'stage'):
                    ids[parts[3]] = (int(parts[1]), int(parts[2]))
    return ids


def preload(directory, in_chroot, root=None, driver_command=None, timeout_s=120):
    """P6c preload refusal controller. `root`/`driver_command` are overridable only for
    the local self-test (the driver then runs as a tracked child of this subreaper); the
    real cases run the bundled driver inside its own transient systemd scope, so the whole
    tree, setsid descendants included, is killed and verified gone through its cgroup."""
    import select
    sys.path.insert(0, HERE)
    import kg1_reference_monitor as monitor
    root = root or ROOT
    if driver_command is None and not require_ledger('preload'):
        return 3
    os.makedirs(directory, exist_ok=True)
    etc = os.path.join(root, 'etc') if in_chroot else '/etc'
    path = os.path.join(etc, 'ld.so.preload')
    stage = os.path.join(etc, f'.kg1-1101-preload-stage-{os.getpid()}')
    for entry in (path, stage):
        if os.path.lexists(entry):
            print(json.dumps({'preload': 'refused', 'reason': f'{entry} exists before the case'}))
            return 3
    # The rootfs /etc belongs to the invoking user; the host /etc needs sudo.
    privileged = [] if in_chroot else ['sudo', '-n']
    saved_mode = os.stat(etc).st_mode & 0o7777

    # Real runs share the transcript's ledger; the local self-test keeps a private one.
    ledger = os.environ.get('KG1_OWNED_LEDGER') if driver_command is None else os.path.join(directory, 'owned.ledger')

    def absent_and_restored():
        return (not os.path.lexists(path) and not os.path.lexists(stage)
                and os.stat(etc).st_mode & 0o7777 == saved_mode)

    def restore():
        if os.stat(etc).st_mode & 0o7777 != saved_mode:
            os.chmod(etc, saved_mode)
        # Ownership is identity only: an entry is removed only if its (dev, ino) is the one
        # recorded before publication. Anything else (a foreign entry after EEXIST, or a
        # staging entry whose identity never reached the ledger) is kept and fails.
        ids, kept = ledger_identities(ledger), []
        for entry in (path, stage):
            if os.path.lexists(entry):
                st = os.lstat(entry)
                if ids.get(entry) != (st.st_dev, st.st_ino):
                    kept.append(entry)
                    continue
                bounded(privileged + ['rm', '-f', entry])
        if kept:
            raise RuntimeError(f'{kept} not created by this case (identity unrecorded or different); not removed')
        if not absent_and_restored():
            raise RuntimeError('restoration not verified')

    def make(kind):
        owned('intent', stage, ledger=ledger)  # before the staging entry can exist
        bounded(privileged + ['python3', '-c', EXCLUSIVE_PUBLISH, kind, stage, path, ledger, PRELOAD_LINK_TARGET])
    variants = {'empty-file': lambda: make('file'), 'dangling-symlink': lambda: make('link')}
    if in_chroot:
        # EACCES: remove every search bit from the private rootfs /etc only (never the
        # host /etc); the non-root, capability-free supervisor then fails the lookup.
        variants['unsearchable-etc'] = lambda: os.chmod(etc, saved_mode & ~0o111)
    driver_args = ['preload', os.path.join(BUNDLE, 'feature-off', 'keeper-graph-query'), os.path.join(directory, 'scratch')] + list(variants)
    driver = os.path.join(BUNDLE, 'target', 'graph-worker', 'keeper-graph-driver')
    unit = None
    if driver_command is not None:
        command = driver_command + list(variants)
        monitor.become_subreaper()
    else:
        unit = f'kg1-1101-preload-{"chroot" if in_chroot else "host"}-{os.getpid()}'
        scope = ['sudo', '-n', 'systemd-run', '--scope', '--quiet', f'--unit={unit}', '--']
        if in_chroot:
            command = scope + jail('sh', '-c', STACK_GUARD, 'sh', chrooted(driver)) + \
                [chrooted(a) if a.startswith(BUNDLE) else a for a in driver_args]
        else:
            command = scope + ['setpriv', f'--reuid={os.getuid()}', f'--regid={os.getgid()}', '--init-groups',
                               'sh', '-c', STACK_GUARD, 'sh', driver] + driver_args
    log, failure, restore_failure, cleanup = [], None, None, None
    stderr_file = open(os.path.join(directory, 'driver.stderr'), 'w')
    if unit is not None:
        register_unit(unit)
    process = subprocess.Popen(command, stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=stderr_file,
                               start_new_session=True)
    scope_cgroup = establish_scope(unit) if unit is not None else None
    fd = process.stdout.fileno()
    os.set_blocking(fd, False)
    buffer = bytearray()

    def next_record(deadline):
        """One complete newline-framed record by the deadline; a partial line, a stall or
        EOF mid-record is an error. Never blocks past the deadline."""
        while b'\n' not in buffer:
            remaining = deadline - time.monotonic()
            if remaining <= 0:
                raise TimeoutError(f'driver record not complete within {timeout_s}s (partial: {bytes(buffer[:80])!r})')
            readable, _, _ = select.select([fd], [], [], remaining)
            if not readable:
                continue
            try:
                chunk = os.read(fd, 65536)
            except BlockingIOError:
                continue
            if not chunk:
                if buffer:
                    raise RuntimeError(f'driver closed stdout mid-record: {bytes(buffer[:80])!r}')
                return None
            buffer.extend(chunk)
        line, _, rest = bytes(buffer).partition(b'\n')
        buffer[:] = rest
        return line.decode()

    def kill_tree():
        if unit is not None:
            state = scope_gone(unit, scope_cgroup)
            leftovers = [p for p in monitor.descendants(os.getpid()) if monitor._alive(p)]
            try:
                process.wait(timeout=30)
            except subprocess.TimeoutExpired:
                return {'verified': False, 'scope': state, 'leader': 'not reaped', 'descendants': leftovers}
            return {'verified': state['gone'] and not leftovers, 'scope': state, 'descendants': leftovers}
        signalled, errors, remaining = monitor.kill_all(process)
        return {'verified': not errors and not remaining, 'signalled': signalled, 'errors': errors, 'remaining': remaining}

    def on_signal(signum, _frame):
        raise monitor.Stop(f'signal {signum}')
    previous = {sig: signal.signal(sig, on_signal) for sig in (signal.SIGINT, signal.SIGTERM, signal.SIGHUP)}
    try:
        while True:
            line = next_record(time.monotonic() + timeout_s)
            if line is None:
                break
            if not line.startswith('{'):
                continue
            record = json.loads(line)
            log.append(record)
            if 'await' not in record:
                continue
            variant = record['variant']
            if record['await'] == 'present':
                variants[variant]()
                log.append({'runner': 'created', 'variant': variant, 'etc_mode': oct(os.stat(etc).st_mode & 0o7777),
                            'stat': run('stat', '-c', '%F %s %N', path) if variant != 'unsearchable-etc' else None})
            else:
                restore()
                log.append({'runner': 'restored', 'variant': variant, 'absent': True, 'etc_mode': oct(saved_mode)})
            process.stdin.write(b'ready\n')
            process.stdin.flush()
        process.wait(timeout=timeout_s)
    except BaseException as error:
        failure = repr(error)
    finally:
        for sig in previous:
            signal.signal(sig, signal.SIG_IGN)
        try:
            try:
                restore()
            except BaseException as error:
                restore_failure = repr(error)
        finally:
            try:
                cleanup = kill_tree()
            except BaseException as error:
                cleanup = {'verified': False, 'error': repr(error)}
            finally:
                stderr_file.close()
                for sig, handler in previous.items():
                    signal.signal(sig, handler)
    restored = absent_and_restored()
    shutil.rmtree(os.path.join(directory, 'scratch'), ignore_errors=True)
    ok = (failure is None and restore_failure is None and restored and cleanup['verified']
          and process.returncode == 0 and any(r.get('complete') for r in log))
    write(os.path.join(directory, 'preload.json'), {'where': 'pinned rootfs (chroot)' if in_chroot else 'host /etc (host libc)',
                                                    'path': path, 'command': command, 'unit': unit, 'driver_exit': process.returncode,
                                                    'failure': failure, 'restore_failure': restore_failure, 'cleanup': cleanup,
                                                    'records': log,
                                                    'stderr_tail': open(os.path.join(directory, 'driver.stderr')).read()[-800:],
                                                    'preload_absent_and_etc_mode_restored': restored, 'pass': ok})
    print(json.dumps({'preload': 'PASS' if ok else 'FAIL', 'where': path, 'driver_exit': process.returncode,
                      'failure': failure, 'restore_failure': restore_failure, 'cleanup_verified': cleanup['verified'],
                      'restored': restored}))
    return 0 if ok else 1

OLD_COMMIT = '82d456086939a75a5801f136a4a27387c4624f08'


def _live(pid):
    try:
        with open(f'/proc/{pid}/stat') as f:
            return f.read().rsplit(')', 1)[1].split()[0] != 'Z'
    except OSError:
        return False


def _kill_pids(pids):
    for pid in pids:
        try:
            os.kill(pid, signal.SIGKILL)
        except (ProcessLookupError, PermissionError):
            pass


def round14_gaps(directory, m, base_sample, series, results, fake_driver):
    """Round-13 review gaps R13-2..R13-5, each with a negative control showing that the
    round-13 code (commit 82d45608, extracted with git) does not catch it."""
    import importlib.util
    old_dir = os.path.join(directory, 'old')
    os.makedirs(old_dir, exist_ok=True)
    for name in ('kg1_reference_monitor.py', 'graph-reference-run.py'):
        with open(os.path.join(old_dir, name), 'w') as f:
            f.write(bounded(['git', '-C', os.path.dirname(HERE), 'show', f'{OLD_COMMIT}:scripts/{name}']).stdout)

    def load(name, path):
        spec = importlib.util.spec_from_file_location(name, path)
        module = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(module)
        return module
    old_m = load('old_kg1_monitor', os.path.join(old_dir, 'kg1_reference_monitor.py'))

    def record(name, new_caught, old_caught, **detail):
        results[f'r14:{name}'] = dict(detail, new_catches=new_caught, old_catches=old_caught,
                                      pass_=None, **{'pass': bool(new_caught) and not old_caught})

    def old_base(t):
        s = base_sample(t)
        s['cpu_stat'] = {'usage_usec': 0}
        return s

    def feed(predicate_cls, samples):
        p = predicate_cls(self_pid=100)
        for s in samples:
            r = p.observe(s)
            if r:
                return r[0]
        return None
    # R13-3: the final snapshot goes through the predicate. The mutation applies only to
    # the final collection: both monitors bind `final = None` just before it, so the
    # caller's frame tells the final call apart from loop samples.
    for name, final_mutation, expected in (
            ('final_snapshot_breach', lambda s: s['events'].update(oom_kill=1), 'MEMORY_STARVED'),
            ('final_snapshot_gap', lambda s: s.update(t=s['t'] + 2.0), 'MONITOR_INVALID'),
            ('final_snapshot_regression', lambda s: s['host_stat']['cpu'].__setitem__(0, 0), 'MONITOR_INVALID')):
        outcome = {}
        for label, module, mk in (('new', m, base_sample), ('old', old_m, old_base)):
            state = {'n': 0}

            def collect(mk=mk, state=state):
                state['n'] += 1
                s = mk(time.monotonic())
                s['procs'] = [os.getpid()]
                for c in s['host_stat']:
                    s['host_stat'][c] = [state['n'] * 100, 0, 0, state['n'] * 100, 0, 0, 0, 0]
                if 'final' in sys._getframe(1).f_locals:
                    final_mutation(s)
                return s
            r = module.run_monitored(['sh', '-c', 'exit 0'], os.path.join(directory, f'{name}-{label}.jsonl'), collect,
                                     baseline_s=0.5, self_pid=os.getpid())
            outcome[label] = r['verdict']
        record(name, outcome['new'] == expected, outcome['old'] != 'PASS', outcome=outcome, expected=expected)
    # R13-5: strict telemetry parsing.
    parse_cases = (
        ('psi_nan', lambda mod: mod.parse_psi('some avg10=nan avg60=0 avg300=0 total=0\nfull avg10=0 avg60=0 avg300=0 total=0\n', 'psi')),
        ('psi_infinite', lambda mod: mod.parse_psi('some avg10=inf avg60=0 avg300=0 total=0\nfull avg10=0 avg60=0 avg300=0 total=0\n', 'psi')),
        ('psi_negative_total', lambda mod: mod.parse_psi('some avg10=0 avg60=0 avg300=0 total=-5\nfull avg10=0 avg60=0 avg300=0 total=0\n', 'psi')),
        ('psi_duplicate_line', lambda mod: mod.parse_psi('some avg10=0 avg60=0 avg300=0 total=0\nsome avg10=9 avg60=0 avg300=0 total=0\nfull avg10=0 avg60=0 avg300=0 total=0\n', 'psi')),
        ('events_negative', lambda mod: mod.parse_keyed('oom -1\noom_kill 0\noom_group_kill 0\nmax 0\nhigh 0\n', 'events', mod.EVENT_KEYS)),
        ('events_duplicate', lambda mod: mod.parse_keyed('oom 0\noom 3\noom_kill 0\noom_group_kill 0\nmax 0\nhigh 0\n', 'events', mod.EVENT_KEYS)),
        ('meminfo_wrong_unit', lambda mod: mod.parse_meminfo('MemTotal: 8 MB\nMemAvailable: 3 MB\nSwapFree: 1 MB\n')),
        ('cpu_stat_missing_throttling', lambda mod: mod.parse_keyed('usage_usec 10\nuser_usec 5\nsystem_usec 5\n', 'cpu.stat',
                                                                     getattr(mod, 'CPU_STAT_KEYS', ('usage_usec',)))),
    )
    for name, call in parse_cases:
        caught = {}
        for label, module in (('new', m), ('old', old_m)):
            try:
                call(module)
                caught[label] = False
            except module.Invalid:
                caught[label] = True
        record(name, caught['new'], caught['old'])
    # R13-5: empty or malformed ancestor cpu.max must not count as verified.
    old_run = load('old_graph_reference_run', os.path.join(old_dir, 'graph-reference-run.py'))
    fake = {'/sys/fs/cgroup/system.slice/memory.max': 'max', '/sys/fs/cgroup/system.slice/memory.swap.max': 'max',
            '/sys/fs/cgroup/system.slice/cpu.max': '', '/sys/fs/cgroup/system.slice/cpuset.cpus.effective': '0-7'}
    cg = '/sys/fs/cgroup/system.slice/kg1-1101-reference-1.scope'
    old_text = old_run.text
    old_run.text = lambda p: fake.get(p, 'unreadable')
    try:
        _, old_problems = old_run.scope_topology(cg)
    finally:
        old_run.text = old_text
    new_problems = m.ancestor_problems({'path': '/sys/fs/cgroup/system.slice', **{k.rsplit('/', 1)[1]: v for k, v in fake.items()}})
    record('ancestor_empty_cpu_max', bool(new_problems), bool(old_problems), new=new_problems, old=old_problems)
    # R13-5: ongoing topology, membership, fresh local counters and complete counter checks.
    def mutate_after(fn, after=2.0):
        def mutate(s, t):
            if t > after:
                fn(s)
        return mutate
    seq_cases = (
        ('ancestor_tightened_mid_run', mutate_after(lambda s: s['ancestors'][0].update({'memory.max': str(4 * 1024 ** 3)}))),
        ('ancestor_changed_mid_run', mutate_after(lambda s: s['ancestors'][0].update({'cpuset.cpus.effective': '0-5'}))),
        ('affinity_changed_mid_run', mutate_after(lambda s: s.update(affinity=[0, 1]))),
        ('member_outside_monitored_tree', mutate_after(lambda s: s.update(procs=[100, 555]))),
        ('initial_local_events_not_fresh', lambda s, t: s['events_local'].update(max=2)),
        ('throttled_counter_regression', lambda s, t: s['cpu_stat'].update(nr_throttled=5 if t < 2 else 1)),
        ('memory_peak_regression', lambda s, t: s.update(memory_peak=900 if t < 2 else 800)),
    )
    for name, mutation in seq_cases:
        new_v = feed(m.Predicate, series(4, mutation))
        old_samples = series(4, mutation)
        old_v = feed(old_m.Predicate, old_samples)
        record(name, new_v == 'MONITOR_INVALID', old_v is not None, new=new_v, old=old_v)
    # R13-4: cleanup when the leader has exited leaving a setsid orphan (fresh processes
    # so the old run is not helped by this process's subreaper bit).
    script = os.path.join(directory, 'cleanup_probe.py')
    with open(script, 'w') as f:
        f.write(r'''import importlib.util, json, os, signal, sys, threading, time
path, mode, pids_file, case = sys.argv[1], sys.argv[2], sys.argv[3], sys.argv[4]
spec = importlib.util.spec_from_file_location('mon', path); m = importlib.util.module_from_spec(spec); spec.loader.exec_module(m)
state = {'n': 0}
def sample():
    state['n'] += 1
    psi = lambda: {'avg10': 0.0, 'avg60': 0.0, 'avg300': 0.0, 'total': 0}
    s = {'t': time.monotonic(), 'wall': 0, 'limits': dict(m.EXPECTED_LIMITS), 'procs': [os.getpid()], 'descendants': [], 'descendants_outside': [],
         'ancestors': [], 'affinity': [0, 1, 2, 3],
         'events': {k: 0 for k in m.EVENT_KEYS}, 'events_local': {k: 0 for k in m.EVENT_KEYS},
         'swap_current': 0, 'swap_peak': 0, 'swap_events': {k: 0 for k in m.SWAP_EVENT_KEYS}, 'memory_current': 1, 'memory_peak': 1,
         'scope_mem_psi': {'some': psi(), 'full': psi()}, 'scope_cpu_psi': {'some': psi()},
         'cpu_stat': {k: 0 for k in getattr(m, 'CPU_STAT_KEYS', ('usage_usec',))}, 'host_mem_available': 3 << 30, 'host_swap_free': 1,
         'host_mem_psi': {'some': psi(), 'full': psi()}, 'host_io_psi': {'some': psi(), 'full': psi()}, 'host_cpu_psi': {'some': psi()},
         'host_stat': {c: [state['n'] * 100, 0, 0, state['n'] * 100, 0, 0, 0, 0] for c in ('cpu',) + m.ASSIGNED_CPUS},
         'host_pswpin': 0, 'host_pswpout': 0}
    if case in ('orphan', 'detached', 'permission') and state['n'] > 12:
        s['events']['oom_kill'] = 1
    if case == 'detached' and state['n'] > 8:
        raise m.Invalid('memory.pressure: empty')
    return s
detached = []
if case == 'detached':
    # A member not descended from this process: double fork via a fresh session.
    os.system(f'setsid sh -c "sleep 300 & echo \\$! >> {pids_file}" < /dev/null > /dev/null 2>&1')
    time.sleep(0.3)
    detached = [int(x) for x in open(pids_file).read().split()]
if case == 'orphan':
    proof = ['sh', '-c', f'setsid sleep 300 & echo $! >> {pids_file}; exit 0']
else:
    proof = ['sleep', '300']
if case == 'signal':
    threading.Timer(2.0, lambda: os.kill(os.getpid(), signal.SIGTERM)).start()
    proof = ['sh', '-c', f'sleep 300 & echo $! >> {pids_file}; wait']
kwargs = dict(baseline_s=0.5, self_pid=os.getpid())
if mode == 'new':
    kwargs['members'] = (lambda: detached)
    if case == 'permission':
        kwargs['members'] = (lambda: [1])
r = m.run_monitored(proof, pids_file + '.jsonl', sample, **kwargs)
print(json.dumps({'verdict': r['verdict'], 'cleanup': r.get('cleanup'), 'latched': r.get('latched')}), flush=True)
''')
    new_path = os.path.join(HERE, 'kg1_reference_monitor.py')
    old_path = os.path.join(old_dir, 'kg1_reference_monitor.py')
    for case in ('orphan', 'detached', 'signal', 'permission'):
        observed = {}
        for label, path in (('new', new_path), ('old', old_path)):
            pids_file = os.path.join(directory, f'{case}-{label}.pids')
            open(pids_file, 'w').close()
            try:
                proc = subprocess.run([sys.executable, script, path, label, pids_file, case], capture_output=True, text=True,
                                      timeout=60, start_new_session=True)
                out = [json.loads(l) for l in proc.stdout.splitlines() if l.startswith('{')]
                verdict_out = out[-1] if out else {'exit': proc.returncode, 'stderr': proc.stderr[-300:]}
            except subprocess.TimeoutExpired:
                verdict_out = {'timeout': True}
            time.sleep(0.3)
            pids = [int(x) for x in open(pids_file).read().split()]
            alive = [p for p in pids if _live(p)]
            _kill_pids(alive)
            observed[label] = {'result': verdict_out, 'spawned': pids, 'alive_after': alive}
        new = observed['new']
        if case == 'permission':
            # A member that cannot be removed (pid 1: EPERM, or ignored SIGKILL inside a pid
            # namespace): the new monitor enforces absence and reports unverified cleanup;
            # the old one reported nothing about cleanup.
            cleanup_report = new['result'].get('cleanup') or {}
            new_ok = new['result'].get('verdict') == 'MEMORY_STARVED' and cleanup_report.get('verified') is False \
                and (1 in cleanup_report.get('remaining', []) or any('kill 1' in e for e in cleanup_report.get('errors', []))) \
                and new['result']['latched'].get('cleanup_unverified')
            old_ok = bool((observed['old']['result'].get('cleanup') or {}).get('errors'))
        elif case == 'signal':
            new_ok = new['result'].get('verdict') == 'MONITOR_INVALID' and new['spawned'] and not new['alive_after'] \
                and new['result']['cleanup']['verified']
            old_ok = bool(observed['old']['result'].get('verdict')) and not observed['old']['alive_after']
        else:
            new_ok = new['result'].get('verdict') in ('MEMORY_STARVED', 'MONITOR_INVALID') and new['spawned'] \
                and not new['alive_after'] and new['result']['cleanup']['verified']
            old_ok = bool(observed['old']['spawned']) and not observed['old']['alive_after']
        record(f'cleanup_{case}', new_ok, old_ok, observed=observed)
    # R13-4: independent watchdog kills a stalled monitor; a live one is left alone.
    fake_inner = os.path.join(directory, 'fake_inner.py')
    with open(fake_inner, 'w') as f:
        f.write('import json, os, sys, time\nhb, beats = sys.argv[1], int(sys.argv[2])\n'
                'for n in range(beats):\n'
                '    open(hb + ".tmp", "w").write(json.dumps({"seq": n + 1})); os.replace(hb + ".tmp", hb); time.sleep(0.2)\n'
                'time.sleep(300 if beats < 20 else 0)\n')
    for label, beats, expect_kill in (('stalled_monitor_killed', 5, True), ('live_monitor_left_alone', 20, False)):
        hb = os.path.join(directory, f'{label}.heartbeat')
        p = subprocess.Popen([sys.executable, fake_inner, hb, str(beats)], start_new_session=True)
        started = time.monotonic()
        events = m.watch(p, hb, lambda p=p: (os.killpg(p.pid, signal.SIGKILL), 'killed')[1], stall_s=1.5, poll_s=0.2)
        killed = any(e['event'] == 'watchdog_kill' for e in events)
        results[f'r14:watchdog_{label}'] = {'events': events, 'elapsed_s': round(time.monotonic() - started, 1),
                                            'pass': killed == expect_kill and p.poll() is not None,
                                            'old': 'round-13 reference() had no watchdog (blocking subprocess.run)'}
    # R13-2: controller failure paths, new versus old preload.
    driver = os.path.join(directory, 'fault_driver.py')
    with open(driver, 'w') as f:
        f.write(r'''import json, os, subprocess, sys, time
etc, mode, pids = sys.argv[1], sys.argv[2], sys.argv[3]
open(pids, 'a').write('%d\n' % os.getpid())
if mode == 'partial':
    sys.stdout.write('{"await": "pres'); sys.stdout.flush(); time.sleep(300)
if mode == 'orphan':
    c = subprocess.Popen(['setsid', 'sleep', '300']); open(pids, 'a').write('%d\n' % c.pid)
    print(json.dumps({'await': 'present', 'variant': 'empty-file'}), flush=True); time.sleep(0.5); sys.exit(5)
if mode == 'restore':
    print(json.dumps({'await': 'present', 'variant': 'unsearchable-etc'}), flush=True)
    sys.stdin.readline()
    print(json.dumps({'await': 'absent', 'variant': 'unsearchable-etc'}), flush=True)
    time.sleep(300)
''')
    harness = os.path.join(directory, 'controller_probe.py')
    with open(harness, 'w') as f:
        f.write(r'''import importlib.util, json, os, sys
runner, root, out, mode, pids, fault = sys.argv[1:7]
spec = importlib.util.spec_from_file_location('runner', runner); r = importlib.util.module_from_spec(spec); spec.loader.exec_module(r)
etc = os.path.join(root, 'etc')
if fault == 'chmod':
    real = os.chmod; calls = {'n': 0}
    def failing(path, mode_, *a, **k):
        calls['n'] += 1
        if calls['n'] > 1 and os.path.abspath(path) == os.path.abspath(etc):
            raise PermissionError('injected restore failure')
        return real(path, mode_, *a, **k)
    os.chmod = failing
code = r.preload(out, True, root=root, driver_command=[sys.executable, sys.argv[7], etc, mode, pids], timeout_s=3)
print(json.dumps({'exit': code}), flush=True)
''')
    old_runner = os.path.join(old_dir, 'graph-reference-run.py')
    for case, mode, fault in (('partial_line_stall', 'partial', 'none'), ('restore_exception', 'restore', 'chmod'),
                              ('exited_leader_setsid_orphan', 'orphan', 'none')):
        observed = {}
        for label, runner_path in (('new', os.path.abspath(__file__)), ('old', old_runner)):
            root = os.path.join(directory, f'ctl-{case}-{label}')
            os.makedirs(os.path.join(root, 'etc'))
            os.chmod(os.path.join(root, 'etc'), 0o755)
            pids = os.path.join(directory, f'ctl-{case}-{label}.pids')
            open(pids, 'w').close()
            out_dir = os.path.join(directory, f'ctl-{case}-{label}-out')
            started = time.monotonic()
            try:
                proc = subprocess.run([sys.executable, harness, runner_path, root, out_dir, mode, pids, fault, driver],
                                      capture_output=True, text=True, timeout=25, start_new_session=True)
                finished = {'exit': proc.returncode, 'stdout': proc.stdout[-300:]}
            except subprocess.TimeoutExpired:
                finished = {'timeout_after_s': 25}
            elapsed = round(time.monotonic() - started, 1)
            time.sleep(0.3)
            spawned = [int(x) for x in open(pids).read().split()]
            alive = [p for p in spawned if _live(p)]
            _kill_pids(alive)
            etc = os.path.join(root, 'etc')
            mode_after = oct(os.stat(etc).st_mode & 0o7777)
            os.chmod(etc, 0o755)
            record_path = os.path.join(out_dir, 'preload.json')
            rec = json.load(open(record_path)) if os.path.exists(record_path) else None
            observed[label] = {'finished': finished, 'elapsed_s': elapsed, 'spawned': spawned, 'alive_after': alive,
                               'etc_mode_after': mode_after,
                               'record': {k: rec.get(k) for k in ('failure', 'restore_failure', 'cleanup', 'pass')} if rec else None}
            shutil.rmtree(root)
        new = observed['new']
        new_ok = 'exit' in new['finished'] and new['record'] is not None and new['record']['pass'] is False \
            and not new['alive_after'] and new['record']['cleanup'] and new['record']['cleanup'].get('verified') \
            and (new['etc_mode_after'] == '0o755' or case == 'restore_exception')
        if case == 'restore_exception':
            new_ok = new_ok and bool(new['record']['restore_failure'])
        old = observed['old']
        old_ok = 'exit' in old['finished'] and not old['alive_after']
        record(case, new_ok, old_ok, observed=observed)

ROUND14_COMMIT = 'fa4cf120'
ROUND15_COMMIT = 'fad43562'
ROUND16_COMMIT = 'd6c9c326'
ROUND17_COMMIT = '970018e9'
ROUND19_COMMIT = '059ab19c'

# The round-14 submitted transcript (report ROUND 14, "Exact rerun transcript plan"),
# verbatim except that its rootfs comes from $1 and its host /etc from $HOST_ETC.
ROUND14_TRANSCRIPT = r'''set -u
B=$1
OUT=$B/bundle/out
R=$B/bundle/scripts/graph-reference-run.py
before_df=$(df -B1 / | tail -1); before_mounts=$(findmnt -rn | sort | sha256sum); before_units=$(systemctl list-units --all --no-legend 'kg1-*' | wc -l)
test ! -e /etc/ld.so.preload || { echo HOST_PRELOAD_PRESENT; exit 3; }
saved_etc_mode=$(stat -c %a $B/etc)
# outer trap: independent of the runner, restores host and private preload state on any exit
trap 'sudo -n rm -f /etc/ld.so.preload; chmod $saved_etc_mode $B/etc; rm -f $B/etc/ld.so.preload;
      for u in $(systemctl list-units --all --no-legend --plain "kg1-1101-*" | cut -d" " -f1); do sudo -n systemctl kill --signal=SIGKILL "$u"; done;
      test ! -e /etc/ld.so.preload && echo HOST_PRELOAD_ABSENT || echo HOST_PRELOAD_LEFT' EXIT INT TERM HUP
cd $B
python3 $R preflight       $OUT/preflight        ; echo preflight=$?
python3 $R preload-chroot  $OUT/preload-chroot   ; echo preload_chroot=$?   # empty-file, dangling-symlink, unsearchable-etc
python3 $R preload-host    $OUT/preload-host     ; echo preload_host=$?     # empty-file, dangling-symlink
python3 $R reference       $OUT/reference        ; echo reference=$?        # 0 PASS, 1 PROOF_FAILED, 5/6/7 STARVED/CONTENDED/INVALID; no retry
echo "$before_df"; df -B1 / | tail -1
test "$(findmnt -rn | sort | sha256sum)" = "$before_mounts" && echo MOUNTS_UNCHANGED || echo MOUNTS_CHANGED
echo units_after=$(systemctl list-units --all --no-legend 'kg1-*' | wc -l)
test "$(stat -c %a $B/etc)" = "$saved_etc_mode" && echo PRIVATE_ETC_MODE_RESTORED
'''.replace('$B/etc/ld.so.preload', '@PRIVATE@').replace('/etc/ld.so.preload', '$HOST_ETC/ld.so.preload').replace('@PRIVATE@', '$B/etc/ld.so.preload')

# The round-14 collection and removal steps, with `ssh delectra` as $REMOTE.
ROUND14_COLLECT = r'''$REMOTE "cd $DIR/rootfs/bundle && tar -cf - out rootfs-manifest.json SHA256SUMS TOPOLOGY.json" | tar -C "$EVID" -xf -
$REMOTE "rm -rf $DIR; test ! -e $DIR && echo BUNDLE_REMOVED"
'''

FAKE_RUNNER = r'''import json, os, subprocess, sys, time
stage, d = sys.argv[1], sys.argv[2]
os.makedirs(d, exist_ok=True)
open(os.path.join(d, 'ran'), 'w').close()
plan = json.load(open(os.environ['FAKE_PLAN'])).get(stage, {})
ledger = os.environ.get('KG1_OWNED_LEDGER')
def own(*fields):
    if ledger:
        with open(ledger, 'a') as f:
            f.write(' '.join(map(str, fields)) + '\n')
            f.flush()
            os.fsync(f.fileno())
host = os.path.join(os.environ['KG1_TEST_HOST_ETC'], 'ld.so.preload')
private = os.path.join(os.environ['FAKE_ROOTFS'], 'etc', 'ld.so.preload')
units = os.path.join(os.environ['FAKE_SYSTEMD'], 'units')
for name, sticky in plan.get('units', []):
    own('unit', name)
    open(os.path.join(units, name), 'w').write('sticky' if sticky else '')
for which in plan.get('files', []):
    path = host if which == 'host' else private
    open(path, 'w').close()
    st = os.lstat(path)
    own('file', st.st_dev, st.st_ino, path)
if plan.get('replace_host'):
    open(host + '.new', 'w').write('foreign')
    os.replace(host + '.new', host)
if plan.get('stray'):
    open(os.path.join(os.environ['FAKE_ROOTFS'], 'bundle', 'stray'), 'w').close()
if plan.get('unregistered_unit'):
    open(os.path.join(units, 'kg1-1101-reference-999'), 'w').close()
if plan.get('orphan'):
    p = subprocess.Popen(['sleep', '60'], cwd=os.environ['FAKE_ROOTFS'], start_new_session=True)
    open(os.environ['FAKE_ORPHAN'], 'w').write(str(p.pid))
if plan.get('unreadable'):
    path = os.path.join(d, 'secret')
    open(path, 'w').write('x')
    os.chmod(path, 0)
if plan.get('unreadable_orphan'):
    open(os.path.join(os.environ['FAKE_SYSTEMD'], 'unreadable_pid'), 'w').write(str(p.pid))
if plan.get('unreadable_stat_orphan'):
    open(os.path.join(os.environ['FAKE_SYSTEMD'], 'unreadable_stat_pid'), 'w').write(str(p.pid))
for marker in ('hang', 'census_fail', 'stat_hang'):
    if plan.get(marker):
        open(os.path.join(os.environ['FAKE_SYSTEMD'], marker), 'w').close()
for which, kind, mode in plan.get('publish', []):
    # The runner's real publication protocol (EXCLUSIVE_PUBLISH): intent for the staging
    # path, then the helper. mode: 'die' = SIGKILL right after publication; 'foreign' =
    # a foreign empty entry appears at the preload path first (EEXIST).
    etc = os.environ['KG1_TEST_HOST_ETC'] if which == 'host' else os.path.join(os.environ['FAKE_ROOTFS'], 'etc')
    path, staging = os.path.join(etc, 'ld.so.preload'), os.path.join(etc, f'.kg1-1101-preload-stage-{os.getpid()}')
    own('intent', staging)
    if mode == 'foreign':
        open(path, 'w').close()
    r = subprocess.run([sys.executable, os.environ['FAKE_PUBLISH'], kind, staging, path, ledger,
                        '/nonexistent/kg1-1101-preload-target'])
    if mode == 'die':
        os.kill(os.getpid(), 9)
    if r.returncode != 0:
        sys.exit(1)
for which, content, die in plan.get('intent_files', []):
    # Intent registered, then the exclusive publication; `die` = SIGKILL before the
    # identity record, the window a controller interruption can hit.
    path = host if which == 'host' else private
    own('intent', path)
    fd = os.open(path, os.O_WRONLY | os.O_CREAT | os.O_EXCL, 0o644)
    os.write(fd, content.encode())
    os.close(fd)
    if die:
        os.kill(os.getpid(), 9)
time.sleep(plan.get('sleep', 0))
sys.exit(plan.get('exit', 0))
'''

FAKE_BIN = {
    'sudo': '#!/bin/sh\n[ "$1" = -n ] && shift\n'
            '[ "$1" = sh ] && [ -e "$FAKE_SYSTEMD/census_fail" ] && { echo "sudo: refused" >&2; exit 1; }\nexec "$@"\n',
    'findmnt': '#!/bin/sh\ncat "$FAKE_MOUNTS"\n',
    'readlink': '#!/bin/sh\nif [ -e "$FAKE_SYSTEMD/unreadable_pid" ]; then\n'
                '  case "$1" in "/proc/$(cat "$FAKE_SYSTEMD/unreadable_pid")/"*) exit 1;; esac\nfi\nexec "$REAL_READLINK" "$@"\n',
    'cat': '#!/bin/sh\nif [ -e "$FAKE_SYSTEMD/unreadable_stat_pid" ] && '
           '[ "$1" = "/proc/$("$REAL_CAT" "$FAKE_SYSTEMD/unreadable_stat_pid")/stat" ]; then exit 1; fi\nexec "$REAL_CAT" "$@"\n',
    'stat': '#!/bin/sh\nif [ -e "$FAKE_SYSTEMD/stat_hang" ] && [ "$1" = -c ] && [ "$2" = %a ]; then exec sleep 3600; fi\n'
            'exec "$REAL_STAT" "$@"\n',
    'systemctl': r'''#!/bin/sh
S=$FAKE_SYSTEMD
[ -e "$S/hang" ] && exec sleep 3600
[ -e "$S/fail" ] && exit 1
case "$1" in
  list-units) for f in "$S"/units/*; do [ -e "$f" ] && echo "$(basename "$f").scope loaded active running fake"; done; exit 0;;
  kill) u=$(basename "${3%.scope}"); f="$S/units/$u"; [ -e "$f" ] && [ "$(cat "$f")" != sticky ] && rm -f "$f"; exit 0;;
  show) u=$(basename "${5%.scope}"); [ -e "$S/units/$u" ] && echo loaded || echo not-found; exit 0;;
esac
exit 1
''',
}


def round15_gaps(directory, m, base_sample, results):
    """Round-14 review blockers R14-1..R14-3, each with a negative control showing that
    the round-14 code (commit fa4cf120, extracted with git; the transcript and collection
    steps from the round-14 report) does not catch it."""
    import importlib.util
    old_dir = os.path.join(directory, 'old15')
    os.makedirs(old_dir, exist_ok=True)
    for name in ('kg1_reference_monitor.py', 'graph-reference-run.py'):
        with open(os.path.join(old_dir, name), 'w') as f:
            f.write(bounded(['git', '-C', os.path.dirname(HERE), 'show', f'{ROUND14_COMMIT}:scripts/{name}']).stdout)

    def load(name, path):
        spec = importlib.util.spec_from_file_location(name, path)
        module = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(module)
        return module
    old_m = load('old15_monitor', os.path.join(old_dir, 'kg1_reference_monitor.py'))
    old_r = load('old15_runner', os.path.join(old_dir, 'graph-reference-run.py'))

    def record(name, new_caught, old_caught, **detail):
        results[f'r15:{name}'] = dict(detail, new_catches=bool(new_caught), old_catches=bool(old_caught),
                                      **{'pass': bool(new_caught) and not old_caught})

    class Result:
        def __init__(self, returncode, stdout=''):
            self.returncode, self.stdout, self.stderr = returncode, stdout, ''

    def show(props):
        return '\n'.join(f'{k}={v}' for k, v in props.items()) + '\n'

    # ---- R14-1: scope absence is a positive observation, never a failed/empty query.
    cg_root = os.path.join(directory, 'cgroupfs')
    live = os.path.join(cg_root, 'system.slice', 'u.scope')
    os.makedirs(live, exist_ok=True)
    with open(os.path.join(live, 'cgroup.procs'), 'w') as f:
        f.write('4242\n')
    nosleep = lambda _s: None
    for name, new_reply, old_reply, cgroup in (
            ('show_failed_is_not_gone', Result(1), Result(1), '/system.slice/u.scope'),
            ('show_empty_is_not_gone', Result(0, ''), Result(0, ''), '/system.slice/u.scope'),
            ('unit_unloaded_but_cgroup_populated', Result(0, show({'LoadState': 'not-found', 'ActiveState': 'inactive', 'SubState': 'dead', 'ControlGroup': ''})),
             Result(0, 'inactive\n'), '/system.slice/u.scope'),
            ('identity_never_established', Result(0, show({'LoadState': 'not-found', 'ActiveState': 'inactive', 'SubState': 'dead', 'ControlGroup': ''})),
             Result(0, 'inactive\n'), None)):
        new = scope_gone('u', cgroup, query=lambda c, check=True, r=new_reply: r, timeout=0, sleep=nosleep, cgroup_root=cg_root)
        saved = old_r.bounded
        old_r.bounded = lambda c, timeout=30, check=True, r=old_reply: r
        try:
            old = old_r.scope_gone('u', timeout=5)
        finally:
            old_r.bounded = saved
        record(name, new['gone'] is False, old['gone'] is False, new=new['gone'], old=old['gone'])
    gone = scope_gone('u', '/system.slice/absent.scope',
                      query=lambda c, check=True: Result(0, show({'LoadState': 'not-found', 'ActiveState': 'inactive', 'SubState': 'dead', 'ControlGroup': ''})),
                      timeout=0, sleep=nosleep, cgroup_root=cg_root)
    record('positive:unloaded_and_cgroup_gone_is_gone', gone['gone'] is True, False, new=gone['gone'])

    def raises():
        raise RuntimeError("['systemctl', 'show'] exceeded 30s")
    outcome = {}
    for label, module in (('new', m), ('old', old_m)):
        process = subprocess.Popen(['sleep', '1'])
        try:
            outcome[label] = {'events': module.watch(process, os.path.join(directory, 'no-heartbeat.json'), raises,
                                                     first_beat_s=0.2, poll_s=0.05)}
        except BaseException as error:
            outcome[label] = {'escaped': repr(error)}
        finally:
            process.kill()
            process.wait()
    new_ev = [e['event'] for e in outcome['new'].get('events', [])]
    record('cleanup_timeout_stays_under_watchdog', 'escaped' not in outcome['new'] and 'kill_scope_failed' in new_ev,
           'escaped' not in outcome['old'], outcome=outcome)

    # ---- R14-2: descendants must be inside the scope; an unreadable subtree is invalid.
    s = base_sample(1.0)
    s['descendants'] = [200]
    s['descendants_outside'] = m.outside_scope('/system.slice/u.scope', [200], read=lambda pid: '0::/user.slice/elsewhere\n')
    new_v = m.Predicate(self_pid=100).observe(s)
    old_v = old_m.Predicate(self_pid=100).observe(s)
    record('descendant_outside_scope', new_v and new_v[0] == 'MONITOR_INVALID', old_v is not None, new=new_v, old=old_v)
    inside = m.outside_scope('/system.slice/u.scope', [200], read=lambda pid: '0::/system.slice/u.scope/child\n')
    record('positive:descendant_in_child_cgroup_is_inside', inside == [], False, new=inside)
    for name, build in (('unlistable_subtree', lambda t: os.chmod(os.path.join(t, 'child'), 0)),
                        ('cgroup_dir_without_procs', lambda t: os.remove(os.path.join(t, 'child', 'cgroup.procs')))):
        tree = os.path.join(directory, name)
        os.makedirs(os.path.join(tree, 'child'), exist_ok=True)
        for d in (tree, os.path.join(tree, 'child')):
            with open(os.path.join(d, 'cgroup.procs'), 'w') as f:
                f.write('')
        build(tree)
        caught = {}
        for label, module in (('new', m), ('old', old_m)):
            try:
                caught[label] = f'members {module.scope_members(tree)}'
            except module.Invalid as error:
                caught[label] = f'Invalid: {error}'
        os.chmod(os.path.join(tree, 'child'), 0o755)
        record(name, caught['new'].startswith('Invalid'), caught['old'].startswith('Invalid'), observed=caught)
    try:
        m.outside_scope('/s', [os.getpid()], read=lambda pid: (_ for _ in ()).throw(PermissionError('denied')))
        unreadable = 'not raised'
    except m.Invalid as error:
        unreadable = f'Invalid: {error}'
    record('descendant_cgroup_unreadable', unreadable.startswith('Invalid'), hasattr(old_m, 'outside_scope'), new=unreadable)

    # ---- R14-3: the outer transcript and collection.
    transcript = os.path.join(HERE, 'kg1-reference-transcript.sh')
    orchestrator = os.path.join(HERE, 'kg1-reference-delectra.sh')
    old_transcript = os.path.join(directory, 'round14-transcript.sh')
    with open(old_transcript, 'w') as f:
        f.write(ROUND14_TRANSCRIPT)

    def world(name, plan):
        w = os.path.join(directory, 'transcript', name)
        shutil.rmtree(w, ignore_errors=True)
        paths = {k: os.path.join(w, k) for k in ('bin', 'hostetc', 'systemd', 'src')}
        for p in paths.values():
            os.makedirs(p)
        os.makedirs(os.path.join(paths['systemd'], 'units'))
        rootfs = os.path.join(paths['src'], 'rootfs')
        os.makedirs(os.path.join(rootfs, 'etc'))
        os.makedirs(os.path.join(rootfs, 'bundle', 'scripts'))
        os.makedirs(os.path.join(rootfs, 'bundle', 'out'))  # shipped empty, as `bundle` makes it
        shutil.copy2(transcript, os.path.join(rootfs, 'bundle', 'scripts', 'kg1-reference-transcript.sh'))
        with open(os.path.join(rootfs, 'bundle', 'scripts', 'graph-reference-run.py'), 'w') as f:
            f.write(FAKE_RUNNER)
        for name_, content in (('TOPOLOGY.json', '{}'), ('rootfs-manifest.json', '{}')):
            with open(os.path.join(rootfs, 'bundle', name_), 'w') as f:
                f.write(content)
        # Rootfs-relative, exactly as `bundle` writes SHA256SUMS.
        sums = bounded(['sh', '-c', 'cd "$1" && sha256sum bundle/scripts/* bundle/TOPOLOGY.json bundle/rootfs-manifest.json', 'sh',
                        rootfs]).stdout
        with open(os.path.join(rootfs, 'bundle', 'SHA256SUMS'), 'w') as f:
            f.write(sums)
        for tool, body in FAKE_BIN.items():
            with open(os.path.join(paths['bin'], tool), 'w') as f:
                f.write(body)
            os.chmod(os.path.join(paths['bin'], tool), 0o755)
        with open(os.path.join(w, 'mounts'), 'w') as f:
            f.write('/ /dev/root ext4 rw\n')
        with open(os.path.join(w, 'plan.json'), 'w') as f:
            json.dump(plan, f)
        env = dict(os.environ, PATH=paths['bin'] + ':' + os.environ['PATH'], FAKE_SYSTEMD=paths['systemd'],
                   FAKE_MOUNTS=os.path.join(w, 'mounts'), FAKE_PLAN=os.path.join(w, 'plan.json'),
                   FAKE_ROOTFS=rootfs, FAKE_ORPHAN=os.path.join(w, 'orphan.pid'),
                   KG1_TEST_HOST_ETC=paths['hostetc'], HOST_ETC=paths['hostetc'], REAL_READLINK=shutil.which('readlink'),
                   REAL_STAT=shutil.which('stat'), REAL_CAT=shutil.which('cat'), FAKE_PUBLISH=os.path.join(w, 'publish.py'))
        with open(os.path.join(w, 'publish.py'), 'w') as f:
            f.write(EXCLUSIVE_PUBLISH)
        return dict(paths, w=w, rootfs=rootfs, env=env, out=os.path.join(rootfs, 'bundle', 'out'),
                    host=os.path.join(paths['hostetc'], 'ld.so.preload'))

    def go(w, script, signal_after=None, timeout=150, args=()):  # above the transcript's 60 s unit deadline
        started = time.monotonic()
        p = subprocess.Popen(['bash', script, w['rootfs'], *args], env=w['env'], stdout=subprocess.PIPE,
                             stderr=subprocess.STDOUT, text=True, start_new_session=True)
        if signal_after is not None:
            marker = os.path.join(w['out'], signal_after, 'ran')
            while not os.path.exists(marker) and time.monotonic() - started < 20:
                time.sleep(0.05)
            time.sleep(0.2)
            p.send_signal(signal.SIGTERM)
        try:
            out, _ = p.communicate(timeout=timeout)
        except subprocess.TimeoutExpired:
            os.killpg(p.pid, signal.SIGKILL)
            out, _ = p.communicate()
        orphan = os.path.join(w['w'], 'orphan.pid')
        if os.path.exists(orphan):
            _kill_pids([int(open(orphan).read())])
            os.remove(orphan)
        log = os.path.join(w['out'], 'transcript.log')
        return {'rc': p.returncode, 'elapsed': round(time.monotonic() - started, 2), 'output': out[-1500:],
                'log': open(log).read() if os.path.exists(log) else '',
                'ran': sorted(s for s in ('preflight', 'preload-chroot', 'preload-host', 'reference', 'rehearsal')
                              if os.path.exists(os.path.join(w['out'], s, 'ran')))}

    def both(name, plan, setup=lambda w: None, **kw):
        runs = {}
        for label, script in (('new', transcript), ('old', old_transcript)):
            w = world(f'{name}-{label}', plan)
            setup(w)
            runs[label] = dict(go(w, script, **kw), world=w)
        return runs

    all_stages = ['preflight', 'preload-chroot', 'preload-host', 'reference']
    # Clean run: every stage, every gate passes (positive control).
    r = both('clean', {'preload-host': {'units': [['kg1-1101-preload-host-1', False]], 'files': ['host']}})
    record('positive:clean_run_passes_and_removes_owned', r['new']['rc'] == 0 and r['new']['ran'] == all_stages
           and 'TRANSCRIPT result=PASS' in r['new']['log'] and not os.path.lexists(r['new']['world']['host']), False,
           new={k: r['new'][k] for k in ('rc', 'ran', 'log')})
    # A preexisting dangling symlink: refused before anything is created, and left alone.
    r = both('dangling', {}, setup=lambda w: os.symlink('/nonexistent/user-preload', w['host']))

    def refused_untouched(x):
        return x['rc'] == 3 and not x['ran'] and not os.listdir(x['world']['out']) and os.path.islink(x['world']['host'])
    record('preexisting_dangling_preload_refused_before_creating', refused_untouched(r['new']), refused_untouched(r['old']),
           new={k: r['new'][k] for k in ('rc', 'ran', 'output')}, old={k: r['old'][k] for k in ('rc', 'ran')},
           old_symlink_kept=os.path.islink(r['old']['world']['host']))
    # A stage that fails stops the transcript.
    r = both('stage_fails', {'preflight': {'exit': 3}})
    record('first_failed_stage_stops', r['new']['rc'] == 3 and r['new']['ran'] == ['preflight'], r['old']['ran'] == ['preflight'],
           new={k: r['new'][k] for k in ('rc', 'ran')}, old={k: r['old'][k] for k in ('rc', 'ran')})
    # A signal during a stage stops it promptly and nothing after it runs.
    r = both('signal', {'preload-chroot': {'sleep': 3}}, signal_after='preload-chroot')
    record('signal_stops_without_resuming', r['new']['rc'] == 143 and r['new']['ran'] == ['preflight', 'preload-chroot']
           and r['new']['elapsed'] < 3 and 'resumed=no' in r['new']['log'],
           r['old']['ran'] == ['preflight', 'preload-chroot'],
           new={k: r['new'][k] for k in ('rc', 'ran', 'elapsed')}, old={k: r['old'][k] for k in ('rc', 'ran', 'elapsed')})
    # The trap removes only what the run created: a replaced (foreign) host preload stays.
    r = both('foreign', {'preload-host': {'files': ['host'], 'replace_host': True}})
    record('trap_keeps_file_with_other_identity', r['new']['rc'] == 8 and os.path.exists(r['new']['world']['host'])
           and 'file_identity=FAIL' in r['new']['log'], os.path.exists(r['old']['world']['host']),
           new={k: r['new'][k] for k in ('rc', 'log')}, old_removed=not os.path.exists(r['old']['world']['host']))
    # Owned-resource gates.
    for name, plan, needle in (
            ('gate_registered_unit_not_gone', {'reference': {'units': [['kg1-1101-reference-7', True]]}}, 'unit_gone=FAIL'),
            ('gate_unregistered_unit', {'reference': {'unregistered_unit': True}}, 'no_kg1_units=FAIL'),
            ('gate_rootfs_changed_outside_out', {'reference': {'stray': True}}, 'rootfs_unchanged_outside_out=FAIL'),
            ('gate_owned_process_left', {'reference': {'orphan': True}}, 'no_owned_processes=FAIL')):
        r = both(name, plan)
        record(name, r['new']['rc'] == 8 and needle in r['new']['log'], r['old']['rc'] != 0,
               new={k: r['new'][k] for k in ('rc', 'log')}, old={k: r['old'][k] for k in ('rc', 'output')})
    # Collection: evidence is copied and verified before anything on delectra is removed.

    def orchestrate(name, plan, old=False, script_path=None, args=()):
        w = world(name, plan)
        home = os.path.join(w['w'], 'remote-home')
        os.makedirs(home)
        evid = os.path.join(w['w'], 'evidence')
        env = dict(w['env'], KG1_REMOTE='sh -c', KG1_REMOTE_DIR='kg1-1101-ref')
        if old:
            # The round-14 plan: ship, run, then copy back and remove unconditionally.
            os.makedirs(evid)
            script = (f'set -u; REMOTE="sh -c"; DIR=kg1-1101-ref; EVID={evid}; mkdir $DIR; '
                      f'tar -C {w["src"]} -cf - rootfs | tar -C $DIR -xpf -; '
                      f'bash $DIR/rootfs/bundle/scripts/kg1-reference-transcript.sh $(cd $DIR/rootfs && pwd -P); ' + ROUND14_COLLECT)
            p = subprocess.run(['bash', '-c', script], cwd=home, env=env, capture_output=True, text=True, timeout=120)
        else:
            p = subprocess.run(['bash', script_path or orchestrator, w['rootfs'], evid, *args], cwd=home, env=env,
                               capture_output=True, text=True, timeout=120)
        remote_dir = os.path.join(home, 'kg1-1101-ref')
        kept = os.path.exists(remote_dir)
        if kept:
            bounded(['chmod', '-R', 'u+rwX', remote_dir])
        return {'rc': p.returncode, 'remote_kept': kept, 'stdout': p.stdout[-1200:], 'stderr': p.stderr[-600:],
                'evidence_log': open(os.path.join(evid, 'out', 'transcript.log')).read()
                if os.path.exists(os.path.join(evid, 'out', 'transcript.log')) else ''}
    ok = orchestrate('collect-clean', {})
    record('positive:collect_verified_then_removed', ok['rc'] == 0 and not ok['remote_kept']
           and 'TRANSCRIPT result=PASS' in ok['evidence_log'] and 'COLLECT verify=PASS' in ok['stdout'], False, new=ok)
    new = orchestrate('collect-unreadable-new', {'reference': {'unreadable': True}})
    old = orchestrate('collect-unreadable-old', {'reference': {'unreadable': True}}, old=True)
    record('unverified_collection_keeps_remote', new['rc'] == 9 and new['remote_kept'] and 'removed=no' in new['stdout'],
           old['remote_kept'], new=new, old=old)

    # ================= Round-15 review R15-1..R15-4, old = checkpoint fad43562 ==========
    import tempfile
    old16_dir = os.path.join(directory, 'old16')
    os.makedirs(old16_dir, exist_ok=True)
    for name in ('graph-reference-run.py', 'kg1-reference-transcript.sh', 'kg1-reference-delectra.sh'):
        with open(os.path.join(old16_dir, name), 'w') as f:
            f.write(bounded(['git', '-C', os.path.dirname(HERE), 'show', f'{ROUND15_COMMIT}:scripts/{name}']).stdout)
    old16_r = load('old16_runner', os.path.join(old16_dir, 'graph-reference-run.py'))
    old16_transcript = os.path.join(old16_dir, 'kg1-reference-transcript.sh')
    old16_orchestrator = os.path.join(old16_dir, 'kg1-reference-delectra.sh')

    def record16(name, new_caught, old_caught, **detail):
        results[f'r16:{name}'] = dict(detail, new_catches=bool(new_caught), old_catches=bool(old_caught),
                                      **{'pass': bool(new_caught) and not old_caught})

    def pair(name, plan, timeout=(60, 60)):
        runs = {}
        for (label, script), limit in zip((('new', transcript), ('old', old16_transcript)), timeout):
            w = world(f'r16-{name}-{label}', plan)
            runs[label] = dict(go(w, script, timeout=limit), world=w)
        return runs

    # R15-1 (controller): a signal lands after the exclusive publication, before the
    # identity is recorded. The runner's own restoration must still remove our entry.
    fake_driver = os.path.join(directory, 'fake_driver.py')
    outcome = {}
    for label, module in (('new', sys.modules[__name__]), ('old', old16_r)):
        root = tempfile.mkdtemp(prefix=f'kg1-r16-publish-{label}-', dir=directory)
        etc = os.path.join(root, 'etc')
        os.makedirs(etc)
        target = os.path.join(etc, 'ld.so.preload')
        real = module.bounded
        fired = []

        def interrupting(command, timeout=30, check=True, real=real, target=target, fired=fired):
            result = real(command, timeout, check)
            if not fired and 'rm' not in command and os.path.lexists(target):
                fired.append(command)
                raise m.Stop('signal 15 after publication, before registration')
            return result
        module.bounded = interrupting
        try:
            module.preload(os.path.join(directory, f'r16-publish-{label}'), True, root=root,
                           driver_command=[sys.executable, fake_driver, etc, 'ok'], timeout_s=10)
        finally:
            module.bounded = real
        rec = json.load(open(os.path.join(directory, f'r16-publish-{label}', 'preload.json')))
        outcome[label] = {'fired': bool(fired), 'left': os.path.lexists(target), 'restore_failure': rec['restore_failure']}
        if os.path.lexists(target):
            os.remove(target)
        shutil.rmtree(root)
    record16('controller_interrupted_after_publication', outcome['new']['fired'] and not outcome['new']['left']
             and outcome['new']['restore_failure'] is None, outcome['old']['fired'] and not outcome['old']['left'], outcome=outcome)
    # R15-1 (outer trap), superseded in round 17: intent-only recovery was the R16-1
    # defect. With staged publication the identity is durable before publication, so a
    # controller SIGKILLed right after publication is recovered by identity (positive).
    w = world('r16-publish-killed', {'preload-host': {'publish': [['host', 'file', 'die']]}})
    x = go(w, transcript)
    record16('positive:trap_recovers_controller_killed_after_publication', x['rc'] == 137 and not os.path.lexists(w['host'])
             and 'OWNED file=' in x['log'] and 'TRANSCRIPT result=FAIL rc=137 gate=PASS' in x['log'],
             False, new={k: x[k] for k in ('rc', 'log')})
    w = world('r16-intent-foreign', {'preload-host': {'intent_files': [['host', 'foreign', False]]}})
    x = go(w, transcript)
    record16('positive:intent_without_our_signature_is_kept', x['rc'] == 8 and os.path.exists(w['host'])
             and 'ledger_path_allowed=FAIL intent' in x['log'], False, new={k: x[k] for k in ('rc', 'log')})
    # R15-2: every systemd helper hangs during cleanup, and a TERM arrives mid-cleanup.
    # Files are restored first, every helper is killed at its deadline, the gate fails,
    # and the cleanup still completes.
    runs = {}
    for label, script, limit in (('new', transcript, 180), ('old', old16_transcript, 45)):
        w = world(f'r16-hang-{label}', {'preload-host': {'units': [['kg1-1101-preload-host-1', False]], 'files': ['host'],
                                                         'hang': True}})
        started = time.monotonic()
        p = subprocess.Popen(['bash', script, w['rootfs']], env=w['env'], stdout=subprocess.PIPE,
                             stderr=subprocess.STDOUT, text=True, start_new_session=True)
        log_path = os.path.join(w['out'], 'transcript.log')
        termed = False
        while time.monotonic() - started < 30 and p.poll() is None:
            if os.path.exists(log_path) and 'STAGE reference exit=' in open(log_path).read():
                time.sleep(3)
                p.send_signal(signal.SIGTERM)
                termed = True
                break
            time.sleep(0.1)
        try:
            p.communicate(timeout=limit)
        except subprocess.TimeoutExpired:
            os.killpg(p.pid, signal.SIGKILL)
            p.communicate()
        log = open(log_path).read() if os.path.exists(log_path) else ''
        runs[label] = {'rc': p.returncode, 'termed': termed, 'elapsed': round(time.monotonic() - started, 1),
                       'host_left': os.path.lexists(w['host']), 'completed': 'TRANSCRIPT result=' in log, 'log': log[-1500:]}
        if os.path.lexists(w['host']):
            os.remove(w['host'])
    n, o = runs['new'], runs['old']
    record16('cleanup_bounded_files_first_signal_ignored', n['termed'] and n['completed'] and not n['host_left'] and n['rc'] == 8
             and 'unit_gone=FAIL' in n['log'] and 'no_kg1_units=FAIL query-failed' in n['log'],
             o['completed'] and not o['host_left'], runs=runs)
    # R15-3: a failed census, and a live owned process that cannot be inspected.
    for name, plan, needle in (('census_failure_is_not_absence', {'reference': {'orphan': True, 'census_fail': True}},
                                'no_owned_processes=FAIL census-failed'),
                               ('unreadable_live_process_is_not_absence', {'reference': {'orphan': True, 'unreadable_orphan': True}},
                                'UNREADABLE')):
        r = pair(name, plan)
        record16(name, r['new']['rc'] == 8 and needle in r['new']['log'], r['old']['rc'] != 0,
                 new={k: r['new'][k] for k in ('rc', 'log')}, old={k: r['old'][k] for k in ('rc', 'log')})
    # R15-4: the gate fails (a kg1 unit survives) but the evidence collects cleanly.
    new = orchestrate('r16-gate-failed-new', {'reference': {'unregistered_unit': True}})
    old = orchestrate('r16-gate-failed-old', {'reference': {'unregistered_unit': True}}, script_path=old16_orchestrator)
    record16('failed_gate_keeps_remote', new['rc'] == 8 and new['remote_kept'] and 'COLLECT verify=PASS' in new['stdout']
             and 'reason=resource-gate-FAIL' in new['stdout'], old['remote_kept'], new=new, old=old)

    # Round 21 REHEARSAL envelope: the transcript in `rehearsal` mode runs exactly
    # preflight and rehearsal (no preload stage, so no preload entry can be made), keeps
    # every gate, and the orchestrator passes the mode and still verifies before removal.
    w = world('r21-rehearsal-transcript', {'rehearsal': {'units': [['kg1-1101-rehearsal-1', False]]}})
    x = go(w, transcript, args=('rehearsal',))
    results['r21:rehearsal_transcript_envelope'] = {
        'rc': x['rc'], 'ran': x['ran'], 'log': x['log'][-1500:],
        'pass': x['rc'] == 0 and x['ran'] == ['preflight', 'rehearsal'] and 'mode=rehearsal' in x['log']
        and 'OWNED unit=kg1-1101-rehearsal-1 gone=yes' in x['log'] and 'TRANSCRIPT result=PASS rc=0 gate=PASS' in x['log']}
    w = world('r21-rehearsal-fails', {'rehearsal': {'exit': 7}})
    x = go(w, transcript, args=('rehearsal',))
    results['r21:rehearsal_failure_stops_and_fails'] = {
        'rc': x['rc'], 'ran': x['ran'], 'pass': x['rc'] == 7 and x['ran'] == ['preflight', 'rehearsal']
        and 'TRANSCRIPT result=FAIL rc=7 gate=PASS' in x['log']}
    ok = orchestrate('r21-rehearsal-orchestrated', {}, args=('rehearsal',))
    results['r21:rehearsal_orchestrated_verified_then_removed'] = {
        **ok, 'pass': ok['rc'] == 0 and not ok['remote_kept'] and 'RUN mode=rehearsal transcript_exit=0' in ok['stdout']
        and 'COLLECT verify=PASS' in ok['stdout'] and 'mode=rehearsal' in ok['evidence_log']}

    # ================= Round-16 review R16-1..R16-3, old = checkpoint d6c9c326 ==========
    old17_dir = os.path.join(directory, 'old17')
    os.makedirs(old17_dir, exist_ok=True)
    for name in ('graph-reference-run.py', 'kg1-reference-transcript.sh'):
        with open(os.path.join(old17_dir, name), 'w') as f:
            f.write(bounded(['git', '-C', os.path.dirname(HERE), 'show', f'{ROUND16_COMMIT}:scripts/{name}']).stdout)
    old17_r = load('old17_runner', os.path.join(old17_dir, 'graph-reference-run.py'))
    old17_transcript = os.path.join(old17_dir, 'kg1-reference-transcript.sh')

    def record17(name, new_caught, old_caught, **detail):
        results[f'r17:{name}'] = dict(detail, new_catches=bool(new_caught), old_catches=bool(old_caught),
                                      **{'pass': bool(new_caught) and not old_caught})

    def pair17(name, plan, timeout=(60, 60)):
        runs = {}
        for (label, script), limit in zip((('new', transcript), ('old', old17_transcript)), timeout):
            w = world(f'r17-{name}-{label}', plan)
            runs[label] = dict(go(w, script, timeout=limit), world=w)
        return runs

    # R16-1 (controller): a foreign empty preload appears after the intent and before the
    # exclusive creation, which therefore fails with EEXIST. The foreign inode must survive.
    outcome = {}
    for label, module in (('new', sys.modules[__name__]), ('old', old17_r)):
        root = tempfile.mkdtemp(prefix=f'kg1-r17-foreign-{label}-', dir=directory)
        etc = os.path.join(root, 'etc')
        os.makedirs(etc)
        target = os.path.join(etc, 'ld.so.preload')
        real = module.bounded
        planted = {}

        def foreign_first(command, timeout=30, check=True, real=real, target=target, planted=planted):
            if not planted and 'python3' in command:
                os.close(os.open(target, os.O_WRONLY | os.O_CREAT | os.O_EXCL, 0o644))
                planted['ino'] = os.lstat(target).st_ino
            return real(command, timeout, check)
        module.bounded = foreign_first
        try:
            code = module.preload(os.path.join(directory, f'r17-foreign-{label}'), True, root=root,
                                  driver_command=[sys.executable, fake_driver, etc, 'ok'], timeout_s=10)
        finally:
            module.bounded = real
        kept = os.path.lexists(target) and os.lstat(target).st_ino == planted.get('ino')
        leftovers = sorted(n for n in os.listdir(etc) if n.startswith('.kg1-1101-preload-stage-'))
        outcome[label] = {'exit': code, 'planted': bool(planted), 'foreign_kept': kept, 'staging_left': leftovers}
        shutil.rmtree(root)
    record17('controller_never_adopts_foreign_entry_after_eexist', outcome['new']['planted'] and outcome['new']['foreign_kept']
             and outcome['new']['exit'] != 0 and not outcome['new']['staging_left'],
             outcome['old']['planted'] and outcome['old']['foreign_kept'], outcome=outcome)
    # R16-1 (outer trap): same input to both, an intent-only ledger and an empty entry at
    # the preload path (indistinguishable from a foreign one after EEXIST).
    r = pair17('intent_only_empty_entry', {'preload-host': {'intent_files': [['host', '', False]]}})
    record17('trap_never_adopts_intent_only_entry', r['new']['rc'] == 8 and os.path.lexists(r['new']['world']['host']),
             os.path.lexists(r['old']['world']['host']),
             new={k: r['new'][k] for k in ('rc', 'log')}, old={k: r['old'][k] for k in ('rc', 'log')})
    w = world('r17-publish-foreign', {'preload-host': {'publish': [['host', 'file', 'foreign']]}})
    x = go(w, transcript)
    record17('positive:publish_eexist_keeps_foreign_removes_staging', x['rc'] == 1 and os.path.lexists(w['host'])
             and 'file_identity=FAIL' in x['log'] and 'OWNED stage=' in x['log']
             and not [n for n in os.listdir(w['hostetc']) if n.startswith('.kg1-1101-preload-stage-')], False,
             new={k: x[k] for k in ('rc', 'log')})
    # R16-2: the private /etc mode stat stalls. Host restoration must come first and every
    # helper before it must be bounded.
    r = pair17('private_mode_stat_stalls', {'preload-host': {'files': ['host'], 'stat_hang': True}}, timeout=(120, 45))
    record17('host_restored_before_stalled_helper', r['new']['rc'] == 8 and not os.path.lexists(r['new']['world']['host'])
             and 'private_etc_mode=FAIL' in r['new']['log'] and 'TRANSCRIPT result=' in r['new']['log'],
             not os.path.lexists(r['old']['world']['host']),
             new={k: r['new'][k] for k in ('rc', 'log', 'elapsed')}, old={k: r['old'][k] for k in ('rc', 'elapsed')})
    # R16-3: the owned orphan's /proc/<pid>/stat cannot be read while it is alive.
    r = pair17('unreadable_live_stat', {'reference': {'orphan': True, 'unreadable_stat_orphan': True}})
    record17('unreadable_live_stat_is_not_absence', r['new']['rc'] == 8 and 'UNREADABLE' in r['new']['log'],
             r['old']['rc'] != 0,
             new={k: r['new'][k] for k in ('rc', 'log')}, old={k: r['old'][k] for k in ('rc', 'log')})


# The jailed probe: it is the monitor's stand-in. The emulated scope is its session (the
# chain is started with start_new_session, so every launched process inherits it); the
# registered relation is that every member is the probe itself or its descendant.
TREE_PROBE = r'''import json, os, subprocess, sys
sys.path.insert(0, sys.argv[1])
import kg1_reference_monitor as m
child = subprocess.Popen(['/usr/bin/sleep', '30'])
me, sid = os.getpid(), os.getsid(0)
members = []
for entry in os.listdir('/proc'):
    if entry.isdigit():
        try:
            fields = open(f'/proc/{entry}/stat').read().rsplit(')', 1)[1].split()
        except OSError:
            continue
        if int(fields[3]) == sid and fields[0] != 'Z':
            members.append(int(entry))
outside = sorted(set(members) - {me} - set(m.descendants(me)))
child.kill(); child.wait()
print(json.dumps({'pid': me, 'ppid': os.getppid(), 'sid': sid, 'members': sorted(members), 'outside': outside,
                  'cwd': os.getcwd(),  # the execve environment, before Python's own locale coercion
                  'env': dict(e.split('=', 1) for e in open('/proc/self/environ').read().split('\0') if e)}))
'''

# Real launcher, privileged syscalls stubbed: this container's AppArmor policy denies
# mount namespaces to unprivileged user namespaces, so unshare/mount/chroot (and an
# unneeded setgroups) are recorded instead of performed. chdir, setgid/setuid, the
# identity verification and execve are real. The stubbed chroot takes away what a real
# one takes from the host interpreter (round 20): its standard library. sys.path and the
# importer cache are cleared and every module a bare `-I -S` launcher would not already
# hold (its own `ctypes, os, sys` closure) is dropped, so any post-chroot import fails
# exactly as on delectra. The stub records calls with builtins only.
LAUNCHER_STUB = r'''import sys
import ctypes, os
held = set(sys.modules)  # what the launcher process itself holds when it reaches chroot
import runpy
calls_path = sys.argv.pop(1)
sys.argv.pop(0)  # argv[0] is now the real launcher, exactly as when it runs directly
calls = []
def dump():
    fd = os.open(calls_path, os.O_WRONLY | os.O_CREAT | os.O_TRUNC, 0o644)
    os.write(fd, repr(calls).encode()); os.close(fd)
class Libc:
    def mount(self, source, target, fstype, flags, data):
        calls.append(['mount', source.decode() if source else None, target.decode(), flags]); return 0
ctypes.CDLL = lambda *a, **k: Libc()
os.unshare = lambda flags: calls.append(['unshare', flags])
def chroot(path):
    calls.append(['chroot', path])
    sys.path.clear(); sys.path_importer_cache.clear()
    for name in [n for n in sys.modules if n not in held and n != '__main__']:
        del sys.modules[name]
    dump()
os.chroot = chroot
os.setgroups = lambda groups: calls.append(['setgroups', groups])
for name in ('execve', 'execvpe'):
    def wrapped(file, args, env, real=getattr(os, name), name=name):
        calls.append([name, file]); dump(); return real(file, args, env)
    setattr(os, name, wrapped)
runpy.run_path(sys.argv[0], run_name='__main__')
'''

# The round-18 failure mode: uutils coreutils 0.2.2 `chroot` runs the command with
# process::Command::status() (spawn and wait) instead of exec'ing it.
CHAIN_SHIMS = {
    'unshare': '#!/bin/sh\nwhile [ "${1#-}" != "$1" ]; do [ "$1" = --propagation ] && shift; shift; done\nexec "$@"\n',
    'mount': '#!/bin/sh\nexit 0\n',
    'chroot': '#!/usr/bin/python3\nimport subprocess, sys\na = sys.argv[1:]\nwhile a[0].startswith("--"):\n    a = a[1:]\n'
              'sys.exit(subprocess.run(a[1:]).returncode)\n',
}


def round19_chain(directory, results):
    """R18: the jail launch chain leaves no process outside the monitored tree. The new
    chain runs the real launcher; the old chain (commit 970018e9) runs with a forking
    chroot shim modelled on uutils 0.2.2. Same probe, same emulated scope."""
    import importlib.util
    base = os.path.join(directory, 'r19')
    os.makedirs(base, exist_ok=True)
    old_path = os.path.join(base, 'old-graph-reference-run.py')
    with open(old_path, 'w') as f:
        f.write(bounded(['git', '-C', os.path.dirname(HERE), 'show', f'{ROUND17_COMMIT}:scripts/graph-reference-run.py']).stdout)
    spec = importlib.util.spec_from_file_location('old19_runner', old_path)
    old_r = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(old_r)
    shims = os.path.join(base, 'shims')
    os.makedirs(shims, exist_ok=True)
    for name, body in CHAIN_SHIMS.items():
        with open(os.path.join(shims, name), 'w') as f:
            f.write(body)
        os.chmod(os.path.join(shims, name), 0o755)
    probe = os.path.join(base, 'probe.py')
    stub = os.path.join(base, 'launcher-stub.py')
    for path, body in ((probe, TREE_PROBE), (stub, LAUNCHER_STUB)):
        with open(path, 'w') as f:
            f.write(body)
    root = os.path.join(base, 'rootfs')
    # The launcher resolves the command against ROOT before chroot: give the stand-in
    # rootfs the one executable path the (stubbed, so host-side) exec will use.
    os.makedirs(os.path.join(root, 'usr', 'bin'), exist_ok=True)
    with open(os.path.join(root, 'usr', 'bin', 'python3'), 'w') as f:
        f.write('#!/bin/false\n')
    os.chmod(os.path.join(root, 'usr', 'bin', 'python3'), 0o755)
    target = ['/usr/bin/python3', probe, HERE]
    env = dict(os.environ, PATH=shims + ':' + os.environ['PATH'])

    def launch(command):
        p = subprocess.Popen(command, env=env, stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True, start_new_session=True)
        try:
            out, err = p.communicate(timeout=60)
        except subprocess.TimeoutExpired:
            os.killpg(p.pid, signal.SIGKILL)
            out, err = p.communicate()
        line = out.strip().splitlines()[-1] if out.strip() else ''
        return p.pid, p.returncode, (json.loads(line) if line.startswith('{') else None), err[-600:]

    def related(pid, observed):
        return (observed is not None and observed['pid'] == pid and observed['sid'] == pid and observed['ppid'] == os.getpid()
                and observed['members'] and not observed['outside'])
    calls_path = os.path.join(base, 'calls.json')

    def stubbed(command, launcher=JAIL_EXEC):
        at = command.index(JAIL_EXEC)
        return command[:at] + [stub, calls_path, launcher] + command[at + 1:]

    def read_calls():
        import ast
        calls = ast.literal_eval(open(calls_path).read()) if os.path.exists(calls_path) else []
        if os.path.exists(calls_path):
            os.remove(calls_path)
        return calls
    new_cmd = stubbed(jail(*target, root=root))
    new_pid, new_rc, new_obs, new_err = launch(new_cmd)
    calls = read_calls()
    old_pid, old_rc, old_obs, old_err = launch(old_r.jail(*target))
    expected_calls = ([['unshare', os.CLONE_NEWNS], ['mount', None, '/', 0x4000 | (1 << 18)]]
                      + [c for d in ('proc', 'sys', 'dev') for c in (['mount', '/' + d, os.path.join(root, d), 0x1000 | 0x4000],
                                                                    ['mount', None, os.path.join(root, d), 0x4000 | (1 << 19)])]
                      + [['chroot', root], ['execve', '/usr/bin/python3']])
    new_ok = (new_rc == 0 and related(new_pid, new_obs) and calls == expected_calls and new_obs['cwd'] == '/'
              and new_obs['env'] == {'PATH': '/usr/bin:/bin', 'HOME': '/bundle', 'PYTHONDONTWRITEBYTECODE': '1'})
    results['r19:jail_chain_leaves_only_the_monitor_tree_in_scope'] = {
        'new': {'popen_pid': new_pid, 'rc': new_rc, 'observed': new_obs, 'calls': calls, 'stderr': new_err},
        'old': {'popen_pid': old_pid, 'rc': old_rc, 'observed': old_obs, 'stderr': old_err},
        'new_catches': new_ok, 'old_catches': related(old_pid, old_obs),
        'pass': new_ok and old_obs is not None and bool(old_obs['outside']) and not related(old_pid, old_obs)}
    # Unstubbed, the real launcher fails closed here (no mount namespace for this user):
    # nonzero before the command runs, and nothing is mounted.
    marker = os.path.join(base, 'ran')
    pid, rc, observed, err = launch(jail('/usr/bin/python3', '-c', f'open({marker!r}, "w")', root=root))
    results['r19:launcher_fails_closed_without_privilege'] = {
        'rc': rc, 'stderr': err, 'command_ran': os.path.exists(marker),
        'pass': rc != 0 and not os.path.exists(marker) and 'Operation not permitted' in err}
    # R20: nothing after chroot may need the host standard library. Both launchers get a
    # bare command name (`python3`, as the preload stage passes `sh`) under the
    # chroot-faithful stub. The round-19 launcher (059ab19c) must fail with round 20's
    # ModuleNotFoundError; the fixed one resolves before chroot and execs.
    old_launcher = os.path.join(base, 'old-kg1-jail-exec.py')
    with open(old_launcher, 'w') as f:
        f.write(bounded(['git', '-C', os.path.dirname(HERE), 'show', f'{ROUND19_COMMIT}:scripts/kg1-jail-exec.py']).stdout)
    bare = ['python3', probe, HERE]
    runs = {}
    for label, launcher in (('new', JAIL_EXEC), ('old', old_launcher)):
        pid, rc, observed, err = launch(stubbed(jail(*bare, root=root), launcher))
        runs[label] = {'popen_pid': pid, 'rc': rc, 'observed': observed, 'stderr': err, 'calls': read_calls()}
    n, o = runs['new'], runs['old']
    new_ok = (n['rc'] == 0 and related(n['popen_pid'], n['observed']) and n['calls'] and n['calls'][-1] == ['execve', '/usr/bin/python3']
              and ['chroot', root] in n['calls'])
    old_same_failure = o['rc'] != 0 and o['observed'] is None and "No module named 'warnings'" in o['stderr']
    results['r21:no_stdlib_after_chroot_bare_command'] = dict(runs, new_catches=new_ok, old_catches=not old_same_failure,
                                                              **{'pass': new_ok and old_same_failure})
    # Post-chroot failures are reported with builtins only and fail closed: a command whose
    # in-root path exists but cannot be exec'd after chroot (here: the stand-in `#!/bin/false`
    # is resolved, but the stub keeps the host root, so give it a path absent on the host).
    ghost = os.path.join(root, 'usr', 'bin', 'kg1-ghost')
    with open(ghost, 'w') as f:
        f.write('#!/bin/false\n')
    os.chmod(ghost, 0o755)
    marker2 = os.path.join(base, 'ghost-ran')
    pid, rc, observed, err = launch(stubbed(jail('kg1-ghost', marker2, root=root)))
    calls = read_calls()
    results['r21:post_chroot_exec_failure_fails_closed'] = {
        'rc': rc, 'stderr': err, 'calls': calls,
        'pass': rc == 126 and 'kg1-jail-exec: FileNotFoundError' in err and not os.path.exists(marker2)
        and calls[-1] == ['execve', '/usr/bin/kg1-ghost']}


# ------------------------------------------------------------------ local self-test

def selftest(directory):
    """Discriminating checks for every monitor rejection branch, invalid telemetry, gap,
    counter regression, child cleanup, the preload controller (including the EACCES
    variant, a hung driver and a failed setup) and topology drift. Local only."""
    import tempfile
    sys.path.insert(0, HERE)
    import kg1_reference_monitor as m
    os.makedirs(directory, exist_ok=True)
    results = {}

    def base_sample(t):
        psi = lambda: {'avg10': 0.0, 'avg60': 0.0, 'avg300': 0.0, 'total': 0}
        return {'t': t, 'wall': t, 'limits': dict(m.EXPECTED_LIMITS), 'procs': [100], 'descendants': [], 'descendants_outside': [],
                'ancestors': [{'path': '/sys/fs/cgroup/system.slice', 'memory.max': 'max', 'memory.swap.max': 'max',
                               'cpu.max': 'max 100000', 'cpuset.cpus.effective': '0-7'}], 'affinity': [0, 1, 2, 3],
                'events': {k: 0 for k in m.EVENT_KEYS + ('low',)}, 'events_local': {k: 0 for k in m.EVENT_KEYS + ('low',)},
                'swap_current': 0, 'swap_peak': 0, 'swap_events': {k: 0 for k in m.SWAP_EVENT_KEYS},
                'memory_current': 1, 'memory_peak': 1,
                'scope_mem_psi': {'some': psi(), 'full': psi()}, 'scope_cpu_psi': {'some': psi(), 'full': psi()},
                'cpu_stat': {k: 0 for k in m.CPU_STAT_KEYS}, 'host_mem_available': 3 * 1024 ** 3, 'host_swap_free': 1,
                'host_mem_psi': {'some': psi(), 'full': psi()}, 'host_io_psi': {'some': psi(), 'full': psi()},
                'host_cpu_psi': {'some': psi(), 'full': psi()},
                'host_stat': {c: [0] * 8 for c in ('cpu',) + m.ASSIGNED_CPUS}, 'host_pswpin': 0, 'host_pswpout': 0}

    def series(seconds, mutate=None, step=0.25):
        """Clean series: every CPU accrues user and idle time; `mutate(sample, t)` alters it."""
        out = []
        for n in range(int(seconds / step) + 1):
            t = n * step
            s = base_sample(t)
            for c in s['host_stat']:
                s['host_stat'][c] = [int(t * 100), 0, 0, int(t * 100), 0, 0, 0, 0]
            if mutate:
                mutate(s, t)
            out.append(s)
        return out

    def verdict(samples):
        p = m.Predicate(self_pid=100)
        for s in samples:
            r = p.observe(s)
            if r:
                return r[0]
        return None

    def psi_at(field, kind, key, value_at):
        def mutate(s, t):
            s[field][kind][key] = value_at(t)
        return mutate

    def steal(cpu, per_s=30.0, after=15):
        # Steady steal accrual (no step): per_s ticks/s against 200 user+idle ticks/s.
        def mutate(s, t):
            s['host_stat'][cpu][7] = int((t - after) * per_s) if t > after else 0
        return mutate

    def zero_cpu(s, t):
        s['host_stat']['cpu3'] = [0] * 8

    def leave(s, t):
        if t > 1:
            s['procs'] = [7]

    def widen(s, t):
        if t > 1:
            s['limits']['memory.max'] = 'max'
    cases = {
        'clean_60s': (series(60), None),
        'oom_event': (series(12, lambda s, t: s['events'].update(oom=int(t > 5))), 'MEMORY_STARVED'),
        'oom_kill_event': (series(12, lambda s, t: s['events'].update(oom_kill=int(t > 5))), 'MEMORY_STARVED'),
        'oom_group_kill_event': (series(12, lambda s, t: s['events'].update(oom_group_kill=int(t > 5))), 'MEMORY_STARVED'),
        'max_event': (series(12, lambda s, t: s['events'].update(max=int(t > 5))), 'MEMORY_STARVED'),
        'high_event_local_only': (series(12, lambda s, t: s['events_local'].update(high=int(t > 5))), 'MEMORY_STARVED'),
        'swap_current': (series(12, lambda s, t: s.update(swap_current=4096 if t > 5 else 0)), 'MEMORY_STARVED'),
        'swap_peak': (series(12, lambda s, t: s.update(swap_peak=4096 if t > 5 else 0)), 'MEMORY_STARVED'),
        'swap_event_fail': (series(12, lambda s, t: s['swap_events'].update(fail=int(t > 5))), 'MEMORY_STARVED'),
        'scope_some_avg10': (series(12, psi_at('scope_mem_psi', 'some', 'avg10', lambda t: 1.5 if t > 5 else 0)), 'MEMORY_STARVED'),
        'scope_some_avg300': (series(12, psi_at('scope_mem_psi', 'some', 'avg300', lambda t: 1.01 if t > 5 else 0)), 'MEMORY_STARVED'),
        'scope_full_avg60': (series(12, psi_at('scope_mem_psi', 'full', 'avg60', lambda t: 0.11 if t > 5 else 0)), 'MEMORY_STARVED'),
        'scope_some_total_abs': (series(8, psi_at('scope_mem_psi', 'some', 'total', lambda t: 10_000_001 if t > 5 else 0)), 'MEMORY_STARVED'),
        'scope_full_total_abs': (series(8, psi_at('scope_mem_psi', 'full', 'total', lambda t: 1_000_001 if t > 5 else 0)), 'MEMORY_STARVED'),
        # 1.05% some for 20 s: under the 10 s absolute ceiling, over the 1.00% cumulative rate
        'scope_some_cumulative_rate': (series(20, psi_at('scope_mem_psi', 'some', 'total', lambda t: int(t * 10_500))), 'MEMORY_STARVED'),
        # 310 s quiet, then 12 ms of full stall: cumulative 0.004%, 10 s window 0.12%
        'scope_full_acute_window': (series(320, psi_at('scope_mem_psi', 'full', 'total', lambda t: 12_000 if t >= 310 else 0)), 'MEMORY_STARVED'),
        'host_mem_available_floor': (series(12, lambda s, t: s.update(host_mem_available=1024 ** 3 - 1 if t > 5 else 3 * 1024 ** 3)), 'MEMORY_STARVED'),
        'host_mem_full_avg10': (series(12, psi_at('host_mem_psi', 'full', 'avg10', lambda t: 0.2 if t > 5 else 0)), 'HOST_CONTENDED'),
        'host_mem_full_window_rate': (series(320, psi_at('host_mem_psi', 'full', 'total', lambda t: 12_000 if t >= 310 else 0)), 'HOST_CONTENDED'),
        'host_io_full_avg10': (series(12, psi_at('host_io_psi', 'full', 'avg10', lambda t: 5.5 if t > 5 else 0)), 'HOST_CONTENDED'),
        'host_io_full_window_rate': (series(30, psi_at('host_io_psi', 'full', 'total', lambda t: 600_000 if t >= 20 else 0)), 'HOST_CONTENDED'),
        'scope_cpu_some_avg10': (series(12, psi_at('scope_cpu_psi', 'some', 'avg10', lambda t: 5.5 if t > 5 else 0)), 'HOST_CONTENDED'),
        'scope_cpu_some_window_rate': (series(30, psi_at('scope_cpu_psi', 'some', 'total', lambda t: 600_000 if t >= 20 else 0)), 'HOST_CONTENDED'),
        'steal_cpu2': (series(30, steal('cpu2')), 'HOST_CONTENDED'),
        'steal_aggregate': (series(30, steal('cpu')), 'HOST_CONTENDED'),
        'steal_cpu1_just_above_1pct': (series(30, steal('cpu1', per_s=2.1, after=0)), 'HOST_CONTENDED'),
        'steal_cpu1_below_1pct_control': (series(30, steal('cpu1', per_s=1.0, after=0)), None),
        'scope_full_avg_at_limit_control': (series(12, psi_at('scope_mem_psi', 'full', 'avg10', lambda t: 0.10)), None),
        'scope_some_avg_at_limit_control': (series(12, psi_at('scope_mem_psi', 'some', 'avg60', lambda t: 1.00)), None),
        'host_mem_available_at_floor_control': (series(12, lambda s, t: s.update(host_mem_available=1024 ** 3)), None),
        # exactly the 1 s absolute ceiling within the first 10 s (rate rules not yet active): not a breach
        'scope_full_total_at_ceiling_control': (series(8, psi_at('scope_mem_psi', 'full', 'total', lambda t: 1_000_000 if t > 5 else 0)), None),
        'host_swap_alone_is_context': (series(30, lambda s, t: s.update(host_pswpout=int(t * 1000))), None),
        'sample_gap': ([base_sample(0.0), base_sample(1.5)], 'MONITOR_INVALID'),
        'counter_decrease': (series(3, psi_at('host_io_psi', 'some', 'total', lambda t: 100 if t < 2 else 50)), 'MONITOR_INVALID'),
        'limits_changed': (series(3, widen), 'MONITOR_INVALID'),
        'monitor_left_scope': (series(3, leave), 'MONITOR_INVALID'),
        'fresh_scope_not_fresh': (series(3, lambda s, t: s['events'].update(high=3)), 'MONITOR_INVALID'),
        'zero_cpu_denominator': (series(12, zero_cpu), 'MONITOR_INVALID'),
    }
    for name, (samples, expected) in cases.items():
        got = verdict(samples)
        results[f'predicate:{name}'] = {'expected': expected, 'got': got, 'pass': got == expected}
    for name, text_value, parser in (('psi_missing_full', 'some avg10=0.00 avg60=0.00 avg300=0.00 total=0\n', lambda x: m.parse_psi(x, 'psi')),
                                     ('psi_garbage', 'some avg10=x avg60=0 avg300=0 total=0\nfull avg10=0 avg60=0 avg300=0 total=0\n', lambda x: m.parse_psi(x, 'psi')),
                                     ('events_missing_oom', 'low 0\nhigh 0\nmax 0\n', lambda x: m.parse_keyed(x, 'events', m.EVENT_KEYS)),
                                     ('stat_missing_cpu3', 'cpu 1 2 3 4 5 6 7 8 0 0\ncpu0 1 2 3 4 5 6 7 8 0 0\n', m.parse_stat)):
        try:
            parser(text_value)
            ok = False
        except m.Invalid:
            ok = True
        results[f'parse:{name}'] = {'expected': 'Invalid', 'pass': ok}
    try:
        m._read(os.path.join(directory, 'no-such-telemetry'))
        unreadable = False
    except m.Invalid:
        unreadable = True
    results['parse:unreadable_file_is_invalid_not_zero'] = {'pass': unreadable}
    # Child cleanup: a proof tree including a setsid grandchild; breach after 3 s.
    clock = {'n': 0, 'breach_after': 12}

    def fake_collect():
        clock['n'] += 1
        s = base_sample(time.monotonic())
        s['procs'] = [os.getpid()]
        for c in s['host_stat']:
            s['host_stat'][c] = [clock['n'] * 100, 0, 0, clock['n'] * 100, 0, 0, 0, 0]
        if clock['breach_after'] and clock['n'] > clock['breach_after']:
            s['events']['oom_kill'] = 1
        return s
    pids_file = os.path.join(directory, 'tree.pids')
    tree = ['sh', '-c', f'sleep 300 & echo $! >> {pids_file}; setsid sleep 300 & echo $! >> {pids_file}; '
            f'sh -c "sleep 300 & echo \\$! >> {pids_file}; wait" & wait']
    r = m.run_monitored(tree, os.path.join(directory, 'cleanup.jsonl'), fake_collect, baseline_s=0.5, self_pid=os.getpid())
    spawned = [int(x) for x in open(pids_file).read().split()]

    def live(pid):
        try:
            return open(f'/proc/{pid}/stat').read().rsplit(')', 1)[1].split()[0] != 'Z'
        except OSError:
            return False
    alive = [p for p in spawned if live(p)]
    results['cleanup:breach_kills_tree'] = {'verdict': r['verdict'], 'latched': r['latched'], 'spawned': spawned,
                                            'alive_after': alive, 'cleanup': r['cleanup'],
                                            'pass': r['verdict'] == 'MEMORY_STARVED' and len(spawned) == 3 and not alive
                                            and r['cleanup']['verified']}
    calls = {'n': 0}

    def failing_collect():
        calls['n'] += 1
        if calls['n'] > 6:
            raise m.Invalid('memory.pressure: empty')
        return fake_collect()
    clock.update(n=0, breach_after=0)
    r = m.run_monitored(['sleep', '300'], os.path.join(directory, 'invalid.jsonl'), failing_collect, baseline_s=0.5,
                        self_pid=os.getpid())
    results['cleanup:telemetry_loss_stops'] = {'verdict': r['verdict'], 'latched': r['latched'], 'cleanup': r['cleanup'],
                                               'pass': r['verdict'] == 'MONITOR_INVALID' and r['cleanup']['verified']}
    clock.update(n=0, breach_after=0)
    r = m.run_monitored(['sh', '-c', 'exit 0'], os.path.join(directory, 'pass.jsonl'), fake_collect, baseline_s=0.5, self_pid=os.getpid())
    results['cleanup:clean_run_passes'] = {'verdict': r['verdict'], 'pass': r['verdict'] == 'PASS'}
    clock.update(n=0, breach_after=0)
    r = m.run_monitored(['sh', '-c', 'exit 3'], os.path.join(directory, 'fail.jsonl'), fake_collect, baseline_s=0.5, self_pid=os.getpid())
    results['cleanup:proof_failure_is_not_pass'] = {'verdict': r['verdict'], 'pass': r['verdict'] == 'PROOF_FAILED'}
    # Preload controller against a fake driver in a private temp root.
    fake_driver = os.path.join(directory, 'fake_driver.py')
    with open(fake_driver, 'w') as f:
        f.write('''import json, os, stat, sys, time
etc, mode, variants = sys.argv[1], sys.argv[2], sys.argv[3:]
path = os.path.join(etc, 'ld.so.preload')
def state():
    try:
        st = os.lstat(path)
    except FileNotFoundError:
        return 'absent'
    except OSError as e:
        return 'errno:%d' % e.errno
    return 'symlink' if stat.S_ISLNK(st.st_mode) else 'file:%d' % st.st_size
expected = {'empty-file': 'file:0', 'dangling-symlink': 'symlink', 'unsearchable-etc': 'errno:13'}
out = []
for v in variants:
    print(json.dumps({'await': 'present', 'variant': v}), flush=True)
    if sys.stdin.readline().strip() != 'ready': sys.exit(3)
    observed = state()
    if mode == 'hang': time.sleep(600)
    print(json.dumps({'await': 'absent', 'variant': v}), flush=True)
    if sys.stdin.readline().strip() != 'ready': sys.exit(3)
    if observed != expected[v] or state() != 'absent':
        print('bad', v, observed, file=sys.stderr); sys.exit(4)
    out.append({'variant': v, 'observed': observed})
print(json.dumps({'mode': 'preload', 'outcomes': out}), flush=True)
print(json.dumps({'complete': True}), flush=True)
''')
    for label, mode, expect_pass, timeout_s, break_setup in (('all_variants_pass', 'ok', True, 20, False),
                                                             ('hung_driver_times_out_and_restores', 'hang', False, 2, False),
                                                             ('setup_failure_aborts_and_restores', 'ok', False, 20, True)):
        root = tempfile.mkdtemp(prefix='kg1-preload-selftest-', dir=directory)
        etc = os.path.join(root, 'etc')
        os.makedirs(etc)
        os.chmod(etc, 0o555 if break_setup else 0o755)
        out_dir = os.path.join(directory, f'preload-{label}')
        code = preload(out_dir, True, root=root, driver_command=[sys.executable, fake_driver, etc, mode], timeout_s=timeout_s)
        record = json.load(open(os.path.join(out_dir, 'preload.json')))
        mode_after = oct(os.stat(etc).st_mode & 0o7777)
        expected_mode = oct(0o555 if break_setup else 0o755)
        unsearchable_seen = any(r.get('variant') == 'unsearchable-etc' and r.get('etc_mode') == oct(0o755 & ~0o111)
                                for r in record['records'] if r.get('runner') == 'created')
        observed = [o.get('observed') for r in record['records'] if r.get('mode') == 'preload' for o in r['outcomes']]
        results[f'preload:{label}'] = {'exit': code, 'failure': record['failure'], 'mode_after': mode_after,
                                       'absent_after': not os.path.lexists(os.path.join(etc, 'ld.so.preload')),
                                       'eacces_variant_exercised': unsearchable_seen, 'observed': observed,
                                       'pass': (code == 0) == expect_pass and mode_after == expected_mode
                                       and not os.path.lexists(os.path.join(etc, 'ld.so.preload'))
                                       and (not expect_pass or (unsearchable_seen and observed == ['file:0', 'symlink', 'errno:13']))}
        os.chmod(etc, 0o755)
        shutil.rmtree(root)
    # Topology drift detection.
    root = tempfile.mkdtemp(prefix='kg1-topology-selftest-', dir=directory)
    os.makedirs(os.path.join(root, 'usr/lib'))
    open(os.path.join(root, 'usr/lib/ld.so'), 'w').close()
    os.chmod(os.path.join(root, 'usr/lib/ld.so'), 0o755)
    os.symlink('usr/lib', os.path.join(root, 'lib'))
    before = topology(root)
    os.remove(os.path.join(root, 'lib'))
    os.symlink('/usr/lib', os.path.join(root, 'lib'))
    os.chmod(os.path.join(root, 'usr/lib/ld.so'), 0o777)
    after = topology(root)
    changed = sorted(k for k in before if before[k] != after.get(k))
    results['topology:link_target_and_mode_drift_detected'] = {'changed': changed, 'pass': changed == ['lib', 'usr/lib/ld.so']}
    shutil.rmtree(root)
    round14_gaps(directory, m, base_sample, series, results, fake_driver)
    round15_gaps(directory, m, base_sample, results)
    round19_chain(directory, results)
    failed = [k for k, v in results.items() if not v['pass']]
    write(os.path.join(directory, 'selftest.json'), {'predicate_version': m.PREDICATE_VERSION,
                                                     'monitor_sha256': sha256(os.path.join(HERE, 'kg1_reference_monitor.py')),
                                                     'runner_sha256': sha256(os.path.abspath(__file__)), 'results': results,
                                                     'failed': failed})
    print(json.dumps({'selftest': 'PASS' if not failed else 'FAIL', 'cases': len(results), 'failed': failed}))
    return 0 if not failed else 1



if __name__ == '__main__':
    commands = {'bundle': bundle, 'preflight': preflight, 'reference': reference, 'inner': inner,
                'rehearsal': lambda d: reference(d, rehearsal=True), 'rehearse-inner': lambda d: inner(d, rehearsal=True),
                'preload-chroot': lambda d: preload(d, True), 'preload-host': lambda d: preload(d, False),
                'selftest': selftest}
    if len(sys.argv) != 3 or sys.argv[1] not in commands:
        sys.exit(__doc__)
    sys.exit(commands[sys.argv[1]](os.path.abspath(sys.argv[2])))
