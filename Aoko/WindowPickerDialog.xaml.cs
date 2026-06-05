using System.Windows;
using System.Windows.Controls;
using Aoko.Core;

namespace Aoko;

public partial class WindowPickerDialog : Window
{
    public WindowTarget? SelectedTarget { get; private set; }
    public string SelectedVersion { get; private set; } = "auto";

    public WindowPickerDialog()
    {
        InitializeComponent();
        RefreshWindowList();
    }

    private void RefreshWindowList()
    {
        var windows = WindowDetection.ListSelectableWindows();
        WindowList.ItemsSource = windows;
        if (windows.Count > 0)
            WindowList.SelectedIndex = 0;
    }

    private void RefreshButton_Click(object sender, RoutedEventArgs e) => RefreshWindowList();

    private void CancelButton_Click(object sender, RoutedEventArgs e)
    {
        DialogResult = false;
        Close();
    }

    private void InjectButton_Click(object sender, RoutedEventArgs e)
    {
        if (WindowList.SelectedItem is not WindowTarget target)
        {
            MessageBox.Show(this,
                "Select a window from the list.",
                "No window selected",
                MessageBoxButton.OK,
                MessageBoxImage.Warning);
            return;
        }

        if (!target.IsJvm)
        {
            MessageBoxResult confirm = MessageBox.Show(this,
                "The selected process is not java/javaw. Injection will likely fail unless this is a Minecraft Java client.",
                "Non-JVM target",
                MessageBoxButton.OKCancel,
                MessageBoxImage.Warning);
            if (confirm != MessageBoxResult.OK)
                return;
        }

        SelectedTarget = target;
        SelectedVersion = ResolveSelectedVersion();
        DialogResult = true;
        Close();
    }

    private string ResolveSelectedVersion()
    {
        if (VersionCombo.SelectedItem is ComboBoxItem item && item.Tag is string tag)
            return tag;

        return "auto";
    }
}
