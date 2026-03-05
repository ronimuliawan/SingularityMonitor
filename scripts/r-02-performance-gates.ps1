param(
    [UInt64]$MaxRssBytes = 6291456,
    [int]$MaxQueryTotalMs = 5000,
    [int]$QueryHistoryHours = 720,
    [int]$QueryPollAppCount = 8,
    [int]$QueryHelperAppCount = 8,
    [int]$MaxImportDurationMs = 30000,
    [int]$ImportDays = 2,
    [int]$ImportChunkHours = 1,
    [double]$MaxCpuPercent1m = 5.0,
    [int]$CpuSampleCount = 8,
    [int]$CpuSampleIntervalMs = 1000,
    [int]$DaemonReadyTimeoutSeconds = 30
)

$ErrorActionPreference = "Stop"
$root = Resolve-Path (Join-Path $PSScriptRoot "..")
$daemonExe = Join-Path $root "target\release\daemon.exe"
$helperExe = Join-Path $root "target\release\helper.exe"

function Invoke-PipeRequest {
    param(
        [Parameter(Mandatory = $true)][string]$Method,
        [Parameter(Mandatory = $true)][hashtable]$Payload
    )

    $pipe = New-Object System.IO.Pipes.NamedPipeClientStream(".", "SingularityMonitor", [System.IO.Pipes.PipeDirection]::InOut)
    $pipe.Connect(5000)
    try {
        $writer = New-Object System.IO.StreamWriter($pipe)
        $writer.AutoFlush = $true
        $reader = New-Object System.IO.StreamReader($pipe)

        $request = @{
            id = [guid]::NewGuid().ToString()
            type = "request"
            method = $Method
            payload = $Payload
            error = $null
        } | ConvertTo-Json -Compress -Depth 10

        $writer.WriteLine($request)
        $responseJson = $reader.ReadLine()
        if ([string]::IsNullOrWhiteSpace($responseJson)) {
            throw "daemon returned an empty response for method '$Method'"
        }

        $response = $responseJson | ConvertFrom-Json
        if ($null -ne $response.error) {
            throw "daemon returned $($response.error.code): $($response.error.message)"
        }

        return $response.payload
    }
    finally {
        $pipe.Dispose()
    }
}

function Wait-ForDaemonReady {
    param(
        [Parameter(Mandatory = $true)][System.Diagnostics.Process]$DaemonProcess,
        [int]$TimeoutSeconds = 30
    )

    $deadline = (Get-Date).AddSeconds($TimeoutSeconds)
    while ((Get-Date) -lt $deadline) {
        if ($DaemonProcess.HasExited) {
            throw "daemon exited before reporting IPC status"
        }

        try {
            return Invoke-PipeRequest -Method "GET_DAEMON_STATUS" -Payload @{}
        }
        catch {
            Start-Sleep -Milliseconds 300
        }
    }

    throw "timed out waiting for daemon IPC readiness after ${TimeoutSeconds}s"
}

function Invoke-InIsolatedDataRoot {
    param(
        [Parameter(Mandatory = $true)][string]$Tag,
        [Parameter(Mandatory = $true)][scriptblock]$Action
    )

    $dataRoot = Join-Path $env:TEMP ("SingularityMonitorR02_" + $Tag + "_" + [guid]::NewGuid().ToString("N"))
    New-Item -ItemType Directory -Path $dataRoot | Out-Null

    $previousDataRoot = $env:SM_DATA_ROOT
    $env:SM_DATA_ROOT = $dataRoot

    try {
        & $Action
    }
    finally {
        if ([string]::IsNullOrWhiteSpace($previousDataRoot)) {
            Remove-Item Env:SM_DATA_ROOT -ErrorAction SilentlyContinue
        }
        else {
            $env:SM_DATA_ROOT = $previousDataRoot
        }

        if (Test-Path $dataRoot) {
            Remove-Item -Path $dataRoot -Recurse -Force -ErrorAction SilentlyContinue
        }
    }
}

$gateResults = New-Object System.Collections.Generic.List[object]

function Invoke-Gate {
    param(
        [Parameter(Mandatory = $true)][string]$Name,
        [Parameter(Mandatory = $true)][string]$Threshold,
        [Parameter(Mandatory = $true)][scriptblock]$Action
    )

    Write-Host "=== Gate: $Name ==="
    $sw = [System.Diagnostics.Stopwatch]::StartNew()
    try {
        $detail = & $Action
        $sw.Stop()
        $result = [pscustomobject]@{
            Name = $Name
            Status = "PASS"
            DurationMs = [math]::Round($sw.Elapsed.TotalMilliseconds, 2)
            Threshold = $Threshold
            Detail = $detail
        }
        $gateResults.Add($result)
        Write-Host ("PASS {0} ({1} ms)" -f $Name, $result.DurationMs)
    }
    catch {
        $sw.Stop()
        $result = [pscustomobject]@{
            Name = $Name
            Status = "FAIL"
            DurationMs = [math]::Round($sw.Elapsed.TotalMilliseconds, 2)
            Threshold = $Threshold
            Detail = $_.Exception.Message
        }
        $gateResults.Add($result)
        Write-Host ("FAIL {0}: {1}" -f $Name, $result.Detail)
        throw
    }
}

Write-Host "R-02 baseline performance gates starting..."

Invoke-Gate -Name "RSS ceiling" -Threshold ("<= {0} bytes" -f $MaxRssBytes) -Action {
    Invoke-InIsolatedDataRoot -Tag "rss" -Action {
        & (Join-Path $PSScriptRoot "m0-feasibility.ps1") -Samples 5 -IntervalMs 1000 -MaxBytes $MaxRssBytes -SkipHelperProbe
    }
    return ("m0-feasibility MaxBytes={0}" -f $MaxRssBytes)
}

Invoke-Gate -Name "Query latency" -Threshold ("<= {0} ms total" -f $MaxQueryTotalMs) -Action {
    & (Join-Path $PSScriptRoot "p0-16-export-perf-smoke.ps1") -MaxTotalMs $MaxQueryTotalMs -HistoryHours $QueryHistoryHours -PollAppCount $QueryPollAppCount -HelperAppCount $QueryHelperAppCount
    return ("p0-16-export-perf-smoke MaxTotalMs={0} HistoryHours={1}" -f $MaxQueryTotalMs, $QueryHistoryHours)
}

Invoke-Gate -Name "Import duration" -Threshold ("<= {0} ms" -f $MaxImportDurationMs) -Action {
    Invoke-InIsolatedDataRoot -Tag "import" -Action {
        $daemon = $null
        try {
            Write-Host "Starting daemon for import timing gate..."
            $daemon = Start-Process -FilePath $daemonExe -ArgumentList "--console" -PassThru
            $null = Wait-ForDaemonReady -DaemonProcess $daemon -TimeoutSeconds $DaemonReadyTimeoutSeconds

            $sw = [System.Diagnostics.Stopwatch]::StartNew()
            & $helperExe --import-history --days $ImportDays --chunk-hours $ImportChunkHours | Out-Host
            $sw.Stop()

            $importMs = [math]::Round($sw.Elapsed.TotalMilliseconds, 2)
            if ($importMs -gt $MaxImportDurationMs) {
                throw "helper import exceeded target (${MaxImportDurationMs}ms): ${importMs}ms"
            }

            $status = $null
            $statusDeadline = (Get-Date).AddSeconds(10)
            while ((Get-Date) -lt $statusDeadline) {
                $status = Invoke-PipeRequest -Method "GET_DAEMON_STATUS" -Payload @{}
                if ([string]$status.import_status -eq "complete" -and [int]$status.import_progress_pct -ge 100) {
                    break
                }
                Start-Sleep -Milliseconds 250
            }

            if ($null -eq $status) {
                throw "failed to query daemon import status after helper import"
            }

            if ([string]$status.import_status -ne "complete") {
                throw "daemon import_status is '$($status.import_status)' after waiting (expected 'complete')"
            }

            if ([int]$status.import_progress_pct -lt 100) {
                throw "daemon import_progress_pct is $($status.import_progress_pct) after waiting (expected >= 100)"
            }

            Write-Host ("IMPORT_DURATION_MS: {0}" -f $importMs)
        }
        finally {
            if ($daemon -and -not $daemon.HasExited) {
                Stop-Process -Id $daemon.Id
                $daemon.WaitForExit()
            }
        }
    }
    return ("helper --import-history --days {0} --chunk-hours {1}" -f $ImportDays, $ImportChunkHours)
}

Invoke-Gate -Name "CPU percent 1m" -Threshold ("<= {0}%" -f $MaxCpuPercent1m) -Action {
    Invoke-InIsolatedDataRoot -Tag "cpu" -Action {
        $daemon = $null
        $samples = New-Object System.Collections.Generic.List[double]
        try {
            Write-Host "Starting daemon for CPU sampling gate..."
            $daemon = Start-Process -FilePath $daemonExe -ArgumentList "--console" -PassThru
            $null = Wait-ForDaemonReady -DaemonProcess $daemon -TimeoutSeconds $DaemonReadyTimeoutSeconds

            for ($i = 1; $i -le $CpuSampleCount; $i++) {
                $status = Invoke-PipeRequest -Method "GET_DAEMON_STATUS" -Payload @{}
                $sample = [double]$status.cpu_percent_1m
                $samples.Add($sample)
                Write-Host ("CPU sample {0}/{1}: {2:N3}%" -f $i, $CpuSampleCount, $sample)
                if ($i -lt $CpuSampleCount) {
                    Start-Sleep -Milliseconds $CpuSampleIntervalMs
                }
            }

            $maxSample = [math]::Round(($samples | Measure-Object -Maximum).Maximum, 3)
            $avgSample = [math]::Round(($samples | Measure-Object -Average).Average, 3)
            Write-Host ("CPU_PCT_1M_MAX: {0:N3}%" -f $maxSample)
            Write-Host ("CPU_PCT_1M_AVG: {0:N3}%" -f $avgSample)

            if ($maxSample -gt $MaxCpuPercent1m) {
                throw "daemon cpu_percent_1m exceeded target (${MaxCpuPercent1m}%): ${maxSample}%"
            }
        }
        finally {
            if ($daemon -and -not $daemon.HasExited) {
                Stop-Process -Id $daemon.Id
                $daemon.WaitForExit()
            }
        }
    }
    return ("GET_DAEMON_STATUS cpu_percent_1m samples={0} interval_ms={1}" -f $CpuSampleCount, $CpuSampleIntervalMs)
}

Write-Host ""
Write-Host "R-02 gate summary:"
foreach ($result in $gateResults) {
    Write-Host (" - {0}: {1} | threshold: {2} | duration_ms: {3} | detail: {4}" -f $result.Name, $result.Status, $result.Threshold, $result.DurationMs, $result.Detail)
}

$failedCount = ($gateResults | Where-Object { $_.Status -eq "FAIL" }).Count
if ($failedCount -gt 0) {
    throw "R-02 performance gates failed ($failedCount gate(s))."
}

Write-Host "R-02 baseline performance gates passed."
