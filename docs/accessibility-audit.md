# Accessibility Audit

## Scope

This audit covers the WinUI viewer shell in `viewer\Views\MainPage.xaml` with emphasis on keyboard reachability, Narrator naming, status announcements, and theme safety.

## Changes landed

- Replaced hard-coded page and card colors with theme-aware brushes defined in `viewer\App.xaml`.
- Added explicit `AutomationProperties.Name` values to key filters, range pickers, settings inputs, refresh actions, cap controls, and detail controls.
- Marked status areas such as daemon status, import progress, settings feedback, reliability text, AFK status, alerts status, and chart-empty states as polite live regions.
- Restored keyboard reachability for read-only results lists that were previously removed from the tab sequence.
- Marked the decorative top-app glyph chrome as raw accessibility content so Narrator focuses on the row data instead of the glyph shell.

## Manual verification checklist

Run this checklist during final QA on each target Windows 11 configuration:

- Keyboard-only: tab from the top export controls through settings, top apps, AFK, caps, alerts, app detail, and interface breakdown without dead ends.
- Narrator: confirm combo boxes and number boxes are announced with their explicit names and that status updates are announced when refresh/import/settings actions complete.
- High contrast: confirm the page background, cards, primary text, warning text, and buttons remain readable with high-contrast themes enabled.
- Text scaling: validate the page at 100%, 150%, and 200% text scaling without clipped labels or unreachable controls.

## Remaining validation work

- Run Accessibility Insights for Windows against the packaged viewer build.
- Capture any per-control focus-order issues discovered on touch or screen-reader hardware.
- Re-check the packaged experience after the final release publisher is configured, since package identity can affect install and first-run paths.
