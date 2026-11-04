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
//  * Allocator-wide view: mallinfo2() in-use bytes (uordblks + hblkhd) for every arena
//    block and mmapped chunk, whatever entry point created it, sampled after every
//    completed interposed call: allocation (success or failure), free, and every
//    realloc outcome. Allocations and frees completed inside one libc call are not
//    observed.
// Charge semantics are a quiescent logical model of Cap: alloc adds the request, free
// removes it, and a successful libc realloc replaces the old request by the new one,
// never counting both. Cap itself refunds a shrink only after System returns, so the
// model describes the state after each Rust-level operation, not Cap's counter at the
// libc instruction.
// std's System realloc takes `realloc_fallback` (alloc new, copy, dealloc old) exactly
// when it would not call libc realloc, and that fallback always allocates through
// aligned_malloc -> posix_memalign (std/src/sys/alloc/unix.rs). So the sample after an
// executable aligned allocation is deferred: if the next interposed call is an
// executable free (the fallback's dealloc, or a plain free), only the post-free state
// is sampled, where the model again equals Cap's delta charge; otherwise the deferred
// sample is taken on entry to that next call, before it changes any state. The skipped
// instant is the in-flight overlap bounded analytically (design X), never measured.
// Old+new overlap of a moving libc realloc is likewise a separate reconstructed
// diagnostic, never mixed into the model charge.
// Per-sample envelope check (design D1): in_use - 2*charged - 64*small32 - library
// chunk bytes, maximised over samples, must not exceed the allocator in-use already
// present (untracked) when the observer published its table.
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
static long long charged, actual_exe, actual_lib, count_exe, count_lib, count_small_exe, count_small32_exe;
static long long peak_remainder, peak_lib, peak_overhead, peak_charged, peak_actual;
static long long peak_count_exe, peak_small_exe, peak_small32_exe, overflow, wide_aligned, max_moved_old;
static long long max_realloc_old, peak_reconstructed_overlap, peak_mmapped, bootstrap_refused;
static long long peak_in_use, peak_in_use_remainder, samples, untracked_start;
static long long peak_excess = -(1LL << 62), aligned_exe, deferred_by_free;
static int deferred;
static void *(*real_malloc)(size_t);
static void (*real_free)(void *);
static void *(*real_calloc)(size_t, size_t);
static void *(*real_realloc)(void *, size_t);
static int (*real_posix_memalign)(void **, size_t, size_t);
static void *(*real_aligned_alloc)(size_t, size_t);
static void *(*real_memalign)(size_t, size_t);
// Bootstrap arena for dlsym's own calloc before resolution: 16-byte aligned blocks,
// each preceded by a 16-byte header recording its usable size.
static _Alignas(16) unsigned char bootstrap[16384];
static size_t bootstrap_used;

static void lock(void) { while (atomic_flag_test_and_set(&guard)) {} }
static void unlock(void) { atomic_flag_clear(&guard); }
static uint64_t hash(uintptr_t p) { return (p >> 4) * 0x9E3779B97F4A7C15ull; }
// Integer address comparison: relational operators on unrelated C pointers are undefined.
static int in_bootstrap(const void *p) {
    uintptr_t address = (uintptr_t)p, base = (uintptr_t)bootstrap;
    return address >= base && address - base < sizeof bootstrap;
}
static void *bootstrap_alloc(size_t size) {
    if (size > sizeof bootstrap) { bootstrap_refused++; return NULL; }
    size_t rounded = (size + 15) & ~(size_t)15;
    if (rounded < size || 16 + rounded > sizeof bootstrap - bootstrap_used) { bootstrap_refused++; return NULL; }
    unsigned char *block = bootstrap + bootstrap_used;
    memcpy(block, &size, sizeof size);
    bootstrap_used += 16 + rounded;
    memset(block + 16, 0, rounded);
    return block + 16;
}
static size_t bootstrap_size(void *p) {
    size_t size;
    memcpy(&size, (unsigned char *)p - 16, sizeof size);
    return size;
}

static long long in_use_now(void) {
    struct mallinfo2 mi = mallinfo2();
    if ((long long)mi.hblkhd > peak_mmapped) peak_mmapped = (long long)mi.hblkhd;
    return (long long)(mi.uordblks + mi.hblkhd);
}
static void attributed_peaks(void) {
    long long actual = actual_exe + actual_lib;
    if (actual - charged > peak_remainder) peak_remainder = actual - charged;
    if (actual_lib > peak_lib) peak_lib = actual_lib;
    if (actual_exe - charged > peak_overhead) peak_overhead = actual_exe - charged;
    if (charged > peak_charged) peak_charged = charged;
    if (actual > peak_actual) peak_actual = actual;
    if (count_exe > peak_count_exe) peak_count_exe = count_exe;
    if (count_small_exe > peak_small_exe) peak_small_exe = count_small_exe;
    if (count_small32_exe > peak_small32_exe) peak_small32_exe = count_small32_exe;
}
// One allocator-wide sample, compared only with model state of the same instant.
static void sample(void) {
    if (!table) return;
    long long in_use = in_use_now();
    samples++;
    if (in_use > peak_in_use) peak_in_use = in_use;
    if (in_use - charged > peak_in_use_remainder) peak_in_use_remainder = in_use - charged;
    long long excess = in_use - 2 * charged - 64 * count_small32_exe - actual_lib;
    if (excess > peak_excess) peak_excess = excess;
}
// Entry of every interposed call: a deferred post-aligned-allocation sample is taken
// now, before this call changes state, unless this call is an executable free.
static void enter(int executable_free) {
    if (!deferred) return;
    deferred = 0;
    if (executable_free) deferred_by_free++;
    else sample();
}
// Exit of every completed interposed call.
static void post(int defer) {
    attributed_peaks();
    if (defer && table) deferred = 1;
    else sample();
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
                if (size < 32) count_small32_exe++;
            }
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
        if ((value >> 1) < 32) count_small32_exe--;
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
    untracked_start = in_use_now();
    table = m;
    unlock();
}
__attribute__((destructor)) static void finish(void) {
    char line[2048];
    lock();
    int n = snprintf(line, sizeof line,
        "KG_ALLOC {\"peak_uncharged_remainder_bytes\":%lld,\"peak_library_bytes\":%lld,"
        "\"peak_executable_chunk_overhead_bytes\":%lld,\"peak_charged_bytes\":%lld,"
        "\"peak_actual_bytes\":%lld,\"peak_live_executable_allocations\":%lld,"
        "\"peak_live_executable_allocations_below_16\":%lld,"
        "\"peak_allocator_in_use_bytes\":%lld,\"peak_allocator_in_use_minus_charged_bytes\":%lld,"
        "\"allocator_samples\":%lld,\"max_moved_realloc_old_bytes\":%lld,"
        "\"alignment_above_16_calls\":%lld,\"table_overflow\":%lld,"
        "\"peak_live_executable_allocations_below_32\":%lld,\"max_realloc_old_bytes\":%lld,"
        "\"peak_reconstructed_overlap_bytes\":%lld,\"peak_mmapped_bytes\":%lld,"
        "\"bootstrap_refused\":%lld,\"bootstrap_used_bytes\":%lld,"
        "\"untracked_in_use_at_start_bytes\":%lld,\"peak_envelope_excess_bytes\":%lld,"
        "\"executable_aligned_calls\":%lld,\"deferred_samples_closed_by_free\":%lld}\n",
        peak_remainder, peak_lib, peak_overhead, peak_charged, peak_actual, peak_count_exe,
        peak_small_exe, peak_in_use, peak_in_use_remainder, samples, max_moved_old,
        wide_aligned, overflow, peak_small32_exe, max_realloc_old, peak_reconstructed_overlap,
        peak_mmapped, bootstrap_refused, (long long)bootstrap_used, untracked_start,
        samples ? peak_excess : 0, aligned_exe, deferred_by_free);
    unlock();
    if (n > 0) (void)!write(2, line, (size_t)n);
}

void *malloc(size_t size) {
    if (!real_malloc) {
        resolve();
        if (!real_malloc) return NULL;
    }
    int library = from_library(__builtin_return_address(0));
    lock();
    enter(0);
    unlock();
    void *p = real_malloc(size);
    lock();
    track(p, size, library);
    post(0);
    unlock();
    return p;
}
void *calloc(size_t count, size_t size) {
    if (!real_calloc) {
        // dlsym itself may calloc before resolution completes (zeroed, checked).
        if (count && size > SIZE_MAX / count) { bootstrap_refused++; return NULL; }
        return bootstrap_alloc(count * size);
    }
    int library = from_library(__builtin_return_address(0));
    lock();
    enter(0);
    unlock();
    void *p = real_calloc(count, size);
    lock();
    // Overflow: libc refuses and returns NULL, so nothing is recorded.
    if (p) track(p, count * size, library);
    post(0);
    unlock();
    return p;
}
void free(void *p) {
    if (!p || in_bootstrap(p)) return;
    int library = from_library(__builtin_return_address(0));
    lock();
    enter(!library);
    untrack(p);
    unlock();
    real_free(p);
    lock();
    post(0);
    unlock();
}
// realloc: the old record stays until realloc returns (a failed nonzero realloc
// leaves it live and recorded; realloc(p, 0) returning NULL released it). On success
// the old record is removed before the new one is added, mirroring Cap's delta charge,
// so the post-call sample never counts old and new requests together. Every outcome is
// sampled. The old+new overlap of a moving call is a separate reconstructed diagnostic.
void *realloc(void *old, size_t size) {
    if (old && in_bootstrap(old)) {
        size_t old_size = bootstrap_size(old);
        void *p = malloc(size);
        if (p) memcpy(p, old, old_size < size ? old_size : size);
        return p;
    }
    int library = from_library(__builtin_return_address(0));
    lock();
    enter(0);
    unlock();
    void *p = real_realloc(old, size);
    lock();
    long found = find(old);
    long long old_size = found >= 0 ? (long long)(table[found].value >> 1) : 0;
    if (found >= 0 && !(table[found].value & 1) && old_size > max_realloc_old) max_realloc_old = old_size;
    if (!p) {
        if (old && size == 0) untrack(old);
    } else {
        if (p != old && found >= 0) {
            long long overlap = actual_exe + actual_lib + (long long)malloc_usable_size(p) + 8;
            if (overlap > peak_reconstructed_overlap) peak_reconstructed_overlap = overlap;
            if (old_size > max_moved_old) max_moved_old = old_size;
        }
        untrack(old);
        track(p, size, library);
    }
    post(0);
    unlock();
    return p;
}
static void aligned_done(void *p, size_t alignment, size_t size, int library) {
    if (alignment > 16) wide_aligned++;
    if (!library) aligned_exe++;
    track(p, size, library);
    // A possible System realloc_fallback allocation: defer its sample (see header).
    post(p && !library);
}
int posix_memalign(void **out, size_t alignment, size_t size) {
    int library = from_library(__builtin_return_address(0));
    lock();
    enter(0);
    unlock();
    int rc = real_posix_memalign(out, alignment, size);
    lock();
    aligned_done(rc == 0 ? *out : NULL, alignment, size, library);
    unlock();
    return rc;
}
void *aligned_alloc(size_t alignment, size_t size) {
    int library = from_library(__builtin_return_address(0));
    lock();
    enter(0);
    unlock();
    void *p = real_aligned_alloc(alignment, size);
    lock();
    aligned_done(p, alignment, size, library);
    unlock();
    return p;
}
void *memalign(size_t alignment, size_t size) {
    int library = from_library(__builtin_return_address(0));
    lock();
    enter(0);
    unlock();
    void *p = real_memalign(alignment, size);
    lock();
    aligned_done(p, alignment, size, library);
    unlock();
    return p;
}

#ifdef KG_INTERPOSER_SELFTEST
// Positive controls for the bootstrap branch, which only runs while dlsym resolves
// the real allocator and so cannot be reached through a normal preload, and for the
// sampling points. Built as a standalone executable by graph-proof.py (the test binary
// is the "executable" for attribution); prints one JSON line.
int main(void) {
    bootstrap_used = 0;
    unsigned char *a = bootstrap_alloc(5);
    unsigned char *b = bootstrap_alloc(40);
    int aligned = a && b && ((uintptr_t)a % 16 == 0) && ((uintptr_t)b % 16 == 0);
    int extent = a && b && bootstrap_size(a) == 5 && bootstrap_size(b) == 40 && b >= a + 16;
    int zeroed = 1;
    for (int i = 0; b && i < 40; i++) zeroed &= b[i] == 0;
    int membership = in_bootstrap(bootstrap) && in_bootstrap(bootstrap + sizeof bootstrap - 1)
        && !in_bootstrap(bootstrap + sizeof bootstrap) && !in_bootstrap(&bootstrap_used);
    long long before = bootstrap_refused;
    int refuses_huge = bootstrap_alloc((size_t)-1) == NULL && bootstrap_alloc(sizeof bootstrap) == NULL;
    int refuses_overflow = bootstrap_refused == before + 2;
    size_t used = bootstrap_used;
    while (bootstrap_alloc(1024)) {}
    int exhausts = bootstrap_used <= sizeof bootstrap && bootstrap_used > used;
    // Growth copies exactly the old extent: the sentinel in a's rounding padding
    // (bytes 5..15) must not reach the new block.
    memcpy(a, "abcde", 5);
    memset(a + 5, 0xA5, 11);
    unsigned char *grown = realloc(a, 64);
    int copied = grown && memcmp(grown, "abcde", 5) == 0;
    int padding_hits = 0;
    for (int i = 5; grown && i < 16; i++) padding_hits += grown[i] == 0xA5;
    int copy_stops_at_old = grown && padding_hits < 11;
    // Shrink copies exactly the new extent and preserves it.
    for (int i = 0; i < 40; i++) b[i] = (unsigned char)(0x40 + i);
    unsigned char *shrunk = realloc(b, 3);
    int shrink_ok = shrunk && shrunk[0] == 0x40 && shrunk[1] == 0x41 && shrunk[2] == 0x42;
    free(bootstrap_alloc(8));  // bootstrap blocks are never passed to the real free
    // Sampling points: free and every realloc outcome take a post-call sample.
    long long s0 = samples;
    free(grown);
    int sampled_free = samples == s0 + 1;
    // A failed realloc leaves the block live; volatile keeps the compiler from treating
    // the retained pointer as consumed.
    unsigned char *volatile held = shrunk;
    s0 = samples;
    void *failed = realloc(held, (size_t)-1 / 2);
    int sampled_failed_realloc = failed == NULL && samples == s0 + 1 && find(held) >= 0;
    s0 = samples;
    void *null_realloc = realloc(NULL, 24);
    int sampled_null_realloc = null_realloc && samples == s0 + 1 && find(null_realloc) >= 0;
    free(null_realloc);
    free(held);
    // An executable aligned allocation defers its sample; an executable free closes it
    // with only the post-free sample, any other call takes it on entry.
    void *x = NULL;
    s0 = samples;
    long long closed = deferred_by_free;
    int deferred_ok = posix_memalign(&x, 16, 48) == 0 && samples == s0 && deferred;
    free(x);
    deferred_ok = deferred_ok && samples == s0 + 1 && deferred_by_free == closed + 1 && !deferred;
    s0 = samples;
    x = aligned_alloc(16, 64);
    void *y = malloc(8);
    int deferred_entry_ok = x && y && samples == s0 + 2 && !deferred;
    free(y);
    free(x);
    int envelope_ok = peak_excess <= untracked_start && aligned_exe == 2;
    printf("{\"aligned\":%d,\"extent\":%d,\"zeroed\":%d,\"membership\":%d,\"refuses_overflow\":%d,"
           "\"refuses_huge\":%d,\"exhausts_within_buffer\":%d,\"realloc_copies_min\":%d,"
           "\"realloc_grow_stops_at_old_extent\":%d,\"realloc_shrink_preserves\":%d,"
           "\"sampled_after_free\":%d,\"sampled_after_failed_realloc\":%d,"
           "\"sampled_after_null_realloc\":%d,\"deferred_closed_by_free\":%d,"
           "\"deferred_taken_on_entry\":%d,\"envelope_within_untracked\":%d}\n",
           aligned, extent, zeroed, membership, refuses_overflow, refuses_huge, exhausts, copied,
           copy_stops_at_old, shrink_ok, sampled_free, sampled_failed_realloc, sampled_null_realloc,
           deferred_ok, deferred_entry_ok, envelope_ok);
    return 0;
}
#endif
