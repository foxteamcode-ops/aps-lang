package main

import (
	"fmt"
	"time"
)

func simulateFrame(count int64, frameIndex int64) int64 {
	var frameEnergy int64 = 0
	var i int64 = 0
	const gravity int64 = 980
	const boxWidth int64 = 100000
	const boxHeight int64 = 100000

	for i < count {
		px := (i*7919 + frameIndex*17) % boxWidth
		py := (i*6271 + frameIndex*31) % boxHeight
		vx := (i*3571)%500 - 250
		vy := (i*4787)%500 - 250 + (frameIndex % 40)

		vy += (gravity / 100)
		px += vx
		py += vy

		if px < 0 {
			px = -px
			vx = -(vx * 85 / 100)
		}
		if px > boxWidth {
			px = boxWidth - (px - boxWidth)
			vx = -(vx * 85 / 100)
		}
		if py < 0 {
			py = -py
			vy = -(vy * 85 / 100)
		}
		if py > boxHeight {
			py = boxHeight - (py - boxHeight)
			vy = -(vy * 85 / 100)
		}

		vSq := (vx * vx) + (vy * vy)
		mass := (i % 8) + 1
		frameEnergy += (mass * vSq) / 2

		i++
	}

	return frameEnergy
}

func main() {
	fmt.Println("==================================================================")
	fmt.Println("      GO (GC-MANAGED) PARTICLE PHYSICS ENGINE & REAL-TIME FPS     ")
	fmt.Println("      Workload: 20,000 Particles/Frame | 100 Frames (2M Updates)  ")
	fmt.Println("==================================================================")

	const particleCount int64 = 20000
	const totalFrames int64 = 100

	fmt.Println("Инициализация физического пространства (100000 x 100000)...")
	fmt.Printf("Количество активных частиц : %d\n", particleCount)
	fmt.Printf("Число симулируемых кадров  : %d\n", totalFrames)
	fmt.Println("------------------------------------------------------------------")

	var totalEnergyChecksum int64 = 0
	var totalTimeNs int64 = 0
	var minFrameNs int64 = 9999999999
	var maxFrameNs int64 = 0

	startAll := time.Now()

	for frame := int64(0); frame < totalFrames; frame++ {
		tStart := time.Now()

		energy := simulateFrame(particleCount, frame)
		totalEnergyChecksum += energy

		dtNs := time.Since(tStart).Nanoseconds()

		totalTimeNs += dtNs
		if dtNs < minFrameNs {
			minFrameNs = dtNs
		}
		if dtNs > maxFrameNs {
			maxFrameNs = dtNs
		}

		if frame%20 == 0 {
			frameUs := dtNs / 1000
			var fps int64 = 999999
			if dtNs > 0 {
				fps = 1000000000 / dtNs
			}
			fmt.Printf("  [Кадр #%d] Время кадра: %d мкс | Текущий FPS: %d\n", frame, frameUs, fps)
		}
	}

	totalWallMs := time.Since(startAll).Milliseconds()
	avgFrameNs := totalTimeNs / totalFrames
	avgFrameUs := avgFrameNs / 1000
	var avgFps int64 = 999999
	if avgFrameNs > 0 {
		avgFps = 1000000000 / avgFrameNs
	}
	var peakFps int64 = 999999
	if minFrameNs > 0 {
		peakFps = 1000000000 / minFrameNs
	}
	var low1PctFps int64 = 0
	if maxFrameNs > 0 {
		low1PctFps = 1000000000 / maxFrameNs
	}

	fmt.Println("==================================================================")
	fmt.Println("              ИТОГОВЫЙ ОТЧЁТ ФИЗИЧЕСКОГО ДВИЖКА (GO)              ")
	fmt.Println("==================================================================")
	fmt.Printf("  Всего физических шагов частиц: %d\n", particleCount*totalFrames)
	fmt.Printf("  Общее время симуляции (мс)   : %d\n", totalWallMs)
	fmt.Printf("  Среднее время одного кадра   : %d мкс\n", avgFrameUs)
	fmt.Printf("  СРЕДНИЙ FPS (Frames / Sec)   : %d\n", avgFps)
	fmt.Printf("  МАКСИМАЛЬНЫЙ ПИКОВЫЙ FPS     : %d\n", peakFps)
	fmt.Printf("  МИНИМАЛЬНЫЙ FPS (1%% Low)     : %d\n", low1PctFps)
	fmt.Printf("  Контрольная сумма энергии    : %d\n", totalEnergyChecksum)
	fmt.Println("==================================================================")
}
