// External measurement-only allocation interposer for the 110.1 F1 memory proof
// (Linux/glibc). Never linked into, shipped with, or loaded by the production
// supervisor; graph-proof.py preloads it into a measured worker run.
//
// Every libc allocation is attributed by its caller: inside the worker executable
// (the Cap-charged Rust global allocator; GraphQLite/SQLite reach libc only through
// it) or inside a shared library (libc-internal or loader allocations that bypass the
// charged budget). At every call it updates, for the same instant:
//   charged = bytes the executable requested (what Cap charges),
//   actual  = chunk bytes really consumed (malloc_usable_size + 8 header),
// and records peaks of `actual - charged` (the uncharged remainder), of shared-library
// actual bytes, and of executable chunk overhead. Results go to fd 2 at exit.
#define _GNU_SOURCE
#include <dlfcn.h>
#include <link.h>
#include <malloc.h>
#include <stdatomic.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/mman.h>
#include <unistd.h>

#define SLOTS (1u << 21)
struct slot { uintptr_t key; uint64_t value; }; // value = size << 1 | from_library
static struct slot *table;
static uintptr_t exe_lo, exe_hi;
static atomic_flag guard = ATOMIC_FLAG_INIT;
static long long charged, actual_exe, actual_lib, count_exe, count_lib;
static long long peak_remainder, peak_lib, peak_overhead, peak_charged, peak_actual;
static long long peak_count_exe, overflow;
static void *(*real_malloc)(size_t);
static void (*real_free)(void *);
static void *(*real_calloc)(size_t, size_t);
static void *(*real_realloc)(void *, size_t);
static int (*real_posix_memalign)(void **, size_t, size_t);
static void *(*real_aligned_alloc)(size_t, size_t);
static void *(*real_memalign)(size_t, size_t);
static char bootstrap[4096];
static size_t bootstrap_used;

static void lock(void) { while (atomic_flag_test_and_set(&guard)) {} }
static void unlock(void) { atomic_flag_clear(&guard); }
static uint64_t hash(uintptr_t p) { return (p >> 4) * 0x9E3779B97F4A7C15ull; }
static int in_bootstrap(void *p) {
    return (char *)p >= bootstrap && (char *)p < bootstrap + sizeof bootstrap;
}

static void peaks(void) {
    long long actual = actual_exe + actual_lib;
    if (actual - charged > peak_remainder) peak_remainder = actual - charged;
    if (actual_lib > peak_lib) peak_lib = actual_lib;
    if (actual_exe - charged > peak_overhead) peak_overhead = actual_exe - charged;
    if (charged > peak_charged) peak_charged = charged;
    if (actual > peak_actual) peak_actual = actual;
    if (count_exe > peak_count_exe) peak_count_exe = count_exe;
}
static void track(void *p, size_t size, int library) {
    if (!p || !table) return;
    long long chunk = (long long)malloc_usable_size(p) + 8;
    uint64_t i = hash((uintptr_t)p) & (SLOTS - 1);
    for (unsigned n = 0; n < SLOTS; n++, i = (i + 1) & (SLOTS - 1)) {
        if (table[i].key == 0) {
            table[i].key = (uintptr_t)p;
            table[i].value = ((uint64_t)size << 1) | (uint64_t)library;
            if (library) { actual_lib += chunk; count_lib++; }
            else { charged += (long long)size; actual_exe += chunk; count_exe++; }
            peaks();
            return;
        }
    }
    overflow++;
}
// Backward-shift deletion keeps probe chains intact without tombstones.
static void untrack(void *p) {
    if (!p || !table) return;
    uint64_t i = hash((uintptr_t)p) & (SLOTS - 1);
    for (unsigned n = 0; n < SLOTS; n++, i = (i + 1) & (SLOTS - 1)) {
        if (table[i].key == 0) return;
        if (table[i].key != (uintptr_t)p) continue;
        long long chunk = (long long)malloc_usable_size(p) + 8;
        uint64_t value = table[i].value;
        if (value & 1) { actual_lib -= chunk; count_lib--; }
        else { charged -= (long long)(value >> 1); actual_exe -= chunk; count_exe--; }
        uint64_t hole = i;
        for (uint64_t j = (i + 1) & (SLOTS - 1); table[j].key; j = (j + 1) & (SLOTS - 1)) {
            uint64_t home = hash(table[j].key) & (SLOTS - 1);
            if (((j - home) & (SLOTS - 1)) >= ((j - hole) & (SLOTS - 1))) {
                table[hole] = table[j];
                hole = j;
            }
        }
        table[hole].key = 0;
        return;
    }
}
static int from_library(void *caller) {
    uintptr_t c = (uintptr_t)caller;
    return !(c >= exe_lo && c < exe_hi);
}
static int executable(struct dl_phdr_info *info, size_t size, void *data) {
    (void)size; (void)data;
    if (info->dlpi_name && info->dlpi_name[0]) return 0; // first entry: main program
    for (int i = 0; i < info->dlpi_phnum; i++) {
        if (info->dlpi_phdr[i].p_type != PT_LOAD) continue;
        uintptr_t lo = info->dlpi_addr + info->dlpi_phdr[i].p_vaddr;
        uintptr_t hi = lo + info->dlpi_phdr[i].p_memsz;
        if (!exe_lo || lo < exe_lo) exe_lo = lo;
        if (hi > exe_hi) exe_hi = hi;
    }
    return 1;
}
static void resolve(void) {
    if (real_malloc) return;
    real_calloc = dlsym(RTLD_NEXT, "calloc");
    real_malloc = dlsym(RTLD_NEXT, "malloc");
    real_free = dlsym(RTLD_NEXT, "free");
    real_realloc = dlsym(RTLD_NEXT, "realloc");
    real_posix_memalign = dlsym(RTLD_NEXT, "posix_memalign");
    real_aligned_alloc = dlsym(RTLD_NEXT, "aligned_alloc");
    real_memalign = dlsym(RTLD_NEXT, "memalign");
}
__attribute__((constructor)) static void start(void) {
    resolve();
    void *m = mmap(NULL, sizeof(struct slot) * SLOTS, PROT_READ | PROT_WRITE,
                   MAP_PRIVATE | MAP_ANONYMOUS, -1, 0);
    if (m == MAP_FAILED) _exit(70);
    dl_iterate_phdr(executable, NULL);
    // The worker refuses any inherited environment; the preload is already active.
    unsetenv("LD_PRELOAD");
    lock();
    table = m;
    unlock();
}
__attribute__((destructor)) static void finish(void) {
    char line[512];
    lock();
    int n = snprintf(line, sizeof line,
        "KG_ALLOC {\"peak_uncharged_remainder_bytes\":%lld,\"peak_library_bytes\":%lld,"
        "\"peak_executable_chunk_overhead_bytes\":%lld,\"peak_charged_bytes\":%lld,"
        "\"peak_actual_bytes\":%lld,\"peak_live_executable_allocations\":%lld,"
        "\"table_overflow\":%lld}\n",
        peak_remainder, peak_lib, peak_overhead, peak_charged, peak_actual, peak_count_exe,
        overflow);
    unlock();
    if (n > 0) (void)!write(2, line, (size_t)n);
}

void *malloc(size_t size) {
    if (!real_malloc) {
        resolve();
        if (!real_malloc) return NULL;
    }
    void *p = real_malloc(size);
    lock();
    track(p, size, from_library(__builtin_return_address(0)));
    unlock();
    return p;
}
void *calloc(size_t count, size_t size) {
    if (!real_calloc) {
        // dlsym itself may calloc before resolution completes.
        size_t want = (count * size + 15) & ~(size_t)15;
        if (bootstrap_used + want > sizeof bootstrap) return NULL;
        void *p = bootstrap + bootstrap_used;
        bootstrap_used += want;
        return p;
    }
    void *p = real_calloc(count, size);
    lock();
    track(p, count * size, from_library(__builtin_return_address(0)));
    unlock();
    return p;
}
void free(void *p) {
    if (!p || in_bootstrap(p)) return;
    lock();
    untrack(p);
    unlock();
    real_free(p);
}
void *realloc(void *old, size_t size) {
    if (old && in_bootstrap(old)) {
        void *p = malloc(size);
        if (p) memcpy(p, old, size < sizeof bootstrap ? size : sizeof bootstrap);
        return p;
    }
    lock();
    untrack(old);
    unlock();
    void *p = real_realloc(old, size);
    lock();
    track(p, size, from_library(__builtin_return_address(0)));
    unlock();
    return p;
}
int posix_memalign(void **out, size_t alignment, size_t size) {
    int rc = real_posix_memalign(out, alignment, size);
    if (rc == 0) {
        lock();
        track(*out, size, from_library(__builtin_return_address(0)));
        unlock();
    }
    return rc;
}
void *aligned_alloc(size_t alignment, size_t size) {
    void *p = real_aligned_alloc(alignment, size);
    lock();
    track(p, size, from_library(__builtin_return_address(0)));
    unlock();
    return p;
}
void *memalign(size_t alignment, size_t size) {
    void *p = real_memalign(alignment, size);
    lock();
    track(p, size, from_library(__builtin_return_address(0)));
    unlock();
    return p;
}
