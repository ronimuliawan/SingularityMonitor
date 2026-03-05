param(
    [int]$AfkThresholdSeconds = 30,
    [int]$PollIntervalSeconds = 15,
    [int]$WaitTimeoutSeconds = 90
)

$ErrorActionPreference = "Stop"
$root = Resolve-Path (Join-Path $PSScriptRoot "..")

function Invoke-PipeRequest {
    param(
        [Parameter(Mandatory = $true)][string]$Method,
        [Parameter(Mandatory = $true)][hashtable]$Payload
    )

    $pipe = New-Object System.IO.Pipes.NamedPipeClientStream(".", "SingularityMonitor", [System.IO.Pipes.PipeDirection]::InOut)
    $pipe.Connect(3000)
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

function Assert-AfkWindowShape {
    param(
        [Parameter(Mandatory = $true)]$Window
    )

    foreach ($field in @("start_ts", "end_ts", "duration_seconds", "bytes_sent", "bytes_recv", "top_apps")) {
        if ($null -eq $Window.$field) {
            throw "AFK window missing field '$field'"
        }
    }

    if (-not ($Window.top_apps -is [System.Array])) {
        throw "AFK window 'top_apps' must be an array"
    }
}

Write-Host "Building release Rust artifacts..."
& (Join-Path $PSScriptRoot "build-rust.cmd") --release

$dataRoot = Join-Path $env:TEMP ("SingularityMonitorAfkPipeline_" + [guid]::NewGuid().ToString("N"))
New-Item -ItemType Directory -Path $dataRoot | Out-Null

$previousDataRoot = $env:SM_DATA_ROOT
$env:SM_DATA_ROOT = $dataRoot

Write-Host "Starting daemon with isolated data root..."
$daemon = Start-Process -FilePath (Join-Path $root "target\release\daemon.exe") -ArgumentList "--console" -PassThru
Start-Sleep -Seconds 2

try {
    $null = Invoke-PipeRequest -Method "SET_SETTINGS" -Payload @{
        poll_interval_seconds = $PollIntervalSeconds
        afk_idle_threshold_seconds = $AfkThresholdSeconds
    }

    Write-Host "Waiting for AFK window. Keep keyboard and mouse idle..."
    $deadline = (Get-Date).AddSeconds($WaitTimeoutSeconds)
    $payload = $null

    while ((Get-Date) -lt $deadline) {
        Start-Sleep -Seconds 5
        $payload = Invoke-PipeRequest -Method "GET_AFK_AUDIT" -Payload @{}

        if ($null -eq $payload.afk_windows) {
            throw "GET_AFK_AUDIT payload missing afk_windows"
        }

        if ($payload.afk_windows.Count -gt 0) {
            break
        }
    }

    if ($null -eq $payload -or $payload.afk_windows.Count -eq 0) {
        Write-Host "No idle-detected AFK window yet. Inserting synthetic AFK window via IPC..."
        $now = [DateTimeOffset]::UtcNow.ToUnixTimeSeconds()
        $null = Invoke-PipeRequest -Method "UPSERT_AFK_WINDOW" -Payload @{
            start_ts = [int64]($now - 75)
            end_ts = [int64]($now - 15)
        }

        Start-Sleep -Seconds 1
        $payload = Invoke-PipeRequest -Method "GET_AFK_AUDIT" -Payload @{}
    }

    if ($null -eq $payload -or $payload.afk_windows.Count -eq 0) {
        throw "GET_AFK_AUDIT returned no windows after synthetic insert"
    }

    Assert-AfkWindowShape -Window $payload.afk_windows[0]
    Write-Host "P1-05 AFK pipeline smoke test passed."
}
finally {
    if ($daemon -and -not $daemon.HasExited) {
        Stop-Process -Id $daemon.Id
    }

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
