using System;
using System.Collections.Generic;
using System.Linq;
using System.Text.RegularExpressions;
using Aoko.Views;

namespace Aoko.Tests;

/// <summary>
/// The XAML compiler resolves handlers but not resource keys or binding paths; both
/// fail at runtime. These checks cover the keys and properties the two tabs use.
/// </summary>
public class XamlBindingTests
{
    private static readonly string Accounts = TestRepo.Read("Aoko", "Views", "AccountsView.xaml");
    private static readonly string AccountsCode = TestRepo.Read("Aoko", "Views", "AccountsView.xaml.cs");
    private static readonly string Servers = TestRepo.Read("Aoko", "Views", "ServersView.xaml");
    private static readonly string Styles = TestRepo.Read("Aoko", "Views", "TabStyles.xaml");
    private static readonly string App = TestRepo.Read("Aoko", "App.xaml");

    [Fact]
    public void EveryStaticResourceKeyIsDefined()
    {
        HashSet<string> defined = DefinedKeys();
        var missing = new SortedSet<string>(StringComparer.Ordinal);

        foreach (string xaml in new[] { Accounts, Servers, Styles })
        {
            foreach (Match use in Regex.Matches(xaml, @"\{StaticResource ([A-Za-z0-9_.]+)\}"))
            {
                if (!defined.Contains(use.Groups[1].Value))
                    missing.Add(use.Groups[1].Value);
            }
        }

        Assert.True(missing.Count == 0, "Undefined StaticResource keys: " + string.Join(", ", missing));
    }

    [Fact]
    public void EveryDynamicResourceKeyIsDefined()
    {
        HashSet<string> defined = DefinedKeys();
        var missing = new SortedSet<string>(StringComparer.Ordinal);

        foreach (string xaml in new[] { Accounts, Servers, Styles })
        {
            foreach (Match use in Regex.Matches(xaml, @"\{DynamicResource ([A-Za-z0-9_.]+)\}"))
            {
                if (!defined.Contains(use.Groups[1].Value))
                    missing.Add(use.Groups[1].Value);
            }
        }

        Assert.True(missing.Count == 0, "Undefined DynamicResource keys: " + string.Join(", ", missing));
    }

    [Fact]
    public void ServerRowBindingsMatchSavedServerRow()
    {
        HashSet<string> properties = typeof(SavedServerRow)
            .GetProperties()
            .Select(property => property.Name)
            .ToHashSet(StringComparer.Ordinal);

        var missing = new SortedSet<string>(StringComparer.Ordinal);
        foreach (string path in BindingPaths(Servers))
        {
            if (!properties.Contains(path))
                missing.Add(path);
        }

        Assert.True(missing.Count == 0, "Server bindings with no property: " + string.Join(", ", missing));
    }

    [Fact]
    public void AccountCardBindingsMatchTheCardModel()
    {
        // The card model lives inside the WPF-dependent control, so its property
        // names come from the source rather than a reflection call.
        HashSet<string> properties = Regex
            .Matches(AccountsCode, @"public (?:string|bool) ([A-Za-z0-9_]+)(?:\s*\{ get; \}| =>)")
            .Select(match => match.Groups[1].Value)
            .ToHashSet(StringComparer.Ordinal);

        Assert.Contains("AutomationName", properties);
        Assert.Contains("UuidTooltip", properties);

        var missing = new SortedSet<string>(StringComparer.Ordinal);
        foreach (string path in BindingPaths(Accounts))
        {
            if (!properties.Contains(path))
                missing.Add(path);
        }

        Assert.True(missing.Count == 0, "Card bindings with no property: " + string.Join(", ", missing));
    }

    private static IEnumerable<string> BindingPaths(string xaml)
        => Regex.Matches(xaml, @"\{Binding ([A-Za-z0-9_]+)\}")
            .Select(match => match.Groups[1].Value)
            .Distinct(StringComparer.Ordinal);

    private static HashSet<string> DefinedKeys()
        => new[] { Accounts, Servers, Styles, App }
            .SelectMany(xaml => Regex.Matches(xaml, @"x:Key=""([^""]+)""")
                .Select(match => match.Groups[1].Value))
            .ToHashSet(StringComparer.Ordinal);
}
