#include <iostream>
#include <vector>
#include <chrono>
#include <cstdint>

#ifdef _WIN32
#include <windows.h>
static int64_t get_time_ns() {
    LARGE_INTEGER freq, counter;
    QueryPerformanceFrequency(&freq);
    QueryPerformanceCounter(&counter);
    return static_cast<int64_t>((counter.QuadPart * 1000000000LL) / freq.QuadPart);
}
#else
static int64_t get_time_ns() {
    auto now = std::chrono::steady_clock::now().time_since_epoch();
    return std::chrono::duration_cast<std::chrono::nanoseconds>(now).count();
}
#endif

static int64_t simulate_frame(int64_t count, int64_t frame_index) {
    int64_t frame_energy = 0;
    int64_t i = 0;
    constexpr int64_t gravity = 980;
    constexpr int64_t box_width = 100000;
    constexpr int64_t box_height = 100000;

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

int main() {
    std::cout << "==================================================================\n";
    std::cout << "      C++20 (CLANG++ -O3) PARTICLE PHYSICS ENGINE & REAL-TIME FPS \n";
    std::cout << "      Workload: 20,000 Particles/Frame | 100 Frames (2M Updates)  \n";
    std::cout << "==================================================================\n";

    constexpr int64_t particle_count = 20000;
    constexpr int64_t total_frames = 100;

    std::cout << "Инициализация физического пространства (100000 x 100000)...\n";
    std::cout << "Количество активных частиц : " << particle_count << "\n";
    std::cout << "Число симулируемых кадров  : " << total_frames << "\n";
    std::cout << "------------------------------------------------------------------\n";

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
            int64_t fps = dt_ns > 0 ? 1000000000LL / dt_ns : 999999;
            std::cout << "  [Кадр #" << frame << "] Время кадра: " << frame_us << " мкс | Текущий FPS: " << fps << "\n";
        }
    }

    int64_t end_all_ns = get_time_ns();
    int64_t total_wall_ms = (end_all_ns - start_all_ns) / 1000000;
    int64_t avg_frame_ns = total_time_ns / total_frames;
    int64_t avg_frame_us = avg_frame_ns / 1000;
    int64_t avg_fps = avg_frame_ns > 0 ? 1000000000LL / avg_frame_ns : 999999;
    int64_t peak_fps = min_frame_ns > 0 ? 1000000000LL / min_frame_ns : 999999;
    int64_t low_1pct_fps = max_frame_ns > 0 ? 1000000000LL / max_frame_ns : 0;

    std::cout << "==================================================================\n";
    std::cout << "              ИТОГОВЫЙ ОТЧЁТ ФИЗИЧЕСКОГО ДВИЖКА (C++20)           \n";
    std::cout << "==================================================================\n";
    std::cout << "  Всего физических шагов частиц: " << (particle_count * total_frames) << "\n";
    std::cout << "  Общее время симуляции (мс)   : " << total_wall_ms << "\n";
    std::cout << "  Среднее время одного кадра   : " << avg_frame_us << " мкс\n";
    std::cout << "  СРЕДНИЙ FPS (Frames / Sec)   : " << avg_fps << "\n";
    std::cout << "  МАКСИМАЛЬНЫЙ ПИКОВЫЙ FPS     : " << peak_fps << "\n";
    std::cout << "  МИНИМАЛЬНЫЙ FPS (1% Low)     : " << low_1pct_fps << "\n";
    std::cout << "  Контрольная сумма энергии    : " << total_energy_checksum << "\n";
    std::cout << "==================================================================\n";

    return 0;
}
