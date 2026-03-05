# Singularity Monitor Master Task List

This is the single source of truth for completed work and remaining work to reach 100% completion against the PRD.

## Tracking Legend

- **Status**: `DONE`, `IN_PROGRESS`, `TODO`, `BLOCKED`
- **Priority**: `Critical`, `High`, `Medium`, `Low`
- **Dependency Type**:
  - `HARD` = strict blocker; predecessor must be `DONE` first
  - `SOFT` = preferred order; can run in parallel with extra risk/rework
- **Owners**:
  - `Rust Eng` = daemon/helper/storage/backend ownership
  - `C# Eng` = WinUI viewer ownership
  - `QA` = automation and validation ownership
  - `DevOps` = CI/CD and packaging pipeline ownership
  - `PM/Design` = product flow, docs, and UX sign-off

## Progress Snapshot

- Completed tasks: `55`
- Remaining tasks: `16`
- Current focus: `P1 alert delivery + AFK UI + release hardening`

---

## 1) Completed Work

| ID | Task | Status | Owner | Priority | Milestone |
|---|---|---|---|---|---|
| C-01 | PRD architecture decisions locked (service-first, helper fallback, SQLite settings source) | DONE | Rust Eng + PM/Design | Critical | M0 |
| C-02 | Rust workspace scaffolded (`daemon`, `helper`, `shared-contracts`, `perf-harness`) | DONE | Rust Eng | High | M0 |
| C-03 | WinUI 3 viewer scaffolded on .NET 8 | DONE | C# Eng | High | M0 |
| C-04 | Build scripts added for Rust/viewer with MSVC env handling | DONE | Rust Eng | High | M0 |
| C-05 | `GetIfTable2` interface polling implemented | DONE | Rust Eng | Critical | M0 |
| C-06 | Differential delta engine for interface counters implemented | DONE | Rust Eng | Critical | M0 |
| C-07 | Oversized interval anomaly clamp implemented (`SM_MAX_DELTA_BYTES`) | DONE | Rust Eng | High | M0 |
| C-08 | SQLite schema v1 initialized with WAL/tuning | DONE | Rust Eng | Critical | M0 |
| C-09 | Named-pipe IPC server implemented (`\\.\\pipe\\SingularityMonitor`) | DONE | Rust Eng | Critical | M0 |
| C-10 | Core IPC methods implemented (`GET_DAEMON_STATUS`, `GET_USAGE_SUMMARY`, `GET_APP_BREAKDOWN`, `SET_SETTINGS`, `GET_AFK_AUDIT`) | DONE | Rust Eng | Critical | M0 |
| C-11 | Daemon service entrypoint and console runtime implemented | DONE | Rust Eng | Critical | M0 |
| C-12 | Daemon memory sampling and optional working-set trim implemented | DONE | Rust Eng | High | M0 |
| C-13 | Helper WinRT attributed usage probe implemented | DONE | Rust Eng | Critical | M1 |
| C-14 | Helper->daemon ingestion IPC (`INGEST_ATTRIBUTED_USAGE`) implemented | DONE | Rust Eng | Critical | M1 |
| C-15 | Helper-attributed rows persisted in `usage_records` | DONE | Rust Eng | Critical | M1 |
| C-16 | Source-aware app breakdown query implemented (helper/import-focused) | DONE | Rust Eng | High | M1 |
| C-17 | Usage dedupe key and upsert safety implemented (`ts, app_id, interface_id, source`) | DONE | Rust Eng | High | M1 |
| C-18 | Helper modes implemented (`--probe`, `--push-once`, `--loop`) | DONE | Rust Eng | High | M1 |
| C-19 | Chunked import mode implemented (`--import-history --days N --chunk-hours N`) | DONE | Rust Eng | Critical | M2 |
| C-20 | Import status updates implemented (`SET_IMPORT_STATUS`) | DONE | Rust Eng | High | M2 |
| C-21 | Viewer daemon status + memory target feedback implemented | DONE | C# Eng | High | M1 |
| C-22 | Viewer helper controls added (start loop + import) | DONE | C# Eng | High | M2 |
| C-23 | Viewer top-app list (24h) connected to daemon breakdown | DONE | C# Eng | High | M2 |
| C-24 | Viewer helper recency + import progress display implemented | DONE | C# Eng | High | M2 |
| C-25 | Per-user helper startup registration (HKCU Run) + launch attempt implemented | DONE | C# Eng | Medium | M2 |
| C-26 | Validation scripts and smoke checks added (`m0-feasibility`, `m1-attribution-smoke`, loop/import helpers) | DONE | Rust Eng + QA | High | M2 |
| C-27 | Service lifecycle scripts added (`service-install/start/stop/restart/status/uninstall`) | DONE | Rust Eng | Critical | M3 |
| C-28 | Viewer dashboard cards expanded (day/week/month totals + split) and top-app date range controls added | DONE | C# Eng | High | M3 |
| C-29 | Interface API endpoints (`GET_INTERFACES`, `GET_INTERFACE_BREAKDOWN`) and viewer interface filter/breakdown/export baseline added | DONE | Rust Eng + C# Eng | High | M4 |

---

## 2) Remaining Work to 100%

## 2.1 P0 Completion (MVP Blockers)

| ID | Task | Status | Owner | Priority | Milestone | Hard Depends On | Soft Depends On |
|---|---|---|---|---|---|---|---|
| P0-02 | Sleep/hibernate resume continuity handling and tests | DONE | Rust Eng + QA | Critical | M3 | None | None |
| P0-03 | Counter reset/regression handling hardening (post-update edge cases) | DONE | Rust Eng | High | M3 | None | P0-02 |
| P0-04 | Per-interface metered flag detection and persistence | DONE | Rust Eng | High | M3 | None | P0-03 |
| P0-05 | First-run onboarding import flow finalized in viewer | DONE | C# Eng + PM/Design | Critical | M3 | None | None |
| P0-06 | Import/live overlap dedupe validation harness | DONE | Rust Eng + QA | High | M3 | None | P0-05 |
| P0-07 | Automated accuracy harness vs OS counters (<=0.1% target) | DONE | Rust Eng + QA | Critical | M3 | None | P0-03 |
| P0-08 | Dashboard day/week/month widgets and mode toggles | DONE | C# Eng | Critical | M3 | None | P0-10 |
| P0-09 | Upload/download split visualizations | DONE | C# Eng | High | M3 | None | P0-08 |
| P0-10 | Date range picker integration across dashboard and breakdowns | DONE | C# Eng | High | M3 | None | None |
| P0-11 | Interface filter (all/Wi-Fi/Ethernet/specific adapter) | DONE | C# Eng + Rust Eng | High | M3 | None | P0-10 |
| P0-12 | Per-interface breakdown page (table + chart) | DONE | C# Eng | High | M3 | P0-11 | P0-10, P0-08 |
| P0-13 | Per-app detail drill-down time series view | DONE | C# Eng | High | M3 | P0-10 | None |
| P0-14 | App list sorting and grouping parity with PRD rules | DONE | C# Eng | High | M3 | None | P0-13 |
| P0-15 | Export controls (CSV/JSON, date range, granularity, app/interface filters) | DONE | C# Eng + Rust Eng | Critical | M4 | P0-10 | P0-11 |
| P0-16 | Export performance validation (<5s up to 1-year hourly data) | DONE | QA | High | M4 | P0-15 | None |
| P0-17 | Full settings page coverage and reset-to-defaults | DONE | C# Eng | High | M4 | None | None |
| P0-18 | Settings hot-reload verification from viewer to daemon | DONE | Rust Eng + C# Eng | High | M4 | P0-17 | P0-03 |
| P0-19 | System tray icon/context menu implementation | DONE | C# Eng | Medium | M4 | None | None |
| P0-20 | Tray tooltip with current-day usage | DONE | C# Eng | Medium | M4 | P0-19 | None |

## 2.2 P1 Feature Completion

| ID | Task | Status | Owner | Priority | Milestone | Hard Depends On | Soft Depends On |
|---|---|---|---|---|---|---|---|
| P1-01 | Cap definition model (monthly/global/per-interface) in DB + UI | DONE | Rust Eng + C# Eng | High | M5 | None | P0-18 |
| P1-02 | Alert threshold engine (50/80/95 + daily cap) | DONE | Rust Eng | High | M5 | P1-01 | None |
| P1-03 | Alert history persistence + viewer tab | DONE | Rust Eng + C# Eng | High | M5 | P1-02 | P1-01 |
| P1-04 | Reliable toast notifications while viewer closed | TODO | C# Eng + Rust Eng | High | M5 | P1-02 | P1-03 |
| P1-05 | AFK detection pipeline (`WTSQuerySessionInformation` + idle threshold) | DONE | Rust Eng | High | M5 | None | P0-03 |
| P1-06 | AFK audit query and usage joins | DONE | Rust Eng | High | M5 | P1-05 | None |
| P1-07 | AFK timeline UI + AFK-only filter | TODO | C# Eng | Medium | M5 | P1-06 | P0-10 |
| P1-08 | AFK CSV export flow | TODO | C# Eng | Medium | M5 | P1-07 | None |
| P1-09 | Retention policy UI wiring and daemon cleanup scheduler | TODO | Rust Eng + C# Eng | High | M5 | None | P0-17 |
| P1-10 | Database size display + manual compact (VACUUM) action | TODO | Rust Eng + C# Eng | Medium | M5 | P1-09 | None |

## 2.3 P2 Feature Completion

| ID | Task | Status | Owner | Priority | Milestone | Hard Depends On | Soft Depends On |
|---|---|---|---|---|---|---|---|
| P2-01 | 7x24 heatmap data aggregation and UI | TODO | Rust Eng + C# Eng | Medium | M6 | P0-10 | P0-08, P0-09 |
| P2-02 | Forecasting model (14-day linear regression) | TODO | Rust Eng | Medium | M6 | None | P2-07 |
| P2-03 | Cost forecast calculations and UI | TODO | Rust Eng + C# Eng | Medium | M6 | P2-02 | P1-01 |
| P2-04 | Confidence interval rendering for forecast output | TODO | Rust Eng + C# Eng | Medium | M6 | P2-02 | None |
| P2-05 | Anomaly detection model (rolling baseline + 3 sigma) | TODO | Rust Eng | Medium | M6 | None | P2-07, P2-08 |
| P2-06 | Anomaly alert integration and per-app mute controls | TODO | Rust Eng + C# Eng | Medium | M6 | P2-05 | P1-02 |
| P2-07 | Hourly pre-aggregation table(s) for long-range query performance | TODO | Rust Eng | Medium | M6 | None | None |
| P2-08 | Query/index tuning for large archives (1y+ usage) | TODO | Rust Eng + QA | Medium | M6 | P2-07 | R-02 |

## 2.4 Release, QA, and Hardening

| ID | Task | Status | Owner | Priority | Milestone | Hard Depends On | Soft Depends On |
|---|---|---|---|---|---|---|---|
| R-01 | CI pipeline for Rust and viewer builds | DONE | DevOps | Critical | M7 | None | None |
| R-02 | Automated performance gates (RSS, CPU, import duration, query latency) | DONE | QA + DevOps | Critical | M7 | R-01 | P0-16 |
| R-03 | IPC contract and schema migration integration tests | TODO | Rust Eng + QA | High | M7 | R-01 | None |
| R-04 | Crash reporting scaffolding and reliability metrics | TODO | Rust Eng | High | M7 | None | R-03 |
| R-05 | MSIX packaging and capability manifest finalization | TODO | DevOps + C# Eng | Critical | M7 | R-01 | None |
| R-06 | Signing workflow integration for release artifacts | TODO | DevOps | High | M7 | R-05 | None |
| R-07 | Winget manifest prep + install/upgrade/uninstall validation | TODO | DevOps + QA | High | M7 | R-05, R-06 | None |
| R-08 | Accessibility audit (keyboard, narrator, focus flow) | TODO | C# Eng + QA | High | M7 | None | P0-08, P0-10 |
| R-09 | User docs finalization (install, usage, troubleshooting) | TODO | PM/Design | Medium | M7 | None | P0-17 |
| R-10 | Full QA matrix on Win11 versions + hardware profiles | TODO | QA | Critical | M7 | R-01, R-02, R-03 | R-08 |
| R-11 | Security review of pipe ACLs, local data permissions, export paths | TODO | Rust Eng + QA | High | M7 | None | R-03 |
| R-12 | Go/no-go checklist and launch readiness sign-off | TODO | PM/Design + QA + Eng | Critical | M7 | R-05, R-06, R-07, R-10, R-11 | R-09 |

---

## 3) Immediate Execution Plan (Step-by-Step + Blockers)

| Step | Task ID | Dependency Gate | Blocker Type | Why Now |
|---|---|---|---|---|
| 1 | P1-04 | `P1-02` (DONE), `P1-03` (DONE) | HARD | Reliable toast delivery is now the primary remaining alerts dependency |
| 2 | P1-07 | `P1-06` (DONE) | HARD | AFK timeline UI is fully unblocked and can proceed in parallel |
| 3 | P1-08 | `P1-07` | HARD | AFK export flow follows AFK timeline/filter behavior |
| 4 | P1-09 | `P0-17` (DONE) | SOFT | Retention scheduling should land before long-run QA and DB size controls |
| 5 | P1-10 | `P1-09` | HARD | DB compact controls depend on retention scheduler and status wiring |
| 6 | R-03 | `R-01` (DONE) | HARD | Contract/schema integration tests should be automated early in hardening |
| 7 | R-05 | `R-01` (DONE) | HARD | Packaging work can proceed now that baseline CI exists |
| 8 | R-08 | None | SOFT | Accessibility issues should be surfaced before final QA matrix and go/no-go |
| 9 | R-10 | `R-01`, `R-02` (DONE), `R-03` | HARD | Full QA matrix is the primary launch-readiness validation gate |

Workflow notes:
- `P1-04` and `P1-07` can run in parallel now that `P1-03` is complete.
- `R-03` and `R-05` can proceed in parallel after `R-01`.

---

## 4) Definition of 100% Complete

The project is 100% complete when:

1. All `TODO`/`IN_PROGRESS` rows in this file are `DONE`.
2. All PRD P0/P1/P2 acceptance criteria pass validation.
3. Performance gates pass on target QA hardware:
   - daemon steady-state memory `P95 < 3MB`
   - hard ceiling `< 5MB`
   - CPU and timing KPIs within PRD thresholds
4. Packaging, signing, and winget distribution are release-ready.
5. QA, accessibility, and launch sign-offs are complete.
