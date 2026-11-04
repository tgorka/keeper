"""Story 110.1 reference-run monitor: the pre-registered fail-closed starvation and
host-contention predicate of final re-review round 12 §5 (proof-only; never shipped).

`Predicate.observe(sample)` is a pure, incremental evaluation over strictly parsed
samples. `run_monitored` samples every 250 ms for a 30-second quiet baseline, launches
the proof, keeps sampling until the proof and every other scope process are gone, and on
the first breach latches the reason, kills and reaps every descendant, takes a final
snapshot and stops. Raw samples are appended to a JSONL file as they are taken.

Verdicts: PASS, PROOF_FAILED, MEMORY_STARVED, HOST_CONTENDED, MONITOR_INVALID.
Thresholds are fixed here and recorded with PREDICATE_VERSION; they are never tuned
after a run.
"""
import json
import os
import signal
import subprocess
import time

PREDICATE_VERSION = 'kg1-1101-r12-section5-v1'
INTERVAL_S = 0.25
MAX_GAP_S = 1.0
BASELINE_S = 30.0
WINDOW_S = 10.0
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
ASSIGNED_CPUS = ('cpu0', 'cpu1', 'cpu2', 'cpu3')
EXPECTED_LIMITS = {'cpuset.cpus.effective': '0-3', 'cpu.max': '400000 100000',
                   'memory.max': str(8 * 1024 ** 3), 'memory.swap.max': '0'}


class Invalid(Exception):
    """Missing, malformed or unreadable telemetry: never a zero."""


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
    try:
        return int(text)
    except (TypeError, ValueError) as error:
        raise Invalid(f'{where}: not an integer: {text!r}') from error


def parse_keyed(text, where, required):
    values = {}
    for line in text.splitlines():
        parts = line.split()
        if len(parts) != 2:
            raise Invalid(f'{where}: malformed line {line!r}')
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
        fields = dict(p.split('=', 1) for p in parts[1:] if '=' in p)
        try:
            out[parts[0]] = {'avg10': float(fields['avg10']), 'avg60': float(fields['avg60']),
                             'avg300': float(fields['avg300']), 'total': int(fields['total'])}
        except (KeyError, ValueError) as error:
            raise Invalid(f'{where}: malformed {line!r}') from error
    missing = [k for k in kinds if k not in out]
    if missing:
        raise Invalid(f'{where}: missing {missing}')
    return out


def parse_stat(text):
    cpus = {}
    for line in text.splitlines():
        parts = line.split()
        if parts and parts[0] in ('cpu',) + ASSIGNED_CPUS:
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
        if ':' in line:
            key, rest = line.split(':', 1)
            parts = rest.split()
            if parts:
                fields[key] = _int(parts[0], '/proc/meminfo') * (1024 if len(parts) > 1 and parts[1] == 'kB' else 1)
    for key in ('MemAvailable', 'SwapFree', 'MemTotal'):
        if key not in fields:
            raise Invalid(f'/proc/meminfo: missing {key}')
    return fields


def collect(cgroup, t=None):
    """One strict sample. Any unreadable/malformed required field raises Invalid."""
    j = lambda name: os.path.join(cgroup, name)
    sample = {'t': time.monotonic() if t is None else t, 'wall': time.time()}
    sample['limits'] = {name: _read(j(name)).strip() for name in EXPECTED_LIMITS}
    sample['procs'] = [_int(p, 'cgroup.procs') for p in _read(j('cgroup.procs')).split()]
    sample['events'] = parse_keyed(_read(j('memory.events')), 'memory.events', EVENT_KEYS)
    sample['events_local'] = parse_keyed(_read(j('memory.events.local')), 'memory.events.local', EVENT_KEYS)
    sample['swap_current'] = _int(_read(j('memory.swap.current')).strip(), 'memory.swap.current')
    sample['swap_peak'] = _int(_read(j('memory.swap.peak')).strip(), 'memory.swap.peak')
    sample['swap_events'] = parse_keyed(_read(j('memory.swap.events')), 'memory.swap.events', SWAP_EVENT_KEYS)
    sample['memory_current'] = _int(_read(j('memory.current')).strip(), 'memory.current')
    sample['memory_peak'] = _int(_read(j('memory.peak')).strip(), 'memory.peak')
    sample['scope_mem_psi'] = parse_psi(_read(j('memory.pressure')), 'memory.pressure')
    sample['scope_cpu_psi'] = parse_psi(_read(j('cpu.pressure')), 'cpu.pressure', ('some',))
    sample['cpu_stat'] = parse_keyed(_read(j('cpu.stat')), 'cpu.stat', ('usage_usec',))
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

    def _counters(self, s):
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
        c['cpu_stat.usage_usec'] = s['cpu_stat']['usage_usec']
        c['swap_peak'] = s['swap_peak']
        return c

    def observe(self, s):
        prev = self.samples[-1] if self.samples else None
        self.samples.append(s)
        first = self.samples[0]
        # Monitor validity
        if s['limits'] != EXPECTED_LIMITS:
            return 'MONITOR_INVALID', f'scope limits changed or unverified: {s["limits"]}'
        if self.self_pid is not None and self.self_pid not in s['procs']:
            return 'MONITOR_INVALID', 'monitor is not a member of the verified scope'
        if prev is not None:
            gap = s['t'] - prev['t']
            if gap > MAX_GAP_S or gap <= 0:
                return 'MONITOR_INVALID', f'sample gap {gap:.3f}s'
            now, before = self._counters(s), self._counters(prev)
            for key, value in now.items():
                if value < before[key]:
                    return 'MONITOR_INVALID', f'counter {key} decreased {before[key]} -> {value}'
        else:
            nonzero = {k: v for k, v in s['events'].items() if k in EVENT_KEYS and v}
            if nonzero or s['swap_current'] or s['swap_peak'] or any(s['swap_events'][k] for k in SWAP_EVENT_KEYS):
                return 'MONITOR_INVALID', f'fresh scope starts with memory events/swap {nonzero}'
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
    """All live descendants of root_pid by walking /proc ppid links."""
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
    return found


def kill_all(process, cgroup_procs=()):
    """SIGKILL the proof's process group, every /proc descendant of the monitor and every
    other member of the verified scope, then reap the direct child."""
    me = os.getpid()
    targets = set(descendants(me)) | {p for p in cgroup_procs if p != me}
    try:
        os.killpg(process.pid, signal.SIGKILL)
    except (ProcessLookupError, PermissionError):
        pass
    for pid in targets:
        try:
            os.kill(pid, signal.SIGKILL)
        except (ProcessLookupError, PermissionError):
            pass
    try:
        process.wait(timeout=30)
    except subprocess.TimeoutExpired:
        pass
    # Reap any other zombie children of the monitor.
    while True:
        try:
            pid, _ = os.waitpid(-1, os.WNOHANG)
        except ChildProcessError:
            break
        if pid == 0:
            break
    return sorted(targets)


def run_monitored(command, samples_path, collect_sample, *, env=None, stdout=None, stderr=None,
                  preexec_fn=None, baseline_s=BASELINE_S, interval_s=INTERVAL_S, self_pid=None,
                  scope_procs=lambda s: s['procs'], sleep=time.sleep, max_s=4 * 3600):
    """Baseline, launch, monitor to the last descendant, latch-stop on breach."""
    predicate = Predicate(self_pid=self_pid)
    latched = None
    process = None
    started = None
    killed = []
    with open(samples_path, 'a') as log:
        def take(phase):
            s = collect_sample()
            s['phase'] = phase
            log.write(json.dumps(s) + '\n')
            log.flush()
            return predicate.observe(s), s
        try:
            t0 = time.monotonic()
            while time.monotonic() - t0 < baseline_s:
                breach, _ = take('baseline')
                if breach:
                    latched = breach + ('baseline',)
                    break
                sleep(interval_s)
            if latched is None:
                started = time.monotonic()
                process = subprocess.Popen(command, env=env, stdout=stdout, stderr=stderr, preexec_fn=preexec_fn,
                                           start_new_session=True)
                while True:
                    breach, s = take('proof')
                    if breach:
                        latched = breach + ('proof',)
                        killed = kill_all(process, scope_procs(s))
                        break
                    if process.poll() is not None:
                        others = [p for p in scope_procs(s) if p != os.getpid()] + descendants(os.getpid())
                        if not others:
                            break
                    if time.monotonic() - started > max_s:
                        latched = ('MONITOR_INVALID', f'proof exceeded the {max_s}s monitor deadline', 'proof')
                        killed = kill_all(process, scope_procs(s))
                        break
                    sleep(interval_s)
        except Invalid as error:
            latched = ('MONITOR_INVALID', str(error), 'proof' if process else 'baseline')
            if process is not None:
                killed = kill_all(process)
        except BaseException as error:
            latched = ('MONITOR_INVALID', f'monitor failure: {error!r}', 'proof' if process else 'baseline')
            if process is not None:
                killed = kill_all(process)
            if not isinstance(error, Exception):
                raise
        final = None
        try:
            final = collect_sample()
            final['phase'] = 'final'
            log.write(json.dumps(final) + '\n')
        except Invalid as error:
            latched = latched or ('MONITOR_INVALID', f'final snapshot: {error}', 'final')
    exit_code = process.returncode if process is not None else None
    if latched:
        verdict = latched[0]
    elif exit_code == 0:
        verdict = 'PASS'
    else:
        verdict = 'PROOF_FAILED'
    return {'predicate_version': PREDICATE_VERSION, 'verdict': verdict,
            'latched': {'verdict': latched[0], 'reason': latched[1], 'phase': latched[2]} if latched else None,
            'proof_exit': exit_code, 'proof_elapsed_s': round(time.monotonic() - started, 1) if started else None,
            'samples': len(predicate.samples), 'killed': killed,
            'leftover_descendants': descendants(os.getpid()), 'final': final}
