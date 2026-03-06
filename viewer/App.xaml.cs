using System.Text.Json;
using Microsoft.UI.Xaml.Navigation;
using Microsoft.UI.Windowing;
using SingularityMonitor.Viewer.Services;
using WinRT.Interop;

namespace SingularityMonitor.Viewer
{
    /// <summary>
    /// Provides application-specific behavior to supplement the default Application class.
    /// </summary>
    public partial class App : Application
    {
        private static readonly string ReliabilityLogPath = ResolveReliabilityLogPath();
        private Window? window;
        private AppWindow? appWindow;
        private TrayIconController? trayIconController;
        private bool suppressCloseToTray;
        private bool isClosingHandlerAttached;

        /// <summary>
        /// Initializes the singleton application object.  This is the first line of authored code
        /// executed, and as such is the logical equivalent of main() or WinMain().
        /// </summary>
        public App()
        {
            this.InitializeComponent();
            AppDomain.CurrentDomain.ProcessExit += OnProcessExit;
            AppDomain.CurrentDomain.UnhandledException += OnCurrentDomainUnhandledException;
            TaskScheduler.UnobservedTaskException += OnUnobservedTaskException;
            UnhandledException += OnApplicationUnhandledException;
            AppendReliabilityEvent("start", "app_ctor", "Viewer process initialized.");
        }

        /// <summary>
        /// Invoked when the application is launched normally by the end user.  Other entry points
        /// will be used such as when the application is launched to open a specific file.
        /// </summary>
        /// <param name="e">Details about the launch request and process.</param>
        protected override void OnLaunched(LaunchActivatedEventArgs e)
        {
            window ??= new Window();
            EnsureAppWindowHooks();

            if (window.Content is not Frame rootFrame)
            {
                rootFrame = new Frame();
                rootFrame.NavigationFailed += OnNavigationFailed;
                window.Content = rootFrame;
            }

            _ = rootFrame.Navigate(typeof(MainPage), e.Arguments);
            ShowMainWindow();

            trayIconController ??= new TrayIconController(
                dispatcherQueue: window.DispatcherQueue,
                openDashboard: ActivateMainWindow,
                exitApplication: ExitApplication);
            trayIconController.Start();
            AppendReliabilityEvent("launch", "on_launched", "Viewer window launched.");
        }

        /// <summary>
        /// Invoked when Navigation to a certain page fails
        /// </summary>
        /// <param name="sender">The Frame which failed navigation</param>
        /// <param name="e">Details about the navigation failure</param>
        void OnNavigationFailed(object sender, NavigationFailedEventArgs e)
        {
            var pageName = e.SourcePageType.FullName ?? e.SourcePageType.Name;
            AppendReliabilityEvent("error", "navigation_failed", pageName);
            throw new Exception("Failed to load Page " + pageName);
        }

        private void ActivateMainWindow()
        {
            ShowMainWindow();
        }

        private void ExitApplication()
        {
            suppressCloseToTray = true;
            DisposeTrayIcon();
            window?.Close();
            Exit();
        }

        private void OnMainWindowClosing(AppWindow sender, AppWindowClosingEventArgs args)
        {
            if (suppressCloseToTray)
            {
                return;
            }

            args.Cancel = true;
            sender.Hide();
        }

        private void OnProcessExit(object? sender, EventArgs e)
        {
            AppendReliabilityEvent("clean_exit", "process_exit", "Viewer process exiting.");
            DisposeTrayIcon();
        }

        private void OnApplicationUnhandledException(object sender, Microsoft.UI.Xaml.UnhandledExceptionEventArgs e)
        {
            AppendReliabilityEvent("crash", "xaml_unhandled", e.Exception?.ToString() ?? e.Message);
        }

        private void OnCurrentDomainUnhandledException(object? sender, System.UnhandledExceptionEventArgs e)
        {
            AppendReliabilityEvent("crash", "appdomain_unhandled", e.ExceptionObject?.ToString() ?? "Unhandled exception");
        }

        private void OnUnobservedTaskException(object? sender, UnobservedTaskExceptionEventArgs e)
        {
            AppendReliabilityEvent("crash", "task_unobserved", e.Exception.ToString());
            e.SetObserved();
        }

        private void DisposeTrayIcon()
        {
            trayIconController?.Dispose();
            trayIconController = null;
        }

        private void EnsureAppWindowHooks()
        {
            if (window is null || isClosingHandlerAttached)
            {
                return;
            }

            var windowHandle = WindowNative.GetWindowHandle(window);
            var windowId = Microsoft.UI.Win32Interop.GetWindowIdFromWindow(windowHandle);
            appWindow = AppWindow.GetFromWindowId(windowId);
            appWindow.Closing += OnMainWindowClosing;
            isClosingHandlerAttached = true;
        }

        private void ShowMainWindow()
        {
            appWindow?.Show();
            window?.Activate();
        }

        private static void AppendReliabilityEvent(string kind, string stage, string message)
        {
            try
            {
                var directory = Path.GetDirectoryName(ReliabilityLogPath);
                if (!string.IsNullOrWhiteSpace(directory))
                {
                    Directory.CreateDirectory(directory);
                }

                var payload = new
                {
                    ts = DateTimeOffset.UtcNow.ToUnixTimeSeconds(),
                    kind,
                    stage,
                    message,
                };
                File.AppendAllText(
                    ReliabilityLogPath,
                    JsonSerializer.Serialize(payload) + Environment.NewLine);
            }
            catch
            {
                // Best effort only.
            }
        }

        private static string ResolveReliabilityLogPath()
        {
            var localAppData = Environment.GetFolderPath(Environment.SpecialFolder.LocalApplicationData);
            return Path.Combine(localAppData, "SingularityMonitor", "viewer-reliability.jsonl");
        }
    }
}
