param(
    [string]$ProjectPath = "viewer\SingularityMonitor.Viewer.csproj",
    [string]$ManifestPath = "viewer\Package.appxmanifest",
    [ValidateSet("win-x64", "win-arm64", "win-x86")]
    [string]$RuntimeIdentifier = "win-x64",
    [string]$Configuration = "Release",
    [string]$Version = "1.0.0.0",
    [string]$Publisher = "CN=SingularityMonitor",
    [string]$PackageDir = "viewer\AppPackages\Release",
    [switch]$BundleHelperRelease,
    [switch]$Sign,
    [string]$CertificateBase64,
    [string]$CertificatePassword,
    [string]$TimestampUrl = "http://timestamp.digicert.com",
    [string]$OutputMetadataPath
)

$ErrorActionPreference = "Stop"

function Resolve-ProjectRoot {
    return (Resolve-Path (Join-Path $PSScriptRoot "..")).Path
}

function Get-PlatformFromRid([string]$Rid) {
    switch ($Rid) {
        "win-x64" { return "x64" }
        "win-arm64" { return "ARM64" }
        "win-x86" { return "x86" }
        default { throw "Unsupported runtime identifier '$Rid'." }
    }
}

function Write-Utf8BomFile([string]$Path, [string]$Content) {
    $encoding = New-Object System.Text.UTF8Encoding($true)
    [System.IO.File]::WriteAllText($Path, $Content, $encoding)
}

function Resolve-SignToolPath {
    $command = Get-Command signtool.exe -ErrorAction SilentlyContinue
    if ($command) {
        return $command.Source
    }

    $kitsRoot = Join-Path ${env:ProgramFiles(x86)} "Windows Kits\10\bin"
    if (-not (Test-Path $kitsRoot)) {
        throw "signtool.exe was not found on PATH and Windows Kits bin directory is missing."
    }

    $candidate = Get-ChildItem -Path $kitsRoot -Filter signtool.exe -Recurse |
        Where-Object { $_.FullName -match "\\x64\\signtool\.exe$" } |
        Sort-Object FullName -Descending |
        Select-Object -First 1

    if (-not $candidate) {
        throw "signtool.exe was not found under '$kitsRoot'."
    }

    return $candidate.FullName
}

$root = Resolve-ProjectRoot
$project = Resolve-Path (Join-Path $root $ProjectPath)
$manifest = Resolve-Path (Join-Path $root $ManifestPath)
$packageRoot = Join-Path $root $PackageDir
$platform = Get-PlatformFromRid $RuntimeIdentifier
$publishProfile = "win-$platform.pubxml"
$metadata = @{}
$tempCertificatePath = $null
$originalManifest = Get-Content $manifest -Raw

try {
    New-Item -ItemType Directory -Force -Path $packageRoot | Out-Null

    if ($BundleHelperRelease.IsPresent) {
        & cargo build -p helper --release
        if ($LASTEXITCODE -ne 0) {
            throw "Failed to build helper release binary."
        }
    }

    [xml]$manifestXml = Get-Content $manifest -Raw
    $identity = $manifestXml.Package.Identity
    $identity.Publisher = $Publisher
    $identity.Version = $Version
    $manifestXml.Save($manifest)

    $publishArgs = @(
        "publish",
        $project.Path,
        "-c",
        $Configuration,
        "-p:Platform=$platform",
        "-p:PublishProfile=$publishProfile",
        "-p:RuntimeIdentifier=$RuntimeIdentifier",
        "-p:WindowsPackageType=MSIX",
        "-p:GenerateAppxPackageOnBuild=true",
        "-p:AppxPackageSigningEnabled=false",
        "-p:UapAppxPackageBuildMode=SideloadOnly",
        "-p:AppxBundle=Never",
        "-p:AppxPackageDir=$([System.IO.Path]::GetFullPath($packageRoot))\\"
    )

    & dotnet @publishArgs
    if ($LASTEXITCODE -ne 0) {
        throw "dotnet publish failed for MSIX packaging."
    }

    $msix = Get-ChildItem -Path $packageRoot -Filter *.msix -Recurse |
        Sort-Object LastWriteTime -Descending |
        Select-Object -First 1
    if (-not $msix) {
        throw "No MSIX package was produced under '$packageRoot'."
    }

    $metadata = [ordered]@{
        MsixPath = $msix.FullName
        MsixName = $msix.Name
        RuntimeIdentifier = $RuntimeIdentifier
        Platform = $platform
        Version = $Version
        Publisher = $Publisher
        Signed = $false
        HelperBundled = Test-Path (Join-Path $root "target\release\helper.exe")
    }

    if ($Sign.IsPresent) {
        if ([string]::IsNullOrWhiteSpace($CertificateBase64)) {
            throw "-Sign requires -CertificateBase64."
        }
        if ([string]::IsNullOrWhiteSpace($CertificatePassword)) {
            throw "-Sign requires -CertificatePassword."
        }

        $tempCertificatePath = Join-Path ([System.IO.Path]::GetTempPath()) ("singularity-monitor-" + [System.Guid]::NewGuid().ToString("N") + ".pfx")
        [System.IO.File]::WriteAllBytes($tempCertificatePath, [Convert]::FromBase64String($CertificateBase64))

        $signtool = Resolve-SignToolPath
        $signArgs = @("sign", "/fd", "SHA256", "/f", $tempCertificatePath, "/p", $CertificatePassword)
        if (-not [string]::IsNullOrWhiteSpace($TimestampUrl)) {
            $signArgs += @("/tr", $TimestampUrl, "/td", "SHA256")
        }
        $signArgs += $msix.FullName

        & $signtool @signArgs
        if ($LASTEXITCODE -ne 0) {
            throw "signtool sign failed for '$($msix.FullName)'."
        }

        & $signtool verify /pa /v $msix.FullName
        if ($LASTEXITCODE -ne 0) {
            throw "signtool verify failed for '$($msix.FullName)'."
        }

        $metadata.Signed = $true
    }

    if (-not [string]::IsNullOrWhiteSpace($OutputMetadataPath)) {
        $metadataPath = Join-Path $root $OutputMetadataPath
        $metadataDirectory = Split-Path -Parent $metadataPath
        if (-not [string]::IsNullOrWhiteSpace($metadataDirectory)) {
            New-Item -ItemType Directory -Force -Path $metadataDirectory | Out-Null
        }
        $metadata | ConvertTo-Json -Depth 3 | Set-Content -Path $metadataPath -Encoding utf8
    }

    $metadata | ConvertTo-Json -Depth 3
}
finally {
    Write-Utf8BomFile -Path $manifest -Content $originalManifest
    if ($tempCertificatePath -and (Test-Path $tempCertificatePath)) {
        Remove-Item $tempCertificatePath -Force
    }
}
