# APS vs Rust vs C Benchmark Runner
$results = @()

$benchmarks = @(
    @{ Id = "01"; Name = "CPU-Intensive (Collatz & Sieve Primes)" },
    @{ Id = "02"; Name = "Allocation-Intensive (Region Arena vs Heap)" },
    @{ Id = "03"; Name = "Memory & Collections Workload" },
    @{ Id = "04"; Name = "HTTP/JSON/DB Transaction Workload" }
)

Write-Host "==========================================================" -ForegroundColor Cyan
Write-Host " Running Benchmark Suite: APS vs Rust vs C (Release -O3) " -ForegroundColor Cyan
Write-Host "==========================================================" -ForegroundColor Cyan

foreach ($b in $benchmarks) {
    $id = $b.Id
    $name = $b.Name
    Write-Host "`n--> Benchmark $($id): $name" -ForegroundColor Yellow

    $targets = @("c", "rust", "aps")
    foreach ($lang in $targets) {
        $exe = "benchmarks/bin/${id}_${lang}.exe"
        if (-not (Test-Path $exe)) {
            Write-Host "  [$lang] Missing $exe" -ForegroundColor Red
            continue
        }
        $fileSize = (Get-Item $exe).Length

        # Run 3 times for warm cache
        $times = @()
        for ($i = 0; $i -lt 3; $i++) {
            $sw = [System.Diagnostics.Stopwatch]::StartNew()
            $proc = Start-Process -FilePath $exe -NoNewWindow -Wait -PassThru
            $sw.Stop()
            $times += $sw.Elapsed.TotalMilliseconds
        }
        $times = $times | Sort-Object
        $medianTime = $times[1]

        Write-Host ("  [{0,-4}] Median Time: {1,8:F2} ms | Size: {2,8} bytes" -f $lang.ToUpper(), $medianTime, $fileSize)

        $results += [PSCustomObject]@{
            Benchmark = "$id - $name"
            Language  = $lang.ToUpper()
            MedianMs  = [math]::Round($medianTime, 2)
            SizeBytes = $fileSize
        }
    }
}

$results | Format-Table -AutoSize
