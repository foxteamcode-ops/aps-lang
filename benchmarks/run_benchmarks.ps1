# Complete 5-Language Benchmark Suite: APS vs C vs C++ vs Rust vs Go
$ErrorActionPreference = "Stop"

Write-Host "==========================================================================" -ForegroundColor Cyan
Write-Host " REAL BENCHMARK SUITE: APS vs C vs C++ vs Rust vs Go (Release -O3)        " -ForegroundColor Cyan
Write-Host " Host: Windows x86-64 | CPU Clocks / QPC Timers / Zero Synthetic Biases   " -ForegroundColor Cyan
Write-Host "==========================================================================" -ForegroundColor Cyan

$benchmarks = @(
    @{ Id = "01"; Name = "CPU Compute (Collatz Conjecture, Sieve Primes, Poly Kernel)" },
    @{ Id = "02"; Name = "Allocation Storm (2M Heap Allocs & Struct Arena Instantiations)" },
    @{ Id = "03"; Name = "Memory, Collections & Algorithms (Hash, Sort, Binary Search, Strings)" },
    @{ Id = "04"; Name = "Enterprise Web Workload (HTTP Parsing + JSON AST + ACID DB)" }
)

$langs = @("aps", "c", "cpp", "rust", "go")

foreach ($b in $benchmarks) {
    $id = $b.Id
    $name = $b.Name
    Write-Host ""
    Write-Host "--------------------------------------------------------------------------" -ForegroundColor DarkCyan
    Write-Host "--> WORKLOAD ${id}: $name" -ForegroundColor Yellow
    Write-Host "--------------------------------------------------------------------------" -ForegroundColor DarkCyan

    foreach ($lang in $langs) {
        $exe = "benchmarks\bin\${lang}_${id}.exe"
        if (-not (Test-Path $exe)) {
            Write-Host "  [$lang] Missing $exe" -ForegroundColor Red
            continue
        }
        $fileSize = (Get-Item $exe).Length

        # Run 3 times, capture stdout on 1st run, record median time
        $times = @()
        $lastOutput = ""
        for ($i = 0; $i -lt 3; $i++) {
            $sw = [System.Diagnostics.Stopwatch]::StartNew()
            $p = Start-Process -FilePath $exe -NoNewWindow -Wait -PassThru -RedirectStandardOutput "benchmarks\bin\tmp_out.txt"
            $sw.Stop()
            $times += $sw.Elapsed.TotalMilliseconds
            if ($i -eq 0 -and (Test-Path "benchmarks\bin\tmp_out.txt")) {
                $lastOutput = (Get-Content "benchmarks\bin\tmp_out.txt" -Raw).Trim()
            }
        }
        if (Test-Path "benchmarks\bin\tmp_out.txt") { Remove-Item "benchmarks\bin\tmp_out.txt" -Force }

        $times = $times | Sort-Object
        $medianMs = [math]::Round($times[1], 2)
        $langUpper = $lang.ToUpper()

        Write-Host "  [$langUpper] Median: $medianMs ms | Size: $fileSize bytes | $lastOutput"
    }
}

Write-Host ""
Write-Host "==========================================================================" -ForegroundColor Cyan
Write-Host " DEEP STRESS WORKLOAD 1: Real-Time Particle Physics (20,000 x 100 frames) " -ForegroundColor Cyan
Write-Host "==========================================================================" -ForegroundColor Cyan

foreach ($lang in $langs) {
    $exe = "benchmarks\stress_workload\bin\${lang}_physics.exe"
    $fileSize = (Get-Item $exe).Length
    $sw = [System.Diagnostics.Stopwatch]::StartNew()
    $p = Start-Process -FilePath $exe -NoNewWindow -Wait -PassThru -RedirectStandardOutput "benchmarks\stress_workload\bin\tmp_phys.txt"
    $sw.Stop()
    $rawOut = ""
    if (Test-Path "benchmarks\stress_workload\bin\tmp_phys.txt") {
        $rawOut = (Get-Content "benchmarks\stress_workload\bin\tmp_phys.txt" -Raw).Trim()
        Remove-Item "benchmarks\stress_workload\bin\tmp_phys.txt" -Force
    }
    $fps = "N/A"
    if ($rawOut -match "FPS.*:\s+(\d+)") {
        $fps = $matches[1]
    }
    $langUpper = $lang.ToUpper()
    $wallMs = [math]::Round($sw.Elapsed.TotalMilliseconds, 2)
    Write-Host "  [$langUpper] Wall: $wallMs ms | FPS: $fps | Size: $fileSize bytes"
}

Write-Host ""
Write-Host "==========================================================================" -ForegroundColor Cyan
Write-Host " DEEP STRESS WORKLOAD 2: Highload Fintech ACID Engine (1,000,000 tx)      " -ForegroundColor Cyan
Write-Host "==========================================================================" -ForegroundColor Cyan

foreach ($lang in $langs) {
    $exe = "benchmarks\stress_workload\bin\${lang}_fintech.exe"
    $fileSize = (Get-Item $exe).Length
    $sw = [System.Diagnostics.Stopwatch]::StartNew()
    $p = Start-Process -FilePath $exe -NoNewWindow -Wait -PassThru -RedirectStandardOutput "benchmarks\stress_workload\bin\tmp_fin.txt"
    $sw.Stop()
    $rawOut = ""
    if (Test-Path "benchmarks\stress_workload\bin\tmp_fin.txt") {
        $rawOut = (Get-Content "benchmarks\stress_workload\bin\tmp_fin.txt" -Raw).Trim()
        Remove-Item "benchmarks\stress_workload\bin\tmp_fin.txt" -Force
    }
    $tps = "N/A"
    if ($rawOut -match "TPS.*:\s+(\d+)") {
        $tps = $matches[1]
    }
    $langUpper = $lang.ToUpper()
    $wallMs = [math]::Round($sw.Elapsed.TotalMilliseconds, 2)
    Write-Host "  [$langUpper] Wall: $wallMs ms | TPS: $tps | Size: $fileSize bytes"
}

Write-Host ""
Write-Host "==========================================================================" -ForegroundColor Cyan
Write-Host "                    ALL 6 WORKLOADS COMPLETED                             " -ForegroundColor Cyan
Write-Host "==========================================================================" -ForegroundColor Cyan
