using System;
using System.Diagnostics;
using System.Threading.Tasks;
using System.Windows;
using System.Windows.Interop;
using System.Windows.Media;
using Aoko.Core;
using Aoko.Core.Visuals;

namespace Aoko;

public partial class App : Application
{
    /// <summary>Background extraction of the built-in visuals; the window waits on it before its first scan.</summary>
    internal static Task<VisualsInstallResult>? VisualsInstall { get; private set; }

    protected override void OnStartup(StartupEventArgs e)
    {
        // Hardware rendering is the default. Force software only as an explicit escape
        // hatch (broken GPU / RDP capture issues) since it CPU-rasterizes the whole window.
        if (Environment.GetEnvironmentVariable("AOKO_SOFTWARE_RENDERING") == "1")
            RenderOptions.ProcessRenderMode = RenderMode.SoftwareOnly;

        VisualsInstall = Task.Run(() =>
        {
            VisualsInstallResult result = VisualsInstaller.InstallEmbedded();
            foreach (string error in result.Errors)
                Debug.WriteLine($"[Visuals] {error}");
            return result;
        });

        base.OnStartup(e);

        // Install global hooks
        InputHooks.Install();
    }

    protected override void OnExit(ExitEventArgs e)
    {
        DiscordRichPresenceService.Instance.Stop();
        // Cleanup hooks
        InputHooks.Uninstall();
        Clicker.Instance.Stop();

        base.OnExit(e);
    }
}
