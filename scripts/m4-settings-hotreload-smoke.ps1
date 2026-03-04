param(
    [int]$TargetPollSeconds = 75,
    [int]$TimeoutSeconds = 8
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

Write-Host "Building release Rust artifacts..."
& (Join-Path $PSScriptRoot "build-rust.cmd") --release

$dataRoot = Join-Path $env:TEMP ("SingularityMonitorSettingsHotReload_" + [guid]::NewGuid().ToString("N"))
New-Item -ItemType Directory -Path $dataRoot | Out-Null

$previousDataRoot = $env:SM_DATA_ROOT
$env:SM_DATA_ROOT = $dataRoot

Write-Host "Starting daemon with isolated data root..."
$daemon = Start-Process -FilePath (Join-Path $root "target\release\daemon.exe") -ArgumentList "--console" -PassThru
Start-Sleep -Seconds 2

try {
    $beforeStatus = Invoke-PipeRequest -Method "GET_DAEMON_STATUS" -Payload @{}
    Write-Host "Initial poll interval: $($beforeStatus.poll_interval_seconds)s"

    $null = Invoke-PipeRequest -Method "SET_SETTINGS" -Payload @{
        poll_interval_seconds = $TargetPollSeconds
        retention_days = 7
        afk_idle_threshold_seconds = 420
    }

    $stopwatch = [System.Diagnostics.Stopwatch]::StartNew()
    $updated = $false
    while ($stopwatch.Elapsed.TotalSeconds -lt $TimeoutSeconds) {
        Start-Sleep -Milliseconds 250
        $status = Invoke-PipeRequest -Method "GET_DAEMON_STATUS" -Payload @{}
        if ([int]$status.poll_interval_seconds -eq $TargetPollSeconds) {
            $updated = $true
            break
        }
    }

    if (-not $updated) {
        throw "poll interval did not hot-reload to $TargetPollSeconds within ${TimeoutSeconds}s"
    }

    $settings = Invoke-PipeRequest -Method "GET_SETTINGS" -Payload @{}
    if ([int]$settings.poll_interval_seconds -ne $TargetPollSeconds) {
        throw "GET_SETTINGS mismatch for poll interval: $($settings.poll_interval_seconds)"
    }
    if ([int]$settings.retention_days -ne 7) {
        throw "GET_SETTINGS mismatch for retention_days: $($settings.retention_days)"
    }
    if ([int]$settings.afk_idle_threshold_seconds -ne 420) {
        throw "GET_SETTINGS mismatch for afk threshold: $($settings.afk_idle_threshold_seconds)"
    }

    Write-Host "M4 settings hot-reload smoke test passed."
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
