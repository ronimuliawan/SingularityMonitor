# Troubleshooting

## Viewer says the daemon is offline

1. Check the service state:

```bat
scripts\service-status.cmd
```

2. If it is stopped, run from an elevated terminal:

```bat
scripts\service-start.cmd
```

3. If the service is missing, reinstall it:

```bat
scripts\build-rust.cmd --release
scripts\service-install.cmd
scripts\service-start.cmd
```

## No data appears after install

- Wait for at least one daemon poll cycle.
- Confirm the daemon status card updates when you press `Refresh`.
- Run the 60-day import from onboarding or `Import 60 Days` if you need immediate history.
- Check `%ProgramData%\SingularityMonitor\daemon.log.jsonl` for collector errors.

## Helper actions fail in the packaged viewer

- The packaged release flow expects `helper.exe` to be bundled with the viewer package.
- If you are running an older or custom build, set `SM_HELPER_PATH` to a valid helper binary before launching the viewer.
- Check `%ProgramData%\SingularityMonitor\helper-reliability.jsonl` for helper startup failures.

## Export failed or created the wrong file

- Re-run the export after confirming the date range and app scope.
- CSV export now writes a unique file name to avoid overwriting an earlier export from the same second.
- If export still fails, confirm the target folder is writable and the daemon responds to refresh requests.

## MSIX installation is blocked

- Verify the package is signed by a trusted certificate.
- If you are testing a locally signed build, import the certificate chain before installing.
- Use the packaged `Install.ps1` when Windows refuses the double-click install path.

## Collect diagnostics

Useful paths:

- `%ProgramData%\SingularityMonitor\daemon.log.jsonl`
- `%ProgramData%\SingularityMonitor\helper-reliability.jsonl`
- `%LocalAppData%\SingularityMonitor\viewer-reliability.jsonl`

Useful commands:

```bat
scripts\service-status.cmd
scripts\run-daemon-console.cmd
```

For regression checks, use the smoke and performance scripts listed in `README.md`.
When a QA step fails, record the exact step, symptom, and collected log paths in `docs\qa-matrix.md`.
