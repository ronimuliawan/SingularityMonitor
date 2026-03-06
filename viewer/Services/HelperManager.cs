using System;
using System.Diagnostics;
using System.IO;
using System.Linq;
using System.Runtime.InteropServices;
using System.Text;
using System.Threading.Tasks;
using Microsoft.Win32;

namespace SingularityMonitor.Viewer.Services;

public sealed class HelperManager
{
    private const string RunKeyPath = @"Software\Microsoft\Windows\CurrentVersion\Run";
    private const string RunValueName = "SingularityMonitorHelper";
    private const int AppModelErrorNoPackage = 15700;

    [DllImport("kernel32.dll", CharSet = CharSet.Unicode, SetLastError = true)]
    private static extern int GetCurrentPackageFullName(ref int packageFullNameLength, StringBuilder? packageFullName);

    public string? ResolveHelperPath()
    {
        var fromEnv = Environment.GetEnvironmentVariable("SM_HELPER_PATH");
        if (!string.IsNullOrWhiteSpace(fromEnv) && File.Exists(fromEnv))
        {
            return fromEnv;
        }

        var local = Path.Combine(AppContext.BaseDirectory, "helper.exe");
        if (IsRunningWithPackageIdentity())
        {
            return File.Exists(local) ? local : null;
        }

        var baseDir = new DirectoryInfo(AppContext.BaseDirectory);
        var probe = baseDir;
        for (var depth = 0; depth < 8 && probe is not null; depth++)
        {
            var release = Path.Combine(probe.FullName, "target", "release", "helper.exe");
            if (File.Exists(release))
            {
                return release;
            }

            var debug = Path.Combine(probe.FullName, "target", "debug", "helper.exe");
            if (File.Exists(debug))
            {
                return debug;
            }

            probe = probe.Parent;
        }

        return File.Exists(local) ? local : null;
    }

    public string EnsureLoopRunning()
    {
        try
        {
            if (Process.GetProcessesByName("helper").Any())
            {
                return "Helper loop already running.";
            }

            var helperPath = ResolveHelperPath();
            if (helperPath is null)
            {
                return IsRunningWithPackageIdentity()
                    ? "helper.exe is not bundled with the packaged viewer. Set SM_HELPER_PATH to an external helper build."
                    : "helper.exe not found. Set SM_HELPER_PATH or build helper first.";
            }

            var process = Process.Start(new ProcessStartInfo
            {
                FileName = helperPath,
                Arguments = "--loop --interval-secs 60 --window-secs 300",
                UseShellExecute = false,
                CreateNoWindow = true,
            });

            return process is null
                ? "Failed to start helper loop process."
                : "Helper loop started.";
        }
        catch (Exception ex)
        {
            return $"Failed to start helper loop: {ex.Message}";
        }
    }

    public string EnsureRunAtLogin()
    {
        if (IsRunningWithPackageIdentity())
        {
            return "Helper startup registration is disabled in packaged viewer builds.";
        }

        try
        {
            var helperPath = ResolveHelperPath();
            if (helperPath is null)
            {
                return "helper.exe not found for startup registration.";
            }

            var command = Quote(helperPath) + " --loop --interval-secs 60 --window-secs 300";
            using var key = Registry.CurrentUser.CreateSubKey(RunKeyPath, true);
            if (key is null)
            {
                return "Unable to open HKCU startup registry key.";
            }

            var existing = key.GetValue(RunValueName) as string;
            if (string.Equals(existing, command, StringComparison.OrdinalIgnoreCase))
            {
                return "Helper startup registration already set.";
            }

            key.SetValue(RunValueName, command, RegistryValueKind.String);
            return "Helper startup registration updated for current user.";
        }
        catch (Exception ex)
        {
            return $"Unable to update helper startup registration: {ex.Message}";
        }
    }

    public async Task<string> RunHistoryImportAsync(int days, int chunkHours)
    {
        try
        {
            var helperPath = ResolveHelperPath();
            if (helperPath is null)
            {
                return IsRunningWithPackageIdentity()
                    ? "helper.exe is not bundled with the packaged viewer. Set SM_HELPER_PATH before running imports."
                    : "helper.exe not found. Set SM_HELPER_PATH or build helper first.";
            }

            var psi = new ProcessStartInfo
            {
                FileName = helperPath,
                Arguments = $"--import-history --days {Math.Max(1, days)} --chunk-hours {Math.Max(1, chunkHours)}",
                UseShellExecute = false,
                CreateNoWindow = true,
                RedirectStandardOutput = true,
                RedirectStandardError = true,
                StandardOutputEncoding = Encoding.UTF8,
                StandardErrorEncoding = Encoding.UTF8,
            };

            using var process = Process.Start(psi);
            if (process is null)
            {
                return "Failed to start helper import process.";
            }

            var outputTask = process.StandardOutput.ReadToEndAsync();
            var errorTask = process.StandardError.ReadToEndAsync();
            await process.WaitForExitAsync();

            var output = await outputTask;
            var error = await errorTask;
            if (process.ExitCode == 0)
            {
                return string.IsNullOrWhiteSpace(output)
                    ? "History import finished."
                    : output.Trim();
            }

            return string.IsNullOrWhiteSpace(error)
                ? $"History import failed with exit code {process.ExitCode}."
                : error.Trim();
        }
        catch (Exception ex)
        {
            return $"Failed to run history import: {ex.Message}";
        }
    }

    private static string Quote(string path) => "\"" + path + "\"";

    private static bool IsRunningWithPackageIdentity()
    {
        var length = 0;
        var result = GetCurrentPackageFullName(ref length, null);
        return result != AppModelErrorNoPackage;
    }
}
