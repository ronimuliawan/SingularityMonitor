# Singularity Monitor — Product Requirements Document

**Version:** 1.0  
**Status:** Draft for Development  
**Last Updated:** March 2026  
**Classification:** Internal — Engineering & Product

---

## Table of Contents

1. [Executive Summary](#1-executive-summary)
2. [Problem Statement](#2-problem-statement)
3. [Solution Overview](#3-solution-overview)
4. [User Personas](#4-user-personas)
5. [Technical Architecture](#5-technical-architecture)
6. [Functional Requirements](#6-functional-requirements)
7. [API Specifications](#7-api-specifications)
8. [Data Models](#8-data-models)
9. [Implementation Plan](#9-implementation-plan)
10. [Success Metrics](#10-success-metrics)

---

## 1. Executive Summary

### Vision & Value Proposition

Singularity Monitor is a precision data-tracking utility for Windows 11 that delivers deep, long-term network analytics with an industry-leading background footprint of under 5MB RAM. In an era where metered connections, mobile tethering, and ISP data caps are everyday realities, most users remain completely blind to what their machine consumes — and why. Windows provides only a shallow 60-day usage window with no per-session granularity, no forecasting, and no audit trail for background activity. Third-party alternatives either resort to heavyweight packet sniffing (ballooning CPU and RAM usage) or offer superficial summaries that fail power users.

Singularity Monitor solves this with a fundamentally different design philosophy: **differential tracking**. Like a digital odometer, it periodically polls native Windows connectivity APIs to compute byte-level deltas per application, per network interface, and per time bucket — with zero packet inspection, zero kernel drivers, and zero elevated-permission footprint. The result is tracking accuracy that matches the OS counters themselves (target: ≤0.1% deviation), achieved at a cost so low it is effectively invisible to the system.

The product's split-process architecture — a headless Rust daemon paired with an on-demand WinUI 3 viewer — ensures that data collection never stops, even when the GUI is closed, while the GUI itself launches instantly from a cold start. A local SQLite archive extends retention indefinitely beyond Windows' native 60-day limit, enabling a class of analytics — predictive cost forecasting, usage heatmaps, AFK audits — that simply do not exist in any competing lightweight tool today. Singularity Monitor is the definitive anti-bloat solution for power users who demand total transparency over their data without sacrificing system performance.

### Key Objectives

| Objective | Target Metric | Measurement Method |
|---|---|---|
| Ultra-low memory footprint | Daemon ≤ 5 MB RAM (steady-state) | Windows Task Manager / perfmon |
| Near-zero CPU impact | ≤ 0.2% average CPU on typical 4-core system | Continuous perfmon sampling over 24h |
| High tracking accuracy | ≤ 0.1% deviation from OS counters | Automated delta vs. GetIfTable2 regression tests |
| Seamless onboarding | History import completes in < 60 seconds | QA timed benchmark on reference hardware |
| Reliability | ≥ 99.5% crash-free sessions per month | Local crash report telemetry |
| Time-to-first-insight | < 2 minutes from installer launch to first dashboard | UX timer in onboarding flow |

### Expected Impact & Success Criteria

At MVP launch, Singularity Monitor targets power users on metered Windows 11 machines — a segment estimated in the tens of millions globally. Success at 90 days post-launch is defined as: daemon performance metrics consistently met on 95%+ of QA hardware configurations; ≥ 40% of active users configuring at least one usage alert; and user retention (weekly active viewer sessions) above 30% after the first month. Long-term, the advanced analytics tier (heatmaps, forecasting, AFK audits) represents the primary feature differentiation that drives organic word-of-mouth among IT professionals and power user communities.

---

## 2. Problem Statement

### Current Market Situation

Windows 11's built-in Data Usage panel (Settings → Network & Internet → Data Usage) provides per-app byte totals with a fixed 30-day rolling window — recently extended to approximately 60 days in some builds — but offers no session-level granularity, no interface separation beyond a single adapter filter, no historical export, and no alerting. Microsoft's own documentation acknowledges the counter resets on major updates. For metered connections, this is functionally useless for month-end reconciliation.

The third-party landscape divides into two camps: **packet sniffers** (Wireshark, GlassWire, NetLimiter) and **superficial dashboards** (DataUsage, NetWorx legacy). Packet sniffers are technically powerful but demand WinPcap/Npcap kernel drivers, require administrator privileges at all times, consume 50–200MB RAM in background mode, and raise legitimate privacy concerns about deep packet inspection. Superficial dashboards offer better overhead but trade accuracy for simplicity — many poll at intervals so coarse they miss burst events entirely, and none offer predictive analytics or AFK attribution.

### User Pain Points — Real Scenarios

**Scenario A — The Surprise Overage:** A remote worker on a 50GB/month mobile hotspot plan receives an overage bill for $45. Checking Windows Data Usage, they see only aggregate totals for the past 30 days with no drill-down. They cannot determine whether the spike occurred during a Windows Update window at 3 AM or during a large video call. They have no tool to set a proactive alert at 80% usage.

**Scenario B — The Audit Failure:** An IT consultant deploying devices for a small business client needs to verify that Windows Update is restricted to the approved maintenance window (weekends, 2–4 AM). Existing tools require installing a kernel-level sniffer on every client machine — unacceptable for security policy — or manually reviewing Event Viewer logs with no bandwidth correlation.

**Scenario C — The AFK Mystery:** A developer leaves their machine overnight for a compile run. In the morning, Windows reports 4.2 GB used since midnight. They cannot determine which process consumed the data, whether it overlapped with their AFK period, or whether this is a one-time event or a recurring pattern.

### Opportunity Size & Cost of Inaction

The global installed base of Windows 11 machines is approximately 400 million as of early 2026. Metered connection awareness has grown significantly as mobile broadband and tethering have moved from niche to mainstream working habits. Conservative estimates suggest 15–25% of Windows 11 power users actively want granular data tracking — a serviceable addressable market in the tens of millions.

The cost of inaction is user trust and wallet: overage charges, throttling surprises, and uncontrolled background bandwidth drain. Singularity Monitor addresses this with a local-first, privacy-respecting, zero-bloat approach that no current tool matches.

---

## 3. Solution Overview

### How the Solution Works

Singularity Monitor operates on a **differential polling** model. Rather than intercepting network packets (which requires kernel drivers and elevated privileges), the daemon calls Windows' native `GetIfTable2` / `GetPerAdapterInfo` and `NetworkIsolation` / `GetNetworkUsage` APIs on a configurable schedule (default: every 60 seconds). Each poll reads the cumulative byte counters maintained by Windows for every active network interface and every application. The daemon subtracts the previous reading to compute a byte delta for that interval, then writes a normalized record to the local SQLite database.

This is conceptually identical to reading a car's odometer at two points in time to compute distance traveled — no GPS required, no black-box inference, just arithmetic on authoritative OS counters. The result is accuracy that is mathematically bounded to the precision of the OS counters themselves.

**On first installation**, the daemon invokes the Windows `GetNetworkUsageList` API (available since Windows 8.1) to bulk-import up to approximately 60 days of per-app, per-interface historical usage data that Windows has already accumulated. This import runs as a background job and completes in under 60 seconds on typical hardware, giving users an instant populated dashboard rather than an empty slate.

**The Viewer GUI** is a separate process that communicates with the daemon via a named pipe IPC channel. It launches on demand — in under 2 seconds — renders charts and tables from the SQLite database, and closes without interrupting collection. This architectural separation means the footprint of "running Singularity Monitor" in the background is purely the daemon: a single Rust binary consuming under 5 MB of RAM and negligible CPU.

### Technical Approach & Key Decisions

| Decision | Choice | Rationale |
|---|---|---|
| Data collection method | Differential polling via Win32 APIs | Zero kernel footprint, no elevated perms, accuracy = OS counter accuracy |
| Daemon language | Rust | Memory safety, minimal runtime, predictable low allocations |
| GUI framework | WinUI 3 (Windows App SDK) | Native Win11 look, Fluent Design, MSIX-compatible |
| IPC mechanism | Named Pipes | Windows-native, low latency, no external dependencies |
| Storage | SQLite (via rusqlite) | Zero-server, ACID, excellent read performance, portable |
| Installer | MSIX + winget | Modern Windows packaging, update support, sandboxed install |
| Historical import | GetNetworkUsageList API | Official, unprivileged, covers up to ~60 days |

### Core Differentiators

1. **Accuracy without sniffing** — No packet inspection means no kernel driver, no privacy exposure, no WinPcap dependency, and no false positives from encrypted traffic. Accuracy is definitionally bounded by the OS counters.
2. **Industry-leading background footprint** — The daemon targets ≤ 5 MB RAM and ≤ 0.2% average CPU. Competing tools that offer background monitoring typically consume 5–20× more RAM.
3. **Instant history on install** — Users see 60 days of data immediately. No competitor offers this without a prior install.
4. **Long-term archive & advanced analytics** — SQLite retention is unlimited. Heatmaps, forecasting models, and AFK audits are unique in the lightweight monitoring category.
5. **Local-first, trust-first** — All data stays on the device. No cloud sync, no telemetry by default, fully exportable.

---

## 4. User Personas

### Persona A — Marcus, Metered Power User

**Role:** Senior software developer, fully remote  
**Age:** 34 | **Location:** Rural area, primary internet via LTE hotspot (100 GB/month plan)  
**Technical Proficiency:** High — comfortable with Task Manager, PowerShell, developer tools  

**Workflow:** Marcus works 9–5 on a Windows 11 laptop tethered to his phone. He participates in 3–4 video calls daily, pushes code to GitHub, and downloads SDKs and Docker images regularly. His data cap is a constant background concern. At month-end, he often discovers he's used 15–20 GB more than expected, with no clear culprit.

**Pain Points:**
- Windows Data Usage resets context after updates and shows no hourly breakdown
- He has no alert before hitting his cap — discovery is always after the fact
- He cannot tell whether his video calls or Windows Update consumed his last 10 GB

**Goals:** Set a monthly cap with alerts at 80% and 95%; see exactly which app used what on any given day; identify what ran while he was AFK overnight.

**Representative Quote:** *"I need a reliable odometer for my data, not a blurry rearview mirror."*

---

### Persona B — Priya, IT Consultant / Sysadmin

**Role:** Independent IT consultant managing 40+ Windows endpoints for SMB clients  
**Age:** 41 | **Location:** Metro area, enterprise-grade internet at office, varied at client sites  
**Technical Proficiency:** Expert — Active Directory, Group Policy, PowerShell scripting, SIEM tools

**Workflow:** Priya deploys and audits Windows 11 devices for clients. She regularly needs to verify that Windows Update delivery is confined to approved maintenance windows, that no unauthorized remote-access software is phoning home, and that background data consumption is within acceptable bounds for clients on metered business internet plans.

**Pain Points:**
- Packet sniffers require kernel drivers and admin rights — politically and practically unacceptable on client machines
- No lightweight tool provides an exportable audit report she can hand to a client
- Windows Event Viewer logs don't correlate network events with bandwidth consumed

**Goals:** Install a zero-footprint auditing agent on client devices; export CSV/JSON usage reports by date range; quickly answer "what used data at 3 AM last Tuesday?"

**Representative Quote:** *"If the tool itself is the bloat problem, I've failed my client before I've started."*

---

### Persona C — Diego, Remote Worker on Tethering

**Role:** Marketing manager, hybrid remote  
**Age:** 28 | **Location:** Urban apartment, fiber at home but frequently travels and tethers  
**Technical Proficiency:** Medium — comfortable with consumer settings, not a developer

**Workflow:** Diego works from coffee shops, airports, and hotel rooms. He uses Teams, Chrome, and cloud storage apps heavily. He is not a power user but has learned — after several expensive hotel Wi-Fi bills — to be conscious of data. He wants simple, actionable answers without configuration overhead.

**Pain Points:**
- He doesn't know which app to blame for heavy usage during meetings
- He can't tell if he's on track for the month without doing manual math
- He wants "traffic light" status, not raw numbers

**Goals:** One-glance daily summary; push notification when approaching daily cap; simple AFK report showing what ran while he was in a meeting.

**Representative Quote:** *"Just tell me if I'm in the green, yellow, or red — and who's causing it."*

---

## 5. Technical Architecture

### System Overview

```
┌─────────────────────────────────────────────────────┐
│                  Windows 11 Host                    │
│                                                     │
│  ┌──────────────┐        ┌───────────────────────┐  │
│  │ Collector    │  IPC   │   Viewer GUI          │  │
│  │ Daemon       │◄──────►│   (WinUI 3)           │  │
│  │ (Rust,       │ Named  │   - Dashboard         │  │
│  │  Win Service)│  Pipe  │   - Charts/Tables     │  │
│  │              │        │   - Settings/Alerts   │  │
│  │  Polls:      │        │   - Export            │  │
│  │  GetIfTable2 │        └───────────────────────┘  │
│  │  GetNetUsage │                                   │
│  │  WMI / APIs  │        ┌───────────────────────┐  │
│  │              │        │  Analytics Engine     │  │
│  │  Writes:     │        │  (Rust, in-process)   │  │
│  │  SQLite DB   │◄──────►│  - Forecasting        │  │
│  └──────────────┘        │  - Heatmaps           │  │
│         │                │  - AFK Audit          │  │
│         ▼                │  - Anomaly Detection  │  │
│  ┌──────────────┐        └───────────────────────┘  │
│  │  SQLite DB   │                                   │
│  │  (local)     │                                   │
│  └──────────────┘                                   │
└─────────────────────────────────────────────────────┘
```

### Component Specifications

#### 5.1 Collector Daemon

| Property | Specification |
|---|---|
| Language | Rust (stable toolchain) |
| Deployment | Windows Service (via `windows-service` crate) |
| Startup | Automatic, runs as `LocalService` account |
| Poll interval | Default 60s; configurable 15s–300s |
| Memory target | ≤ 5 MB RSS steady-state |
| CPU target | ≤ 0.2% average on 4-core/8-thread system |
| IPC server | Named pipe: `\\.\pipe\SingularityMonitor` |
| Logging | Structured JSON to local rotating file (max 10 MB) |

**Core responsibilities:**
- Poll `GetIfTable2` for interface-level byte counters
- Poll `GetNetworkUsageList` for per-app byte counters (Windows 8.1+ API)
- Compute byte deltas vs previous reading; handle counter wraps gracefully
- Write normalized delta records to SQLite within same polling cycle
- Serve real-time snapshots and query results to Viewer via IPC
- Execute initial history import on first run

**Windows APIs used:**

| API | Purpose | Header |
|---|---|---|
| `GetIfTable2` | Interface-level TX/RX counters | `netioapi.h` |
| `GetNetworkUsageList` | Per-app usage history | `netioapi.h` |
| `GetAdaptersInfo` / `GetAdaptersAddresses` | Adapter metadata | `iphlpapi.h` |
| `WTSQuerySessionInformation` | AFK/session state | `wtsapi32.h` |
| `SetConsoleCtrlHandler` | Graceful shutdown | `wincon.h` |

#### 5.2 Viewer GUI

| Property | Specification |
|---|---|
| Framework | Windows App SDK 1.5+ / WinUI 3 |
| Language | C# (.NET 8) |
| Launch time | < 2 seconds cold start |
| Process lifetime | On-demand; exits when window closes |
| IPC client | Named pipe client to daemon |
| Charting | WinUI community toolkit charts or LiveCharts2 |

**Views:**
- **Dashboard** — daily/weekly/monthly totals, top-5 apps widget, interface summary
- **App Detail** — per-app time series, drill-down by interface, export button
- **AFK Audit** — timeline overlay of user-inactive periods vs. bandwidth events
- **Heatmap** — 7-day × 24-hour grid coloured by usage intensity
- **Forecast** — projected month-end usage with cap threshold overlay
- **Alerts** — configure thresholds, view alert history
- **Settings** — poll interval, retention policy, cap definitions, export defaults

#### 5.3 SQLite Storage Layer

- **Location:** `%LOCALAPPDATA%\SingularityMonitor\data.db`
- **Engine:** SQLite 3.45+ via `rusqlite` (daemon) and `Microsoft.Data.Sqlite` (GUI)
- **WAL mode:** Enabled for concurrent read/write without blocking
- **Target growth rate:** < 50 MB/year for typical single-user machine
- **Backup:** Optional export of full DB on demand; no cloud sync

#### 5.4 Analytics Engine

Implemented as a Rust library linked into the daemon; query results served to GUI via IPC.

| Feature | Method | Complexity |
|---|---|---|
| Usage forecasting | Linear regression on rolling 14-day window | Low |
| Usage heatmap | Aggregate bucketing by hour-of-day × day-of-week | Low |
| AFK audit | Join usage records with session-idle events | Medium |
| Anomaly detection | Z-score on per-app 30-day rolling baseline | Medium |
| Cost forecasting | User-defined $/GB rate × projected usage | Low |

### Data Flow

```
[Windows APIs] → [Daemon Poll Loop (60s)] → [Delta Computation] → [SQLite Write]
                                                                         │
[GUI Launch] → [IPC Request] → [Daemon Query Handler] → [SQLite Read] ──┘
                                        │
                              [Analytics Engine]
                                        │
                              [IPC Response → GUI Render]
```

### Security & Privacy Considerations

| Concern | Mitigation |
|---|---|
| Privilege escalation | Daemon runs as `LocalService`; no admin rights required post-install |
| Data exfiltration | All data stored locally; no network egress by daemon |
| IPC spoofing | Named pipe ACL restricted to current user SID |
| SQLite injection | All queries use parameterized statements |
| Installer trust | MSIX package signed with EV certificate |
| Export privacy | User-initiated only; no automatic sharing |

---

## 6. Functional Requirements

### Priority Definitions
- **P0** — MVP blocker; must ship in Phase 1–3
- **P1** — High value; ships in Phase 3 or early post-MVP
- **P2** — Roadmap; ships in Phase 4+

---

### US-01 — Differential Polling Collection (P0)

**Story:** As a user, I want the app to automatically track my network usage in the background so I don't have to manually initiate anything.

**Acceptance Criteria:**
- Daemon starts automatically on Windows boot as a service
- Poll interval defaults to 60 seconds; configurable in Settings (15s–300s)
- Delta computed per application, per network interface, per poll cycle
- Counter wrap (overflow) handled without data loss or negative deltas
- Polling continues when GUI is closed
- Polling resumes correctly after system sleep/hibernate

**Edge Cases:** Handle counter reset after Windows Update; detect and skip anomalous deltas > 10 GB in a single 60s interval (log warning, do not record).

---

### US-02 — Initial History Import (P0)

**Story:** As a new user, I want to see up to 60 days of my existing usage history immediately after installing so I can start with meaningful data.

**Acceptance Criteria:**
- On first run, daemon calls `GetNetworkUsageList` for all available historical periods
- Import covers all available history up to 60 days prior
- Import completes in < 60 seconds on reference hardware (Intel i5-class, SSD)
- Progress indicator shown in GUI during first-launch onboarding
- Imported data deduplication: no double-counting if OS and daemon periods overlap
- Import can be re-triggered manually from Settings if needed

---

### US-03 — Dashboard: Total Usage Overview (P0)

**Story:** As a user, I want a dashboard that shows my total data usage by day, week, and month so I can understand my consumption at a glance.

**Acceptance Criteria:**
- Dashboard loads within 1 second of GUI launch
- Displays: total usage for current day, current week, current month (calendar)
- Toggle between daily bar chart, weekly trend line, and monthly summary
- Shows upload vs. download split
- Interface filter (All / Wi-Fi / Ethernet / specific adapter) available in toolbar
- Date range picker allows selecting any historical period in the archive

---

### US-04 — Per-App Usage Breakdown (P0)

**Story:** As a user, I want to see which applications consumed the most data so I can identify the top offenders.

**Acceptance Criteria:**
- App list sortable by: total bytes (default), upload, download, app name
- Each row shows: app name/icon, process name, total bytes, upload, download, last-seen timestamp
- Clicking an app opens a detail view with time-series chart for that app
- Unknown/system processes grouped under "System" with drill-down available
- Filter by date range applies to per-app breakdown
- Minimum display threshold: apps with < 1 MB in period shown in collapsed "Other" group

---

### US-05 — Per-Interface Breakdown (P0)

**Story:** As a user, I want to see usage broken down by network interface (Wi-Fi, Ethernet, mobile hotspot) so I know which connection was used.

**Acceptance Criteria:**
- Interfaces listed with display names matching Windows adapter names
- Metered vs. unmetered interface flag shown (sourced from Windows connection profile)
- Total bytes per interface per selected period shown in table and chart
- Interface names persist in DB even after adapter is disconnected

---

### US-06 — Usage Alerts & Thresholds (P1)

**Story:** As a user, I want to configure alerts when I reach a percentage of my data cap so I can take action before incurring overages.

**Acceptance Criteria:**
- User can define a monthly cap (in GB or MB) per interface or globally
- Alert thresholds configurable: e.g., notify at 50%, 80%, 95% of cap
- Alerts delivered via Windows toast notifications
- Alert history viewable in the Alerts tab
- Daily cap alert also supported (e.g., "alert if > 5 GB in a single day")
- Alerts fire even when GUI is closed (daemon triggers notification)

---

### US-07 — AFK Audit (P1)

**Story:** As a power user, I want to see what data was consumed while I was away from my computer so I can identify background update activity.

**Acceptance Criteria:**
- AFK period defined as: no keyboard/mouse input for ≥ 5 minutes (configurable)
- AFK windows detected using `WTSQuerySessionInformation` + idle timer
- AFK audit view shows: timeline of AFK periods overlaid with bandwidth chart
- Per-app breakdown within each AFK period
- Filter: "AFK only" checkbox on main app list
- Export of AFK audit to CSV

---

### US-08 — Usage Heatmap (P2)

**Story:** As a power user, I want to see a heatmap of my usage by hour of day and day of week to understand when I use the most data.

**Acceptance Criteria:**
- 7×24 grid (day of week × hour of day) coloured by usage intensity
- Colour scale: white (zero) → blue (low) → red (high)
- Hover tooltip shows exact bytes for that cell
- Date range selector adjusts heatmap source data
- Per-app heatmap: select an app to see its hourly pattern

---

### US-09 — Predictive Cost Forecasting (P2)

**Story:** As a user on a metered plan, I want to see a projected month-end usage and cost so I can manage my plan proactively.

**Acceptance Criteria:**
- User inputs: cap size (GB), cost per GB over cap, billing cycle start date
- Forecast uses linear regression on last 14 days of usage
- Dashboard widget shows: projected month-end GB, projected overage GB, projected overage cost
- Confidence interval displayed (±X GB at 80% confidence)
- Forecast updates daily

---

### US-10 — Data Export (P0)

**Story:** As a user or IT admin, I want to export my usage data to CSV or JSON so I can analyze it externally or share it with a client.

**Acceptance Criteria:**
- Export button available on Dashboard and App Detail views
- Formats: CSV and JSON
- Configurable fields: date range, granularity (hour/day/week), apps, interfaces
- Export completes in < 5 seconds for up to 1 year of hourly data
- File saved to user-specified path; default: `%USERPROFILE%\Downloads\singularity_export_YYYYMMDD.csv`

---

### US-11 — Retention Policy Configuration (P1)

**Story:** As a user, I want to configure how long my usage history is retained so I can manage disk space.

**Acceptance Criteria:**
- Options: Unlimited, 3 months, 6 months, 12 months, 24 months
- Daemon runs a nightly cleanup job to delete records older than the policy
- Current DB size shown in Settings
- Manual "Compact Database" button runs SQLite VACUUM

---

### US-12 — Anomaly Detection Alerts (P2)

**Story:** As a power user, I want to be notified when an app's usage is abnormally high compared to its recent baseline so I can investigate potential issues.

**Acceptance Criteria:**
- Baseline: 30-day rolling average + standard deviation per app
- Alert fires when app usage in a single poll interval exceeds baseline mean + 3σ
- Alert details: app name, current value, baseline, deviation
- User can mute anomaly alerts per app
- Anomaly history stored in DB for review

---

### US-13 — Settings & Configuration (P0)

**Story:** As a user, I want to configure the app's behavior so it fits my preferences and workflow.

**Acceptance Criteria:**
- Settings persisted in `%LOCALAPPDATA%\SingularityMonitor\settings.json`
- Configurable: poll interval, retention policy, cap definitions, alert thresholds, AFK timeout, export defaults, theme (light/dark/system)
- "Reset to Defaults" button available
- Settings changes applied without restarting the daemon (hot-reload via IPC)

---

### US-14 — Startup & Tray Icon (P0)

**Story:** As a user, I want Singularity Monitor to start automatically and be accessible from the system tray without cluttering my taskbar.

**Acceptance Criteria:**
- Daemon starts as Windows Service on boot (no user login required for collection)
- Viewer launches minimized to system tray by default (configurable)
- Tray icon shows current-day usage as tooltip
- Tray context menu: Open Dashboard, Today's Usage Summary, Quit Viewer (daemon keeps running)

---

### US-15 — Installer & First-Run Experience (P0)

**Story:** As a new user, I want a clean, fast install experience that gets me to my first insight within 2 minutes.

**Acceptance Criteria:**
- MSIX installer, available via winget (`winget install SingularityMonitor`)
- Install completes in < 30 seconds on SSD
- First-run wizard: (1) Welcome, (2) History Import Progress, (3) Optional cap setup, (4) Dashboard
- No reboot required post-install
- Uninstall removes all app files; offers to retain or delete user data

---

## 7. API Specifications

The IPC protocol between daemon and GUI uses a line-delimited JSON-over-named-pipe protocol. Each message is a UTF-8 JSON object terminated by `\n`. The daemon acts as server; the GUI is the client.

### 7.1 Connection

**Pipe name:** `\\.\pipe\SingularityMonitor`  
**Access:** Read/Write, restricted to current user SID  
**Encoding:** UTF-8, newline-delimited JSON  
**Timeout:** Client connects with 5-second timeout; retries 3× before showing "daemon unavailable" error

---

### 7.2 Message Envelope

Every message (request and response) uses this envelope:

```json
{
  "id": "uuid-v4-string",
  "type": "request|response|event",
  "method": "string",
  "payload": { },
  "error": null
}
```

Error response:
```json
{
  "id": "same-as-request-id",
  "type": "response",
  "method": "same-as-request",
  "payload": null,
  "error": { "code": 404, "message": "No data for specified range" }
}
```

---

### 7.3 Endpoints

#### GET_USAGE_SUMMARY

Returns aggregated usage for a time range.

**Request:**
```json
{
  "id": "a1b2c3d4-...",
  "type": "request",
  "method": "GET_USAGE_SUMMARY",
  "payload": {
    "start_ts": 1740000000,
    "end_ts": 1742678400,
    "granularity": "day",
    "interface_id": null,
    "app_filter": null
  }
}
```

**Response:**
```json
{
  "id": "a1b2c3d4-...",
  "type": "response",
  "method": "GET_USAGE_SUMMARY",
  "payload": {
    "buckets": [
      {
        "ts": 1740000000,
        "bytes_sent": 1234567890,
        "bytes_recv": 9876543210,
        "interface_id": null
      }
    ],
    "total_sent": 1234567890,
    "total_recv": 9876543210
  },
  "error": null
}
```

**Parameters:**

| Field | Type | Required | Notes |
|---|---|---|---|
| `start_ts` | Unix timestamp (i64) | Yes | Inclusive |
| `end_ts` | Unix timestamp (i64) | Yes | Exclusive |
| `granularity` | `"hour"`, `"day"`, `"week"`, `"month"` | Yes | Bucket size |
| `interface_id` | string or null | No | Filter to adapter GUID; null = all |
| `app_filter` | string or null | No | Filter to process name; null = all |

---

#### GET_APP_BREAKDOWN

Returns per-app usage for a time range.

**Request:**
```json
{
  "id": "e5f6g7h8-...",
  "type": "request",
  "method": "GET_APP_BREAKDOWN",
  "payload": {
    "start_ts": 1740000000,
    "end_ts": 1742678400,
    "interface_id": null,
    "limit": 50,
    "sort_by": "total_bytes_desc"
  }
}
```

**Response:**
```json
{
  "id": "e5f6g7h8-...",
  "type": "response",
  "method": "GET_APP_BREAKDOWN",
  "payload": {
    "apps": [
      {
        "process_name": "msedge.exe",
        "display_name": "Microsoft Edge",
        "bytes_sent": 204800000,
        "bytes_recv": 1073741824,
        "last_seen_ts": 1742670000
      }
    ],
    "total_apps": 47
  },
  "error": null
}
```

---

#### GET_DAEMON_STATUS

Returns daemon health and performance metrics.

**Response payload:**
```json
{
  "version": "1.0.0",
  "uptime_seconds": 86400,
  "memory_bytes": 4718592,
  "cpu_percent_1m": 0.08,
  "last_poll_ts": 1742678340,
  "next_poll_ts": 1742678400,
  "poll_interval_seconds": 60,
  "db_size_bytes": 8388608,
  "import_status": "complete",
  "import_progress_pct": 100
}
```

---

#### SET_SETTINGS

Updates daemon configuration. Applied immediately (hot-reload).

**Request payload:**
```json
{
  "poll_interval_seconds": 60,
  "retention_days": 365,
  "afk_idle_threshold_seconds": 300
}
```

---

#### GET_AFK_AUDIT

Returns AFK windows with associated usage.

**Request payload:**
```json
{
  "start_ts": 1742592000,
  "end_ts": 1742678400
}
```

**Response payload:**
```json
{
  "afk_windows": [
    {
      "start_ts": 1742610000,
      "end_ts": 1742613600,
      "duration_seconds": 3600,
      "bytes_sent": 10240,
      "bytes_recv": 5368709120,
      "top_apps": [
        { "process_name": "MoUsoCoreWorker.exe", "bytes_recv": 5100000000 }
      ]
    }
  ]
}
```

---

#### SUBSCRIBE_EVENTS (Push)

Client sends a subscribe request; daemon pushes events as they occur.

**Subscribe request:**
```json
{
  "id": "i9j0k1l2-...",
  "type": "request",
  "method": "SUBSCRIBE_EVENTS",
  "payload": { "event_types": ["poll_complete", "alert_triggered", "import_progress"] }
}
```

**Daemon push event example:**
```json
{
  "id": null,
  "type": "event",
  "method": "alert_triggered",
  "payload": {
    "alert_id": "cap_80pct",
    "message": "You have used 80% of your monthly cap",
    "current_bytes": 42949672960,
    "cap_bytes": 53687091200,
    "ts": 1742678400
  },
  "error": null
}
```

---

### 7.4 Error Codes

| Code | Meaning |
|---|---|
| 400 | Bad request — missing or invalid parameters |
| 404 | No data found for specified range or entity |
| 409 | Conflict — settings update rejected (invalid value) |
| 500 | Internal daemon error — see daemon log |
| 503 | Daemon temporarily busy (import in progress) |

---

## 8. Data Models

### 8.1 Database Schema

```sql
-- Network interfaces / adapters
CREATE TABLE interfaces (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    guid        TEXT NOT NULL UNIQUE,           -- Windows adapter GUID
    name        TEXT NOT NULL,                  -- Human-readable name
    type        TEXT NOT NULL,                  -- 'ethernet', 'wifi', 'loopback', 'other'
    is_metered  INTEGER NOT NULL DEFAULT 0,     -- 0 = unmetered, 1 = metered (from Windows profile)
    first_seen  INTEGER NOT NULL,               -- Unix timestamp
    last_seen   INTEGER NOT NULL
);
CREATE INDEX idx_interfaces_guid ON interfaces(guid);

-- Applications / processes
CREATE TABLE apps (
    id           INTEGER PRIMARY KEY AUTOINCREMENT,
    process_name TEXT NOT NULL UNIQUE,          -- e.g., 'msedge.exe'
    display_name TEXT,                          -- From version info / registry, nullable
    first_seen   INTEGER NOT NULL,
    last_seen    INTEGER NOT NULL
);
CREATE INDEX idx_apps_process_name ON apps(process_name);

-- Raw usage delta records (primary fact table)
CREATE TABLE usage_records (
    id              INTEGER PRIMARY KEY AUTOINCREMENT,
    ts              INTEGER NOT NULL,            -- Unix timestamp of poll end (bucket end)
    app_id          INTEGER NOT NULL REFERENCES apps(id),
    interface_id    INTEGER NOT NULL REFERENCES interfaces(id),
    bytes_sent      INTEGER NOT NULL DEFAULT 0, -- Delta bytes TX in this interval
    bytes_recv      INTEGER NOT NULL DEFAULT 0, -- Delta bytes RX in this interval
    interval_secs   INTEGER NOT NULL DEFAULT 60,-- Actual poll interval (may vary)
    source          TEXT NOT NULL DEFAULT 'poll' -- 'poll' | 'import'
);
CREATE INDEX idx_usage_ts ON usage_records(ts);
CREATE INDEX idx_usage_app_ts ON usage_records(app_id, ts);
CREATE INDEX idx_usage_iface_ts ON usage_records(interface_id, ts);

-- AFK / session idle windows
CREATE TABLE afk_windows (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    start_ts    INTEGER NOT NULL,
    end_ts      INTEGER NOT NULL,
    source      TEXT NOT NULL DEFAULT 'wts'     -- Detection method
);
CREATE INDEX idx_afk_start ON afk_windows(start_ts);
CREATE INDEX idx_afk_end ON afk_windows(end_ts);

-- Alert definitions
CREATE TABLE alert_definitions (
    id              INTEGER PRIMARY KEY AUTOINCREMENT,
    name            TEXT NOT NULL,
    alert_type      TEXT NOT NULL,              -- 'cap_percent', 'daily_cap', 'anomaly'
    interface_id    INTEGER REFERENCES interfaces(id),  -- NULL = all interfaces
    threshold_value REAL NOT NULL,              -- e.g., 0.80 for 80%, or bytes for daily cap
    cap_bytes       INTEGER,                    -- Monthly cap in bytes (nullable)
    is_active       INTEGER NOT NULL DEFAULT 1,
    created_at      INTEGER NOT NULL
);

-- Alert event history
CREATE TABLE alert_events (
    id              INTEGER PRIMARY KEY AUTOINCREMENT,
    definition_id   INTEGER NOT NULL REFERENCES alert_definitions(id),
    fired_at        INTEGER NOT NULL,
    current_bytes   INTEGER NOT NULL,
    message         TEXT NOT NULL
);
CREATE INDEX idx_alert_events_ts ON alert_events(fired_at);

-- Application settings (key-value)
CREATE TABLE settings (
    key     TEXT PRIMARY KEY,
    value   TEXT NOT NULL,
    updated_at INTEGER NOT NULL
);

-- Import log (for deduplication and audit)
CREATE TABLE import_log (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    started_at  INTEGER NOT NULL,
    completed_at INTEGER,
    periods_imported INTEGER,
    status      TEXT NOT NULL               -- 'running', 'complete', 'failed'
);
```

### 8.2 Data Validation Rules

| Field | Rule |
|---|---|
| `usage_records.bytes_sent` | ≥ 0; reject negative deltas (counter regression logged, not stored) |
| `usage_records.bytes_recv` | ≥ 0; same rule |
| `usage_records.ts` | Must be within ±5 minutes of wall clock at insert time |
| `usage_records.interval_secs` | 10–600; values outside range clamped with warning |
| `apps.process_name` | Non-empty, max 260 chars (MAX_PATH), stripped of null bytes |
| `interfaces.guid` | Must match Windows GUID format `{xxxxxxxx-xxxx-xxxx-xxxx-xxxxxxxxxxxx}` |
| `alert_definitions.threshold_value` | 0 < value ≤ 1.0 for `cap_percent`; > 0 for `daily_cap` |
| `settings.value` | Max 4096 chars; JSON-validated where applicable |

### 8.3 Storage Requirements

| Scenario | Estimated DB Size |
|---|---|
| 1 app, 1 interface, 60s polls, 1 year | ~4 MB |
| 50 apps, 3 interfaces, 60s polls, 1 year | ~200 MB |
| 200 apps, 5 interfaces, 60s polls, 2 years | ~1.5 GB |
| Typical home user (20 apps, 2 interfaces, 1 year) | ~30 MB |

**Notes:**
- SQLite WAL journal adds temporary overhead (< 10 MB typical)
- `VACUUM` compacts the DB after bulk deletes; run monthly or after import
- Hourly pre-aggregated summary table (not shown) to be added in Phase 4 for query performance at scale

---

## 9. Implementation Plan

### Team Composition

| Role | Headcount | Responsibilities |
|---|---|---|
| Rust Systems Engineer | 1 | Daemon, IPC server, SQLite integration, Windows API bindings |
| C# / WinUI Engineer | 1 | Viewer GUI, charts, settings UI, IPC client |
| QA / Automation Engineer | 0.5 | Test matrix, performance benchmarks, regression tests |
| Product / Design | 0.5 | UX flows, design review, requirements clarification |

---

### Phase 0 — Foundations (Weeks 1–2)

**Goal:** All foundational decisions locked; no ambiguity entering build phase.

| Task | Owner | Effort |
|---|---|---|
| Confirm Windows API availability on target Win11 builds | Rust Eng | 2d |
| Define polling + delta computation logic (spec doc) | Rust Eng + PM | 1d |
| Draft SQLite schema v1 (reviewed, approved) | Rust Eng | 1d |
| Define IPC message protocol (full spec) | Both Eng | 1d |
| Set up CI/CD pipeline (GitHub Actions, MSIX signing test) | Both | 2d |
| Dev environment setup + crate selection (`rusqlite`, `windows`, etc.) | Rust Eng | 1d |
| WinUI 3 project scaffold, navigation framework | C# Eng | 2d |

**Exit criteria:** Schema v1 approved; IPC protocol doc signed off; CI green; local build compiles.

---

### Phase 1 — Collector + DB (Weeks 3–5)

**Goal:** Daemon fully functional; data flowing to SQLite; import working.

| Task | Owner | Effort |
|---|---|---|
| Windows Service scaffolding (install, start, stop, uninstall) | Rust Eng | 2d |
| Implement `GetIfTable2` poller with delta computation | Rust Eng | 2d |
| Implement `GetNetworkUsageList` poller | Rust Eng | 2d |
| Counter wrap / anomaly handling | Rust Eng | 1d |
| SQLite write layer (prepared statements, WAL, error handling) | Rust Eng | 2d |
| Initial history import pipeline + deduplication | Rust Eng | 3d |
| IPC named pipe server (request/response + event push) | Rust Eng | 3d |
| AFK detection (`WTSQuerySessionInformation`) | Rust Eng | 1d |
| Daemon performance profiling (≤5MB target validation) | Rust Eng + QA | 2d |
| Logging + local crash report scaffolding | Rust Eng | 1d |

**Exit criteria:** Daemon installs as service; polling confirmed accurate vs. `netstat -e`; import completes in < 60s on reference hardware; RAM ≤ 5 MB confirmed.

---

### Phase 2 — Viewer v0 (Weeks 6–8)

**Goal:** Basic GUI operational; user can view data end-to-end.

| Task | Owner | Effort |
|---|---|---|
| IPC client implementation (named pipe, async) | C# Eng | 2d |
| Dashboard view (totals widget, basic bar chart) | C# Eng | 3d |
| Per-app breakdown table (sortable, filterable) | C# Eng | 2d |
| Per-interface breakdown view | C# Eng | 1d |
| Date range picker + filter bar | C# Eng | 2d |
| App detail drill-down view | C# Eng | 2d |
| System tray icon + context menu | C# Eng | 1d |
| CSV/JSON export flow | C# Eng | 1d |
| First-run onboarding wizard (import progress) | C# Eng | 2d |
| Light/dark theme support | C# Eng | 1d |

**Exit criteria:** User can install, see import progress, view dashboard, drill into apps, and export CSV. End-to-end QA pass on 2 hardware configs.

---

### Phase 3 — MVP Polish (Weeks 9–10)

**Goal:** MVP feature complete; performance hardened; ready for limited release.

| Task | Owner | Effort |
|---|---|---|
| Alert system: definition UI + daemon alert evaluation | Both | 3d |
| Windows toast notifications for alerts | C# Eng | 1d |
| Retention policy settings + nightly cleanup job | Rust Eng | 1d |
| Settings view (all configurable options) | C# Eng | 2d |
| Hot-reload settings via IPC (SET_SETTINGS) | Both | 1d |
| Performance hardening + memory profiling sweep | Rust Eng | 2d |
| MSIX installer + winget submission prep | Both | 2d |
| QA matrix (Win11 22H2, 23H2, 24H2; 3 hardware configs) | QA | 3d |
| Accessibility audit (keyboard nav, screen reader) | C# Eng | 1d |
| Documentation: README, user guide v1 | PM | 2d |

**Exit criteria:** All P0 user stories pass acceptance criteria; daemon memory ≤ 5 MB on all QA configs; no P0 bugs; MSIX installs cleanly.

---

### Phase 4 — Advanced Analytics (Weeks 11–16, Post-MVP)

| Feature | Owner | Effort |
|---|---|---|
| Usage heatmap view | C# Eng + Rust Eng | 5d |
| Predictive cost forecasting (regression model + UI) | Rust Eng + C# Eng | 5d |
| AFK audit view (timeline overlay) | Both | 5d |
| Anomaly detection engine + alert integration | Rust Eng | 4d |
| Pre-aggregated hourly summary table (query optimization) | Rust Eng | 2d |
| Extended export: PDF report, Excel | C# Eng | 3d |
| Settings: notifications, auto-export scheduling | C# Eng | 2d |

---

## 10. Success Metrics

### 10.1 Performance KPIs

| KPI | Target | Measurement Method | Review Cadence |
|---|---|---|---|
| Daemon memory (steady-state) | ≤ 5 MB RSS | `GetProcessMemoryInfo` sampled every 5 min over 24h in QA harness | Every build (CI gate) |
| Daemon CPU (average) | ≤ 0.2% on 4-core/8-thread | `GetProcessTimes` averaged over 24h continuous run | Every build (CI gate) |
| Daemon CPU (poll spike) | ≤ 2% for < 500ms per poll | Same method, peak tracking | Weekly review |
| DB growth rate (typical) | < 50 MB/year | Simulated 1-year run with 20 apps, 2 interfaces | Per phase milestone |
| GUI launch time (cold) | < 2 seconds | Automated UI test timer from process start to dashboard rendered | Every release |
| Import time (60 days) | < 60 seconds | QA timed benchmark, reference i5 hardware, SSD | Phase 1 gate |

### 10.2 Accuracy & Reliability KPIs

| KPI | Target | Measurement Method | Review Cadence |
|---|---|---|---|
| Tracking accuracy vs OS counters | ≤ 0.1% deviation | Automated regression: run side-by-side with `GetIfTable2` raw reads for 1h, compare totals | Every build |
| Import accuracy | ≤ 0.1% deviation from Windows history | Compare imported totals vs `GetNetworkUsageList` direct query | Phase 1 gate |
| Crash-free sessions | ≥ 99.5% per month | Local crash report file count / total session count | Monthly review |
| Import success rate | ≥ 99% on supported configs | QA matrix across 3 hardware configs × 3 Win11 versions | Phase 1 gate |
| Alert delivery accuracy | 100% (no missed alerts, no false positives in lab) | Alert test harness: trigger known usage events, verify notifications | Phase 3 gate |

### 10.3 User Value KPIs

| KPI | Target | Measurement Method | Review Cadence |
|---|---|---|---|
| Time-to-first-insight | < 2 minutes from installer launch | UX timer in onboarding (local, opt-in telemetry) | First release |
| Alert configuration rate | ≥ 40% of active users set ≥ 1 alert | Local settings file audit (no cloud) | 30/60/90 days post-launch |
| Weekly active viewer sessions | ≥ 30% retention at day 30 | Local session log (opt-in) | Monthly |
| Export usage | ≥ 15% of users export data in first 30 days | Local export log (opt-in) | 30 days post-launch |
| Crash report resolution time | ≤ 5 business days for P0 crashes | GitHub Issues tracking | Weekly |

### 10.4 Review Schedule

| Milestone | Review Type | Participants |
|---|---|---|
| End of Phase 0 | Architecture review | All team |
| End of Phase 1 | Performance gate review | Eng + QA |
| End of Phase 2 | UX walkthrough + QA | All team |
| End of Phase 3 (MVP) | Full QA matrix + go/no-go | All team + stakeholder |
| 30 days post-launch | Metrics review | PM + Eng lead |
| 90 days post-launch | Roadmap planning | All team |

---

*Document prepared for Singularity Monitor development team. All placeholder values (marked in scaffold) are reflected as targets in this document and should be validated against actual hardware benchmarks during Phase 1. Update this document when values are confirmed.*
