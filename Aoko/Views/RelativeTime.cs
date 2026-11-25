using System;

namespace Aoko.Views;

/// <summary>
/// Compact relative timestamps for account and server cards ("just now", "3d ago").
/// One implementation, so the two tabs cannot drift; the sentence around it is the
/// view's job ("Last used 3d ago" / "Last joined 3d ago").
/// </summary>
public static class RelativeTime
{
    public static string Format(DateTimeOffset value)
    {
        TimeSpan elapsed = DateTimeOffset.UtcNow - value;
        if (elapsed < TimeSpan.Zero || elapsed.TotalMinutes < 1)
            return "just now";
        if (elapsed.TotalHours < 1)
            return $"{(int)elapsed.TotalMinutes}m ago";
        if (elapsed.TotalDays < 1)
            return $"{(int)elapsed.TotalHours}h ago";
        if (elapsed.TotalDays < 30)
            return $"{(int)elapsed.TotalDays}d ago";
        if (elapsed.TotalDays < 365)
            return $"{(int)(elapsed.TotalDays / 30)}mo ago";
        return $"{(int)(elapsed.TotalDays / 365)}y ago";
    }
}
