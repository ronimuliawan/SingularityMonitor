using System;
using System.Drawing;
using System.Threading;
using System.Threading.Tasks;
using Microsoft.UI.Dispatching;
using Forms = System.Windows.Forms;

namespace SingularityMonitor.Viewer.Services;

public sealed class TrayIconController : IDisposable
{
    private const int NotifyIconTextLimit = 63;
    private static readonly TimeSpan TooltipRefreshInterval = TimeSpan.FromSeconds(60);

    private readonly DispatcherQueue dispatcherQueue;
    private readonly Action openDashboard;
    private readonly Action exitApplication;
    private readonly DaemonClient daemonClient = new();
    private readonly SemaphoreSlim refreshGate = new(1, 1);

    private readonly Forms.NotifyIcon notifyIcon;
    private readonly Forms.ContextMenuStrip contextMenu;

    private CancellationTokenSource? refreshLoopCts;
    private Task? refreshLoopTask;
    private bool disposed;

    public TrayIconController(DispatcherQueue dispatcherQueue, Action openDashboard, Action exitApplication)
    {
        this.dispatcherQueue = dispatcherQueue;
        this.openDashboard = openDashboard;
        this.exitApplication = exitApplication;

        contextMenu = new Forms.ContextMenuStrip();
        contextMenu.Items.Add("Open Dashboard", null, (_, _) => _ = dispatcherQueue.TryEnqueue(() => this.openDashboard()));
        contextMenu.Items.Add("Refresh Tooltip", null, (_, _) => _ = RefreshTooltipAsync(CancellationToken.None));
        contextMenu.Items.Add("Exit", null, (_, _) => _ = dispatcherQueue.TryEnqueue(() => this.exitApplication()));

        notifyIcon = new Forms.NotifyIcon
        {
            Icon = SystemIcons.Application,
            ContextMenuStrip = contextMenu,
            Text = ClampNotifyIconText("Starting..."),
            Visible = true,
        };

        notifyIcon.DoubleClick += (_, _) => _ = dispatcherQueue.TryEnqueue(() => this.openDashboard());
    }

    public void Start()
    {
        if (disposed || refreshLoopTask is not null)
        {
            return;
        }

        refreshLoopCts = new CancellationTokenSource();
        _ = RefreshTooltipAsync(refreshLoopCts.Token);
        refreshLoopTask = RunRefreshLoopAsync(refreshLoopCts.Token);
    }

    public void Dispose()
    {
        if (disposed)
        {
            return;
        }

        disposed = true;

        var loopTask = refreshLoopTask;
        refreshLoopTask = null;

        if (refreshLoopCts is not null)
        {
            refreshLoopCts.Cancel();
            refreshLoopCts.Dispose();
            refreshLoopCts = null;
        }

        if (loopTask is not null)
        {
            try
            {
                loopTask.GetAwaiter().GetResult();
            }
            catch (OperationCanceledException)
            {
                // Expected on shutdown.
            }
            catch
            {
                // Best effort during shutdown.
            }
        }

        try
        {
            notifyIcon.Visible = false;
            notifyIcon.Dispose();
            contextMenu.Dispose();
        }
        catch
        {
            // Ignore disposal errors during shutdown.
        }
    }

    private async Task RunRefreshLoopAsync(CancellationToken cancellationToken)
    {
        try
        {
            using var timer = new PeriodicTimer(TooltipRefreshInterval);
            while (await timer.WaitForNextTickAsync(cancellationToken))
            {
                await RefreshTooltipAsync(cancellationToken);
            }
        }
        catch (OperationCanceledException)
        {
            // Expected on shutdown.
        }
    }

    private async Task RefreshTooltipAsync(CancellationToken cancellationToken)
    {
        if (disposed)
        {
            return;
        }

        if (!await refreshGate.WaitAsync(0, cancellationToken))
        {
            return;
        }

        try
        {
            string tooltip;
            try
            {
                var summary = await daemonClient.GetTodaySummaryAsync(cancellationToken);
                tooltip = BuildSummaryTooltip(summary);
            }
            catch
            {
                tooltip = "Today usage unavailable (daemon offline)";
            }

            var safeText = ClampNotifyIconText(tooltip);
            _ = dispatcherQueue.TryEnqueue(() =>
            {
                if (!disposed)
                {
                    notifyIcon.Text = safeText;
                }
            });
        }
        finally
        {
            refreshGate.Release();
        }
    }

    private static string BuildSummaryTooltip(UsageSummary summary)
    {
        var sent = FormatBytesCompact(summary.TotalSent);
        var recv = FormatBytesCompact(summary.TotalRecv);
        var total = FormatBytesCompact(summary.TotalSent + summary.TotalRecv);
        return $"Today U:{sent} D:{recv} T:{total}";
    }

    private static string ClampNotifyIconText(string value)
    {
        var text = value.Replace('\r', ' ').Replace('\n', ' ').Trim();
        if (text.Length <= NotifyIconTextLimit)
        {
            return text;
        }

        return text[..NotifyIconTextLimit];
    }

    private static string FormatBytesCompact(ulong bytes)
    {
        const double kb = 1024d;
        const double mb = 1024d * kb;
        const double gb = 1024d * mb;

        if (bytes >= gb)
        {
            return $"{bytes / gb:F1}GB";
        }

        if (bytes >= mb)
        {
            return $"{bytes / mb:F1}MB";
        }

        if (bytes >= kb)
        {
            return $"{bytes / kb:F1}KB";
        }

        return $"{bytes}B";
    }
}
