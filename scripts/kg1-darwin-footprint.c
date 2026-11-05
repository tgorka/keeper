/* Story 110.1, Darwin only, measurement-only (never linked into, shipped with or built by
 * any Keeper target; built by scripts/graph-proof.py on macOS like alloc-interposer.c).
 *
 * Owner-approved Darwin criterion (2026-11-05 addendum, Option A): on the tested runs,
 * the kernel task ledger's lifetime-maximum physical footprint of the measured process.
 * This is an empirical per-run measurement, not the Linux accounted-cost proof.
 *
 *   measure [--timeout-ms N] [--read-delay-ms N] [--pre-wait-delay-ms N] STDIN STDOUT STDERR
 *           [NAME=VALUE...] -- PATH ARG...
 *     Spawns PATH (no PATH search) with exactly NAME=VALUE as its environment, fd 0/1/2
 *     opened from STDIN/STDOUT/STDERR and every other descriptor closed
 *     (POSIX_SPAWN_CLOEXEC_DEFAULT). Waits for termination without reaping, reads
 *     RUSAGE_INFO_V6 of the zombie, reaps exactly that child with wait4 and checks the
 *     reap against the wait. Prints one JSON record on stdout and exits 0 only if every
 *     step succeeded; otherwise prints {"error":...} and exits 2. The record never
 *     defaults a missing value to zero.
 *
 *     Deadline (race-safe): an absolute CLOCK_MONOTONIC deadline fixed right after the
 *     spawn. The wait loop checks it at the top of every iteration and again after the
 *     successful termination observation, polls waitid(WEXITED|WNOHANG|WNOWAIT), and
 *     blocks only in kevent() on EVFILT_PROC/NOTE_EXIT with a relative timeout recomputed
 *     from the deadline. No signal carries the deadline, so its enforcement does not
 *     depend on when any signal arrives. Past the deadline the child is SIGKILLed and
 *     reaped and the run fails closed ("timeout"), even if it has already exited.
 *     SIGUSR1 is caught (no SA_RESTART) and only counted: a delivery during kevent() is an
 *     EINTR return, counted in `wait_eintr` and retried, as is EINTR from wait4.
 *     `--pre-wait-delay-ms` (test-only, default 0) sleeps between spawn and the wait, so a
 *     control can make the deadline expire before the blocking call starts.
 *     Built with -DKG1_LEGACY_ALARM_WAIT, the ROUND 29 wait (one-shot SIGALRM checked only
 *     on an EINTR of a blocking waitid) is compiled instead; that binary exists only as the
 *     negative control demonstrating the overrun and is never used for a measurement.
 *   probe MODE MIB CHUNK
 *     Calibration target (run under `measure`): MODE touch | untouched | kill | sleep | hold.
 *     Allocates MIB MiB in CHUNK-byte blocks with every malloc checked, touches every byte
 *     (touch/kill/hold), reads it back into a printed checksum so the writes cannot be
 *     elided, frees everything and reports the exact bytes allocated and touched. `kill`
 *     raises SIGKILL after touching; `hold` stays alive 3 s after touching (the
 *     interrupted-wait control); `sleep` blocks for the timeout control.
 *   probe mapfile PATH
 *     Maps PATH read-only and reads every page (clean file-backed pages).
 */
#include <errno.h>
#include <fcntl.h>
#include <libproc.h>
#include <signal.h>
#include <spawn.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/mman.h>
#include <sys/resource.h>
#include <sys/stat.h>
#include <sys/time.h>
#include <sys/wait.h>
#include <sys/event.h>
#include <time.h>
#include <unistd.h>

static volatile sig_atomic_t usr1_count = 0;
static void on_usr1(int sig) { (void)sig; usr1_count++; }
#ifdef KG1_LEGACY_ALARM_WAIT
static volatile sig_atomic_t alarm_fired = 0;
static void on_alarm(int sig) { (void)sig; alarm_fired = 1; }
#endif

static int fail(const char *what, int code) {
    printf("{\"error\":\"%s\",\"errno\":%d}\n", what, code);
    fflush(stdout);
    return 2;
}

static int parse_u64(const char *text, unsigned long long max, unsigned long long *out) {
    if (!text || !*text || *text == '-' || *text == '+') return 0;
    char *end = NULL;
    errno = 0;
    unsigned long long v = strtoull(text, &end, 10);
    if (errno || !end || *end || v > max) return 0;
    *out = v;
    return 1;
}

static void sleep_ms(long ms) {
    struct timespec t = {ms / 1000, (ms % 1000) * 1000000L};
    while (nanosleep(&t, &t) != 0 && errno == EINTR) {
    }
}

/* Kill and reap the child on an error path; never leaves a zombie or a live child. */
static void discard(pid_t pid) {
    kill(pid, SIGKILL);
    int status;
    while (waitpid(pid, &status, 0) < 0 && errno == EINTR) {
    }
}

#ifndef KG1_LEGACY_ALARM_WAIT
static long long now_ns(void) {
    struct timespec t;
    clock_gettime(CLOCK_MONOTONIC, &t);
    return (long long)t.tv_sec * 1000000000LL + t.tv_nsec;
}
#endif

static int measure(int argc, char **argv) {
    unsigned long long timeout_ms = 600000, delay_ms = 0, pre_wait_ms = 0, wait_eintr = 0;
    int i = 2;
    while (i < argc && strncmp(argv[i], "--", 2) == 0 && strcmp(argv[i], "--") != 0) {
        if (i + 1 >= argc) return fail("usage", 0);
        if (strcmp(argv[i], "--timeout-ms") == 0) {
            if (!parse_u64(argv[i + 1], 86400000ULL, &timeout_ms) || !timeout_ms) return fail("usage_timeout", 0);
        } else if (strcmp(argv[i], "--read-delay-ms") == 0) {
            if (!parse_u64(argv[i + 1], 60000ULL, &delay_ms)) return fail("usage_delay", 0);
        } else if (strcmp(argv[i], "--pre-wait-delay-ms") == 0) {
            if (!parse_u64(argv[i + 1], 60000ULL, &pre_wait_ms)) return fail("usage_pre_wait", 0);
        } else {
            return fail("usage_option", 0);
        }
        i += 2;
    }
    if (argc - i < 5) return fail("usage", 0);
    const char *in = argv[i], *out = argv[i + 1], *err = argv[i + 2];
    int sep = -1;
    for (int j = i + 3; j < argc; j++)
        if (strcmp(argv[j], "--") == 0) { sep = j; break; }
    if (sep < 0 || sep + 1 >= argc || argv[sep + 1][0] != '/') return fail("usage_command", 0);
    int nenv = sep - (i + 3);
    char **envp = calloc((size_t)nenv + 1, sizeof *envp);
    if (!envp) return fail("calloc", errno);
    for (int j = 0; j < nenv; j++) {
        const char *a = argv[i + 3 + j];
        const char *eq = strchr(a, '=');
        if (!eq || eq == a) return fail("usage_env", 0);
        envp[j] = argv[i + 3 + j];
    }
    /* Installed before the child exists; no SA_RESTART, so a delivery interrupts a
     * blocking wait with EINTR. Caught dispositions reset to default in the child at exec. */
    struct sigaction sa;
    memset(&sa, 0, sizeof sa);
    sigemptyset(&sa.sa_mask);
    sa.sa_handler = on_usr1;
    if (sigaction(SIGUSR1, &sa, NULL) != 0) { free(envp); return fail("sigaction_usr1", errno); }
#ifdef KG1_LEGACY_ALARM_WAIT
    sa.sa_handler = on_alarm;
    if (sigaction(SIGALRM, &sa, NULL) != 0) { free(envp); return fail("sigaction_alrm", errno); }
#endif
    posix_spawn_file_actions_t actions;
    posix_spawnattr_t attr;
    int rc = posix_spawn_file_actions_init(&actions);
    if (rc) return fail("file_actions_init", rc);
    rc = posix_spawnattr_init(&attr);
    if (rc) { posix_spawn_file_actions_destroy(&actions); return fail("spawnattr_init", rc); }
    if ((rc = posix_spawn_file_actions_addopen(&actions, 0, in, O_RDONLY, 0)) ||
        (rc = posix_spawn_file_actions_addopen(&actions, 1, out, O_WRONLY | O_CREAT | O_TRUNC, 0644)) ||
        (rc = posix_spawn_file_actions_addopen(&actions, 2, err, O_WRONLY | O_CREAT | O_TRUNC, 0644)) ||
        (rc = posix_spawnattr_setflags(&attr, POSIX_SPAWN_CLOEXEC_DEFAULT))) {
        posix_spawn_file_actions_destroy(&actions);
        posix_spawnattr_destroy(&attr);
        return fail("spawn_setup", rc);
    }
    pid_t pid = 0;
    rc = posix_spawn(&pid, argv[sep + 1], &actions, &attr, &argv[sep + 1], envp);
    posix_spawn_file_actions_destroy(&actions);
    posix_spawnattr_destroy(&attr);
    free(envp);
    if (rc || pid <= 0) return fail("spawn", rc);
    siginfo_t info;
#ifdef KG1_LEGACY_ALARM_WAIT
    struct itimerval timer;
    memset(&timer, 0, sizeof timer);
    timer.it_value.tv_sec = (time_t)(timeout_ms / 1000);
    timer.it_value.tv_usec = (suseconds_t)((timeout_ms % 1000) * 1000);
    if (setitimer(ITIMER_REAL, &timer, NULL) != 0) { int e = errno; discard(pid); return fail("setitimer", e); }
    if (pre_wait_ms) sleep_ms((long)pre_wait_ms);
    for (;;) {
        memset(&info, 0, sizeof info);
        if (waitid(P_PID, (id_t)pid, &info, WEXITED | WNOWAIT) == 0) break;
        int e = errno;
        if (e != EINTR) { discard(pid); return fail("waitid", e); }
        wait_eintr++;
        if (alarm_fired) { discard(pid); return fail("timeout", 0); }
    }
    memset(&timer, 0, sizeof timer);
    if (setitimer(ITIMER_REAL, &timer, NULL) != 0) { int e = errno; discard(pid); return fail("setitimer_cancel", e); }
#else
    const long long deadline = now_ns() + (long long)timeout_ms * 1000000LL;
    if (pre_wait_ms) sleep_ms((long)pre_wait_ms);
    int kq = kqueue();
    if (kq < 0) { int e = errno; discard(pid); return fail("kqueue", e); }
    struct kevent change;
    EV_SET(&change, (uintptr_t)pid, EVFILT_PROC, EV_ADD | EV_ONESHOT, NOTE_EXIT, 0, NULL);
    /* ESRCH: the child already exited; the waitid poll below observes the zombie. */
    int registered = kevent(kq, &change, 1, NULL, 0, NULL) == 0;
    int register_errno = registered ? 0 : errno;
    for (;;) {
        if (now_ns() >= deadline) { close(kq); discard(pid); return fail("timeout", 0); }
        memset(&info, 0, sizeof info);
        if (waitid(P_PID, (id_t)pid, &info, WEXITED | WNOHANG | WNOWAIT) != 0) {
            int e = errno;
            if (e == EINTR) { wait_eintr++; continue; }
            close(kq);
            discard(pid);
            return fail("waitid", e);
        }
        if (info.si_pid == pid) {
            if (now_ns() >= deadline) { close(kq); discard(pid); return fail("timeout", 0); }
            break;
        }
        if (info.si_pid != 0) { close(kq); discard(pid); return fail("waitid_foreign_pid", (int)info.si_pid); }
        if (!registered) { close(kq); discard(pid); return fail("kevent_register", register_errno); }
        long long left = deadline - now_ns();
        if (left <= 0) continue;
        struct timespec ts = {(time_t)(left / 1000000000LL), (long)(left % 1000000000LL)};
        struct kevent event;
        int n = kevent(kq, NULL, 0, &event, 1, &ts);
        if (n < 0) {
            int e = errno;
            if (e == EINTR) { wait_eintr++; continue; }
            close(kq);
            discard(pid);
            return fail("kevent", e);
        }
        if (n == 1 && (event.flags & EV_ERROR)) {
            close(kq);
            discard(pid);
            return fail("kevent_event", (int)event.data);
        }
    }
    close(kq);
#endif
    if (info.si_pid != pid) { discard(pid); return fail("waitid_foreign_pid", (int)info.si_pid); }
    if (info.si_code != CLD_EXITED && info.si_code != CLD_KILLED && info.si_code != CLD_DUMPED) {
        discard(pid);
        return fail("waitid_not_terminal", info.si_code);
    }
    if (delay_ms) sleep_ms((long)delay_ms);
    struct rusage_info_v6 ri;
    memset(&ri, 0, sizeof ri);
    int rr = proc_pid_rusage(pid, RUSAGE_INFO_V6, (rusage_info_t *)&ri);
    int rusage_errno = rr ? errno : 0;
    int status = 0;
    struct rusage ru;
    pid_t reaped;
    for (;;) {
        reaped = wait4(pid, &status, 0, &ru);
        if (reaped >= 0 || errno != EINTR) break;
        wait_eintr++;
    }
    if (reaped != pid) return fail("wait4", reaped < 0 ? errno : 0);
    if (rr != 0) return fail("proc_pid_rusage", rusage_errno);
    int exited = WIFEXITED(status), signalled = WIFSIGNALED(status);
    int consistent = (info.si_code == CLD_EXITED && exited && WEXITSTATUS(status) == info.si_status) ||
                     (info.si_code != CLD_EXITED && signalled && WTERMSIG(status) == info.si_status);
    if (!consistent) return fail("wait_status_inconsistent", status);
    printf("{\"helper\":\"kg1-darwin-footprint\",\"rusage_version\":6,\"pid\":%d,\"waitid_pid\":%d,\"reaped_pid\":%d,"
           "\"exit\":%d,\"signal\":%d,\"lifetime_max_phys_footprint\":%llu,\"phys_footprint_at_read\":%llu,"
           "\"ru_maxrss\":%ld,\"read_delay_ms\":%llu,\"wait_eintr\":%llu,\"sigusr1_received\":%d,\"uuid\":\"",
           pid, info.si_pid, reaped, exited ? WEXITSTATUS(status) : -1, signalled ? WTERMSIG(status) : 0,
           (unsigned long long)ri.ri_lifetime_max_phys_footprint, (unsigned long long)ri.ri_phys_footprint,
           (long)ru.ru_maxrss, delay_ms, wait_eintr, (int)usr1_count);
    for (int b = 0; b < 16; b++) printf("%02x", ri.ri_uuid[b]);
    printf("\"}\n");
    fflush(stdout);
    return 0;
}

static int probe(int argc, char **argv) {
    if (argc == 4 && strcmp(argv[2], "mapfile") == 0) {
        int fd = open(argv[3], O_RDONLY);
        struct stat st;
        if (fd < 0 || fstat(fd, &st) != 0 || st.st_size <= 0) return 71;
        unsigned char *p = mmap(NULL, (size_t)st.st_size, PROT_READ, MAP_PRIVATE, fd, 0);
        if (p == MAP_FAILED) return 71;
        unsigned long long sum = 0;
        for (off_t k = 0; k < st.st_size; k += 4096) sum += p[k];
        printf("{\"mapped_bytes\":%lld,\"checksum\":%llu}\n", (long long)st.st_size, sum);
        munmap(p, (size_t)st.st_size);
        close(fd);
        return 0;
    }
    unsigned long long mib, chunk;
    if (argc != 5 || !parse_u64(argv[3], 4096, &mib) || !parse_u64(argv[4], 1ULL << 30, &chunk) || chunk < 16) return 64;
    const char *mode = argv[2];
    int hold = strcmp(mode, "hold") == 0;
    int touch = strcmp(mode, "touch") == 0 || strcmp(mode, "kill") == 0 || hold;
    if (!touch && strcmp(mode, "untouched") != 0 && strcmp(mode, "sleep") != 0) return 64;
    if (strcmp(mode, "sleep") == 0) { sleep_ms(600000); return 0; }
    unsigned long long total = mib << 20, n = total / chunk, remainder = total % chunk;
    volatile unsigned char **blocks = n ? calloc((size_t)n, sizeof *blocks) : NULL;
    if (n && !blocks) return 71;
    unsigned long long allocated = 0, touched = 0, sum = 0;
    for (unsigned long long k = 0; k < n; k++) {
        blocks[k] = malloc((size_t)chunk);
        if (!blocks[k]) return 71;
        allocated += chunk;
        if (touch) {
            memset((void *)blocks[k], (int)(0xa5 ^ (k & 0xff)), (size_t)chunk);
            touched += chunk;
        }
    }
    if (touch)
        for (unsigned long long k = 0; k < n; k++)
            for (unsigned long long off = 0; off < chunk; off += 4096) sum += blocks[k][off];
    if (strcmp(mode, "kill") == 0) { fflush(stdout); raise(SIGKILL); }
    if (hold) sleep_ms(3000);
    for (unsigned long long k = 0; k < n; k++) free((void *)blocks[k]);
    free((void *)blocks);
    printf("{\"mode\":\"%s\",\"allocated_bytes\":%llu,\"touched_bytes\":%llu,\"chunks\":%llu,\"chunk_bytes\":%llu,"
           "\"remainder_bytes_not_allocated\":%llu,\"pointer_array_bytes\":%llu,\"checksum\":%llu}\n",
           mode, allocated, touched, n, chunk, remainder, (unsigned long long)(n * sizeof *blocks), sum);
    return 0;
}

int main(int argc, char **argv) {
    if (argc >= 2 && strcmp(argv[1], "measure") == 0) return measure(argc, argv);
    if (argc >= 2 && strcmp(argv[1], "probe") == 0) return probe(argc, argv);
    fprintf(stderr, "usage: kg1-darwin-footprint measure|probe ...\n");
    return 64;
}
