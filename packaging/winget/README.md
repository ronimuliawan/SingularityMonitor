# Winget Packaging Notes

This directory stores generated winget manifests and the release-time instructions needed to validate them.

## Current stop point

- Manifest generation and `winget validate` are wired and verified.
- The signed localhost install/upgrade/uninstall rehearsal is intentionally deferred because it requires a trusted signing certificate and changes the local machine state.
- Do not run the local rehearsal on this workstation unless you explicitly want to modify certificate trust and package-install state.
- Use `docs\winget-rehearsal-runbook.md` plus `docs\qa-matrix.md` when you are ready to execute the signed lifecycle test on a disposable machine.

## Generate manifests from a built MSIX

```powershell
powershell -ExecutionPolicy Bypass -File scripts\generate-winget-manifests.ps1 `
  -MsixPath viewer\AppPackages\Release\SingularityMonitor.Viewer_1.0.0.0_x64.msix `
  -InstallerUrl https://github.com/ronimuliawan/SingularityMonitor/releases/download/v1.0.0.0/SingularityMonitor.Viewer_1.0.0.0_x64.msix `
  -Architecture x64 `
  -PackageVersion 1.0.0.0
```

Generated output is written under `packaging\winget\generated\SingularityMonitor.Viewer\<version>`.

## Validate generated manifests

```powershell
powershell -ExecutionPolicy Bypass -File scripts\validate-winget.ps1 -ManifestRoot packaging\winget\generated\SingularityMonitor.Viewer\1.0.0.0
```

## Local install, upgrade, and uninstall rehearsal

Use a signed MSIX and a trusted certificate before running these steps.

1. Enable local manifest support once:

```powershell
winget settings --enable LocalManifestFiles
```

2. Serve the signed MSIX from a local HTTP endpoint such as `http://127.0.0.1:8080/<package>.msix`.
3. Generate manifests that reference the localhost URL.
4. Rehearse the package lifecycle:

```powershell
winget install --manifest <manifest-folder>
winget upgrade --manifest <next-version-manifest-folder>
winget uninstall --id SingularityMonitor.Viewer
```

Record every result and captured log path in `docs\qa-matrix.md`.

## Required final-release metadata

- Public `InstallerUrl`
- `InstallerSha256`
- `PackageFamilyName`
- `SignatureSha256` for signed MSIX packages
- Stable `PackageIdentifier`, version, and publisher identity
