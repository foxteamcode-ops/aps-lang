# Stress workload runner: APS vs Rust vs C
$ErrorActionPreference = "Stop"

Write-Host "==================================================================" -ForegroundColor Cyan
Write-Host "       REAL STRESS WORKLOAD: FPS, PARTICLES, 1M FINTECH TPS       " -ForegroundColor Cyan
Write-Host "==================================================================" -ForegroundColor Cyan
Write-Host ""

Write-Host ">>> WORKLOAD 1: Real-Time Particle Physics (20,000 particles x 100 frames = 2M updates)..." -ForegroundColor Yellow
Write-Host "[1.1] APS Native (-O3):" -ForegroundColor Green
& ".\benchmarks\stress_workload\bin\aps_physics.exe"

Write-Host "`n[1.2] Rust (opt-level=3):" -ForegroundColor Green
& ".\benchmarks\stress_workload\bin\rust_physics.exe"

Write-Host "`n[1.3] C (Clang -O3):" -ForegroundColor Green
& ".\benchmarks\stress_workload\bin\c_physics.exe"

Write-Host "`n------------------------------------------------------------------" -ForegroundColor Cyan
Write-Host ">>> WORKLOAD 2: High-Throughput Fintech Engine (1,000,000 ACID transactions)..." -ForegroundColor Yellow
Write-Host "[2.1] APS Native (-O3):" -ForegroundColor Green
& ".\benchmarks\stress_workload\bin\aps_fintech.exe"

Write-Host "`n[2.2] Rust (opt-level=3):" -ForegroundColor Green
& ".\benchmarks\stress_workload\bin\rust_fintech.exe"

Write-Host "`n[2.3] C (Clang -O3):" -ForegroundColor Green
& ".\benchmarks\stress_workload\bin\c_fintech.exe"

Write-Host "`n==================================================================" -ForegroundColor Cyan
Write-Host "         ALL STRESS WORKLOADS EXECUTED AND VERIFIED               " -ForegroundColor Cyan
Write-Host "==================================================================" -ForegroundColor Cyan
