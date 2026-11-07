use std::time::Instant;

fn simulate_frame(count: i64, frame_index: i64) -> i64 {
    let mut frame_energy: i64 = 0;
    let mut i: i64 = 0;
    let gravity: i64 = 980;
    let box_width: i64 = 100000;
    let box_height: i64 = 100000;

    while i < count {
        let mut px = (i * 7919 + frame_index * 17) % box_width;
        let mut py = (i * 6271 + frame_index * 31) % box_height;
        let mut vx = (i * 3571) % 500 - 250;
        let mut vy = (i * 4787) % 500 - 250 + (frame_index % 40);

        vy += gravity / 100;
        px += vx;
        py += vy;

        if px < 0 {
            px = -px;
            vx = -(vx * 85 / 100);
        }
        if px > box_width {
            px = box_width - (px - box_width);
            vx = -(vx * 85 / 100);
        }
        if py < 0 {
            py = -py;
            vy = -(vy * 85 / 100);
        }
        if py > box_height {
            py = box_height - (py - box_height);
            vy = -(vy * 85 / 100);
        }

        let v_sq = (vx * vx) + (vy * vy);
        let mass = (i % 8) + 1;
        frame_energy += (mass * v_sq) / 2;

        i += 1;
    }

    frame_energy
}

fn main() {
    println!("==================================================================");
    println!("      RUST PARTICLE PHYSICS ENGINE & REAL-TIME FPS BENCHMARK      ");
    println!("      Workload: 20,000 Particles/Frame | 100 Frames (2M Updates)  ");
    println!("==================================================================");

    let particle_count: i64 = 20000;
    let total_frames: i64 = 100;

    println!("Инициализация физического пространства (100000 x 100000)...");
    println!("Количество активных частиц : {}", particle_count);
    println!("Число симулируемых кадров  : {}", total_frames);
    println!("------------------------------------------------------------------");

    let mut total_energy_checksum: i64 = 0;
    let mut total_time_ns: u128 = 0;
    let mut min_frame_ns: u128 = u128::MAX;
    let mut max_frame_ns: u128 = 0;

    let start_all = Instant::now();

    for frame in 0..total_frames {
        let t_start = Instant::now();

        let energy = simulate_frame(particle_count, frame);
        total_energy_checksum += energy;

        let dt_ns = t_start.elapsed().as_nanos();
        total_time_ns += dt_ns;

        if dt_ns < min_frame_ns {
            min_frame_ns = dt_ns;
        }
        if dt_ns > max_frame_ns {
            max_frame_ns = dt_ns;
        }

        if frame % 20 == 0 {
            let frame_us = dt_ns / 1000;
            let instant_fps = if frame_us > 0 { 1000000 / frame_us } else { 9999 };
            println!(
                "  [Кадр #{}] Время кадра: {} мкс | Текущий FPS: {}",
                frame, frame_us, instant_fps
            );
        }
    }

    let elapsed_total_ms = start_all.elapsed().as_millis();
    let avg_frame_us = (total_time_ns / (total_frames as u128)) / 1000;
    let avg_fps = if avg_frame_us > 0 { 1000000 / avg_frame_us } else { 9999 };
    let max_fps = if min_frame_ns > 0 { 1000000000 / min_frame_ns } else { 9999 };
    let min_fps = if max_frame_ns > 0 { 1000000000 / max_frame_ns } else { 9999 };

    println!("==================================================================");
    println!("              ИТОГОВЫЙ ОТЧЁТ ФИЗИЧЕСКОГО ДВИЖКА (RUST)            ");
    println!("==================================================================");
    println!("  Всего физических шагов частиц: {}", particle_count * total_frames);
    println!("  Общее время симуляции (мс)   : {}", elapsed_total_ms);
    println!("  Среднее время одного кадра   : {} мкс", avg_frame_us);
    println!("  СРЕДНИЙ FPS (Frames / Sec)   : {}", avg_fps);
    println!("  МАКСИМАЛЬНЫЙ ПИКОВЫЙ FPS     : {}", max_fps);
    println!("  МИНИМАЛЬНЫЙ FPS (1% Low)     : {}", min_fps);
    println!("  Контрольная сумма энергии    : {}", total_energy_checksum);
    println!("==================================================================");
}
