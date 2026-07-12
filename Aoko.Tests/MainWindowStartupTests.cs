using System.Threading;
using System.Windows;
using Aoko.Core;

namespace Aoko.Tests;

public class MainWindowStartupTests
{
    [Fact]
    public void MainWindow_CanInitializeAndClose()
    {
        Exception? failure = null;

        var thread = new Thread(() =>
        {
            try
            {
                if (Application.Current == null)
                {
                    var app = new App();
                    app.InitializeComponent();
                }

                var window = new MainWindow();
                window.Close();
            }
            catch (Exception ex)
            {
                failure = ex;
            }
            finally
            {
                DiscordRichPresenceService.Instance.Stop();
                StatsTracker.Instance.StopSessionTimer();
            }
        });

        thread.SetApartmentState(ApartmentState.STA);
        thread.Start();
        thread.Join();

        Assert.Null(failure);
    }
}
