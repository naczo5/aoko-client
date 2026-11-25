using System.Runtime.CompilerServices;

namespace Aoko.Tests;

/// <summary>
/// Locates the repository root for tests that assert on source files. Works in the
/// repo and when a test file is compiled from an external harness by absolute path
/// (<see cref="CallerFilePathAttribute"/> keeps the original path).
/// </summary>
internal static class TestRepo
{
    private static readonly Lazy<string> RootValue = new(Locate);

    internal static string Root => RootValue.Value;

    internal static string Read(params string[] relativeParts)
        => File.ReadAllText(Path.Combine(new[] { Root }.Concat(relativeParts).ToArray()));

    private static string Locate()
    {
        string fromTestsDirectory = Path.GetFullPath(
            Path.Combine(Path.GetDirectoryName(ThisFile())!, ".."));

        foreach (string candidate in Candidates(fromTestsDirectory))
        {
            if (File.Exists(Path.Combine(candidate, "Aoko", "Core", "GameStateClient.cs")))
                return candidate;
        }

        throw new DirectoryNotFoundException("Could not locate Aoko/Core/GameStateClient.cs.");
    }

    private static IEnumerable<string> Candidates(string first)
    {
        yield return first;

        DirectoryInfo? directory = new(AppContext.BaseDirectory);
        while (directory != null)
        {
            yield return directory.FullName;
            directory = directory.Parent;
        }
    }

    private static string ThisFile([CallerFilePath] string path = "") => path;
}
