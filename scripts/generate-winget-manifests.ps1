param(
    [Parameter(Mandatory = $true)]
    [string]$MsixPath,
    [Parameter(Mandatory = $true)]
    [string]$InstallerUrl,
    [ValidateSet("x64", "ARM64", "x86")]
    [string]$Architecture = "x64",
    [string]$PackageIdentifier = "SingularityMonitor.Viewer",
    [string]$PackageVersion,
    [string]$PackageLocale = "en-US",
    [string]$Publisher = "Singularity Monitor",
    [string]$PackageName = "Singularity Monitor",
    [string]$License = "Proprietary",
    [string]$ShortDescription = "Low-overhead network monitor for Windows 11.",
    [string]$OutputRoot = "packaging\winget\generated",
    [string]$ManifestVersion = "1.9.0",
    [switch]$AllowUnsignedMsix
)

$ErrorActionPreference = "Stop"

function Resolve-ProjectRoot {
    return (Resolve-Path (Join-Path $PSScriptRoot "..")).Path
}

function Get-PackageFamilyName([string]$Name, [string]$PublisherValue) {
    if ([string]::IsNullOrWhiteSpace($Name) -or [string]::IsNullOrWhiteSpace($PublisherValue)) {
        throw "Package family name requires both identity name and publisher."
    }

    $bytes = [System.Text.Encoding]::Unicode.GetBytes($PublisherValue)
    $sha256 = [System.Security.Cryptography.SHA256]::Create()
    try {
        $hash = $sha256.ComputeHash($bytes)
    }
    finally {
        $sha256.Dispose()
    }

    $alphabet = "abcdefghijklmnopqrstuvwxyz234567"
    $builder = New-Object System.Text.StringBuilder
    $buffer = 0
    $bitsLeft = 0

    foreach ($byte in $hash) {
        $buffer = ($buffer -shl 8) -bor $byte
        $bitsLeft += 8
        while ($bitsLeft -ge 5) {
            $index = ($buffer -shr ($bitsLeft - 5)) -band 31
            [void]$builder.Append($alphabet[$index])
            $bitsLeft -= 5
        }
    }

    if ($bitsLeft -gt 0) {
        $index = ($buffer -shl (5 - $bitsLeft)) -band 31
        [void]$builder.Append($alphabet[$index])
    }

    $publisherHash = $builder.ToString().Substring(0, 13)
    return "${Name}_$publisherHash"
}

function Get-SignatureSha256([string]$PackagePath, [switch]$AllowUnsigned) {
    if ($AllowUnsigned) {
        return $null
    }

    if (-not (Get-Command winget -ErrorAction SilentlyContinue)) {
        throw "winget is required to compute SignatureSha256."
    }

    $output = & winget hash --msix $PackagePath 2>&1
    if ($LASTEXITCODE -ne 0) {
        throw "winget hash --msix failed: $output"
    }

    foreach ($line in $output) {
        if ($line -match "SignatureSha256:\s*(?<hash>[A-Fa-f0-9]{64})") {
            return $matches.hash.ToUpperInvariant()
        }
    }

    throw "winget hash --msix did not return SignatureSha256."
}

$root = Resolve-ProjectRoot
$resolvedMsix = Resolve-Path (Join-Path $root $MsixPath)
$tempDir = Join-Path ([System.IO.Path]::GetTempPath()) ([System.Guid]::NewGuid().ToString("N"))
$archivePath = Join-Path ([System.IO.Path]::GetTempPath()) ([System.Guid]::NewGuid().ToString("N") + ".zip")

try {
    Copy-Item -Path $resolvedMsix.Path -Destination $archivePath -Force
    Expand-Archive -Path $archivePath -DestinationPath $tempDir -Force
    [xml]$manifest = Get-Content (Join-Path $tempDir "AppxManifest.xml")
    $identity = $manifest.Package.Identity

    if ([string]::IsNullOrWhiteSpace($PackageVersion)) {
        $PackageVersion = $identity.Version
    }

    $packageFamilyName = Get-PackageFamilyName -Name $identity.Name -PublisherValue $identity.Publisher
    $installerHash = (Get-FileHash -Path $resolvedMsix.Path -Algorithm SHA256).Hash.ToUpperInvariant()
    $signatureHash = Get-SignatureSha256 -PackagePath $resolvedMsix.Path -AllowUnsigned:$AllowUnsignedMsix

    $outputDir = Join-Path $root (Join-Path $OutputRoot (Join-Path $PackageIdentifier $PackageVersion))
    New-Item -ItemType Directory -Force -Path $outputDir | Out-Null

    $versionManifest = @"
# yaml-language-server: `$schema=https://aka.ms/winget-manifest.version.$ManifestVersion.schema.json
PackageIdentifier: $PackageIdentifier
PackageVersion: $PackageVersion
DefaultLocale: $PackageLocale
ManifestType: version
ManifestVersion: $ManifestVersion
"@

    $defaultLocaleManifest = @"
# yaml-language-server: `$schema=https://aka.ms/winget-manifest.defaultLocale.$ManifestVersion.schema.json
PackageIdentifier: $PackageIdentifier
PackageVersion: $PackageVersion
PackageLocale: $PackageLocale
Publisher: $Publisher
PackageName: $PackageName
License: $License
ShortDescription: $ShortDescription
ManifestType: defaultLocale
ManifestVersion: $ManifestVersion
"@

    $installerLines = @(
        "# yaml-language-server: `$schema=https://aka.ms/winget-manifest.installer.$ManifestVersion.schema.json",
        "PackageIdentifier: $PackageIdentifier",
        "PackageVersion: $PackageVersion",
        "Platform:",
        "- Windows.Desktop",
        "MinimumOSVersion: 10.0.17763.0",
        "InstallModes:",
        "- interactive",
        "- silent",
        "Installers:",
        "- Architecture: $Architecture",
        "  InstallerType: msix",
        "  InstallerUrl: $InstallerUrl",
        "  InstallerSha256: $installerHash",
        "  PackageFamilyName: $packageFamilyName"
    )
    if ($signatureHash) {
        $installerLines += "  SignatureSha256: $signatureHash"
    }
    $installerLines += @(
        "ManifestType: installer",
        "ManifestVersion: $ManifestVersion"
    )

    Set-Content -Path (Join-Path $outputDir "$PackageIdentifier.yaml") -Value $versionManifest -Encoding utf8
    Set-Content -Path (Join-Path $outputDir "$PackageIdentifier.locale.$PackageLocale.yaml") -Value $defaultLocaleManifest -Encoding utf8
    Set-Content -Path (Join-Path $outputDir "$PackageIdentifier.installer.yaml") -Value ($installerLines -join [Environment]::NewLine) -Encoding utf8

    [ordered]@{
        OutputDir = $outputDir
        PackageIdentifier = $PackageIdentifier
        PackageVersion = $PackageVersion
        PackageFamilyName = $packageFamilyName
        InstallerSha256 = $installerHash
        SignatureSha256 = $signatureHash
    } | ConvertTo-Json -Depth 3
}
finally {
    if (Test-Path $tempDir) {
        Remove-Item $tempDir -Recurse -Force
    }
    if (Test-Path $archivePath) {
        Remove-Item $archivePath -Force
    }
}
