# Go / No-Go Checklist

## Release blockers

- Signed MSIX release artifact is produced from `.github\workflows\release.yml`
- Winget manifests are generated and validated from the same signed artifact
- Signed `winget` install, upgrade, and uninstall rehearsal is recorded in `docs\winget-rehearsal-runbook.md`
- Daemon service install, start, restart, and uninstall flows are verified on target hardware
- Accessibility audit follow-up is complete on final packaged builds
- Completed per-machine QA runbooks in `docs\qa-matrix.md` are all marked pass or have an accepted disposition
- User-facing docs are current: `docs\install.md`, `docs\user-guide.md`, `docs\troubleshooting.md`

## Shipping evidence

- Build and test logs
- Release asset URL
- Winget manifest validation output
- Signed `winget` rehearsal notes
- Accessibility notes
- Per-machine QA runbook sign-off

## No-go conditions

- Package publisher and signing certificate subject do not match
- Install, upgrade, or uninstall fails on any target configuration
- Performance gates regress beyond the PRD thresholds
- Critical viewer workflows fail without a running daemon or helper
- Unresolved data-integrity or export-safety issues remain open
