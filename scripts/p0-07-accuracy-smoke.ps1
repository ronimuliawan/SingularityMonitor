param(
    [int]$WindowSeconds = 180,
    [double]$MaxDeviationPct = 0.1,
    [uint64]$MinTotalBytes = 1048576,
    [int]$MaxAttempts = 2,
    [switch]$AllowLowTraffic
)

$ErrorActionPreference = "Stop"
$root = Resolve-Path (Join-Path $PSScriptRoot "..")
$pollIntervalSeconds = 15

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

function Wait-ForDaemonStatus {
    param(
        [Parameter(Mandatory = $true)]$DaemonProcess,
        [int]$TimeoutSeconds = 30
    )

    $deadline = (Get-Date).AddSeconds($TimeoutSeconds)
    while ((Get-Date) -lt $deadline) {
        try {
            return Invoke-PipeRequest -Method "GET_DAEMON_STATUS" -Payload @{}
        }
        catch {
            Start-Sleep -Milliseconds 300
        }
    }

    if ($DaemonProcess.HasExited) {
        throw "daemon exited before reporting status"
    }

    throw "timed out waiting for daemon IPC readiness after ${TimeoutSeconds}s"
}

function Wait-ForPollAdvance {
    param(
        [Parameter(Mandatory = $true)]$DaemonProcess,
        [Parameter(Mandatory = $true)][int64]$PreviousPollTs,
        [int]$TimeoutSeconds = 90
    )

    $deadline = (Get-Date).AddSeconds($TimeoutSeconds)
    while ((Get-Date) -lt $deadline) {
        $status = Wait-ForDaemonStatus -DaemonProcess $DaemonProcess -TimeoutSeconds 3
        $lastPollTs = [int64]$status.last_poll_ts
        if ($lastPollTs -gt $PreviousPollTs) {
            return $lastPollTs
        }

        Start-Sleep -Milliseconds 50
    }

    throw "timed out waiting for daemon poll advance after ${TimeoutSeconds}s"
}

function Get-OsNetworkTotals {
    $output = netstat -e
    if (-not $output) {
        throw "netstat -e returned no output"
    }

    $bytesLine = $null
    foreach ($line in $output) {
        if ($line -match '^\s*Bytes\s+(\d+)\s+(\d+)\s*$') {
            $bytesLine = $matches
            break
        }
    }

    if ($null -eq $bytesLine) {
        throw "failed to parse byte counters from netstat -e output"
    }

    $totalRecv = [decimal]$bytesLine[1]
    $totalSent = [decimal]$bytesLine[2]

    [pscustomobject]@{
        SentBytes = $totalSent
        RecvBytes = $totalRecv
        TotalBytes = $totalSent + $totalRecv
        Source = "netstat -e"
    }
}

function Invoke-AccuracyAttempt {
    param(
        [Parameter(Mandatory = $true)]$DaemonProcess,
        [Parameter(Mandatory = $true)][int]$IntervalCount
    )

    $currentStatus = Wait-ForDaemonStatus -DaemonProcess $DaemonProcess -TimeoutSeconds 5
    $currentPollTs = [int64]$currentStatus.last_poll_ts
    if ($currentPollTs -le 0) {
        $currentPollTs = Wait-ForPollAdvance -DaemonProcess $DaemonProcess -PreviousPollTs 0 -TimeoutSeconds 120
    }

    $baselinePollTs = Wait-ForPollAdvance -DaemonProcess $DaemonProcess -PreviousPollTs $currentPollTs -TimeoutSeconds 120
    $osStart = Get-OsNetworkTotals

    $endPollTs = $baselinePollTs
    for ($i = 0; $i -lt $IntervalCount; $i++) {
        $endPollTs = Wait-ForPollAdvance -DaemonProcess $DaemonProcess -PreviousPollTs $endPollTs -TimeoutSeconds 120
    }

    $osEnd = Get-OsNetworkTotals

    $osSentDelta = [Math]::Max([decimal]0, $osEnd.SentBytes - $osStart.SentBytes)
    $osRecvDelta = [Math]::Max([decimal]0, $osEnd.RecvBytes - $osStart.RecvBytes)
    $osTotalDelta = $osSentDelta + $osRecvDelta

    $windowStartTs = [int64]($baselinePollTs + 1)
    $windowEndTs = [int64]($endPollTs + 1)
    $windowObservedSeconds = [int]($endPollTs - $baselinePollTs)

    $summary = Invoke-PipeRequest -Method "GET_USAGE_SUMMARY" -Payload @{
        start_ts = $windowStartTs
        end_ts = $windowEndTs
        granularity = "day"
        interface_id = $null
        interface_type = $null
        app_filter = $null
    }

    $daemonSent = [decimal]$summary.total_sent
    $daemonRecv = [decimal]$summary.total_recv
    $daemonTotal = $daemonSent + $daemonRecv

    if ($osTotalDelta -le 0) {
        throw "OS counter delta is zero; cannot compute deviation percentage"
    }

    $deviationPct = [Math]::Abs([double](($daemonTotal - $osTotalDelta) * 100 / $osTotalDelta))

    [pscustomobject]@{
        WindowStartTs = $windowStartTs
        WindowEndTs = $windowEndTs
        WindowObservedSeconds = $windowObservedSeconds
        Source = $osStart.Source
        OsSentDelta = $osSentDelta
        OsRecvDelta = $osRecvDelta
        OsTotalDelta = $osTotalDelta
        DaemonSent = $daemonSent
        DaemonRecv = $daemonRecv
        DaemonTotal = $daemonTotal
        DeviationPct = $deviationPct
    }
}

Write-Host "Building release Rust artifacts..."
& (Join-Path $PSScriptRoot "build-rust.cmd") --release

$dataRoot = Join-Path $env:TEMP ("SingularityMonitorAccuracySmoke_" + [guid]::NewGuid().ToString("N"))
New-Item -ItemType Directory -Path $dataRoot | Out-Null

$previousDataRoot = $env:SM_DATA_ROOT
$previousPollInterval = $env:SM_POLL_INTERVAL_SECS
$env:SM_DATA_ROOT = $dataRoot
$env:SM_POLL_INTERVAL_SECS = "$pollIntervalSeconds"

$daemon = $null
try {
    Write-Host "Starting daemon (poll interval: ${pollIntervalSeconds}s) with isolated data root..."
    $daemon = Start-Process -FilePath (Join-Path $root "target\release\daemon.exe") -ArgumentList "--console" -PassThru

    $status = Wait-ForDaemonStatus -DaemonProcess $daemon -TimeoutSeconds 30

    $settings = Invoke-PipeRequest -Method "GET_SETTINGS" -Payload @{}
    $null = Invoke-PipeRequest -Method "SET_SETTINGS" -Payload @{
        poll_interval_seconds = $pollIntervalSeconds
        retention_days = [int]$settings.retention_days
        afk_idle_threshold_seconds = [int]$settings.afk_idle_threshold_seconds
    }

    $reloadDeadline = (Get-Date).AddSeconds(12)
    while ((Get-Date) -lt $reloadDeadline) {
        Start-Sleep -Milliseconds 250
        $status = Wait-ForDaemonStatus -DaemonProcess $daemon -TimeoutSeconds 3
        if ([int]$status.poll_interval_seconds -eq $pollIntervalSeconds) {
            break
        }
    }

    if ([int]$status.poll_interval_seconds -ne $pollIntervalSeconds) {
        throw "daemon did not apply poll interval ${pollIntervalSeconds}s"
    }

    $intervalCount = [Math]::Max(1, [int][Math]::Ceiling([double]$WindowSeconds / $pollIntervalSeconds))
    if ($MaxAttempts -lt 1) {
        throw "MaxAttempts must be >= 1"
    }

    Write-Host "Waiting for daemon warm-up polls..."
    $null = Wait-ForPollAdvance -DaemonProcess $daemon -PreviousPollTs 0 -TimeoutSeconds 120

    $result = $null
    for ($attempt = 1; $attempt -le $MaxAttempts; $attempt++) {
        Write-Host "Collecting OS counter deltas over $intervalCount poll intervals (~$($intervalCount * $pollIntervalSeconds)s), attempt $attempt/$MaxAttempts..."
        $result = Invoke-AccuracyAttempt -DaemonProcess $daemon -IntervalCount $intervalCount

        Write-Host "Accuracy smoke window: [$($result.WindowStartTs), $($result.WindowEndTs)) ($($result.WindowObservedSeconds) s), source: $($result.Source)"
        Write-Host ("OS total (sent+recv): {0} bytes (sent={1}, recv={2})" -f [uint64]$result.OsTotalDelta, [uint64]$result.OsSentDelta, [uint64]$result.OsRecvDelta)
        Write-Host ("Daemon total (sent+recv): {0} bytes (sent={1}, recv={2})" -f [uint64]$result.DaemonTotal, [uint64]$result.DaemonSent, [uint64]$result.DaemonRecv)
        Write-Host ("Absolute deviation: {0:N6}% (threshold: {1:N6}%)" -f $result.DeviationPct, $MaxDeviationPct)

        if (-not $AllowLowTraffic -and $result.OsTotalDelta -lt [decimal]$MinTotalBytes) {
            throw "low-traffic window guard hit: observed $([uint64]$result.OsTotalDelta) bytes < MinTotalBytes ($MinTotalBytes). Re-run with more traffic or use -AllowLowTraffic to bypass."
        }

        if ($result.DeviationPct -le $MaxDeviationPct) {
            Write-Host "P0-07 accuracy smoke test passed."
            break
        }

        if ($attempt -eq $MaxAttempts) {
            throw "accuracy smoke failed after $MaxAttempts attempt(s): deviation $($result.DeviationPct)% exceeds threshold $MaxDeviationPct%"
        }

        Write-Host "Deviation above threshold; retrying..."
    }
}
finally {
    if ($daemon -and -not $daemon.HasExited) {
        Stop-Process -Id $daemon.Id
        $daemon.WaitForExit()
    }

    if ([string]::IsNullOrWhiteSpace($previousDataRoot)) {
        Remove-Item Env:SM_DATA_ROOT -ErrorAction SilentlyContinue
    }
    else {
        $env:SM_DATA_ROOT = $previousDataRoot
    }

    if ([string]::IsNullOrWhiteSpace($previousPollInterval)) {
        Remove-Item Env:SM_POLL_INTERVAL_SECS -ErrorAction SilentlyContinue
    }
    else {
        $env:SM_POLL_INTERVAL_SECS = $previousPollInterval
    }

    if (Test-Path $dataRoot) {
        Remove-Item -Path $dataRoot -Recurse -Force -ErrorAction SilentlyContinue
    }
}
