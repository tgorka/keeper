"""Story 110.1 reference-run monitor: the pre-registered fail-closed starvation and
host-contention predicate of final re-review round 12 §5 (proof-only; never shipped).

`Predicate.observe(sample)` is a pure, incremental evaluation over strictly parsed
samples, including the final snapshot. `run_monitored` becomes a child subreaper,
samples every 250 ms for a 30-second quiet baseline, launches the proof, keeps
sampling until the proof and every other process of the scope subtree are gone, and
on the first breach (or a signal, or any monitor failure) latches the reason with the
sample's time and counters, kills every scope member and descendant, reaps and
verifies that none remains, then evaluates a final snapshot. Each sample is appended
to a JSONL file and a heartbeat file is replaced, so an independent watchdog outside
the scope can kill it if the monitor stalls or dies.

Verdicts: PASS, PROOF_FAILED, MEMORY_STARVED, HOST_CONTENDED, MONITOR_INVALID.
Thresholds are those registered as PREDICATE_VERSION; they are never tuned after a run.
"""
import ctypes
import json
import math
import os
import signal
import subprocess
import time

PREDICATE_VERSION = 'kg1-1101-r12-section5-v1'
IMPLEMENTATION = 'r14'
INTERVAL_S = 0.25
MAX_GAP_S = 1.0
BASELINE_S = 30.0
WINDOW_S = 10.0
CLEANUP_S = 10.0
MEM_AVAILABLE_FLOOR = 1024 ** 3
SCOPE_MEM_SOME_AVG = 1.00
SCOPE_MEM_FULL_AVG = 0.10
SCOPE_MEM_SOME_TOTAL_US = 10_000_000
SCOPE_MEM_FULL_TOTAL_US = 1_000_000
SCOPE_MEM_SOME_RATE = 0.01
SCOPE_MEM_FULL_RATE = 0.001
HOST_MEM_FULL_AVG = 0.10
HOST_MEM_FULL_RATE = 0.001
HOST_IO_FULL_AVG10 = 5.00
HOST_IO_FULL_RATE = 0.05
STEAL_RATE = 0.01
SCOPE_CPU_SOME_AVG10 = 5.00
SCOPE_CPU_SOME_RATE = 0.05
EVENT_KEYS = ('oom', 'oom_kill', 'oom_group_kill', 'max', 'high')
SWAP_EVENT_KEYS = ('high', 'max', 'fail')
CPU_STAT_KEYS = ('usage_usec', 'user_usec', 'system_usec', 'nr_periods', 'nr_throttled', 'throttled_usec')
ASSIGNED_CPUS = ('cpu0', 'cpu1', 'cpu2', 'cpu3')
EXPECTED_LIMITS = {'cpuset.cpus.effective': '0-3', 'cpu.max': '400000 100000',
                   'memory.max': str(8 * 1024 ** 3), 'memory.swap.max': '0'}
EIGHT_GIB = 8 * 1024 ** 3


class Invalid(Exception):
    """Missing, malformed, negative, non-finite or unreadable telemetry: never a zero."""


class Stop(BaseException):
    """Raised by the signal handlers; latched as MONITOR_INVALID."""


def _read(path):
    try:
        with open(path) as f:
            value = f.read()
    except OSError as error:
        raise Invalid(f'{path}: {error}') from error
    if not value.strip():
        raise Invalid(f'{path}: empty')
    return value


def _int(text, where):
    if not isinstance(text, str) or not text.isdigit():
        raise Invalid(f'{where}: not a nonnegative integer: {text!r}')
    return int(text)


def _float(text, where):
    try:
        value = float(text)
    except (TypeError, ValueError) as error:
        raise Invalid(f'{where}: not a number: {text!r}') from error
    if not math.isfinite(value) or value < 0 or value > 100:
        raise Invalid(f'{where}: out of range: {text!r}')
    return value


def parse_keyed(text, where, required):
    values = {}
    for line in text.splitlines():
        parts = line.split()
        if len(parts) != 2:
            raise Invalid(f'{where}: malformed line {line!r}')
        if parts[0] in values:
            raise Invalid(f'{where}: duplicate key {parts[0]}')
        values[parts[0]] = _int(parts[1], where)
    missing = [k for k in required if k not in values]
    if missing:
        raise Invalid(f'{where}: missing {missing}')
    return values


def parse_psi(text, where, kinds=('some', 'full')):
    out = {}
    for line in text.splitlines():
        parts = line.split()
        if not parts:
            continue
        if parts[0] in out:
            raise Invalid(f'{where}: duplicate {parts[0]}')
        fields = {}
        for p in parts[1:]:
            if '=' not in p:
                raise Invalid(f'{where}: malformed field {p!r}')
            key, value = p.split('=', 1)
            if key in fields:
                raise Invalid(f'{where}: duplicate field {key}')
            fields[key] = value
        if set(fields) != {'avg10', 'avg60', 'avg300', 'total'}:
            raise Invalid(f'{where}: unexpected fields {sorted(fields)}')
        out[parts[0]] = {'avg10': _float(fields['avg10'], where), 'avg60': _float(fields['avg60'], where),
                         'avg300': _float(fields['avg300'], where), 'total': _int(fields['total'], where)}
    missing = [k for k in kinds if k not in out]
    if missing:
        raise Invalid(f'{where}: missing {missing}')
    return out


def parse_stat(text):
    cpus = {}
    for line in text.splitlines():
        parts = line.split()
        if parts and parts[0] in ('cpu',) + ASSIGNED_CPUS:
            if parts[0] in cpus:
                raise Invalid(f'/proc/stat: duplicate {parts[0]}')
            if len(parts) < 9:
                raise Invalid(f'/proc/stat: short line {line!r}')
            # user nice system idle iowait irq softirq steal (guest/guest_nice excluded)
            cpus[parts[0]] = [_int(x, '/proc/stat') for x in parts[1:9]]
    missing = [c for c in ('cpu',) + ASSIGNED_CPUS if c not in cpus]
    if missing:
        raise Invalid(f'/proc/stat: missing {missing}')
    return cpus


def parse_meminfo(text):
    fields = {}
    for line in text.splitlines():
        if ':' not in line:
            continue
        key, rest = line.split(':', 1)
        parts = rest.split()
        if key in fields:
            raise Invalid(f'/proc/meminfo: duplicate {key}')
        fields[key] = parts
    out = {}
    for key in ('MemAvailable', 'SwapFree', 'MemTotal'):
        parts = fields.get(key)
        if not parts or len(parts) != 2 or parts[1] != 'kB':
            raise Invalid(f'/proc/meminfo: {key} missing or not in kB: {parts}')
        out[key] = _int(parts[0], '/proc/meminfo') * 1024
    return out


def scope_members(cgroup):
    """Every pid in the scope cgroup and all of its child cgroups. An unreadable or
    malformed membership file is Invalid; an empty child cgroup is legitimately empty."""
    pids = set()
    for directory, _, files in os.walk(cgroup):
        if 'cgroup.procs' not in files:
            continue
        try:
            with open(os.path.join(directory, 'cgroup.procs')) as f:
                content = f.read()
        except OSError as error:
            raise Invalid(f'{directory}/cgroup.procs: {error}') from error
        pids.update(_int(p, 'cgroup.procs') for p in content.split())
    return sorted(pids)


def expand_cpus(spec):
    cpus = set()
    for part in spec.split(','):
        if not part:
            raise Invalid(f'cpuset: empty element in {spec!r}')
        if '-' in part:
            low, high = part.split('-', 1)
            cpus.update(range(_int(low, 'cpuset'), _int(high, 'cpuset') + 1))
        else:
            cpus.add(_int(part, 'cpuset'))
    return cpus


def ancestor_problems(entry):
    """Ancestor restrictions that are tighter than (or cannot be shown to admit) the scope."""
    problems = []
    memory = entry['memory.max']
    if memory != 'max' and (not memory.isdigit() or int(memory) < EIGHT_GIB):
        problems.append(f'{entry["path"]} memory.max {memory!r} is tighter than 8 GiB or malformed')
    quota = entry['cpu.max'].split()
    if len(quota) != 2 or not quota[1].isdigit() or int(quota[1]) == 0 or \
            (quota[0] != 'max' and (not quota[0].isdigit() or int(quota[0]) * 100000 < 400000 * int(quota[1]))):
        problems.append(f'{entry["path"]} cpu.max {entry["cpu.max"]!r} is tighter than 400% or malformed')
    try:
        if not {0, 1, 2, 3} <= expand_cpus(entry['cpuset.cpus.effective']):
            problems.append(f'{entry["path"]} cpuset {entry["cpuset.cpus.effective"]!r} does not include 0-3')
    except Invalid as error:
        problems.append(f'{entry["path"]} cpuset malformed: {error}')
    return problems


def ancestors(cgroup, read=lambda p: _read(p).strip()):
    out = []
    node = os.path.dirname(cgroup)
    while node.startswith('/sys/fs/cgroup') and node != '/sys/fs/cgroup':
        entry = {'path': node}
        for name in ('memory.max', 'memory.swap.max', 'cpu.max', 'cpuset.cpus.effective'):
            entry[name] = read(os.path.join(node, name))
        out.append(entry)
        node = os.path.dirname(node)
    return out


def collect(cgroup, t=None):
    """One strict sample. Any unreadable, malformed, negative or non-finite required field
    raises Invalid."""
    j = lambda name: os.path.join(cgroup, name)
    sample = {'t': time.monotonic() if t is None else t, 'wall': time.time()}
    sample['limits'] = {name: _read(j(name)).strip() for name in EXPECTED_LIMITS}
    sample['ancestors'] = ancestors(cgroup)
    sample['affinity'] = sorted(os.sched_getaffinity(0))
    procs = scope_members(cgroup)
    sample['descendants'] = descendants(os.getpid())
    # A member that exited between the two reads is not "outside"; a live one is kept.
    sample['procs'] = [p for p in procs if p == os.getpid() or p in sample['descendants'] or _alive(p)]
    sample['events'] = parse_keyed(_read(j('memory.events')), 'memory.events', EVENT_KEYS)
    sample['events_local'] = parse_keyed(_read(j('memory.events.local')), 'memory.events.local', EVENT_KEYS)
    sample['swap_current'] = _int(_read(j('memory.swap.current')).strip(), 'memory.swap.current')
    sample['swap_peak'] = _int(_read(j('memory.swap.peak')).strip(), 'memory.swap.peak')
    sample['swap_events'] = parse_keyed(_read(j('memory.swap.events')), 'memory.swap.events', SWAP_EVENT_KEYS)
    sample['memory_current'] = _int(_read(j('memory.current')).strip(), 'memory.current')
    sample['memory_peak'] = _int(_read(j('memory.peak')).strip(), 'memory.peak')
    sample['scope_mem_psi'] = parse_psi(_read(j('memory.pressure')), 'memory.pressure')
    sample['scope_cpu_psi'] = parse_psi(_read(j('cpu.pressure')), 'cpu.pressure', ('some',))
    sample['cpu_stat'] = parse_keyed(_read(j('cpu.stat')), 'cpu.stat', CPU_STAT_KEYS)
    meminfo = parse_meminfo(_read('/proc/meminfo'))
    sample['host_mem_available'] = meminfo['MemAvailable']
    sample['host_swap_free'] = meminfo['SwapFree']
    sample['host_mem_psi'] = parse_psi(_read('/proc/pressure/memory'), '/proc/pressure/memory')
    sample['host_io_psi'] = parse_psi(_read('/proc/pressure/io'), '/proc/pressure/io')
    sample['host_cpu_psi'] = parse_psi(_read('/proc/pressure/cpu'), '/proc/pressure/cpu', ('some',))
    sample['host_stat'] = parse_stat(_read('/proc/stat'))
    vm = parse_keyed('\n'.join(l for l in _read('/proc/vmstat').splitlines() if l.split()[0] in ('pswpin', 'pswpout')),
                     '/proc/vmstat', ('pswpin', 'pswpout'))
    sample['host_pswpin'], sample['host_pswpout'] = vm['pswpin'], vm['pswpout']
    return sample


class Predicate:
    """Incremental evaluation. `observe` returns None or (verdict, reason)."""

    def __init__(self, self_pid=None):
        self.samples = []
        self.self_pid = self_pid

    @staticmethod
    def _counters(s):
        c = {f'events.{k}': s['events'][k] for k in EVENT_KEYS}
        c.update({f'events_local.{k}': s['events_local'][k] for k in EVENT_KEYS})
        c.update({f'swap_events.{k}': s['swap_events'][k] for k in SWAP_EVENT_KEYS})
        for name in ('scope_mem_psi', 'host_mem_psi', 'host_io_psi'):
            c[f'{name}.some'] = s[name]['some']['total']
            c[f'{name}.full'] = s[name]['full']['total']
        c['scope_cpu_psi.some'] = s['scope_cpu_psi']['some']['total']
        c['host_cpu_psi.some'] = s['host_cpu_psi']['some']['total']
        for cpu, fields in s['host_stat'].items():
            for n, v in enumerate(fields):
                c[f'stat.{cpu}.{n}'] = v
        c['pswpin'], c['pswpout'] = s['host_pswpin'], s['host_pswpout']
        c.update({f'cpu_stat.{k}': s['cpu_stat'][k] for k in CPU_STAT_KEYS})
        c['swap_peak'] = s['swap_peak']
        c['memory_peak'] = s['memory_peak']
        return c

    def observe(self, s):
        prev = self.samples[-1] if self.samples else None
        self.samples.append(s)
        first = self.samples[0]
        # Monitor validity: limits, ancestors, affinity and membership on every sample
        if s['limits'] != EXPECTED_LIMITS:
            return 'MONITOR_INVALID', f'scope limits changed or unverified: {s["limits"]}'
        for entry in s['ancestors']:
            problems = ancestor_problems(entry)
            if problems:
                return 'MONITOR_INVALID', f'ancestor restriction: {problems[0]}'
        if s['ancestors'] != first['ancestors']:
            return 'MONITOR_INVALID', 'ancestor restrictions changed during the run'
        if s['affinity'] != [0, 1, 2, 3]:
            return 'MONITOR_INVALID', f'affinity changed: {s["affinity"]}'
        if self.self_pid is not None:
            if self.self_pid not in s['procs']:
                return 'MONITOR_INVALID', 'monitor is not a member of the verified scope'
            outside = sorted(set(s['procs']) - {self.self_pid} - set(s['descendants']))
            if outside:
                return 'MONITOR_INVALID', f'scope members outside the monitored process tree: {outside}'
        if prev is not None:
            gap = s['t'] - prev['t']
            if gap > MAX_GAP_S or gap <= 0:
                return 'MONITOR_INVALID', f'sample gap {gap:.3f}s'
            now, before = self._counters(s), self._counters(prev)
            for key, value in now.items():
                if value < before[key]:
                    return 'MONITOR_INVALID', f'counter {key} decreased {before[key]} -> {value}'
        else:
            dirty = {f'events.{k}': s['events'][k] for k in EVENT_KEYS if s['events'][k]}
            dirty.update({f'events_local.{k}': s['events_local'][k] for k in EVENT_KEYS if s['events_local'][k]})
            dirty.update({f'swap_events.{k}': s['swap_events'][k] for k in SWAP_EVENT_KEYS if s['swap_events'][k]})
            if dirty or s['swap_current'] or s['swap_peak']:
                return 'MONITOR_INVALID', f'fresh scope starts with memory events/swap {dirty}'
        # MEMORY_STARVED 1-2: events and swap
        for key in EVENT_KEYS:
            if s['events'][key] > first['events'][key] or s['events_local'][key] > first['events_local'][key]:
                return 'MEMORY_STARVED', f'memory.events {key} increased'
        if s['swap_current'] > 0 or s['swap_peak'] > 0:
            return 'MEMORY_STARVED', f'scope swap current={s["swap_current"]} peak={s["swap_peak"]}'
        for key in SWAP_EVENT_KEYS:
            if s['swap_events'][key] > first['swap_events'][key]:
                return 'MEMORY_STARVED', f'memory.swap.events {key} increased'
        # MEMORY_STARVED 3: sampled averages and absolute cumulative ceilings
        psi = s['scope_mem_psi']
        for avg in ('avg10', 'avg60', 'avg300'):
            if psi['some'][avg] > SCOPE_MEM_SOME_AVG:
                return 'MEMORY_STARVED', f'scope memory some {avg}={psi["some"][avg]}'
            if psi['full'][avg] > SCOPE_MEM_FULL_AVG:
                return 'MEMORY_STARVED', f'scope memory full {avg}={psi["full"][avg]}'
        some_delta = psi['some']['total'] - first['scope_mem_psi']['some']['total']
        full_delta = psi['full']['total'] - first['scope_mem_psi']['full']['total']
        if some_delta > SCOPE_MEM_SOME_TOTAL_US:
            return 'MEMORY_STARVED', f'scope memory some total delta {some_delta}us'
        if full_delta > SCOPE_MEM_FULL_TOTAL_US:
            return 'MEMORY_STARVED', f'scope memory full total delta {full_delta}us'
        # MEMORY_STARVED 5: host floor
        if s['host_mem_available'] < MEM_AVAILABLE_FLOOR:
            return 'MEMORY_STARVED', f'host MemAvailable {s["host_mem_available"]} < 1 GiB'
        elapsed = s['t'] - first['t']
        # MEMORY_STARVED 4: cumulative rates once ten seconds have elapsed
        if elapsed >= WINDOW_S:
            if some_delta / (elapsed * 1e6) > SCOPE_MEM_SOME_RATE:
                return 'MEMORY_STARVED', f'scope memory some cumulative rate {some_delta / (elapsed * 1e6):.4%}'
            if full_delta / (elapsed * 1e6) > SCOPE_MEM_FULL_RATE:
                return 'MEMORY_STARVED', f'scope memory full cumulative rate {full_delta / (elapsed * 1e6):.4%}'
        # HOST_CONTENDED: sampled averages
        for avg in ('avg10', 'avg60', 'avg300'):
            if s['host_mem_psi']['full'][avg] > HOST_MEM_FULL_AVG:
                return 'HOST_CONTENDED', f'host memory full {avg}={s["host_mem_psi"]["full"][avg]}'
        if s['host_io_psi']['full']['avg10'] > HOST_IO_FULL_AVG10:
            return 'HOST_CONTENDED', f'host io full avg10={s["host_io_psi"]["full"]["avg10"]}'
        if s['scope_cpu_psi']['some']['avg10'] > SCOPE_CPU_SOME_AVG10:
            return 'HOST_CONTENDED', f'scope cpu some avg10={s["scope_cpu_psi"]["some"]["avg10"]}'
        # Trailing ten-second window ending at this sample (actual elapsed time)
        start = None
        for older in reversed(self.samples[:-1]):
            if s['t'] - older['t'] >= WINDOW_S:
                start = older
                break
        if start is not None:
            span_us = (s['t'] - start['t']) * 1e6
            rate = lambda a, b: (a - b) / span_us
            checks = (
                ('MEMORY_STARVED', 'scope memory some 10s rate', rate(psi['some']['total'], start['scope_mem_psi']['some']['total']), SCOPE_MEM_SOME_RATE),
                ('MEMORY_STARVED', 'scope memory full 10s rate', rate(psi['full']['total'], start['scope_mem_psi']['full']['total']), SCOPE_MEM_FULL_RATE),
                ('HOST_CONTENDED', 'host memory full 10s rate', rate(s['host_mem_psi']['full']['total'], start['host_mem_psi']['full']['total']), HOST_MEM_FULL_RATE),
                ('HOST_CONTENDED', 'host io full 10s rate', rate(s['host_io_psi']['full']['total'], start['host_io_psi']['full']['total']), HOST_IO_FULL_RATE),
                ('HOST_CONTENDED', 'scope cpu some 10s rate', rate(s['scope_cpu_psi']['some']['total'], start['scope_cpu_psi']['some']['total']), SCOPE_CPU_SOME_RATE),
            )
            for verdict, name, value, limit in checks:
                if value > limit:
                    return verdict, f'{name} {value:.4%} > {limit:.2%}'
            for cpu in ('cpu',) + ASSIGNED_CPUS:
                deltas = [a - b for a, b in zip(s['host_stat'][cpu], start['host_stat'][cpu])]
                total = sum(deltas)
                if total <= 0:
                    return 'MONITOR_INVALID', f'/proc/stat {cpu} zero denominator over a 10s window'
                if deltas[7] / total > STEAL_RATE:
                    return 'HOST_CONTENDED', f'{cpu} steal {deltas[7] / total:.4%} over 10s'
        return None


def descendants(root_pid):
    """All live (non-zombie excluded is the caller's choice) descendants of root_pid via /proc ppid links."""
    children = {}
    for entry in os.listdir('/proc'):
        if entry.isdigit():
            try:
                with open(f'/proc/{entry}/stat') as f:
                    fields = f.read().rsplit(')', 1)[1].split()
                children.setdefault(int(fields[1]), []).append(int(entry))
            except (OSError, IndexError):
                continue
    found, stack = [], [root_pid]
    while stack:
        for child in children.get(stack.pop(), []):
            found.append(child)
            stack.append(child)
    return sorted(found)


def become_subreaper():
    """PR_SET_CHILD_SUBREAPER: orphaned descendants (setsid, exited leaders) reparent to
    this process, so they stay enumerable and reapable."""
    libc = ctypes.CDLL(None, use_errno=True)
    if libc.prctl(36, 1, 0, 0, 0) != 0:
        raise OSError(ctypes.get_errno(), 'prctl(PR_SET_CHILD_SUBREAPER)')


def _reap(keep=()):
    """Reap zombie children (orphans reparented to this subreaper) by pid, never the
    Popen-tracked leaders in `keep`, so their exit status is preserved."""
    me = os.getpid()
    for entry in os.listdir('/proc'):
        if not entry.isdigit() or int(entry) in keep:
            continue
        try:
            with open(f'/proc/{entry}/stat') as f:
                fields = f.read().rsplit(')', 1)[1].split()
        except OSError:
            continue
        if fields[0] == 'Z' and int(fields[1]) == me:
            try:
                os.waitpid(int(entry), os.WNOHANG)
            except ChildProcessError:
                pass


def _alive(pid):
    try:
        with open(f'/proc/{pid}/stat') as f:
            return f.read().rsplit(')', 1)[1].split()[0] != 'Z'
    except OSError:
        return False


def kill_all(process, members=lambda: [], deadline_s=CLEANUP_S):
    """SIGKILL the proof's process group, every descendant of the monitor and every other
    scope member, repeatedly, reaping as subreaper, until none remains or the deadline
    passes. Returns (signalled, errors, remaining); errors and remaining are failures."""
    me = os.getpid()
    signalled, errors = set(), []
    end = time.monotonic() + deadline_s
    while True:
        try:
            current = set(members())
        except Invalid as error:
            errors.append(f'membership unreadable: {error}')
            current = set()
        targets = (current | set(descendants(me))) - {me}
        live = {p for p in targets if _alive(p)}
        if process is not None:
            try:
                os.killpg(process.pid, signal.SIGKILL)
            except ProcessLookupError:
                pass
            except PermissionError as error:
                errors.append(f'killpg {process.pid}: {error}')
        for pid in live:
            try:
                os.kill(pid, signal.SIGKILL)
                signalled.add(pid)
            except ProcessLookupError:
                pass
            except PermissionError as error:
                errors.append(f'kill {pid}: {error}')
        if process is not None:
            process.poll()
        _reap(keep=(process.pid,) if process is not None else ())
        if not live:
            break
        if time.monotonic() > end:
            break
        time.sleep(0.05)
    if process is not None and process.poll() is None:
        try:
            process.wait(timeout=max(0.0, end - time.monotonic()))
        except subprocess.TimeoutExpired:
            errors.append(f'proof leader {process.pid} not reaped')
    _reap(keep=(process.pid,) if process is not None else ())
    try:
        remaining_members = set(members())
    except Invalid as error:
        errors.append(f'membership unreadable after cleanup: {error}')
        remaining_members = set()
    remaining = sorted(p for p in (remaining_members | set(descendants(me))) - {me} if _alive(p))
    return sorted(signalled), sorted(set(errors)), remaining


def run_monitored(command, samples_path, collect_sample, *, env=None, stdout=None, stderr=None,
                  preexec_fn=None, baseline_s=BASELINE_S, interval_s=INTERVAL_S, self_pid=None,
                  members=lambda: [], heartbeat_path=None, sleep=time.sleep, max_s=4 * 3600,
                  subreaper=True):
    """Baseline, launch, monitor to the last descendant, latch-stop on breach, verified
    cleanup, final snapshot evaluated by the same predicate."""
    if subreaper:
        become_subreaper()
    predicate = Predicate(self_pid=self_pid)
    latched = None
    process = None
    started = None
    cleanup = None
    seq = {'n': 0}

    def stop(signum, _frame):
        raise Stop(f'signal {signum}')
    previous = {sig: signal.signal(sig, stop) for sig in (signal.SIGTERM, signal.SIGHUP, signal.SIGINT)}

    def latch(verdict, reason, phase, sample=None):
        nonlocal latched
        if latched is None:
            latched = {'verdict': verdict, 'reason': reason, 'phase': phase, 't': time.monotonic(),
                       'sample_index': len(predicate.samples) - 1 if sample is not None else None,
                       'counters': Predicate._counters(sample) if sample is not None else None}

    with open(samples_path, 'a') as log:
        def beat(phase):
            if heartbeat_path:
                seq['n'] += 1
                tmp = heartbeat_path + '.tmp'
                with open(tmp, 'w') as f:
                    json.dump({'seq': seq['n'], 'phase': phase, 'pid': os.getpid(), 'mono': time.monotonic()}, f)
                os.replace(tmp, heartbeat_path)

        def take(phase):
            s = collect_sample()
            s['phase'] = phase
            log.write(json.dumps(s) + '\n')
            log.flush()
            beat(phase)
            return predicate.observe(s), s
        try:
            t0 = time.monotonic()
            while time.monotonic() - t0 < baseline_s:
                breach, s = take('baseline')
                if breach:
                    latch(*breach, 'baseline', s)
                    break
                sleep(interval_s)
            if latched is None:
                started = time.monotonic()
                process = subprocess.Popen(command, env=env, stdout=stdout, stderr=stderr, preexec_fn=preexec_fn,
                                           start_new_session=True)
                while True:
                    breach, s = take('proof')
                    if breach:
                        latch(*breach, 'proof', s)
                        break
                    if process.poll() is not None:
                        _reap()
                        others = [p for p in set(s['procs']) | set(descendants(os.getpid())) if p != os.getpid() and _alive(p)]
                        if not others:
                            break
                    if time.monotonic() - started > max_s:
                        latch('MONITOR_INVALID', f'proof exceeded the {max_s}s monitor deadline', 'proof', s)
                        break
                    sleep(interval_s)
        except Invalid as error:
            latch('MONITOR_INVALID', str(error), 'proof' if process else 'baseline')
        except Stop as error:
            latch('MONITOR_INVALID', f'monitor stopped: {error}', 'proof' if process else 'baseline')
        except Exception as error:
            latch('MONITOR_INVALID', f'monitor failure: {error!r}', 'proof' if process else 'baseline')
        finally:
            # From here to the return, a further signal must not abandon cleanup or the
            # final snapshot; the watchdog outside the scope still bounds a hang.
            for sig in previous:
                signal.signal(sig, signal.SIG_IGN)
            try:
                cleanup = kill_all(process, members)
            except Exception as error:
                cleanup = ([], [f'cleanup raised {error!r}'], [-1])
        final = None
        try:
            final = collect_sample()
            final['phase'] = 'final'
            log.write(json.dumps(final) + '\n')
            log.flush()
            beat('final')
            breach = predicate.observe(final)
            if breach:
                latch(*breach, 'final', final)
        except Exception as error:
            latch('MONITOR_INVALID', f'final snapshot: {error!r}', 'final')
    for sig, handler in previous.items():
        signal.signal(sig, handler)
    signalled, errors, remaining = cleanup
    cleanup_verified = not errors and not remaining
    exit_code = process.returncode if process is not None else None
    if latched:
        verdict = latched['verdict']
    elif not cleanup_verified:
        verdict = 'MONITOR_INVALID'
    elif exit_code == 0:
        verdict = 'PASS'
    else:
        verdict = 'PROOF_FAILED'
    if not cleanup_verified and latched and latched['verdict'] != 'MONITOR_INVALID':
        # The first breach stays the verdict; unverified cleanup is recorded beside it and
        # can never yield PASS.
        latched['cleanup_unverified'] = True
    return {'predicate_version': PREDICATE_VERSION, 'implementation': IMPLEMENTATION, 'verdict': verdict,
            'latched': latched, 'proof_exit': exit_code,
            'proof_elapsed_s': round(time.monotonic() - started, 1) if started else None,
            'samples': len(predicate.samples),
            'cleanup': {'signalled': signalled, 'errors': errors, 'remaining': remaining, 'verified': cleanup_verified},
            'final': final}


def watch(process, heartbeat_path, kill_scope, *, stall_s=5.0, first_beat_s=120.0, poll_s=0.5, max_s=5 * 3600):
    """Independent watchdog, run outside the scope: if the monitor's heartbeat stops
    advancing (stall or death) or this process is signalled, kill the whole scope."""
    events = []
    last_seq, last_change = None, time.monotonic()
    begun = time.monotonic()

    def stop(signum, _frame):
        raise Stop(f'signal {signum}')
    previous = {sig: signal.signal(sig, stop) for sig in (signal.SIGTERM, signal.SIGHUP, signal.SIGINT)}
    try:
        while process.poll() is None:
            try:
                with open(heartbeat_path) as f:
                    seq = json.load(f).get('seq')
            except (OSError, ValueError):
                seq = None
            now = time.monotonic()
            if seq is not None and seq != last_seq:
                last_seq, last_change = seq, now
            stalled = (last_seq is None and now - begun > first_beat_s) or (last_seq is not None and now - last_change > stall_s)
            if stalled or now - begun > max_s:
                events.append({'event': 'watchdog_kill', 'reason': 'stall' if stalled else 'deadline', 'last_seq': last_seq,
                               'since_last_beat_s': round(now - last_change, 2)})
                events.append({'event': 'kill_scope', 'result': kill_scope()})
                break
            time.sleep(poll_s)
    except Stop as error:
        events.append({'event': 'watchdog_signal', 'reason': str(error)})
        events.append({'event': 'kill_scope', 'result': kill_scope()})
    finally:
        for sig, handler in previous.items():
            signal.signal(sig, handler)
    try:
        process.wait(timeout=60)
    except subprocess.TimeoutExpired:
        events.append({'event': 'scope_process_not_reaped'})
    return events
