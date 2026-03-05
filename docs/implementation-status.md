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

## App detail and sorting polish (P0-13/P0-14)

- **App detail chart parity:** app detail section now includes a built-in time-series chart-style panel (bar rows over bucket series) alongside the bucket table.
- **Backend sort parity:** `GET_APP_BREAKDOWN` now honors requested sort mode (`total/upload/download/name`) and applies deterministic SQL tie-break ordering before limit.
- **Viewer sort propagation:** top-app refresh now forwards selected sort mode to daemon to avoid high-cardinality truncation bias from client-only resorting.
- **Selection continuity:** top-app selection now resolves across both regroup directions (app->group and group->app fallback when grouping boundaries change).
- **System grouping parity:** unknown/unattributed process rows are now folded into the `System` aggregate group.
- **Top-app readability polish:** app rows now show process, last-seen, and transfer split in clearer dedicated fields.

## Dashboard range and mode integration (P0-08/P0-09/P0-10/P0-11)

- **Overview mode toggle:** dashboard summary cards now support `Calendar` mode and `Selected Range` mode.
- **Range preset controls:** date range selection now supports presets (`Today`, `Last 7 Days`, `Last 30 Days`, `Custom`) and synchronizes with date pickers.
- **Range refresh parity:** applying range/preset now refreshes overview, top-app breakdown, app detail, and interface breakdown together.
- **Interface-filtered summary action:** summary action now respects active interface filter and overview mode/range.
- **Split visualization polish:** overview cards now include upload-share visual indicators (progress bar + percent text) alongside existing upload/download totals.

## Interface breakdown chart parity (P0-12)

- **Interface chart panel:** interface breakdown view now includes a chart-style usage-share panel (horizontal bars) in addition to the interface table.
- **Chart metrics:** each row now shows total usage, upload/download split text, and percent share of selected range.
- **Parity behavior:** chart and table both follow the same selected date range and interface filter scope.

## Collector hardening (P0 continuity)

- **Sleep/resume-aware interval handling:** daemon now computes observed elapsed seconds from wall-clock poll timestamps and scales anomaly budget to that interval.
- **Counter regression guardrails:** when counters move backwards (reset/regression), daemon only accepts bounded reset deltas and suppresses oversized regression spikes.
- **Metered profile sync:** poller now attempts WinRT connection-profile cost mapping to persist per-interface metered flags (`is_metered`) in the `interfaces` table.
- **Unit tests expanded:** coverage now includes first-sample behavior, scaled anomaly budget behavior, forward-anomaly suppression recovery, regression suppression recovery, mixed-counter regression/growth behavior, reset handling, and observed-interval resolution.
- **Source cutover dedupe logic:** analytics queries now enforce source precedence to prevent overlap double-counting (`helper` preferred over recent `import` for app analytics, `interface_poll`/real-interface presence preferred over recent `import` for total summaries).
- **App detail source fix:** `GET_USAGE_SUMMARY` now switches to helper/import-backed aggregation when an `app_filter` is supplied.

## M2 overlap dedupe validation harness

- **Smoke script added:** `scripts/m2-overlap-dedupe-smoke.ps1` validates import/live overlap behavior end-to-end against daemon IPC.
- **Harness checks:**
  - summary source cutover excludes post-cutover import rows
  - app breakdown excludes post-helper-cutover import rows
  - app-filtered summary aligns with app breakdown totals under overlap

## P0 export performance validation

- **Export perf harness:** `scripts/p0-16-export-perf-smoke.ps1` seeds 1-year hourly synthetic history and times export-query IPC sequence (`GET_USAGE_SUMMARY`, `GET_APP_BREAKDOWN`, `GET_INTERFACE_BREAKDOWN`).
- **Gate enforcement:** script fails if combined query flow exceeds 5000ms (`MaxTotalMs` configurable).
- **Latest run:** full query sequence completed in ~0.50s on current environment, under the P0 threshold.
- **Export parity completion:** export flow now supports app scope filtering (`All apps` vs `Selected app`) and records app filter metadata in both CSV and JSON outputs.

## P0 accuracy harness

- **Accuracy smoke harness:** `scripts/p0-07-accuracy-smoke.ps1` now compares daemon summary totals against OS byte counters (`netstat -e`) over poll-aligned windows.
- **Quality gates:** default threshold is `<= 0.1%` absolute deviation with low-traffic guard (`MinTotalBytes`) to avoid meaningless pass/fail outcomes.
- **Latest run:** observed deviation ~`0.018%` over a ~182s window (pass vs `0.1%` target).
- **Stability hardening:** harness now supports retry attempts (`MaxAttempts`) to reduce false failures during bursty traffic windows.

## Alerts threshold engine baseline (P1-02)

- **Runtime evaluation hooks:** daemon now evaluates cap thresholds after each poll write and immediately after cap upsert operations.
- **Threshold coverage:** active cap definitions now emit one-shot monthly threshold events at `50%`, `80%`, and `95%`, plus a daily cap event derived from `ceil(monthly_cap_bytes/30)`.
- **Event persistence model:** new `cap_alert_events` table stores threshold crossings with window scope, usage/cap bytes, and delivery state, with unique dedupe across `(cap, window, threshold)`.
- **Idempotency tests:** daemon DB tests now validate once-per-window dedupe and progressive threshold firing as usage grows.
- **Post-change gate check:** `scripts/r-02-performance-gates.ps1` re-run passes all gates after threshold engine integration.

## Alerts history tab baseline (P1-03)

- **Alert history IPC:** shared contracts now expose `LIST_CAP_ALERT_EVENTS` with typed request/response DTOs for cap alert history queries.
- **Daemon query path:** daemon now serves filtered/limited alert history from `cap_alert_events` ordered newest-first, with scope/window/threshold filters for viewer usage.
- **Viewer panel:** main page now includes an `Alerts History` panel with refresh action and selected-range context, showing threshold label, scope, usage vs cap, fired time, and window text.
- **Cap workflow wiring:** cap create/update/delete and global refresh flows now refresh alerts history so the panel tracks newly emitted threshold events.
- **Regression coverage:** daemon tests now verify alert-history ordering/limit behavior and scope+threshold filter behavior.

## R-02 performance gates automation

- **Gate orchestrator:** added `scripts/r-02-performance-gates.ps1` to enforce RSS, query latency, import duration, and daemon CPU gates with fail-fast behavior.
- **Script hardening:** `scripts/m0-feasibility.ps1` now supports `-SkipHelperProbe` for deterministic CI/perf-gate runs.
- **Import status race hardening:** R-02 import gate now polls daemon status after helper import completion and waits for `import_status=complete` before final timing assertion.
- **CI integration:** `.github/workflows/ci.yml` now executes R-02 baseline performance gates on Windows after build/test.
- **Latest run:** all four gates passed after race hardening (RSS, query latency, import duration, CPU 1m).

## Tray and tooltip baseline

- **Tray controller:** viewer now initializes a process tray icon with context menu actions (`Open Dashboard`, `Refresh Tooltip`, `Exit`).
- **Tooltip updates:** tray tooltip now surfaces current-day usage and refreshes periodically, with daemon-offline fallback text.
- **Close-to-tray behavior:** main window close now hides to tray; tray open action restores and activates the dashboard window.
- **Lifecycle cleanup:** tray resources are disposed on explicit exit/process shutdown paths with cancellation-safe refresh loop teardown.

## First-run onboarding flow (P0-05)

- **Onboarding state persistence:** daemon settings now include `onboarding_completed` in `GET_SETTINGS`/`SET_SETTINGS` and persist it in SQLite.
- **Viewer onboarding card:** first-run users now see a dedicated setup card with guided initial import and skip actions.
- **Guided import completion:** onboarding import path reuses helper 60-day import flow and marks onboarding complete on successful completion; skip action also persists completion.

## Metered flag validation (P0-04)

- **Validation smoke automation:** `scripts/m3-metered-flag-smoke.ps1` validates `GET_INTERFACES` and `GET_INTERFACE_BREAKDOWN` payload shape, interface type normalization, and metered boolean fields.
- **Latest run:** interfaces discovered with metered/unmetered counts reported and API validation checks passing.

## Settings and hot-reload wiring

- **Settings query IPC:** daemon now exposes `GET_SETTINGS` with `poll_interval_seconds`, `retention_days`, and `afk_idle_threshold_seconds` sourced from SQLite settings.
- **Settings write clamping:** daemon now clamps settings values on write/read (`poll 15-300`, `retention <= 3650`, `afk 30-3600`).
- **Viewer settings controls:** main page now includes collector settings controls with apply and reset-default flows.
- **Hot-reload path:** viewer settings writes call `SET_SETTINGS`, then refresh daemon status/settings to verify poll-interval hot reload behavior.
- **Hot-reload smoke automation:** `scripts/m4-settings-hotreload-smoke.ps1` validates status/settings convergence after runtime settings updates.
- **Export default settings:** daemon/viewer settings now include export defaults (`granularity`, `include summary/apps/interfaces`) and apply/reset these values through the same hot-reload path.
- **Hot-reload validation expansion:** M4 smoke now verifies the full settings surface (including onboarding and export defaults) and granularity normalization behavior.

## Sleep/resume continuity harness (P0-02)

- **Continuity smoke automation:** `scripts/m3-sleep-resume-continuity-smoke.ps1` suspends/resumes the daemon process to emulate sleep/hibernate gaps and validates post-resume polling behavior.
- **DB-level continuity checks:** script verifies post-resume poll rows, long observed `interval_secs` capture, and oversize anomaly suppression in the observed interval window.
- **Latest run:** observed long interval ~95s after a 35s suspend simulation with zero oversize anomaly rows.

## CI baseline (R-01)

- **GitHub Actions workflow:** added `.github/workflows/ci.yml` for push/pull_request validation.
- **CI scope:** runs on Windows with Rust workspace build/tests plus viewer Release build.

## Delivered components

- Rust workspace with four crates:
  - `daemon`
  - `helper`
  - `shared-contracts`
  - `perf-harness`
- WinUI 3 viewer (`viewer/`) on .NET 8.
- Build and run scripts under `scripts/`.

## Outstanding work by phase

- **P0 completion:** all tracked P0 items are complete.
- **P1:** toast routing while viewer is closed, AFK timeline + AFK export UI, retention/compact workflows
- **P2:** heatmap, forecasting, anomaly model, confidence interval surfaces
