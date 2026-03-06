# QA Matrix and Runbook

## Purpose

This document tracks release validation for `R-10` and doubles as the per-machine worksheet used during final release testing.

## Target environment inventory

| Windows build | Hardware profile | Install path | Status | Notes |
|---|---|---|---|---|
| 24H2 | Desktop / dev workstation | Repo build + service scripts | PASS | Current workspace baseline completed on 2026-03-06 |
| 24H2 | Laptop / battery-managed | Signed MSIX + service install | TODO | Validate sleep/resume, tray, import, and battery behavior |
| 23H2 | Desktop / SSD | Signed MSIX + service install | TODO | Validate install, export, and cap alerts |
| 22H2 | Lower-spec device | Signed MSIX + service install | TODO | Validate startup, responsiveness, and memory hints |

## How to use this document

- Fill one worksheet section per machine under test.
- If the machine is validating signed `winget` lifecycle behavior, also follow `docs/winget-rehearsal-runbook.md`.
- Record the exact command, symptom, and log path for every failure.
- Attach screenshot paths, release asset URLs, or artifact IDs in the evidence fields.

## Machine under test

- Machine name:
- Tester / owner:
- Test date:
- Windows edition / build:
- Device type:
- CPU / RAM:
- Battery-managed: Yes / No
- Install path under test: `repo build` / `signed MSIX` / `winget`
- Viewer artifact version:
- Daemon artifact version:
- Certificate publisher:
- Additional notes:

## Run order

1. Snapshot or checkpoint the machine.
2. Complete the pre-flight checklist.
3. Install or update the daemon and viewer artifacts.
4. Run the automated checks that apply to this machine.
5. Run the manual workflow checks.
6. Run packaging lifecycle checks if this machine covers signed MSIX or `winget`.
7. Capture logs, screenshots, and final disposition.

## Pre-flight checklist

| Step | Expected result | Status | Notes |
|---|---|---|---|
| Clean machine snapshot available | Machine can be reverted after testing |  |  |
| Admin rights confirmed | Service install and certificate trust changes are allowed |  |  |
| `winget --version` checked | Local manifest support is available when needed |  |  |
| Viewer artifact(s) present | Correct version(s) copied to the test machine |  |  |
| Daemon artifact present | Service install can proceed |  |  |
| Signing cert trusted | Signed MSIX shows as trusted |  |  |
| Localhost or public installer URL ready | Installer downloads succeed |  |  |
| Existing viewer package state recorded | Fresh install vs upgrade path is explicit |  |  |
| Existing daemon service state recorded | Clean service install or upgrade path is explicit |  |  |

## Automated checks

| Step | Command | Expected result | Status | Evidence / notes |
|---|---|---|---|---|
| Rust tests | `cargo test --workspace` | All tests pass |  |  |
| Full build | `scripts\build-all.cmd` | Rust + viewer builds succeed |  |  |
| Performance gates | `powershell -ExecutionPolicy Bypass -File scripts\r-02-performance-gates.ps1` | All gates pass |  |  |
| Feasibility smoke | `powershell -ExecutionPolicy Bypass -File scripts\m0-feasibility.ps1` | Pass |  |  |
| Attribution smoke | `powershell -ExecutionPolicy Bypass -File scripts\m1-attribution-smoke.ps1` | Pass |  |  |
| Overlap dedupe | `powershell -ExecutionPolicy Bypass -File scripts\m2-overlap-dedupe-smoke.ps1` | Pass |  |  |
| Metered flags | `powershell -ExecutionPolicy Bypass -File scripts\m3-metered-flag-smoke.ps1` | Pass |  |  |
| Sleep/resume | `powershell -ExecutionPolicy Bypass -File scripts\m3-sleep-resume-continuity-smoke.ps1` | Pass |  |  |
| Settings hot reload | `powershell -ExecutionPolicy Bypass -File scripts\m4-settings-hotreload-smoke.ps1` | Pass |  |  |
| Accuracy gate | `powershell -ExecutionPolicy Bypass -File scripts\p0-07-accuracy-smoke.ps1` | Pass |  |  |
| Export performance | `powershell -ExecutionPolicy Bypass -File scripts\p0-16-export-perf-smoke.ps1` | Pass |  |  |
| AFK pipeline | `powershell -ExecutionPolicy Bypass -File scripts\p1-05-afk-pipeline-smoke.ps1` | Pass |  |  |

## Manual checks

| Area | Scenario | Expected result | Status | Evidence / notes |
|---|---|---|---|---|
| Install | Viewer package install | Viewer installs without trust or dependency errors |  |  |
| Service | Daemon install/start | Service is present and running |  |  |
| First run | Viewer connects to daemon | `Daemon Status` shows a live connection |  |  |
| Onboarding | Initial import trigger | Import can be started and progress is visible |  |  |
| Dashboard | Overview cards refresh | Current usage and split cards populate |  |  |
| Apps | Top apps and app detail | App list refreshes and detail panel loads |  |  |
| Export | CSV and JSON export | Files are created successfully |  |  |
| AFK | AFK timeline and selected-window apps | Panels load without errors |  |  |
| Caps / alerts | Monthly cap workflow | Save/delete/refresh behavior works |  |  |
| Tray | Close-to-tray, restore, tooltip, exit | Tray workflow works correctly |  |  |
| Offline UX | Stop daemon, then open viewer | Viewer degrades gracefully without crashing |  |  |
| Accessibility | Keyboard, Narrator, high contrast | Core workflow remains usable |  |  |

## Packaging and lifecycle checks

Complete this table when the machine covers signed MSIX or `winget` release validation.

| Flow | Versions | Expected result | Status | Evidence / notes |
|---|---|---|---|---|
| Signed MSIX install |  | Package installs cleanly |  |  |
| Signed MSIX upgrade |  | New version upgrades in place |  |  |
| Signed MSIX uninstall |  | Viewer package removes cleanly |  |  |
| `winget install` |  | Viewer installs from manifest |  |  |
| `winget upgrade` |  | Viewer upgrades from manifest |  |  |
| `winget uninstall` |  | Viewer removes from manifest identity |  |  |
| Daemon uninstall |  | Service removes cleanly after viewer testing |  |  |

## Issues and evidence

- Blocking issues:
- Accepted deviations:
- Log paths captured:
- Screenshot or artifact links:
- Follow-up owner:

## Current workspace baseline

Recorded on the active 24H2 development workstation:

- `cargo test --workspace`
- `cargo test -p daemon`
- `cargo build -p helper`
- `dotnet build "viewer\SingularityMonitor.Viewer.csproj" -c Release -p:Platform=x64`
- `powershell -ExecutionPolicy Bypass -File scripts\release-msix.ps1 -BundleHelperRelease -RuntimeIdentifier win-x64 -Version 1.0.0.0`
- `powershell -ExecutionPolicy Bypass -File scripts\generate-winget-manifests.ps1 ... -AllowUnsignedMsix`
- `powershell -ExecutionPolicy Bypass -File scripts\validate-winget.ps1 -ManifestRoot packaging\winget\generated\SingularityMonitor.Viewer\1.0.0.0`

Deliberately not executed on this workstation:

- Signed localhost `winget install/upgrade/uninstall` rehearsal, because it would modify local trust and installed-package state.
- Cross-hardware validation outside the current 24H2 workstation.

## Final result

- Overall result: PASS / FAIL / PASS WITH WAIVER
- Tester sign-off:
- Release lead disposition:
- Completion date:
