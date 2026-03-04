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
        CancellationToken cancellationToken = default)
    {
        var envelope = await SendRequestAsync(
            "SET_SETTINGS",
            payload: new
            {
                poll_interval_seconds = pollIntervalSeconds,
                retention_days = retentionDays,
                afk_idle_threshold_seconds = afkIdleThresholdSeconds,
            },
            cancellationToken);

        _ = ParsePayload<JsonElement>(envelope);
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
        CancellationToken cancellationToken = default)
    {
        var normalizedLimit = Math.Max(1, limit);

        var envelope = await SendRequestAsync(
            "GET_APP_BREAKDOWN",
            payload: new
            {
                start_ts = startTs,
                end_ts = endTs,
                interface_id = interfaceId,
                interface_type = interfaceType,
                limit = normalizedLimit,
                sort_by = "total_bytes_desc",
            },
            cancellationToken);

        return ParsePayload<AppBreakdown>(envelope);
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
}

public sealed class SettingsResponse
{
    [JsonPropertyName("poll_interval_seconds")]
    public uint PollIntervalSeconds { get; set; }

    [JsonPropertyName("retention_days")]
    public uint RetentionDays { get; set; }

    [JsonPropertyName("afk_idle_threshold_seconds")]
    public uint AfkIdleThresholdSeconds { get; set; }
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
