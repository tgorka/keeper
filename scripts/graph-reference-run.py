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
                'scripts/graph-reference-run.py', 'scripts/kg1_reference_monitor.py', 'src-tauri/Cargo.lock',
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
    tools = {t: shutil.which(t) for t in ('sudo', 'systemd-run', 'unshare', 'chroot', 'mount', 'dpkg-query')}
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


def jail(*command):
    """Root: private mount namespace, the pinned rootfs's /proc /sys /dev bound from the
    host, then chroot as the invoking user. The namespace (and every mount) ends with
    the command; nothing is mounted on the host."""
    script = ('set -e; for d in proc sys dev; do mount --rbind /$d "$1/$d"; mount --make-rslave "$1/$d"; done; '
              'root="$1"; shift; exec chroot --userspec="$KG1_UID:$KG1_GID" "$root" "$@"')
    return ['unshare', '-m', '--propagation', 'private', 'env', f'KG1_UID={os.getuid()}', f'KG1_GID={os.getgid()}',
            'sh', '-c', script, 'sh', ROOT, '/usr/bin/env', '-i', PATH, 'HOME=/bundle'] + list(command)


def bounded(command, timeout=30, check=True):
    """Every controller helper is time-bounded; a hang or failure is an error, not a pass."""
    try:
        result = subprocess.run(command, capture_output=True, text=True, timeout=timeout)
    except subprocess.TimeoutExpired as error:
        raise RuntimeError(f'{command} exceeded {timeout}s') from error
    if check and result.returncode != 0:
        raise RuntimeError(f'{command} failed ({result.returncode}): {result.stderr.strip()[-300:]}')
    return result


def scope_gone(unit, timeout=30):
    """Kill a transient scope's whole cgroup and wait until systemd reports it gone."""
    kill = bounded(['sudo', '-n', 'systemctl', 'kill', '--signal=SIGKILL', f'{unit}.scope'], check=False)
    end = time.monotonic() + timeout
    state = None
    while time.monotonic() < end:
        state = bounded(['systemctl', 'show', '-p', 'ActiveState', '-p', 'ControlGroup', '--value', f'{unit}.scope'], check=False).stdout.split()
        if not state or state[0] in ('inactive', 'failed') and len(state) == 1:
            return {'kill_exit': kill.returncode, 'state': state, 'gone': True}
        time.sleep(0.5)
    return {'kill_exit': kill.returncode, 'state': state, 'gone': False}


def reference(directory):
    sys.path.insert(0, HERE)
    import kg1_reference_monitor as monitor
    os.makedirs(directory, exist_ok=True)
    if preflight(directory) != 0:
        return 3
    unit = f'kg1-1101-reference-{os.getpid()}'
    inside = chrooted(directory)
    heartbeat = os.path.join(directory, 'heartbeat.json')
    command = ['sudo', '-n', 'systemd-run', '--scope', '--quiet', f'--unit={unit}',
               '-p', 'AllowedCPUs=0-3', '-p', 'CPUQuota=400%', '-p', 'MemoryMax=8G', '-p', 'MemorySwapMax=0', '--'] + \
        jail('/usr/bin/python3', chrooted(os.path.abspath(__file__)), 'inner', inside)
    started = time.time()
    process = subprocess.Popen(command, start_new_session=True)
    # Independent of the monitor inside the scope: a stalled or dead monitor (heartbeat
    # not advancing for 15 s, longer than its 10 s bounded cleanup) or a signal to this
    # process kills the whole scope cgroup.
    events = monitor.watch(process, heartbeat, lambda: scope_gone(unit), stall_s=15.0)
    after = scope_gone(unit)
    outer = {'command': command, 'unit': unit, 'exit': process.returncode, 'watchdog_events': events,
             'scope_after': after, 'elapsed_s': round(time.time() - started, 1)}
    write(os.path.join(directory, 'outer.json'), outer)
    if events or not after['gone']:
        print(json.dumps({'reference': 'MONITOR_INVALID', 'watchdog_events': events, 'scope_after': after}))
        return 7
    return process.returncode


# --------------------------------------------------------- inside the scope and chroot

def scope_topology(cgroup):
    """The verified dedicated scope and every ancestor's restriction (none tighter, none
    empty or malformed)."""
    sys.path.insert(0, HERE)
    import kg1_reference_monitor as monitor
    problems = []
    if not os.path.basename(cgroup).startswith('kg1-1101-reference-') or not cgroup.endswith('.scope'):
        problems.append(f'not the dedicated kg1 reference scope: {cgroup}')
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


def inner(directory):
    import resource
    os.makedirs(directory, exist_ok=True)
    sys.path.insert(0, HERE)
    import kg1_reference_monitor as monitor
    cgroup = '/sys/fs/cgroup' + text('/proc/self/cgroup').split('::', 1)[-1]
    effective = {name: text(os.path.join(cgroup, name)) for name in LIMITS}
    affinity = sorted(os.sched_getaffinity(0))
    ancestors, problems = scope_topology(cgroup)
    soft, hard = resource.getrlimit(resource.RLIMIT_STACK)
    if effective != LIMITS or affinity != [0, 1, 2, 3]:
        problems.append('effective scope limits or affinity not verified')
    if hard != resource.RLIM_INFINITY:
        problems.append(f'inherited hard RLIMIT_STACK is {hard}, not unlimited')
    diagnostics = subprocess.run(['/lib64/ld-linux-x86-64.so.2', '--list-diagnostics'], env={}, capture_output=True, text=True)
    with open(os.path.join(directory, 'ld-so-diagnostics.txt'), 'w') as f:
        f.write(diagnostics.stdout)
    record = {'predicate_version': monitor.PREDICATE_VERSION, 'monitor_sha256': sha256(os.path.join(HERE, 'kg1_reference_monitor.py')),
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
    with open(os.path.join(directory, 'proof.json'), 'w') as stdout, open(os.path.join(directory, 'proof.err'), 'w') as stderr:
        result = monitor.run_monitored(
            [sys.executable, os.path.join(BUNDLE, 'scripts', 'graph-proof.py'), os.path.join(BUNDLE, 'target', 'graph-worker'),
             scratch, '--benchmark'], os.path.join(directory, 'samples.jsonl'), lambda: monitor.collect(cgroup),
            env=env, stdout=stdout, stderr=stderr, preexec_fn=soft_stack, self_pid=os.getpid(),
            members=lambda: monitor.scope_members(cgroup), heartbeat_path=os.path.join(directory, 'heartbeat.json'))
    shutil.rmtree(scratch, ignore_errors=True)
    record['monitor'] = result
    write(os.path.join(directory, 'reference.json'), record)
    print(json.dumps({'reference': result['verdict'], 'latched': result['latched'], 'proof_exit': result['proof_exit']}))
    return {'PASS': 0, 'PROOF_FAILED': 1, 'MEMORY_STARVED': 5, 'HOST_CONTENDED': 6, 'MONITOR_INVALID': 7}[result['verdict']]


# --------------------------------------------------------------------- P6c preload

STACK_GUARD = 'test "$(ulimit -H -s)" = unlimited || exit 97; ulimit -S -s 8192 && exec "$@"'


def preload(directory, in_chroot, root=None, driver_command=None, timeout_s=120):
    """P6c preload refusal controller. `root`/`driver_command` are overridable only for
    the local self-test (the driver then runs as a tracked child of this subreaper); the
    real cases run the bundled driver inside its own transient systemd scope, so the whole
    tree, setsid descendants included, is killed and verified gone through its cgroup."""
    import select
    sys.path.insert(0, HERE)
    import kg1_reference_monitor as monitor
    root = root or ROOT
    os.makedirs(directory, exist_ok=True)
    etc = os.path.join(root, 'etc') if in_chroot else '/etc'
    path = os.path.join(etc, 'ld.so.preload')
    if os.path.lexists(path):
        print(json.dumps({'preload': 'refused', 'reason': f'{path} exists before the case'}))
        return 3
    # The rootfs /etc belongs to the invoking user; the host /etc needs sudo.
    privileged = [] if in_chroot else ['sudo', '-n']
    saved_mode = os.stat(etc).st_mode & 0o7777

    def absent_and_restored():
        return not os.path.lexists(path) and os.stat(etc).st_mode & 0o7777 == saved_mode

    def restore():
        if os.stat(etc).st_mode & 0o7777 != saved_mode:
            os.chmod(etc, saved_mode)
        if os.path.lexists(path):
            bounded(privileged + ['rm', '-f', path])
        if not absent_and_restored():
            raise RuntimeError('restoration not verified')
    variants = {'empty-file': lambda: bounded(privileged + ['install', '-m', '0644', '/dev/null', path]),
                'dangling-symlink': lambda: bounded(privileged + ['ln', '-s', '/nonexistent/kg1-1101-preload-target', path])}
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
    process = subprocess.Popen(command, stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=stderr_file,
                               start_new_session=True)
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
            state = scope_gone(unit)
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
    s = {'t': time.monotonic(), 'wall': 0, 'limits': dict(m.EXPECTED_LIMITS), 'procs': [os.getpid()], 'descendants': [],
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
        return {'t': t, 'wall': t, 'limits': dict(m.EXPECTED_LIMITS), 'procs': [100], 'descendants': [],
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
    failed = [k for k, v in results.items() if not v['pass']]
    write(os.path.join(directory, 'selftest.json'), {'predicate_version': m.PREDICATE_VERSION,
                                                     'monitor_sha256': sha256(os.path.join(HERE, 'kg1_reference_monitor.py')),
                                                     'runner_sha256': sha256(os.path.abspath(__file__)), 'results': results,
                                                     'failed': failed})
    print(json.dumps({'selftest': 'PASS' if not failed else 'FAIL', 'cases': len(results), 'failed': failed}))
    return 0 if not failed else 1



if __name__ == '__main__':
    commands = {'bundle': bundle, 'preflight': preflight, 'reference': reference, 'inner': inner,
                'preload-chroot': lambda d: preload(d, True), 'preload-host': lambda d: preload(d, False),
                'selftest': selftest}
    if len(sys.argv) != 3 or sys.argv[1] not in commands:
        sys.exit(__doc__)
    sys.exit(commands[sys.argv[1]](os.path.abspath(sys.argv[2])))
