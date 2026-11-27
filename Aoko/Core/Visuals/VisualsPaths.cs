using System;
using System.IO;

namespace Aoko.Core.Visuals;

/// <summary>Where the visuals directory lives. Tests point <see cref="RootOverride"/> at a temp folder.</summary>
public static class VisualsPaths
{
    public const string CapesFolder = "capes";
    public const string SkiesFolder = "skies";
    public const string CosmeticsFolder = "cosmetics";

    /// <summary>When set, <see cref="Root"/> returns this path instead of the AppData location.</summary>
    internal static string? RootOverride { get; set; }

    /// <summary>Absolute path of the visuals directory, <c>%AppData%\Aoko\visuals</c> by default.</summary>
    public static string Root => RootOverride ?? ResolveRoot(
        Environment.GetFolderPath(Environment.SpecialFolder.ApplicationData));

    internal static string ResolveRoot(string appData)
        => Path.Combine(appData, "Aoko", "visuals");
}
