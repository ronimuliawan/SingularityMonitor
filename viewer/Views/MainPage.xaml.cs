using System;
using System.Collections.Generic;
using System.IO;
using System.Linq;
using System.Text;
using System.Text.Json;
using System.Threading.Tasks;
using Microsoft.UI.Xaml.Controls;

namespace SingularityMonitor.Viewer.Views
{
    public partial class MainPage : Page
    {
        private readonly Services.DaemonClient daemonClient = new();
        private readonly Services.HelperManager helperManager = new();
        private const ulong OtherGroupThresholdBytes = 1024 * 1024;
        private const int GroupDetailMemberLimit = 20;
        private const uint DefaultPollIntervalSeconds = 60;
        private const uint DefaultRetentionDays = 0;
        private const uint DefaultAfkIdleThresholdSeconds = 300;

        private string startupStatus = string.Empty;
        private string? selectedInterfaceId;
        private string? selectedInterfaceType;
        private string selectedTopAppsSort = "total_desc";
        private bool pageReady;

        public MainPage()
        {
            this.InitializeComponent();
            Loaded += OnLoaded;
        }

        private async void OnLoaded(object sender, RoutedEventArgs e)
        {
            if (pageReady)
            {
                return;
            }

            InitializeDateRangePickers();
            InitializeExportControls();
            InitializeTopAppsControls();
            InitializeAppDetailControls();
            InitializeSettingsControls();
            await LoadInterfaceOptionsAsync();
            pageReady = true;

            var registration = helperManager.EnsureRunAtLogin();
            var loop = helperManager.EnsureLoopRunning();
            startupStatus = registration + " " + loop;
            HelperStatusText.Text = startupStatus;

            await RefreshStatusAsync();
            await LoadSettingsAsync();
            await RefreshOverviewAsync();
            await RefreshAppsAsync();
            await RefreshSelectedAppDetailAsync();
            await RefreshInterfaceBreakdownAsync();
        }

        private void InitializeExportControls()
        {
            ExportGranularityComboBox.ItemsSource = new[] { "hour", "day", "week", "month" };
            ExportGranularityComboBox.SelectedItem = "day";
        }

        private void InitializeTopAppsControls()
        {
            var options = new List<AppSortOption>
            {
                new() { Key = "total_desc", DisplayName = "Total usage" },
                new() { Key = "upload_desc", DisplayName = "Upload" },
                new() { Key = "download_desc", DisplayName = "Download" },
                new() { Key = "name_asc", DisplayName = "App name" },
            };

            TopAppsSortComboBox.DisplayMemberPath = nameof(AppSortOption.DisplayName);
            TopAppsSortComboBox.ItemsSource = options;
            TopAppsSortComboBox.SelectedItem = options[0];
            selectedTopAppsSort = options[0].Key;
        }

        private void InitializeAppDetailControls()
        {
            AppDetailGranularityComboBox.ItemsSource = new[] { "hour", "day", "week", "month" };
            AppDetailGranularityComboBox.SelectedItem = "hour";
        }

        private void InitializeSettingsControls()
        {
            PollIntervalNumberBox.Value = DefaultPollIntervalSeconds;
            RetentionDaysNumberBox.Value = DefaultRetentionDays;
            AfkIdleThresholdNumberBox.Value = DefaultAfkIdleThresholdSeconds;
        }

        private void InitializeDateRangePickers()
        {
            var localNow = DateTimeOffset.Now;
            var todayStart = new DateTimeOffset(localNow.Year, localNow.Month, localNow.Day, 0, 0, 0, localNow.Offset);
            StartDatePicker.Date = todayStart.AddDays(-1);
            EndDatePicker.Date = todayStart;
        }

        private async Task LoadInterfaceOptionsAsync()
        {
            try
            {
                var response = await daemonClient.GetInterfacesAsync();
                var items = new List<InterfaceFilterOption>
                {
                    new() { DisplayName = "All Interfaces", InterfaceId = null, InterfaceType = null },
                    new() { DisplayName = "Wi-Fi Interfaces", InterfaceId = null, InterfaceType = "wifi" },
                    new() { DisplayName = "Ethernet Interfaces", InterfaceId = null, InterfaceType = "ethernet" },
                };

                var specificAdapters = response.Interfaces
                    .Where(ShouldIncludeAdapterInFilter)
                    .OrderBy(i => i.Name, StringComparer.OrdinalIgnoreCase)
                    .Select(i => new InterfaceFilterOption
                    {
                        DisplayName = BuildInterfaceDisplayName(i),
                        InterfaceId = i.Guid,
                        InterfaceType = null,
                    });
                items.AddRange(specificAdapters);

                InterfaceFilterComboBox.DisplayMemberPath = nameof(InterfaceFilterOption.DisplayName);
                InterfaceFilterComboBox.ItemsSource = items;

                var selected = items.FirstOrDefault(x => x.InterfaceId == selectedInterfaceId && x.InterfaceType == selectedInterfaceType)
                    ?? items[0];
                InterfaceFilterComboBox.SelectedItem = selected;
                selectedInterfaceId = selected.InterfaceId;
                selectedInterfaceType = selected.InterfaceType;
            }
            catch (Exception ex)
            {
                InterfaceFilterComboBox.ItemsSource = new List<InterfaceFilterOption>
                {
                    new() { DisplayName = "All Interfaces", InterfaceId = null, InterfaceType = null }
                };
                InterfaceFilterComboBox.SelectedIndex = 0;
                selectedInterfaceId = null;
                selectedInterfaceType = null;
                HelperStatusText.Text = $"Interface list unavailable: {ex.Message}. {startupStatus}";
            }
        }

        private async void OnRefreshClicked(object sender, RoutedEventArgs e)
        {
            await LoadInterfaceOptionsAsync();
            await RefreshStatusAsync();
            await LoadSettingsAsync();
            await RefreshOverviewAsync();
            await RefreshAppsAsync();
            await RefreshSelectedAppDetailAsync();
            await RefreshInterfaceBreakdownAsync();
        }

        private async void OnApplySettingsClicked(object sender, RoutedEventArgs e)
        {
            var triggerButton = sender as Button;
            if (triggerButton is not null)
            {
                triggerButton.IsEnabled = false;
            }

            try
            {
                await ApplySettingsFromInputsAsync();
            }
            catch (Exception ex)
            {
                SettingsStatusText.Text = $"Failed to save settings: {ex.Message}";
            }
            finally
            {
                if (triggerButton is not null)
                {
                    triggerButton.IsEnabled = true;
                }
            }
        }

        private async void OnResetSettingsClicked(object sender, RoutedEventArgs e)
        {
            PollIntervalNumberBox.Value = DefaultPollIntervalSeconds;
            RetentionDaysNumberBox.Value = DefaultRetentionDays;
            AfkIdleThresholdNumberBox.Value = DefaultAfkIdleThresholdSeconds;
            try
            {
                await ApplySettingsFromInputsAsync();
            }
            catch (Exception ex)
            {
                SettingsStatusText.Text = $"Failed to reset settings: {ex.Message}";
            }
        }

        private async Task ApplySettingsFromInputsAsync()
        {
            var poll = ResolveNumberBoxValue(PollIntervalNumberBox, DefaultPollIntervalSeconds, 15, 300);
            var retention = ResolveNumberBoxValue(RetentionDaysNumberBox, DefaultRetentionDays, 0, 3650);
            var afk = ResolveNumberBoxValue(AfkIdleThresholdNumberBox, DefaultAfkIdleThresholdSeconds, 30, 3600);

            await daemonClient.ApplySettingsAsync(
                pollIntervalSeconds: poll,
                retentionDays: retention,
                afkIdleThresholdSeconds: afk);

            SettingsStatusText.Text =
                $"Saved at {DateTimeOffset.Now:HH:mm:ss}. Poll={poll}s, Retention={retention}d, AFK={afk}s.";
            await RefreshStatusAsync();
            await LoadSettingsAsync();
        }

        private async void OnSummaryClicked(object sender, RoutedEventArgs e)
        {
            try
            {
                var summary = await daemonClient.GetTodaySummaryAsync();
                StatusText.Text =
                    $"Today usage: {FormatBytes(summary.TotalSent + summary.TotalRecv)} total " +
                    $"({FormatBytes(summary.TotalSent)} up, {FormatBytes(summary.TotalRecv)} down).";
            }
            catch (Exception ex)
            {
                StatusText.Text = $"Unable to query usage summary: {ex.Message}";
            }
        }

        private async void OnStartHelperClicked(object sender, RoutedEventArgs e)
        {
            var registration = helperManager.EnsureRunAtLogin();
            var start = helperManager.EnsureLoopRunning();
            startupStatus = registration + " " + start;
            HelperStatusText.Text = startupStatus;
            await RefreshStatusAsync();
        }

        private async void OnImportHistoryClicked(object sender, RoutedEventArgs e)
        {
            var triggerButton = sender as Button;
            if (triggerButton is not null)
            {
                triggerButton.IsEnabled = false;
            }

            try
            {
                HelperStatusText.Text = "Running 60-day import in helper process...";
                ImportProgressBar.Value = 0;

                var importTask = helperManager.RunHistoryImportAsync(days: 60, chunkHours: 6);
                while (!importTask.IsCompleted)
                {
                    await RefreshStatusAsync();
                    await Task.Delay(1000);
                }

                var output = await importTask;
                HelperStatusText.Text = output;

                await LoadInterfaceOptionsAsync();
                await RefreshStatusAsync();
                await RefreshOverviewAsync();
                await RefreshAppsAsync();
                await RefreshSelectedAppDetailAsync();
                await RefreshInterfaceBreakdownAsync();
            }
            finally
            {
                if (triggerButton is not null)
                {
                    triggerButton.IsEnabled = true;
                }
            }
        }

        private async void OnRefreshAppsClicked(object sender, RoutedEventArgs e)
        {
            await RefreshAppsAsync();
            await RefreshSelectedAppDetailAsync();
        }

        private async void OnRefreshInterfaceBreakdownClicked(object sender, RoutedEventArgs e)
        {
            await RefreshInterfaceBreakdownAsync();
        }

        private async void OnApplyRangeClicked(object sender, RoutedEventArgs e)
        {
            await RefreshAppsAsync();
            await RefreshSelectedAppDetailAsync();
            await RefreshInterfaceBreakdownAsync();
        }

        private async void OnInterfaceFilterChanged(object sender, SelectionChangedEventArgs e)
        {
            if (!pageReady)
            {
                return;
            }

            selectedInterfaceId = ResolveSelectedInterfaceId();
            selectedInterfaceType = ResolveSelectedInterfaceType();
            await RefreshOverviewAsync();
            await RefreshAppsAsync();
            await RefreshSelectedAppDetailAsync();
            await RefreshInterfaceBreakdownAsync();
        }

        private async void OnTopAppsSortChanged(object sender, SelectionChangedEventArgs e)
        {
            if (!pageReady)
            {
                return;
            }

            selectedTopAppsSort = ResolveSelectedTopAppsSortKey();
            await RefreshAppsAsync();
            await RefreshSelectedAppDetailAsync();
        }

        private async void OnTopAppSelectionChanged(object sender, SelectionChangedEventArgs e)
        {
            if (!pageReady)
            {
                return;
            }

            await RefreshSelectedAppDetailAsync();
        }

        private async void OnAppDetailGranularityChanged(object sender, SelectionChangedEventArgs e)
        {
            if (!pageReady)
            {
                return;
            }

            await RefreshSelectedAppDetailAsync();
        }

        private async void OnRefreshAppDetailClicked(object sender, RoutedEventArgs e)
        {
            await RefreshSelectedAppDetailAsync();
        }

        private async void OnExportCsvClicked(object sender, RoutedEventArgs e)
        {
            await ExportCurrentRangeAsync(asJson: false);
        }

        private async void OnExportJsonClicked(object sender, RoutedEventArgs e)
        {
            await ExportCurrentRangeAsync(asJson: true);
        }

        private async Task ExportCurrentRangeAsync(bool asJson)
        {
            try
            {
                var interfaceId = ResolveSelectedInterfaceId();
                var interfaceType = ResolveSelectedInterfaceType();
                var granularity = ResolveSelectedGranularity();
                var includeSummary = IncludeSummaryCheckBox.IsChecked == true;
                var includeApps = IncludeAppsCheckBox.IsChecked == true;
                var includeInterfaces = IncludeInterfacesCheckBox.IsChecked == true;

                if (!includeSummary && !includeApps && !includeInterfaces)
                {
                    HelperStatusText.Text = "Select at least one export section (summary/apps/interfaces).";
                    return;
                }

                var (startUtc, endUtc) = ResolveTopAppsRangeUtc();
                Services.UsageSummary? summary = null;
                Services.AppBreakdown? appBreakdown = null;
                Services.InterfaceBreakdownResponse? interfaceBreakdown = null;

                if (includeSummary)
                {
                    summary = await daemonClient.GetUsageSummaryAsync(
                        startUtc.ToUnixTimeSeconds(),
                        endUtc.ToUnixTimeSeconds(),
                        granularity: granularity,
                        interfaceId: interfaceId,
                        interfaceType: interfaceType);
                }

                if (includeApps)
                {
                    appBreakdown = await daemonClient.GetAppBreakdownAsync(
                        startUtc.ToUnixTimeSeconds(),
                        endUtc.ToUnixTimeSeconds(),
                        limit: 500,
                        interfaceId: interfaceId,
                        interfaceType: interfaceType);
                }

                if (includeInterfaces)
                {
                    interfaceBreakdown = await daemonClient.GetInterfaceBreakdownAsync(
                        startUtc.ToUnixTimeSeconds(),
                        endUtc.ToUnixTimeSeconds(),
                        interfaceId: interfaceId,
                        interfaceType: interfaceType);
                }

                var downloads = Path.Combine(Environment.GetFolderPath(Environment.SpecialFolder.UserProfile), "Downloads");
                Directory.CreateDirectory(downloads);
                var stamp = DateTimeOffset.Now.ToString("yyyyMMdd_HHmmss");

                if (asJson)
                {
                    var path = Path.Combine(downloads, $"singularity_export_{stamp}.json");
                    var payload = new
                    {
                        generated_at_utc = DateTimeOffset.UtcNow,
                        start_ts = startUtc.ToUnixTimeSeconds(),
                        end_ts = endUtc.ToUnixTimeSeconds(),
                        granularity,
                        interface_id = interfaceId,
                        interface_type = interfaceType,
                        apps_scope = interfaceId is not null || interfaceType is not null ? "filtered" : "global",
                        summary,
                        apps = appBreakdown?.Apps,
                        interfaces = interfaceBreakdown?.Interfaces,
                    };
                    var json = JsonSerializer.Serialize(payload, new JsonSerializerOptions { WriteIndented = true });
                    await File.WriteAllTextAsync(path, json);
                    HelperStatusText.Text = $"Exported JSON to {path}";
                }
                else
                {
                    var path = Path.Combine(downloads, $"singularity_export_{stamp}.csv");
                    var csv = BuildCsvExport(
                        startUtc.ToUnixTimeSeconds(),
                        endUtc.ToUnixTimeSeconds(),
                        interfaceId,
                        interfaceType,
                        granularity,
                        summary,
                        appBreakdown,
                        interfaceBreakdown);
                    await File.WriteAllTextAsync(path, csv);
                    HelperStatusText.Text = $"Exported CSV to {path}";
                }
            }
            catch (Exception ex)
            {
                HelperStatusText.Text = $"Export failed: {ex.Message}";
            }
        }

        private static string BuildCsvExport(
            long startTs,
            long endTs,
            string? interfaceId,
            string? interfaceType,
            string granularity,
            Services.UsageSummary? summary,
            Services.AppBreakdown? appBreakdown,
            Services.InterfaceBreakdownResponse? interfaceBreakdown)
        {
            var sb = new StringBuilder();
            sb.AppendLine("meta_key,meta_value");
            sb.AppendLine($"start_ts,{startTs}");
            sb.AppendLine($"end_ts,{endTs}");
            sb.AppendLine($"granularity,{EscapeCsv(granularity)}");
            sb.AppendLine($"interface_id,{EscapeCsv(interfaceId ?? "all")}");
            sb.AppendLine($"interface_type,{EscapeCsv(interfaceType ?? "all")}");
            sb.AppendLine();

            if (summary is not null)
            {
                sb.AppendLine("section,bucket_ts,bytes_sent,bytes_recv,total_bytes,granularity");
                foreach (var bucket in summary.Buckets)
                {
                    var total = bucket.BytesSent + bucket.BytesRecv;
                    sb.AppendLine(string.Join(",",
                        "summary",
                        bucket.Ts,
                        bucket.BytesSent,
                        bucket.BytesRecv,
                        total,
                        EscapeCsv(granularity)));
                }
                sb.AppendLine();
            }

            sb.AppendLine("section,process_name,display_name,bytes_sent,bytes_recv,total_bytes,interface_id,interface_name,interface_type,is_metered");
            if (appBreakdown is not null)
            {
                foreach (var app in appBreakdown.Apps)
                {
                    var total = app.BytesSent + app.BytesRecv;
                    sb.AppendLine(string.Join(",",
                        "app",
                        EscapeCsv(app.ProcessName),
                        EscapeCsv(app.DisplayName),
                        app.BytesSent,
                        app.BytesRecv,
                        total,
                        "",
                        "",
                        "",
                        ""));
                }
            }

            if (interfaceBreakdown is not null)
            {
                foreach (var iface in interfaceBreakdown.Interfaces)
                {
                    var total = iface.BytesSent + iface.BytesRecv;
                    sb.AppendLine(string.Join(",",
                        "interface",
                        "",
                        "",
                        iface.BytesSent,
                        iface.BytesRecv,
                        total,
                        EscapeCsv(iface.InterfaceId),
                        EscapeCsv(iface.InterfaceName),
                        EscapeCsv(iface.InterfaceType),
                        iface.IsMetered ? "1" : "0"));
                }
            }

            return sb.ToString();
        }

        private async Task LoadSettingsAsync()
        {
            try
            {
                var settings = await daemonClient.GetSettingsAsync();
                PollIntervalNumberBox.Value = settings.PollIntervalSeconds;
                RetentionDaysNumberBox.Value = settings.RetentionDays;
                AfkIdleThresholdNumberBox.Value = settings.AfkIdleThresholdSeconds;
                if (string.IsNullOrWhiteSpace(SettingsStatusText.Text))
                {
                    SettingsStatusText.Text = "Settings loaded from daemon.";
                }
            }
            catch (Exception ex)
            {
                SettingsStatusText.Text = $"Settings unavailable: {ex.Message}";
            }
        }

        private async Task RefreshStatusAsync()
        {
            try
            {
                var status = await daemonClient.GetDaemonStatusAsync();
                StatusText.Text =
                    $"Daemon v{status.Version} | Uptime {status.UptimeSeconds}s | " +
                    $"Poll {status.PollIntervalSeconds}s | Last poll {status.LastPollTs} | " +
                    $"Import {status.ImportStatus} {status.ImportProgressPct}%";

                ImportProgressBar.Value = Math.Clamp(status.ImportProgressPct, 0, 100);

                if (status.LastHelperIngestTs > 0)
                {
                    var ts = DateTimeOffset.FromUnixTimeSeconds(status.LastHelperIngestTs).ToLocalTime();
                    HelperStatusText.Text = $"Last helper ingestion: {ts:G}. {startupStatus}";
                }
                else
                {
                    HelperStatusText.Text = "No helper ingestion yet. Start helper loop or run import. " + startupStatus;
                }

                var memory = status.MemoryBytes / (1024d * 1024d);
                var memoryText = $"Current daemon RSS: {memory:F2} MB";
                if (memory <= 3.0)
                {
                    MemoryHintText.Text = memoryText + " (inside target).";
                }
                else if (memory <= 5.0)
                {
                    MemoryHintText.Text = memoryText + " (inside ceiling, above target).";
                }
                else
                {
                    MemoryHintText.Text = memoryText + " (over ceiling, needs optimization).";
                }
            }
            catch (Exception ex)
            {
                StatusText.Text = "Daemon unavailable. Start the collector service or run daemon --console.";
                HelperStatusText.Text = "Helper status unavailable while daemon is offline.";
                MemoryHintText.Text = $"Status check failed: {ex.Message}";
            }
        }

        private async Task RefreshOverviewAsync()
        {
            try
            {
                var interfaceId = ResolveSelectedInterfaceId();
                var interfaceType = ResolveSelectedInterfaceType();
                var localNow = DateTimeOffset.Now;
                var nowUtc = DateTimeOffset.UtcNow;

                var dayStartLocal = new DateTimeOffset(localNow.Year, localNow.Month, localNow.Day, 0, 0, 0, localNow.Offset);
                var weekStartLocal = dayStartLocal.AddDays(-ToMondayOffset(localNow.DayOfWeek));
                var monthStartLocal = new DateTimeOffset(localNow.Year, localNow.Month, 1, 0, 0, 0, localNow.Offset);

                var today = await daemonClient.GetUsageSummaryAsync(
                    dayStartLocal.ToUnixTimeSeconds(),
                    nowUtc.ToUnixTimeSeconds(),
                    granularity: "day",
                    interfaceId: interfaceId,
                    interfaceType: interfaceType);
                var week = await daemonClient.GetUsageSummaryAsync(
                    weekStartLocal.ToUnixTimeSeconds(),
                    nowUtc.ToUnixTimeSeconds(),
                    granularity: "day",
                    interfaceId: interfaceId,
                    interfaceType: interfaceType);
                var month = await daemonClient.GetUsageSummaryAsync(
                    monthStartLocal.ToUnixTimeSeconds(),
                    nowUtc.ToUnixTimeSeconds(),
                    granularity: "day",
                    interfaceId: interfaceId,
                    interfaceType: interfaceType);

                ApplySummaryCard(TodayTotalText, TodaySplitText, today);
                ApplySummaryCard(WeekTotalText, WeekSplitText, week);
                ApplySummaryCard(MonthTotalText, MonthSplitText, month);
            }
            catch
            {
                TodayTotalText.Text = "-";
                TodaySplitText.Text = "-";
                WeekTotalText.Text = "-";
                WeekSplitText.Text = "-";
                MonthTotalText.Text = "-";
                MonthSplitText.Text = "-";
            }
        }

        private async Task RefreshAppsAsync()
        {
            var previouslySelected = TopAppsList.SelectedItem as AppDisplayRow;
            try
            {
                var interfaceId = ResolveSelectedInterfaceId();
                var interfaceType = ResolveSelectedInterfaceType();
                var sortKey = ResolveSelectedTopAppsSortKey();
                selectedTopAppsSort = sortKey;
                var (startUtc, endUtc) = ResolveTopAppsRangeUtc();
                TopAppsRangeText.Text =
                    $"({startUtc.LocalDateTime:yyyy-MM-dd} to {endUtc.LocalDateTime.AddSeconds(-1):yyyy-MM-dd})";

                var breakdown = await daemonClient.GetAppBreakdownAsync(
                    startUtc.ToUnixTimeSeconds(),
                    endUtc.ToUnixTimeSeconds(),
                    limit: 500,
                    interfaceId: interfaceId,
                    interfaceType: interfaceType);

                var rows = BuildTopAppRows(breakdown.Apps, sortKey);

                if (rows.Count == 0)
                {
                    rows.Add(new AppDisplayRow
                    {
                        DisplayName = "No helper-attributed data in this range yet",
                        UsageText = "-",
                        DetailText = "",
                        IsPlaceholder = true,
                        SelectionKey = "placeholder:none",
                    });
                }

                TopAppsList.ItemsSource = rows;

                var selected = previouslySelected is null
                    ? null
                    : rows.FirstOrDefault(row => row.SelectionKey == previouslySelected.SelectionKey);
                selected ??= rows.FirstOrDefault(row => !row.IsPlaceholder);
                TopAppsList.SelectedItem = selected;
            }
            catch (Exception ex)
            {
                TopAppsList.ItemsSource = new List<AppDisplayRow>
                {
                    new()
                    {
                        DisplayName = $"Failed to load app breakdown: {ex.Message}",
                        UsageText = "-",
                        DetailText = "",
                        IsPlaceholder = true,
                        SelectionKey = "placeholder:error",
                    },
                };
                TopAppsList.SelectedItem = null;
            }
        }

        private async Task RefreshInterfaceBreakdownAsync()
        {
            try
            {
                var interfaceId = ResolveSelectedInterfaceId();
                var interfaceType = ResolveSelectedInterfaceType();
                var (startUtc, endUtc) = ResolveTopAppsRangeUtc();
                InterfaceBreakdownRangeText.Text =
                    $"({startUtc.LocalDateTime:yyyy-MM-dd} to {endUtc.LocalDateTime.AddSeconds(-1):yyyy-MM-dd})";

                var response = await daemonClient.GetInterfaceBreakdownAsync(
                    startUtc.ToUnixTimeSeconds(),
                    endUtc.ToUnixTimeSeconds(),
                    interfaceId: interfaceId,
                    interfaceType: interfaceType);

                var rows = response.Interfaces
                    .Select(row => new AppDisplayRow
                    {
                        DisplayName = BuildInterfaceUsageLabel(row),
                        UsageText = FormatBytes(row.BytesSent + row.BytesRecv),
                    })
                    .ToList();

                if (rows.Count == 0)
                {
                    rows.Add(new AppDisplayRow
                    {
                        DisplayName = "No interface-poll data in this range yet",
                        UsageText = "-",
                    });
                }

                InterfaceBreakdownList.ItemsSource = rows;
            }
            catch (Exception ex)
            {
                InterfaceBreakdownList.ItemsSource = new List<AppDisplayRow>
                {
                    new()
                    {
                        DisplayName = $"Failed to load interface breakdown: {ex.Message}",
                        UsageText = "-",
                    },
                };
            }
        }

        private async Task RefreshSelectedAppDetailAsync()
        {
            try
            {
                if (TopAppsList.SelectedItem is not AppDisplayRow selected || selected.IsPlaceholder)
                {
                    AppDetailTitleText.Text = "Select an app from the list";
                    AppDetailSummaryText.Text = "Pick an app to view time series buckets.";
                    AppDetailBucketsList.ItemsSource = new List<AppDetailBucketRow>();
                    return;
                }

                var interfaceId = ResolveSelectedInterfaceId();
                var interfaceType = ResolveSelectedInterfaceType();
                var granularity = ResolveSelectedAppDetailGranularity();
                var (startUtc, endUtc) = ResolveTopAppsRangeUtc();

                Services.UsageSummary summary;
                string groupNote = string.Empty;
                if (selected.IsAggregateGroup)
                {
                    var members = selected.GroupMembers
                        .Take(GroupDetailMemberLimit)
                        .ToArray();
                    summary = await QueryGroupedUsageSummaryAsync(
                        members,
                        startUtc.ToUnixTimeSeconds(),
                        endUtc.ToUnixTimeSeconds(),
                        granularity,
                        interfaceId,
                        interfaceType);

                    if (selected.GroupMembers.Length > GroupDetailMemberLimit)
                    {
                        groupNote = $" Showing top {GroupDetailMemberLimit} processes only.";
                    }
                }
                else
                {
                    summary = await daemonClient.GetUsageSummaryAsync(
                        startUtc.ToUnixTimeSeconds(),
                        endUtc.ToUnixTimeSeconds(),
                        granularity: granularity,
                        interfaceId: interfaceId,
                        interfaceType: interfaceType,
                        appFilter: selected.ProcessName);
                }

                var bucketRows = summary.Buckets
                    .OrderBy(bucket => bucket.Ts)
                    .Select(bucket => new AppDetailBucketRow
                    {
                        BucketLabel = FormatBucketLabel(bucket.Ts, granularity),
                        SplitText = $"Up {FormatBytes(bucket.BytesSent)} | Down {FormatBytes(bucket.BytesRecv)}",
                        UsageText = FormatBytes(bucket.BytesSent + bucket.BytesRecv),
                    })
                    .ToList();

                if (bucketRows.Count == 0)
                {
                    bucketRows.Add(new AppDetailBucketRow
                    {
                        BucketLabel = "No buckets in selected range",
                        SplitText = "-",
                        UsageText = "-",
                    });
                }

                var scope = BuildInterfaceScopeLabel(interfaceId, interfaceType);
                var appLabel = selected.IsAggregateGroup
                    ? selected.DisplayName
                    : $"{selected.DisplayName} ({selected.ProcessName})";
                AppDetailTitleText.Text = appLabel;
                AppDetailSummaryText.Text =
                    $"{scope} Total {FormatBytes(summary.TotalSent + summary.TotalRecv)} " +
                    $"(Up {FormatBytes(summary.TotalSent)} | Down {FormatBytes(summary.TotalRecv)})." +
                    groupNote;
                AppDetailBucketsList.ItemsSource = bucketRows;
            }
            catch (Exception ex)
            {
                AppDetailSummaryText.Text = $"Failed to load app detail: {ex.Message}";
                AppDetailBucketsList.ItemsSource = new List<AppDetailBucketRow>
                {
                    new()
                    {
                        BucketLabel = "Error",
                        SplitText = "-",
                        UsageText = "-",
                    },
                };
            }
        }

        private async Task<Services.UsageSummary> QueryGroupedUsageSummaryAsync(
            IReadOnlyCollection<AppGroupMember> members,
            long startTs,
            long endTs,
            string granularity,
            string? interfaceId,
            string? interfaceType)
        {
            var totalsByTs = new Dictionary<long, (ulong Sent, ulong Recv)>();
            ulong totalSent = 0;
            ulong totalRecv = 0;

            foreach (var member in members)
            {
                var summary = await daemonClient.GetUsageSummaryAsync(
                    startTs,
                    endTs,
                    granularity: granularity,
                    interfaceId: interfaceId,
                    interfaceType: interfaceType,
                    appFilter: member.ProcessName);

                totalSent = SaturatingAdd(totalSent, summary.TotalSent);
                totalRecv = SaturatingAdd(totalRecv, summary.TotalRecv);

                foreach (var bucket in summary.Buckets)
                {
                    totalsByTs.TryGetValue(bucket.Ts, out var current);
                    totalsByTs[bucket.Ts] = (
                        SaturatingAdd(current.Sent, bucket.BytesSent),
                        SaturatingAdd(current.Recv, bucket.BytesRecv));
                }
            }

            var mergedBuckets = totalsByTs
                .OrderBy(pair => pair.Key)
                .Select(pair => new Services.UsageBucket
                {
                    Ts = pair.Key,
                    BytesSent = pair.Value.Sent,
                    BytesRecv = pair.Value.Recv,
                })
                .ToArray();

            return new Services.UsageSummary
            {
                Buckets = mergedBuckets,
                TotalSent = totalSent,
                TotalRecv = totalRecv,
            };
        }

        private List<AppDisplayRow> BuildTopAppRows(IEnumerable<Services.AppBreakdownRow> apps, string sortKey)
        {
            var normalRows = new List<AppDisplayRow>();
            var systemMembers = new List<AppGroupMember>();
            var otherMembers = new List<AppGroupMember>();

            foreach (var app in apps)
            {
                var totalBytes = app.BytesSent + app.BytesRecv;
                var displayName = string.IsNullOrWhiteSpace(app.DisplayName) ? app.ProcessName : app.DisplayName;
                if (IsSystemGroupedApp(app.ProcessName, displayName))
                {
                    systemMembers.Add(new AppGroupMember
                    {
                        ProcessName = app.ProcessName,
                        DisplayName = displayName,
                        BytesSent = app.BytesSent,
                        BytesRecv = app.BytesRecv,
                        LastSeenTs = app.LastSeenTs,
                    });
                    continue;
                }

                if (totalBytes < OtherGroupThresholdBytes)
                {
                    otherMembers.Add(new AppGroupMember
                    {
                        ProcessName = app.ProcessName,
                        DisplayName = displayName,
                        BytesSent = app.BytesSent,
                        BytesRecv = app.BytesRecv,
                        LastSeenTs = app.LastSeenTs,
                    });
                    continue;
                }

                normalRows.Add(new AppDisplayRow
                {
                    DisplayName = displayName,
                    ProcessName = app.ProcessName,
                    BytesSent = app.BytesSent,
                    BytesRecv = app.BytesRecv,
                    LastSeenTs = app.LastSeenTs,
                    UsageText = FormatBytes(totalBytes),
                    DetailText = BuildAppRowDetailText(app.ProcessName, app.BytesSent, app.BytesRecv, app.LastSeenTs),
                    SelectionKey = $"app:{app.ProcessName.ToLowerInvariant()}",
                });
            }

            if (systemMembers.Count > 0)
            {
                normalRows.Add(BuildAggregateGroupRow("system", "System", systemMembers));
            }

            if (otherMembers.Count > 0)
            {
                normalRows.Add(BuildAggregateGroupRow("other", "Other (< 1 MB each)", otherMembers));
            }

            IOrderedEnumerable<AppDisplayRow> ordered = sortKey switch
            {
                "name_asc" => normalRows.OrderBy(row => row.DisplayName, StringComparer.OrdinalIgnoreCase),
                "upload_desc" => normalRows.OrderByDescending(row => row.BytesSent)
                    .ThenByDescending(row => row.BytesRecv),
                "download_desc" => normalRows.OrderByDescending(row => row.BytesRecv)
                    .ThenByDescending(row => row.BytesSent),
                _ => normalRows.OrderByDescending(row => row.TotalBytes)
                    .ThenBy(row => row.DisplayName, StringComparer.OrdinalIgnoreCase),
            };

            return ordered.ToList();
        }

        private static AppDisplayRow BuildAggregateGroupRow(
            string groupKey,
            string displayName,
            IReadOnlyCollection<AppGroupMember> members)
        {
            var orderedMembers = members
                .OrderByDescending(member => member.BytesSent + member.BytesRecv)
                .ToArray();

            var totalSent = orderedMembers.Aggregate(0UL, (sum, member) => SaturatingAdd(sum, member.BytesSent));
            var totalRecv = orderedMembers.Aggregate(0UL, (sum, member) => SaturatingAdd(sum, member.BytesRecv));
            var latestSeenTs = orderedMembers.Length == 0 ? 0 : orderedMembers.Max(member => member.LastSeenTs);

            return new AppDisplayRow
            {
                DisplayName = displayName,
                ProcessName = string.Empty,
                BytesSent = totalSent,
                BytesRecv = totalRecv,
                LastSeenTs = latestSeenTs,
                UsageText = FormatBytes(totalSent + totalRecv),
                DetailText = $"{members.Count} grouped processes | Up {FormatBytes(totalSent)} | Down {FormatBytes(totalRecv)}",
                SelectionKey = $"group:{groupKey}",
                IsAggregateGroup = true,
                GroupMembers = orderedMembers,
            };
        }

        private static bool IsSystemGroupedApp(string processName, string displayName)
        {
            var normalized = processName.Trim().ToLowerInvariant();
            if (normalized is "" or "system" or "system idle process" or "idle" or "unattributed")
            {
                return true;
            }

            return normalized is "svchost.exe"
                or "services.exe"
                or "wininit.exe"
                or "lsass.exe"
                or "smss.exe"
                or "csrss.exe"
                || displayName.Equals("System", StringComparison.OrdinalIgnoreCase);
        }

        private static string BuildAppRowDetailText(string processName, ulong sent, ulong recv, long lastSeenTs)
        {
            var lastSeen = lastSeenTs > 0
                ? DateTimeOffset.FromUnixTimeSeconds(lastSeenTs).ToLocalTime().ToString("g")
                : "-";
            return $"{processName} | Up {FormatBytes(sent)} | Down {FormatBytes(recv)} | Last seen {lastSeen}";
        }

        private static string BuildInterfaceScopeLabel(string? interfaceId, string? interfaceType)
        {
            if (!string.IsNullOrWhiteSpace(interfaceId))
            {
                return $"Interface scope: {interfaceId}.";
            }

            if (!string.IsNullOrWhiteSpace(interfaceType))
            {
                return $"Interface scope: {interfaceType}.";
            }

            return "Interface scope: all interfaces.";
        }

        private static string FormatBucketLabel(long ts, string granularity)
        {
            var local = DateTimeOffset.FromUnixTimeSeconds(ts).ToLocalTime();
            return granularity switch
            {
                "hour" => local.ToString("yyyy-MM-dd HH:00"),
                "week" => local.ToString("yyyy-MM-dd") + " week",
                "month" => local.ToString("yyyy-MM") + " month",
                _ => local.ToString("yyyy-MM-dd"),
            };
        }

        private static ulong SaturatingAdd(ulong left, ulong right)
        {
            var maxDelta = ulong.MaxValue - left;
            return right > maxDelta ? ulong.MaxValue : left + right;
        }

        private (DateTimeOffset StartUtc, DateTimeOffset EndUtc) ResolveTopAppsRangeUtc()
        {
            var startDate = StartDatePicker.Date;
            var endDate = EndDatePicker.Date;

            var startLocal = new DateTimeOffset(startDate.Year, startDate.Month, startDate.Day, 0, 0, 0, startDate.Offset);
            var endLocalExclusive = new DateTimeOffset(endDate.Year, endDate.Month, endDate.Day, 0, 0, 0, endDate.Offset).AddDays(1);
            if (endLocalExclusive <= startLocal)
            {
                endLocalExclusive = startLocal.AddDays(1);
            }

            return (startLocal.ToUniversalTime(), endLocalExclusive.ToUniversalTime());
        }

        private string? ResolveSelectedInterfaceId()
        {
            if (InterfaceFilterComboBox.SelectedItem is InterfaceFilterOption option)
            {
                selectedInterfaceId = option.InterfaceId;
            }

            return selectedInterfaceId;
        }

        private string? ResolveSelectedInterfaceType()
        {
            if (InterfaceFilterComboBox.SelectedItem is InterfaceFilterOption option)
            {
                selectedInterfaceType = option.InterfaceType;
            }

            return selectedInterfaceType;
        }

        private static uint ResolveNumberBoxValue(NumberBox box, uint fallback, uint min, uint max)
        {
            var value = box.Value;
            if (double.IsNaN(value) || double.IsInfinity(value))
            {
                return Math.Clamp(fallback, min, max);
            }

            var rounded = (uint)Math.Round(value);
            var normalized = Math.Clamp(rounded, min, max);
            box.Value = normalized;
            return normalized;
        }

        private string ResolveSelectedTopAppsSortKey()
        {
            if (TopAppsSortComboBox.SelectedItem is AppSortOption option)
            {
                selectedTopAppsSort = option.Key;
            }

            return selectedTopAppsSort;
        }

        private string ResolveSelectedAppDetailGranularity()
        {
            if (AppDetailGranularityComboBox.SelectedItem is string granularity)
            {
                var normalized = granularity.Trim().ToLowerInvariant();
                if (normalized == "hour" || normalized == "day" || normalized == "week" || normalized == "month")
                {
                    return normalized;
                }
            }

            return "hour";
        }

        private string ResolveSelectedGranularity()
        {
            if (ExportGranularityComboBox.SelectedItem is string granularity)
            {
                var normalized = granularity.Trim().ToLowerInvariant();
                if (normalized == "hour" || normalized == "day" || normalized == "week" || normalized == "month")
                {
                    return normalized;
                }
            }

            return "day";
        }

        private static bool ShouldIncludeAdapterInFilter(Services.InterfaceInfo info)
        {
            if (info.Name.StartsWith("Attributed Usage", StringComparison.OrdinalIgnoreCase))
            {
                return false;
            }

            var kind = info.InterfaceType.ToLowerInvariant();
            if (kind != "wifi" && kind != "ethernet")
            {
                return false;
            }

            return !IsNoiseAdapterName(info.Name);
        }

        private static bool IsNoiseAdapterName(string name)
        {
            var lower = name.ToLowerInvariant();
            return lower.Contains("npcap")
                || lower.Contains("wfp ")
                || lower.Contains("qos packet scheduler")
                || lower.Contains("kernel debugger")
                || lower.Contains("native wifi filter")
                || lower.Contains("virtual wifi filter")
                || lower.Contains("hyper-v virtual switch extension")
                || lower.Contains("virtual filtering platform")
                || lower.StartsWith("local area connection*", StringComparison.Ordinal);
        }

        private static string BuildInterfaceDisplayName(Services.InterfaceInfo info)
        {
            var meter = info.IsMetered ? "metered" : "unmetered";
            return $"{info.Name} ({info.InterfaceType}, {meter})";
        }

        private static string BuildInterfaceUsageLabel(Services.InterfaceUsageRow row)
        {
            var meter = row.IsMetered ? "metered" : "unmetered";
            return $"{row.InterfaceName} ({row.InterfaceType}, {meter})";
        }

        private static int ToMondayOffset(DayOfWeek day)
        {
            return day switch
            {
                DayOfWeek.Monday => 0,
                DayOfWeek.Tuesday => 1,
                DayOfWeek.Wednesday => 2,
                DayOfWeek.Thursday => 3,
                DayOfWeek.Friday => 4,
                DayOfWeek.Saturday => 5,
                _ => 6,
            };
        }

        private static void ApplySummaryCard(TextBlock total, TextBlock split, Services.UsageSummary summary)
        {
            total.Text = FormatBytes(summary.TotalSent + summary.TotalRecv);
            split.Text = $"Up {FormatBytes(summary.TotalSent)} | Down {FormatBytes(summary.TotalRecv)}";
        }

        private static string FormatBytes(ulong bytes)
        {
            const double kb = 1024;
            const double mb = 1024 * kb;
            const double gb = 1024 * mb;

            if (bytes >= gb)
            {
                return $"{bytes / gb:F2} GB";
            }

            if (bytes >= mb)
            {
                return $"{bytes / mb:F2} MB";
            }

            if (bytes >= kb)
            {
                return $"{bytes / kb:F2} KB";
            }

            return $"{bytes} B";
        }

        private static string EscapeCsv(string value)
        {
            var escaped = value.Replace("\"", "\"\"");
            return '"' + escaped + '"';
        }

        public sealed class AppDisplayRow
        {
            public string DisplayName { get; init; } = string.Empty;

            public string ProcessName { get; init; } = string.Empty;

            public ulong BytesSent { get; init; }

            public ulong BytesRecv { get; init; }

            public ulong TotalBytes => BytesSent + BytesRecv;

            public long LastSeenTs { get; init; }

            public string UsageText { get; init; } = string.Empty;

            public string DetailText { get; init; } = string.Empty;

            public string SelectionKey { get; init; } = string.Empty;

            public bool IsPlaceholder { get; init; }

            public bool IsAggregateGroup { get; init; }

            public AppGroupMember[] GroupMembers { get; init; } = Array.Empty<AppGroupMember>();
        }

        public sealed class AppGroupMember
        {
            public string ProcessName { get; init; } = string.Empty;

            public string DisplayName { get; init; } = string.Empty;

            public ulong BytesSent { get; init; }

            public ulong BytesRecv { get; init; }

            public long LastSeenTs { get; init; }
        }

        public sealed class AppSortOption
        {
            public string Key { get; init; } = string.Empty;

            public string DisplayName { get; init; } = string.Empty;
        }

        public sealed class AppDetailBucketRow
        {
            public string BucketLabel { get; init; } = string.Empty;

            public string SplitText { get; init; } = string.Empty;

            public string UsageText { get; init; } = string.Empty;
        }

        public sealed class InterfaceFilterOption
        {
            public string DisplayName { get; init; } = string.Empty;

            public string? InterfaceId { get; init; }

            public string? InterfaceType { get; init; }
        }
    }
}
