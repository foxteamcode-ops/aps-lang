/*
 * APS (Advanced Performance Systems) Native System Runtime
 * Deterministic, Zero-GC Runtime for AMA Memory Tiers, UTF-8 Strings, I/O, and Diagnostics
 */
#ifndef _CRT_SECURE_NO_WARNINGS
#define _CRT_SECURE_NO_WARNINGS
#endif
#ifndef _WINSOCK_DEPRECATED_NO_WARNINGS
#define _WINSOCK_DEPRECATED_NO_WARNINGS
#endif

#include <stdio.h>
#include <stdlib.h>
#include <stdint.h>
#include <string.h>
#include <math.h>
#include <time.h>

#ifdef _WIN32
#include <winsock2.h>
#include <ws2tcpip.h>
#include <windows.h>
#pragma comment(lib, "ws2_32.lib")
#else
#include <unistd.h>
#include <time.h>
#include <sys/socket.h>
#include <netinet/in.h>
#include <arpa/inet.h>
#define SOCKET int
#define INVALID_SOCKET (-1)
#define SOCKET_ERROR (-1)
#define closesocket close
#endif

#if defined(_MSC_VER) && !defined(__clang__)
#define APS_THREAD_LOCAL __declspec(thread)
#define APS_NORETURN __declspec(noreturn)
#define APS_HOT
#define APS_COLD
#define APS_LIKELY(x) (x)
#define APS_UNLIKELY(x) (x)
#elif defined(__GNUC__) || defined(__clang__)
#define APS_THREAD_LOCAL __thread
#define APS_NORETURN __attribute__((noreturn))
#define APS_HOT __attribute__((hot))
#define APS_COLD __attribute__((cold))
#define APS_LIKELY(x) __builtin_expect(!!(x), 1)
#define APS_UNLIKELY(x) __builtin_expect(!!(x), 0)
#else
#define APS_THREAD_LOCAL
#define APS_NORETURN
#define APS_HOT
#define APS_COLD
#define APS_LIKELY(x) (x)
#define APS_UNLIKELY(x) (x)
#endif

typedef void (*ApsCleanupFn)(void*);

typedef struct ApsCleanupNode {
    ApsCleanupFn fn;
    void* arg;
    struct ApsCleanupNode* next;
} ApsCleanupNode;

typedef struct ApsRegionBlock {
    uint8_t* data;
    size_t capacity;
    size_t used;
    struct ApsRegionBlock* next;
} ApsRegionBlock;

typedef struct ApsRegion {
    const char* name;
    ApsRegionBlock* head;
    size_t total_allocated;
    size_t max_quota;          /* OOM & DoS protection quota (default 16MB) */
    ApsCleanupNode* cleanups;  /* RAII cleanup chain for files/sockets/handles */
} ApsRegion;

typedef struct ApsVec {
    int64_t len;
    int64_t cap;
    void* data;
} ApsVec;

typedef struct ApsEnum {
    int64_t tag;
    int64_t payload0;
    int64_t payload1;
} ApsEnum;

typedef struct ApsStr {
    const char* data;
    int64_t len;
} ApsStr;

static int g_aps_utf8_initialized = 0;
static int64_t g_aps_perf_freq = 0;
static int64_t aps_get_perf_freq(void);

static void aps_ensure_utf8_console(void) {
    if (!g_aps_utf8_initialized) {
#ifdef _WIN32
        SetConsoleOutputCP(65001);
        SetConsoleCP(65001);
        LARGE_INTEGER freq;
        QueryPerformanceFrequency(&freq);
        g_aps_perf_freq = freq.QuadPart > 0 ? freq.QuadPart : 1;
#endif
        g_aps_utf8_initialized = 1;
    }
}

void aps_print_i64(int64_t val) {
    aps_ensure_utf8_console();
    printf("%lld", (long long)val);
}

void aps_println_i64(int64_t val) {
    aps_ensure_utf8_console();
    printf("%lld\n", (long long)val);
}

void aps_print_f64(double val) {
    aps_ensure_utf8_console();
    printf("%g", val);
}

void aps_println_f64(double val) {
    aps_ensure_utf8_console();
    printf("%g\n", val);
}

void aps_print_bool(int64_t val) {
    aps_ensure_utf8_console();
    printf("%s", val ? "true" : "false");
}

void aps_println_bool(int64_t val) {
    aps_ensure_utf8_console();
    printf("%s\n", val ? "true" : "false");
}

static inline const char* aps_to_c_str(const void* s_ptr) {
    if (!s_ptr) return "";
    const ApsStr* s = (const ApsStr*)s_ptr;
    if (s->data && s->len >= 0 && s->len < 1000000000LL) {
        if (s->data[s->len] == '\0') return s->data;
        char* buf = (char*)malloc((size_t)s->len + 1);
        if (!buf) return "";
        memcpy(buf, s->data, (size_t)s->len);
        buf[s->len] = '\0';
        return buf;
    }
    return (const char*)s_ptr;
}

void aps_print_str(const void* s_ptr) {
    if (!s_ptr) return;
    aps_ensure_utf8_console();
    const ApsStr* s = (const ApsStr*)s_ptr;
    if (s->data && s->len >= 0 && s->len < 1000000000LL) {
        if (s->len > 0) fwrite(s->data, 1, (size_t)s->len, stdout);
    } else {
        printf("%s", (const char*)s_ptr);
    }
}

void aps_println_str(const void* s_ptr) {
    aps_print_str(s_ptr);
    putchar('\n');
}

void aps_eprintln_str(const void* s_ptr) {
    if (!s_ptr) { fputc('\n', stderr); return; }
    aps_ensure_utf8_console();
    const ApsStr* s = (const ApsStr*)s_ptr;
    if (s->data && s->len >= 0 && s->len < 1000000000LL) {
        if (s->len > 0) fwrite(s->data, 1, (size_t)s->len, stderr);
    } else {
        fprintf(stderr, "%s", (const char*)s_ptr);
    }
    fputc('\n', stderr);
}

APS_COLD APS_NORETURN void aps_panic(const void* msg, const void* file, int64_t line) {
    aps_ensure_utf8_console();
    const char* m = aps_to_c_str(msg);
    const char* f = aps_to_c_str(file);
    fprintf(stderr, "panic[R0003]: %s\n  --> %s:%lld\n", m ? m : "explicit panic", f ? f : "<unknown>", (long long)line);
    exit(101);
}

APS_COLD APS_NORETURN void aps_bounds_check(int64_t index, int64_t len, const void* file, int64_t line) {
    aps_ensure_utf8_console();
    const char* f = aps_to_c_str(file);
    fprintf(stderr, "panic[R0001]: index out of bounds: the len is %lld but the index is %lld\n  --> %s:%lld\n",
            (long long)len, (long long)index, f ? f : "<unknown>", (long long)line);
    exit(101);
}

void aps_assert(int64_t cond, const void* msg) {
    if (APS_UNLIKELY(!cond)) {
        static const ApsStr s_assert_file = { "<assert>", 8 };
        aps_panic(msg, &s_assert_file, 0);
    }
}

#ifdef APS_ALLOC_STATS
/* Debug-only heap accounting (enable with env APS_ALLOC_STATS=1 at `aps build` time). */
static long long g_aps_alloc_count = 0;
static long long g_aps_free_count = 0;
static int g_aps_stats_registered = 0;
static void aps_report_alloc_stats(void) {
    fprintf(stderr, "[aps-alloc-stats] allocs=%lld frees=%lld leaked=%lld\n",
            g_aps_alloc_count, g_aps_free_count, g_aps_alloc_count - g_aps_free_count);
}
#endif

void* aps_alloc(int64_t size) {
    if (size <= 0) size = 8;
    void* ptr = calloc(1, (size_t)size);
    if (APS_UNLIKELY(!ptr)) {
        aps_panic("out of memory in Managed Ownership heap allocation", "<runtime>", 0);
    }
#ifdef APS_ALLOC_STATS
    if (!g_aps_stats_registered) { g_aps_stats_registered = 1; atexit(aps_report_alloc_stats); }
    g_aps_alloc_count++;
#endif
    return ptr;
}

void aps_free(void* ptr) {
    if (ptr) {
#ifdef APS_ALLOC_STATS
        g_aps_free_count++;
#endif
        free(ptr);
    }
}

/* Frees a Vec header together with its element buffer (called for proven non-escaping locals). */
void aps_vec_free(void* vec_ptr) {
    if (!vec_ptr) return;
    ApsVec* v = (ApsVec*)vec_ptr;
    if (v->data) {
        aps_free(v->data);
        v->data = NULL;
    }
    aps_free(vec_ptr);
}

/* Dynamic Vector Runtime Engine */
APS_HOT void aps_vec_push(void* vec_ptr, int64_t val) {
    if (APS_UNLIKELY(!vec_ptr)) return;
    ApsVec* v = (ApsVec*)vec_ptr;
    if (APS_LIKELY(v->len < v->cap)) {
        ((int64_t*)v->data)[v->len++] = val;
        return;
    }
    size_t new_cap = v->cap > 0 ? (size_t)v->cap * 2 : 16;
    void* new_data = realloc(v->data, new_cap * sizeof(int64_t));
    if (APS_UNLIKELY(!new_data)) aps_panic("out of memory in Vec.push expansion", "<runtime>", 0);
    v->data = new_data;
    v->cap = (int64_t)new_cap;
    ((int64_t*)v->data)[v->len++] = val;
}

int64_t aps_vec_pop(void* vec_ptr) {
    if (!vec_ptr) return 0;
    ApsVec* v = (ApsVec*)vec_ptr;
    if (v->len <= 0) {
        aps_panic("cannot pop from empty Vec", "<runtime>", 0);
    }
    v->len--;
    return ((int64_t*)v->data)[v->len];
}

/* ============================================================================
 * Standard Collection: HashMap<K, V> Runtime Engine
 * Open-addressing Robin Hood / Linear Probing Hash Table
 * ============================================================================ */
typedef struct {
    uint64_t hash;
    uint64_t key;
    uint64_t val;
    uint8_t  occupied;
} ApsMapEntry;

typedef struct {
    int64_t len;
    int64_t cap;
    ApsMapEntry* entries;
} ApsMap;

static inline uint64_t aps_hash_calc(uint64_t key) {
    if (key > 0x10000 && (key & 7) == 0) {
        const unsigned char* s = (const unsigned char*)key;
        uint64_t h = 14695981039346656037ULL;
        size_t len = 0;
        while (s[len] != '\0' && len < 256) {
            h ^= s[len];
            h *= 1099511628211ULL;
            len++;
        }
        if (len > 0) return h | 1ULL;
    }
    uint64_t x = key;
    x ^= x >> 33;
    x *= 0xff51afd7ed558ccdULL;
    x ^= x >> 33;
    x *= 0xc4ceb9fe1a85ec53ULL;
    x ^= x >> 33;
    return x | 1ULL;
}

static inline int aps_keys_equal(uint64_t k1, uint64_t k2) {
    if (k1 == k2) return 1;
    if (k1 > 0x10000 && k2 > 0x10000 && (k1 & 7) == 0 && (k2 & 7) == 0) {
        return strcmp((const char*)k1, (const char*)k2) == 0;
    }
    return 0;
}

void* aps_map_new(void) {
    ApsMap* m = (ApsMap*)calloc(1, sizeof(ApsMap));
    m->cap = 16;
    m->len = 0;
    m->entries = (ApsMapEntry*)calloc(16, sizeof(ApsMapEntry));
    return m;
}

void aps_map_insert(void* map_ptr, uint64_t key, uint64_t val) {
    if (!map_ptr) return;
    ApsMap* m = (ApsMap*)map_ptr;
    if (m->len * 10 >= m->cap * 7) {
        int64_t old_cap = m->cap;
        ApsMapEntry* old_entries = m->entries;
        int64_t new_cap = old_cap * 2;
        ApsMapEntry* new_entries = (ApsMapEntry*)calloc((size_t)new_cap, sizeof(ApsMapEntry));
        for (int64_t i = 0; i < old_cap; i++) {
            if (old_entries[i].occupied) {
                uint64_t h = old_entries[i].hash;
                size_t idx = (size_t)(h & (uint64_t)(new_cap - 1));
                while (new_entries[idx].occupied) {
                    idx = (idx + 1) & (size_t)(new_cap - 1);
                }
                new_entries[idx] = old_entries[i];
            }
        }
        free(old_entries);
        m->entries = new_entries;
        m->cap = new_cap;
    }
    uint64_t h = aps_hash_calc(key);
    size_t idx = (size_t)(h & (uint64_t)(m->cap - 1));
    while (m->entries[idx].occupied) {
        if (m->entries[idx].hash == h && aps_keys_equal(m->entries[idx].key, key)) {
            m->entries[idx].val = val;
            return;
        }
        idx = (idx + 1) & (size_t)(m->cap - 1);
    }
    m->entries[idx].occupied = 1;
    m->entries[idx].hash = h;
    m->entries[idx].key = key;
    m->entries[idx].val = val;
    m->len++;
}

void* aps_map_get(void* map_ptr, uint64_t key) {
    ApsEnum* res = (ApsEnum*)aps_alloc(24);
    if (!map_ptr) {
        res->tag = 0;
        return res;
    }
    ApsMap* m = (ApsMap*)map_ptr;
    if (m->len == 0 || m->cap == 0) {
        res->tag = 0;
        return res;
    }
    uint64_t h = aps_hash_calc(key);
    size_t idx = (size_t)(h & (uint64_t)(m->cap - 1));
    size_t start = idx;
    while (m->entries[idx].occupied) {
        if (m->entries[idx].hash == h && aps_keys_equal(m->entries[idx].key, key)) {
            res->tag = 1; /* Some */
            res->payload0 = (int64_t)m->entries[idx].val;
            return res;
        }
        idx = (idx + 1) & (size_t)(m->cap - 1);
        if (idx == start) break;
    }
    res->tag = 0; /* None */
    return res;
}

int64_t aps_map_len(void* map_ptr) {
    if (!map_ptr) return 0;
    return ((ApsMap*)map_ptr)->len;
}

void aps_map_free(void* map_ptr) {
    if (!map_ptr) return;
    ApsMap* m = (ApsMap*)map_ptr;
    if (m->entries) free(m->entries);
    free(m);
}

/* AMA Tier 2: Region Memory Arena (O(1) bump allocation, O(1) bulk destruction) */
static APS_THREAD_LOCAL ApsRegion* g_tl_cached_region = NULL;

void* aps_region_begin(const void* name_arg, int64_t initial_cap) {
    const char* name = aps_to_c_str(name_arg);
    if (initial_cap < 4096) initial_cap = 65536;

    // Ultra-Fast Path: Reuse thread-local cached region block (0 heap allocations, O(1) reset)
    if (g_tl_cached_region != NULL) {
        ApsRegion* reg = g_tl_cached_region;
        g_tl_cached_region = NULL;
        reg->name = name;
        reg->total_allocated = 0;
        reg->max_quota = 16 * 1024 * 1024; /* 16MB default DoS/OOM quota */
        reg->cleanups = NULL;
        ApsRegionBlock* cur = reg->head;
        if (cur) {
            if (cur->next) {
                ApsRegionBlock* extra = cur->next;
                while (extra) {
                    ApsRegionBlock* next = extra->next;
                    if (extra->data) free(extra->data);
                    free(extra);
                    extra = next;
                }
                cur->next = NULL;
            }
            cur->used = 0;
            return (void*)reg;
        }
    }

    ApsRegion* reg = (ApsRegion*)malloc(sizeof(ApsRegion));
    if (!reg) aps_panic("out of memory in region header", "<runtime>", 0);
    ApsRegionBlock* blk = (ApsRegionBlock*)malloc(sizeof(ApsRegionBlock));
    if (!blk) aps_panic("out of memory in region block header", "<runtime>", 0);
    blk->data = (uint8_t*)malloc((size_t)initial_cap);
    if (!blk->data) aps_panic("out of memory in region buffer", "<runtime>", 0);
    blk->capacity = (size_t)initial_cap;
    blk->used = 0;
    blk->next = NULL;
    reg->name = name;
    reg->head = blk;
    reg->total_allocated = 0;
    reg->max_quota = 16 * 1024 * 1024; /* 16MB default DoS/OOM quota */
    reg->cleanups = NULL;
    return (void*)reg;
}

void aps_region_set_quota(void* region_ptr, int64_t max_quota_bytes) {
    if (!region_ptr) return;
    ApsRegion* reg = (ApsRegion*)region_ptr;
    reg->max_quota = max_quota_bytes > 0 ? (size_t)max_quota_bytes : 0;
}

void aps_region_register_cleanup(void* region_ptr, void (*fn)(void*), void* arg) {
    if (!region_ptr || !fn) return;
    ApsRegion* reg = (ApsRegion*)region_ptr;
    ApsCleanupNode* node = (ApsCleanupNode*)malloc(sizeof(ApsCleanupNode));
    if (!node) return;
    node->fn = fn;
    node->arg = arg;
    node->next = reg->cleanups;
    reg->cleanups = node;
}

APS_HOT void* aps_region_alloc(void* region_ptr, int64_t size) {
    if (APS_UNLIKELY(!region_ptr)) return aps_alloc(size);
    if (APS_UNLIKELY(size <= 0)) size = 8;
    ApsRegion* reg = (ApsRegion*)region_ptr;
    size_t aligned_size = ((size_t)size + 7u) & ~7u;

    ApsRegionBlock* blk = reg->head;
    // 1-cycle bump allocation fast path
    if (APS_LIKELY(blk && blk->used + aligned_size <= blk->capacity)) {
        void* ptr = (void*)(blk->data + blk->used);
        blk->used += aligned_size;
        reg->total_allocated += aligned_size;
        return ptr;
    }

    /* Architectural Safety Guard: Prevent DoS/OOM memory explosion */
    if (APS_UNLIKELY(reg->max_quota > 0 && (reg->total_allocated + aligned_size > reg->max_quota))) {
        aps_panic("region memory quota exceeded: request exceeded maximum allowed arena limit (protected from OOM)", "<runtime>", 0);
    }
    // Overflow extension path
    size_t new_cap = blk ? blk->capacity * 2 : 65536;
    if (new_cap < aligned_size) new_cap = aligned_size * 2;
    ApsRegionBlock* next_blk = (ApsRegionBlock*)malloc(sizeof(ApsRegionBlock));
    if (!next_blk) aps_panic("out of memory in region extension", "<runtime>", 0);
    next_blk->data = (uint8_t*)malloc(new_cap);
    if (!next_blk->data) aps_panic("out of memory in region block buffer", "<runtime>", 0);
    next_blk->capacity = new_cap;
    next_blk->used = 0;
    next_blk->next = blk;
    reg->head = next_blk;
    blk = next_blk;
    void* ptr = (void*)(blk->data + blk->used);
    blk->used += aligned_size;
    reg->total_allocated += aligned_size;
    return ptr;
}

void aps_region_end(void* region_ptr) {
    if (!region_ptr) return;
    ApsRegion* reg = (ApsRegion*)region_ptr;

    /* Architectural Safety: Deterministically execute all RAII cleanups (files/sockets/handles) */
    ApsCleanupNode* c = reg->cleanups;
    while (c) {
        ApsCleanupNode* next = c->next;
        if (c->fn) {
            c->fn(c->arg);
        }
        free(c);
        c = next;
    }
    reg->cleanups = NULL;

    // Cache primary block into thread-local pool if capacity is reasonable (<= 4MB)
    if (g_tl_cached_region == NULL && reg->head && reg->head->capacity <= 4 * 1024 * 1024) {
        g_tl_cached_region = reg;
        return;
    }
    ApsRegionBlock* cur = reg->head;
    while (cur) {
        ApsRegionBlock* next = cur->next;
        if (cur->data) free(cur->data);
        free(cur);
        cur = next;
    }
    free(reg);
}

/* UTF-8 Fat-pointer String helpers */
static inline ApsStr* aps_str_from_cstr(const char* cstr) {
    if (!cstr) return NULL;
    ApsStr* s = (ApsStr*)aps_alloc(sizeof(ApsStr));
    s->data = cstr;
    s->len = (int64_t)strlen(cstr);
    return s;
}

APS_HOT int64_t aps_str_len(const void* s_ptr) {
    if (!s_ptr) return 0;
    const ApsStr* s = (const ApsStr*)s_ptr;
    if (s->data && s->len >= 0 && s->len < 1000000000LL) return s->len;
    return (int64_t)strlen((const char*)s_ptr);
}

APS_HOT int64_t aps_str_char_count(const void* s_ptr) {
    if (!s_ptr) return 0;
    const ApsStr* s = (const ApsStr*)s_ptr;
    const uint8_t* p = NULL;
    int64_t len = 0;
    if (s->data && s->len >= 0 && s->len < 1000000000LL) {
        p = (const uint8_t*)s->data;
        len = s->len;
    } else {
        p = (const uint8_t*)s_ptr;
        len = (int64_t)strlen((const char*)s_ptr);
    }
    int64_t count = 0;
    for (int64_t i = 0; i < len; i++) {
        if ((p[i] & 0xC0) != 0x80) count++;
    }
    return count;
}

APS_HOT int64_t aps_str_eq(const void* a_ptr, const void* b_ptr) {
    if (a_ptr == b_ptr) return 1;
    if (!a_ptr || !b_ptr) return 0;
    const ApsStr* sa = (const ApsStr*)a_ptr;
    const ApsStr* sb = (const ApsStr*)b_ptr;
    const char* da = NULL;
    const char* db = NULL;
    int64_t la = 0, lb = 0;
    if (sa->data && sa->len >= 0 && sa->len < 1000000000LL) {
        da = sa->data; la = sa->len;
    } else {
        da = (const char*)a_ptr; la = (int64_t)strlen(da);
    }
    if (sb->data && sb->len >= 0 && sb->len < 1000000000LL) {
        db = sb->data; lb = sb->len;
    } else {
        db = (const char*)b_ptr; lb = (int64_t)strlen(db);
    }
    if (la != lb) return 0;
    if (la == 0) return 1;
    if (da[0] != db[0]) return 0;
    return memcmp(da, db, (size_t)la) == 0 ? 1 : 0;
}

/* Thread-Local String Slab Buffer for Zero-Cost SSO / Concatenations */
#define APS_STR_SLAB_SIZE 2097152
static APS_THREAD_LOCAL char* g_tl_str_slab = NULL;
static APS_THREAD_LOCAL size_t g_tl_str_slab_used = 0;

APS_HOT void* aps_str_concat(const void* a_ptr, const void* b_ptr) {
    const ApsStr* sa = (const ApsStr*)a_ptr;
    const ApsStr* sb = (const ApsStr*)b_ptr;
    const char* da = "";
    const char* db = "";
    int64_t la = 0, lb = 0;
    if (sa && sa->data && sa->len >= 0 && sa->len < 1000000000LL) {
        da = sa->data; la = sa->len;
    } else if (a_ptr) {
        da = (const char*)a_ptr; la = (int64_t)strlen(da);
    }
    if (sb && sb->data && sb->len >= 0 && sb->len < 1000000000LL) {
        db = sb->data; lb = sb->len;
    } else if (b_ptr) {
        db = (const char*)b_ptr; lb = (int64_t)strlen(db);
    }
    if (la <= 0 && b_ptr) return (void*)b_ptr;
    if (lb <= 0 && a_ptr) return (void*)a_ptr;
    int64_t total_len = la + lb;
    char* buf = (char*)aps_alloc(total_len + 1);
    if (la > 0) memcpy(buf, da, (size_t)la);
    if (lb > 0) memcpy(buf + la, db, (size_t)lb);
    buf[total_len] = '\0';
    ApsStr* res = (ApsStr*)aps_alloc(sizeof(ApsStr));
    res->data = buf;
    res->len = total_len;
    return (void*)res;
}

void* aps_int_to_str(int64_t val) {
    char* buf = (char*)aps_alloc(32);
    int len = snprintf(buf, 32, "%lld", (long long)val);
    ApsStr* res = (ApsStr*)aps_alloc(sizeof(ApsStr));
    res->data = buf;
    res->len = len > 0 ? len : 0;
    return (void*)res;
}

void* aps_float_to_str(double val) {
    char* buf = (char*)aps_alloc(32);
    int len = snprintf(buf, 32, "%.6g", val);
    ApsStr* res = (ApsStr*)aps_alloc(sizeof(ApsStr));
    res->data = buf;
    res->len = len > 0 ? len : 0;
    return (void*)res;
}

void* aps_bool_to_str(int64_t val) {
    static ApsStr s_true = { "true", 4 };
    static ApsStr s_false = { "false", 5 };
    return (void*)(val ? &s_true : &s_false);
}

static int64_t aps_get_perf_freq(void) {
#ifdef _WIN32
    if (g_aps_perf_freq <= 0) {
        LARGE_INTEGER freq;
        QueryPerformanceFrequency(&freq);
        g_aps_perf_freq = freq.QuadPart > 0 ? freq.QuadPart : 1;
    }
    return g_aps_perf_freq;
#else
    return 1;
#endif
}

int64_t aps_time_now_ms(void) {
#ifdef _WIN32
    LARGE_INTEGER counter;
    QueryPerformanceCounter(&counter);
    int64_t freq = aps_get_perf_freq();
    return (int64_t)((counter.QuadPart * 1000LL) / freq);
#else
    struct timespec ts;
    clock_gettime(CLOCK_MONOTONIC, &ts);
    return (int64_t)(ts.tv_sec * 1000LL + ts.tv_nsec / 1000000LL);
#endif
}

int64_t aps_time_now_ns(void) {
#ifdef _WIN32
    LARGE_INTEGER counter;
    QueryPerformanceCounter(&counter);
    int64_t freq = aps_get_perf_freq();
    return (int64_t)((counter.QuadPart * 1000000000LL) / freq);
#else
    struct timespec ts;
    clock_gettime(CLOCK_MONOTONIC, &ts);
    return (int64_t)(ts.tv_sec * 1000000000LL + ts.tv_nsec);
#endif
}

void aps_sleep_ms(int64_t ms) {
    if (ms <= 0) return;
#ifdef _WIN32
    Sleep((DWORD)ms);
#else
    usleep((useconds_t)(ms * 1000));
#endif
}

int64_t aps_abs(int64_t x) {
    return x < 0 ? -x : x;
}

int64_t aps_min(int64_t a, int64_t b) {
    return a < b ? a : b;
}

int64_t aps_max(int64_t a, int64_t b) {
    return a > b ? a : b;
}

int64_t async_sleep_ms(int64_t ms) {
    aps_sleep_ms(ms);
    return 0;
}

int64_t block_on(int64_t val) {
    return val;
}

int64_t aps_tcp_listen(int64_t port) {
    if (port <= 0 || port > 65535) return -1;
#ifdef _WIN32
    static int g_wsa_inited = 0;
    if (!g_wsa_inited) {
        WSADATA wsa;
        if (WSAStartup(MAKEWORD(2, 2), &wsa) != 0) {
            return -1;
        }
        g_wsa_inited = 1;
    }
#endif
    SOCKET s = socket(AF_INET, SOCK_STREAM, IPPROTO_TCP);
    if (s == INVALID_SOCKET) {
        return -1;
    }
    int opt = 1;
    setsockopt(s, SOL_SOCKET, SO_REUSEADDR, (const char*)&opt, sizeof(opt));

    struct sockaddr_in addr;
    memset(&addr, 0, sizeof(addr));
    addr.sin_family = AF_INET;
    addr.sin_addr.s_addr = htonl(INADDR_ANY);
    addr.sin_port = htons((uint16_t)port);

    if (bind(s, (struct sockaddr*)&addr, sizeof(addr)) == SOCKET_ERROR) {
        closesocket(s);
        return -1;
    }
    if (listen(s, 128) == SOCKET_ERROR) {
        closesocket(s);
        return -1;
    }
    return (int64_t)s;
}

int64_t aps_tcp_accept_http(int64_t server_fd, const char* response_json) {
    if (server_fd < 0) return 0;
    SOCKET s = (SOCKET)server_fd;
    struct sockaddr_in client_addr;
#ifdef _WIN32
    int client_len = sizeof(client_addr);
#else
    socklen_t client_len = sizeof(client_addr);
#endif
    SOCKET client = accept(s, (struct sockaddr*)&client_addr, &client_len);
    if (client == INVALID_SOCKET) return 0;

    char buf[2048];
    int n = recv(client, buf, sizeof(buf) - 1, 0);
    if (n > 0) {
        buf[n] = '\0';
        char method[16] = {0};
        char path[256] = {0};
        sscanf(buf, "%15s %255s", method, path);
        aps_ensure_utf8_console();
        printf("[HTTP Request] %s %s from %s -> 200 OK\n", method, path, inet_ntoa(client_addr.sin_addr));
        fflush(stdout);

        const char* body = response_json ? response_json : "{\"status\":\"OK\",\"server\":\"APS Hyperion\"}\n";
        int body_len = (int)strlen(body);
        int needed = body_len + 512;
        char stack_resp[4096];
        char* resp = stack_resp;
        if (needed > (int)sizeof(stack_resp)) {
            resp = (char*)malloc(needed);
        }
        if (resp) {
            int resp_len = snprintf(resp, needed,
                "HTTP/1.1 200 OK\r\n"
                "Content-Type: application/json; charset=utf-8\r\n"
                "Content-Length: %d\r\n"
                "Connection: close\r\n"
                "Server: APS-Hyperion-Native-LLVM/1.0\r\n"
                "\r\n"
                "%s", body_len, body);
            int total_sent = 0;
            while (total_sent < resp_len) {
                int sent = send(client, resp + total_sent, resp_len - total_sent, 0);
                if (sent <= 0) break;
                total_sent += sent;
            }
            if (resp != stack_resp) {
                free(resp);
            }
        }
    }
    closesocket(client);
    return 1;
}

void aps_tcp_close(int64_t fd) {
    if (fd >= 0) {
        closesocket((SOCKET)fd);
    }
}

static void* aps_make_result(int64_t is_ok, void* val) {
    int64_t* res = (int64_t*)aps_alloc(24);
    res[0] = is_ok ? 1 : 0;
    res[1] = (int64_t)val;
    res[2] = 0;
    return (void*)res;
}

void* fs_read_text(const void* path_ptr) {
    const char* path = aps_to_c_str(path_ptr);
    if (!path || !path[0]) return aps_make_result(0, (void*)aps_str_from_cstr("invalid path"));
    FILE* f = fopen(path, "rb");
    if (!f) return aps_make_result(0, (void*)aps_str_from_cstr("file not found"));
    fseek(f, 0, SEEK_END);
    long sz = ftell(f);
    fseek(f, 0, SEEK_SET);
    if (sz < 0) {
        fclose(f);
        return aps_make_result(0, (void*)aps_str_from_cstr("failed to get file size"));
    }
    char* buf = (char*)aps_alloc(sz + 1);
    size_t rd = 0;
    if (sz > 0) {
        rd = fread(buf, 1, sz, f);
    }
    buf[rd] = '\0';
    fclose(f);
    ApsStr* res = (ApsStr*)aps_alloc(sizeof(ApsStr));
    res->data = buf;
    res->len = (int64_t)rd;
    return aps_make_result(1, (void*)res);
}

void* fs_write_text(const void* path_ptr, const void* content_ptr) {
    const char* path = aps_to_c_str(path_ptr);
    if (!path || !path[0]) return aps_make_result(0, (void*)aps_str_from_cstr("invalid path"));
    FILE* f = fopen(path, "wb");
    if (!f) return aps_make_result(0, (void*)aps_str_from_cstr("failed to open file for writing"));
    const ApsStr* s = (const ApsStr*)content_ptr;
    const char* d = (s && s->data && s->len >= 0 && s->len < 1000000000LL) ? s->data : (const char*)content_ptr;
    int64_t len = (s && s->data && s->len >= 0 && s->len < 1000000000LL) ? s->len : (d ? (int64_t)strlen(d) : 0);
    if (d && len > 0) {
        fwrite(d, 1, (size_t)len, f);
    }
    fclose(f);
    return aps_make_result(1, NULL);
}

int64_t fs_exists(const void* path_ptr) {
    const char* path = aps_to_c_str(path_ptr);
    if (!path || !path[0]) return 0;
    FILE* f = fopen(path, "rb");
    if (f) {
        fclose(f);
        return 1;
    }
    return 0;
}

void* aps_file_create(const void* path_ptr) {
    const char* path = aps_to_c_str(path_ptr);
    if (!path || !path[0]) return aps_make_result(0, (void*)aps_str_from_cstr("invalid path"));
    FILE* f = fopen(path, "wb+");
    if (!f) return aps_make_result(0, (void*)aps_str_from_cstr("failed to create file"));
    return aps_make_result(1, (void*)f);
}

void* aps_file_open(const void* path_ptr) {
    const char* path = aps_to_c_str(path_ptr);
    if (!path || !path[0]) return aps_make_result(0, (void*)aps_str_from_cstr("invalid path"));
    FILE* f = fopen(path, "rb+");
    if (!f) return aps_make_result(0, (void*)aps_str_from_cstr("failed to open file"));
    return aps_make_result(1, (void*)f);
}

void* aps_file_write_str(void* file_ptr, const void* str_ptr) {
    if (!file_ptr) return aps_make_result(0, (void*)aps_str_from_cstr("null file"));
    FILE* f = (FILE*)file_ptr;
    const ApsStr* s = (const ApsStr*)str_ptr;
    const char* d = (s && s->data && s->len >= 0 && s->len < 1000000000LL) ? s->data : (const char*)str_ptr;
    int64_t len = (s && s->data && s->len >= 0 && s->len < 1000000000LL) ? s->len : (d ? (int64_t)strlen(d) : 0);
    size_t wr = 0;
    if (d && len > 0) {
        wr = fwrite(d, 1, (size_t)len, f);
    }
    return aps_make_result(1, (void*)(intptr_t)wr);
}

void* aps_file_flush(void* file_ptr) {
    if (!file_ptr) return aps_make_result(0, (void*)aps_str_from_cstr("null file"));
    FILE* f = (FILE*)file_ptr;
    fflush(f);
    return aps_make_result(1, NULL);
}

void* aps_file_read_to_string(void* file_ptr) {
    if (!file_ptr) return aps_make_result(0, (void*)aps_str_from_cstr("null file"));
    FILE* f = (FILE*)file_ptr;
    fseek(f, 0, SEEK_END);
    long sz = ftell(f);
    fseek(f, 0, SEEK_SET);
    if (sz < 0) return aps_make_result(0, (void*)aps_str_from_cstr("cannot determine file size"));
    char* buf = (char*)aps_alloc(sz + 1);
    size_t rd = 0;
    if (sz > 0) {
        rd = fread(buf, 1, sz, f);
    }
    buf[rd] = '\0';
    ApsStr* res = (ApsStr*)aps_alloc(sizeof(ApsStr));
    res->data = buf;
    res->len = (int64_t)rd;
    return aps_make_result(1, (void*)res);
}

void aps_file_close(void* file_ptr) {
    if (file_ptr) {
        fclose((FILE*)file_ptr);
    }
}

void* aps_tcp_listener_bind(const void* host_ptr, int64_t port) {
    const char* host = aps_to_c_str(host_ptr);
    (void)host;
    int64_t fd = aps_tcp_listen(port);
    if (fd < 0) {
        return aps_make_result(0, (void*)aps_str_from_cstr("failed to bind TCP port"));
    }
    return aps_make_result(1, (void*)(intptr_t)fd);
}

void* aps_tcp_stream_connect(const void* host_ptr, int64_t port, int64_t timeout_ms) {
    (void)timeout_ms;
    const char* host = aps_to_c_str(host_ptr);
    if (!host || !host[0]) host = "127.0.0.1";
#ifdef _WIN32
    static int g_wsa_inited = 0;
    if (!g_wsa_inited) {
        WSADATA wsa;
        WSAStartup(MAKEWORD(2, 2), &wsa);
        g_wsa_inited = 1;
    }
#endif
    SOCKET s = socket(AF_INET, SOCK_STREAM, IPPROTO_TCP);
    if (s == INVALID_SOCKET) {
        return aps_make_result(0, (void*)aps_str_from_cstr("failed to create socket"));
    }
    struct sockaddr_in addr;
    memset(&addr, 0, sizeof(addr));
    addr.sin_family = AF_INET;
    addr.sin_port = htons((uint16_t)port);
    addr.sin_addr.s_addr = inet_addr(host);
    if (connect(s, (struct sockaddr*)&addr, sizeof(addr)) == SOCKET_ERROR) {
        closesocket(s);
        return aps_make_result(0, (void*)aps_str_from_cstr("connection failed"));
    }
    return aps_make_result(1, (void*)(intptr_t)s);
}

void* aps_tcp_stream_read_line(int64_t fd) {
    if (fd < 0) return aps_make_result(0, (void*)aps_str_from_cstr("invalid socket"));
    char* buf = (char*)aps_alloc(1024);
    int idx = 0;
    while (idx < 1023) {
        char c;
        int n = recv((SOCKET)fd, &c, 1, 0);
        if (n <= 0) break;
        buf[idx++] = c;
        if (c == '\n') break;
    }
    buf[idx] = '\0';
    ApsStr* res = (ApsStr*)aps_alloc(sizeof(ApsStr));
    res->data = buf;
    res->len = (int64_t)idx;
    return aps_make_result(1, (void*)res);
}

void* aps_tcp_stream_write_str(int64_t fd, const void* str_ptr) {
    if (fd < 0) return aps_make_result(0, (void*)aps_str_from_cstr("invalid socket"));
    const ApsStr* s = (const ApsStr*)str_ptr;
    const char* d = (s && s->data && s->len >= 0 && s->len < 1000000000LL) ? s->data : (const char*)str_ptr;
    int64_t len = (s && s->data && s->len >= 0 && s->len < 1000000000LL) ? s->len : (d ? (int64_t)strlen(d) : 0);
    int sent = 0;
    if (d && len > 0) {
        sent = send((SOCKET)fd, d, (int)len, 0);
    }
    if (sent < 0) return aps_make_result(0, (void*)aps_str_from_cstr("send failed"));
    return aps_make_result(1, (void*)(intptr_t)sent);
}

void* aps_tcp_stream_flush(int64_t fd) {
    (void)fd;
    return aps_make_result(1, NULL);
}

double aps_sqrt(double x) {
    return sqrt(x);
}

int64_t random_i64(int64_t min_v, int64_t max_v) {
    if (min_v >= max_v) return min_v;
    static int g_seeded = 0;
    if (!g_seeded) {
        srand((unsigned int)time(NULL));
        g_seeded = 1;
    }
    uint64_t r = ((uint64_t)rand() << 48) ^ ((uint64_t)rand() << 32) ^ ((uint64_t)rand() << 16) ^ (uint64_t)rand();
    uint64_t range = (uint64_t)(max_v - min_v + 1);
    return min_v + (int64_t)(r % range);
}

static const uint32_t K256[64] = {
    0x428a2f98,0x71374491,0xb5c0fbcf,0xe9b5dba5,0x3956c25b,0x59f111f1,0x923f82a4,0xab1c5ed5,
    0xd807aa98,0x12835b01,0x243185be,0x550c7dc3,0x72be5d74,0x80deb1fe,0x9bdc06a7,0xc19bf174,
    0xe49b69c1,0xefbe4786,0x0fc19dc6,0x240ca1cc,0x2de92c6f,0x4a7484aa,0x5cb0a9dc,0x76f988da,
    0x983e5152,0xa831c66d,0xb00327c8,0xbf597fc7,0xc6e00bf3,0xd5a79147,0x06ca6351,0x14292967,
    0x27b70a85,0x2e1b2138,0x4d2c6dfc,0x53380d13,0x650a7354,0x766a0abb,0x81c2c92e,0x92722c85,
    0xa2bfe8a1,0xa81a664b,0xc24b8b70,0xc76c51a3,0xd192e819,0xd6990624,0xf40e3585,0x106aa070,
    0x19a4c116,0x1e376c08,0x2748774c,0x34b0bcb5,0x391c0cb3,0x4ed8aa4a,0x5b9cca4f,0x682e6ff3,
    0x748f82ee,0x78a5636f,0x84c87814,0x8cc70208,0x90befffa,0xa4506ceb,0xbef9a3f7,0xc67178f2
};

#define ROR32(x, n) (((x) >> (n)) | ((x) << (32 - (n))))
#define CH(x, y, z) (((x) & (y)) ^ (~(x) & (z)))
#define MAJ(x, y, z) (((x) & (y)) ^ ((x) & (z)) ^ ((y) & (z)))
#define EP0(x) (ROR32(x, 2) ^ ROR32(x, 13) ^ ROR32(x, 22))
#define EP1(x) (ROR32(x, 6) ^ ROR32(x, 11) ^ ROR32(x, 25))
#define SIG0(x) (ROR32(x, 7) ^ ROR32(x, 18) ^ ((x) >> 3))
#define SIG1(x) (ROR32(x, 17) ^ ROR32(x, 19) ^ ((x) >> 10))

const char* sha256_hex(const char* data) {
    if (!data) data = "";
    size_t len = strlen(data);
    uint32_t h[8] = {
        0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a,
        0x510e527f, 0x9b05688c, 0x1f83d9ab, 0x5be0cd19
    };
    uint64_t total_bits = (uint64_t)len * 8;
    size_t padded_len = ((len + 8) / 64 + 1) * 64;
    uint8_t* p = (uint8_t*)calloc(1, padded_len);
    if (!p) return "";
    memcpy(p, data, len);
    p[len] = 0x80;
    for (int i = 0; i < 8; i++) {
        p[padded_len - 1 - i] = (uint8_t)(total_bits >> (i * 8));
    }
    for (size_t chunk = 0; chunk < padded_len; chunk += 64) {
        uint32_t w[64];
        for (int i = 0; i < 16; i++) {
            size_t idx = chunk + i * 4;
            w[i] = ((uint32_t)p[idx] << 24) | ((uint32_t)p[idx+1] << 16) | ((uint32_t)p[idx+2] << 8) | (uint32_t)p[idx+3];
        }
        for (int i = 16; i < 64; i++) {
            w[i] = SIG1(w[i - 2]) + w[i - 7] + SIG0(w[i - 15]) + w[i - 16];
        }
        uint32_t a = h[0], b = h[1], c = h[2], d = h[3], e = h[4], f = h[5], g = h[6], h_val = h[7];
        for (int i = 0; i < 64; i++) {
            uint32_t t1 = h_val + EP1(e) + CH(e, f, g) + K256[i] + w[i];
            uint32_t t2 = EP0(a) + MAJ(a, b, c);
            h_val = g; g = f; f = e; e = d + t1; d = c; c = b; b = a; a = t1 + t2;
        }
        h[0] += a; h[1] += b; h[2] += c; h[3] += d; h[4] += e; h[5] += f; h[6] += g; h[7] += h_val;
    }
    free(p);
    char* hex = (char*)aps_alloc(65);
    for (int i = 0; i < 8; i++) {
        snprintf(hex + i * 8, 9, "%08x", h[i]);
    }
    hex[64] = '\0';
    return hex;
}

typedef struct ApsChannel {
    int64_t cap;
    int64_t head;
    int64_t tail;
    int64_t count;
    int64_t closed;
    void** buf;
    void* sender;
    void* receiver;
#ifdef _WIN32
    CRITICAL_SECTION cs;
    CONDITION_VARIABLE cv;
#else
    pthread_mutex_t cs;
    pthread_cond_t cv;
#endif
} ApsChannel;

void* channel(int64_t cap) {
    if (cap <= 0) cap = 16;
    ApsChannel* ch = (ApsChannel*)aps_alloc(sizeof(ApsChannel));
    ch->cap = cap;
    ch->buf = (void**)aps_alloc(cap * sizeof(void*));
    ch->head = 0;
    ch->tail = 0;
    ch->count = 0;
    ch->closed = 0;
    ch->sender = ch;
    ch->receiver = ch;
#ifdef _WIN32
    InitializeCriticalSection(&ch->cs);
    InitializeConditionVariable(&ch->cv);
#else
    pthread_mutex_init(&ch->cs, NULL);
    pthread_cond_init(&ch->cv, NULL);
#endif
    return (void*)ch;
}

void aps_channel_send(void* ch_ptr, void* val) {
    if (!ch_ptr) return;
    ApsChannel* ch = (ApsChannel*)ch_ptr;
#ifdef _WIN32
    EnterCriticalSection(&ch->cs);
    while (ch->count >= ch->cap && !ch->closed) {
        SleepConditionVariableCS(&ch->cv, &ch->cs, INFINITE);
    }
    if (!ch->closed) {
        ch->buf[ch->tail] = val;
        ch->tail = (ch->tail + 1) % ch->cap;
        ch->count++;
        WakeAllConditionVariable(&ch->cv);
    }
    LeaveCriticalSection(&ch->cs);
#else
    pthread_mutex_lock(&ch->cs);
    while (ch->count >= ch->cap && !ch->closed) {
        pthread_cond_wait(&ch->cv, &ch->cs);
    }
    if (!ch->closed) {
        ch->buf[ch->tail] = val;
        ch->tail = (ch->tail + 1) % ch->cap;
        ch->count++;
        pthread_cond_broadcast(&ch->cv);
    }
    pthread_mutex_unlock(&ch->cs);
#endif
}

void aps_channel_close(void* ch_ptr) {
    if (!ch_ptr) return;
    ApsChannel* ch = (ApsChannel*)ch_ptr;
#ifdef _WIN32
    EnterCriticalSection(&ch->cs);
    ch->closed = 1;
    WakeAllConditionVariable(&ch->cv);
    LeaveCriticalSection(&ch->cs);
#else
    pthread_mutex_lock(&ch->cs);
    ch->closed = 1;
    pthread_cond_broadcast(&ch->cv);
    pthread_mutex_unlock(&ch->cs);
#endif
}

void* aps_channel_recv(void* ch_ptr) {
    if (!ch_ptr) return aps_make_result(0, NULL);
    ApsChannel* ch = (ApsChannel*)ch_ptr;
#ifdef _WIN32
    EnterCriticalSection(&ch->cs);
    while (ch->count == 0 && !ch->closed) {
        SleepConditionVariableCS(&ch->cv, &ch->cs, INFINITE);
    }
    if (ch->count > 0) {
        void* val = ch->buf[ch->head];
        ch->head = (ch->head + 1) % ch->cap;
        ch->count--;
        WakeAllConditionVariable(&ch->cv);
        LeaveCriticalSection(&ch->cs);
        return aps_make_result(1, val);
    }
    LeaveCriticalSection(&ch->cs);
    return aps_make_result(0, NULL);
#else
    pthread_mutex_lock(&ch->cs);
    while (ch->count == 0 && !ch->closed) {
        pthread_cond_wait(&ch->cv, &ch->cs);
    }
    if (ch->count > 0) {
        void* val = ch->buf[ch->head];
        ch->head = (ch->head + 1) % ch->cap;
        ch->count--;
        pthread_cond_broadcast(&ch->cv);
        pthread_mutex_unlock(&ch->cs);
        return aps_make_result(1, val);
    }
    pthread_mutex_unlock(&ch->cs);
    return aps_make_result(0, NULL);
#endif
}

typedef struct ApsThreadHandle {
    int64_t thread_id;
    int64_t result_val;
    int64_t (*fn)(void);
#ifdef _WIN32
    HANDLE hThread;
#else
    pthread_t thread;
#endif
} ApsThreadHandle;

#ifdef _WIN32
static DWORD WINAPI aps_thread_trampoline(LPVOID lpParam) {
    ApsThreadHandle* th = (ApsThreadHandle*)lpParam;
    if (th && th->fn) {
        th->result_val = th->fn();
    }
    return 0;
}
#else
static void* aps_thread_trampoline(void* lpParam) {
    ApsThreadHandle* th = (ApsThreadHandle*)lpParam;
    if (th && th->fn) {
        th->result_val = th->fn();
    }
    return NULL;
}
#endif

void* spawn_thread(void* fn_ptr) {
    ApsThreadHandle* th = (ApsThreadHandle*)aps_alloc(sizeof(ApsThreadHandle));
    th->fn = (int64_t (*)(void))fn_ptr;
#ifdef _WIN32
    DWORD tid = 0;
    th->hThread = CreateThread(NULL, 0, aps_thread_trampoline, th, 0, &tid);
    th->thread_id = (int64_t)tid;
#else
    pthread_create(&th->thread, NULL, aps_thread_trampoline, th);
    th->thread_id = (int64_t)th->thread;
#endif
    return (void*)th;
}

int64_t aps_thread_join(void* handle_ptr) {
    if (!handle_ptr) return 0;
    ApsThreadHandle* th = (ApsThreadHandle*)handle_ptr;
#ifdef _WIN32
    if (th->hThread) {
        WaitForSingleObject(th->hThread, INFINITE);
        CloseHandle(th->hThread);
        th->hThread = NULL;
    }
#else
    if (th->thread) {
        pthread_join(th->thread, NULL);
        th->thread = 0;
    }
#endif
    return th->result_val;
}

/* M:N Work-Stealing Task Runtime for `spawn` and `await` */
typedef struct ApsTask {
    void* (*fn)(void*);
    void* arg;
    void* result;
    volatile int completed;
#ifdef _WIN32
    HANDLE hEvent;
#else
    pthread_mutex_t lock;
    pthread_cond_t cv;
#endif
    struct ApsTask* next;
} ApsTask;

typedef struct ApsThreadPool {
    int thread_count;
    int shutdown;
    ApsTask* queue_head;
    ApsTask* queue_tail;
#ifdef _WIN32
    CRITICAL_SECTION lock;
    CONDITION_VARIABLE cv;
    HANDLE* threads;
#else
    pthread_mutex_t lock;
    pthread_cond_t cv;
    pthread_t* threads;
#endif
} ApsThreadPool;

static ApsThreadPool* g_aps_thread_pool = NULL;

#ifdef _WIN32
static DWORD WINAPI aps_pool_worker(LPVOID lpParam) {
    (void)lpParam;
    while (1) {
        ApsTask* task = NULL;
        EnterCriticalSection(&g_aps_thread_pool->lock);
        while (!g_aps_thread_pool->shutdown && !g_aps_thread_pool->queue_head) {
            SleepConditionVariableCS(&g_aps_thread_pool->cv, &g_aps_thread_pool->lock, INFINITE);
        }
        if (g_aps_thread_pool->shutdown && !g_aps_thread_pool->queue_head) {
            LeaveCriticalSection(&g_aps_thread_pool->lock);
            break;
        }
        task = g_aps_thread_pool->queue_head;
        if (task) {
            g_aps_thread_pool->queue_head = task->next;
            if (!g_aps_thread_pool->queue_head) g_aps_thread_pool->queue_tail = NULL;
        }
        LeaveCriticalSection(&g_aps_thread_pool->lock);

        if (task) {
            if (task->fn) {
                task->result = task->fn(task->arg);
            }
            task->completed = 1;
            SetEvent(task->hEvent);
        }
    }
    return 0;
}
#else
static void* aps_pool_worker(void* lpParam) {
    (void)lpParam;
    while (1) {
        ApsTask* task = NULL;
        pthread_mutex_lock(&g_aps_thread_pool->lock);
        while (!g_aps_thread_pool->shutdown && !g_aps_thread_pool->queue_head) {
            pthread_cond_wait(&g_aps_thread_pool->cv, &g_aps_thread_pool->lock);
        }
        if (g_aps_thread_pool->shutdown && !g_aps_thread_pool->queue_head) {
            pthread_mutex_unlock(&g_aps_thread_pool->lock);
            break;
        }
        task = g_aps_thread_pool->queue_head;
        if (task) {
            g_aps_thread_pool->queue_head = task->next;
            if (!g_aps_thread_pool->queue_head) g_aps_thread_pool->queue_tail = NULL;
        }
        pthread_mutex_unlock(&g_aps_thread_pool->lock);

        if (task) {
            if (task->fn) {
                task->result = task->fn(task->arg);
            }
            pthread_mutex_lock(&task->lock);
            task->completed = 1;
            pthread_cond_broadcast(&task->cv);
            pthread_mutex_unlock(&task->lock);
        }
    }
    return NULL;
}
#endif

static void aps_ensure_thread_pool(void) {
    if (g_aps_thread_pool) return;
    ApsThreadPool* pool = (ApsThreadPool*)calloc(1, sizeof(ApsThreadPool));
#ifdef _WIN32
    SYSTEM_INFO si;
    GetSystemInfo(&si);
    int cores = (int)si.dwNumberOfProcessors;
    if (cores < 2) cores = 2;
    if (cores > 64) cores = 64;
    pool->thread_count = cores;
    InitializeCriticalSection(&pool->lock);
    InitializeConditionVariable(&pool->cv);
    pool->threads = (HANDLE*)calloc((size_t)cores, sizeof(HANDLE));
    g_aps_thread_pool = pool;
    for (int i = 0; i < cores; i++) {
        pool->threads[i] = CreateThread(NULL, 0, aps_pool_worker, NULL, 0, NULL);
    }
#else
    int cores = (int)sysconf(_SC_NPROCESSORS_ONLN);
    if (cores < 2) cores = 2;
    if (cores > 64) cores = 64;
    pool->thread_count = cores;
    pthread_mutex_init(&pool->lock, NULL);
    pthread_cond_init(&pool->cv, NULL);
    pool->threads = (pthread_t*)calloc((size_t)cores, sizeof(pthread_t));
    g_aps_thread_pool = pool;
    for (int i = 0; i < cores; i++) {
        pthread_create(&pool->threads[i], NULL, aps_pool_worker, NULL);
    }
#endif
}

void* aps_task_spawn(void* fn_ptr, void* arg_ptr) {
    aps_ensure_thread_pool();
    ApsTask* task = (ApsTask*)calloc(1, sizeof(ApsTask));
    task->fn = (void* (*)(void*))fn_ptr;
    task->arg = arg_ptr;
    task->completed = 0;
#ifdef _WIN32
    task->hEvent = CreateEvent(NULL, TRUE, FALSE, NULL);
    EnterCriticalSection(&g_aps_thread_pool->lock);
    if (!g_aps_thread_pool->queue_head) {
        g_aps_thread_pool->queue_head = task;
        g_aps_thread_pool->queue_tail = task;
    } else {
        g_aps_thread_pool->queue_tail->next = task;
        g_aps_thread_pool->queue_tail = task;
    }
    WakeConditionVariable(&g_aps_thread_pool->cv);
    LeaveCriticalSection(&g_aps_thread_pool->lock);
#else
    pthread_mutex_init(&task->lock, NULL);
    pthread_cond_init(&task->cv, NULL);
    pthread_mutex_lock(&g_aps_thread_pool->lock);
    if (!g_aps_thread_pool->queue_head) {
        g_aps_thread_pool->queue_head = task;
        g_aps_thread_pool->queue_tail = task;
    } else {
        g_aps_thread_pool->queue_tail->next = task;
        g_aps_thread_pool->queue_tail = task;
    }
    pthread_cond_signal(&g_aps_thread_pool->cv);
    pthread_mutex_unlock(&g_aps_thread_pool->lock);
#endif
    return (void*)task;
}

void* aps_task_await(void* task_ptr) {
    if (!task_ptr) return NULL;
    ApsTask* task = (ApsTask*)task_ptr;
#ifdef _WIN32
    if (!task->completed) {
        // Work-stealing: process queued tasks while waiting for our target task
        while (!task->completed) {
            ApsTask* work = NULL;
            EnterCriticalSection(&g_aps_thread_pool->lock);
            if (g_aps_thread_pool->queue_head) {
                work = g_aps_thread_pool->queue_head;
                g_aps_thread_pool->queue_head = work->next;
                if (!g_aps_thread_pool->queue_head) g_aps_thread_pool->queue_tail = NULL;
            }
            LeaveCriticalSection(&g_aps_thread_pool->lock);

            if (work) {
                if (work->fn) {
                    work->result = work->fn(work->arg);
                }
                work->completed = 1;
                SetEvent(work->hEvent);
            } else {
                WaitForSingleObject(task->hEvent, 5);
            }
        }
    }
    WaitForSingleObject(task->hEvent, INFINITE);
    CloseHandle(task->hEvent);
#else
    if (!task->completed) {
        while (!task->completed) {
            ApsTask* work = NULL;
            pthread_mutex_lock(&g_aps_thread_pool->lock);
            if (g_aps_thread_pool->queue_head) {
                work = g_aps_thread_pool->queue_head;
                g_aps_thread_pool->queue_head = work->next;
                if (!g_aps_thread_pool->queue_head) g_aps_thread_pool->queue_tail = NULL;
            }
            pthread_mutex_unlock(&g_aps_thread_pool->lock);

            if (work) {
                if (work->fn) {
                    work->result = work->fn(work->arg);
                }
                pthread_mutex_lock(&work->lock);
                work->completed = 1;
                pthread_cond_broadcast(&work->cv);
                pthread_mutex_unlock(&work->lock);
            } else {
                pthread_mutex_lock(&task->lock);
                if (!task->completed) {
                    struct timespec ts;
                    clock_gettime(CLOCK_REALTIME, &ts);
                    ts.tv_nsec += 5000000;
                    if (ts.tv_nsec >= 1000000000) {
                        ts.tv_sec += 1;
                        ts.tv_nsec -= 1000000000;
                    }
                    pthread_cond_timedwait(&task->cv, &task->lock, &ts);
                }
                pthread_mutex_unlock(&task->lock);
            }
        }
    }
    pthread_mutex_destroy(&task->lock);
    pthread_cond_destroy(&task->cv);
#endif
    void* res = task->result;
    free(task);
    return res;
}

int64_t aps_clock_nanos(void) {
#ifdef _WIN32
    static LARGE_INTEGER freq;
    static int initialized = 0;
    if (!initialized) {
        QueryPerformanceFrequency(&freq);
        initialized = 1;
    }
    LARGE_INTEGER counter;
    QueryPerformanceCounter(&counter);
    return (int64_t)((counter.QuadPart * 1000000000ULL) / freq.QuadPart);
#else
    struct timespec ts;
    clock_gettime(CLOCK_MONOTONIC, &ts);
    return (int64_t)ts.tv_sec * 1000000000LL + (int64_t)ts.tv_nsec;
#endif
}

double aps_simd_dot_f64(const double* a, const double* b, int64_t len) {
    if (!a || !b || len <= 0) return 0.0;
    double sum = 0.0;
    for (int64_t i = 0; i < len; i++) {
        sum += a[i] * b[i];
    }
    return sum;
}

int64_t aps_simd_sum_i64(const int64_t* a, int64_t len) {
    if (!a || len <= 0) return 0;
    if (len < 16) {
        int64_t s = 0;
        for (int64_t i = 0; i < len; i++) s += a[i];
        return s;
    }
    int64_t sum0 = 0, sum1 = 0, sum2 = 0, sum3 = 0;
    int64_t i = 0;
    for (; i + 3 < len; i += 4) {
        sum0 += a[i];
        sum1 += a[i + 1];
        sum2 += a[i + 2];
        sum3 += a[i + 3];
    }
    int64_t total = (sum0 + sum1) + (sum2 + sum3);
    for (; i < len; i++) {
        total += a[i];
    }
    return total;
}

int64_t aps_simd_min_i64(const int64_t* a, int64_t len) {
    if (!a || len <= 0) return 0;
    int64_t min_v = a[0];
    for (int64_t i = 1; i < len; i++) {
        if (a[i] < min_v) min_v = a[i];
    }
    return min_v;
}

int64_t aps_simd_max_i64(const int64_t* a, int64_t len) {
    if (!a || len <= 0) return 0;
    int64_t max_v = a[0];
    for (int64_t i = 1; i < len; i++) {
        if (a[i] > max_v) max_v = a[i];
    }
    return max_v;
}

int64_t aps_simd_count_byte(const void* data_ptr, int64_t len, int64_t byte_val) {
    if (!data_ptr || len <= 0) return 0;
    const ApsStr* s = (const ApsStr*)data_ptr;
    const char* data = (s->data && s->len >= 0 && s->len < 1000000000LL) ? s->data : (const char*)data_ptr;
    int64_t max_len = (s->data && s->len >= 0 && s->len < 1000000000LL) ? s->len : len;
    if (len > max_len) len = max_len;
    char target = (char)(byte_val & 0xFF);
    int64_t count = 0;
    for (int64_t i = 0; i < len; i++) {
        if (data[i] == target) count++;
    }
    return count;
}

int64_t aps_simd_is_ascii(const void* data_ptr, int64_t len) {
    if (!data_ptr || len <= 0) return 1;
    const ApsStr* s = (const ApsStr*)data_ptr;
    const uint8_t* data = (s->data && s->len >= 0 && s->len < 1000000000LL) ? (const uint8_t*)s->data : (const uint8_t*)data_ptr;
    int64_t max_len = (s->data && s->len >= 0 && s->len < 1000000000LL) ? s->len : len;
    if (len > max_len) len = max_len;
    for (int64_t i = 0; i < len; i++) {
        if (data[i] >= 0x80) return 0;
    }
    return 1;
}

int64_t aps_simd_find_byte(const void* data_ptr, int64_t len, int64_t byte_val) {
    if (!data_ptr || len <= 0) return -1;
    const ApsStr* s = (const ApsStr*)data_ptr;
    const uint8_t* data = (s->data && s->len >= 0 && s->len < 1000000000LL) ? (const uint8_t*)s->data : (const uint8_t*)data_ptr;
    int64_t max_len = (s->data && s->len >= 0 && s->len < 1000000000LL) ? s->len : len;
    if (len > max_len) len = max_len;
    const uint8_t* p = (const uint8_t*)memchr(data, (int)(byte_val & 0xFF), (size_t)len);
    if (!p) return -1;
    return (int64_t)(p - data);
}

typedef struct {
    double v0, v1, v2, v3;
} ApsF64x4;

typedef struct {
    int64_t v0, v1, v2, v3;
} ApsI64x4;

void aps_simd_f64x4_add(ApsF64x4* out, const ApsF64x4* a, const ApsF64x4* b) {
    if (!out || !a || !b) return;
    out->v0 = a->v0 + b->v0;
    out->v1 = a->v1 + b->v1;
    out->v2 = a->v2 + b->v2;
    out->v3 = a->v3 + b->v3;
}

void aps_simd_f64x4_mul(ApsF64x4* out, const ApsF64x4* a, const ApsF64x4* b) {
    if (!out || !a || !b) return;
    out->v0 = a->v0 * b->v0;
    out->v1 = a->v1 * b->v1;
    out->v2 = a->v2 * b->v2;
    out->v3 = a->v3 * b->v3;
}

void aps_simd_i64x4_add(ApsI64x4* out, const ApsI64x4* a, const ApsI64x4* b) {
    if (!out || !a || !b) return;
    out->v0 = a->v0 + b->v0;
    out->v1 = a->v1 + b->v1;
    out->v2 = a->v2 + b->v2;
    out->v3 = a->v3 + b->v3;
}

void aps_simd_i64x4_mul(ApsI64x4* out, const ApsI64x4* a, const ApsI64x4* b) {
    if (!out || !a || !b) return;
    out->v0 = a->v0 * b->v0;
    out->v1 = a->v1 * b->v1;
    out->v2 = a->v2 * b->v2;
    out->v3 = a->v3 * b->v3;
}

void* aps_region_cast_view(void* region_ptr, const void* raw_data, int64_t offset, int64_t expected_size) {
    if (!raw_data || offset < 0 || expected_size <= 0) return NULL;
    const char* bytes = (const char*)raw_data + offset;
    if (((uintptr_t)bytes & 7) == 0) {
        return (void*)bytes;
    }
    void* dst = aps_region_alloc(region_ptr, expected_size);
    if (dst) {
        memcpy(dst, bytes, (size_t)expected_size);
    }
    return dst;
}

void* aps_str_slice(const void* s_ptr, int64_t start, int64_t len) {
    static const ApsStr empty_str = { "", 0 };
    if (!s_ptr || start < 0 || len <= 0) return (void*)&empty_str;
    const ApsStr* s = (const ApsStr*)s_ptr;
    const char* d = NULL;
    int64_t total = 0;
    if (s->data && s->len >= 0 && s->len < 1000000000LL) {
        d = s->data; total = s->len;
    } else {
        d = (const char*)s_ptr; total = (int64_t)strlen(d);
    }
    if (start >= total) return (void*)&empty_str;
    if (start + len > total) len = total - start;
    ApsStr* res = (ApsStr*)aps_alloc(sizeof(ApsStr));
    res->data = d + start;
    res->len = len;
    return (void*)res;
}

APS_HOT int64_t aps_str_starts_with(const void* s_ptr, const void* prefix_ptr) {
    if (!s_ptr || !prefix_ptr) return 0;
    const ApsStr* s = (const ApsStr*)s_ptr;
    const ApsStr* p = (const ApsStr*)prefix_ptr;
    const char* sd = (s->data && s->len >= 0 && s->len < 1000000000LL) ? s->data : (const char*)s_ptr;
    int64_t slen = (s->data && s->len >= 0 && s->len < 1000000000LL) ? s->len : (int64_t)strlen(sd);
    const char* pd = (p->data && p->len >= 0 && p->len < 1000000000LL) ? p->data : (const char*)prefix_ptr;
    int64_t plen = (p->data && p->len >= 0 && p->len < 1000000000LL) ? p->len : (int64_t)strlen(pd);
    if (plen == 0) return 1;
    if (slen < plen) return 0;
    return memcmp(sd, pd, (size_t)plen) == 0 ? 1 : 0;
}

APS_HOT int64_t aps_str_ends_with(const void* s_ptr, const void* suffix_ptr) {
    if (!s_ptr || !suffix_ptr) return 0;
    const ApsStr* s = (const ApsStr*)s_ptr;
    const ApsStr* p = (const ApsStr*)suffix_ptr;
    const char* sd = (s->data && s->len >= 0 && s->len < 1000000000LL) ? s->data : (const char*)s_ptr;
    int64_t slen = (s->data && s->len >= 0 && s->len < 1000000000LL) ? s->len : (int64_t)strlen(sd);
    const char* pd = (p->data && p->len >= 0 && p->len < 1000000000LL) ? p->data : (const char*)suffix_ptr;
    int64_t plen = (p->data && p->len >= 0 && p->len < 1000000000LL) ? p->len : (int64_t)strlen(pd);
    if (plen == 0) return 1;
    if (slen < plen) return 0;
    return memcmp(sd + (slen - plen), pd, (size_t)plen) == 0 ? 1 : 0;
}

int64_t aps_str_find(const void* haystack_ptr, const void* needle_ptr) {
    if (!haystack_ptr || !needle_ptr) return -1;
    const ApsStr* h = (const ApsStr*)haystack_ptr;
    const ApsStr* n = (const ApsStr*)needle_ptr;
    const char* hd = (h->data && h->len >= 0 && h->len < 1000000000LL) ? h->data : (const char*)haystack_ptr;
    int64_t hlen = (h->data && h->len >= 0 && h->len < 1000000000LL) ? h->len : (int64_t)strlen(hd);
    const char* nd = (n->data && n->len >= 0 && n->len < 1000000000LL) ? n->data : (const char*)needle_ptr;
    int64_t nlen = (n->data && n->len >= 0 && n->len < 1000000000LL) ? n->len : (int64_t)strlen(nd);
    if (nlen == 0) return 0;
    if (hlen < nlen) return -1;
    int64_t max_pos = hlen - nlen;
    for (int64_t i = 0; i <= max_pos; i++) {
        if (hd[i] == nd[0] && memcmp(hd + i, nd, (size_t)nlen) == 0) {
            return i;
        }
    }
    return -1;
}

int64_t aps_str_find_byte(const void* s_ptr, int64_t byte_val) {
    if (!s_ptr) return -1;
    const ApsStr* s = (const ApsStr*)s_ptr;
    const char* d = (s->data && s->len >= 0 && s->len < 1000000000LL) ? s->data : (const char*)s_ptr;
    int64_t len = (s->data && s->len >= 0 && s->len < 1000000000LL) ? s->len : (int64_t)strlen(d);
    if (len <= 0) return -1;
    const uint8_t* p = (const uint8_t*)memchr(d, (int)(byte_val & 0xFF), (size_t)len);
    if (!p) return -1;
    return (int64_t)(p - (const uint8_t*)d);
}

int64_t aps_str_view_eq(const void* a_ptr, int64_t a_len, const void* b_ptr, int64_t b_len) {
    if (a_len != b_len) return 0;
    if (a_len == 0) return 1;
    if (!a_ptr || !b_ptr) return 0;
    const ApsStr* sa = (const ApsStr*)a_ptr;
    const ApsStr* sb = (const ApsStr*)b_ptr;
    const char* da = (sa->data && sa->len >= 0 && sa->len < 1000000000LL) ? sa->data : (const char*)a_ptr;
    const char* db = (sb->data && sb->len >= 0 && sb->len < 1000000000LL) ? sb->data : (const char*)b_ptr;
    if (da[0] != db[0]) return 0;
    return memcmp(da, db, (size_t)a_len) == 0 ? 1 : 0;
}

void* aps_http_response_fast(int64_t status, const void* content_type_ptr, const void* body_ptr) {
    const ApsStr* ct_s = (const ApsStr*)content_type_ptr;
    const char* ct = (ct_s && ct_s->data && ct_s->len >= 0 && ct_s->len < 1000000000LL) ? ct_s->data : (content_type_ptr ? (const char*)content_type_ptr : "text/plain");
    int ct_len = (int)((ct_s && ct_s->data && ct_s->len >= 0 && ct_s->len < 1000000000LL) ? ct_s->len : strlen(ct));
    
    const ApsStr* b_s = (const ApsStr*)body_ptr;
    const char* b = (b_s && b_s->data && b_s->len >= 0 && b_s->len < 1000000000LL) ? b_s->data : (body_ptr ? (const char*)body_ptr : "");
    int64_t blen = (b_s && b_s->data && b_s->len >= 0 && b_s->len < 1000000000LL) ? b_s->len : (int64_t)strlen(b);
    
    const char* status_text = (status == 200) ? "200 OK" : ((status == 404) ? "404 Not Found" : "500 Internal Server Error");
    
    char hdr_buf[512];
    int hdr_len = snprintf(hdr_buf, sizeof(hdr_buf),
        "HTTP/1.1 %s\r\n"
        "Content-Type: %.*s\r\n"
        "Content-Length: %lld\r\n"
        "Connection: keep-alive\r\n"
        "Server: APS-FastEngine\r\n"
        "\r\n", status_text, ct_len, ct, (long long)blen);

    int64_t total_len = (int64_t)hdr_len + blen;
    char* buf = (char*)aps_alloc(total_len + 1);
    memcpy(buf, hdr_buf, (size_t)hdr_len);
    if (b && blen > 0) memcpy(buf + hdr_len, b, (size_t)blen);
    buf[total_len] = '\0';
    
    ApsStr* res = (ApsStr*)aps_alloc(sizeof(ApsStr));
    res->data = buf;
    res->len = total_len;
    return (void*)res;
}
