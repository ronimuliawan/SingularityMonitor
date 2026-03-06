# User Guide

## First launch

- Start the viewer after the daemon service is running.
- On a clean install, use `Run Initial 60-Day Import` to backfill recent Windows usage history.
- If you skip onboarding, you can still run the import later from the `Daemon Status` card.

## Dashboard basics

- Use `Overview Mode` to switch between calendar-style totals and the active selected range.
- Use the top export row to choose interface scope, export granularity, and app scope.
- The summary cards show total usage plus upload-share indicators for today, this week, and this month.

## Date ranges and filters

- Use the `Preset`, `From`, and `To` controls in `Top Apps` to change the analysis window.
- The same active range also drives AFK, alerts history, app detail, and interface breakdown panels.
- Turn on `AFK only` in `Top Apps` to limit the list to apps seen during AFK windows.

## Top apps and detail views

- `Top Apps` sorts by total, upload, download, or app name.
- Selecting a row opens the `App Detail` section with chart-style buckets and a table view.
- Low-volume apps are grouped into `Other (< 1 MB each)` and unattributed system traffic is grouped into `System`.

## AFK and alerts

- `AFK Timeline` shows idle windows and the top apps active during each window.
- `Monthly Caps` lets you define global or per-interface caps.
- `Alerts History` shows the threshold events already recorded by the daemon.

## Settings and maintenance

- `Collector Settings` controls poll interval, retention, AFK threshold, and export defaults.
- `Compact DB` runs a manual SQLite maintenance pass to reclaim space.
- `Collector Target` summarizes daemon memory and reliability information.

## Tray behavior

- Closing the window hides the viewer to the tray instead of exiting it.
- Use the tray menu to reopen the dashboard, refresh tooltip data, or exit the app.
- The tray tooltip shows current-day usage while the viewer stays hidden.
