// External measurement-only allocation observer for the 110.1 F1 memory proof
// (Linux/glibc). Inventory row M1: never linked into, shipped with, or loaded by the
// production supervisor; graph-proof.py preloads it into a measured worker run.
//
// Two independent views, reported side by side:
//  * Attributed view: calls that reach the interposed exported symbols (malloc, calloc,
//    realloc, free, posix_memalign, aligned_alloc, memalign) are attributed by caller
//    address to the worker executable (Cap-charged Rust allocator; GraphQLite/SQLite
//    reach libc only through it) or to a shared library (libc-internal or loader).
//    This sees only calls routed through those exported symbols.
//  * Allocator-wide view: at every interposed call, mallinfo2() in-use bytes
//    (uordblks + hblkhd) for every arena block and mmapped chunk, whatever entry point
//    created it (internal or hidden callers included), sampled at those instants only.
// Heap chunk cost uses glibc 2.43's model (malloc_usable_size + 8); mmapped chunks are
// visible in the allocator-wide view (hblkhd). The observer's own table is a direct
// mmap and appears in neither view. Results go to fd 2 at exit.
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
// value = requested size << 1 | from_library; chunk = cost recorded at allocation, so
// release/overlap accounting never reads a freed block.
struct slot { uintptr_t key; uint64_t value; long long chunk; };
static struct slot *table;
static uintptr_t exe_lo, exe_hi;
static atomic_flag guard = ATOMIC_FLAG_INIT;
static long long charged, actual_exe, actual_lib, count_exe, count_lib, count_small_exe;
static long long peak_remainder, peak_lib, peak_overhead, peak_charged, peak_actual;
static long long peak_count_exe, peak_small_exe, overflow, wide_aligned, max_moved_old;
static long long peak_in_use, peak_in_use_remainder, samples;
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
    if (count_small_exe > peak_small_exe) peak_small_exe = count_small_exe;
    if (!table) return;
    struct mallinfo2 mi = mallinfo2();
    long long in_use = (long long)(mi.uordblks + mi.hblkhd);
    samples++;
    if (in_use > peak_in_use) peak_in_use = in_use;
    if (in_use - charged > peak_in_use_remainder) peak_in_use_remainder = in_use - charged;
}
static void track(void *p, size_t size, int library) {
    if (!p || !table) return;
    long long chunk = (long long)malloc_usable_size(p) + 8;
    uint64_t i = hash((uintptr_t)p) & (SLOTS - 1);
    for (unsigned n = 0; n < SLOTS; n++, i = (i + 1) & (SLOTS - 1)) {
        if (table[i].key == 0) {
            table[i].key = (uintptr_t)p;
            table[i].value = ((uint64_t)size << 1) | (uint64_t)library;
            table[i].chunk = chunk;
            if (library) { actual_lib += chunk; count_lib++; }
            else {
                charged += (long long)size; actual_exe += chunk; count_exe++;
                if (size < 16) count_small_exe++;
            }
            peaks();
            return;
        }
    }
    overflow++;
}
static long find(void *p) {
    if (!p || !table) return -1;
    uint64_t i = hash((uintptr_t)p) & (SLOTS - 1);
    for (unsigned n = 0; n < SLOTS; n++, i = (i + 1) & (SLOTS - 1)) {
        if (table[i].key == 0) return -1;
        if (table[i].key == (uintptr_t)p) return (long)i;
    }
    return -1;
}
// Backward-shift deletion keeps probe chains intact without tombstones.
static void untrack(void *p) {
    long found = find(p);
    if (found < 0) return;
    uint64_t i = (uint64_t)found, value = table[i].value;
    long long chunk = table[i].chunk;
    if (value & 1) { actual_lib -= chunk; count_lib--; }
    else {
        charged -= (long long)(value >> 1); actual_exe -= chunk; count_exe--;
        if ((value >> 1) < 16) count_small_exe--;
    }
    uint64_t hole = i;
    for (uint64_t j = (i + 1) & (SLOTS - 1); table[j].key; j = (j + 1) & (SLOTS - 1)) {
        uint64_t home = hash(table[j].key) & (SLOTS - 1);
        if (((j - home) & (SLOTS - 1)) >= ((j - hole) & (SLOTS - 1))) {
            table[hole] = table[j];
            hole = j;
        }
    }
    table[hole].key = 0;
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
    char line[1024];
    lock();
    int n = snprintf(line, sizeof line,
        "KG_ALLOC {\"peak_uncharged_remainder_bytes\":%lld,\"peak_library_bytes\":%lld,"
        "\"peak_executable_chunk_overhead_bytes\":%lld,\"peak_charged_bytes\":%lld,"
        "\"peak_actual_bytes\":%lld,\"peak_live_executable_allocations\":%lld,"
        "\"peak_live_executable_allocations_below_16\":%lld,"
        "\"peak_allocator_in_use_bytes\":%lld,\"peak_allocator_in_use_minus_charged_bytes\":%lld,"
        "\"allocator_samples\":%lld,\"max_moved_realloc_old_bytes\":%lld,"
        "\"alignment_above_16_calls\":%lld,\"table_overflow\":%lld}\n",
        peak_remainder, peak_lib, peak_overhead, peak_charged, peak_actual, peak_count_exe,
        peak_small_exe, peak_in_use, peak_in_use_remainder, samples, max_moved_old,
        wide_aligned, overflow);
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
    if (count && size > SIZE_MAX / count) {
        // Overflow: libc refuses; nothing is allocated, nothing is recorded.
        return real_calloc ? real_calloc(count, size) : NULL;
    }
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
// The old block stays recorded until realloc returns. A move is recorded with old and
// new simultaneously live (the copy overlap) before the old one is released; a failed
// nonzero realloc leaves the old block live and recorded; realloc(p, 0) returning NULL
// released p.
void *realloc(void *old, size_t size) {
    if (old && in_bootstrap(old)) {
        void *p = malloc(size);
        if (p) memcpy(p, old, size < sizeof bootstrap ? size : sizeof bootstrap);
        return p;
    }
    int library = from_library(__builtin_return_address(0));
    void *p = real_realloc(old, size);
    lock();
    if (!p) {
        if (old && size == 0) untrack(old);
    } else if (p == old) {
        untrack(old);
        track(p, size, library);
    } else {
        long found = find(old);
        if (found >= 0) {
            long long old_size = (long long)(table[found].value >> 1);
            if (old_size > max_moved_old) max_moved_old = old_size;
        }
        track(p, size, library);
        untrack(old);
    }
    unlock();
    return p;
}
static void note_alignment(size_t alignment) {
    if (alignment > 16) wide_aligned++;
}
int posix_memalign(void **out, size_t alignment, size_t size) {
    int rc = real_posix_memalign(out, alignment, size);
    if (rc == 0) {
        lock();
        note_alignment(alignment);
        track(*out, size, from_library(__builtin_return_address(0)));
        unlock();
    }
    return rc;
}
void *aligned_alloc(size_t alignment, size_t size) {
    void *p = real_aligned_alloc(alignment, size);
    lock();
    note_alignment(alignment);
    track(p, size, from_library(__builtin_return_address(0)));
    unlock();
    return p;
}
void *memalign(size_t alignment, size_t size) {
    void *p = real_memalign(alignment, size);
    lock();
    note_alignment(alignment);
    track(p, size, from_library(__builtin_return_address(0)));
    unlock();
    return p;
}
