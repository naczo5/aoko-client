using System;
using System.Windows;
using System.Windows.Interop;
using System.Windows.Media;
using Aoko.Core;

namespace Aoko;

public partial class App : Application
{
    protected override void OnStartup(StartupEventArgs e)
    {
        // Hardware rendering is the default. Force software only as an explicit escape
        // hatch (broken GPU / RDP capture issues) since it CPU-rasterizes the whole window.
        if (Environment.GetEnvironmentVariable("AOKO_SOFTWARE_RENDERING") == "1")
            RenderOptions.ProcessRenderMode = RenderMode.SoftwareOnly;

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
