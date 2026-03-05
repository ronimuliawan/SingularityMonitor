param(
    [int]$WarmupTimeoutSeconds = 45,
    [int]$BreakdownWindowSeconds = 3600
)

$ErrorActionPreference = "Stop"
$root = Resolve-Path (Join-Path $PSScriptRoot "..")
$knownInterfaceTypes = @("wifi", "ethernet", "loopback", "other")

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

function Wait-ForInitialPoll {
    param(
        [Parameter(Mandatory = $true)]$DaemonProcess,
        [int]$TimeoutSeconds = 45
    )

    $deadline = (Get-Date).AddSeconds($TimeoutSeconds)
    while ((Get-Date) -lt $deadline) {
        $status = Wait-ForDaemonStatus -DaemonProcess $DaemonProcess -TimeoutSeconds 3
        if ([int64]$status.last_poll_ts -gt 0) {
            return [int64]$status.last_poll_ts
        }

        Start-Sleep -Milliseconds 250
    }

    throw "timed out waiting for daemon poll warm-up after ${TimeoutSeconds}s"
}

function Test-RequiredFields {
    param(
        [Parameter(Mandatory = $true)]$Object,
        [Parameter(Mandatory = $true)][string[]]$FieldNames,
        [Parameter(Mandatory = $true)][string]$Context
    )

    $availableNames = @($Object.PSObject.Properties.Name)
    foreach ($fieldName in $FieldNames) {
        if ($availableNames -notcontains $fieldName) {
            throw "$Context missing required field '$fieldName'"
        }
    }
}

function Test-NonNegativeIntegerField {
    param(
        [Parameter(Mandatory = $true)]$Value,
        [Parameter(Mandatory = $true)][string]$FieldName,
        [Parameter(Mandatory = $true)][string]$Context
    )

    [uint64]$parsed = 0
    if (-not [uint64]::TryParse([string]$Value, [ref]$parsed)) {
        throw "$Context has non-numeric $FieldName value '$Value'"
    }
}

Write-Host "Building release Rust artifacts..."
& (Join-Path $PSScriptRoot "build-rust.cmd") --release

$dataRoot = Join-Path $env:TEMP ("SingularityMonitorMeteredFlagSmoke_" + [guid]::NewGuid().ToString("N"))
New-Item -ItemType Directory -Path $dataRoot | Out-Null

$previousDataRoot = $env:SM_DATA_ROOT
$env:SM_DATA_ROOT = $dataRoot

$daemon = $null
try {
    Write-Host "Starting daemon with isolated data root..."
    $daemon = Start-Process -FilePath (Join-Path $root "target\release\daemon.exe") -ArgumentList "--console" -PassThru

    $null = Wait-ForDaemonStatus -DaemonProcess $daemon -TimeoutSeconds 30
    Write-Host "Daemon IPC ready. Waiting for poll warm-up..."
    $lastPollTs = Wait-ForInitialPoll -DaemonProcess $daemon -TimeoutSeconds $WarmupTimeoutSeconds
    Write-Host "Warm-up complete at poll ts=$lastPollTs"

    $interfacesPayload = Invoke-PipeRequest -Method "GET_INTERFACES" -Payload @{}
    Test-RequiredFields -Object $interfacesPayload -FieldNames @("interfaces") -Context "GET_INTERFACES payload"

    $interfaces = @($interfacesPayload.interfaces)
    if ($interfaces.Count -le 0) {
        throw "GET_INTERFACES returned zero interfaces"
    }

    $meteredCount = 0
    $unmeteredCount = 0
    foreach ($iface in $interfaces) {
        Test-RequiredFields -Object $iface -FieldNames @("guid", "name", "interface_type", "is_metered") -Context "GET_INTERFACES interface row"

        if ([string]::IsNullOrWhiteSpace([string]$iface.guid)) {
            throw "GET_INTERFACES row has empty guid"
        }
        if ([string]::IsNullOrWhiteSpace([string]$iface.name)) {
            throw "GET_INTERFACES row has empty name"
        }

        $interfaceType = ([string]$iface.interface_type).ToLowerInvariant()
        if ($knownInterfaceTypes -notcontains $interfaceType) {
            throw "GET_INTERFACES row has unknown interface_type '$($iface.interface_type)'"
        }

        if ($iface.is_metered -isnot [bool]) {
            throw "GET_INTERFACES row has non-boolean is_metered value '$($iface.is_metered)'"
        }

        if ([bool]$iface.is_metered) {
            $meteredCount++
        }
        else {
            $unmeteredCount++
        }
    }

    $endTs = [DateTimeOffset]::UtcNow.ToUnixTimeSeconds()
    $startTs = $endTs - [Math]::Max(60, $BreakdownWindowSeconds)
    $breakdownPayload = Invoke-PipeRequest -Method "GET_INTERFACE_BREAKDOWN" -Payload @{
        start_ts = [int64]$startTs
        end_ts = [int64]$endTs
        interface_id = $null
        interface_type = $null
    }

    Test-RequiredFields -Object $breakdownPayload -FieldNames @("interfaces", "total_interfaces") -Context "GET_INTERFACE_BREAKDOWN payload"

    if ($breakdownPayload.total_interfaces -isnot [int] -and $breakdownPayload.total_interfaces -isnot [long]) {
        throw "GET_INTERFACE_BREAKDOWN total_interfaces is not numeric: '$($breakdownPayload.total_interfaces)'"
    }

    $breakdownRows = @($breakdownPayload.interfaces)
    if ([int]$breakdownPayload.total_interfaces -ne $breakdownRows.Count) {
        throw "GET_INTERFACE_BREAKDOWN total_interfaces mismatch: payload=$($breakdownPayload.total_interfaces), rows=$($breakdownRows.Count)"
    }

    foreach ($row in $breakdownRows) {
        Test-RequiredFields -Object $row -FieldNames @("interface_id", "interface_name", "interface_type", "is_metered", "bytes_sent", "bytes_recv") -Context "GET_INTERFACE_BREAKDOWN row"

        if ([string]::IsNullOrWhiteSpace([string]$row.interface_id)) {
            throw "GET_INTERFACE_BREAKDOWN row has empty interface_id"
        }
        if ([string]::IsNullOrWhiteSpace([string]$row.interface_name)) {
            throw "GET_INTERFACE_BREAKDOWN row has empty interface_name"
        }

        $breakdownType = ([string]$row.interface_type).ToLowerInvariant()
        if ($knownInterfaceTypes -notcontains $breakdownType) {
            throw "GET_INTERFACE_BREAKDOWN row has unknown interface_type '$($row.interface_type)'"
        }

        if ($row.is_metered -isnot [bool]) {
            throw "GET_INTERFACE_BREAKDOWN row has non-boolean is_metered value '$($row.is_metered)'"
        }

        Test-NonNegativeIntegerField -Value $row.bytes_sent -FieldName "bytes_sent" -Context "GET_INTERFACE_BREAKDOWN row"
        Test-NonNegativeIntegerField -Value $row.bytes_recv -FieldName "bytes_recv" -Context "GET_INTERFACE_BREAKDOWN row"
    }

    Write-Host ("Interfaces discovered: {0}" -f $interfaces.Count)
    Write-Host ("Metered interfaces: {0}" -f $meteredCount)
    Write-Host ("Unmetered interfaces: {0}" -f $unmeteredCount)
    Write-Host ("Breakdown interfaces in window [{0}, {1}): {2}" -f $startTs, $endTs, $breakdownRows.Count)
    Write-Host "M3 metered flag validation smoke test passed."
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

    if (Test-Path $dataRoot) {
        Remove-Item -Path $dataRoot -Recurse -Force -ErrorAction SilentlyContinue
    }
}
