param(
    [int]$WindowSecs = 300
)

$ErrorActionPreference = "Stop"
$root = Resolve-Path (Join-Path $PSScriptRoot "..")

Write-Host "Building release artifacts..."
& (Join-Path $PSScriptRoot "build-rust.cmd") --release

Write-Host "Starting daemon console..."
$daemon = Start-Process -FilePath (Join-Path $root "target\release\daemon.exe") -ArgumentList "--console" -PassThru
Start-Sleep -Seconds 2

try {
    Write-Host "Pushing attributed usage snapshot..."
    & (Join-Path $root "target\release\helper.exe") --push-once --window-secs $WindowSecs | Out-Host

    Write-Host "Fetching app breakdown over pipe..."
    $pipe = New-Object System.IO.Pipes.NamedPipeClientStream(".", "SingularityMonitor", [System.IO.Pipes.PipeDirection]::InOut)
    $pipe.Connect(3000)
    $writer = New-Object System.IO.StreamWriter($pipe)
    $writer.AutoFlush = $true
    $reader = New-Object System.IO.StreamReader($pipe)

    $request = @{
        id = [guid]::NewGuid().ToString()
        type = "request"
        method = "GET_APP_BREAKDOWN"
        payload = @{
            start_ts = 0
            end_ts = 9999999999
            interface_id = $null
            limit = 20
            sort_by = "total_bytes_desc"
        }
        error = $null
    } | ConvertTo-Json -Compress

    $writer.WriteLine($request)
    $response = $reader.ReadLine()
    Write-Host $response
    $pipe.Dispose()
}
finally {
    if ($daemon -and -not $daemon.HasExited) {
        Stop-Process -Id $daemon.Id
    }
}

Write-Host "M1 attribution smoke test complete."
