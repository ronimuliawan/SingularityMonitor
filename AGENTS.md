# AGENTS.md
Guidance for coding agents operating in this repository.

## 1) Scope and priorities
- Platform: Windows 11 only.
- Product: low-overhead network monitor (Rust daemon + Rust helper + WinUI viewer).
- Primary constraints: correctness, data integrity, daemon memory/CPU budget.
- Keep changes focused; avoid broad refactors unless explicitly requested.

## 2) Repository map
- `crates/daemon`: Rust collector service, DB write/query path, IPC server.
- `crates/helper`: Rust WinRT attribution helper (session/user side).
- `crates/shared-contracts`: IPC envelope and request/response DTOs.
- `crates/perf-harness`: memory/perf tooling.
- `viewer`: WinUI 3 (.NET 8) desktop app.
- `scripts`: build, run, service, and smoke validation scripts.

## 3) Agent rule files in this repo
Checked locations:
- `.cursor/rules/`
- `.cursorrules`
- `.github/copilot-instructions.md`
Status: none found at the time this file was written.
If these files are added later, treat them as higher-priority instructions.

## 4) Build commands (run from `D:\projects\SingularityMonitor`)
Preferred entrypoints:
- Full build (Rust release + viewer release): `scripts\build-all.cmd`
- Rust only (debug/release via args):
  - `scripts\build-rust.cmd`
  - `scripts\build-rust.cmd --release`
- Viewer only: `scripts\build-viewer.cmd`
Direct toolchain builds:
- Rust workspace: `cargo build --workspace` and `cargo build --workspace --release`
- Viewer: `dotnet build "viewer\SingularityMonitor.Viewer.csproj" -c Release`
Note: `scripts\build-rust.cmd` loads VS Build Tools env via `VsDevCmd.bat`.

## 5) Lint and formatting commands
No dedicated lint script exists yet; use tool defaults.
- Rust format: `cargo fmt --all`
- Rust format check: `cargo fmt --all -- --check`
- Rust lint (if Clippy installed): `cargo clippy --workspace --all-targets -- -D warnings`
- C# format (if dotnet-format installed):
  - `dotnet format "viewer\SingularityMonitor.Viewer.csproj" --verify-no-changes`

## 6) Test commands (especially single test)
Full tests:
- `cargo test --workspace`
Package tests:
- `cargo test -p daemon`
- `cargo test -p shared-contracts`
Single Rust test (exact):
- `cargo test -p daemon delta::tests::small_regression_is_treated_as_reset -- --exact`
- `cargo test -p daemon runtime::tests::uses_elapsed_seconds_when_available -- --exact`
Module substring test:
- `cargo test -p daemon delta::tests::`
.NET tests:
- There is currently no dedicated viewer test project.
- If one is added: `dotnet test <path-to-test-csproj> --filter "FullyQualifiedName~<TestName>"`

## 7) Runtime and smoke validation commands
Runtime helpers:
- `scripts\run-daemon-console.cmd`
- `scripts\run-helper-loop.cmd`
- `scripts\import-history.cmd`
Smoke scripts (PowerShell):
- `scripts\m0-feasibility.ps1`
- `scripts\m1-attribution-smoke.ps1`
- `scripts\m2-overlap-dedupe-smoke.ps1`
- `scripts\m3-metered-flag-smoke.ps1`
- `scripts\m3-sleep-resume-continuity-smoke.ps1`
- `scripts\m4-settings-hotreload-smoke.ps1`
- `scripts\p0-07-accuracy-smoke.ps1`
- `scripts\p0-16-export-perf-smoke.ps1`

## 8) General coding conventions
- Keep contracts backward-compatible when possible.
- Prefer additive changes over breaking schema/API changes.
- Avoid unrelated formatting churn in untouched files.
- Preserve existing architecture and naming patterns.
- Add comments only for non-obvious logic.
- Update related docs/scripts/tests when behavior changes.

## 9) Rust style guidelines
- Edition: 2024 (workspace default).
Naming:
- Types/enums/traits: `PascalCase`
- Functions/variables/modules: `snake_case`
- Constants: `SCREAMING_SNAKE_CASE`
Imports/formatting:
- Keep `use` statements grouped and rustfmt-compatible.
- Run `cargo fmt` on Rust edits.
Types and APIs:
- Prefer explicit units in names (`*_ts`, `*_secs`, bytes).
- Use typed DTOs from `shared-contracts`; do not duplicate IPC schemas.
Error handling:
- Use `anyhow::Result` at fallible boundaries.
- Add `Context` for external/system failures.
- Runtime/server paths should return recoverable errors, not panic.
- `expect` only for clear internal invariants.
Numeric safety:
- Use `saturating_*`, `clamp`, and bounds checks for counters/settings.
Unsafe and Win32:
- Keep `unsafe` blocks minimal and localized.
- Validate OS return codes and map to structured errors.
IPC behavior:
- Add/modify method constants in `shared-contracts` first.
- Invalid payloads -> `400`; internal failures -> `500`.
Tests:
- Add tests in `#[cfg(test)]` near changed logic.

## 10) C# / WinUI style guidelines
Project settings:
- Nullable is enabled; honor nullable annotations.
Naming:
- Public types/members: `PascalCase`
- Private fields: `camelCase`
- Event handlers: `OnXxx...`
- Async `Task` methods: suffix `Async`
Imports/formatting:
- Keep `using` directives tidy and scoped to file conventions.
- Prefer formatter-friendly C# style; avoid unrelated style churn.
Async/UI behavior:
- `async void` only for UI event handlers.
- Keep long work off UI thread; marshal UI updates via dispatcher when needed.
IPC/JSON models:
- Keep C# properties `PascalCase`.
- Map daemon snake_case fields via `[JsonPropertyName("snake_case")]`.
- Treat daemon contract fields as source of truth.
Resilience:
- Fail gracefully when daemon is offline.
- Surface user-facing status text instead of throwing to UI.
- Preserve tray behavior and proper disposal on shutdown.

## 11) PowerShell script conventions
- Start scripts with `param(...)` and `$ErrorActionPreference = "Stop"`.
- Use helper functions for named-pipe request/response logic.
- Use isolated temp `SM_DATA_ROOT` in smoke scripts.
- Restore env vars and cleanup temp paths in `finally`.
- Throw on validation failures (non-zero exit).

## 12) Data and contract conventions
- Settings persist in SQLite `settings` table.
- IPC uses newline-delimited JSON over `\\.\pipe\SingularityMonitor`.
- Keep DTO changes synchronized across:
  - `crates/shared-contracts`
  - daemon request handlers + DB layer
  - viewer client models + UI wiring
- Preserve source-cutover/dedupe behavior for analytics queries.

## 13) Minimal verification checklist per change
- Build: `scripts\build-all.cmd`
- Tests: `cargo test --workspace`
- IPC/settings changes: `scripts\m4-settings-hotreload-smoke.ps1`
- Collector continuity changes: `scripts\m3-sleep-resume-continuity-smoke.ps1`
- Export/query path changes: `scripts\p0-16-export-perf-smoke.ps1`
- Overlap/dedupe changes: `scripts\m2-overlap-dedupe-smoke.ps1`

## 14) What to avoid
- Do not introduce alternate IPC schemas outside shared contracts.
- Do not bypass validation/clamping for settings and counters.
- Do not add heavyweight background processing in viewer.
- Do not silently change units, timestamp semantics, or source tags.
