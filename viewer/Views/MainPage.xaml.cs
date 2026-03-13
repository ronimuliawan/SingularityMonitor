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
        private const string DefaultExportGranularity = "day";
        private const bool DefaultExportIncludeSummary = true;
        private const bool DefaultExportIncludeApps = true;
        private const bool DefaultExportIncludeInterfaces = true;
        private const bool DefaultExportIncludeAfk = true;
        private const string OverviewModeCalendar = "calendar";
        private const string OverviewModeSelectedRange = "selected_range";
        private const string RangePresetToday = "today";
        private const string RangePresetLast7Days = "last_7_days";
        private const string RangePresetLast30Days = "last_30_days";
        private const string RangePresetCustom = "custom";
        private const string CapScopeGlobal = "global";
        private const string CapScopeInterface = "interface";
        private const ulong BytesPerGb = 1024UL * 1024UL * 1024UL;

        private string startupStatus = string.Empty;
        private string? selectedInterfaceId;
        private string? selectedInterfaceType;
        private string selectedTopAppsSort = "total_desc";
        private string selectedOverviewMode = OverviewModeCalendar;
        private string selectedRangePreset = RangePresetLast7Days;
        private string selectedCapScope = CapScopeGlobal;
        private long? selectedCapDefinitionId;
        private List<Services.InterfaceInfo> latestInterfaces = new();
        private List<Services.AfkWindowUsage> latestAfkWindows = new();
        private bool afkOnlyFilterEnabled;
        private bool pageReady;
        private bool onboardingCompleted;
        private bool onboardingStateLoaded;
        private bool isImportRunning;
        private bool suppressRangeControlEvents;

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
            InitializeRangePresetControls();
            InitializeOverviewControls();
            InitializeExportControls();
            InitializeTopAppsControls();
            InitializeAppDetailControls();
            InitializeSettingsControls();
            InitializeCapControls();
            UpdateOnboardingCard();
            await LoadInterfaceOptionsAsync();
            pageReady = true;

            var registration = helperManager.EnsureRunAtLogin();
            var loop = helperManager.EnsureLoopRunning();
            startupStatus = registration + " " + loop;
            HelperStatusText.Text = startupStatus;

            await LoadSettingsAsync();
            await RefreshStatusAsync();
            await RefreshOverviewAsync();
            await RefreshAfkTimelineAsync();
            await RefreshAppsAsync();
            await RefreshSelectedAppDetailAsync();
            await RefreshInterfaceBreakdownAsync();
            await RefreshCapsAsync();
            await RefreshAlertsHistoryAsync();
        }

        private void InitializeExportControls()
        {
            ExportGranularityComboBox.ItemsSource = new[] { "hour", "day", "week", "month" };
            ExportGranularityComboBox.SelectedItem = DefaultExportGranularity;

            var appScopes = new List<ExportAppScopeOption>
            {
                new() { Key = "all", DisplayName = "All apps" },
                new() { Key = "selected", DisplayName = "Selected app" },
            };

            ExportAppScopeComboBox.DisplayMemberPath = nameof(ExportAppScopeOption.DisplayName);
            ExportAppScopeComboBox.ItemsSource = appScopes;
            ExportAppScopeComboBox.SelectedItem = appScopes[0];

            IncludeSummaryCheckBox.IsChecked = DefaultExportIncludeSummary;
            IncludeAppsCheckBox.IsChecked = DefaultExportIncludeApps;
            IncludeInterfacesCheckBox.IsChecked = DefaultExportIncludeInterfaces;
            IncludeAfkCheckBox.IsChecked = DefaultExportIncludeAfk;
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

        private void InitializeOverviewControls()
        {
            var options = new List<OverviewModeOption>
            {
                new() { Key = OverviewModeCalendar, DisplayName = "Calendar (Today / Week / Month)" },
                new() { Key = OverviewModeSelectedRange, DisplayName = "Selected Range" },
            };

            OverviewModeComboBox.DisplayMemberPath = nameof(OverviewModeOption.DisplayName);
            OverviewModeComboBox.ItemsSource = options;
            OverviewModeComboBox.SelectedItem = options[0];
            selectedOverviewMode = options[0].Key;
        }

        private void InitializeRangePresetControls()
        {
            var options = new List<RangePresetOption>
            {
                new() { Key = RangePresetToday, DisplayName = "Today" },
                new() { Key = RangePresetLast7Days, DisplayName = "Last 7 Days" },
                new() { Key = RangePresetLast30Days, DisplayName = "Last 30 Days" },
                new() { Key = RangePresetCustom, DisplayName = "Custom" },
            };

            RangePresetComboBox.DisplayMemberPath = nameof(RangePresetOption.DisplayName);
            RangePresetComboBox.ItemsSource = options;
            SetRangePresetSelection(RangePresetLast7Days);
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
            SettingsExportGranularityComboBox.ItemsSource = new[] { "hour", "day", "week", "month" };
            SettingsExportGranularityComboBox.SelectedItem = DefaultExportGranularity;
            SettingsIncludeSummaryCheckBox.IsChecked = DefaultExportIncludeSummary;
            SettingsIncludeAppsCheckBox.IsChecked = DefaultExportIncludeApps;
            SettingsIncludeInterfacesCheckBox.IsChecked = DefaultExportIncludeInterfaces;
        }

        private void InitializeCapControls()
        {
            var scopes = new List<CapScopeOption>
            {
                new() { Key = CapScopeGlobal, DisplayName = "Global" },
                new() { Key = CapScopeInterface, DisplayName = "Per interface" },
            };

            CapScopeComboBox.DisplayMemberPath = nameof(CapScopeOption.DisplayName);
            CapScopeComboBox.ItemsSource = scopes;
            CapScopeComboBox.SelectedItem = scopes[0];
            selectedCapScope = scopes[0].Key;
            CapMonthlyGbNumberBox.Value = 100;
            CapActiveToggle.IsOn = true;
            DeleteCapButton.IsEnabled = false;
            UpdateCapInterfaceControls();
        }

        private void InitializeDateRangePickers()
        {
            ApplyRangePresetToDatePickers(RangePresetLast7Days);
        }

        private async Task LoadInterfaceOptionsAsync()
        {
            try
            {
                var response = await daemonClient.GetInterfacesAsync();
                latestInterfaces = response.Interfaces
                    .OrderBy(i => i.Name, StringComparer.OrdinalIgnoreCase)
                    .ToList();
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
                UpdateCapInterfaceOptions();
            }
            catch (Exception ex)
            {
                latestInterfaces = new List<Services.InterfaceInfo>();
                InterfaceFilterComboBox.ItemsSource = new List<InterfaceFilterOption>
                {
                    new() { DisplayName = "All Interfaces", InterfaceId = null, InterfaceType = null }
                };
                InterfaceFilterComboBox.SelectedIndex = 0;
                selectedInterfaceId = null;
                selectedInterfaceType = null;
                UpdateCapInterfaceOptions();
                HelperStatusText.Text = $"Interface list unavailable: {ex.Message}. {startupStatus}";
            }
        }

        private async void OnRefreshClicked(object sender, RoutedEventArgs e)
        {
            await LoadInterfaceOptionsAsync();
            await RefreshStatusAsync();
            await LoadSettingsAsync();
            await RefreshOverviewAsync();
            await RefreshAfkTimelineAsync();
            await RefreshAppsAsync();
            await RefreshSelectedAppDetailAsync();
            await RefreshInterfaceBreakdownAsync();
            await RefreshCapsAsync();
            await RefreshAlertsHistoryAsync();
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
            SettingsExportGranularityComboBox.SelectedItem = DefaultExportGranularity;
            SettingsIncludeSummaryCheckBox.IsChecked = DefaultExportIncludeSummary;
            SettingsIncludeAppsCheckBox.IsChecked = DefaultExportIncludeApps;
            SettingsIncludeInterfacesCheckBox.IsChecked = DefaultExportIncludeInterfaces;
            try
            {
                await ApplySettingsFromInputsAsync();
            }
            catch (Exception ex)
            {
                SettingsStatusText.Text = $"Failed to reset settings: {ex.Message}";
            }
        }

        private async void OnCompactDatabaseClicked(object sender, RoutedEventArgs e)
        {
            var triggerButton = sender as Button;
            if (triggerButton is not null)
            {
                triggerButton.IsEnabled = false;
            }

            try
            {
                var result = await daemonClient.CompactDatabaseAsync();
                SettingsStatusText.Text =
                    $"Compacted DB in {result.DurationMs}ms. Reclaimed {FormatBytes(result.ReclaimedBytes)} " +
                    $"({FormatBytes(result.BeforeBytes)} -> {FormatBytes(result.AfterBytes)}).";
                await RefreshStatusAsync();
            }
            catch (Exception ex)
            {
                SettingsStatusText.Text = $"Failed to compact database: {ex.Message}";
            }
            finally
            {
                if (triggerButton is not null)
                {
                    triggerButton.IsEnabled = true;
                }
            }
        }

        private async Task ApplySettingsFromInputsAsync()
        {
            var poll = ResolveNumberBoxValue(PollIntervalNumberBox, DefaultPollIntervalSeconds, 15, 300);
            var retention = ResolveNumberBoxValue(RetentionDaysNumberBox, DefaultRetentionDays, 0, 3650);
            var afk = ResolveNumberBoxValue(AfkIdleThresholdNumberBox, DefaultAfkIdleThresholdSeconds, 30, 3600);
            var exportDefaultGranularity = ResolveSettingsExportGranularity();
            var exportDefaultIncludeSummary = SettingsIncludeSummaryCheckBox.IsChecked == true;
            var exportDefaultIncludeApps = SettingsIncludeAppsCheckBox.IsChecked == true;
            var exportDefaultIncludeInterfaces = SettingsIncludeInterfacesCheckBox.IsChecked == true;

            await daemonClient.ApplySettingsAsync(
                pollIntervalSeconds: poll,
                retentionDays: retention,
                afkIdleThresholdSeconds: afk,
                exportDefaultGranularity: exportDefaultGranularity,
                exportDefaultIncludeSummary: exportDefaultIncludeSummary,
                exportDefaultIncludeApps: exportDefaultIncludeApps,
                exportDefaultIncludeInterfaces: exportDefaultIncludeInterfaces);

            SettingsStatusText.Text =
                $"Saved at {DateTimeOffset.Now:HH:mm:ss}. Poll={poll}s, Retention={retention}d, AFK={afk}s, Export={exportDefaultGranularity}.";
            await RefreshStatusAsync();
            await LoadSettingsAsync();
        }

        private async void OnSummaryClicked(object sender, RoutedEventArgs e)
        {
            try
            {
                var interfaceId = ResolveSelectedInterfaceId();
                var interfaceType = ResolveSelectedInterfaceType();
                var mode = ResolveSelectedOverviewModeKey();
                var (startUtc, endUtc, label) = ResolveSummaryActionRangeUtc(mode);
                var summary = await daemonClient.GetUsageSummaryAsync(
                    startUtc.ToUnixTimeSeconds(),
                    endUtc.ToUnixTimeSeconds(),
                    granularity: "day",
                    interfaceId: interfaceId,
                    interfaceType: interfaceType);
                StatusText.Text =
                    $"{label} usage: {FormatBytes(summary.TotalSent + summary.TotalRecv)} total " +
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
            await RunHistoryImportAsync(sender as Button, markOnboardingCompleteOnSuccess: false);
        }

        private async void OnOnboardingImportClicked(object sender, RoutedEventArgs e)
        {
            await RunHistoryImportAsync(sender as Button, markOnboardingCompleteOnSuccess: true);
        }

        private async void OnOnboardingSkipClicked(object sender, RoutedEventArgs e)
        {
            await MarkOnboardingCompletedAsync("Onboarding dismissed. You can still run Import 60 Days anytime.");
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

        private async void OnRefreshAfkTimelineClicked(object sender, RoutedEventArgs e)
        {
            await RefreshAfkTimelineAsync();
            if (afkOnlyFilterEnabled)
            {
                await RefreshAppsAsync();
                await RefreshSelectedAppDetailAsync();
            }
        }

        private void OnAfkTimelineSelectionChanged(object sender, SelectionChangedEventArgs e)
        {
            RefreshSelectedAfkWindowApps();
        }

        private async void OnAfkOnlyFilterChanged(object sender, RoutedEventArgs e)
        {
            if (!pageReady)
            {
                return;
            }

            afkOnlyFilterEnabled = AfkOnlyTopAppsCheckBox.IsChecked == true;
            await RefreshAppsAsync();
            await RefreshSelectedAppDetailAsync();
        }

        private async void OnRefreshAlertsHistoryClicked(object sender, RoutedEventArgs e)
        {
            await RefreshAlertsHistoryAsync();
        }

        private void OnCapScopeChanged(object sender, SelectionChangedEventArgs e)
        {
            selectedCapScope = ResolveSelectedCapScopeKey();
            UpdateCapInterfaceControls();
        }

        private async void OnSaveCapClicked(object sender, RoutedEventArgs e)
        {
            var triggerButton = sender as Button;
            if (triggerButton is not null)
            {
                triggerButton.IsEnabled = false;
            }

            try
            {
                var scope = ResolveSelectedCapScopeKey();
                var interfaceGuid = scope == CapScopeInterface
                    ? ResolveSelectedCapInterfaceGuid()
                    : null;
                if (scope == CapScopeInterface && string.IsNullOrWhiteSpace(interfaceGuid))
                {
                    CapStatusText.Text = "Select an interface for per-interface caps.";
                    return;
                }

                var capGb = ResolveCapGigabytes();
                var capBytes = capGb * BytesPerGb;
                var upsert = await daemonClient.UpsertCapDefinitionAsync(
                    scope,
                    capBytes,
                    CapActiveToggle.IsOn,
                    interfaceGuid,
                    selectedCapDefinitionId);

                selectedCapDefinitionId = upsert.Cap.Id;
                CapStatusText.Text = $"Cap saved: {BuildCapScopeText(upsert.Cap)} at {DateTimeOffset.Now:HH:mm:ss}.";
                await RefreshCapsAsync();
                await RefreshAlertsHistoryAsync();
            }
            catch (Exception ex)
            {
                CapStatusText.Text = $"Failed to save cap: {ex.Message}";
            }
            finally
            {
                if (triggerButton is not null)
                {
                    triggerButton.IsEnabled = true;
                }
            }
        }

        private async void OnDeleteCapClicked(object sender, RoutedEventArgs e)
        {
            var selected = CapDefinitionsList.SelectedItem as CapDefinitionRow;
            if (selected is null)
            {
                CapStatusText.Text = "Select a cap row to delete.";
                return;
            }

            try
            {
                var result = await daemonClient.DeleteCapDefinitionAsync(selected.Id);
                if (!result.Deleted)
                {
                    CapStatusText.Text = "Cap was not found. List has been refreshed.";
                }
                else
                {
                    CapStatusText.Text = $"Deleted cap {selected.Id}.";
                }

                selectedCapDefinitionId = null;
                await RefreshCapsAsync();
                await RefreshAlertsHistoryAsync();
            }
            catch (Exception ex)
            {
                CapStatusText.Text = $"Failed to delete cap: {ex.Message}";
            }
        }

        private void OnCapSelectionChanged(object sender, SelectionChangedEventArgs e)
        {
            if (CapDefinitionsList.SelectedItem is not CapDefinitionRow selected)
            {
                selectedCapDefinitionId = null;
                DeleteCapButton.IsEnabled = false;
                return;
            }

            selectedCapDefinitionId = selected.Id;
            selectedCapScope = selected.ScopeKey;
            SetCapScopeSelection(selected.ScopeKey);
            UpdateCapInterfaceControls();
            SetCapInterfaceSelection(selected.InterfaceGuid);
            CapMonthlyGbNumberBox.Value = selected.MonthlyCapGb;
            CapActiveToggle.IsOn = selected.IsActive;
            DeleteCapButton.IsEnabled = true;
            CapStatusText.Text = $"Editing cap {selected.Id}. Save to update or delete to remove.";
        }

        private async void OnApplyRangeClicked(object sender, RoutedEventArgs e)
        {
            await RefreshOverviewAsync();
            await RefreshAfkTimelineAsync();
            await RefreshAppsAsync();
            await RefreshSelectedAppDetailAsync();
            await RefreshInterfaceBreakdownAsync();
            await RefreshAlertsHistoryAsync();
        }

        private async void OnOverviewModeChanged(object sender, SelectionChangedEventArgs e)
        {
            if (!pageReady)
            {
                return;
            }

            selectedOverviewMode = ResolveSelectedOverviewModeKey();
            await RefreshOverviewAsync();
        }

        private async void OnRangePresetChanged(object sender, SelectionChangedEventArgs e)
        {
            if (suppressRangeControlEvents)
            {
                return;
            }

            var preset = ResolveSelectedRangePresetKey();
            selectedRangePreset = preset;

            if (preset == RangePresetCustom)
            {
                return;
            }

            ApplyRangePresetToDatePickers(preset);
            if (!pageReady)
            {
                return;
            }

            await RefreshOverviewAsync();
            await RefreshAfkTimelineAsync();
            await RefreshAppsAsync();
            await RefreshSelectedAppDetailAsync();
            await RefreshInterfaceBreakdownAsync();
            await RefreshAlertsHistoryAsync();
        }

        private void OnDateRangeChanged(object sender, DatePickerValueChangedEventArgs args)
        {
            if (suppressRangeControlEvents)
            {
                return;
            }

            SetRangePresetSelection(RangePresetCustom);
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
                var includeAfk = IncludeAfkCheckBox.IsChecked == true;
                var appScope = ResolveSelectedExportAppScope();
                var selectedProcess = appScope == "selected" ? ResolveSelectedConcreteTopAppProcess() : null;

                if (!includeSummary && !includeApps && !includeInterfaces && !includeAfk)
                {
                    HelperStatusText.Text = "Select at least one export section (summary/apps/interfaces/afk).";
                    return;
                }

                if (appScope == "selected" && string.IsNullOrWhiteSpace(selectedProcess))
                {
                    HelperStatusText.Text =
                        "Export scope is set to Selected app, but no concrete Top Apps process is selected.";
                    return;
                }

                var (startUtc, endUtc) = ResolveTopAppsRangeUtc();
                var startTs = startUtc.ToUnixTimeSeconds();
                var endTs = endUtc.ToUnixTimeSeconds();
                Services.UsageSummary? summary = null;
                Services.AppBreakdown? appBreakdown = null;
                Services.InterfaceBreakdownResponse? interfaceBreakdown = null;
                Services.AfkWindowUsage[] afkWindows = Array.Empty<Services.AfkWindowUsage>();

                if (includeSummary)
                {
                    summary = await daemonClient.GetUsageSummaryAsync(
                        startTs,
                        endTs,
                        granularity: granularity,
                        interfaceId: interfaceId,
                        interfaceType: interfaceType,
                        appFilter: selectedProcess);
                }

                if (includeApps)
                {
                    appBreakdown = await daemonClient.GetAppBreakdownAsync(
                        startTs,
                        endTs,
                        limit: 500,
                        interfaceId: interfaceId,
                        interfaceType: interfaceType);

                    if (!string.IsNullOrWhiteSpace(selectedProcess))
                    {
                        appBreakdown.Apps = appBreakdown.Apps
                            .Where(app => app.ProcessName.Equals(selectedProcess, StringComparison.OrdinalIgnoreCase))
                            .ToArray();
                    }
                }

                if (includeInterfaces)
                {
                    interfaceBreakdown = await daemonClient.GetInterfaceBreakdownAsync(
                        startTs,
                        endTs,
                        interfaceId: interfaceId,
                        interfaceType: interfaceType);
                }

                if (includeAfk)
                {
                    var afkAudit = await daemonClient.GetAfkAuditAsync(
                        startTs: startTs,
                        endTs: endTs,
                        limit: 1000);
                    afkWindows = BuildExportAfkWindows(afkAudit.AfkWindows, startTs, endTs, selectedProcess);
                }

                var downloads = Path.Combine(Environment.GetFolderPath(Environment.SpecialFolder.UserProfile), "Downloads");
                Directory.CreateDirectory(downloads);
                var stamp = DateTimeOffset.Now.ToString("yyyyMMdd_HHmmss");

                if (asJson)
                {
                    var path = CreateUniqueExportPath(downloads, $"singularity_export_{stamp}", ".json");
                    var payload = new
                    {
                        generated_at_utc = DateTimeOffset.UtcNow,
                        start_ts = startTs,
                        end_ts = endTs,
                        granularity,
                        interface_id = interfaceId,
                        interface_type = interfaceType,
                        apps_scope = interfaceId is not null || interfaceType is not null ? "filtered" : "global",
                        app_filter_scope = appScope,
                        app_filter_process = selectedProcess,
                        summary,
                        apps = appBreakdown?.Apps,
                        interfaces = interfaceBreakdown?.Interfaces,
                        afk_windows = afkWindows,
                    };
                    var json = JsonSerializer.Serialize(payload, new JsonSerializerOptions { WriteIndented = true });
                    await File.WriteAllTextAsync(path, json);
                    HelperStatusText.Text = $"Exported JSON to {path}";
                }
                else
                {
                    var path = CreateUniqueExportPath(downloads, $"singularity_export_{stamp}", ".csv");
                    var csv = BuildCsvExport(
                        startTs,
                        endTs,
                        interfaceId,
                        interfaceType,
                        granularity,
                        appScope,
                        selectedProcess,
                        summary,
                        appBreakdown,
                        interfaceBreakdown,
                        afkWindows);
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
            string appScope,
            string? appFilterProcess,
            Services.UsageSummary? summary,
            Services.AppBreakdown? appBreakdown,
            Services.InterfaceBreakdownResponse? interfaceBreakdown,
            IReadOnlyCollection<Services.AfkWindowUsage>? afkWindows)
        {
            var sb = new StringBuilder();
            sb.AppendLine("meta_key,meta_value");
            sb.AppendLine($"start_ts,{startTs}");
            sb.AppendLine($"end_ts,{endTs}");
            sb.AppendLine($"granularity,{EscapeCsv(granularity)}");
            sb.AppendLine($"interface_id,{EscapeCsv(interfaceId ?? "all")}");
            sb.AppendLine($"interface_type,{EscapeCsv(interfaceType ?? "all")}");
            sb.AppendLine($"app_filter_scope,{EscapeCsv(appScope)}");
            sb.AppendLine($"app_filter_process,{EscapeCsv(appFilterProcess ?? "all")}");
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

            if (afkWindows is not null && afkWindows.Count > 0)
            {
                sb.AppendLine();
                sb.AppendLine("section,window_start_ts,window_end_ts,duration_seconds,bytes_sent,bytes_recv,total_bytes,process_name,display_name,last_seen_ts");
                foreach (var window in afkWindows)
                {
                    var windowTotal = window.BytesSent + window.BytesRecv;
                    sb.AppendLine(string.Join(",",
                        "afk_window",
                        window.StartTs,
                        window.EndTs,
                        window.DurationSeconds,
                        window.BytesSent,
                        window.BytesRecv,
                        windowTotal,
                        "",
                        "",
                        ""));

                    foreach (var app in window.TopApps)
                    {
                        var appTotal = app.BytesSent + app.BytesRecv;
                        sb.AppendLine(string.Join(",",
                            "afk_top_app",
                            window.StartTs,
                            window.EndTs,
                            window.DurationSeconds,
                            app.BytesSent,
                            app.BytesRecv,
                            appTotal,
                            EscapeCsv(app.ProcessName),
                            EscapeCsv(app.DisplayName),
                            app.LastSeenTs));
                    }
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
                SettingsExportGranularityComboBox.SelectedItem = NormalizeGranularity(settings.ExportDefaultGranularity);
                SettingsIncludeSummaryCheckBox.IsChecked = settings.ExportDefaultIncludeSummary;
                SettingsIncludeAppsCheckBox.IsChecked = settings.ExportDefaultIncludeApps;
                SettingsIncludeInterfacesCheckBox.IsChecked = settings.ExportDefaultIncludeInterfaces;
                ApplyExportDefaultsToControls(
                    settings.ExportDefaultGranularity,
                    settings.ExportDefaultIncludeSummary,
                    settings.ExportDefaultIncludeApps,
                    settings.ExportDefaultIncludeInterfaces);
                onboardingCompleted = settings.OnboardingCompleted;
                onboardingStateLoaded = true;
                UpdateOnboardingCard();
                if (string.IsNullOrWhiteSpace(SettingsStatusText.Text))
                {
                    SettingsStatusText.Text = "Settings loaded from daemon.";
                }
            }
            catch (Exception ex)
            {
                onboardingStateLoaded = false;
                UpdateOnboardingCard();
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
                DbSizeStatusText.Text = $"Database size: {FormatBytes(status.DbSizeBytes)}.";

                isImportRunning = status.ImportStatus.Equals("running", StringComparison.OrdinalIgnoreCase);
                UpdateOnboardingCard();
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

                ReliabilityStatusText.Text = BuildReliabilityStatusText(status);
                RetentionCleanupStatusText.Text = BuildRetentionCleanupStatusText(status);
            }
            catch (Exception ex)
            {
                isImportRunning = false;
                UpdateOnboardingCard();
                StatusText.Text = "Daemon unavailable. Start the collector service or run daemon --console.";
                HelperStatusText.Text = "Helper status unavailable while daemon is offline.";
                MemoryHintText.Text = $"Status check failed: {ex.Message}";
                DbSizeStatusText.Text = "Database size unavailable.";
                ReliabilityStatusText.Text = "Reliability metrics unavailable.";
                RetentionCleanupStatusText.Text = "Retention cleanup status unavailable.";
            }
        }

        private async Task RunHistoryImportAsync(Button? triggerButton, bool markOnboardingCompleteOnSuccess)
        {
            SetImportControlsEnabled(false, triggerButton);
            try
            {
                HelperStatusText.Text = "Running 60-day import in helper process...";
                if (markOnboardingCompleteOnSuccess)
                {
                    OnboardingStatusText.Text = "Starting initial import...";
                }

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
                await RefreshAfkTimelineAsync();
                await RefreshAppsAsync();
                await RefreshSelectedAppDetailAsync();
                await RefreshInterfaceBreakdownAsync();
                await RefreshAlertsHistoryAsync();

                if (markOnboardingCompleteOnSuccess)
                {
                    var importSucceeded = output.Contains("import complete", StringComparison.OrdinalIgnoreCase)
                        || output.Contains("History import finished", StringComparison.OrdinalIgnoreCase);

                    if (importSucceeded)
                    {
                        await MarkOnboardingCompletedAsync("Initial import finished. Onboarding is complete.");
                    }
                    else
                    {
                        OnboardingStatusText.Text = "Initial import did not report success. You can retry or skip for now.";
                    }
                }
            }
            finally
            {
                SetImportControlsEnabled(true, triggerButton);
                UpdateOnboardingCard();
            }
        }

        private async Task MarkOnboardingCompletedAsync(string statusText)
        {
            try
            {
                await daemonClient.ApplySettingsAsync(onboardingCompleted: true);
                onboardingCompleted = true;
                onboardingStateLoaded = true;
                UpdateOnboardingCard();
                OnboardingStatusText.Text = statusText;
            }
            catch (Exception ex)
            {
                OnboardingStatusText.Text = $"Unable to update onboarding flag: {ex.Message}";
            }
        }

        private void SetImportControlsEnabled(bool enabled, Button? triggerButton)
        {
            if (triggerButton is not null)
            {
                triggerButton.IsEnabled = enabled;
            }

            if (OnboardingImportButton is not null)
            {
                OnboardingImportButton.IsEnabled = enabled;
            }

            if (OnboardingSkipButton is not null)
            {
                OnboardingSkipButton.IsEnabled = enabled;
            }
        }

        private void UpdateOnboardingCard()
        {
            if (OnboardingCard is null)
            {
                return;
            }

            var shouldShow = onboardingStateLoaded && !onboardingCompleted;
            OnboardingCard.Visibility = shouldShow ? Visibility.Visible : Visibility.Collapsed;
            if (!shouldShow)
            {
                return;
            }

            var busy = isImportRunning;
            if (OnboardingImportButton is not null)
            {
                OnboardingImportButton.IsEnabled = !busy;
            }

            if (OnboardingSkipButton is not null)
            {
                OnboardingSkipButton.IsEnabled = !busy;
            }

            if (busy)
            {
                OnboardingStatusText.Text = "Initial import is running...";
            }
            else if (string.IsNullOrWhiteSpace(OnboardingStatusText.Text))
            {
                OnboardingStatusText.Text = "Run import now, or skip and continue with live-only data.";
            }
        }

        private async Task RefreshOverviewAsync()
        {
            try
            {
                var interfaceId = ResolveSelectedInterfaceId();
                var interfaceType = ResolveSelectedInterfaceType();
                var mode = ResolveSelectedOverviewModeKey();
                var localNow = DateTimeOffset.Now;
                var nowUtc = DateTimeOffset.UtcNow;

                Services.UsageSummary first;
                Services.UsageSummary second;
                Services.UsageSummary third;

                if (mode == OverviewModeSelectedRange)
                {
                    var (rangeStartUtc, rangeEndUtc) = ResolveTopAppsRangeUtc();
                    var startLocal = rangeStartUtc.ToLocalTime();
                    var endLocalInclusive = rangeEndUtc.AddSeconds(-1).ToLocalTime();
                    var totalDays = Math.Max(1, (int)(endLocalInclusive.Date - startLocal.Date).TotalDays + 1);

                    var firstDayEndUtc = rangeStartUtc.AddDays(1);
                    if (firstDayEndUtc > rangeEndUtc)
                    {
                        firstDayEndUtc = rangeEndUtc;
                    }

                    var lastDayStartUtc = rangeEndUtc.AddDays(-1);
                    if (lastDayStartUtc < rangeStartUtc)
                    {
                        lastDayStartUtc = rangeStartUtc;
                    }

                    first = await daemonClient.GetUsageSummaryAsync(
                        rangeStartUtc.ToUnixTimeSeconds(),
                        rangeEndUtc.ToUnixTimeSeconds(),
                        granularity: "day",
                        interfaceId: interfaceId,
                        interfaceType: interfaceType);
                    second = await daemonClient.GetUsageSummaryAsync(
                        rangeStartUtc.ToUnixTimeSeconds(),
                        firstDayEndUtc.ToUnixTimeSeconds(),
                        granularity: "day",
                        interfaceId: interfaceId,
                        interfaceType: interfaceType);
                    third = await daemonClient.GetUsageSummaryAsync(
                        lastDayStartUtc.ToUnixTimeSeconds(),
                        rangeEndUtc.ToUnixTimeSeconds(),
                        granularity: "day",
                        interfaceId: interfaceId,
                        interfaceType: interfaceType);

                    TodayCardTitleText.Text = $"Selected ({totalDays}d)";
                    WeekCardTitleText.Text = "Range Start Day";
                    MonthCardTitleText.Text = "Range End Day";
                }
                else
                {
                    var dayStartLocal = new DateTimeOffset(localNow.Year, localNow.Month, localNow.Day, 0, 0, 0, localNow.Offset);
                    var weekStartLocal = dayStartLocal.AddDays(-ToMondayOffset(localNow.DayOfWeek));
                    var monthStartLocal = new DateTimeOffset(localNow.Year, localNow.Month, 1, 0, 0, 0, localNow.Offset);

                    first = await daemonClient.GetUsageSummaryAsync(
                        dayStartLocal.ToUnixTimeSeconds(),
                        nowUtc.ToUnixTimeSeconds(),
                        granularity: "day",
                        interfaceId: interfaceId,
                        interfaceType: interfaceType);
                    second = await daemonClient.GetUsageSummaryAsync(
                        weekStartLocal.ToUnixTimeSeconds(),
                        nowUtc.ToUnixTimeSeconds(),
                        granularity: "day",
                        interfaceId: interfaceId,
                        interfaceType: interfaceType);
                    third = await daemonClient.GetUsageSummaryAsync(
                        monthStartLocal.ToUnixTimeSeconds(),
                        nowUtc.ToUnixTimeSeconds(),
                        granularity: "day",
                        interfaceId: interfaceId,
                        interfaceType: interfaceType);

                    TodayCardTitleText.Text = "Today";
                    WeekCardTitleText.Text = "This Week";
                    MonthCardTitleText.Text = "This Month";
                }

                ApplySummaryCard(TodayTotalText, TodaySplitText, TodayUploadShareBar, TodayUploadShareText, first);
                ApplySummaryCard(WeekTotalText, WeekSplitText, WeekUploadShareBar, WeekUploadShareText, second);
                ApplySummaryCard(MonthTotalText, MonthSplitText, MonthUploadShareBar, MonthUploadShareText, third);
            }
            catch
            {
                TodayCardTitleText.Text = "Today";
                TodayTotalText.Text = "-";
                TodaySplitText.Text = "-";
                TodayUploadShareBar.Value = 0;
                TodayUploadShareText.Text = "Upload share -";
                WeekCardTitleText.Text = "This Week";
                WeekTotalText.Text = "-";
                WeekSplitText.Text = "-";
                WeekUploadShareBar.Value = 0;
                WeekUploadShareText.Text = "Upload share -";
                MonthCardTitleText.Text = "This Month";
                MonthTotalText.Text = "-";
                MonthSplitText.Text = "-";
                MonthUploadShareBar.Value = 0;
                MonthUploadShareText.Text = "Upload share -";
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
                    interfaceType: interfaceType,
                    sortBy: MapTopAppsSortToDaemon(sortKey));

                afkOnlyFilterEnabled = AfkOnlyTopAppsCheckBox.IsChecked == true;
                var filteredApps = breakdown.Apps.AsEnumerable();
                if (afkOnlyFilterEnabled)
                {
                    var afkProcesses = BuildAfkProcessAllowSet();
                    filteredApps = filteredApps.Where(app =>
                    {
                        var process = app.ProcessName?.Trim();
                        return !string.IsNullOrWhiteSpace(process) && afkProcesses.Contains(process);
                    });
                }

                var rows = BuildTopAppRows(filteredApps, sortKey);

                if (rows.Count == 0)
                {
                    rows.Add(new AppDisplayRow
                    {
                        DisplayName = afkOnlyFilterEnabled
                            ? "No AFK-attributed app activity in this range yet"
                            : "No helper-attributed data in this range yet",
                        UsageText = "-",
                        ProcessText = "",
                        LastSeenText = "",
                        TransferSplitText = "",
                        IsPlaceholder = true,
                        SelectionKey = "placeholder:none",
                        IconGlyph = "\uE711",
                        IconForeground = "#9AAEBE",
                    });
                }

                TopAppsList.ItemsSource = rows;

                var selected = ResolveTopAppSelection(rows, previouslySelected);
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
                        ProcessText = "",
                        LastSeenText = "",
                        TransferSplitText = "",
                        IsPlaceholder = true,
                        SelectionKey = "placeholder:error",
                        IconGlyph = "\uEA39",
                        IconForeground = "#F2C078",
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

                var chartRows = BuildInterfaceChartRows(response.Interfaces);

                if (rows.Count == 0)
                {
                    rows.Add(new AppDisplayRow
                    {
                        DisplayName = "No interface-poll data in this range yet",
                        UsageText = "-",
                    });
                }

                InterfaceBreakdownList.ItemsSource = rows;
                InterfaceBreakdownChartList.ItemsSource = chartRows;
                InterfaceBreakdownChartStatusText.Text = "No interface chart data in this range yet.";
                InterfaceBreakdownChartStatusText.Visibility = chartRows.Count == 0 ? Visibility.Visible : Visibility.Collapsed;
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
                InterfaceBreakdownChartList.ItemsSource = new List<InterfaceChartRow>();
                InterfaceBreakdownChartStatusText.Text = $"Failed to load interface chart: {ex.Message}";
                InterfaceBreakdownChartStatusText.Visibility = Visibility.Visible;
            }
        }

        private async Task RefreshCapsAsync()
        {
            try
            {
                var response = await daemonClient.ListCapDefinitionsAsync();
                var rows = response.Caps
                    .Select(cap => new CapDefinitionRow
                    {
                        Id = cap.Id,
                        ScopeKey = NormalizeCapScope(cap.Scope),
                        InterfaceGuid = cap.InterfaceGuid,
                        MonthlyCapBytes = cap.MonthlyCapBytes,
                        MonthlyCapGb = Math.Max(1, cap.MonthlyCapBytes / BytesPerGb),
                        IsActive = cap.IsActive,
                        ScopeText = BuildCapScopeText(cap),
                        CapText = FormatBytes(cap.MonthlyCapBytes),
                        ActiveText = cap.IsActive ? "active" : "inactive",
                    })
                    .ToList();

                CapDefinitionsList.ItemsSource = rows;
                if (rows.Count == 0)
                {
                    selectedCapDefinitionId = null;
                    DeleteCapButton.IsEnabled = false;
                    if (CapDefinitionsList.SelectedItem is not null)
                    {
                        CapDefinitionsList.SelectedItem = null;
                    }

                    CapStatusText.Text = "No monthly caps defined yet.";
                    return;
                }

                var selected = rows.FirstOrDefault(row => row.Id == selectedCapDefinitionId);
                if (selected is null)
                {
                    CapDefinitionsList.SelectedItem = null;
                    DeleteCapButton.IsEnabled = false;
                }
                else
                {
                    CapDefinitionsList.SelectedItem = selected;
                }
            }
            catch (Exception ex)
            {
                CapDefinitionsList.ItemsSource = new List<CapDefinitionRow>();
                selectedCapDefinitionId = null;
                DeleteCapButton.IsEnabled = false;
                CapStatusText.Text = $"Cap definitions unavailable: {ex.Message}";
            }
        }

        private async Task RefreshAfkTimelineAsync()
        {
            try
            {
                var (startUtc, endUtc) = ResolveTopAppsRangeUtc();
                var startTs = startUtc.ToUnixTimeSeconds();
                var endTs = endUtc.ToUnixTimeSeconds();
                AfkTimelineRangeText.Text =
                    $"({startUtc.LocalDateTime:yyyy-MM-dd} to {endUtc.LocalDateTime.AddSeconds(-1):yyyy-MM-dd})";

                var response = await daemonClient.GetAfkAuditAsync(
                    startTs: startTs,
                    endTs: endTs,
                    limit: 1000);
                latestAfkWindows = response.AfkWindows
                    .Where(window => window.EndTs >= startTs && window.StartTs < endTs)
                    .OrderByDescending(window => window.StartTs)
                    .ToList();

                var previousSelection = AfkTimelineList.SelectedItem as AfkTimelineRow;
                var rows = latestAfkWindows
                    .Select(BuildAfkTimelineRow)
                    .ToList();

                AfkTimelineList.ItemsSource = rows;
                AfkTimelineStatusText.Text = rows.Count == 0
                    ? "No AFK windows in selected range."
                    : $"{rows.Count} AFK window(s) in selected range.";

                var selected = previousSelection is null
                    ? null
                    : rows.FirstOrDefault(row => row.SelectionKey == previousSelection.SelectionKey);
                selected ??= rows.FirstOrDefault();
                AfkTimelineList.SelectedItem = selected;
                RefreshSelectedAfkWindowApps();
            }
            catch (Exception ex)
            {
                latestAfkWindows = new List<Services.AfkWindowUsage>();
                AfkTimelineList.ItemsSource = new List<AfkTimelineRow>();
                AfkWindowAppsList.ItemsSource = new List<AfkWindowAppRow>();
                AfkTimelineStatusText.Text = $"Failed to load AFK timeline: {ex.Message}";
            }
        }

        private void RefreshSelectedAfkWindowApps()
        {
            if (AfkTimelineList.SelectedItem is not AfkTimelineRow selected)
            {
                AfkWindowAppsList.ItemsSource = new List<AfkWindowAppRow>
                {
                    new()
                    {
                        AppText = "Select an AFK window to view top apps",
                        UsageText = "-",
                        SplitText = "-",
                        LastSeenText = "-",
                    },
                };
                return;
            }

            var rows = selected.Window.TopApps
                .OrderByDescending(app => app.BytesSent + app.BytesRecv)
                .Select(app => new AfkWindowAppRow
                {
                    AppText = string.IsNullOrWhiteSpace(app.DisplayName)
                        ? app.ProcessName
                        : $"{app.DisplayName} ({app.ProcessName})",
                    UsageText = FormatBytes(app.BytesSent + app.BytesRecv),
                    SplitText = $"Up {FormatBytes(app.BytesSent)} | Down {FormatBytes(app.BytesRecv)}",
                    LastSeenText = app.LastSeenTs > 0
                        ? $"Last seen {DateTimeOffset.FromUnixTimeSeconds(app.LastSeenTs).ToLocalTime():g}"
                        : "Last seen -",
                })
                .ToList();

            if (rows.Count == 0)
            {
                rows.Add(new AfkWindowAppRow
                {
                    AppText = "No app usage recorded in this AFK window",
                    UsageText = "-",
                    SplitText = "-",
                    LastSeenText = "-",
                });
            }

            AfkWindowAppsList.ItemsSource = rows;
        }

        private static AfkTimelineRow BuildAfkTimelineRow(Services.AfkWindowUsage window)
        {
            var startLocal = DateTimeOffset.FromUnixTimeSeconds(window.StartTs).ToLocalTime();
            var endTs = window.EndTs > window.StartTs ? window.EndTs - 1 : window.EndTs;
            var endLocal = DateTimeOffset.FromUnixTimeSeconds(endTs).ToLocalTime();
            var topApps = BuildAfkTopAppsPreview(window.TopApps);
            var duration = TimeSpan.FromSeconds(window.DurationSeconds);

            return new AfkTimelineRow
            {
                SelectionKey = $"{window.StartTs}:{window.EndTs}",
                WindowText = $"{startLocal:g} - {endLocal:g}",
                DurationText = duration.TotalHours >= 1
                    ? $"{(int)duration.TotalHours}h {duration.Minutes}m"
                    : $"{duration.Minutes}m {duration.Seconds}s",
                UsageText = FormatBytes(window.BytesSent + window.BytesRecv),
                TopAppsPreviewText = topApps,
                Window = window,
            };
        }

        private static string BuildAfkTopAppsPreview(IReadOnlyCollection<Services.AppBreakdownRow> apps)
        {
            if (apps.Count == 0)
            {
                return "No top apps";
            }

            var top = apps
                .Take(2)
                .Select(app => string.IsNullOrWhiteSpace(app.DisplayName) ? app.ProcessName : app.DisplayName)
                .ToList();
            if (apps.Count > top.Count)
            {
                top.Add($"+{apps.Count - top.Count} more");
            }

            return string.Join(", ", top);
        }

        private HashSet<string> BuildAfkProcessAllowSet()
        {
            return latestAfkWindows
                .SelectMany(window => window.TopApps)
                .Select(app => app.ProcessName?.Trim())
                .Where(process => !string.IsNullOrWhiteSpace(process))
                .Cast<string>()
                .ToHashSet(StringComparer.OrdinalIgnoreCase);
        }

        private static Services.AfkWindowUsage[] BuildExportAfkWindows(
            IEnumerable<Services.AfkWindowUsage> windows,
            long startTs,
            long endTs,
            string? selectedProcess)
        {
            var normalizedSelectedProcess = selectedProcess?.Trim();
            var hasSelectedProcess = !string.IsNullOrWhiteSpace(normalizedSelectedProcess);

            return windows
                .Where(window => window.EndTs >= startTs && window.StartTs < endTs)
                .Select(window =>
                {
                    var topApps = window.TopApps
                        .Where(app => !hasSelectedProcess
                            || app.ProcessName.Equals(
                                normalizedSelectedProcess,
                                StringComparison.OrdinalIgnoreCase))
                        .ToArray();

                    return new Services.AfkWindowUsage
                    {
                        StartTs = window.StartTs,
                        EndTs = window.EndTs,
                        DurationSeconds = window.DurationSeconds,
                        BytesSent = window.BytesSent,
                        BytesRecv = window.BytesRecv,
                        TopApps = topApps,
                    };
                })
                .ToArray();
        }

        private async Task RefreshAlertsHistoryAsync()
        {
            try
            {
                var (startUtc, endUtc) = ResolveTopAppsRangeUtc();
                AlertsHistoryRangeText.Text =
                    $"({startUtc.LocalDateTime:yyyy-MM-dd} to {endUtc.LocalDateTime.AddSeconds(-1):yyyy-MM-dd})";

                var response = await daemonClient.ListCapAlertEventsAsync(
                    startTs: startUtc.ToUnixTimeSeconds(),
                    endTs: endUtc.ToUnixTimeSeconds(),
                    limit: 200);

                var rows = response.Events
                    .Select(BuildAlertHistoryRow)
                    .ToList();

                AlertsHistoryList.ItemsSource = rows;
                AlertsHistoryStatusText.Text = rows.Count == 0
                    ? "No cap alerts fired in selected range."
                    : $"{rows.Count} cap alert event(s) in selected range.";
            }
            catch (Exception ex)
            {
                AlertsHistoryList.ItemsSource = new List<AlertHistoryRow>();
                AlertsHistoryStatusText.Text = $"Failed to load alerts history: {ex.Message}";
            }
        }

        private async void OnRefreshForecastClicked(object sender, RoutedEventArgs e) => await RefreshForecastAsync();
        private async void OnRefreshHeatmapClicked(object sender, RoutedEventArgs e) => await RefreshHeatmapAsync();
        private async void OnRefreshAnomaliesClicked(object sender, RoutedEventArgs e) => await RefreshAnomaliesAsync();

        private async Task RefreshForecastAsync()
        {
            try
            {
                var interfaceId = ResolveSelectedInterfaceId();
                var interfaceType = ResolveSelectedInterfaceType();
                var forecast = await daemonClient.GetForecastAsync(interfaceId, interfaceType);

                ForecastProjectedTotalText.Text = FormatBytes(forecast.ProjectedMonthEndBytes);
                ForecastProjectedCostText.Text = $"Cost: {forecast.ProjectedMonthEndCost:C2}";
                ForecastDailyAverageText.Text = FormatBytes(forecast.DailyAverageBytes);
                ForecastConfidenceText.Text = $"{FormatBytes(forecast.ConfidenceIntervalLow)} to {FormatBytes(forecast.ConfidenceIntervalHigh)}";
            }
            catch (Exception ex)
            {
                ForecastProjectedTotalText.Text = "Error";
                ForecastConfidenceText.Text = ex.Message;
            }
        }

        private async Task RefreshHeatmapAsync()
        {
            try
            {
                var (startUtc, endUtc) = ResolveTopAppsRangeUtc();
                var interfaceId = ResolveSelectedInterfaceId();
                var interfaceType = ResolveSelectedInterfaceType();
                var response = await daemonClient.GetUsageHeatmapAsync(
                    startUtc.ToUnixTimeSeconds(),
                    endUtc.ToUnixTimeSeconds(),
                    interfaceId,
                    interfaceType);

                var cells = new HeatmapCellViewModel[7 * 24];
                var maxUsage = response.Cells.Length > 0 ? response.Cells.Max(c => c.BytesTotal) : 0;

                for (uint d = 0; d < 7; d++)
                {
                    for (uint h = 0; h < 24; h++)
                    {
                        var cell = response.Cells.FirstOrDefault(c => c.DayOfWeek == d && c.HourOfDay == h);
                        var usage = cell?.BytesTotal ?? 0;
                        var intensity = maxUsage == 0 ? 0 : (double)usage / maxUsage;
                        var dayName = ((DayOfWeek)d).ToString();

                        cells[d * 24 + h] = new HeatmapCellViewModel
                        {
                            ToolTip = $"{dayName} {h:D2}:00 - {FormatBytes(usage)}",
                            IntensityBrush = new Microsoft.UI.Xaml.Media.SolidColorBrush(Windows.UI.Color.FromArgb(
                                (byte)(20 + (intensity * 235)), 0, 120, 215))
                        };
                    }
                }

                HeatmapGrid.ItemsSource = cells;
                HeatmapStatusText.Text = $"Showing 7x24 usage pattern for {cells.Length} slots.";
            }
            catch (Exception ex)
            {
                HeatmapStatusText.Text = $"Failed to load heatmap: {ex.Message}";
            }
        }

        private async Task RefreshAnomaliesAsync()
        {
            try
            {
                var (startUtc, endUtc) = ResolveTopAppsRangeUtc();
                var response = await daemonClient.GetAnomaliesAsync(startUtc.ToUnixTimeSeconds(), endUtc.ToUnixTimeSeconds());

                var rows = response.Anomalies.Select(a => new AnomalyDisplayRow
                {
                    TimestampText = DateTimeOffset.FromUnixTimeSeconds(a.Ts).ToLocalTime().ToString("g"),
                    AppId = a.AppId,
                    UsageText = FormatBytes(a.BytesTotal),
                    ZScoreText = $"Z: {a.ZScore:F1}"
                }).ToList();

                AnomaliesList.ItemsSource = rows;
                AnomaliesStatusText.Text = rows.Count == 0 ? "No anomalies detected." : $"Detected {rows.Count} anomalies.";
            }
            catch (Exception ex)
            {
                AnomaliesStatusText.Text = $"Failed to load anomalies: {ex.Message}";
            }
        }

        private AlertHistoryRow BuildAlertHistoryRow(Services.CapAlertEvent alert)
        {
            var state = string.IsNullOrWhiteSpace(alert.DeliveryState)
                ? "new"
                : alert.DeliveryState.Trim().ToLowerInvariant();
            var firedAtText = alert.FiredAt > 0
                ? $"Fired {DateTimeOffset.FromUnixTimeSeconds(alert.FiredAt).ToLocalTime():g}"
                : "Fired -";

            return new AlertHistoryRow
            {
                TitleText = $"{BuildAlertThresholdText(alert)} ({state})",
                ScopeText = BuildAlertScopeText(alert),
                UsageText = BuildAlertUsageText(alert.UsageBytes, alert.CapBytes),
                WindowText = BuildAlertWindowText(alert),
                FiredAtText = firedAtText,
            };
        }

        private string BuildAlertScopeText(Services.CapAlertEvent alert)
        {
            if (NormalizeCapScope(alert.Scope) != CapScopeInterface)
            {
                return "Scope: global cap";
            }

            var guid = alert.InterfaceGuid?.Trim();
            if (string.IsNullOrWhiteSpace(guid))
            {
                return "Scope: interface cap";
            }

            var match = latestInterfaces.FirstOrDefault(i => i.Guid.Equals(guid, StringComparison.OrdinalIgnoreCase));
            if (match is null)
            {
                return $"Scope: interface {guid}";
            }

            return $"Scope: {BuildInterfaceDisplayName(match)}";
        }

        private static string BuildAlertThresholdText(Services.CapAlertEvent alert)
        {
            var threshold = alert.ThresholdKind.Trim().ToLowerInvariant();
            return threshold switch
            {
                "pct_50" => "50% monthly threshold reached",
                "pct_80" => "80% monthly threshold reached",
                "pct_95" => "95% monthly threshold reached",
                "daily_cap" => "Daily cap threshold reached",
                _ => $"Threshold reached ({alert.ThresholdKind})",
            };
        }

        private static string BuildAlertUsageText(ulong usageBytes, ulong capBytes)
        {
            if (capBytes == 0)
            {
                return $"Usage {FormatBytes(usageBytes)}";
            }

            var percent = (usageBytes * 100d) / capBytes;
            return $"Usage {FormatBytes(usageBytes)} of {FormatBytes(capBytes)} ({percent:F1}%)";
        }

        private static string BuildAlertWindowText(Services.CapAlertEvent alert)
        {
            var windowKind = alert.WindowKind.Trim().ToLowerInvariant();
            var windowLabel = windowKind switch
            {
                "daily" => "Daily window",
                "monthly" => "Monthly window",
                _ => "Window",
            };

            if (alert.WindowStartTs <= 0 || alert.WindowEndTs <= 0)
            {
                return windowLabel;
            }

            var endTs = alert.WindowEndTs > alert.WindowStartTs
                ? alert.WindowEndTs - 1
                : alert.WindowEndTs;
            var start = DateTimeOffset.FromUnixTimeSeconds(alert.WindowStartTs).ToLocalTime();
            var end = DateTimeOffset.FromUnixTimeSeconds(endTs).ToLocalTime();
            return $"{windowLabel}: {start:g} - {end:g}";
        }

        private static List<InterfaceChartRow> BuildInterfaceChartRows(IEnumerable<Services.InterfaceUsageRow> interfaces)
        {
            var sourceRows = interfaces
                .Select(row => new
                {
                    DisplayName = BuildInterfaceUsageLabel(row),
                    BytesSent = row.BytesSent,
                    BytesRecv = row.BytesRecv,
                    TotalBytes = row.BytesSent + row.BytesRecv,
                })
                .OrderByDescending(row => row.TotalBytes)
                .ToList();

            var grandTotal = sourceRows.Aggregate(0UL, (sum, row) => SaturatingAdd(sum, row.TotalBytes));

            return sourceRows
                .Select(row =>
                {
                    var sharePercent = grandTotal == 0
                        ? 0d
                        : (row.TotalBytes * 100d) / grandTotal;
                    return new InterfaceChartRow
                    {
                        DisplayName = row.DisplayName,
                        SharePercent = Math.Clamp(sharePercent, 0d, 100d),
                        ShareText = $"{sharePercent:F1}% of selected range",
                        SplitText = $"Up {FormatBytes(row.BytesSent)} | Down {FormatBytes(row.BytesRecv)}",
                        TotalText = FormatBytes(row.TotalBytes),
                    };
                })
                .ToList();
        }

        private async Task RefreshSelectedAppDetailAsync()
        {
            try
            {
                if (TopAppsList.SelectedItem is not AppDisplayRow selected || selected.IsPlaceholder)
                {
                    AppDetailTitleText.Text = "Select an app from the list";
                    AppDetailSummaryText.Text = "Pick an app to view time series buckets.";
                    AppDetailChartStatusText.Text = "No chart data for the selected app and range.";
                    AppDetailChartStatusText.Visibility = Visibility.Visible;
                    AppDetailChartList.ItemsSource = new List<AppDetailChartRow>();
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

                var chartRows = BuildAppDetailChartRows(summary.Buckets, granularity);

                var scope = BuildInterfaceScopeLabel(interfaceId, interfaceType);
                var appLabel = selected.IsAggregateGroup
                    ? selected.DisplayName
                    : $"{selected.DisplayName} ({selected.ProcessName})";
                AppDetailTitleText.Text = appLabel;
                AppDetailSummaryText.Text =
                    $"{scope} Total {FormatBytes(summary.TotalSent + summary.TotalRecv)} " +
                    $"(Up {FormatBytes(summary.TotalSent)} | Down {FormatBytes(summary.TotalRecv)})." +
                    groupNote;
                AppDetailChartList.ItemsSource = chartRows;
                AppDetailChartStatusText.Text = "No chart data for the selected app and range.";
                AppDetailChartStatusText.Visibility = chartRows.Count == 0 ? Visibility.Visible : Visibility.Collapsed;
                AppDetailBucketsList.ItemsSource = bucketRows;
            }
            catch (Exception ex)
            {
                AppDetailSummaryText.Text = $"Failed to load app detail: {ex.Message}";
                AppDetailChartList.ItemsSource = new List<AppDetailChartRow>();
                AppDetailChartStatusText.Text = $"Failed to load app detail chart: {ex.Message}";
                AppDetailChartStatusText.Visibility = Visibility.Visible;
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

        private static AppDisplayRow? ResolveTopAppSelection(
            IReadOnlyCollection<AppDisplayRow> rows,
            AppDisplayRow? previousSelection)
        {
            if (previousSelection is null || previousSelection.IsPlaceholder)
            {
                return null;
            }

            var directMatch = rows.FirstOrDefault(row => row.SelectionKey == previousSelection.SelectionKey);
            if (directMatch is not null)
            {
                return directMatch;
            }

            if (previousSelection.IsAggregateGroup)
            {
                foreach (var member in previousSelection.GroupMembers)
                {
                    var ungroupedMatch = rows.FirstOrDefault(row =>
                        !row.IsPlaceholder
                        && !row.IsAggregateGroup
                        && row.ProcessName.Equals(member.ProcessName, StringComparison.OrdinalIgnoreCase));
                    if (ungroupedMatch is not null)
                    {
                        return ungroupedMatch;
                    }
                }
            }

            if (!previousSelection.IsAggregateGroup)
            {
                var processName = previousSelection.ProcessName.Trim();
                if (!string.IsNullOrWhiteSpace(processName))
                {
                    var groupedMatch = rows.FirstOrDefault(row =>
                        row.IsAggregateGroup
                        && row.GroupMembers.Any(member =>
                            member.ProcessName.Equals(processName, StringComparison.OrdinalIgnoreCase)));
                    if (groupedMatch is not null)
                    {
                        return groupedMatch;
                    }
                }
            }

            return null;
        }

        private static List<AppDetailChartRow> BuildAppDetailChartRows(
            IEnumerable<Services.UsageBucket> buckets,
            string granularity)
        {
            var normalized = buckets
                .OrderBy(bucket => bucket.Ts)
                .Select(bucket => new
                {
                    BucketLabel = FormatBucketLabel(bucket.Ts, granularity),
                    BytesSent = bucket.BytesSent,
                    BytesRecv = bucket.BytesRecv,
                    TotalBytes = bucket.BytesSent + bucket.BytesRecv,
                })
                .ToList();

            if (normalized.Count == 0)
            {
                return new List<AppDetailChartRow>();
            }

            var peakBytes = normalized.Max(bucket => bucket.TotalBytes);

            return normalized
                .Select(bucket =>
                {
                    var relative = peakBytes == 0
                        ? 0d
                        : (bucket.TotalBytes * 100d) / peakBytes;
                    return new AppDetailChartRow
                    {
                        BucketLabel = bucket.BucketLabel,
                        SplitText = $"Up {FormatBytes(bucket.BytesSent)} | Down {FormatBytes(bucket.BytesRecv)}",
                        UsageText = FormatBytes(bucket.TotalBytes),
                        RelativePercent = Math.Clamp(relative, 0d, 100d),
                        RelativeText = peakBytes == 0
                            ? "Peak 0%"
                            : $"Peak {relative:F1}%",
                    };
                })
                .ToList();
        }

        private List<AppDisplayRow> BuildTopAppRows(IEnumerable<Services.AppBreakdownRow> apps, string sortKey)
        {
            var normalRows = new List<AppDisplayRow>();
            var systemMembers = new List<AppGroupMember>();
            var otherMembers = new List<AppGroupMember>();

            foreach (var app in apps)
            {
                var processName = NormalizeProcessName(app.ProcessName);
                var totalBytes = app.BytesSent + app.BytesRecv;
                var displayName = string.IsNullOrWhiteSpace(app.DisplayName) ? processName : app.DisplayName.Trim();
                if (IsSystemGroupedApp(processName, displayName))
                {
                    systemMembers.Add(new AppGroupMember
                    {
                        ProcessName = processName,
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
                        ProcessName = processName,
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
                    ProcessName = processName,
                    BytesSent = app.BytesSent,
                    BytesRecv = app.BytesRecv,
                    LastSeenTs = app.LastSeenTs,
                    UsageText = FormatBytes(totalBytes),
                    ProcessText = processName,
                    LastSeenText = BuildLastSeenText(app.LastSeenTs),
                    TransferSplitText = BuildTransferSplitText(app.BytesSent, app.BytesRecv),
                    SelectionKey = $"app:{processName.ToLowerInvariant()}",
                    IconGlyph = "\uE71B",
                    IconForeground = "#7FD1AE",
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
                "name_asc" => normalRows.OrderBy(row => row.DisplayName, StringComparer.OrdinalIgnoreCase)
                    .ThenBy(row => row.ProcessName, StringComparer.OrdinalIgnoreCase)
                    .ThenByDescending(row => row.TotalBytes)
                    .ThenByDescending(row => row.BytesSent)
                    .ThenByDescending(row => row.BytesRecv),
                "upload_desc" => normalRows.OrderByDescending(row => row.BytesSent)
                    .ThenByDescending(row => row.BytesRecv)
                    .ThenByDescending(row => row.TotalBytes)
                    .ThenBy(row => row.DisplayName, StringComparer.OrdinalIgnoreCase)
                    .ThenBy(row => row.ProcessName, StringComparer.OrdinalIgnoreCase),
                "download_desc" => normalRows.OrderByDescending(row => row.BytesRecv)
                    .ThenByDescending(row => row.BytesSent)
                    .ThenByDescending(row => row.TotalBytes)
                    .ThenBy(row => row.DisplayName, StringComparer.OrdinalIgnoreCase)
                    .ThenBy(row => row.ProcessName, StringComparer.OrdinalIgnoreCase),
                _ => normalRows.OrderByDescending(row => row.TotalBytes)
                    .ThenByDescending(row => row.BytesSent)
                    .ThenByDescending(row => row.BytesRecv)
                    .ThenBy(row => row.DisplayName, StringComparer.OrdinalIgnoreCase)
                    .ThenBy(row => row.ProcessName, StringComparer.OrdinalIgnoreCase),
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
                ProcessText = $"{members.Count} grouped processes",
                LastSeenText = BuildLastSeenText(latestSeenTs),
                TransferSplitText = BuildTransferSplitText(totalSent, totalRecv),
                SelectionKey = $"group:{groupKey}",
                IsAggregateGroup = true,
                GroupMembers = orderedMembers,
                IconGlyph = groupKey.Equals("system", StringComparison.OrdinalIgnoreCase) ? "\uE770" : "\uE8FD",
                IconForeground = groupKey.Equals("system", StringComparison.OrdinalIgnoreCase) ? "#84B5F6" : "#D6B87B",
            };
        }

        private static bool IsSystemGroupedApp(string processName, string displayName)
        {
            var normalized = processName.Trim().ToLowerInvariant();
            if (normalized is "" or "unknown" or "system" or "system idle process" or "idle" or "unattributed")
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

        private static string BuildTransferSplitText(ulong sent, ulong recv)
        {
            return $"Up {FormatBytes(sent)} | Down {FormatBytes(recv)}";
        }

        private static string BuildLastSeenText(long lastSeenTs)
        {
            return lastSeenTs > 0
                ? $"Last seen {DateTimeOffset.FromUnixTimeSeconds(lastSeenTs).ToLocalTime():g}"
                : "Last seen -";
        }

        private static string NormalizeProcessName(string processName)
        {
            var normalized = processName?.Trim();
            return string.IsNullOrWhiteSpace(normalized) ? "unknown" : normalized;
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

        private (DateTimeOffset StartUtc, DateTimeOffset EndUtc, string Label) ResolveSummaryActionRangeUtc(string mode)
        {
            if (mode == OverviewModeSelectedRange)
            {
                var (startUtc, endUtc) = ResolveTopAppsRangeUtc();
                var label = $"Selected range ({startUtc.LocalDateTime:yyyy-MM-dd} to {endUtc.AddSeconds(-1).LocalDateTime:yyyy-MM-dd})";
                return (startUtc, endUtc, label);
            }

            var localNow = DateTimeOffset.Now;
            var dayStartLocal = new DateTimeOffset(localNow.Year, localNow.Month, localNow.Day, 0, 0, 0, localNow.Offset);
            return (dayStartLocal.ToUniversalTime(), DateTimeOffset.UtcNow, "Today");
        }

        private string ResolveSelectedOverviewModeKey()
        {
            if (OverviewModeComboBox.SelectedItem is OverviewModeOption option)
            {
                selectedOverviewMode = option.Key;
            }

            return selectedOverviewMode;
        }

        private string ResolveSelectedRangePresetKey()
        {
            if (RangePresetComboBox.SelectedItem is RangePresetOption option)
            {
                selectedRangePreset = option.Key;
            }

            return selectedRangePreset;
        }

        private void SetRangePresetSelection(string presetKey)
        {
            selectedRangePreset = presetKey;
            if (RangePresetComboBox.ItemsSource is not IEnumerable<RangePresetOption> options)
            {
                return;
            }

            var selected = options.FirstOrDefault(option => option.Key == presetKey)
                ?? options.FirstOrDefault(option => option.Key == RangePresetCustom)
                ?? options.FirstOrDefault();
            if (selected is null)
            {
                return;
            }

            suppressRangeControlEvents = true;
            try
            {
                RangePresetComboBox.SelectedItem = selected;
            }
            finally
            {
                suppressRangeControlEvents = false;
            }
        }

        private void ApplyRangePresetToDatePickers(string presetKey)
        {
            var localNow = DateTimeOffset.Now;
            var todayLocal = new DateTimeOffset(localNow.Year, localNow.Month, localNow.Day, 0, 0, 0, localNow.Offset);
            DateTimeOffset startLocal;
            DateTimeOffset endLocal;

            switch (presetKey)
            {
                case RangePresetToday:
                    startLocal = todayLocal;
                    endLocal = todayLocal;
                    break;
                case RangePresetLast30Days:
                    startLocal = todayLocal.AddDays(-29);
                    endLocal = todayLocal;
                    break;
                case RangePresetLast7Days:
                default:
                    startLocal = todayLocal.AddDays(-6);
                    endLocal = todayLocal;
                    break;
            }

            suppressRangeControlEvents = true;
            try
            {
                StartDatePicker.Date = startLocal;
                EndDatePicker.Date = endLocal;
            }
            finally
            {
                suppressRangeControlEvents = false;
            }
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

        private string ResolveSelectedCapScopeKey()
        {
            if (CapScopeComboBox.SelectedItem is CapScopeOption option)
            {
                selectedCapScope = option.Key;
            }

            return selectedCapScope;
        }

        private void SetCapScopeSelection(string scopeKey)
        {
            selectedCapScope = NormalizeCapScope(scopeKey);
            if (CapScopeComboBox.ItemsSource is not IEnumerable<CapScopeOption> options)
            {
                return;
            }

            var selected = options.FirstOrDefault(option => option.Key == selectedCapScope)
                ?? options.FirstOrDefault();
            if (selected is null)
            {
                return;
            }

            CapScopeComboBox.SelectedItem = selected;
        }

        private void UpdateCapInterfaceOptions()
        {
            var currentGuid = ResolveSelectedCapInterfaceGuid();
            var items = latestInterfaces
                .Where(ShouldIncludeAdapterInFilter)
                .Select(i => new CapInterfaceOption
                {
                    Guid = i.Guid,
                    DisplayName = BuildInterfaceDisplayName(i),
                })
                .ToList();

            CapInterfaceComboBox.DisplayMemberPath = nameof(CapInterfaceOption.DisplayName);
            CapInterfaceComboBox.ItemsSource = items;
            var selected = items.FirstOrDefault(item => item.Guid == currentGuid) ?? items.FirstOrDefault();
            CapInterfaceComboBox.SelectedItem = selected;
            UpdateCapInterfaceControls();
        }

        private void UpdateCapInterfaceControls()
        {
            var requiresInterface = ResolveSelectedCapScopeKey() == CapScopeInterface;
            CapInterfaceComboBox.IsEnabled = requiresInterface;
            CapInterfaceComboBox.Opacity = requiresInterface ? 1.0 : 0.6;
        }

        private string? ResolveSelectedCapInterfaceGuid()
        {
            return (CapInterfaceComboBox.SelectedItem as CapInterfaceOption)?.Guid;
        }

        private void SetCapInterfaceSelection(string? guid)
        {
            if (CapInterfaceComboBox.ItemsSource is not IEnumerable<CapInterfaceOption> options)
            {
                return;
            }

            var selected = options.FirstOrDefault(option => option.Guid == guid)
                ?? options.FirstOrDefault();
            CapInterfaceComboBox.SelectedItem = selected;
        }

        private ulong ResolveCapGigabytes()
        {
            var value = CapMonthlyGbNumberBox.Value;
            if (double.IsNaN(value) || double.IsInfinity(value))
            {
                CapMonthlyGbNumberBox.Value = 1;
                return 1;
            }

            var rounded = (ulong)Math.Round(value);
            var normalized = Math.Clamp(rounded, 1UL, 1_048_576UL);
            CapMonthlyGbNumberBox.Value = normalized;
            return normalized;
        }

        private static string NormalizeCapScope(string scope)
        {
            return scope.Trim().Equals(CapScopeInterface, StringComparison.OrdinalIgnoreCase)
                ? CapScopeInterface
                : CapScopeGlobal;
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

        private static string MapTopAppsSortToDaemon(string sortKey)
        {
            return sortKey switch
            {
                "upload_desc" => "bytes_sent_desc",
                "download_desc" => "bytes_recv_desc",
                "name_asc" => "display_name_asc",
                _ => "total_bytes_desc",
            };
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
                return NormalizeGranularity(granularity);
            }

            return DefaultExportGranularity;
        }

        private string ResolveSettingsExportGranularity()
        {
            if (SettingsExportGranularityComboBox.SelectedItem is string granularity)
            {
                var normalized = NormalizeGranularity(granularity);
                SettingsExportGranularityComboBox.SelectedItem = normalized;
                return normalized;
            }

            SettingsExportGranularityComboBox.SelectedItem = DefaultExportGranularity;
            return DefaultExportGranularity;
        }

        private static string NormalizeGranularity(string? value)
        {
            var normalized = value?.Trim().ToLowerInvariant();
            return normalized is "hour" or "day" or "week" or "month"
                ? normalized
                : DefaultExportGranularity;
        }

        private void ApplyExportDefaultsToControls(
            string granularity,
            bool includeSummary,
            bool includeApps,
            bool includeInterfaces)
        {
            ExportGranularityComboBox.SelectedItem = NormalizeGranularity(granularity);
            IncludeSummaryCheckBox.IsChecked = includeSummary;
            IncludeAppsCheckBox.IsChecked = includeApps;
            IncludeInterfacesCheckBox.IsChecked = includeInterfaces;
            IncludeAfkCheckBox.IsChecked = DefaultExportIncludeAfk;
        }

        private string ResolveSelectedExportAppScope()
        {
            if (ExportAppScopeComboBox.SelectedItem is ExportAppScopeOption option)
            {
                return option.Key;
            }

            return "all";
        }

        private string? ResolveSelectedConcreteTopAppProcess()
        {
            if (TopAppsList.SelectedItem is not AppDisplayRow selected || selected.IsPlaceholder || selected.IsAggregateGroup)
            {
                return null;
            }

            var process = selected.ProcessName?.Trim();
            return string.IsNullOrWhiteSpace(process) ? null : process;
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

        private string BuildCapScopeText(Services.CapDefinition cap)
        {
            if (NormalizeCapScope(cap.Scope) == CapScopeGlobal)
            {
                return "Global cap";
            }

            var guid = cap.InterfaceGuid?.Trim();
            if (string.IsNullOrWhiteSpace(guid))
            {
                return "Interface cap (unknown interface)";
            }

            var match = latestInterfaces.FirstOrDefault(i => i.Guid.Equals(guid, StringComparison.OrdinalIgnoreCase));
            if (match is null)
            {
                return $"Interface cap ({guid})";
            }

            return $"Interface cap ({BuildInterfaceDisplayName(match)})";
        }

        private static string BuildRetentionCleanupStatusText(Services.DaemonStatus status)
        {
            var result = status.RetentionCleanupLastResult?.Trim().ToLowerInvariant();
            if (string.IsNullOrWhiteSpace(result) || result == "never")
            {
                return "Retention cleanup has not run yet.";
            }

            if (result == "skipped_unlimited")
            {
                var skippedAt = status.RetentionCleanupLastRunTs > 0
                    ? DateTimeOffset.FromUnixTimeSeconds(status.RetentionCleanupLastRunTs).ToLocalTime().ToString("g")
                    : "unknown";
                return $"Retention cleanup skipped (retention_days=0) at {skippedAt}.";
            }

            var runAtText = status.RetentionCleanupLastRunTs > 0
                ? DateTimeOffset.FromUnixTimeSeconds(status.RetentionCleanupLastRunTs).ToLocalTime().ToString("g")
                : "unknown";
            var cutoffText = status.RetentionCleanupCutoffTs > 0
                ? DateTimeOffset.FromUnixTimeSeconds(status.RetentionCleanupCutoffTs).ToLocalTime().ToString("g")
                : "n/a";

            return $"Retention cleanup {result} at {runAtText}. " +
                   $"Cutoff {cutoffText}. Deleted {status.RetentionCleanupDeletedUsageRecords} usage rows, {status.RetentionCleanupDeletedAfkWindows} AFK windows.";
        }

        private static string BuildReliabilityStatusText(Services.DaemonStatus status)
        {
            var lastStartText = status.DaemonLastStartTs > 0
                ? DateTimeOffset.FromUnixTimeSeconds(status.DaemonLastStartTs).ToLocalTime().ToString("g")
                : "unknown";
            var baseline =
                $"Daemon starts {status.DaemonStartCount}, clean exits {status.DaemonCleanExitCount}, unexpected exits {status.DaemonUnexpectedExitCount}, poll errors {status.PollErrorCount}, IPC errors {status.IpcErrorCount}. Last start {lastStartText}.";

            if (status.DaemonLastErrorTs <= 0 || string.IsNullOrWhiteSpace(status.DaemonLastErrorStage))
            {
                return baseline;
            }

            var lastErrorText = DateTimeOffset.FromUnixTimeSeconds(status.DaemonLastErrorTs).ToLocalTime().ToString("g");
            var message = string.IsNullOrWhiteSpace(status.DaemonLastErrorMessage)
                ? "no message"
                : status.DaemonLastErrorMessage;
            return baseline + $" Last error {status.DaemonLastErrorStage} at {lastErrorText}: {message}";
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

        private static void ApplySummaryCard(
            TextBlock total,
            TextBlock split,
            ProgressBar uploadShareBar,
            TextBlock uploadShareText,
            Services.UsageSummary summary)
        {
            var totalBytes = summary.TotalSent + summary.TotalRecv;
            total.Text = FormatBytes(totalBytes);
            split.Text = $"Up {FormatBytes(summary.TotalSent)} | Down {FormatBytes(summary.TotalRecv)}";

            var uploadPct = totalBytes == 0 ? 0d : (summary.TotalSent * 100d) / totalBytes;
            uploadShareBar.Value = uploadPct;
            uploadShareText.Text = $"Upload share {uploadPct:F1}%";
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
            var sanitized = string.IsNullOrEmpty(value) ? value : NeutralizeCsvFormula(value);
            var escaped = sanitized.Replace("\"", "\"\"");
            return '"' + escaped + '"';
        }

        private static string NeutralizeCsvFormula(string value)
        {
            if (string.IsNullOrEmpty(value))
            {
                return value;
            }

            var leading = value[0];
            return leading is '=' or '+' or '-' or '@'
                ? "'" + value
                : value;
        }

        private static string CreateUniqueExportPath(string directory, string baseFileName, string extension)
        {
            var candidate = Path.Combine(directory, baseFileName + extension);
            if (!File.Exists(candidate))
            {
                return candidate;
            }

            for (var suffix = 1; suffix < 10_000; suffix++)
            {
                candidate = Path.Combine(directory, $"{baseFileName}_{suffix:000}{extension}");
                if (!File.Exists(candidate))
                {
                    return candidate;
                }
            }

            throw new IOException($"Unable to create unique export path in {directory}.");
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

            public string ProcessText { get; init; } = string.Empty;

            public string LastSeenText { get; init; } = string.Empty;

            public string TransferSplitText { get; init; } = string.Empty;

            public string SelectionKey { get; init; } = string.Empty;

            public bool IsPlaceholder { get; init; }

            public bool IsAggregateGroup { get; init; }

            public AppGroupMember[] GroupMembers { get; init; } = Array.Empty<AppGroupMember>();

            public string IconGlyph { get; init; } = "\uE71B";

            public string IconForeground { get; init; } = "#7FD1AE";
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

        public sealed class OverviewModeOption
        {
            public string Key { get; init; } = string.Empty;

            public string DisplayName { get; init; } = string.Empty;
        }

        public sealed class RangePresetOption
        {
            public string Key { get; init; } = string.Empty;

            public string DisplayName { get; init; } = string.Empty;
        }

        public sealed class ExportAppScopeOption
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

        public sealed class AppDetailChartRow
        {
            public string BucketLabel { get; init; } = string.Empty;

            public string UsageText { get; init; } = string.Empty;

            public string SplitText { get; init; } = string.Empty;

            public string RelativeText { get; init; } = string.Empty;

            public double RelativePercent { get; init; }
        }

        public sealed class InterfaceFilterOption
        {
            public string DisplayName { get; init; } = string.Empty;

            public string? InterfaceId { get; init; }

            public string? InterfaceType { get; init; }
        }

        public sealed class InterfaceChartRow
        {
            public string DisplayName { get; init; } = string.Empty;

            public string TotalText { get; init; } = string.Empty;

            public string SplitText { get; init; } = string.Empty;

            public string ShareText { get; init; } = string.Empty;

            public double SharePercent { get; init; }
        }

        public sealed class CapScopeOption
        {
            public string Key { get; init; } = string.Empty;

            public string DisplayName { get; init; } = string.Empty;
        }

        public sealed class CapInterfaceOption
        {
            public string Guid { get; init; } = string.Empty;

            public string DisplayName { get; init; } = string.Empty;
        }

        public sealed class CapDefinitionRow
        {
            public long Id { get; init; }

            public string ScopeKey { get; init; } = CapScopeGlobal;

            public string? InterfaceGuid { get; init; }

            public ulong MonthlyCapBytes { get; init; }

            public ulong MonthlyCapGb { get; init; }

            public bool IsActive { get; init; }

            public string ScopeText { get; init; } = string.Empty;

            public string CapText { get; init; } = string.Empty;

            public string ActiveText { get; init; } = string.Empty;
        }

        public sealed class AlertHistoryRow
        {
            public string TitleText { get; init; } = string.Empty;

            public string ScopeText { get; init; } = string.Empty;

            public string UsageText { get; init; } = string.Empty;

            public string WindowText { get; init; } = string.Empty;

            public string FiredAtText { get; init; } = string.Empty;
        }

        public sealed class AfkTimelineRow
        {
            public string SelectionKey { get; init; } = string.Empty;

            public string WindowText { get; init; } = string.Empty;

            public string DurationText { get; init; } = string.Empty;

            public string UsageText { get; init; } = string.Empty;

            public string TopAppsPreviewText { get; init; } = string.Empty;

            public Services.AfkWindowUsage Window { get; init; } = new();
        }

        public sealed class AfkWindowAppRow
        {
            public string AppText { get; init; } = string.Empty;

            public string UsageText { get; init; } = string.Empty;

            public string SplitText { get; init; } = string.Empty;

            public string LastSeenText { get; init; } = string.Empty;
        }

        public sealed class HeatmapCellViewModel
        {
            public string ToolTip { get; init; } = string.Empty;
            public Microsoft.UI.Xaml.Media.Brush IntensityBrush { get; init; } = new Microsoft.UI.Xaml.Media.SolidColorBrush(Microsoft.UI.Colors.Transparent);
        }

        public sealed class AnomalyDisplayRow
        {
            public string TimestampText { get; init; } = string.Empty;
            public string AppId { get; init; } = string.Empty;
            public string UsageText { get; init; } = string.Empty;
            public string ZScoreText { get; init; } = string.Empty;
        }
    }
}
