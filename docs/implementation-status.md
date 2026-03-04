# Singularity Monitor - Implementation Status

## Architecture decisions applied

- Full-scope architecture is retained (P0/P1/P2) with phased delivery.
- Service-first model implemented in daemon crate (`LocalService` target path).
- Daemon-owned data path defaults to `%ProgramData%\SingularityMonitor`.
- Settings source of truth is SQLite (`settings` table).
- Per-app source strategy is WinRT attribution through user-session helper.

## M0 feasibility checkpoints

- **Interface polling:** `GetIfTable2` collector implemented and verified through one-shot run.
- **Per-app attribution feasibility:** helper probe successfully retrieved attributed usage rows from WinRT APIs.
- **IPC feasibility:** named pipe `\\.\pipe\SingularityMonitor` request/response validated.
- **Memory gate:** release daemon sampled at ~2.35 MB working set with trim enabled (`perf-harness`).

## M1 attribution ingestion checkpoints

- **Helper-to-daemon ingestion IPC:** implemented via `INGEST_ATTRIBUTED_USAGE` on named pipe.
- **Storage path:** helper-attributed rows are persisted into `usage_records` with source `helper`.
- **App breakdown path:** `GET_APP_BREAKDOWN` now reads helper/import rows, avoiding interface-poll `System` dominance.
- **Smoke automation:** `scripts/m1-attribution-smoke.ps1` validates end-to-end ingest + query.
- **Memory under ingest load:** daemon sampled around ~2.78 MB after helper push, remaining below target.

## M2 history import and viewer orchestration

- **Chunked history import:** helper supports `--import-history --days 60 --chunk-hours 6` and tags rows with source `import`.
- **Deduplication safety:** daemon enforces unique usage keys `(ts, app_id, interface_id, source)` with upsert behavior for helper/import writes.
- **Viewer helper controls:** WinUI now provides `Start Helper Loop` and `Import 60 Days` actions.
- **Viewer app insights:** top app list (24h) now renders helper/import attributed usage.
- **Import progress tracking:** helper updates daemon import status via `SET_IMPORT_STATUS` (`running` -> `complete`) and viewer consumes it from daemon status.
- **Per-user startup automation:** viewer writes HKCU Run entry for helper loop and starts helper on launch when not already running.

## M3 service lifecycle and dashboard expansion

- **Service lifecycle scripts:** added elevated install/start/stop/restart/status/uninstall flow via `scripts/service-daemon.ps1` and command wrappers.
- **Daemon import-state persistence:** daemon now stores and serves import status/progress from settings, rather than volatile runtime-only values.
- **Dashboard expansion:** viewer now renders day/week/month totals with upload/download split cards.
- **Date-range filtering:** top-app breakdown now supports user-selected date range through start/end date pickers.
- **Import UX improvement:** import action now polls daemon status continuously so progress updates are visible while import runs.

## M4 interface filtering and export baseline

- **Interface endpoints:** daemon now exposes `GET_INTERFACES` and `GET_INTERFACE_BREAKDOWN` methods.
- **Interface sync behavior:** daemon syncs interface metadata each poll cycle even before deltas are recorded.
- **Viewer filter control:** interface selector added and wired into summary and app-breakdown queries.
- **Viewer interface breakdown panel:** selected-range per-interface totals displayed in UI.
- **Viewer export baseline:** CSV and JSON export actions added for selected range with app and interface sections.
- **App analytics UX expansion:** top-app list now supports sort modes (total/upload/download/name) with grouped "System" and "Other (<1 MB)" rows.
- **App detail drill-down:** selecting an app now loads per-app time-series buckets for the selected range, interface scope, and granularity.

## Collector hardening (P0 continuity)

- **Sleep/resume-aware interval handling:** daemon now computes observed elapsed seconds from wall-clock poll timestamps and scales anomaly budget to that interval.
- **Counter regression guardrails:** when counters move backwards (reset/regression), daemon only accepts bounded reset deltas and suppresses oversized regression spikes.
- **Metered profile sync:** poller now attempts WinRT connection-profile cost mapping to persist per-interface metered flags (`is_metered`) in the `interfaces` table.
- **Unit tests added:** coverage includes first-sample behavior, scaled anomaly budget behavior, regression suppression, reset handling, and observed-interval resolution.
- **Source cutover dedupe logic:** analytics queries now enforce source precedence to prevent overlap double-counting (`helper` preferred over recent `import` for app analytics, `interface_poll`/real-interface presence preferred over recent `import` for total summaries).
- **App detail source fix:** `GET_USAGE_SUMMARY` now switches to helper/import-backed aggregation when an `app_filter` is supplied.

## M2 overlap dedupe validation harness

- **Smoke script added:** `scripts/m2-overlap-dedupe-smoke.ps1` validates import/live overlap behavior end-to-end against daemon IPC.
- **Harness checks:**
  - summary source cutover excludes post-cutover import rows
  - app breakdown excludes post-helper-cutover import rows
  - app-filtered summary aligns with app breakdown totals under overlap

## Settings and hot-reload wiring

- **Settings query IPC:** daemon now exposes `GET_SETTINGS` with `poll_interval_seconds`, `retention_days`, and `afk_idle_threshold_seconds` sourced from SQLite settings.
- **Settings write clamping:** daemon now clamps settings values on write/read (`poll 15-300`, `retention <= 3650`, `afk 30-3600`).
- **Viewer settings controls:** main page now includes collector settings controls with apply and reset-default flows.
- **Hot-reload path:** viewer settings writes call `SET_SETTINGS`, then refresh daemon status/settings to verify poll-interval hot reload behavior.
- **Hot-reload smoke automation:** `scripts/m4-settings-hotreload-smoke.ps1` validates status/settings convergence after runtime settings updates.

## Delivered components

- Rust workspace with four crates:
  - `daemon`
  - `helper`
  - `shared-contracts`
  - `perf-harness`
- WinUI 3 viewer (`viewer/`) on .NET 8.
- Build and run scripts under `scripts/`.

## Outstanding work by phase

- **P0 completion:**
  - full export controls (granularity and field-selection parity pending)
  - complete dashboard chart modes and full interface parity
- **P1:** alerts engine, toast routing via helper/session bridge, AFK joins
- **P2:** heatmap, forecasting, anomaly model, confidence interval surfaces
