param(
    [int]$SuspendSeconds = 35,
    [int]$PollIntervalSeconds = 15,
    [int]$WarmupTimeoutSeconds = 90,
    [int]$PostResumeTimeoutSeconds = 120
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

function Wait-ForDaemonStatus {
    param(
        [Parameter(Mandatory = $true)]$DaemonProcess,
        [int]$TimeoutSeconds = 30
    )

    $deadline = (Get-Date).AddSeconds($TimeoutSeconds)
    while ((Get-Date) -lt $deadline) {
        $DaemonProcess.Refresh()
        if ($DaemonProcess.HasExited) {
            throw "daemon exited before reporting status"
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

function Wait-ForPollAfter {
    param(
        [Parameter(Mandatory = $true)]$DaemonProcess,
        [Parameter(Mandatory = $true)][int64]$AfterTs,
        [int]$TimeoutSeconds = 60,
        [int]$StatusTimeoutSeconds = 3
    )

    $deadline = (Get-Date).AddSeconds($TimeoutSeconds)
    while ((Get-Date) -lt $deadline) {
        $status = Wait-ForDaemonStatus -DaemonProcess $DaemonProcess -TimeoutSeconds $StatusTimeoutSeconds
        $lastPollTs = [int64]$status.last_poll_ts
        if ($lastPollTs -gt $AfterTs) {
            return $lastPollTs
        }

        Start-Sleep -Milliseconds 250
    }

    throw "timed out waiting for poll ts > $AfterTs after ${TimeoutSeconds}s"
}

function Get-SuspendResumeStrategy {
    $suspendCommand = Get-Command -Name "Suspend-Process" -ErrorAction SilentlyContinue
    $resumeCommand = Get-Command -Name "Resume-Process" -ErrorAction SilentlyContinue
    if ($suspendCommand -and $resumeCommand) {
        return "cmdlet"
    }

    try {
        Use-NativeProcessSuspendResume
        return "native"
    }
    catch {
        throw "Process suspend/resume is unavailable in this environment. Suspend-Process/Resume-Process cmdlets were not found and native NtSuspendProcess setup failed."
    }
}

function Use-NativeProcessSuspendResume {
    if (-not ("SingularityMonitor.NativeProcessControl" -as [type])) {
        Add-Type -TypeDefinition @"
using System;
using System.Runtime.InteropServices;

namespace SingularityMonitor {
    public static class NativeProcessControl {
        private const uint PROCESS_SUSPEND_RESUME = 0x0800;

        [DllImport("kernel32.dll", SetLastError = true)]
        private static extern IntPtr OpenProcess(uint desiredAccess, bool inheritHandle, int processId);

        [DllImport("kernel32.dll", SetLastError = true)]
        private static extern bool CloseHandle(IntPtr handle);

        [DllImport("ntdll.dll")]
        private static extern int NtSuspendProcess(IntPtr processHandle);

        [DllImport("ntdll.dll")]
        private static extern int NtResumeProcess(IntPtr processHandle);

        public static void Suspend(int processId) {
            IntPtr handle = OpenProcess(PROCESS_SUSPEND_RESUME, false, processId);
            if (handle == IntPtr.Zero) {
                throw new InvalidOperationException("OpenProcess failed for suspend.");
            }

            try {
                int status = NtSuspendProcess(handle);
                if (status != 0) {
                    throw new InvalidOperationException("NtSuspendProcess failed with status " + status + ".");
                }
            }
            finally {
                CloseHandle(handle);
            }
        }

        public static void Resume(int processId) {
            IntPtr handle = OpenProcess(PROCESS_SUSPEND_RESUME, false, processId);
            if (handle == IntPtr.Zero) {
                throw new InvalidOperationException("OpenProcess failed for resume.");
            }

            try {
                int status = NtResumeProcess(handle);
                if (status != 0) {
                    throw new InvalidOperationException("NtResumeProcess failed with status " + status + ".");
                }
            }
            finally {
                CloseHandle(handle);
            }
        }
    }
}
"@
    }
}

function Suspend-TargetProcess {
    param(
        [Parameter(Mandatory = $true)][int]$ProcessId,
        [Parameter(Mandatory = $true)][string]$Strategy
    )

    if ($Strategy -eq "cmdlet") {
        Suspend-Process -Id $ProcessId
        return
    }

    Use-NativeProcessSuspendResume
    [SingularityMonitor.NativeProcessControl]::Suspend($ProcessId)
}

function Resume-TargetProcess {
    param(
        [Parameter(Mandatory = $true)][int]$ProcessId,
        [Parameter(Mandatory = $true)][string]$Strategy
    )

    if ($Strategy -eq "cmdlet") {
        Resume-Process -Id $ProcessId
        return
    }

    Use-NativeProcessSuspendResume
    [SingularityMonitor.NativeProcessControl]::Resume($ProcessId)
}

function Resolve-PythonCommand {
    $py = Get-Command -Name "py" -ErrorAction SilentlyContinue
    if ($py) {
        return @{ FilePath = $py.Source; PrefixArgs = @("-3") }
    }

    $python = Get-Command -Name "python" -ErrorAction SilentlyContinue
    if ($python) {
        return @{ FilePath = $python.Source; PrefixArgs = @() }
    }

    throw "Python 3 is required for SQLite validation, but neither 'py' nor 'python' was found."
}

if ($SuspendSeconds -lt 20) {
    throw "SuspendSeconds must be at least 20 seconds for continuity validation."
}

$suspendStrategy = Get-SuspendResumeStrategy
Write-Host "Using suspend/resume strategy: $suspendStrategy"

Write-Host "Building release Rust artifacts..."
& (Join-Path $PSScriptRoot "build-rust.cmd") --release

$dataRoot = Join-Path $env:TEMP ("SingularityMonitorSleepResumeContinuity_" + [guid]::NewGuid().ToString("N"))
New-Item -ItemType Directory -Path $dataRoot | Out-Null

$previousDataRoot = $env:SM_DATA_ROOT
$previousPollInterval = $env:SM_POLL_INTERVAL_SECS

$env:SM_DATA_ROOT = $dataRoot
$env:SM_POLL_INTERVAL_SECS = [string]$PollIntervalSeconds

$daemon = $null
try {
    Write-Host "Starting daemon in console mode with isolated data root..."
    $daemon = Start-Process -FilePath (Join-Path $root "target\release\daemon.exe") -ArgumentList "--console" -PassThru

    $null = Wait-ForDaemonStatus -DaemonProcess $daemon -TimeoutSeconds 30
    Write-Host "Daemon IPC ready. Waiting for warm-up polls..."

    $firstPollTs = Wait-ForPollAfter -DaemonProcess $daemon -AfterTs 0 -TimeoutSeconds $WarmupTimeoutSeconds
    $secondPollTs = Wait-ForPollAfter -DaemonProcess $daemon -AfterTs $firstPollTs -TimeoutSeconds $WarmupTimeoutSeconds
    Write-Host "Warm-up complete at poll ts values: $firstPollTs -> $secondPollTs"

    $preSuspendTs = $secondPollTs
    $thresholdSecs = [Math]::Max([int]($SuspendSeconds - 3), [int]($PollIntervalSeconds + 1))

    Write-Host "Suspending daemon process $($daemon.Id) for ${SuspendSeconds}s..."
    Suspend-TargetProcess -ProcessId $daemon.Id -Strategy $suspendStrategy
    try {
        Start-Sleep -Seconds $SuspendSeconds
    }
    finally {
        Write-Host "Resuming daemon process $($daemon.Id)..."
        Resume-TargetProcess -ProcessId $daemon.Id -Strategy $suspendStrategy
    }

    $postResumePollTs = Wait-ForPollAfter -DaemonProcess $daemon -AfterTs $preSuspendTs -TimeoutSeconds $PostResumeTimeoutSeconds
    Write-Host "Observed post-resume poll at ts=$postResumePollTs"
    Start-Sleep -Seconds 1

    $dbPath = Join-Path $dataRoot "data.db"
    if (-not (Test-Path $dbPath)) {
        throw "expected database at '$dbPath' was not created"
    }

    $python = Resolve-PythonCommand
    $maxDeltaBytes = 10GB
    if (-not [string]::IsNullOrWhiteSpace($env:SM_MAX_DELTA_BYTES)) {
        [uint64]$parsedMaxDelta = 0
        if ([uint64]::TryParse($env:SM_MAX_DELTA_BYTES, [ref]$parsedMaxDelta)) {
            $maxDeltaBytes = $parsedMaxDelta
        }
    }

    $validationScriptPath = Join-Path $dataRoot "validate_sleep_resume_continuity.py"
    @"
import json
import sqlite3
import sys

db_path = sys.argv[1]
pre_suspend_ts = int(sys.argv[2])
long_interval_threshold = int(sys.argv[3])
nominal_interval_secs = max(1, int(sys.argv[4]))
max_delta_bytes = int(sys.argv[5])

conn = sqlite3.connect(db_path)
conn.row_factory = sqlite3.Row
cur = conn.cursor()

sources = ("interface_poll", "poll")

cur.execute(
    """
    SELECT COUNT(*) AS c
    FROM usage_records
    WHERE ts > ?
      AND source IN (?, ?)
    """,
    (pre_suspend_ts, sources[0], sources[1]),
)
post_resume_poll_rows = int(cur.fetchone()["c"])

cur.execute(
    """
    SELECT COUNT(*) AS c
    FROM usage_records
    WHERE ts > ?
      AND source IN (?, ?)
      AND interval_secs >= ?
    """,
    (pre_suspend_ts, sources[0], sources[1], long_interval_threshold),
)
long_interval_rows = int(cur.fetchone()["c"])

cur.execute(
    """
    SELECT ts, interval_secs
    FROM usage_records
    WHERE ts > ?
      AND source IN (?, ?)
      AND interval_secs >= ?
    ORDER BY ts ASC
    LIMIT 1
    """,
    (pre_suspend_ts, sources[0], sources[1], long_interval_threshold),
)
observed = cur.fetchone()

observed_ts = None
observed_interval_secs = None
anomaly_window_rows = 0
oversize_rows = 0

if observed is not None:
    observed_ts = int(observed["ts"])
    observed_interval_secs = int(observed["interval_secs"])
    window_start = observed_ts - observed_interval_secs
    window_end = observed_ts + 1

    cur.execute(
        """
        SELECT
            COUNT(*) AS window_rows,
            SUM(
                CASE
                    WHEN (bytes_sent + bytes_recv) > ((? * interval_secs + ? - 1) / ?)
                    THEN 1
                    ELSE 0
                END
            ) AS oversize_count
        FROM usage_records
        WHERE source IN (?, ?)
          AND ts >= ?
          AND ts < ?
        """,
        (
            max_delta_bytes,
            nominal_interval_secs,
            nominal_interval_secs,
            sources[0],
            sources[1],
            window_start,
            window_end,
        ),
    )
    row = cur.fetchone()
    anomaly_window_rows = int(row["window_rows"] or 0)
    oversize_rows = int(row["oversize_count"] or 0)

print(
    json.dumps(
        {
            "post_resume_poll_rows": post_resume_poll_rows,
            "long_interval_rows": long_interval_rows,
            "long_interval_threshold": long_interval_threshold,
            "observed_ts": observed_ts,
            "observed_interval_secs": observed_interval_secs,
            "anomaly_window_rows": anomaly_window_rows,
            "oversize_rows": oversize_rows,
        }
    )
)
"@ | Set-Content -Path $validationScriptPath -Encoding ascii

    $pythonArgs = @() + $python.PrefixArgs + @(
        $validationScriptPath,
        $dbPath,
        [string]$preSuspendTs,
        [string]$thresholdSecs,
        [string]$PollIntervalSeconds,
        [string]$maxDeltaBytes
    )
    $validationJson = & $python.FilePath @pythonArgs
    $validation = $validationJson | ConvertFrom-Json

    $passed = $true
    if ([int]$validation.post_resume_poll_rows -lt 1) {
        Write-Host "Validation failed: no poll usage rows recorded after resume."
        $passed = $false
    }
    if ([int]$validation.long_interval_rows -lt 1) {
        Write-Host "Validation failed: no long interval rows >= $($validation.long_interval_threshold)s after resume."
        $passed = $false
    }
    if ([int]$validation.oversize_rows -gt 0) {
        Write-Host "Validation failed: detected $($validation.oversize_rows) oversize row(s) in observed interval window."
        $passed = $false
    }

    Write-Host ""
    Write-Host "Sleep/resume continuity validation summary"
    Write-Host "  Data root: $dataRoot"
    Write-Host "  Poll interval configured: ${PollIntervalSeconds}s"
    Write-Host "  Suspend duration: ${SuspendSeconds}s"
    Write-Host "  Long interval threshold: $($validation.long_interval_threshold)s"
    Write-Host "  Pre-suspend last poll ts: $preSuspendTs"
    Write-Host "  Post-resume poll ts: $postResumePollTs"
    Write-Host "  Poll usage rows after resume: $($validation.post_resume_poll_rows)"
    Write-Host "  Long interval rows after resume: $($validation.long_interval_rows)"
    Write-Host "  Observed long interval ts: $($validation.observed_ts)"
    Write-Host "  Observed interval_secs: $($validation.observed_interval_secs)"
    Write-Host "  Window rows checked: $($validation.anomaly_window_rows)"
    Write-Host "  Oversize anomaly rows: $($validation.oversize_rows)"

    if (-not $passed) {
        throw "M3 sleep/resume continuity smoke test failed."
    }

    Write-Host "M3 sleep/resume continuity smoke test passed."
}
finally {
    if ($daemon) {
        $daemon.Refresh()
        if (-not $daemon.HasExited) {
            Stop-Process -Id $daemon.Id -ErrorAction SilentlyContinue
            try {
                $daemon.WaitForExit()
            }
            catch {
            }
        }
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
