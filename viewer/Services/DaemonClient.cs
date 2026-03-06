using System;
using System.IO;
using System.IO.Pipes;
using System.Text;
using System.Text.Json;
using System.Text.Json.Serialization;
using System.Threading;
using System.Threading.Tasks;

namespace SingularityMonitor.Viewer.Services;

public sealed class DaemonClient
{
    private static readonly JsonSerializerOptions JsonOptions = new()
    {
        PropertyNameCaseInsensitive = true,
    };

    private const string PipeName = "SingularityMonitor";

    public async Task<DaemonStatus> GetDaemonStatusAsync(CancellationToken cancellationToken = default)
    {
        var envelope = await SendRequestAsync(
            "GET_DAEMON_STATUS",
            payload: new { },
            cancellationToken);

        return ParsePayload<DaemonStatus>(envelope);
    }

    public async Task<SettingsResponse> GetSettingsAsync(CancellationToken cancellationToken = default)
    {
        var envelope = await SendRequestAsync(
            "GET_SETTINGS",
            payload: new { },
            cancellationToken);

        return ParsePayload<SettingsResponse>(envelope);
    }

    public async Task ApplySettingsAsync(
        uint? pollIntervalSeconds = null,
        uint? retentionDays = null,
        uint? afkIdleThresholdSeconds = null,
        bool? onboardingCompleted = null,
        string? exportDefaultGranularity = null,
        bool? exportDefaultIncludeSummary = null,
        bool? exportDefaultIncludeApps = null,
        bool? exportDefaultIncludeInterfaces = null,
        CancellationToken cancellationToken = default)
    {
        var envelope = await SendRequestAsync(
            "SET_SETTINGS",
            payload: new
            {
                poll_interval_seconds = pollIntervalSeconds,
                retention_days = retentionDays,
                afk_idle_threshold_seconds = afkIdleThresholdSeconds,
                onboarding_completed = onboardingCompleted,
                export_default_granularity = exportDefaultGranularity,
                export_default_include_summary = exportDefaultIncludeSummary,
                export_default_include_apps = exportDefaultIncludeApps,
                export_default_include_interfaces = exportDefaultIncludeInterfaces,
            },
            cancellationToken);

        _ = ParsePayload<JsonElement>(envelope);
    }

    public async Task<CompactDatabaseResponse> CompactDatabaseAsync(CancellationToken cancellationToken = default)
    {
        var envelope = await SendRequestAsync(
            "COMPACT_DATABASE",
            payload: new { },
            cancellationToken);

        return ParsePayload<CompactDatabaseResponse>(envelope);
    }

    public async Task<UsageSummary> GetTodaySummaryAsync(CancellationToken cancellationToken = default)
    {
        var now = DateTimeOffset.UtcNow;
        var startOfDay = new DateTimeOffset(now.Year, now.Month, now.Day, 0, 0, 0, TimeSpan.Zero)
            .ToUnixTimeSeconds();

        return await GetUsageSummaryAsync(
            startOfDay,
            now.ToUnixTimeSeconds(),
            granularity: "day",
            cancellationToken: cancellationToken);
    }

    public async Task<UsageSummary> GetUsageSummaryAsync(
        long startTs,
        long endTs,
        string granularity = "day",
        string? interfaceId = null,
        string? interfaceType = null,
        string? appFilter = null,
        CancellationToken cancellationToken = default)
    {
        var envelope = await SendRequestAsync(
            "GET_USAGE_SUMMARY",
            payload: new
            {
                start_ts = startTs,
                end_ts = endTs,
                granularity,
                interface_id = interfaceId,
                interface_type = interfaceType,
                app_filter = appFilter,
            },
            cancellationToken);

        return ParsePayload<UsageSummary>(envelope);
    }

    public async Task<AppBreakdown> GetRecentAppBreakdownAsync(
        int lookbackHours = 24,
        int limit = 12,
        CancellationToken cancellationToken = default)
    {
        var now = DateTimeOffset.UtcNow.ToUnixTimeSeconds();
        var start = now - Math.Max(1, lookbackHours) * 3600L;

        return await GetAppBreakdownAsync(
            start,
            now,
            limit,
            cancellationToken: cancellationToken);
    }

    public async Task<AppBreakdown> GetAppBreakdownAsync(
        long startTs,
        long endTs,
        int limit = 12,
        string? interfaceId = null,
        string? interfaceType = null,
        string? sortBy = null,
        CancellationToken cancellationToken = default)
    {
        var normalizedLimit = Math.Max(1, limit);
        var normalizedSortBy = NormalizeAppBreakdownSort(sortBy);

        var envelope = await SendRequestAsync(
            "GET_APP_BREAKDOWN",
            payload: new
            {
                start_ts = startTs,
                end_ts = endTs,
                interface_id = interfaceId,
                interface_type = interfaceType,
                limit = normalizedLimit,
                sort_by = normalizedSortBy,
            },
            cancellationToken);

        return ParsePayload<AppBreakdown>(envelope);
    }

    private static string NormalizeAppBreakdownSort(string? sortBy)
    {
        var normalized = sortBy?.Trim().ToLowerInvariant();
        return normalized is "total_bytes_desc" or "bytes_sent_desc" or "bytes_recv_desc" or "display_name_asc"
            ? normalized
            : "total_bytes_desc";
    }

    public async Task<InterfaceListResponse> GetInterfacesAsync(CancellationToken cancellationToken = default)
    {
        var envelope = await SendRequestAsync(
            "GET_INTERFACES",
            payload: new { },
            cancellationToken);

        return ParsePayload<InterfaceListResponse>(envelope);
    }

    public async Task<InterfaceBreakdownResponse> GetInterfaceBreakdownAsync(
        long startTs,
        long endTs,
        string? interfaceId = null,
        string? interfaceType = null,
        CancellationToken cancellationToken = default)
    {
        var envelope = await SendRequestAsync(
            "GET_INTERFACE_BREAKDOWN",
            payload: new
            {
                start_ts = startTs,
                end_ts = endTs,
                interface_id = interfaceId,
                interface_type = interfaceType,
            },
            cancellationToken);

        return ParsePayload<InterfaceBreakdownResponse>(envelope);
    }

    public async Task<CapDefinitionsResponse> ListCapDefinitionsAsync(CancellationToken cancellationToken = default)
    {
        var envelope = await SendRequestAsync(
            "LIST_CAP_DEFINITIONS",
            payload: new { },
            cancellationToken);

        return ParsePayload<CapDefinitionsResponse>(envelope);
    }

    public async Task<CapDefinitionUpsertResponse> UpsertCapDefinitionAsync(
        string scope,
        ulong monthlyCapBytes,
        bool isActive,
        string? interfaceGuid = null,
        long? id = null,
        CancellationToken cancellationToken = default)
    {
        var envelope = await SendRequestAsync(
            "UPSERT_CAP_DEFINITION",
            payload: new
            {
                id,
                scope,
                interface_guid = interfaceGuid,
                monthly_cap_bytes = monthlyCapBytes,
                is_active = isActive,
            },
            cancellationToken);

        return ParsePayload<CapDefinitionUpsertResponse>(envelope);
    }

    public async Task<CapDefinitionDeleteResponse> DeleteCapDefinitionAsync(
        long id,
        CancellationToken cancellationToken = default)
    {
        var envelope = await SendRequestAsync(
            "DELETE_CAP_DEFINITION",
            payload: new { id },
            cancellationToken);

        return ParsePayload<CapDefinitionDeleteResponse>(envelope);
    }

    public async Task<CapAlertEventsResponse> ListCapAlertEventsAsync(
        long? startTs = null,
        long? endTs = null,
        string? scope = null,
        string? interfaceGuid = null,
        string? windowKind = null,
        string? thresholdKind = null,
        string? deliveryState = null,
        int limit = 200,
        CancellationToken cancellationToken = default)
    {
        var normalizedLimit = Math.Clamp(limit, 1, 1000);

        var envelope = await SendRequestAsync(
            "LIST_CAP_ALERT_EVENTS",
            payload: new
            {
                start_ts = startTs,
                end_ts = endTs,
                scope,
                interface_guid = interfaceGuid,
                window_kind = windowKind,
                threshold_kind = thresholdKind,
                delivery_state = deliveryState,
                limit = normalizedLimit,
            },
            cancellationToken);

        return ParsePayload<CapAlertEventsResponse>(envelope);
    }

    public async Task<MarkCapAlertEventsDeliveredResponse> MarkCapAlertEventsDeliveredAsync(
        long[] eventIds,
        CancellationToken cancellationToken = default)
    {
        var envelope = await SendRequestAsync(
            "MARK_CAP_ALERT_EVENTS_DELIVERED",
            payload: new
            {
                event_ids = eventIds,
            },
            cancellationToken);

        return ParsePayload<MarkCapAlertEventsDeliveredResponse>(envelope);
    }

    public async Task<AfkAuditResponse> GetAfkAuditAsync(
        long? startTs = null,
        long? endTs = null,
        int? limit = null,
        CancellationToken cancellationToken = default)
    {
        var normalizedLimit = limit.HasValue
            ? Math.Clamp(limit.Value, 1, 1000)
            : (int?)null;

        var envelope = await SendRequestAsync(
            "GET_AFK_AUDIT",
            payload: new
            {
                start_ts = startTs,
                end_ts = endTs,
                limit = normalizedLimit,
            },
            cancellationToken);

        return ParsePayload<AfkAuditResponse>(envelope);
    }

    private static T ParsePayload<T>(IpcEnvelope envelope)
    {
        if (envelope.Error is not null)
        {
            throw new InvalidOperationException($"Daemon error {envelope.Error.Code}: {envelope.Error.Message}");
        }

        var value = envelope.Payload.Deserialize<T>(JsonOptions);
        if (value is null)
        {
            throw new InvalidOperationException("Daemon returned an empty payload.");
        }

        return value;
    }

    private static async Task<IpcEnvelope> SendRequestAsync(
        string method,
        object payload,
        CancellationToken cancellationToken)
    {
        using var pipe = new NamedPipeClientStream(
            ".",
            PipeName,
            PipeDirection.InOut,
            PipeOptions.Asynchronous);

        using var timeout = CancellationTokenSource.CreateLinkedTokenSource(cancellationToken);
        timeout.CancelAfter(TimeSpan.FromSeconds(3));

        await pipe.ConnectAsync(timeout.Token);
        using var writer = new StreamWriter(pipe, new UTF8Encoding(false), leaveOpen: true)
        {
            AutoFlush = true,
        };
        using var reader = new StreamReader(pipe, Encoding.UTF8, detectEncodingFromByteOrderMarks: false, leaveOpen: true);

        var request = new IpcEnvelope
        {
            Id = Guid.NewGuid().ToString(),
            Type = "request",
            Method = method,
            Payload = JsonSerializer.SerializeToElement(payload, JsonOptions),
            Error = null,
        };

        var line = JsonSerializer.Serialize(request, JsonOptions);
        await writer.WriteLineAsync(line);

        var response = await reader.ReadLineAsync(timeout.Token);
        if (string.IsNullOrWhiteSpace(response))
        {
            throw new InvalidOperationException("Daemon returned an empty response.");
        }

        var envelope = JsonSerializer.Deserialize<IpcEnvelope>(response, JsonOptions);
        return envelope ?? throw new InvalidOperationException("Invalid daemon response envelope.");
    }
}

public sealed class DaemonStatus
{
    [JsonPropertyName("version")]
    public string Version { get; set; } = string.Empty;

    [JsonPropertyName("uptime_seconds")]
    public ulong UptimeSeconds { get; set; }

    [JsonPropertyName("memory_bytes")]
    public ulong MemoryBytes { get; set; }

    [JsonPropertyName("db_size_bytes")]
    public ulong DbSizeBytes { get; set; }

    [JsonPropertyName("last_poll_ts")]
    public long LastPollTs { get; set; }

    [JsonPropertyName("poll_interval_seconds")]
    public uint PollIntervalSeconds { get; set; }

    [JsonPropertyName("last_helper_ingest_ts")]
    public long LastHelperIngestTs { get; set; }

    [JsonPropertyName("import_status")]
    public string ImportStatus { get; set; } = string.Empty;

    [JsonPropertyName("import_progress_pct")]
    public int ImportProgressPct { get; set; }

    [JsonPropertyName("retention_cleanup_last_run_ts")]
    public long RetentionCleanupLastRunTs { get; set; }

    [JsonPropertyName("retention_cleanup_cutoff_ts")]
    public long RetentionCleanupCutoffTs { get; set; }

    [JsonPropertyName("retention_cleanup_deleted_usage_records")]
    public ulong RetentionCleanupDeletedUsageRecords { get; set; }

    [JsonPropertyName("retention_cleanup_deleted_afk_windows")]
    public ulong RetentionCleanupDeletedAfkWindows { get; set; }

    [JsonPropertyName("retention_cleanup_last_result")]
    public string RetentionCleanupLastResult { get; set; } = string.Empty;

    [JsonPropertyName("daemon_start_count")]
    public ulong DaemonStartCount { get; set; }

    [JsonPropertyName("daemon_clean_exit_count")]
    public ulong DaemonCleanExitCount { get; set; }

    [JsonPropertyName("daemon_unexpected_exit_count")]
    public ulong DaemonUnexpectedExitCount { get; set; }

    [JsonPropertyName("daemon_last_start_ts")]
    public long DaemonLastStartTs { get; set; }

    [JsonPropertyName("daemon_last_exit_ts")]
    public long DaemonLastExitTs { get; set; }

    [JsonPropertyName("daemon_last_error_ts")]
    public long DaemonLastErrorTs { get; set; }

    [JsonPropertyName("daemon_last_error_stage")]
    public string DaemonLastErrorStage { get; set; } = string.Empty;

    [JsonPropertyName("daemon_last_error_message")]
    public string DaemonLastErrorMessage { get; set; } = string.Empty;

    [JsonPropertyName("poll_error_count")]
    public ulong PollErrorCount { get; set; }

    [JsonPropertyName("ipc_error_count")]
    public ulong IpcErrorCount { get; set; }
}

public sealed class CompactDatabaseResponse
{
    [JsonPropertyName("before_bytes")]
    public ulong BeforeBytes { get; set; }

    [JsonPropertyName("after_bytes")]
    public ulong AfterBytes { get; set; }

    [JsonPropertyName("reclaimed_bytes")]
    public ulong ReclaimedBytes { get; set; }

    [JsonPropertyName("duration_ms")]
    public ulong DurationMs { get; set; }
}

public sealed class SettingsResponse
{
    [JsonPropertyName("poll_interval_seconds")]
    public uint PollIntervalSeconds { get; set; }

    [JsonPropertyName("retention_days")]
    public uint RetentionDays { get; set; }

    [JsonPropertyName("afk_idle_threshold_seconds")]
    public uint AfkIdleThresholdSeconds { get; set; }

    [JsonPropertyName("onboarding_completed")]
    public bool OnboardingCompleted { get; set; }

    [JsonPropertyName("export_default_granularity")]
    public string ExportDefaultGranularity { get; set; } = "day";

    [JsonPropertyName("export_default_include_summary")]
    public bool ExportDefaultIncludeSummary { get; set; } = true;

    [JsonPropertyName("export_default_include_apps")]
    public bool ExportDefaultIncludeApps { get; set; } = true;

    [JsonPropertyName("export_default_include_interfaces")]
    public bool ExportDefaultIncludeInterfaces { get; set; } = true;
}

public sealed class UsageSummary
{
    [JsonPropertyName("buckets")]
    public UsageBucket[] Buckets { get; set; } = Array.Empty<UsageBucket>();

    [JsonPropertyName("total_sent")]
    public ulong TotalSent { get; set; }

    [JsonPropertyName("total_recv")]
    public ulong TotalRecv { get; set; }
}

public sealed class UsageBucket
{
    [JsonPropertyName("ts")]
    public long Ts { get; set; }

    [JsonPropertyName("bytes_sent")]
    public ulong BytesSent { get; set; }

    [JsonPropertyName("bytes_recv")]
    public ulong BytesRecv { get; set; }
}

public sealed class AppBreakdown
{
    [JsonPropertyName("apps")]
    public AppBreakdownRow[] Apps { get; set; } = Array.Empty<AppBreakdownRow>();
}

public sealed class AppBreakdownRow
{
    [JsonPropertyName("process_name")]
    public string ProcessName { get; set; } = string.Empty;

    [JsonPropertyName("display_name")]
    public string DisplayName { get; set; } = string.Empty;

    [JsonPropertyName("bytes_sent")]
    public ulong BytesSent { get; set; }

    [JsonPropertyName("bytes_recv")]
    public ulong BytesRecv { get; set; }

    [JsonPropertyName("last_seen_ts")]
    public long LastSeenTs { get; set; }
}

public sealed class InterfaceListResponse
{
    [JsonPropertyName("interfaces")]
    public InterfaceInfo[] Interfaces { get; set; } = Array.Empty<InterfaceInfo>();
}

public sealed class InterfaceInfo
{
    [JsonPropertyName("guid")]
    public string Guid { get; set; } = string.Empty;

    [JsonPropertyName("name")]
    public string Name { get; set; } = string.Empty;

    [JsonPropertyName("interface_type")]
    public string InterfaceType { get; set; } = string.Empty;

    [JsonPropertyName("is_metered")]
    public bool IsMetered { get; set; }
}

public sealed class InterfaceBreakdownResponse
{
    [JsonPropertyName("interfaces")]
    public InterfaceUsageRow[] Interfaces { get; set; } = Array.Empty<InterfaceUsageRow>();
}

public sealed class InterfaceUsageRow
{
    [JsonPropertyName("interface_id")]
    public string InterfaceId { get; set; } = string.Empty;

    [JsonPropertyName("interface_name")]
    public string InterfaceName { get; set; } = string.Empty;

    [JsonPropertyName("interface_type")]
    public string InterfaceType { get; set; } = string.Empty;

    [JsonPropertyName("is_metered")]
    public bool IsMetered { get; set; }

    [JsonPropertyName("bytes_sent")]
    public ulong BytesSent { get; set; }

    [JsonPropertyName("bytes_recv")]
    public ulong BytesRecv { get; set; }
}

public sealed class CapDefinitionsResponse
{
    [JsonPropertyName("caps")]
    public CapDefinition[] Caps { get; set; } = Array.Empty<CapDefinition>();
}

public sealed class CapDefinitionUpsertResponse
{
    [JsonPropertyName("cap")]
    public CapDefinition Cap { get; set; } = new();
}

public sealed class CapDefinitionDeleteResponse
{
    [JsonPropertyName("deleted")]
    public bool Deleted { get; set; }
}

public sealed class CapDefinition
{
    [JsonPropertyName("id")]
    public long Id { get; set; }

    [JsonPropertyName("scope")]
    public string Scope { get; set; } = string.Empty;

    [JsonPropertyName("interface_guid")]
    public string? InterfaceGuid { get; set; }

    [JsonPropertyName("monthly_cap_bytes")]
    public ulong MonthlyCapBytes { get; set; }

    [JsonPropertyName("is_active")]
    public bool IsActive { get; set; }

    [JsonPropertyName("created_at")]
    public long CreatedAt { get; set; }

    [JsonPropertyName("updated_at")]
    public long UpdatedAt { get; set; }
}

public sealed class CapAlertEventsResponse
{
    [JsonPropertyName("events")]
    public CapAlertEvent[] Events { get; set; } = Array.Empty<CapAlertEvent>();
}

public sealed class CapAlertEvent
{
    [JsonPropertyName("id")]
    public long Id { get; set; }

    [JsonPropertyName("cap_definition_id")]
    public long CapDefinitionId { get; set; }

    [JsonPropertyName("scope")]
    public string Scope { get; set; } = string.Empty;

    [JsonPropertyName("interface_guid")]
    public string? InterfaceGuid { get; set; }

    [JsonPropertyName("window_kind")]
    public string WindowKind { get; set; } = string.Empty;

    [JsonPropertyName("window_start_ts")]
    public long WindowStartTs { get; set; }

    [JsonPropertyName("window_end_ts")]
    public long WindowEndTs { get; set; }

    [JsonPropertyName("threshold_kind")]
    public string ThresholdKind { get; set; } = string.Empty;

    [JsonPropertyName("threshold_value")]
    public ulong ThresholdValue { get; set; }

    [JsonPropertyName("usage_bytes")]
    public ulong UsageBytes { get; set; }

    [JsonPropertyName("cap_bytes")]
    public ulong CapBytes { get; set; }

    [JsonPropertyName("fired_at")]
    public long FiredAt { get; set; }

    [JsonPropertyName("delivery_state")]
    public string DeliveryState { get; set; } = string.Empty;

    [JsonPropertyName("delivered_at")]
    public long? DeliveredAt { get; set; }
}

public sealed class MarkCapAlertEventsDeliveredResponse
{
    [JsonPropertyName("updated")]
    public uint Updated { get; set; }
}

public sealed class AfkAuditResponse
{
    [JsonPropertyName("afk_windows")]
    public AfkWindowUsage[] AfkWindows { get; set; } = Array.Empty<AfkWindowUsage>();
}

public sealed class AfkWindowUsage
{
    [JsonPropertyName("start_ts")]
    public long StartTs { get; set; }

    [JsonPropertyName("end_ts")]
    public long EndTs { get; set; }

    [JsonPropertyName("duration_seconds")]
    public uint DurationSeconds { get; set; }

    [JsonPropertyName("bytes_sent")]
    public ulong BytesSent { get; set; }

    [JsonPropertyName("bytes_recv")]
    public ulong BytesRecv { get; set; }

    [JsonPropertyName("top_apps")]
    public AppBreakdownRow[] TopApps { get; set; } = Array.Empty<AppBreakdownRow>();
}

public sealed class IpcEnvelope
{
    [JsonPropertyName("id")]
    public string? Id { get; set; }

    [JsonPropertyName("type")]
    public string Type { get; set; } = string.Empty;

    [JsonPropertyName("method")]
    public string Method { get; set; } = string.Empty;

    [JsonPropertyName("payload")]
    public JsonElement Payload { get; set; }

    [JsonPropertyName("error")]
    public IpcError? Error { get; set; }
}

public sealed class IpcError
{
    [JsonPropertyName("code")]
    public int Code { get; set; }

    [JsonPropertyName("message")]
    public string Message { get; set; } = string.Empty;
}
