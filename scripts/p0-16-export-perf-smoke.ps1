param(
    [int]$MaxTotalMs = 5000,
    [int]$HistoryHours = 8760,
    [int]$PollAppCount = 8,
    [int]$HelperAppCount = 8
)

$ErrorActionPreference = "Stop"
$root = Resolve-Path (Join-Path $PSScriptRoot "..")

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

function Invoke-TimedPipeRequest {
    param(
        [Parameter(Mandatory = $true)][string]$Method,
        [Parameter(Mandatory = $true)][hashtable]$Payload
    )

    $sw = [System.Diagnostics.Stopwatch]::StartNew()
    $payloadResult = Invoke-PipeRequest -Method $Method -Payload $Payload
    $sw.Stop()

    [pscustomobject]@{
        Method = $Method
        ElapsedMs = [math]::Round($sw.Elapsed.TotalMilliseconds, 2)
        Payload = $payloadResult
    }
}

Write-Host "Building release Rust artifacts..."
& (Join-Path $PSScriptRoot "build-rust.cmd") --release

$dataRoot = Join-Path $env:TEMP ("SingularityMonitorExportPerf_" + [guid]::NewGuid().ToString("N"))
New-Item -ItemType Directory -Path $dataRoot | Out-Null

$previousDataRoot = $env:SM_DATA_ROOT
$env:SM_DATA_ROOT = $dataRoot

$daemon = $null
$seedScriptPath = Join-Path $dataRoot "seed_export_perf.py"

try {
    Write-Host "Initializing daemon DB schema in isolated data root..."
    $daemon = Start-Process -FilePath (Join-Path $root "target\release\daemon.exe") -ArgumentList "--console" -PassThru
    Start-Sleep -Seconds 2
    if (-not $daemon.HasExited) {
        Stop-Process -Id $daemon.Id
        $daemon.WaitForExit()
    }
    $daemon = $null

    $hourSecs = 3600
    $alignedNowTs = [int64]([math]::Floor([double][DateTimeOffset]::UtcNow.ToUnixTimeSeconds() / $hourSecs) * $hourSecs)
    $startTs = $alignedNowTs - ([int64]$HistoryHours * $hourSecs)
    $endTs = $alignedNowTs
    $dbPath = Join-Path $dataRoot "data.db"

    if (-not (Test-Path $dbPath)) {
        throw "expected initialized sqlite db at '$dbPath'"
    }

    Write-Host "Seeding synthetic $HistoryHours-hour historical dataset..."
    @'
import sqlite3
import sys

db_path = sys.argv[1]
start_ts = int(sys.argv[2])
history_hours = int(sys.argv[3])
poll_app_count = int(sys.argv[4])
helper_app_count = int(sys.argv[5])

ATTR_GUID = "{11111111-1111-1111-1111-111111111111}"
ETH_GUID = "{22222222-2222-2222-2222-222222222222}"
WIFI_GUID = "{33333333-3333-3333-3333-333333333333}"

conn = sqlite3.connect(db_path)
cur = conn.cursor()
cur.execute("PRAGMA foreign_keys = ON")
cur.execute("PRAGMA synchronous = OFF")
cur.execute("PRAGMA temp_store = MEMORY")

cur.execute("BEGIN")
cur.execute("DELETE FROM usage_records")
cur.execute("DELETE FROM apps")
cur.execute("DELETE FROM interfaces")

end_ts = start_ts + (history_hours * 3600)
interfaces = [
    (ATTR_GUID, "Attributed Usage (Perf Seed)", "other", 0, start_ts, end_ts),
    (ETH_GUID, "Ethernet Perf", "ethernet", 0, start_ts, end_ts),
    (WIFI_GUID, "Wi-Fi Perf", "wifi", 0, start_ts, end_ts),
]
cur.executemany(
    """
    INSERT INTO interfaces(guid, name, type, is_metered, first_seen, last_seen)
    VALUES(?, ?, ?, ?, ?, ?)
    """,
    interfaces,
)

poll_apps = [f"sm_poll_{i:02d}.exe" for i in range(poll_app_count)]
helper_apps = [f"sm_helper_{i:02d}.exe" for i in range(helper_app_count)]
all_apps = poll_apps + helper_apps

cur.executemany(
    """
    INSERT INTO apps(process_name, display_name, first_seen, last_seen)
    VALUES(?, ?, ?, ?)
    """,
    [(name, name, start_ts, end_ts) for name in all_apps],
)

cur.execute("SELECT id, guid FROM interfaces")
interface_ids = {guid: iid for (iid, guid) in cur.fetchall()}
cur.execute("SELECT id, process_name FROM apps")
app_ids = {name: aid for (aid, name) in cur.fetchall()}

attr_id = interface_ids[ATTR_GUID]
poll_iface_ids = [interface_ids[ETH_GUID], interface_ids[WIFI_GUID]]

insert_sql = """
INSERT INTO usage_records(
    ts,
    app_id,
    interface_id,
    bytes_sent,
    bytes_recv,
    interval_secs,
    source
) VALUES(?, ?, ?, ?, ?, ?, ?)
"""

rows = []
batch_size = 5000
helper_cutover_hour = history_hours // 2

for hour_idx in range(history_hours):
    ts = start_ts + (hour_idx * 3600)

    for app_idx, app_name in enumerate(poll_apps):
        app_id = app_ids[app_name]
        sent_base = 1800 + (app_idx * 37)
        recv_base = 2300 + (app_idx * 41)
        for iface_offset, iface_id in enumerate(poll_iface_ids):
            rows.append((
                ts,
                app_id,
                iface_id,
                sent_base + iface_offset * 300 + (hour_idx % 17),
                recv_base + iface_offset * 280 + (hour_idx % 19),
                3600,
                "interface_poll",
            ))

    helper_source = "import" if hour_idx < helper_cutover_hour else "helper"
    for app_idx, app_name in enumerate(helper_apps):
        app_id = app_ids[app_name]
        rows.append((
            ts,
            app_id,
            attr_id,
            1500 + (app_idx * 53) + (hour_idx % 23),
            1900 + (app_idx * 47) + (hour_idx % 29),
            3600,
            helper_source,
        ))

    if len(rows) >= batch_size:
        cur.executemany(insert_sql, rows)
        rows.clear()

if rows:
    cur.executemany(insert_sql, rows)

conn.commit()
conn.close()
'@ | Set-Content -Path $seedScriptPath -Encoding UTF8

    if (Get-Command python -ErrorAction SilentlyContinue) {
        & python $seedScriptPath $dbPath $startTs $HistoryHours $PollAppCount $HelperAppCount
    }
    elseif (Get-Command py -ErrorAction SilentlyContinue) {
        & py -3 $seedScriptPath $dbPath $startTs $HistoryHours $PollAppCount $HelperAppCount
    }
    else {
        throw "python runtime not found (expected 'python' or 'py -3')"
    }

    Write-Host "Starting daemon for export performance validation..."
    $daemon = Start-Process -FilePath (Join-Path $root "target\release\daemon.exe") -ArgumentList "--console" -PassThru
    Start-Sleep -Seconds 2

    Write-Host "Running export IPC query sequence..."
    $sequenceSw = [System.Diagnostics.Stopwatch]::StartNew()
    $usageSummary = Invoke-TimedPipeRequest -Method "GET_USAGE_SUMMARY" -Payload @{
        start_ts = $startTs
        end_ts = $endTs
        granularity = "hour"
        interface_id = $null
        interface_type = $null
        app_filter = $null
    }
    $appBreakdown = Invoke-TimedPipeRequest -Method "GET_APP_BREAKDOWN" -Payload @{
        start_ts = $startTs
        end_ts = $endTs
        interface_id = $null
        interface_type = $null
        limit = 200
        sort_by = "total_bytes_desc"
    }
    $interfaceBreakdown = Invoke-TimedPipeRequest -Method "GET_INTERFACE_BREAKDOWN" -Payload @{
        start_ts = $startTs
        end_ts = $endTs
        interface_id = $null
        interface_type = $null
    }
    $sequenceSw.Stop()

    if ([int]$usageSummary.Payload.buckets.Count -le 0) {
        throw "GET_USAGE_SUMMARY returned no buckets"
    }
    if ([int]$appBreakdown.Payload.total_apps -le 0) {
        throw "GET_APP_BREAKDOWN returned no apps"
    }
    if ([int]$interfaceBreakdown.Payload.total_interfaces -le 0) {
        throw "GET_INTERFACE_BREAKDOWN returned no interfaces"
    }

    $totalMs = [math]::Round($sequenceSw.Elapsed.TotalMilliseconds, 2)

    Write-Host ("GET_USAGE_SUMMARY: {0} ms" -f $usageSummary.ElapsedMs)
    Write-Host ("GET_APP_BREAKDOWN: {0} ms" -f $appBreakdown.ElapsedMs)
    Write-Host ("GET_INTERFACE_BREAKDOWN: {0} ms" -f $interfaceBreakdown.ElapsedMs)
    Write-Host ("TOTAL_QUERY_FLOW: {0} ms" -f $totalMs)

    if ($totalMs -gt $MaxTotalMs) {
        throw "export query flow exceeded target (${MaxTotalMs}ms): ${totalMs}ms"
    }

    Write-Host "P0-16 export performance smoke test passed."
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

    if (Test-Path $seedScriptPath) {
        Remove-Item -Path $seedScriptPath -Force -ErrorAction SilentlyContinue
    }

    if (Test-Path $dataRoot) {
        Remove-Item -Path $dataRoot -Recurse -Force -ErrorAction SilentlyContinue
    }
}
