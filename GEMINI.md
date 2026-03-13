# Singularity Monitor - Project Context

Windows 11 network-usage monitor built for ultra-low steady-state overhead (≤5MB RAM) and high accuracy (≤0.1% deviation from OS counters).

## Project Overview

Singularity Monitor uses a **differential polling** architecture to track network usage without packet sniffing or kernel drivers. It periodically computes byte-level deltas from native Windows APIs (`GetIfTable2`, `GetNetworkUsageList`) and stores them in a local SQLite database for long-term analytics.

### Core Architecture
- **Daemon (`crates/daemon`)**: A headless Rust service that handles polling, SQLite persistence, and IPC. Targets ≤5MB RAM and ≤0.2% CPU.
- **Helper (`crates/helper`)**: A Rust probe that uses WinRT APIs to attribute usage to specific user-session applications.
- **Shared Contracts (`crates/shared-contracts`)**: A Rust crate defining the newline-delimited JSON IPC protocol and DTOs.
- **Viewer (`viewer`)**: A WinUI 3 (.NET 8) desktop application for dashboarding, analytics, and settings.
- **Storage**: Local SQLite database in `%LOCALAPPDATA%\SingularityMonitor\data.db`.
- **IPC**: Named Pipe at `\\.\pipe\SingularityMonitor`.

## Key Technologies
- **Rust**: Systems-level performance and memory safety for the collector.
- **WinUI 3 (.NET 8)**: Native Windows 11 Fluent Design interface.
- **SQLite**: ACID-compliant local storage.
- **Named Pipes**: Low-latency Windows-native IPC.
- **Windows APIs**: `GetIfTable2`, `GetNetworkUsageList`, `WinRT`, `WTSQuerySessionInformation`.

## Development Workflows

### Build Commands
- **Build All**: `scripts\build-all.cmd` (Full Rust + Viewer release)
- **Rust Workspace**: `scripts\build-rust.cmd [--release]`
- **Viewer Only**: `scripts\build-viewer.cmd [--msix]`

### Running & Service Lifecycle (Elevated)
- **Console Mode (Daemon)**: `scripts\run-daemon-console.cmd`
- **Install/Start/Stop Service**: `scripts\service-[install|start|stop].cmd`
- **Helper Loop**: `scripts\run-helper-loop.cmd`
- **Import History**: `scripts\import-history.cmd`

### Verification & Testing
- **Rust Tests**: `cargo test --workspace`
- **Smoke Validation (PowerShell)**:
  - Feasibility: `scripts\m0-feasibility.ps1`
  - Attribution: `scripts\m1-attribution-smoke.ps1`
  - Accuracy: `scripts\p0-07-accuracy-smoke.ps1`
  - Settings/Hot-Reload: `scripts\m4-settings-hotreload-smoke.ps1`

## Development Conventions

### General Rules
- **Windows 11 Only**: No cross-platform abstractions unless necessary.
- **Low Overhead**: Every allocation in the daemon must be justified.
- **Data Integrity**: Never record negative deltas; handle counter wraps gracefully.
- **Backward Compatibility**: Keep IPC contracts and SQLite schemas additive.

### Rust Coding Style
- **Edition**: 2024.
- **Error Handling**: `anyhow::Result` for boundaries; avoid `panic!`.
- **Naming**: `snake_case` for functions/variables; `PascalCase` for types.
- **Units**: Use explicit suffixes like `_ts`, `_secs`, `_bytes`.
- **IPC**: Define all methods and DTOs in `shared-contracts` first.

### C# / WinUI Coding Style
- **Framework**: .NET 8, WinUI 3 (Windows App SDK).
- **Naming**: `PascalCase` for public members; `camelCase` for private fields.
- **Async**: Use `Async` suffix; `async void` only for event handlers.
- **IPC**: Map `snake_case` JSON fields to `PascalCase` properties using `[JsonPropertyName]`.

## Directory Map
- `crates/daemon/`: Collector service and DB engine.
- `crates/helper/`: Attribution probe (WinRT).
- `crates/shared-contracts/`: IPC schemas and message framing.
- `viewer/`: WinUI 3 application source.
- `docs/`: PRD, status, and user/developer guides.
- `scripts/`: Build, run, and smoke test automation.
- `packaging/`: Winget manifests and MSIX assets.

## Important Documentation
- **PRD**: `docs/singularity-monitor-prd.md` (Vision and technical specs)
- **Agent Rules**: `AGENTS.md` (Detailed coding and toolchain guidance)
- **Status**: `docs/implementation-status.md` (Latest feature progress)
- **IPC Protocol**: Defined in `crates/shared-contracts/src/lib.rs`
