using System;
using System.Collections.Generic;
using System.Linq;
using System.Reflection;
using Aoko.Core;

namespace Aoko.Tests;

/// <summary>
/// Covers the command search ranking contract and guards the duplication that makes the overlay work:
/// every GUI module needs a search row, a tab, a card element, and the toggle property its card binds.
/// MainWindow.xaml is the source of truth for those strings.
/// </summary>
public class ModuleSearchIndexTests
{
    private static readonly string MainWindowXaml = TestRepo.Read("Aoko", "MainWindow.xaml");
    private static readonly ModuleSearchIndex Index = ModuleSearchIndex.Default;

    [Fact]
    public void Rank_EmptyQuery_ReturnsEveryEntryInCatalogOrder()
    {
        IReadOnlyList<ModuleSearchHit> hits = Index.Rank("   ");

        Assert.Equal(Index.Entries.Count, hits.Count);
        Assert.Equal(
            Index.Entries.OrderBy(e => e.Order).Select(e => e.Id),
            hits.Select(h => h.Entry.Id));
    }

    [Fact]
    public void Rank_NoResults_ReturnsEmpty()
    {
        Assert.Empty(Index.Rank("zzzzz"));
    }

    [Fact]
    public void Rank_PrefixBeatsWordPrefixBeatsSubstring()
    {
        // "esp" is a word prefix of both ESP modules and nothing else outranks them.
        IReadOnlyList<ModuleSearchHit> hits = Index.Rank("esp");

        Assert.Equal("chestesp", hits[0].Entry.Id);
        Assert.Equal("blockesp", hits[1].Entry.Id);
        Assert.Equal(ModuleSearchIndex.RankWordPrefix, hits[0].Rank);
        Assert.Equal(ModuleSearchIndex.RankWordPrefix, hits[1].Rank);

        // "tnttimer" is an alias prefix; "tnt" alone is still a display-name prefix.
        Assert.Equal("tnttimer", Index.Rank("tnt")[0].Entry.Id);
        Assert.Equal(ModuleSearchIndex.RankExactPrefix, Index.Rank("tnt")[0].Rank);
    }

    [Fact]
    public void Rank_WordPrefixBeatsSubsequence()
    {
        IReadOnlyList<ModuleSearchHit> aura = Index.Rank("aura");

        Assert.Equal("killaura", aura[0].Entry.Id);
        Assert.Equal(ModuleSearchIndex.RankWordPrefix, aura[0].Rank);
        Assert.All(aura, hit => Assert.True(hit.Rank >= ModuleSearchIndex.RankWordPrefix));
    }

    [Fact]
    public void Rank_MatchesModuleIdViaAliases()
    {
        IReadOnlyList<ModuleSearchHit> hits = Index.Rank("ka");

        Assert.Equal("killaura", hits[0].Entry.Id);
        Assert.Equal(ModuleSearchIndex.RankAliasPrefix, hits[0].Rank);
        Assert.Empty(hits[0].NameRanges);
        Assert.Equal("ka", hits[0].MatchedAlias);
    }

    [Fact]
    public void Rank_ExplainsCompactNameMatches()
    {
        ModuleSearchHit hit = Index.Rank("hitselect")[0];

        Assert.Equal("hitselect", hit.Entry.Id);
        Assert.Equal(ModuleSearchIndex.RankCompactSubstring, hit.Rank);
        Assert.Empty(hit.NameRanges);
        Assert.Equal("hitselect", hit.MatchedAlias);
    }

    [Fact]
    public void Rank_CarriesTheAliasSoTheHintCanExplainTheMatch()
    {
        ModuleSearchHit hit = Index.Rows("cps", _ => true)[0];

        Assert.Equal("autoclicker", hit.Entry.Id);
        Assert.Contains("cps", hit.Entry.Synonyms);
        Assert.Equal("cps", hit.MatchedAlias);
        Assert.Empty(hit.NameRanges);

        // A display-name match explains itself through the highlighted ranges.
        Assert.Null(Index.Rank("esp").First(h => h.Entry.Id == "chestesp").MatchedAlias);
    }

    /// <summary>
    /// The overlay renders the ranked list as is: the first visible row is the top-ranked hit, so
    /// the first row, the highlighted row and the Enter target cannot disagree. Exact name-prefix
    /// ties ("hit" also prefixes Hit Select) keep catalogue order, which is what Rank pins.
    /// </summary>
    [Theory]
    [InlineData("hit", false, "hitdelayfix")]
    [InlineData("ka", true, "killaura")]
    [InlineData("esp", false, "chestesp")]
    [InlineData("auto", false, "autoclicker")]
    public void Rows_FirstVisibleRowIsTheTopRankedHit(string query, bool devMode, string expectedId)
    {
        IReadOnlyList<ModuleSearchHit> hits = Index.Rank(query);
        IReadOnlyList<ModuleSearchHit> rows = Index.Rows(query, entry => !entry.DevOnly || devMode);

        Assert.NotEmpty(rows);
        Assert.Equal(expectedId, rows[0].Entry.Id);
        Assert.Same(hits[0].Entry, rows[0].Entry);
        Assert.Equal(
            hits.Where(hit => !hit.Entry.DevOnly || devMode).Select(hit => hit.Entry.Id),
            rows.Select(hit => hit.Entry.Id));
    }

    [Fact]
    public void Rows_DropHiddenEntriesWithoutReorderingTheRest()
    {
        IReadOnlyList<ModuleSearchHit> hits = Index.Rank("ka");
        Assert.Equal("killaura", hits[0].Entry.Id);
        Assert.True(hits[0].Entry.DevOnly);

        IReadOnlyList<ModuleSearchHit> rows = Index.Rows("ka", entry => !entry.DevOnly);

        Assert.DoesNotContain(rows, hit => hit.Entry.Id == "killaura");
        Assert.Equal(hits.Skip(1).Select(hit => hit.Entry.Id), rows.Select(hit => hit.Entry.Id));
    }

    [Fact]
    public void Rows_AreSortedForEveryQuerySoTheSelectionTargetStaysFirst()
    {
        foreach (string query in new[] { "a", "s", "hit", "ka", "esp", "auto", "e", "r" })
        {
            IReadOnlyList<ModuleSearchHit> rows = Index.Rows(query, _ => true);

            for (int i = 1; i < rows.Count; i++)
            {
                Assert.True(
                    rows[i - 1].Rank < rows[i].Rank
                    || (rows[i - 1].Rank == rows[i].Rank && rows[i - 1].Entry.Order < rows[i].Entry.Order),
                    $"Rows for '{query}' are not in rank order at index {i}");
            }
        }
    }

    [Fact]
    public void Rank_MatchesCuratedSynonyms()
    {
        Assert.Equal("autoclicker", Index.Rank("cps")[0].Entry.Id);
        Assert.Equal("autoclicker", Index.Rank("clicker")[0].Entry.Id);
        Assert.Equal("pickaxe", Index.Rank("pickaxe")[0].Entry.Synonyms[^1]);
    }

    [Fact]
    public void Rank_MatchesTabNames()
    {
        IReadOnlyList<ModuleSearchHit> hits = Index.Rank("render");

        Assert.NotEmpty(hits);
        Assert.All(hits, hit => Assert.Equal("Render", hit.Entry.Tab));
    }

    [Fact]
    public void Rank_SubsequenceMatchesScatteredCharacters()
    {
        ModuleSearchHit? hit = Index.Rank("ntgs").FirstOrDefault(h => h.Entry.Id == "nametags");

        Assert.NotNull(hit);
        Assert.Equal(ModuleSearchIndex.RankSubsequence, hit!.Rank);
        Assert.Equal(
            new[] { new ModuleSearchRange(0, 1), new ModuleSearchRange(4, 1), new ModuleSearchRange(6, 2) },
            hit.NameRanges);
    }

    [Fact]
    public void Rank_ReportsMatchRangesForDisplayName()
    {
        ModuleSearchHit hit = Index.Rank("esp").First(h => h.Entry.Id == "chestesp");

        Assert.Equal(new[] { new ModuleSearchRange(6, 3) }, hit.NameRanges);
    }

    [Fact]
    public void Rank_IsDeterministicAndSorted()
    {
        IReadOnlyList<ModuleSearchHit> first = Index.Rank("s");
        IReadOnlyList<ModuleSearchHit> second = Index.Rank("s");

        Assert.Equal(first.Select(h => h.Entry.Id), second.Select(h => h.Entry.Id));

        for (int i = 1; i < first.Count; i++)
        {
            Assert.True(
                first[i - 1].Rank < first[i].Rank
                || (first[i - 1].Rank == first[i].Rank && first[i - 1].Entry.Order < first[i].Entry.Order),
                $"Ranking is not sorted at index {i}");
        }
    }

    [Fact]
    public void Entries_HaveUniqueIdsAndKnownGroups()
    {
        var duplicates = Index.Entries
            .GroupBy(e => e.Id, StringComparer.OrdinalIgnoreCase)
            .Where(g => g.Count() > 1)
            .Select(g => g.Key)
            .ToList();
        Assert.Empty(duplicates);

        Assert.All(Index.Entries, entry => Assert.Contains(entry.Group, ModuleSearchIndex.GroupOrder));
        Assert.All(
            Index.Entries.Where(e => !e.IsDestination),
            entry => Assert.Equal(entry.Tab, entry.Group));
    }

    [Fact]
    public void EveryGuiModule_IsSearchableAndAnchoredToMainWindowXaml()
    {
        string xaml = MainWindowXaml;
        var rows = Index.Entries.ToDictionary(e => e.Id, StringComparer.OrdinalIgnoreCase);

        // HUD Editor is a GUI toggle in Render without a keybind button, so it is not KeybindGui.
        var guiModuleIds = ModuleCatalog.Requiring(ModuleCatalog.Surfaces.KeybindGui)
            .Select(e => e.Id)
            .ToHashSet(StringComparer.OrdinalIgnoreCase);
        guiModuleIds.Add("hudeditor");

        Clicker clicker = Clicker.Instance;
        bool savedDevMode = clicker.DevMode;
        try
        {
            foreach (ModuleCatalog.Entry entry in ModuleCatalog.All.Where(e => guiModuleIds.Contains(e.Id)))
            {
                clicker.DevMode = entry.DevOnly;
                Assert.True(ModuleCatalog.IsGuiVisible(entry, clicker),
                    $"'{entry.Id}' should be visible with DevMode={entry.DevOnly}");

                Assert.True(rows.TryGetValue(entry.Id, out ModuleSearchEntry? row),
                    $"ModuleCatalog entry '{entry.Id}' has no command search row. Add it to ModuleSearchIndex.GuiRows.");
                Assert.Equal(entry.DisplayName, row!.DisplayName);
                Assert.False(string.IsNullOrWhiteSpace(row.Tab));
                Assert.Contains($"Header=\"{row.Tab}\"", xaml, StringComparison.Ordinal);
                Assert.Contains($"x:Name=\"{row.CardName}\"", xaml, StringComparison.Ordinal);

                if (row.AvailabilityLabel.Length > 0)
                    Assert.Contains($"x:Name=\"{row.AvailabilityLabel}\"", xaml, StringComparison.Ordinal);

                if (entry.Id == "panic")
                {
                    Assert.Equal(string.Empty, row.ToggleProperty);
                    continue;
                }

                Assert.False(string.IsNullOrWhiteSpace(row.ToggleProperty),
                    $"'{entry.Id}' has no toggle property, so its row cannot show a switch.");
                Assert.Contains($"Binding {row.ToggleProperty},", xaml, StringComparison.Ordinal);

                PropertyInfo? property = typeof(Clicker).GetProperty(row.ToggleProperty);
                Assert.NotNull(property);
                Assert.Equal(typeof(bool), property!.PropertyType);
            }
        }
        finally
        {
            clicker.DevMode = savedDevMode;
        }
    }

    [Fact]
    public void Destinations_PointAtExistingTabs()
    {
        string xaml = MainWindowXaml;
        var destinations = Index.Entries.Where(e => e.IsDestination).ToList();

        Assert.NotEmpty(destinations);
        Assert.All(destinations, destination =>
        {
            Assert.Equal("Tabs", destination.Group);
            Assert.Contains($"Header=\"{destination.Tab}\"", xaml, StringComparison.Ordinal);
            Assert.Equal(string.Empty, destination.ToggleProperty);
        });
    }

}
