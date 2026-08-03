using System.Collections.Generic;
using System.Windows;
using System.Windows.Controls;
using Aoko.Core;

namespace Aoko;

public partial class WindowPickerDialog : Window
{
    public InjectionTarget? SelectedTarget { get; private set; }
    public string SelectedVersion { get; private set; } = "auto";

    public WindowPickerDialog(IReadOnlyList<InjectionTarget>? initialTargets = null)
    {
        InitializeComponent();
        RefreshWindowList(initialTargets);
    }

    private void RefreshWindowList(IReadOnlyList<InjectionTarget>? targets = null)
    {
        var candidates = targets ?? InjectionTargetDiscovery.ListTargets();
        WindowList.ItemsSource = candidates;
        if (candidates.Count > 0)
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
        if (WindowList.SelectedItem is not InjectionTarget target)
        {
            MessageBox.Show(this,
                "Select a Java process from the list.",
                "No process selected",
                MessageBoxButton.OK,
                MessageBoxImage.Warning);
            return;
        }

        if (!target.IsLikelyMinecraft)
        {
            MessageBoxResult confirm = MessageBox.Show(this,
                "A Minecraft/client marker was not found for this Java process. Injection may fail if it is not a Minecraft client.",
                "Unidentified Java process",
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
