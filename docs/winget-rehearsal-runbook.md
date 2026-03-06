# Signed Winget Rehearsal Runbook

## Purpose

Use this runbook on a disposable Windows 11 machine to rehearse the signed viewer lifecycle through `winget install`, `winget upgrade`, and `winget uninstall` before public release.

## Use this only on a disposable test machine

- Start from a clean VM snapshot or a machine that can be wiped after testing.
- This flow changes certificate trust and package-install state.
- The daemon service is still installed separately from the viewer MSIX.

## Required inputs

- Windows 11 test machine with administrator rights
- `winget` installed and working
- Repository checkout with this documentation and the helper/daemon scripts
- Two signed viewer MSIX artifacts with the same package identity and increasing versions, for example:
  - `SingularityMonitor.Viewer_1.0.0.0_x64.msix`
  - `SingularityMonitor.Viewer_1.0.1.0_x64.msix`
- Trusted signing certificate chain for the MSIX publisher
- Local HTTP hosting option for the signed packages or a stable internal URL

## Pre-flight checklist

- Confirm the machine snapshot or rollback plan is ready.
- Confirm both viewer packages are signed and trusted:

```powershell
signtool verify /pa /v "<path-to-v1-msix>"
signtool verify /pa /v "<path-to-v2-msix>"
Get-AuthenticodeSignature "<path-to-v1-msix>" | Format-List Status, SignerCertificate, TimeStamperCertificate
```

- Confirm the second package version is higher and the publisher identity is unchanged.
- Enable local manifest support once:

```powershell
winget settings --enable LocalManifestFiles
```

- Confirm the daemon service is not already installed from a previous test run:

```bat
scripts\service-status.cmd
```

- Start a local HTTP server or choose the exact installer URLs that the manifest will use.

## Example installer URLs

- `http://127.0.0.1:8080/SingularityMonitor.Viewer_1.0.0.0_x64.msix`
- `http://127.0.0.1:8080/SingularityMonitor.Viewer_1.0.1.0_x64.msix`

## Step sequence

### 1. Start local package hosting

Example with Python:

```powershell
py -m http.server 8080 --bind 127.0.0.1 --directory "<folder-containing-signed-msix-files>"
```

### 2. Install the daemon service

Run from an elevated terminal:

```bat
scripts\build-rust.cmd --release
scripts\service-install.cmd
scripts\service-start.cmd
scripts\service-status.cmd
```

### 3. Generate and validate version 1 manifests

```powershell
powershell -ExecutionPolicy Bypass -File scripts\generate-winget-manifests.ps1 `
  -MsixPath "<path-to-v1-msix>" `
  -InstallerUrl "http://127.0.0.1:8080/<v1-msix-name>" `
  -Architecture x64 `
  -PackageVersion 1.0.0.0

powershell -ExecutionPolicy Bypass -File scripts\validate-winget.ps1 `
  -ManifestRoot "packaging\winget\generated\SingularityMonitor.Viewer\1.0.0.0"
```

### 4. Install version 1 with winget

```powershell
winget install --manifest "packaging\winget\generated\SingularityMonitor.Viewer\1.0.0.0"
```

### 5. Validate version 1 install

- `winget list SingularityMonitor.Viewer` shows version `1.0.0.0`
- The Start menu shows `Singularity Monitor`
- The viewer launches and connects to the daemon
- The tray icon and dashboard load without startup errors

Record the results in `docs\qa-matrix.md`.

### 6. Generate and validate version 2 manifests

```powershell
powershell -ExecutionPolicy Bypass -File scripts\generate-winget-manifests.ps1 `
  -MsixPath "<path-to-v2-msix>" `
  -InstallerUrl "http://127.0.0.1:8080/<v2-msix-name>" `
  -Architecture x64 `
  -PackageVersion 1.0.1.0

powershell -ExecutionPolicy Bypass -File scripts\validate-winget.ps1 `
  -ManifestRoot "packaging\winget\generated\SingularityMonitor.Viewer\1.0.1.0"
```

### 7. Upgrade to version 2

```powershell
winget upgrade --manifest "packaging\winget\generated\SingularityMonitor.Viewer\1.0.1.0"
```

### 8. Validate upgrade

- `winget list SingularityMonitor.Viewer` shows version `1.0.1.0`
- The viewer upgrades in place instead of appearing as a second package
- The upgraded viewer still connects to the already-installed daemon
- First-run or settings state is preserved as expected

Record the results in `docs\qa-matrix.md`.

### 9. Uninstall the viewer package

```powershell
winget uninstall --id SingularityMonitor.Viewer
```

### 10. Validate uninstall and remove the daemon

- `winget list SingularityMonitor.Viewer` no longer shows the package
- The Start menu entry is gone
- The daemon service still exists until you remove it separately, which is expected

Then remove the daemon:

```bat
scripts\service-stop.cmd
scripts\service-uninstall.cmd
```

## Cleanup and rollback

Preferred cleanup:

- Revert the machine snapshot or VM checkpoint

Manual cleanup if snapshot rollback is not used:

- Uninstall the viewer package
- Stop and remove the daemon service
- Remove temporary trusted certificates that were added only for rehearsal
- Stop the local HTTP server
- Delete generated manifests under `packaging\winget\generated\SingularityMonitor.Viewer\`
- Remove local data if a true clean state is required:
  - `%ProgramData%\SingularityMonitor`
  - `%LocalAppData%\SingularityMonitor\viewer-reliability.jsonl`

## Common failure points

- Certificate trust is missing or incomplete
- MSIX publisher changed between versions, breaking identity continuity
- Package version was not incremented
- Local manifest support was not enabled
- `InstallerUrl` points to the wrong file or an unavailable HTTP server
- Manifest hashes were generated before the final signed package was produced
- Daemon service scripts were run without elevation

## Evidence to record

For each run, record the following in `docs\qa-matrix.md`:

- Machine identity and Windows build
- Exact versions tested
- Install, upgrade, and uninstall result
- Any screenshots or artifact URLs
- Relevant log paths for failures
- Final tester sign-off
