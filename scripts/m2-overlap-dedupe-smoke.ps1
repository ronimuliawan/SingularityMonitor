param()

$ErrorActionPreference = "Stop"
$root = Resolve-Path (Join-Path $PSScriptRoot "..")
$attributionGuid = "{11111111-1111-1111-1111-111111111111}"

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

$dataRoot = Join-Path $env:TEMP ("SingularityMonitorOverlapDedupe_" + [guid]::NewGuid().ToString("N"))
New-Item -ItemType Directory -Path $dataRoot | Out-Null

$previousDataRoot = $env:SM_DATA_ROOT
$env:SM_DATA_ROOT = $dataRoot

Write-Host "Starting daemon with isolated data root..."
$daemon = Start-Process -FilePath (Join-Path $root "target\release\daemon.exe") -ArgumentList "--console" -PassThru
Start-Sleep -Seconds 2

try {
    $statusPayload = Invoke-PipeRequest -Method "GET_DAEMON_STATUS" -Payload @{}
    $lastPollTs = [int64]$statusPayload.last_poll_ts
    if ($lastPollTs -le 0) {
        throw "daemon has not completed an initial poll yet"
    }

    Write-Host "Validating summary source cutover (poll vs import)..."
    $importBeforePollTs = $lastPollTs - 120
    $importAfterPollTs = $lastPollTs + 120

    $overallAppOld = "sm_overall_old_" + [guid]::NewGuid().ToString("N") + ".exe"
    $overallAppNew = "sm_overall_new_" + [guid]::NewGuid().ToString("N") + ".exe"

    $null = Invoke-PipeRequest -Method "INGEST_ATTRIBUTED_USAGE" -Payload @{
        start_ts = $importBeforePollTs - 60
        end_ts = $importBeforePollTs
        profile_name = "SmokeImportBeforePoll"
        source = "import"
        samples = @(
            @{ attribution_id = $overallAppOld; bytes_sent = 90; bytes_recv = 30 }
        )
    }

    $null = Invoke-PipeRequest -Method "INGEST_ATTRIBUTED_USAGE" -Payload @{
        start_ts = $importAfterPollTs - 60
        end_ts = $importAfterPollTs
        profile_name = "SmokeImportAfterPoll"
        source = "import"
        samples = @(
            @{ attribution_id = $overallAppNew; bytes_sent = 900; bytes_recv = 300 }
        )
    }

    $summaryNoFilter = Invoke-PipeRequest -Method "GET_USAGE_SUMMARY" -Payload @{
        start_ts = $importBeforePollTs - 1
        end_ts = $importAfterPollTs + 1
        granularity = "hour"
        interface_id = $attributionGuid
        interface_type = $null
        app_filter = $null
    }

    if ([int64]$summaryNoFilter.total_sent -ne 90 -or [int64]$summaryNoFilter.total_recv -ne 30) {
        throw "summary cutover mismatch: expected sent=90 recv=30, got sent=$($summaryNoFilter.total_sent) recv=$($summaryNoFilter.total_recv)"
    }

    Write-Host "Validating helper/import overlap dedupe for app analytics..."
    $appName = "sm_overlap_" + [guid]::NewGuid().ToString("N") + ".exe"
    $helperCutoverTs = $lastPollTs + 2000
    $importOldTs = $helperCutoverTs - 600
    $importOverlapTs = $helperCutoverTs + 60

    $null = Invoke-PipeRequest -Method "INGEST_ATTRIBUTED_USAGE" -Payload @{
        start_ts = $importOldTs - 300
        end_ts = $importOldTs
        profile_name = "OverlapImportOld"
        source = "import"
        samples = @(
            @{ attribution_id = $appName; bytes_sent = 100; bytes_recv = 50 }
        )
    }

    $null = Invoke-PipeRequest -Method "INGEST_ATTRIBUTED_USAGE" -Payload @{
        start_ts = $helperCutoverTs - 300
        end_ts = $helperCutoverTs
        profile_name = "OverlapHelper"
        source = "helper"
        samples = @(
            @{ attribution_id = $appName; bytes_sent = 40; bytes_recv = 10 }
        )
    }

    $null = Invoke-PipeRequest -Method "INGEST_ATTRIBUTED_USAGE" -Payload @{
        start_ts = $importOverlapTs - 300
        end_ts = $importOverlapTs
        profile_name = "OverlapImportRecent"
        source = "import"
        samples = @(
            @{ attribution_id = $appName; bytes_sent = 400; bytes_recv = 100 }
        )
    }

    $breakdown = Invoke-PipeRequest -Method "GET_APP_BREAKDOWN" -Payload @{
        start_ts = $importOldTs - 1
        end_ts = $importOverlapTs + 1
        interface_id = $null
        interface_type = $null
        limit = 200
        sort_by = "total_bytes_desc"
    }
    $appRow = $breakdown.apps | Where-Object { $_.process_name -eq $appName } | Select-Object -First 1
    if ($null -eq $appRow) {
        throw "expected overlap test app '$appName' in breakdown"
    }

    if ([int64]$appRow.bytes_sent -ne 140 -or [int64]$appRow.bytes_recv -ne 60) {
        throw "app breakdown dedupe mismatch: expected sent=140 recv=60, got sent=$($appRow.bytes_sent) recv=$($appRow.bytes_recv)"
    }

    $appSummary = Invoke-PipeRequest -Method "GET_USAGE_SUMMARY" -Payload @{
        start_ts = $importOldTs - 1
        end_ts = $importOverlapTs + 1
        granularity = "hour"
        interface_id = $attributionGuid
        interface_type = $null
        app_filter = $appName
    }

    if ([int64]$appSummary.total_sent -ne 140 -or [int64]$appSummary.total_recv -ne 60) {
        throw "app summary dedupe mismatch: expected sent=140 recv=60, got sent=$($appSummary.total_sent) recv=$($appSummary.total_recv)"
    }

    Write-Host "M2 overlap dedupe smoke test passed."
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
