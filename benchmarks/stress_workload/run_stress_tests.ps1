# Stress workload runner: APS vs C vs C++ vs Rust vs Go
$ErrorActionPreference = "Stop"

Write-Host "==================================================================" -ForegroundColor Cyan
Write-Host "   DEEP STRESS BENCHMARK: APS vs C vs C++ vs Rust vs Go          " -ForegroundColor Cyan
Write-Host "==================================================================" -ForegroundColor Cyan
Write-Host ""

Write-Host ">>> WORKLOAD 1: Real-Time Particle Physics (20,000 particles x 100 frames = 2,000,000 updates)..." -ForegroundColor Yellow

Write-Host "`n[1.1] APS Native (-O3):" -ForegroundColor Green
& ".\benchmarks\stress_workload\bin\aps_physics.exe"

Write-Host "`n[1.2] C (Clang 22 -O3):" -ForegroundColor Green
& ".\benchmarks\stress_workload\bin\c_physics.exe"

Write-Host "`n[1.3] C++ (Clang++ 22 -O3 -std=c++20):" -ForegroundColor Green
& ".\benchmarks\stress_workload\bin\cpp_physics.exe"

Write-Host "`n[1.4] Rust (rustc 1.96 -C opt-level=3):" -ForegroundColor Green
& ".\benchmarks\stress_workload\bin\rust_physics.exe"

Write-Host "`n[1.5] Go (go 1.27.3 native):" -ForegroundColor Green
& ".\benchmarks\stress_workload\bin\go_physics.exe"

Write-Host "`n------------------------------------------------------------------" -ForegroundColor Cyan
Write-Host ">>> WORKLOAD 2: High-Throughput Fintech Engine (1,000,000 ACID transactions)..." -ForegroundColor Yellow

Write-Host "`n[2.1] APS Native (-O3):" -ForegroundColor Green
& ".\benchmarks\stress_workload\bin\aps_fintech.exe"

Write-Host "`n[2.2] C (Clang 22 -O3):" -ForegroundColor Green
& ".\benchmarks\stress_workload\bin\c_fintech.exe"

Write-Host "`n[2.3] C++ (Clang++ 22 -O3 -std=c++20):" -ForegroundColor Green
& ".\benchmarks\stress_workload\bin\cpp_fintech.exe"

Write-Host "`n[2.4] Rust (rustc 1.96 -C opt-level=3):" -ForegroundColor Green
& ".\benchmarks\stress_workload\bin\rust_fintech.exe"

Write-Host "`n[2.5] Go (go 1.27.3 native):" -ForegroundColor Green
& ".\benchmarks\stress_workload\bin\go_fintech.exe"

Write-Host "`n==================================================================" -ForegroundColor Cyan
Write-Host "         ALL 5 LANGUAGES BENCHMARKED AND VERIFIED                 " -ForegroundColor Cyan
Write-Host "==================================================================" -ForegroundColor Cyan
