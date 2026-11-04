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
        return subprocess.run(command, capture_output=True, text=True).stdout.strip()
    except OSError:
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
                'scripts/graph-reference-run.py', 'src-tauri/Cargo.lock',
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
                if not os.path.islink(path) and not path.endswith('/bundle/SHA256SUMS'):
                    f.write(f'{sha256(path)}  {os.path.relpath(path, rootfs)}\n')
    print(json.dumps({'bundle': rootfs, 'dsos': len(packages), 'pinned_glibc': manifest['pinned_glibc']}))
    return 0


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


def reference(directory):
    os.makedirs(directory, exist_ok=True)
    if preflight(directory) != 0:
        return 3
    unit = f'kg1-1101-reference-{os.getpid()}'
    inside = chrooted(directory)
    command = ['sudo', '-n', 'systemd-run', '--scope', '--quiet', f'--unit={unit}',
               '-p', 'AllowedCPUs=0-3', '-p', 'CPUQuota=400%', '-p', 'MemoryMax=8G', '-p', 'MemorySwapMax=0', '--'] + \
        jail('/usr/bin/python3', chrooted(os.path.abspath(__file__)), 'inner', inside)
    started = time.time()
    process = subprocess.run(command)
    write(os.path.join(directory, 'outer.json'), {'command': command, 'unit': unit, 'exit': process.returncode,
                                                  'elapsed_s': round(time.time() - started, 1)})
    return process.returncode


# --------------------------------------------------------- inside the scope and chroot

def inner(directory):
    cgroup = '/sys/fs/cgroup' + text('/proc/self/cgroup').split('::', 1)[-1]
    effective = {name: text(os.path.join(cgroup, name)) for name in LIMITS}
    affinity = sorted(os.sched_getaffinity(0))
    verified = effective == LIMITS and affinity == [0, 1, 2, 3]
    limits = {'cgroup': cgroup, 'expected': LIMITS, 'effective': effective, 'affinity': affinity, 'verified': verified,
              'documented_deviation': 'MemoryMax=8G exceeds this host\'s physical RAM (recorded below); '
                                      'this is not a clean 4-core/8-GiB result',
              'pinned_libc_in_chroot': platform.libc_ver(), 'host_before': host_snapshot(directory)}
    if not verified:
        write(os.path.join(directory, 'limits.json'), limits)
        print(json.dumps({'reference': 'refused', 'reason': 'effective limits not verified', 'effective': effective,
                          'affinity': affinity}))
        return 4
    scratch = os.path.join(directory, 'scratch')
    env = {'PATH': '/usr/bin:/bin', 'HOME': '/bundle', 'KG1_WORKER': os.path.join(BUNDLE, 'feature-off', 'keeper-graph-query'),
           'KG1_UNWIND_CONTROL_WORKER': os.path.join(BUNDLE, 'release', 'keeper-graph-query'),
           'KG1_NATIVE_PREBUILT': os.path.join(BUNDLE, 'native')}
    started = time.time()
    with open(os.path.join(directory, 'proof.json'), 'w') as stdout, open(os.path.join(directory, 'proof.err'), 'w') as stderr:
        # Soft limit only: P6c's case raises the soft limit above 8 MiB, which a hard
        # limit of 8 MiB (plain `ulimit -s`) would reject with EINVAL.
        process = subprocess.run(['sh', '-c', 'ulimit -S -s 8192 && exec "$@"', 'sh', sys.executable,
                                  os.path.join(BUNDLE, 'scripts', 'graph-proof.py'), os.path.join(BUNDLE, 'target', 'graph-worker'),
                                  scratch, '--benchmark'], env=env, stdout=stdout, stderr=stderr)
    elapsed = round(time.time() - started, 1)
    after = host_snapshot(directory)
    names = ('memory.peak', 'memory.events', 'memory.events.local', 'memory.pressure', 'cpu.pressure', 'io.pressure',
             'memory.swap.current', 'memory.swap.peak', 'memory.swap.events', 'memory.stat', 'cpu.stat')
    cgroup_after = {name: text(os.path.join(cgroup, name)) for name in names}
    events = dict(line.split() for line in cgroup_after['memory.events'].splitlines() if len(line.split()) == 2)
    full = next((line for line in cgroup_after['memory.pressure'].splitlines() if line.startswith('full')), '')
    full_total = int(dict(p.split('=') for p in full.split()[1:]).get('total', '0')) if full else None
    swap_peak = cgroup_after['memory.swap.peak']
    host_swapout = int(after['pswpout'] or 0) - int(limits['host_before']['pswpout'] or 0)
    starved = {'oom_events': {k: int(events.get(k, 0)) for k in ('oom', 'oom_kill', 'oom_group_kill')},
               'cgroup_swap_peak': swap_peak, 'host_pswpout_delta': host_swapout,
               'memory_full_stall_total_us': full_total, 'full_stall_limit_us': FULL_STALL_LIMIT_US}
    is_starved = (any(starved['oom_events'].values()) or (swap_peak.isdigit() and int(swap_peak) > 0)
                  or host_swapout > 0 or full_total is None or full_total > FULL_STALL_LIMIT_US)
    shutil.rmtree(scratch, ignore_errors=True)
    limits.update({'host_after': after, 'cgroup_after': cgroup_after, 'proof_exit': process.returncode,
                   'proof_elapsed_s': elapsed, 'starvation': starved, 'memory_starved': is_starved})
    write(os.path.join(directory, 'limits.json'), limits)
    verdict = 'MEMORY_STARVED' if is_starved else ('PASS' if process.returncode == 0 else 'PROOF_FAILED')
    print(json.dumps({'reference': verdict, 'proof_exit': process.returncode, 'memory_peak': cgroup_after['memory.peak'],
                      'starvation': starved}))
    return 5 if is_starved else process.returncode


# --------------------------------------------------------------------- P6c preload

def preload(directory, in_chroot):
    os.makedirs(directory, exist_ok=True)
    path = os.path.join(ROOT, 'etc', 'ld.so.preload') if in_chroot else '/etc/ld.so.preload'
    if os.path.lexists(path):
        print(json.dumps({'preload': 'refused', 'reason': f'{path} exists before the case'}))
        return 3
    # The rootfs /etc belongs to the invoking user; the host /etc needs sudo.
    privileged = [] if in_chroot else ['sudo', '-n']
    variants = {'empty-file': privileged + ['install', '-m', '0644', '/dev/null', path],
                'dangling-symlink': privileged + ['ln', '-s', '/nonexistent/kg1-1101-preload-target', path]}
    created = {'value': False}

    def remove(*_):
        if created['value'] or os.path.lexists(path):
            subprocess.run(privileged + ['rm', '-f', path])
            created['value'] = False
    for sig in (signal.SIGINT, signal.SIGTERM, signal.SIGHUP):
        signal.signal(sig, lambda *a: (remove(), sys.exit(130)))
    driver_args = ['preload', os.path.join(BUNDLE, 'feature-off', 'keeper-graph-query'), os.path.join(directory, 'scratch')] + list(variants)
    driver = os.path.join(BUNDLE, 'target', 'graph-worker', 'keeper-graph-driver')
    if in_chroot:
        command = ['sudo', '-n'] + jail('sh', '-c', 'ulimit -S -s 8192 && exec "$@"', 'sh', chrooted(driver)) + \
            [chrooted(a) if a.startswith(BUNDLE) else a for a in driver_args]
    else:
        command = ['sh', '-c', 'ulimit -S -s 8192 && exec "$@"', 'sh', driver] + driver_args
    log = []
    process = subprocess.Popen(command, stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True)
    try:
        for line in process.stdout:
            if not line.startswith('{'):
                continue
            record = json.loads(line)
            log.append(record)
            if 'await' not in record:
                continue
            if record['await'] == 'present':
                result = subprocess.run(variants[record['variant']], capture_output=True, text=True)
                created['value'] = True
                log.append({'runner': 'created', 'variant': record['variant'], 'exit': result.returncode,
                            'stat': run('stat', '-c', '%F %s %N', path)})
            else:
                remove()
                log.append({'runner': 'removed', 'variant': record['variant'], 'absent': not os.path.lexists(path)})
            process.stdin.write('ready\n')
            process.stdin.flush()
        process.wait(timeout=300)
    finally:
        remove()
        if process.poll() is None:
            process.kill()
    absent = not os.path.lexists(path)
    stderr = process.stderr.read()
    shutil.rmtree(os.path.join(directory, 'scratch'), ignore_errors=True)
    ok = process.returncode == 0 and absent and any(r.get('complete') for r in log)
    write(os.path.join(directory, 'preload.json'), {'where': 'pinned rootfs (chroot)' if in_chroot else 'host /etc (host libc)',
                                                    'path': path, 'command': command, 'driver_exit': process.returncode,
                                                    'records': log, 'stderr_tail': stderr[-800:],
                                                    'preload_absent_after': absent, 'pass': ok})
    print(json.dumps({'preload': 'PASS' if ok else 'FAIL', 'where': path, 'driver_exit': process.returncode, 'absent_after': absent}))
    return 0 if ok else 1


if __name__ == '__main__':
    commands = {'bundle': bundle, 'preflight': preflight, 'reference': reference, 'inner': inner,
                'preload-chroot': lambda d: preload(d, True), 'preload-host': lambda d: preload(d, False)}
    if len(sys.argv) != 3 or sys.argv[1] not in commands:
        sys.exit(__doc__)
    sys.exit(commands[sys.argv[1]](os.path.abspath(sys.argv[2])))
