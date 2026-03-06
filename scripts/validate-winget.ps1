param(
    [Parameter(Mandatory = $true)]
    [string]$ManifestRoot
)

$ErrorActionPreference = "Stop"

$resolvedRoot = Resolve-Path $ManifestRoot
if (-not (Get-Command winget -ErrorAction SilentlyContinue)) {
    throw "winget is required to validate winget manifests."
}

& winget validate --manifest $resolvedRoot.Path
if ($LASTEXITCODE -ne 0) {
    throw "winget validate reported manifest errors."
}

Write-Host "Winget manifest validation passed: $($resolvedRoot.Path)"
