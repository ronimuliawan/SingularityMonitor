# Install Guide

## Requirements

- Windows 11
- Administrator rights to install or update the daemon service
- A signed MSIX package for end-user viewer installs

## What gets installed today

The current release flow packages the WinUI viewer as an MSIX and bundles `helper.exe` with the viewer package.
The Rust daemon still runs as a Windows service and is installed separately with the repo service scripts.

## Option 1: Install the viewer MSIX

1. Download the latest `.msix` release asset.
2. If Windows warns about sideloading, install the trusted signing certificate chain or use the signed production package.
3. Install the package by double-clicking it or by using the generated `Install.ps1` in the package output folder.
4. Launch `Singularity Monitor` from the Start menu.

## Option 2: Install the daemon service

Open an elevated terminal in the repository root and run:

```bat
scripts\build-rust.cmd --release
scripts\service-install.cmd
scripts\service-start.cmd
```

Check the service state with:

```bat
scripts\service-status.cmd
```

## First-run verification

After both viewer and daemon are installed:

1. Open the viewer.
2. Confirm the `Daemon Status` card shows a live connection.
3. If this is the first launch, run the 60-day import from the onboarding card.
4. Confirm current-day usage appears in the dashboard cards and tray tooltip.

If you are validating a release build, record the result in `docs\qa-matrix.md`.

## Update flow

- Viewer MSIX: install the newer signed `.msix` over the existing package.
- Daemon service: rebuild the daemon, then run `scripts\service-restart.cmd` from an elevated shell.

## Uninstall flow

- Viewer MSIX: uninstall `Singularity Monitor` from Settings > Apps or with `winget uninstall SingularityMonitor.Viewer` once the public package is published.
- Daemon service: run `scripts\service-stop.cmd` and `scripts\service-uninstall.cmd` from an elevated shell.

## Data and log locations

- Daemon data root: `%ProgramData%\SingularityMonitor`
- Daemon log: `%ProgramData%\SingularityMonitor\daemon.log.jsonl`
- Helper reliability log: `%ProgramData%\SingularityMonitor\helper-reliability.jsonl`
- Viewer reliability log: `%LocalAppData%\SingularityMonitor\viewer-reliability.jsonl`

The uninstall flow currently does not delete collected history automatically.
