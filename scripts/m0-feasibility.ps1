param(
    [int]$Samples = 5,
    [int]$IntervalMs = 1000,
    [UInt64]$MaxBytes = 5242880,
    [switch]$SkipHelperProbe
)

$ErrorActionPreference = "Stop"
$root = Resolve-Path (Join-Path $PSScriptRoot "..")

Write-Host "Building release artifacts..."
& (Join-Path $PSScriptRoot "build-rust.cmd") --release

if (-not $SkipHelperProbe) {
    Write-Host "Running helper attribution probe..."
    & (Join-Path $root "target\release\helper.exe") | Out-Host
}
else {
    Write-Host "Skipping helper attribution probe."
}

Write-Host "Starting daemon console..."
$daemon = Start-Process -FilePath (Join-Path $root "target\release\daemon.exe") -ArgumentList "--console" -PassThru
Start-Sleep -Seconds 2

try {
    Write-Host "Checking daemon status over named pipe..."
    $pipe = New-Object System.IO.Pipes.NamedPipeClientStream(".", "SingularityMonitor", [System.IO.Pipes.PipeDirection]::InOut)
    $pipe.Connect(3000)
    $writer = New-Object System.IO.StreamWriter($pipe)
    $writer.AutoFlush = $true
    $reader = New-Object System.IO.StreamReader($pipe)
    $request = @{ id = [guid]::NewGuid().ToString(); type = "request"; method = "GET_DAEMON_STATUS"; payload = @{}; error = $null } | ConvertTo-Json -Compress
    $writer.WriteLine($request)
    $response = $reader.ReadLine()
    Write-Host $response
    $pipe.Dispose()

    Write-Host "Sampling daemon memory..."
    & (Join-Path $root "target\release\perf-harness.exe") --pid $daemon.Id --samples $Samples --interval-ms $IntervalMs --max-bytes $MaxBytes | Out-Host
}
finally {
    if ($daemon -and -not $daemon.HasExited) {
        Stop-Process -Id $daemon.Id
    }
}

Write-Host "M0 feasibility checks complete."
