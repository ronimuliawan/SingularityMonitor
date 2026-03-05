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
        }

        /// <summary>
        /// Invoked when Navigation to a certain page fails
        /// </summary>
        /// <param name="sender">The Frame which failed navigation</param>
        /// <param name="e">Details about the navigation failure</param>
        void OnNavigationFailed(object sender, NavigationFailedEventArgs e)
        {
            throw new Exception("Failed to load Page " + e.SourcePageType.FullName);
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
            DisposeTrayIcon();
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
    }
}
