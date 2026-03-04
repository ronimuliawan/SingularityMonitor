param(
    [ValidateSet("install", "uninstall", "start", "stop", "restart", "status")]
    [string]$Action = "status",
    [string]$ServiceName = "SingularityMonitorDaemon",
    [string]$DisplayName = "Singularity Monitor Daemon",
    [string]$DaemonPath
)

$ErrorActionPreference = "Stop"

function Assert-Administrator {
    $identity = [Security.Principal.WindowsIdentity]::GetCurrent()
    $principal = New-Object Security.Principal.WindowsPrincipal($identity)
    if (-not $principal.IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)) {
        throw "Administrator rights are required. Run this script from an elevated shell."
    }
}

function Resolve-ProjectRoot {
    return (Resolve-Path (Join-Path $PSScriptRoot "..")).Path
}

function Resolve-DaemonExecutable([string]$PathFromParam) {
    if (-not [string]::IsNullOrWhiteSpace($PathFromParam)) {
        $resolved = Resolve-Path $PathFromParam -ErrorAction Stop
        return $resolved.Path
    }

    $root = Resolve-ProjectRoot
    $release = Join-Path $root "target\release\daemon.exe"
    if (Test-Path $release) {
        return $release
    }

    $debug = Join-Path $root "target\debug\daemon.exe"
    if (Test-Path $debug) {
        return $debug
    }

    throw "daemon.exe not found. Build first using scripts\\build-rust.cmd --release or pass -DaemonPath explicitly."
}

function Get-ServiceSafe([string]$Name) {
    return Get-Service -Name $Name -ErrorAction SilentlyContinue
}

function Install-ServiceInternal {
    $existing = Get-ServiceSafe $ServiceName
    if ($existing) {
        Write-Host "Service '$ServiceName' already exists."
        return
    }

    $daemonExe = Resolve-DaemonExecutable $DaemonPath
    $binPath = '"' + $daemonExe + '" --service'

    & sc.exe create $ServiceName binPath= $binPath start= auto DisplayName= $DisplayName obj= "NT AUTHORITY\LocalService" type= own | Out-Host
    if ($LASTEXITCODE -ne 0) {
        throw "Failed to create service '$ServiceName'."
    }

    & sc.exe description $ServiceName "Singularity Monitor background collector service" | Out-Host
    & sc.exe failure $ServiceName reset= 86400 actions= restart/5000/restart/15000/restart/60000 | Out-Host
    & sc.exe failureflag $ServiceName 1 | Out-Host

    Write-Host "Service '$ServiceName' installed successfully."
    Write-Host "Binary: $daemonExe"
}

function Start-ServiceInternal {
    $service = Get-ServiceSafe $ServiceName
    if (-not $service) {
        throw "Service '$ServiceName' does not exist. Install it first."
    }

    if ($service.Status -eq [System.ServiceProcess.ServiceControllerStatus]::Running) {
        Write-Host "Service '$ServiceName' is already running."
        return
    }

    Start-Service -Name $ServiceName
    Write-Host "Service '$ServiceName' started."
}

function Stop-ServiceInternal {
    $service = Get-ServiceSafe $ServiceName
    if (-not $service) {
        Write-Host "Service '$ServiceName' is not installed."
        return
    }

    if ($service.Status -eq [System.ServiceProcess.ServiceControllerStatus]::Stopped) {
        Write-Host "Service '$ServiceName' is already stopped."
        return
    }

    Stop-Service -Name $ServiceName -Force
    Write-Host "Service '$ServiceName' stopped."
}

function Restart-ServiceInternal {
    $service = Get-ServiceSafe $ServiceName
    if (-not $service) {
        throw "Service '$ServiceName' does not exist. Install it first."
    }

    if ($service.Status -ne [System.ServiceProcess.ServiceControllerStatus]::Stopped) {
        Stop-Service -Name $ServiceName -Force
    }
    Start-Sleep -Milliseconds 500
    Start-Service -Name $ServiceName
    Write-Host "Service '$ServiceName' restarted."
}

function Uninstall-ServiceInternal {
    $service = Get-ServiceSafe $ServiceName
    if (-not $service) {
        Write-Host "Service '$ServiceName' is not installed."
        return
    }

    if ($service.Status -ne [System.ServiceProcess.ServiceControllerStatus]::Stopped) {
        Stop-Service -Name $ServiceName -Force
        Start-Sleep -Milliseconds 500
    }

    & sc.exe delete $ServiceName | Out-Host
    if ($LASTEXITCODE -ne 0) {
        throw "Failed to delete service '$ServiceName'."
    }

    Write-Host "Service '$ServiceName' deleted."
}

function Show-ServiceStatusInternal {
    $service = Get-ServiceSafe $ServiceName
    if (-not $service) {
        Write-Host "Service '$ServiceName' is not installed."
        return
    }

    $svc = Get-CimInstance Win32_Service -Filter "Name='$ServiceName'"
    Write-Host "Name:        $($svc.Name)"
    Write-Host "DisplayName: $($svc.DisplayName)"
    Write-Host "State:       $($svc.State)"
    Write-Host "StartMode:   $($svc.StartMode)"
    Write-Host "StartName:   $($svc.StartName)"
    Write-Host "PathName:    $($svc.PathName)"
}

if ($Action -ne "status") {
    Assert-Administrator
}

switch ($Action) {
    "install" { Install-ServiceInternal }
    "uninstall" { Uninstall-ServiceInternal }
    "start" { Start-ServiceInternal }
    "stop" { Stop-ServiceInternal }
    "restart" { Restart-ServiceInternal }
    "status" { Show-ServiceStatusInternal }
    default { throw "Unsupported action '$Action'" }
}
