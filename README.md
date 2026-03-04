# Singularity Monitor

Windows 11 network-usage monitor built for low steady-state overhead.

## Repository layout

- `crates/daemon` - Rust collector service (GetIfTable2 polling, SQLite write path, named-pipe IPC)
- `crates/helper` - Rust user-session attribution probe (WinRT usage APIs)
- `crates/perf-harness` - Rust memory sampler for KPI gates
- `crates/shared-contracts` - shared JSON IPC envelope and payload types
- `viewer` - WinUI 3 (.NET 8) viewer
- `scripts` - build and run helpers for local development

## Build

```bat
scripts\build-rust.cmd --release
scripts\build-viewer.cmd
```

Or build everything:

```bat
scripts\build-all.cmd
```

Run M0 feasibility checks (helper probe + pipe + memory gate):

```powershell
powershell -ExecutionPolicy Bypass -File scripts\m0-feasibility.ps1
```

Run M1 attribution smoke test (helper -> daemon ingest -> app breakdown):

```powershell
powershell -ExecutionPolicy Bypass -File scripts\m1-attribution-smoke.ps1
```

Run M2 overlap dedupe smoke test (import/live source cutover validation):

```powershell
powershell -ExecutionPolicy Bypass -File scripts\m2-overlap-dedupe-smoke.ps1
```

Run M4 settings hot-reload smoke test (`SET_SETTINGS` + `GET_SETTINGS` + daemon status):

```powershell
powershell -ExecutionPolicy Bypass -File scripts\m4-settings-hotreload-smoke.ps1
```

## Run daemon in console mode

```bat
scripts\run-daemon-console.cmd
```

## Service lifecycle (elevated shell)

```bat
scripts\service-install.cmd
scripts\service-start.cmd
scripts\service-status.cmd
scripts\service-stop.cmd
scripts\service-restart.cmd
scripts\service-uninstall.cmd
```

All service commands route through `scripts\service-daemon.ps1` and require Administrator privileges.
`status` can be queried without elevation.

Run helper ingestion loop:

```bat
scripts\run-helper-loop.cmd
```

Run 60-day history import via helper:

```bat
scripts\import-history.cmd
```

One-shot helper modes:

```bat
cargo run -p helper -- --probe --window-secs 300
cargo run -p helper -- --push-once --window-secs 300
cargo run -p helper -- --import-history --days 60 --chunk-hours 6
```

The script sets `SM_DATA_ROOT` to `runtime-data` in this repository to avoid writing into system locations during local development.

## IPC

- Named pipe: `\\.\pipe\SingularityMonitor`
- Message framing: newline-delimited JSON
- Methods currently wired:
  - `GET_DAEMON_STATUS`
  - `GET_SETTINGS`
  - `GET_USAGE_SUMMARY`
  - `GET_APP_BREAKDOWN`
  - `GET_INTERFACES`
  - `GET_INTERFACE_BREAKDOWN`
  - `SET_SETTINGS`
  - `SET_IMPORT_STATUS`
  - `GET_AFK_AUDIT`
  - `INGEST_ATTRIBUTED_USAGE` (helper internal)

## Current implementation status

- Differential interface polling pipeline is active.
- SQLite schema and write path are active.
- WinUI viewer can query daemon status, helper ingest recency, import progress, day/week/month totals, and top app breakdown with date range.
- WinUI viewer supports interface filter selection, interface breakdown list, and CSV/JSON exports for the selected range.
- WinUI viewer includes top-app sort/group controls and app detail drill-down buckets.
- WinUI viewer includes collector settings controls (poll interval, retention, AFK threshold) with apply/reset wiring.
- Helper can now push WinRT attributed usage snapshots into daemon storage.
- Helper supports chunked 60-day import mode for initial backfill.
- Import progress is tracked through daemon status (`import_status`, `import_progress_pct`).
- Viewer auto-registers helper loop at user logon (HKCU Run key) and attempts to start helper on launch.
- Daemon supports optional working-set trim (`SM_TRIM_WORKING_SET=1`, default enabled) to keep steady-state RSS within the PRD envelope.
- Daemon query path now enforces source-cutover precedence to avoid overlap double-counting between import and live collection streams.

Next milestones are full P0 dashboard/export parity, AFK/event overlays, alerting, and analytics completion from the PRD.
