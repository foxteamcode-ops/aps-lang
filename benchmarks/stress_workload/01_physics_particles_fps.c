#include <stdio.h>
#include <stdint.h>
#include <stdlib.h>

#ifdef _WIN32
#include <windows.h>
static int64_t get_time_ns(void) {
    LARGE_INTEGER freq, counter;
    QueryPerformanceFrequency(&freq);
    QueryPerformanceCounter(&counter);
    return (int64_t)((counter.QuadPart * 1000000000LL) / freq.QuadPart);
}
#else
#include <time.h>
static int64_t get_time_ns(void) {
    struct timespec ts;
    clock_gettime(CLOCK_MONOTONIC, &ts);
    return (int64_t)(ts.tv_sec * 1000000000LL + ts.tv_nsec);
}
#endif

static int64_t simulate_frame(int64_t count, int64_t frame_index) {
    int64_t frame_energy = 0;
    int64_t i = 0;
    int64_t gravity = 980;
    int64_t box_width = 100000;
    int64_t box_height = 100000;

    while (i < count) {
        int64_t px = (i * 7919 + frame_index * 17) % box_width;
        int64_t py = (i * 6271 + frame_index * 31) % box_height;
        int64_t vx = (i * 3571) % 500 - 250;
        int64_t vy = (i * 4787) % 500 - 250 + (frame_index % 40);

        vy += (gravity / 100);
        px += vx;
        py += vy;

        if (px < 0) {
            px = -px;
            vx = -(vx * 85 / 100);
        }
        if (px > box_width) {
            px = box_width - (px - box_width);
            vx = -(vx * 85 / 100);
        }
        if (py < 0) {
            py = -py;
            vy = -(vy * 85 / 100);
        }
        if (py > box_height) {
            py = box_height - (py - box_height);
            vy = -(vy * 85 / 100);
        }

        int64_t v_sq = (vx * vx) + (vy * vy);
        int64_t mass = (i % 8) + 1;
        frame_energy += (mass * v_sq) / 2;

        i++;
    }

    return frame_energy;
}

int main(void) {
    printf("==================================================================\n");
    printf("      C (CLANG -O3) PARTICLE PHYSICS ENGINE & REAL-TIME FPS       \n");
    printf("      Workload: 20,000 Particles/Frame | 100 Frames (2M Updates)  \n");
    printf("==================================================================\n");

    int64_t particle_count = 20000;
    int64_t total_frames = 100;

    printf("Инициализация физического пространства (100000 x 100000)...\n");
    printf("Количество активных частиц : %lld\n", (long long)particle_count);
    printf("Число симулируемых кадров  : %lld\n", (long long)total_frames);
    printf("------------------------------------------------------------------\n");

    int64_t total_energy_checksum = 0;
    int64_t total_time_ns = 0;
    int64_t min_frame_ns = 9999999999LL;
    int64_t max_frame_ns = 0;

    int64_t start_all_ns = get_time_ns();

    for (int64_t frame = 0; frame < total_frames; frame++) {
        int64_t t_start = get_time_ns();

        int64_t energy = simulate_frame(particle_count, frame);
        total_energy_checksum += energy;

        int64_t t_end = get_time_ns();
        int64_t dt_ns = t_end - t_start;

        total_time_ns += dt_ns;
        if (dt_ns < min_frame_ns) min_frame_ns = dt_ns;
        if (dt_ns > max_frame_ns) max_frame_ns = dt_ns;

        if (frame % 20 == 0) {
            int64_t frame_us = dt_ns / 1000;
            int64_t instant_fps = frame_us > 0 ? (1000000 / frame_us) : 9999;
            printf("  [Кадр #%lld] Время кадра: %lld мкс | Текущий FPS: %lld\n",
                   (long long)frame, (long long)frame_us, (long long)instant_fps);
        }
    }

    int64_t end_all_ns = get_time_ns();
    int64_t elapsed_total_ms = (end_all_ns - start_all_ns) / 1000000;
    int64_t avg_frame_us = (total_time_ns / total_frames) / 1000;
    int64_t avg_fps = avg_frame_us > 0 ? (1000000 / avg_frame_us) : 9999;
    int64_t max_fps = min_frame_ns > 0 ? (1000000000LL / min_frame_ns) : 9999;
    int64_t min_fps = max_frame_ns > 0 ? (1000000000LL / max_frame_ns) : 9999;

    printf("==================================================================\n");
    printf("              ИТОГОВЫЙ ОТЧЁТ ФИЗИЧЕСКОГО ДВИЖКА (C CLANG)         \n");
    printf("==================================================================\n");
    printf("  Всего физических шагов частиц: %lld\n", (long long)(particle_count * total_frames));
    printf("  Общее время симуляции (мс)   : %lld\n", (long long)elapsed_total_ms);
    printf("  Среднее время одного кадра   : %lld мкс\n", (long long)avg_frame_us);
    printf("  СРЕДНИЙ FPS (Frames / Sec)   : %lld\n", (long long)avg_fps);
    printf("  МАКСИМАЛЬНЫЙ ПИКОВЫЙ FPS     : %lld\n", (long long)max_fps);
    printf("  МИНИМАЛЬНЫЙ FPS (1%% Low)    : %lld\n", (long long)min_fps);
    printf("  Контрольная сумма энергии    : %lld\n", (long long)total_energy_checksum);
    printf("==================================================================\n");
    return 0;
}
