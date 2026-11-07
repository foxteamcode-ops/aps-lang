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

#if defined(_MSC_VER)
#define APS_THREAD_LOCAL __declspec(thread)
#define APS_NORETURN __declspec(noreturn)
#elif defined(__GNUC__) || defined(__clang__)
#define APS_THREAD_LOCAL __thread
#define APS_NORETURN __attribute__((noreturn))
#else
#define APS_THREAD_LOCAL
#define APS_NORETURN
#endif

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
} ApsRegion;

typedef struct ApsVec {
    int64_t len;
    int64_t cap;
    void* data;
} ApsVec;

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

void aps_print_str(const char* s) {
    aps_ensure_utf8_console();
    if (s) {
        printf("%s", s);
    }
}

void aps_println_str(const char* s) {
    aps_ensure_utf8_console();
    if (s) {
        printf("%s\n", s);
    } else {
        printf("\n");
    }
}

void aps_eprintln_str(const char* s) {
    aps_ensure_utf8_console();
    if (s) {
        fprintf(stderr, "%s\n", s);
    } else {
        fprintf(stderr, "\n");
    }
}

APS_NORETURN void aps_panic(const char* msg, const char* file, int64_t line) {
    aps_ensure_utf8_console();
    fprintf(stderr, "panic[R0003]: %s\n  --> %s:%lld\n", msg ? msg : "explicit panic", file ? file : "<unknown>", (long long)line);
    exit(101);
}

APS_NORETURN void aps_bounds_check(int64_t index, int64_t len, const char* file, int64_t line) {
    aps_ensure_utf8_console();
    fprintf(stderr, "panic[R0001]: index out of bounds: the len is %lld but the index is %lld\n  --> %s:%lld\n",
            (long long)len, (long long)index, file ? file : "<unknown>", (long long)line);
    exit(101);
}

void aps_assert(int64_t cond, const char* msg) {
    if (!cond) {
        aps_panic(msg ? msg : "assertion failed", "<runtime>", 0);
    }
}

void* aps_alloc(int64_t size) {
    if (size <= 0) size = 8;
    void* ptr = calloc(1, (size_t)size);
    if (!ptr) {
        aps_panic("out of memory in Managed Ownership heap allocation", "<runtime>", 0);
    }
    return ptr;
}

void aps_free(void* ptr) {
    if (ptr) {
        free(ptr);
    }
}

/* Dynamic Vector Runtime Engine */
void aps_vec_push(void* vec_ptr, int64_t val) {
    if (!vec_ptr) return;
    ApsVec* v = (ApsVec*)vec_ptr;
    if (v->len >= v->cap) {
        size_t new_cap = v->cap > 0 ? (size_t)v->cap * 2 : 16;
        void* new_data = realloc(v->data, new_cap * sizeof(int64_t));
        if (!new_data) aps_panic("out of memory in Vec.push expansion", "<runtime>", 0);
        v->data = new_data;
        v->cap = (int64_t)new_cap;
    }
    ((int64_t*)v->data)[v->len] = val;
    v->len++;
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

/* AMA Tier 2: Region Memory Arena (O(1) bump allocation, O(1) bulk destruction) */
static APS_THREAD_LOCAL ApsRegion* g_tl_cached_region = NULL;

void* aps_region_begin(const char* name, int64_t initial_cap) {
    if (initial_cap < 4096) initial_cap = 65536;

    // Ultra-Fast Path: Reuse thread-local cached region block (0 heap allocations, O(1) reset)
    if (g_tl_cached_region != NULL) {
        ApsRegion* reg = g_tl_cached_region;
        g_tl_cached_region = NULL;
        reg->name = name;
        reg->total_allocated = 0;
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
    return (void*)reg;
}

void* aps_region_alloc(void* region_ptr, int64_t size) {
    if (!region_ptr) return aps_alloc(size);
    ApsRegion* reg = (ApsRegion*)region_ptr;
    size_t aligned_size = ((size_t)(size <= 0 ? 8 : size) + 7u) & ~7u;
    ApsRegionBlock* blk = reg->head;
    // 1-cycle bump allocation fast path
    if (blk && blk->used + aligned_size <= blk->capacity) {
        void* ptr = (void*)(blk->data + blk->used);
        blk->used += aligned_size;
        reg->total_allocated += aligned_size;
        return ptr;
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

/* UTF-8 String helpers */
int64_t aps_str_len(const char* s) {
    return s ? (int64_t)strlen(s) : 0;
}

int64_t aps_str_eq(const char* a, const char* b) {
    if (a == b) return 1;
    if (!a || !b) return 0;
    if (*a != *b) return 0; // O(1) first-byte inequality check
    return strcmp(a, b) == 0 ? 1 : 0;
}

/* Thread-Local String Slab Buffer for Zero-Cost SSO / Concatenations */
#define APS_STR_SLAB_SIZE 262144
static APS_THREAD_LOCAL char* g_tl_str_slab = NULL;
static APS_THREAD_LOCAL size_t g_tl_str_slab_used = 0;

const char* aps_str_concat(const char* a, const char* b) {
    if (!a) a = "";
    if (!b) b = "";
    size_t la = strlen(a);
    size_t lb = strlen(b);
    if (la == 0) return b;
    if (lb == 0) return a;
    size_t total_len = la + lb;
    if (total_len > SIZE_MAX - 1) {
        aps_panic("string concatenation size overflow", "<runtime>", 0);
    }
    size_t aligned_size = (total_len + 8) & ~7u;

    // Fast-path: allocate from Thread-Local String Slab (Zero-Cost bump pointer, 0 OS malloc calls)
    if (!g_tl_str_slab) {
        g_tl_str_slab = (char*)malloc(APS_STR_SLAB_SIZE);
        g_tl_str_slab_used = 0;
    }
    if (g_tl_str_slab && (g_tl_str_slab_used + aligned_size <= APS_STR_SLAB_SIZE)) {
        char* out = g_tl_str_slab + g_tl_str_slab_used;
        g_tl_str_slab_used += aligned_size;
        memcpy(out, a, la);
        memcpy(out + la, b, lb);
        out[total_len] = '\0';
        return out;
    }

    // Fallback: standard malloc if slab is full
    char* out = (char*)malloc(total_len + 1);
    if (!out) aps_panic("out of memory in str_concat", "<runtime>", 0);
    memcpy(out, a, la);
    memcpy(out + la, b, lb);
    out[total_len] = '\0';
    return out;
}

const char* aps_int_to_str(int64_t val) {
    char* buf = (char*)aps_alloc(32);
    snprintf(buf, 32, "%lld", (long long)val);
    return buf;
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
            send(client, resp, resp_len, 0);
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
